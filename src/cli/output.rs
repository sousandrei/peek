//! Human-readable output for headless image analysis.

use std::io::{self, IsTerminal, Write};

use anstream::ColorChoice;
use unicode_width::UnicodeWidthChar;

use super::history;
use crate::oci::{Analysis, Change, ChangeKind, LayerAnalysis};

pub(super) fn color_choice() -> ColorChoice {
    let plain = !io::stdout().is_terminal()
        || std::env::var_os("CI").is_some()
        || std::env::var_os("NO_COLOR").is_some()
        || std::env::var_os("TERM").is_some_and(|term| term == "dumb");

    if plain {
        ColorChoice::Never
    } else {
        ColorChoice::Auto
    }
}

pub(super) fn analysis(analysis: &Analysis, output: &mut impl Write) -> io::Result<()> {
    let width = terminal_width();

    writeln!(
        output,
        "\x1b[1;36mImage:\x1b[0m {}",
        analysis.image.reference.escape_debug()
    )?;
    writeln!(output, "Layers: {}", analysis.layers.len())?;
    writeln!(output, "Visible paths: {}", analysis.filesystem.len())?;

    for layer in &analysis.layers {
        layer_output(layer, analysis.layers.len(), output, width)?;
    }

    Ok(())
}

fn terminal_width() -> usize {
    if io::stdout().is_terminal() {
        ratatui::crossterm::terminal::size()
            .ok()
            .map(|(width, _)| usize::from(width))
    } else {
        None
    }
    .filter(|width| *width > 0)
    .unwrap_or(100)
}

fn layer_output(
    layer: &LayerAnalysis,
    total: usize,
    output: &mut impl Write,
    width: usize,
) -> io::Result<()> {
    writeln!(
        output,
        "\n\x1b[1;36mLayer {}/{}\x1b[0m",
        layer.layer.index + 1,
        total
    )?;
    command(&layer.layer.command, output, width)?;

    if layer.changes.is_empty() {
        writeln!(output, "  \x1b[2mNo filesystem changes.\x1b[0m")?;
        return Ok(());
    }

    change_groups(&layer.changes, output)
}

fn change_groups(changes: &[Change], output: &mut impl Write) -> io::Result<()> {
    for (kind, label, marker, color) in [
        (ChangeKind::Added, "Added", '+', 32),
        (ChangeKind::Modified, "Modified", '~', 33),
        (ChangeKind::Removed, "Deleted", '-', 31),
    ] {
        let entries = changes.iter().filter(|change| change.kind == kind);
        let count = entries.clone().count();

        if count == 0 {
            continue;
        }

        writeln!(output, "\n  \x1b[1;{color}m{label} ({count})\x1b[0m")?;
        for change in entries {
            writeln!(
                output,
                "    \x1b[{color}m{marker}\x1b[0m {}",
                change.path.escape_debug()
            )?;
        }
    }

    Ok(())
}

fn command(command: &str, output: &mut impl Write, width: usize) -> io::Result<()> {
    writeln!(output, "  Command:")?;
    let width = width.saturating_sub(4).max(1);

    let command = history::layout(command);

    for line in command.lines() {
        let line: String = line
            .chars()
            .map(|character| match character {
                '\t' => "    ".to_owned(),
                character if character.is_control() => character.escape_debug().to_string(),
                character => character.to_string(),
            })
            .collect();
        wrapped_line(&line, output, width)?;
    }

    Ok(())
}

fn wrapped_line(line: &str, output: &mut impl Write, width: usize) -> io::Result<()> {
    if line.is_empty() {
        return writeln!(output);
    }

    let mut remaining = line;
    while !remaining.is_empty() {
        let end = line_break(remaining, width);
        writeln!(output, "    {}", remaining[..end].trim_end())?;
        remaining = remaining[end..].trim_start_matches(' ');
    }

    Ok(())
}

fn line_break(line: &str, width: usize) -> usize {
    let mut columns = 0;
    let mut space = None;

    for (index, character) in line.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if columns + character_width > width {
            let end = space.filter(|index| *index > 0).unwrap_or(index);
            return if end == 0 { character.len_utf8() } else { end };
        }

        columns += character_width;
        if character == ' ' {
            space = Some(index);
        }
    }

    line.len()
}
