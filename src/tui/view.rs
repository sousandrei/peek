//! Layout and rendering for the layer explorer.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::cli::history;
use crate::oci::FileEntry;

use super::state::{Action, App, Areas, Focus};
use super::tree::Category;

pub struct Theme {
    pub background: Color,
    pub panel: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub selection: Color,
    pub added: Color,
    pub modified: Color,
    pub removed: Color,
}

impl Theme {
    pub fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self {
                background: Color::Reset,
                panel: Color::Reset,
                text: Color::Reset,
                muted: Color::Reset,
                border: Color::Reset,
                accent: Color::Reset,
                selection: Color::Reset,
                added: Color::Reset,
                modified: Color::Reset,
                removed: Color::Reset,
            };
        }
        let truecolor =
            std::env::var("COLORTERM").is_ok_and(|value| value == "truecolor" || value == "24bit");
        if truecolor {
            Self {
                background: Color::Rgb(14, 18, 26),
                panel: Color::Rgb(20, 25, 35),
                text: Color::Rgb(218, 226, 238),
                muted: Color::Rgb(131, 147, 170),
                border: Color::Rgb(49, 62, 82),
                accent: Color::Rgb(110, 207, 229),
                selection: Color::Rgb(36, 54, 74),
                added: Color::Rgb(144, 211, 151),
                modified: Color::Rgb(234, 192, 124),
                removed: Color::Rgb(234, 143, 153),
            }
        } else {
            Self {
                background: Color::Reset,
                panel: Color::Reset,
                text: Color::Gray,
                muted: Color::DarkGray,
                border: Color::DarkGray,
                accent: Color::Cyan,
                selection: Color::DarkGray,
                added: Color::Green,
                modified: Color::Yellow,
                removed: Color::Red,
            }
        }
    }

    fn category(&self, category: Category) -> Color {
        match category {
            Category::Added => self.added,
            Category::Modified => self.modified,
            Category::Removed => self.removed,
            Category::Unchanged => self.muted,
        }
    }
}

pub fn draw_loading(
    frame: &mut Frame<'_>,
    theme: &Theme,
    reference: &str,
    stage: &str,
    elapsed: std::time::Duration,
    cancelling: bool,
) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::default().bg(theme.background).fg(theme.text)),
        area,
    );
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let tick = (elapsed.as_millis() / 100 % spinner.len() as u128) as usize;
    let status = if cancelling {
        "Cancelling image loading"
    } else {
        stage
    };
    let hint = if cancelling {
        Line::from("Waiting for the current operation to stop")
    } else {
        Line::from(
            shortcut_spans("q", " / ")
                .into_iter()
                .chain(shortcut_spans("Esc", " quit"))
                .collect::<Vec<_>>(),
        )
    }
    .style(Style::default().fg(theme.muted));
    let lines = vec![
        Line::from(Span::styled(
            "PEEK",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(clean(reference)),
        Line::default(),
        Line::from(Span::styled(
            format!("{} {status}", spinner[tick]),
            Style::default().fg(theme.accent),
        )),
        Line::from(Span::styled(
            format!("{:.1}s elapsed", elapsed.as_secs_f64()),
            Style::default().fg(theme.muted),
        )),
        Line::default(),
        hint,
    ];
    let height = (lines.len() as u16).min(area.height);
    let content = Rect::new(
        area.x,
        area.y + (area.height - height) / 2,
        area.width,
        height,
    );
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        content,
    );
}

