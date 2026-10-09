//! Build a cumulative tree and keep display categories separate from archive data.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use regex::Regex;

use crate::oci::{Analysis, ChangeKind, FileEntry, FileKind, Filesystem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Added,
    Modified,
    Removed,
    Unchanged,
}

impl Category {
    pub fn index(self) -> usize {
        match self {
            Self::Added => 0,
            Self::Modified => 1,
            Self::Removed => 2,
            Self::Unchanged => 3,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Modified => "~",
            Self::Removed => "−",
            Self::Unchanged => "·",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    NameAscending,
    NameDescending,
    SizeAscending,
    SizeDescending,
}

impl SortOrder {
    pub fn next(self) -> Self {
        match self {
            Self::NameAscending => Self::NameDescending,
            Self::NameDescending => Self::SizeAscending,
            Self::SizeAscending => Self::SizeDescending,
            Self::SizeDescending => Self::NameAscending,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NameAscending => "s Name ↑",
            Self::NameDescending => "s Name ↓",
            Self::SizeAscending => "s Size ↑",
            Self::SizeDescending => "s Size ↓",
        }
    }

    fn compare(self, a: &Node, b: &Node) -> Ordering {
        let name = a.name().cmp(b.name());
        let size = || {
            a.entry()
                .map_or(0, |entry| entry.size_bytes)
                .cmp(&b.entry().map_or(0, |entry| entry.size_bytes))
        };
        b.directory.cmp(&a.directory).then_with(|| match self {
            Self::NameAscending => name,
            Self::NameDescending => name.reverse(),
            Self::SizeAscending => size().then(name),
            Self::SizeDescending => size().reverse().then(name),
        })
    }
}

pub struct Node {
    pub path: String,
    pub category: Category,
    pub before: Option<Arc<FileEntry>>,
    pub after: Option<Arc<FileEntry>>,
    pub directory: bool,
}

impl Node {
    pub fn entry(&self) -> Option<&FileEntry> {
        self.after.as_deref().or(self.before.as_deref())
    }

    pub fn depth(&self) -> usize {
        self.path
            .bytes()
            .filter(|byte| *byte == b'/')
            .count()
            .saturating_sub(1)
    }

    pub fn name(&self) -> &str {
        if self.path == "/" {
            "/"
        } else {
            self.path.rsplit('/').next().unwrap_or(&self.path)
        }
    }
}

pub struct Tree {
    pub nodes: BTreeMap<String, Node>,
    children: BTreeMap<String, Vec<String>>,
    pub counts: [usize; 4],
}

impl Tree {
    pub fn build(analysis: &Analysis, layer_index: usize, aggregate: bool) -> Self {
        let mut filesystem = Filesystem::new();
        for layer in analysis.layers.iter().take(layer_index + 1) {
            layer.apply_to(&mut filesystem);
        }

        let mut nodes: BTreeMap<_, _> = filesystem
            .iter()
            .map(|(path, entry)| {
                (
                    path.clone(),
                    node(
                        path,
                        Category::Unchanged,
                        Some(entry.clone()),
                        Some(entry.clone()),
                    ),
                )
            })
            .collect();

        if aggregate && layer_index > 0 {
            let mut baseline = Filesystem::new();
            if let Some(first) = analysis.layers.first() {
                first.apply_to(&mut baseline);
            }
            let paths: BTreeSet<_> = baseline.keys().chain(filesystem.keys()).collect();
            for path in paths {
                let before = baseline.get(path).cloned();
                let after = filesystem.get(path).cloned();
                let category = match (&before, &after) {
                    (None, Some(_)) => Category::Added,
                    (Some(_), None) => Category::Removed,
                    (Some(a), Some(b)) if !a.same_file(b) => Category::Modified,
                    _ => Category::Unchanged,
                };
                nodes.insert(path.clone(), node(path, category, before, after));
            }
        } else if let Some(layer) = analysis.layers.get(layer_index) {
            for change in &layer.changes {
                let category = match change.kind {
                    ChangeKind::Added => Category::Added,
                    ChangeKind::Modified => Category::Modified,
                    ChangeKind::Removed => Category::Removed,
                };
                nodes.insert(
                    change.path.clone(),
                    node(
                        &change.path,
                        category,
                        change.before.clone(),
                        change.after.clone(),
                    ),
                );
            }
        }

        let mut counts = [0; 4];
        for node in nodes.values() {
            counts[node.category.index()] += 1;
        }
        add_parents(&mut nodes);

        let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for path in nodes.keys().filter(|path| path.as_str() != "/") {
            children
                .entry(parent(path).into())
                .or_default()
                .push(path.clone());
        }
        Self {
            nodes,
            children,
            counts,
        }
    }

    pub fn visible(
        &self,
        categories: [bool; 4],
        filter: Option<&Regex>,
        collapsed: &BTreeSet<String>,
        sort: SortOrder,
    ) -> Vec<String> {
        let mut retained = BTreeSet::new();
        for node in self.nodes.values() {
            if categories[node.category.index()]
                && filter.is_none_or(|regex| regex.is_match(&node.path))
            {
                let mut path = node.path.as_str();
                retained.insert(path.to_owned());
                while path != "/" {
                    path = parent(path);
                    retained.insert(path.to_owned());
                }
            }
        }

        let mut visible = Vec::new();
        let mut pending = vec!["/".to_owned()];
        while let Some(path) = pending.pop() {
            if !retained.contains(&path) {
                continue;
            }
            visible.push(path.clone());
            if filter.is_none() && collapsed.contains(&path) {
                continue;
            }
            if let Some(children) = self.children.get(&path) {
                let mut children = children.clone();
                children.sort_by(|a, b| sort.compare(&self.nodes[a], &self.nodes[b]));
                pending.extend(children.into_iter().rev());
            }
        }
        visible
    }
}

pub fn parent(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
}

fn node(
    path: &str,
    category: Category,
    before: Option<Arc<FileEntry>>,
    after: Option<Arc<FileEntry>>,
) -> Node {
    let directory = after
        .as_ref()
        .or(before.as_ref())
        .is_some_and(|entry| entry.kind == FileKind::Directory);
    Node {
        path: path.into(),
        category,
        before,
        after,
        directory,
    }
}

fn add_parents(nodes: &mut BTreeMap<String, Node>) {
    let paths: Vec<_> = nodes.keys().cloned().collect();
    for path in paths {
        let mut path = path.as_str();
        while path != "/" {
            path = parent(path);
            nodes.entry(path.into()).or_insert_with(|| Node {
                path: path.into(),
                category: Category::Unchanged,
                before: None,
                after: None,
                directory: true,
            });
        }
    }
    // Parent categories reflect changed descendants; explicit changed directory entries win.
    let paths: Vec<_> = nodes.keys().rev().cloned().collect();
    for path in paths {
        if path == "/" {
            continue;
        }
        let category = nodes[&path].category;
        if category != Category::Unchanged {
            let parent = nodes.get_mut(parent(&path)).expect("tree parent exists");
            if parent.category == Category::Unchanged {
                parent.category = category;
            } else if parent.category != category {
                parent.category = Category::Modified;
            }
        }
    }
}
