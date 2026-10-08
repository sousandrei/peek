use anyhow::{Context, Result, bail};

use super::{AnalyzeOptions, ImageOptions, LayerFormat, Shell, Source};
use crate::{export, oci, tui};

fn load(options: &ImageOptions) -> Result<oci::Analysis> {
    if options.ignore_errors {
        bail!("best-effort archive parsing is not implemented; omit --ignore-errors");
    }

    let reference = options
        .image
        .as_deref()
        .context("an image reference is required")?;
    let source = match options.source {
        Source::Docker => oci::Source::Docker,
        Source::DockerArchive => oci::Source::DockerArchive,
        Source::Podman => oci::Source::Podman,
    };

    oci::analyze(oci::load(reference, source)?)
}

pub(super) fn inspect(options: &ImageOptions) -> Result<()> {
    tui::run(&load(options)?)
}

pub(super) fn analyze(options: &AnalyzeOptions) -> Result<()> {
    if options.lowest_efficiency.is_some()
        || options.highest_wasted_bytes.is_some()
        || options.highest_user_wasted_percent.is_some()
    {
        bail!("rule evaluation is not implemented yet; omit the threshold flags");
    }

    let analysis = load(&options.image)?;

    if let Some(destination) = &options.json {
        let view = match options.layers {
            LayerFormat::Diff => export::LayerView::Diff,
            LayerFormat::Full => export::LayerView::Full,
        };
        export::json(&analysis, view, destination)
    } else {
        println!(
            "{}: {} layers, {} visible paths",
            analysis.image.reference,
            analysis.layers.len(),
            analysis.filesystem.len()
        );

        for layer in &analysis.layers {
            println!(
                "Layer {}: {} changes — {}",
                layer.layer.index,
                layer.changes.len(),
                layer.layer.command
            );
        }
        Ok(())
    }
}

pub(super) fn version() -> Result<()> {
    println!("peek {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

pub(super) fn completion(_shell: Shell) -> Result<()> {
    Ok(())
}
