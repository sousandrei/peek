//! Versioned JSON views of the OCI analysis model.

use std::io::Write;

use anyhow::Result;
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::oci::{Analysis, Change, FileEntry, Filesystem, LayerAnalysis};

#[derive(Clone, Copy)]
pub enum LayerView {
    Diff,
    Full,
}

pub fn json(analysis: &Analysis, view: LayerView, output: &mut impl Write) -> Result<()> {
    serde_json::to_writer_pretty(&mut *output, &Document { analysis, view })?;
    writeln!(output)?;
    output.flush()?;

    Ok(())
}

struct Document<'a> {
    analysis: &'a Analysis,
    view: LayerView,
}

impl Serialize for Document<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut document = serializer.serialize_struct("Analysis", 3)?;
        document.serialize_field("schemaVersion", &1)?;
        document.serialize_field("image", &self.analysis.image)?;
        document.serialize_field(
            "layers",
            &Layers {
                layers: &self.analysis.layers,
                view: self.view,
            },
        )?;
        document.end()
    }
}

struct Layers<'a> {
    layers: &'a [LayerAnalysis],
    view: LayerView,
}

impl Serialize for Layers<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.layers.len()))?;
        let mut filesystem = Filesystem::new();

        for layer in self.layers {
            let data = match self.view {
                LayerView::Diff => LayerData::Diff {
                    changes: &layer.changes,
                },
                LayerView::Full => {
                    layer.apply_to(&mut filesystem);
                    LayerData::Full {
                        files: filesystem.values().map(|entry| entry.as_ref()).collect(),
                    }
                }
            };

            sequence.serialize_element(&LayerOutput {
                index: layer.layer.index,
                id: &layer.layer.id,
                diff_id: &layer.layer.diff_id,
                command: &layer.layer.command,
                blob_size_bytes: layer.layer.blob_size_bytes,
                data,
            })?;
        }

        sequence.end()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LayerOutput<'a> {
    index: usize,
    id: &'a str,
    diff_id: &'a Option<String>,
    command: &'a str,
    blob_size_bytes: u64,
    data: LayerData<'a>,
}

#[derive(Serialize)]
#[serde(tag = "view", rename_all = "camelCase")]
enum LayerData<'a> {
    Diff { changes: &'a [Change] },
    Full { files: Vec<&'a FileEntry> },
}

#[cfg(test)]
pub fn value(analysis: &Analysis, view: LayerView) -> serde_json::Value {
    serde_json::to_value(Document { analysis, view }).expect("serialize test analysis")
}