pub fn draw(frame: &mut Frame<'_>, app: &mut App<'_>, theme: &Theme) {
    app.areas = Areas::default();
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::default().bg(theme.background).fg(theme.text)),
        area,
    );
    if area.width < 40 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("Peek\n\nResize to at least 40 × 12.\nPress q to quit.")
                .style(Style::default().fg(theme.accent)),
            area,
        );
        return;
    }

    let buttons = control_buttons(app, theme);
    let button_rects = control_rects(&buttons, area.width);
    let controls_height = button_rects.last().map_or(0, |rect| rect.y + 1);
    let status_height = u16::from(app.filter_edit || !app.filter.is_empty());

    let [header, body, status, controls, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(status_height),
        Constraint::Length(controls_height),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, app, theme, header);

    if area.width >= 90 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(66)])
                .areas(body);
        let [layers, command] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(left);
        let [files, details] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(7)]).areas(right);
        draw_layers(frame, app, theme, layers);
        draw_command(frame, app, theme, command);
        draw_files(frame, app, theme, files);
        draw_details(frame, app, theme, details);
    } else {
        match app.focus {
            Focus::Layers => draw_layers(frame, app, theme, body),
            Focus::Command => draw_command(frame, app, theme, body),
            Focus::Files => {
                let [files, details] =
                    Layout::vertical([Constraint::Min(3), Constraint::Length(5)]).areas(body);
                draw_files(frame, app, theme, files);
                draw_details(frame, app, theme, details);
            }
        }
    }

    draw_status(frame, app, theme, status);
    draw_controls(frame, app, buttons, button_rects, controls);
    let mut hint = vec![Span::raw(" ")];
    if app.filter_edit {
        hint.push(Span::raw("Type a path regex  ·  "));
        hint.extend(shortcut_spans("Enter", " apply  ·  "));
        hint.extend(shortcut_spans("Esc", " clear"));
    } else {
        hint.extend(shortcut_spans("Tab", " layers/files  "));
        hint.extend(shortcut_spans("↑↓", " move  "));
        hint.extend(shortcut_spans("[]", " layer  "));
        hint.extend(shortcut_spans("/", " filter"));
    }
    let [shortcuts, exit] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(16)]).areas(footer);
    frame.render_widget(
        Paragraph::new(Line::from(hint)).style(Style::default().fg(theme.muted)),
        shortcuts,
    );
    frame.render_widget(
        Paragraph::new(Line::from(
            shortcut_spans("?", " help  ")
                .into_iter()
                .chain(shortcut_spans("q", " quit"))
                .collect::<Vec<_>>(),
        ))
        .style(Style::default().fg(theme.muted)),
        exit,
    );
    app.areas
        .buttons
        .push((Rect::new(exit.x, exit.y, 6, 1), Action::Help));
    if app.help {
        draw_help(frame, theme, area);
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &App<'_>, theme: &Theme, area: Rect) {
    let layer_count = app.analysis.layers.len();
    let bytes = app
        .analysis
        .layers
        .iter()
        .map(|layer| layer.layer.blob_size_bytes)
        .sum::<u64>();
    let title = Line::from(vec![
        Span::styled(
            " PEEK ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(clean(&app.analysis.image.reference)),
        Span::styled(
            format!(
                "  {} / {}  ·  {layer_count} layers  ·  {} stored",
                app.analysis.image.os,
                app.analysis.image.architecture,
                bytes_label(bytes)
            ),
            Style::default().fg(theme.muted),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), Rect { height: 1, ..area });
}

fn control_buttons(app: &App<'_>, theme: &Theme) -> Vec<(String, Action, Color)> {
    let mut buttons = Vec::new();
    let compare = if app.aggregate {
        "c Since base"
    } else {
        "c This layer"
    };
    let sort = app.sort.label();
    let attrs = if app.attributes {
        "b Attributes on"
    } else {
        "b Attributes off"
    };
    for (label, action) in [
        (compare, Action::Compare),
        (sort, Action::Sort),
        (attrs, Action::Attributes),
    ] {
        buttons.push((label.into(), action, theme.muted));
    }
    for (index, (name, color)) in [
        ("a Added", theme.added),
        ("m Modified", theme.modified),
        ("d Deleted", theme.removed),
        ("u Unchanged", theme.muted),
    ]
    .into_iter()
    .enumerate()
    {
        let enabled = app.categories[index];
        buttons.push((
            format!("{} {name}", if enabled { "●" } else { "○" }),
            Action::Category(index),
            if enabled { color } else { theme.muted },
        ));
    }

    buttons
}

fn control_rects(buttons: &[(String, Action, Color)], width: u16) -> Vec<Rect> {
    let available = width.saturating_sub(2);
    let (display, categories) = buttons.split_at(3);
    let mut left = control_group_rects(display, available, false);
    let mut right = control_group_rects(categories, available, true);

    let same_row = left.last().is_some_and(|rect| rect.y == 0)
        && right.last().is_some_and(|rect| rect.y == 0)
        && left.last().unwrap().right() + 3 <= right[0].x;
    if !same_row {
        let offset = left.last().map_or(0, |rect| rect.y + 1);
        for rect in &mut right {
            rect.y += offset;
        }
    }
    left.extend(right);
    left
}

fn control_group_rects(
    buttons: &[(String, Action, Color)],
    available: u16,
    align_right: bool,
) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut start = 0;
    let mut y = 0;

    while start < buttons.len() {
        let mut end = start;
        let mut used = 0;
        while end < buttons.len() {
            let label_width = buttons[end].0.width() as u16;
            let next = used + label_width + if end > start { 2 } else { 0 };
            if next > available && end > start {
                break;
            }
            used = next;
            end += 1;
        }

        let mut x = 1 + if align_right {
            available.saturating_sub(used)
        } else {
            0
        };
        for (label, _, _) in &buttons[start..end] {
            let label_width = (label.width() as u16).min(available);
            rects.push(Rect::new(x, y, label_width, 1));
            x += label_width + 2;
        }
        start = end;
        y += 1;
    }
    rects
}

fn draw_controls(
    frame: &mut Frame<'_>,
    app: &mut App<'_>,
    buttons: Vec<(String, Action, Color)>,
    rects: Vec<Rect>,
    area: Rect,
) {
    for ((label, action, color), rect) in buttons.into_iter().zip(rects) {
        let rect = Rect::new(area.x + rect.x, area.y + rect.y, rect.width, 1);
        frame.render_widget(
            Paragraph::new(control_label(&label)).style(Style::default().fg(color)),
            rect,
        );
        app.areas.buttons.push((rect, action));
    }
}

fn shortcut_spans<'a>(key: &'a str, description: &'a str) -> [Span<'a>; 2] {
    [
        Span::styled(key, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(description),
    ]
}

fn control_label(label: &str) -> Line<'_> {
    let (first, rest) = label.split_once(' ').unwrap_or((label, ""));
    let (prefix, key, description) = if matches!(first, "●" | "○") {
        let (key, description) = rest.split_once(' ').unwrap_or((rest, ""));
        (format!("{first} "), key, description)
    } else {
        (String::new(), first, rest)
    };
    let key = Span::styled(key, Style::default().add_modifier(Modifier::BOLD));
    Line::from(vec![
        Span::raw(prefix),
        key,
        Span::raw(format!(" {description}")),
    ])
}

