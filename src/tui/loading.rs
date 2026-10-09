//! Load on a worker while the terminal stays responsive.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
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
    let cancelled = Arc::new(AtomicBool::new(false));
    let (updates, receiver) = mpsc::channel();
    let progress = LoadProgress::new(updates, Arc::clone(&cancelled));
    let worker_reference = reference.to_owned();
    let platform = platform.cloned();

    terminal
        .draw(|frame| view::draw_loading(frame, theme, reference, stage, started.elapsed(), false))
        .context("cannot draw loading screen")?;
    let worker = thread::Builder::new()
        .name("image-loader".into())
        .spawn(move || {
            let image =
                oci::load_with_progress(&worker_reference, source, platform.as_ref(), &progress)?;
            oci::analyze_with_progress(image, &progress)
        })
        .context("cannot start image loader")?;

    let result = wait_for_image(
        terminal, theme, reference, started, &cancelled, &receiver, &worker,
    );
    if result.is_err() {
        cancelled.store(true, Ordering::Relaxed);
    }
    // Cancellation is cooperative during decoding; engine children are killed
    // and reaped. Keep drawing while cancellation finishes, then join the worker.
    let analysis = worker
        .join()
        .map_err(|_| anyhow::anyhow!("image loader panicked"))?;
    result?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    analysis.map(Some)
}

fn wait_for_image(
    terminal: &mut DefaultTerminal,
    theme: &Theme,
    reference: &str,
    started: Instant,
    cancelled: &AtomicBool,
    receiver: &mpsc::Receiver<&'static str>,
    worker: &thread::JoinHandle<Result<Analysis>>,
) -> Result<()> {
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
                    cancelled.load(Ordering::Relaxed),
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
            cancelled.store(true, Ordering::Relaxed);
        }
    }
    Ok(())
}
