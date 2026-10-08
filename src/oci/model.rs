//! Image and virtual filesystem data shared by analysis and presentation.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

pub type Filesystem = BTreeMap<String, Arc<FileEntry>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    pub reference: String,
    pub tags: Vec<String>,
    pub architecture: String,
    pub os: String,
}

#[derive(Debug)]
pub struct Image {
    pub metadata: ImageMetadata,
    pub layers: Vec<Layer>,
}

#[derive(Debug)]
pub struct Layer {
    pub index: usize,
    pub id: String,
    pub diff_id: Option<String>,
    pub command: String,
    pub blob_size_bytes: u64,
    pub entries: Vec<Arc<FileEntry>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    Hardlink,
    Fifo,
    CharacterDevice,
    BlockDevice,
    Whiteout,
    OpaqueWhiteout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub kind: FileKind,
    pub size_bytes: u64,
    pub mode: u32,
    pub uid: u64,
    pub gid: u64,
    pub mtime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_major: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_minor: Option<u32>,
    pub xattrs: BTreeMap<String, String>,
    pub layer_index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_layer_index: Option<usize>,
}

impl FileEntry {
    pub fn same_file(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.size_bytes == other.size_bytes
            && self.mode == other.mode
            && self.uid == other.uid
            && self.gid == other.gid
            && self.mtime == other.mtime
            && self.link_target == other.link_target
            && self.content_digest == other.content_digest
            && self.device_major == other.device_major
            && self.device_minor == other.device_minor
            && self.xattrs == other.xattrs
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub before: Option<Arc<FileEntry>>,
    pub after: Option<Arc<FileEntry>>,
}

#[derive(Debug)]
pub struct LayerAnalysis {
    pub layer: Layer,
    pub changes: Vec<Change>,
    pub updates: Vec<Arc<FileEntry>>,
}

#[derive(Debug)]
pub struct Analysis {
    pub image: ImageMetadata,
    pub layers: Vec<LayerAnalysis>,
    pub filesystem: Filesystem,
}

impl LayerAnalysis {
    pub fn apply_to(&self, filesystem: &mut Filesystem) {
        for change in &self.changes {
            if change.after.is_none() {
                filesystem.remove(&change.path);
            }
        }
        for entry in &self.updates {
            filesystem.insert(entry.path.clone(), Arc::clone(entry));
        }
    }
}