fn panel<'a>(title: impl Into<Line<'a>>, focused: bool, theme: &Theme) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(if focused { theme.accent } else { theme.border }))
        .style(Style::default().bg(theme.panel))
}

fn draw_layers(frame: &mut Frame<'_>, app: &mut App<'_>, theme: &Theme, area: Rect) {
    let block = panel(
        format!(
            " Layers  {}/{} ",
            app.layer + usize::from(!app.analysis.layers.is_empty()),
            app.analysis.layers.len()
        ),
        app.focus == Focus::Layers,
        theme,
    );
    app.areas.layers = block.inner(area);
    let width = usize::from(app.areas.layers.width.saturating_sub(20));
    let items: Vec<_> = app
        .analysis
        .layers
        .iter()
        .map(|layer| {
            let command = layer
                .layer
                .command
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let command = truncate(&clean(&command), width);
            let counts = layer.changes.iter().fold([0; 3], |mut counts, change| {
                counts[match change.kind {
                    crate::oci::ChangeKind::Added => 0,
                    crate::oci::ChangeKind::Modified => 1,
                    crate::oci::ChangeKind::Removed => 2,
                }] += 1;
                counts
            });
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:>2} ", layer.layer.index + 1),
                    Style::default().fg(theme.muted),
                ),
                Span::styled(
                    format!("{:>9} ", bytes_label(layer.layer.blob_size_bytes)),
                    Style::default().fg(theme.accent),
                ),
                Span::raw(command),
            ]))
            .style(if counts.iter().all(|count| *count == 0) {
                Style::default().fg(theme.muted)
            } else {
                Style::default().fg(theme.text)
            })
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(theme.selection)
                    .fg(theme.text)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▌"),
        area,
        &mut app.layers,
    );
}

