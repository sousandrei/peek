//! Apply OCI changesets to a RAM filesystem and retain incremental before/after records.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use super::model::{
    Analysis, Change, ChangeKind, FileEntry, FileKind, Filesystem, Image, LayerAnalysis,
};

type PreviousEntries = BTreeMap<String, Option<Arc<FileEntry>>>;

pub fn analyze(image: Image) -> Result<Analysis> {
    let mut filesystem = Filesystem::new();
    let mut layers = Vec::with_capacity(image.layers.len());

    for layer in image.layers {
        let (changes, updates) = apply_layer(&mut filesystem, &layer.entries)
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
) -> Result<(Vec<Change>, Vec<Arc<FileEntry>>)> {
    let mut previous = PreviousEntries::new();

    apply_whiteouts(filesystem, &mut previous, entries)?;
    let links = apply_entries(filesystem, &mut previous, entries)?;
    resolve_hardlinks(filesystem, &mut previous, links)?;

    Ok(collect_changes(filesystem, previous))
}

// Whiteouts affect inherited paths before any entries from this layer are inserted.
fn apply_whiteouts(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    entries: &[Arc<FileEntry>],
) -> Result<()> {
    for entry in entries {
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
                remove_subtree(filesystem, previous, &target, false);
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
                );
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
) -> Result<Vec<Arc<FileEntry>>> {
    let mut links = Vec::new();
    for entry in entries {
        match entry.kind {
            FileKind::Whiteout | FileKind::OpaqueWhiteout => {}
            FileKind::Hardlink => links.push(Arc::clone(entry)),
            _ => insert_entry(filesystem, previous, Arc::clone(entry))?,
        }
    }

    Ok(links)
}

// Resolve links after ordinary entries, including lower-layer targets and chains.
fn resolve_hardlinks(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    mut links: Vec<Arc<FileEntry>>,
) -> Result<()> {
    let mut pending: BTreeSet<String> = links.iter().map(|entry| entry.path.clone()).collect();

    while !links.is_empty() {
        let mut remaining = Vec::new();
        let mut progressed = false;
        for link in links {
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

            insert_entry(filesystem, previous, Arc::new(resolved))?;
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
) -> (Vec<Change>, Vec<Arc<FileEntry>>) {
    let mut changes = Vec::new();
    let mut updates = Vec::new();
    for (path, before) in previous {
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
    (changes, updates)
}

fn insert_entry(
    filesystem: &mut Filesystem,
    previous: &mut PreviousEntries,
    entry: Arc<FileEntry>,
) -> Result<()> {
    let mut parent = entry.path.as_str();
    while let Some((ancestor, _)) = parent.rsplit_once('/') {
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
        remove_subtree(filesystem, previous, &entry.path, false);
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
) {
    let prefix = if path == "/" {
        "/".to_owned()
    } else {
        format!("{path}/")
    };
    let mut paths: Vec<String> = filesystem
        .range(prefix.clone()..)
        .take_while(|(key, _)| key.starts_with(&prefix))
        .filter(|(key, _)| key.as_str() != path)
        .map(|(key, _)| key.clone())
        .collect();
    if !descendants_only && filesystem.contains_key(path) {
        paths.push(path.to_owned());
    }

    for path in paths {
        let removed = filesystem.remove(&path);
        previous.entry(path).or_insert(removed);
    }
}
