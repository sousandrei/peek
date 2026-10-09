//! Interactive layer and filesystem exploration using Ratatui.

mod input;
mod loading;
mod state;
mod tree;
mod view;

use std::io::{self, IsTerminal};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
};

use crate::oci::{Platform, Source};

pub fn run(reference: &str, source: Source, platform: Option<&Platform>) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "interactive inspection requires a terminal; use peek analyze IMAGE for text or JSON output"
    );
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init().context("cannot initialize terminal")?;
    execute!(io::stdout(), EnableMouseCapture).context("cannot enable mouse capture")?;

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        previous(panic);
    }));

    let theme = view::Theme::detect();
    let Some(analysis) = loading::load(&mut terminal, &theme, reference, source, platform)? else {
        return Ok(());
    };
    let mut app = state::App::new(&analysis);
    while !app.quit {
        terminal
            .draw(|frame| view::draw(frame, &mut app, &theme))
            .context("cannot draw image explorer")?;
        if event::poll(Duration::from_millis(250)).context("cannot poll terminal events")? {
            input::handle(
                &mut app,
                event::read().context("cannot read terminal event")?,
            );
        }
    }
    Ok(())
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            ratatui::crossterm::cursor::Show
        );
        ratatui::restore();
    }
}

#[cfg(test)]
mod tests;
