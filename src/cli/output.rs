//! Human-readable output for headless image analysis.

use std::io::{self, IsTerminal, Write};

use anstream::ColorChoice;
use unicode_width::UnicodeWidthChar;

use super::history;
use crate::{
    colors::{ColorRole, Colors},
    oci::{Analysis, Change, ChangeKind, LayerAnalysis},
};

pub(super) fn color_choice() -> ColorChoice {
    Colors::detect(io::stdout().is_terminal()).color_choice()
}

pub(super) fn analysis(
    analysis: &Analysis,
    output: &mut impl Write,
    colors: Colors,
) -> io::Result<()> {
    let width = terminal_width();

    write_styled(output, colors, ColorRole::Accent, "Image:", true)?;
    writeln!(output, " {}", analysis.image.reference.escape_debug())?;
    writeln!(output, "Layers: {}", analysis.layers.len())?;
    writeln!(output, "Visible paths: {}", analysis.filesystem.len())?;

    for layer in &analysis.layers {
        layer_output(layer, analysis.layers.len(), output, width, colors)?;
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
    colors: Colors,
) -> io::Result<()> {
    writeln!(output)?;
    write_styled(
        output,
        colors,
        ColorRole::Accent,
        &format!("Layer {}/{}", layer.layer.index + 1, total),
        true,
    )?;
    writeln!(output)?;
    command(&layer.layer.command, output, width)?;

    if layer.changes.is_empty() {
        write_styled(
            output,
            colors,
            ColorRole::Muted,
            "  No filesystem changes.",
            false,
        )?;
        writeln!(output)?;
        return Ok(());
    }

    change_groups(&layer.changes, output, colors)
}

fn change_groups(changes: &[Change], output: &mut impl Write, colors: Colors) -> io::Result<()> {
    for (kind, label, marker, role) in [
        (ChangeKind::Added, "Added", '+', ColorRole::Added),
        (ChangeKind::Modified, "Modified", '~', ColorRole::Modified),
        (ChangeKind::Removed, "Deleted", '-', ColorRole::Removed),
    ] {
        let entries = changes.iter().filter(|change| change.kind == kind);
        let count = entries.clone().count();

        if count == 0 {
            continue;
        }

        write!(output, "\n  ")?;
        write_styled(output, colors, role, &format!("{label} ({count})"), true)?;
        writeln!(output)?;
        for change in entries {
            write!(output, "    ")?;
            write_styled(output, colors, role, &marker.to_string(), false)?;
            writeln!(output, " {}", change.path.escape_debug())?;
        }
    }

    Ok(())
}

fn write_styled(
    output: &mut impl Write,
    colors: Colors,
    role: ColorRole,
    text: &str,
    bold: bool,
) -> io::Result<()> {
    let style = colors.ansi(role);
    let style = if bold && colors.is_enabled() {
        style.bold()
    } else {
        style
    };

    write!(output, "{style}{text}{style:#}")
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
