//! Apply OCI changesets to a RAM filesystem and retain incremental before/after records.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use super::{LoadProgress, progress};

use super::model::{
    Analysis, Change, ChangeKind, FileEntry, FileKind, Filesystem, Image, LayerAnalysis,
};

type PreviousEntries = BTreeMap<String, Option<Arc<FileEntry>>>;

pub fn analyze(image: Image) -> Result<Analysis> {
    analyze_image(image, None)
}

pub fn analyze_with_progress(image: Image, progress: &LoadProgress) -> Result<Analysis> {
    analyze_image(image, Some(progress))
}

fn analyze_image(image: Image, progress: Option<&LoadProgress>) -> Result<Analysis> {
    progress::report(progress, "Analyzing layer differences")?;
    let mut filesystem = Filesystem::new();
    let mut layers = Vec::with_capacity(image.layers.len());

    for layer in image.layers {
        progress::report(progress, "Analyzing layer differences")?;
        let (changes, updates) = apply_layer(&mut filesystem, &layer.entries, progress)
            .with_context(|| format!("cannot apply layer {} ({})", layer.index, layer.id))?;
        layers.push(LayerAnalysis {
            layer,
            changes,
            updates,
        });
    }

    Ok(Analysis {
        image: image.metadata,
        layers,
        filesystem,
    })
}

fn apply_layer(
    filesystem: &mut Filesystem,
    entries: &[Arc<FileEntry>],
    progress: Option<&LoadProgress>,
) -> Result<(Vec<Change>, Vec<Arc<FileEntry>>)> {
    let mut previous = PreviousEntries::new();

    apply_whiteouts(filesystem, &mut previous, entries, progress)?;
    let links = apply_entries(filesystem, &mut previous, entries, progress)?;
    resolve_hardlinks(filesystem, &mut previous, links, progress)?;

    collect_changes(filesystem, previous, progress)
}

