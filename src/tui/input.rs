//! Keyboard shortcuts and hit testing share the same state operations.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

use super::state::{Action, App, Focus};

pub fn handle(app: &mut App<'_>, event: Event) {
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => keyboard(app, key),
        Event::Mouse(mouse) => mouse_event(app, mouse),
        _ => {}
    }
}

fn keyboard(app: &mut App<'_>, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    if app.help {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) {
            app.help = false;
        }
        return;
    }
    if app.filter_edit {
        filter_key(app, key);
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Tab | KeyCode::BackTab => app.toggle_main_focus(),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.focus = Focus::Command;
        }
        KeyCode::Up => app.move_selection(-1),
        KeyCode::Down => app.move_selection(1),
        KeyCode::PageUp => app.move_selection(-page_size(app)),
        KeyCode::PageDown => app.move_selection(page_size(app)),
        KeyCode::Home | KeyCode::Char('g') => app.move_selection(isize::MIN),
        KeyCode::End | KeyCode::Char('G') => app.move_selection(isize::MAX),
        KeyCode::Char('[') => app.select_layer(app.layer.saturating_sub(1)),
        KeyCode::Char(']') => app.select_layer(app.layer + 1),
        KeyCode::Char('/') => {
            app.filter_edit = true;
            app.focus = Focus::Files;
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.filter_edit = true;
            app.focus = Focus::Files;
        }
        KeyCode::Esc => app.clear_filter(),
        KeyCode::Enter | KeyCode::Char(' ') if app.focus == Focus::Files => app.fold_selected(),
        KeyCode::Left | KeyCode::Char('h') if app.focus == Focus::Files => app.tree_left(),
        KeyCode::Right | KeyCode::Char('l') if app.focus == Focus::Files => app.tree_right(),
        KeyCode::Char('z') => app.fold_all(),
        KeyCode::Char('a') => app.action(Action::Category(0)),
        KeyCode::Char('m') => app.action(Action::Category(1)),
        KeyCode::Char('d') => app.action(Action::Category(2)),
        KeyCode::Char('u') => app.action(Action::Category(3)),
        KeyCode::Char('c') => app.action(Action::Compare),
        KeyCode::Char('s') => app.action(Action::Sort),
        KeyCode::Char('b') => app.action(Action::Attributes),
        KeyCode::Char('w') => {
            app.wrap = !app.wrap;
            app.command_scroll = 0;
        }
        KeyCode::Char('?') => app.help = true,
        _ => {}
    }
}

fn filter_key(app: &mut App<'_>, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.clear_filter(),
        KeyCode::Enter => app.filter_edit = false,
        KeyCode::Backspace => {
            app.filter.pop();
            app.update_filter();
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.filter.clear();
            app.update_filter();
        }
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.filter.push(character);
            app.update_filter();
        }
        _ => {}
    }
}

fn page_size(app: &App<'_>) -> isize {
    let height = match app.focus {
        Focus::Layers => app.areas.layers.height,
        Focus::Files => app.areas.files.height,
        Focus::Command => app.areas.command.height,
    };
    height.saturating_sub(2).max(1) as isize
}

fn mouse_event(app: &mut App<'_>, mouse: MouseEvent) {
    let point = Position::new(mouse.column, mouse.row);
    if app.help {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            app.help = false;
        }
        return;
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some((_, action)) = app
            .areas
            .buttons
            .iter()
            .find(|(rect, _)| rect.contains(point))
    {
        let action = *action;
        app.action(action);
        return;
    }

    let focus = if app.areas.layers.contains(point) {
        Some(Focus::Layers)
    } else if app.areas.files.contains(point) {
        Some(Focus::Files)
    } else if app.areas.command.contains(point) {
        Some(Focus::Command)
    } else {
        None
    };
    let Some(focus) = focus else {
        return;
    };
    app.focus = focus;

    match mouse.kind {
        MouseEventKind::ScrollUp => app.move_selection(-1),
        MouseEventKind::ScrollDown => app.move_selection(1),
        MouseEventKind::Down(MouseButton::Left) => match focus {
            Focus::Layers => {
                let row = usize::from(mouse.row.saturating_sub(app.areas.layers.y));
                app.select_layer(row + app.layers.offset());
            }
            Focus::Files => {
                let row =
                    usize::from(mouse.row.saturating_sub(app.areas.files.y)) + app.files.offset();
                if row < app.visible.len() {
                    app.files.select(Some(row));
                    app.fold_selected();
                }
            }
            Focus::Command => {}
        },
        _ => {}
    }
}
