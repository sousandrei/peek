//! Read existing image archives without extracting them to the host filesystem.

mod archive;
mod digest;
mod engine;
mod filesystem;
mod input;
mod layer;
mod model;
mod platform;

pub use filesystem::analyze;
pub use model::{Analysis, Change, ChangeKind, FileEntry, Filesystem, Image, LayerAnalysis};
pub use platform::Platform;

use std::fs::File;
#[cfg(test)]
use std::io::Cursor;

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
            engine::load(engine, reference, platform)
        }
    }
}

#[cfg(test)]
pub(crate) use model::FileKind;

#[cfg(test)]
pub fn archive_for_test(bytes: &[u8]) -> Result<Image> {
    archive::read(&mut Cursor::new(bytes), "fixture", None)
}