// Whiteouts affect inherited paths before any entries from this layer are inserted.
fn apply_whiteouts(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    entries: &[Arc<FileEntry>],
    progress: Option<&LoadProgress>,
) -> Result<()> {
    for entry in entries {
        progress::check(progress)?;
        match entry.kind {
            FileKind::Whiteout => {
                let (parent, name) = entry
                    .path
                    .rsplit_once('/')
                    .context("whiteout without parent")?;
                let target = format!(
                    "{parent}/{}",
                    name.strip_prefix(".wh.").context("invalid whiteout")?
                );
                remove_subtree(filesystem, previous, &target, false, progress)?;
            }
            FileKind::OpaqueWhiteout => {
                let (parent, _) = entry
                    .path
                    .rsplit_once('/')
                    .context("opaque whiteout without parent")?;
                remove_subtree(
                    filesystem,
                    previous,
                    if parent.is_empty() { "/" } else { parent },
                    true,
                    progress,
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn apply_entries(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    entries: &[Arc<FileEntry>],
    progress: Option<&LoadProgress>,
) -> Result<Vec<Arc<FileEntry>>> {
    let mut links = Vec::new();
    for entry in entries {
        progress::check(progress)?;
        match entry.kind {
            FileKind::Whiteout | FileKind::OpaqueWhiteout => {}
            FileKind::Hardlink => links.push(Arc::clone(entry)),
            _ => insert_entry(filesystem, previous, Arc::clone(entry), progress)?,
        }
    }

    Ok(links)
}

// Resolve links after ordinary entries, including lower-layer targets and chains.
fn resolve_hardlinks(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    mut links: Vec<Arc<FileEntry>>,
    progress: Option<&LoadProgress>,
) -> Result<()> {
    let mut pending: BTreeSet<String> = links
        .iter()
        .map(|entry| {
            progress::check(progress)?;
            Ok(entry.path.clone())
        })
        .collect::<Result<_>>()?;

    while !links.is_empty() {
        let mut remaining = Vec::new();
        let mut progressed = false;
        for link in links {
            progress::check(progress)?;
            let target = link
                .link_target
                .as_ref()
                .context("hardlink has no target")?;
            if pending.contains(target) || !filesystem.contains_key(target) {
                remaining.push(link);
                continue;
            }

            let source = filesystem
                .get(target)
                .context("hardlink target disappeared")?;
            ensure!(
                matches!(source.kind, FileKind::File | FileKind::Hardlink),
                "unsupported hardlink target {target}"
            );

            let mut resolved = (*link).clone();
            resolved.size_bytes = source.size_bytes;
            resolved.content_digest = source.content_digest.clone();
            resolved.content_layer_index = source.content_layer_index;

            // A hardlink shares the target inode's attributes; tar link headers are not a new inode.
            resolved.mode = source.mode;
            resolved.uid = source.uid;
            resolved.gid = source.gid;
            resolved.mtime = source.mtime.clone();
            resolved.xattrs = source.xattrs.clone();

            insert_entry(filesystem, previous, Arc::new(resolved), progress)?;
            pending.remove(&link.path);
            progressed = true;
        }

        ensure!(
            progressed,
            "unresolved or cyclic hardlinks: {}",
            pending.into_iter().collect::<Vec<_>>().join(", ")
        );
        links = remaining;
    }

    Ok(())
}

fn collect_changes(
    filesystem: &Filesystem,
    previous: PreviousEntries,
    progress: Option<&LoadProgress>,
) -> Result<(Vec<Change>, Vec<Arc<FileEntry>>)> {
    let mut changes = Vec::new();
    let mut updates = Vec::new();
    for (path, before) in previous {
        progress::check(progress)?;
        let after = filesystem.get(&path).cloned();
        if let Some(entry) = &after {
            updates.push(Arc::clone(entry));
        }
        let kind = match (&before, &after) {
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Removed,
            (Some(before), Some(after)) if !before.same_file(after) => ChangeKind::Modified,
            _ => continue,
        };
        changes.push(Change {
            path,
            kind,
            before,
            after,
        });
    }
    Ok((changes, updates))
}

fn insert_entry(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    entry: Arc<FileEntry>,
    progress: Option<&LoadProgress>,
) -> Result<()> {
    let mut parent = entry.path.as_str();
    while let Some((ancestor, _)) = parent.rsplit_once('/') {
        progress::check(progress)?;
        if ancestor.is_empty() {
            break;
        }
        if let Some(existing) = filesystem.get(ancestor) {
            ensure!(
                existing.kind == FileKind::Directory,
                "non-directory ancestor {ancestor} for {}",
                entry.path
            );
        }
        parent = ancestor;
    }
    ensure!(
        entry.path != "/" || entry.kind == FileKind::Directory,
        "filesystem root must be a directory"
    );

    let keep_children = entry.kind == FileKind::Directory
        && filesystem
            .get(&entry.path)
            .is_none_or(|old| old.kind == FileKind::Directory);
    if !keep_children {
        remove_subtree(filesystem, previous, &entry.path, false, progress)?;
    }

    previous
        .entry(entry.path.clone())
        .or_insert_with(|| filesystem.get(&entry.path).cloned());
    filesystem.insert(entry.path.clone(), entry);
    Ok(())
}

fn remove_subtree(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    path: &str,
    descendants_only: bool,
    progress: Option<&LoadProgress>,
) -> Result<()> {
    let prefix = if path == "/" {
        "/".to_owned()
    } else {
        format!("{path}/")
    };
    let mut paths: Vec<String> = filesystem
        .range(prefix.clone()..)
        .take_while(|(key, _)| key.starts_with(&prefix))
        .filter(|(key, _)| key.as_str() != path)
        .map(|(key, _)| {
            progress::check(progress)?;
            Ok(key.clone())
        })
        .collect::<Result<_>>()?;
    if !descendants_only && filesystem.contains_key(path) {
        paths.push(path.to_owned());
    }

    for path in paths {
        progress::check(progress)?;
        let removed = filesystem.remove(&path);
        previous.entry(path).or_insert(removed);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn cancellation_stops_subtree_removal_and_diff_collection_inside_a_layer() {
        let (sender, _) = mpsc::channel();
        let progress = LoadProgress::new(sender);
        let entry = Arc::new(FileEntry {
            path: "/dir/file".into(),
            kind: FileKind::File,
            size_bytes: 0,
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: "0".into(),
            link_target: None,
            content_digest: None,
            device_major: None,
            device_minor: None,
            xattrs: BTreeMap::new(),
            layer_index: 0,
            content_layer_index: Some(0),
        });
        let mut filesystem = Filesystem::from([(entry.path.clone(), Arc::clone(&entry))]);
        let mut previous = PreviousEntries::from([(entry.path.clone(), None)]);
        // Model cancellation after earlier entries have been applied, before
        // the current layer's subtree removal or final diff collection.
        progress.cancel();
        let error = collect_changes(&filesystem, previous.clone(), Some(&progress)).unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        let error = remove_subtree(
            &mut filesystem,
            &mut previous,
            "/dir",
            false,
            Some(&progress),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(filesystem.contains_key(&entry.path));
        let error = apply_layer(&mut filesystem, &[entry], Some(&progress)).unwrap_err();
        assert!(error.to_string().contains("cancelled"));
    }
}
