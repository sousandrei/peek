//! Read existing image archives without extracting them to the host filesystem.

mod archive;
mod digest;
mod engine;
mod filesystem;
mod input;
mod layer;
mod model;
mod platform;
mod progress;

pub use filesystem::{analyze, analyze_with_progress};
pub use model::{
    Analysis, Change, ChangeKind, FileEntry, FileKind, Filesystem, Image, LayerAnalysis,
};
pub use platform::Platform;
pub use progress::LoadProgress;

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

pub fn load(reference: &str, source: Source, platform: Option<&Platform>) -> Result<Image> {
    load_image(reference, source, platform, None)
}

pub fn load_with_progress(
    reference: &str,
    source: Source,
    platform: Option<&Platform>,
    progress: &LoadProgress,
) -> Result<Image> {
    load_image(reference, source, platform, Some(progress))
}

fn load_image(
    reference: &str,
    mut source: Source,
    platform: Option<&Platform>,
    progress: Option<&LoadProgress>,
) -> Result<Image> {
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
            progress::report(progress, "Reading image archive")?;
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
            engine::load(engine, reference, platform, progress)
        }
    }
}

#[cfg(test)]
pub fn archive_for_test(bytes: &[u8]) -> Result<Image> {
    archive::read(&mut Cursor::new(bytes), "fixture", None)
}
