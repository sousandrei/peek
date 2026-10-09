use std::fs::OpenOptions;
use std::io::BufWriter;

use anyhow::{Context, Result, bail};

use super::{AnalyzeOptions, ImageOptions, LayerFormat, Shell, Source, output};
use crate::{export, oci, tui};

fn image_input(options: &ImageOptions) -> Result<(&str, oci::Source)> {
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

    Ok((reference, source))
}

pub(super) fn inspect(options: &ImageOptions) -> Result<()> {
    let (reference, source) = image_input(options)?;
    tui::run(reference, source, options.platform.as_ref())
}

pub(super) fn analyze(options: &AnalyzeOptions) -> Result<()> {
    let (reference, source) = image_input(&options.image)?;
    let analysis = oci::analyze(oci::load(
        reference,
        source,
        options.image.platform.as_ref(),
    )?)?;

    if options.json {
        json_output(&analysis, options)
    } else {
        let mut stdout =
            anstream::AutoStream::new(std::io::stdout(), output::color_choice()).lock();
        output::analysis(&analysis, &mut stdout).context("cannot write analysis output")
    }
}

fn json_output(analysis: &oci::Analysis, options: &AnalyzeOptions) -> Result<()> {
    let view = match options.layers {
        LayerFormat::Diff => export::LayerView::Diff,
        LayerFormat::Full => export::LayerView::Full,
    };

    if let Some(destination) = &options.output {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .with_context(|| {
                format!(
                    "cannot create JSON output {} (existing files are not overwritten)",
                    destination.display()
                )
            })?;

        export::json(analysis, view, &mut BufWriter::new(file))
            .with_context(|| format!("cannot write JSON output {}", destination.display()))
    } else {
        export::json(analysis, view, &mut std::io::stdout().lock())
            .context("cannot write JSON to stdout")
    }
}

pub(super) fn version() -> Result<()> {
    println!("peek {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

pub(super) fn completion(_shell: Shell) -> Result<()> {
    Ok(())
}
