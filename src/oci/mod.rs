//! Read existing image archives without extracting them to the host filesystem.

mod archive;
mod digest;
mod filesystem;
mod layer;
mod model;

pub use filesystem::analyze;
pub use model::{Analysis, Change, FileEntry, Filesystem, Image, LayerAnalysis};

use std::fs::File;
use std::io::Cursor;
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy)]
pub enum Source {
    Docker,
    DockerArchive,
    Podman,
}

pub fn load(reference: &str, mut source: Source) -> Result<Image> {
    let reference = if let Some((scheme, image)) = reference.split_once("://") {
        source = match scheme {
            "docker" => Source::Docker,
            "docker-archive" | "docker-tar" => Source::DockerArchive,
            "podman" => Source::Podman,
            _ => bail!("unsupported image source scheme {scheme:?}"),
        };
        image
    } else {
        reference
    };

    match source {
        Source::DockerArchive => {
            let mut file = File::open(reference)
                .with_context(|| format!("cannot open image archive {reference:?}"))?;
            archive::read(&mut file, reference)
                .with_context(|| format!("cannot read image archive {reference:?}"))
        }
        Source::Docker | Source::Podman => {
            let engine = match source {
                Source::Docker => "docker",
                _ => "podman",
            };
            // Capture the saved archive in RAM; these commands do not build or pull images.
            let mut command = Command::new(engine);
            command.args(["image", "save"]);
            if matches!(source, Source::Podman) {
                command.args(["--format", "oci-archive"]);
            }
            let output = command
                .args(["--", reference])
                .output()
                .with_context(|| format!("cannot run {engine} image save"))?;
            if !output.status.success() {
                bail!(
                    "{engine} image save failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            archive::read(&mut Cursor::new(output.stdout), reference)
                .with_context(|| format!("cannot read archive returned by {engine}"))
        }
    }
}

#[cfg(test)]
pub(crate) use model::{ChangeKind, FileKind};

#[cfg(test)]
pub fn archive_for_test(bytes: &[u8]) -> Result<Image> {
    archive::read(&mut Cursor::new(bytes), "fixture")
}
