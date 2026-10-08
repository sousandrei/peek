//! Read existing image archives without extracting them to the host filesystem.

mod archive;
mod digest;
mod filesystem;
mod layer;
mod model;
mod platform;

pub use filesystem::analyze;
pub use model::{Analysis, Change, ChangeKind, FileEntry, Filesystem, Image, LayerAnalysis};
pub use platform::Platform;

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

pub fn load(reference: &str, mut source: Source, platform: Option<&Platform>) -> Result<Image> {
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
            archive::read(&mut file, reference, platform)
                .with_context(|| format!("cannot read image archive {reference:?}"))
        }
        Source::Docker | Source::Podman => {
            let engine = match source {
                Source::Docker => "docker",
                _ => "podman",
            };
            let saved_reference = engine_reference(reference);

            // Capture the saved archive in RAM; these commands do not build or pull images.
            let mut command = Command::new(engine);
            command.args(["image", "save"]);
            if matches!(source, Source::Podman) {
                command.args(["--format", "oci-archive"]);
            }
            let output = command
                .args(["--", &saved_reference])
                .output()
                .with_context(|| format!("cannot run {engine} image save"))?;
            if !output.status.success() {
                bail!(
                    "{engine} image save for {saved_reference:?} failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            archive::read(&mut Cursor::new(output.stdout), reference, platform)
                .with_context(|| format!("cannot read archive returned by {engine}"))
        }
    }
}

fn engine_reference(reference: &str) -> String {
    let name = reference.rsplit('/').next().unwrap_or(reference);
    let is_id = (12..=64).contains(&reference.len())
        && reference.bytes().all(|byte| byte.is_ascii_hexdigit());

    if reference.contains('@') || name.contains(':') || is_id {
        reference.into()
    } else {
        // Saving an untagged repository exports every tag instead of resolving :latest.
        format!("{reference}:latest")
    }
}

#[cfg(test)]
pub(crate) use model::FileKind;

#[cfg(test)]
pub fn archive_for_test(bytes: &[u8]) -> Result<Image> {
    archive::read(&mut Cursor::new(bytes), "fixture", None)
}
