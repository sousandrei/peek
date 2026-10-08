use std::io;

use super::{AnalyzeOptions, ImageOptions, Shell};
use crate::{analysis, export, image, tui};

pub(super) fn inspect(_options: &ImageOptions) -> io::Result<()> {
    image::load()?;
    analysis::analyze(None, None, None)?;

    tui::run()
}

pub(super) fn analyze(options: &AnalyzeOptions) -> io::Result<()> {
    image::load()?;
    analysis::analyze(
        options.lowest_efficiency.as_deref(),
        options.highest_wasted_bytes.as_deref(),
        options.highest_user_wasted_percent.as_deref(),
    )?;

    if options.json.is_some() {
        export::json()
    } else {
        Ok(())
    }
}

pub(super) fn version() -> io::Result<()> {
    println!("peek {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

pub(super) fn completion(_shell: Shell) -> io::Result<()> {
    Ok(())
}
