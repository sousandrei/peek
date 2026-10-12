//! Load on a worker while the terminal stays responsive.

use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
};

use super::view::{self, Theme};
use crate::oci::{self, Analysis, LoadProgress, Platform, Source};

pub fn load(
    terminal: &mut DefaultTerminal,
    theme: &Theme,
    reference: &str,
    source: Source,
    platform: Option<&Platform>,
) -> Result<Option<Analysis>> {
    let started = Instant::now();
    let stage = "Loading image";
    let (updates, receiver) = mpsc::channel();
    let progress = Arc::new(LoadProgress::new(updates));
    let worker_progress = Arc::clone(&progress);
    let worker_reference = reference.to_owned();
    let platform = platform.cloned();

    terminal
        .draw(|frame| view::draw_loading(frame, theme, reference, stage, started.elapsed(), false))
        .context("cannot draw loading screen")?;
    let worker = thread::Builder::new()
        .name("image-loader".into())
        .spawn(move || {
            let image = oci::load_with_progress(
                &worker_reference,
                source,
                platform.as_ref(),
                &worker_progress,
            )?;
            oci::analyze_with_progress(image, &worker_progress)
        })
        .context("cannot start image loader")?;

    let cancelled = match wait_for_image(
        terminal, theme, reference, started, &progress, &receiver, &worker,
    ) {
        Ok(cancelled) => cancelled,
        Err(error) => {
            progress.cancel();
            drop(worker);
            return Err(error);
        }
    };
    if cancelled {
        drop(worker);
        return Ok(None);
    }
    let analysis = worker
        .join()
        .map_err(|_| anyhow::anyhow!("image loader panicked"))?;
    analysis.map(Some)
}

fn wait_for_image(
    terminal: &mut DefaultTerminal,
    theme: &Theme,
    reference: &str,
    started: Instant,
    progress: &LoadProgress,
    receiver: &mpsc::Receiver<&'static str>,
    worker: &thread::JoinHandle<Result<Analysis>>,
) -> Result<bool> {
    let mut stage = "Loading image";
    while !worker.is_finished() {
        while let Ok(update) = receiver.try_recv() {
            stage = update;
        }
        terminal
            .draw(|frame| {
                view::draw_loading(
                    frame,
                    theme,
                    reference,
                    stage,
                    started.elapsed(),
                    progress.is_cancelled(),
                )
            })
            .context("cannot draw loading screen")?;
        if event::poll(Duration::from_millis(80)).context("cannot poll loading events")?
            && let Event::Key(key) = event::read().context("cannot read loading event")?
            && key.kind != KeyEventKind::Release
            && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            progress.cancel();
            return Ok(true);
        }
    }
    Ok(false)
}