fn draw_files(frame: &mut Frame<'_>, app: &mut App<'_>, theme: &Theme, area: Rect) {
    let block = panel(
        format!(" Files  {} shown ", app.visible.len()),
        app.focus == Focus::Files,
        theme,
    );
    app.areas.files = block.inner(area);
    if app.visible.is_empty() {
        let message = if app.filter_error.is_some() {
            "Invalid path expression. Edit the filter or press Esc."
        } else if app.analysis.layers.is_empty() {
            "This image has no filesystem layers."
        } else {
            "No matching files. Clear the filter or enable more categories."
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(block)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(theme.muted)),
            area,
        );
        return;
    }

    let attributes = app.attributes && app.areas.files.width >= 55;
    let items: Vec<_> = app
        .visible
        .iter()
        .map(|path| {
            let node = &app.tree.nodes[path];
            let fold = if node.directory {
                if app.collapsed.contains(path) && app.filter.is_empty() {
                    "▸"
                } else {
                    "▾"
                }
            } else {
                " "
            };
            let mut spans = vec![Span::styled(
                format!(" {} ", node.category.symbol()),
                Style::default().fg(theme.category(node.category)),
            )];
            if attributes {
                let attribute = node
                    .entry()
                    .map(|entry| {
                        format!(
                            "{:04o} {:>3}:{:<3} {:>9}",
                            entry.mode,
                            entry.uid,
                            entry.gid,
                            bytes_label(entry.size_bytes)
                        )
                    })
                    .unwrap_or_else(|| "                    ".into());
                spans.push(Span::styled(
                    format!("{attribute}  "),
                    Style::default().fg(theme.muted),
                ));
            }
            let name = format!(
                "{}{} {}{}",
                "  ".repeat(node.depth().min(20)),
                fold,
                clean(node.name()),
                if node.directory && path != "/" {
                    "/"
                } else {
                    ""
                }
            );
            spans.push(Span::styled(
                name,
                Style::default().fg(if node.directory {
                    theme.accent
                } else {
                    theme.text
                }),
            ));
            if let Some(target) = node.entry().and_then(|entry| entry.link_target.as_ref()) {
                spans.push(Span::styled(
                    format!(" → {}", clean(target)),
                    Style::default().fg(theme.muted),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_symbol("▌")
            .highlight_style(
                Style::default()
                    .bg(theme.selection)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
        &mut app.files,
    );
}

fn draw_command(frame: &mut Frame<'_>, app: &mut App<'_>, theme: &Theme, area: Rect) {
    let block = panel(
        " Command  ·  Ctrl+↓ to focus ",
        app.focus == Focus::Command,
        theme,
    );
    app.areas.command = block.inner(area);
    let lines = if let Some(layer) = app.analysis.layers.get(app.layer) {
        let [a, m, d, _] = app.tree.counts;
        let mut lines = vec![
            Line::from(Span::styled(
                format!(" +{a} added  ~{m} modified  −{d} deleted"),
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::styled(
                clean(&layer.layer.id),
                Style::default().fg(theme.muted),
            )),
            Line::default(),
        ];
        lines.extend(
            history::layout(&layer.layer.command)
                .lines()
                .map(|line| Line::from(clean(line))),
        );
        lines
    } else {
        vec![Line::from("No filesystem history.")]
    };
    let mut paragraph = Paragraph::new(lines).style(Style::default().fg(theme.text));
    if app.wrap {
        paragraph = paragraph.wrap(Wrap { trim: false });
    }
    let line_count = paragraph.line_count(app.areas.command.width);
    app.command_limit = line_count
        .saturating_sub(usize::from(app.areas.command.height))
        .min(u16::MAX as usize) as u16;
    app.command_scroll = app.command_scroll.min(app.command_limit);
    frame.render_widget(paragraph.block(block).scroll((app.command_scroll, 0)), area);
}

fn draw_details(frame: &mut Frame<'_>, app: &App<'_>, theme: &Theme, area: Rect) {
    let block = panel(" Selected file ", false, theme);
    let Some(node) = app
        .selected_path()
        .and_then(|path| app.tree.nodes.get(path))
    else {
        frame.render_widget(
            Paragraph::new("Select a file to compare metadata.")
                .block(block)
                .style(Style::default().fg(theme.muted)),
            area,
        );
        return;
    };
    let mut lines = vec![Line::from(Span::styled(
        clean(&node.path),
        Style::default().fg(theme.accent),
    ))];
    if node.before.is_none() && node.after.is_none() {
        lines.push(Line::from("Directory inferred from its children."));
    } else {
        lines.push(Line::from(vec![
            Span::styled(" Before  ", Style::default().fg(theme.muted)),
            Span::raw(metadata(node.before.as_deref())),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" After   ", Style::default().fg(theme.muted)),
            Span::raw(metadata(node.after.as_deref())),
        ]));
        if let Some(entry) = node.entry() {
            lines.push(Line::from(Span::styled(
                format!(
                    " Entry layer {}  ·  content layer {}",
                    entry.layer_index + 1,
                    entry
                        .content_layer_index
                        .map(|index| (index + 1).to_string())
                        .unwrap_or_else(|| "—".into())
                ),
                Style::default().fg(theme.muted),
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block)
            .style(Style::default().fg(theme.text)),
        area,
    );
}

fn metadata(entry: Option<&FileEntry>) -> String {
    entry
        .map(|entry| {
            format!(
                "{:?}  {}  {:04o}  {}:{}{}",
                entry.kind,
                bytes_label(entry.size_bytes),
                entry.mode,
                entry.uid,
                entry.gid,
                entry
                    .link_target
                    .as_ref()
                    .map(|target| format!(" → {}", clean(target)))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_else(|| "not present".into())
}

fn draw_status(frame: &mut Frame<'_>, app: &App<'_>, theme: &Theme, area: Rect) {
    let text = format!(
        " / {}{}",
        clean(&app.filter),
        if app.filter_edit { "▏" } else { "" }
    );
    let color = if app.filter_error.is_some() {
        theme.removed
    } else {
        theme.accent
    };
    frame.render_widget(Paragraph::new(text).style(Style::default().fg(color)), area);
}

fn draw_help(frame: &mut Frame<'_>, theme: &Theme, area: Rect) {
    let width = area.width.saturating_sub(4).min(72);
    let height = area.height.saturating_sub(2).min(24);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let lines = [
        "",
        "  NAVIGATION",
        "  Tab / Shift+Tab    Switch layers and files",
        "  Ctrl+↓ / click     Focus command · Tab returns to layers",
        "  ↑↓                 Move · mouse wheel scrolls",
        "  [ / ]              Previous / next layer from any pane",
        "  Home/End · g/G     First / last · PgUp/PgDn page",
        "",
        "  FILESYSTEM",
        "  Enter / Space      Fold folder · click folders to fold",
        "  ←/h · →/l          Parent / expand · z fold/expand all",
        "  / or Ctrl+f        Case-insensitive path regex",
        "  Enter · Esc        Apply filter / clear filter",
        "  a / m / d / u      Toggle added / modified / deleted / unchanged",
        "  c                  This layer / changes since first layer",
        "  s                  Sort Name ↑ / Name ↓ / Size ↑ / Size ↓",
        "  b                  Show/hide attributes",
        "  w                  Wrap / unwrap command",
        "",
        "  q / Ctrl+c         Quit · terminal state is restored",
        "  ? / Esc            Close help · toolbar controls are clickable",
        "",
    ];
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .block(panel(" Keyboard & mouse ", true, theme))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.text)),
        rect,
    );
}

pub fn clean(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.into();
    }
    let mut text = String::new();
    for character in value.chars() {
        if text.width() + unicode_width::UnicodeWidthChar::width(character).unwrap_or(0) >= width {
            break;
        }
        text.push(character);
    }
    if width > 0 {
        text.push('…');
    }
    text
}

pub fn bytes_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
