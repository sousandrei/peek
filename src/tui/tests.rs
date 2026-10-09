use std::io::Cursor;

use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
};
use serde_json::json;

use super::{
    input,
    state::{Action, App, Focus},
    tree::{Category, SortOrder},
    view::{self, Theme},
};
use crate::oci::{self, Analysis};

fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, Cursor::new(bytes))
            .unwrap();
    }
    tar.into_inner().unwrap()
}

fn fixture() -> Analysis {
    let base = tar(&[
        ("etc/app.conf", b"old"),
        ("etc/keep", b"same"),
        ("var/cache.tmp", b"cache"),
        ("usr/猫.txt", b"unicode"),
    ]);
    let update = tar(&[
        ("etc/app.conf", b"updated"),
        ("var/.wh.cache.tmp", b""),
        ("opt/new.txt", b"new"),
    ]);
    let config = serde_json::to_vec(&json!({"os":"linux","architecture":"amd64","history":[{"created_by":"COPY root filesystem"},{"created_by":"RUN /bin/sh -c apt-get update; apt-get install -y redis; rm -rf /var/lib/apt/lists/*"}]})).unwrap();
    let manifest = br#"[{"Config":"config.json","Layers":["base.tar","update.tar"]}]"#;
    let archive = tar(&[
        ("config.json", &config),
        ("manifest.json", manifest),
        ("base.tar", &base),
        ("update.tar", &update),
    ]);
    oci::analyze(oci::archive_for_test(&archive).unwrap()).unwrap()
}

fn key(app: &mut App<'_>, code: KeyCode) {
    input::handle(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

#[test]
fn tree_preserves_removed_entries_and_infers_parent_directories() {
    let analysis = fixture();
    let app = App::new(&analysis);
    assert_eq!(app.tree.nodes["/etc/app.conf"].category, Category::Modified);
    assert_eq!(app.tree.nodes["/var/cache.tmp"].category, Category::Removed);
    assert!(app.tree.nodes["/var/cache.tmp"].after.is_none());
    assert_eq!(app.tree.nodes["/opt/new.txt"].category, Category::Added);
    assert!(app.tree.nodes["/opt"].directory);
    assert!(app.visible.contains(&"/usr/猫.txt".into()));
}

#[test]
fn regex_filter_retains_ancestors_and_reports_invalid_expressions() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    app.filter = "app\\.conf$".into();
    app.update_filter();
    assert_eq!(app.visible, ["/", "/etc", "/etc/app.conf"]);
    app.filter = "[".into();
    app.update_filter();
    assert!(app.filter_error.is_some());
    assert!(app.visible.is_empty());
    app.clear_filter();
    assert!(app.filter_error.is_none());
    assert!(app.visible.contains(&"/usr/猫.txt".into()));
}

#[test]
fn filtering_and_collapse_preserve_a_valid_selection() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    app.focus = Focus::Files;
    let index = app.visible.iter().position(|path| path == "/etc").unwrap();
    app.files.select(Some(index));
    app.fold_selected();
    assert!(!app.visible.contains(&"/etc/app.conf".into()));
    assert_eq!(app.selected_path(), Some("/etc"));
    app.fold_selected();
    assert!(app.visible.contains(&"/etc/app.conf".into()));
    app.action(Action::Category(Category::Unchanged.index()));
    assert!(!app.visible.contains(&"/etc/keep".into()));
    assert!(
        app.files
            .selected()
            .is_some_and(|index| index < app.visible.len())
    );
}

#[test]
fn keyboard_navigation_changes_layer_focus_and_compare_mode() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.layer, 0);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Files);
    key(&mut app, KeyCode::Char(']'));
    assert_eq!(app.layer, 1);
    key(&mut app, KeyCode::Char('c'));
    assert!(app.aggregate);
    key(&mut app, KeyCode::Char('/'));
    assert!(app.filter_edit);
    key(&mut app, KeyCode::Char('q'));
    assert!(!app.quit);
    assert_eq!(app.filter, "q");
    key(&mut app, KeyCode::Esc);
    assert!(app.filter.is_empty());
    key(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

fn render(app: &mut App<'_>, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| view::draw(frame, app, &Theme::detect()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if let Ok(directory) = std::env::var("PEEK_TUI_SNAPSHOTS") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            format!(
                "{directory}/{width}x{height}-{:?}-{}.txt",
                app.focus, app.help
            ),
            &text,
        )
        .unwrap();
        let cells: Vec<_> = (0..height).flat_map(|y| (0..width).map(move |x| {
            let cell = &buffer[(x,y)];
            json!({"x":x,"y":y,"text":cell.symbol(),"fg":format!("{:?}",cell.fg),"bg":format!("{:?}",cell.bg),"bold":cell.modifier.contains(ratatui::style::Modifier::BOLD)})
        })).collect();
        std::fs::write(
            format!(
                "{directory}/{width}x{height}-{:?}-{}.json",
                app.focus, app.help
            ),
            serde_json::to_vec(&cells).unwrap(),
        )
        .unwrap();
    }
    text
}

#[test]
fn layouts_render_at_wide_narrow_and_tiny_sizes() {
    let analysis = fixture();
    for (width, height) in [(120, 36), (90, 24), (80, 24), (40, 12), (20, 8)] {
        let mut app = App::new(&analysis);
        let text = render(&mut app, width, height);
        assert!(text.contains("PEEK") || text.contains("Resize"));
        if width >= 90 {
            assert!(text.contains("Selected file"));
            assert!(text.contains("Command"));
        }
        app.focus = Focus::Files;
        render(&mut app, width, height);
        app.help = true;
        render(&mut app, width, height);
    }
}

#[test]
fn mouse_selects_layers_and_folds_folders() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    render(&mut app, 120, 36);
    let layer_point = app.areas.layers;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: layer_point.x,
            row: layer_point.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(app.layer, 0);
    app.focus = Focus::Files;
    render(&mut app, 120, 36);
    let row = app.visible.iter().position(|path| path == "/etc").unwrap();
    let files = app.areas.files;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: files.x + 5,
            row: files.y + row as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(app.selected_path(), Some("/etc"));
    assert!(app.collapsed.contains("/etc"));
}

#[test]
fn control_characters_are_displayed_without_terminal_side_effects() {
    assert_eq!(view::clean("/猫\x1b[2J\t\r"), "/猫\\u{1b}[2J\\t\\r");
}

#[test]
fn controls_live_in_footer_without_top_tabs_or_vim_vertical_keys() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    let text = render(&mut app, 120, 36);
    let lines: Vec<_> = text.lines().collect();
    assert!(lines[0].contains("PEEK"));
    assert!(lines[1].contains("Layers  2/2"));
    assert!(!lines[1].contains("Added"));
    assert!(lines[34].contains("Added"));
    assert!(lines[34].contains("This layer"));
    assert!(!text.contains("jk move"));
    assert!(app.areas.buttons.iter().all(|(rect, _)| rect.y >= 34));

    let layer = app.layer;
    key(&mut app, KeyCode::Char('k'));
    key(&mut app, KeyCode::Char('j'));
    assert_eq!(app.layer, layer);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.layer, layer - 1);

    let point = app
        .areas
        .buttons
        .iter()
        .find(|(_, action)| matches!(action, Action::Category(0)))
        .unwrap()
        .0;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: point.x,
            row: point.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(!app.categories[0]);
}

#[test]
fn footer_wraps_all_controls_and_only_shows_active_filter_status() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    app.files
        .select(app.visible.iter().position(|path| path == "/etc/app.conf"));

    for width in [120, 90, 80, 40] {
        let text = render(&mut app, width, 24);
        assert_eq!(app.areas.buttons.len(), 8);
        let first_row = app.areas.buttons[0].0.y;
        let footer = text
            .lines()
            .skip(usize::from(first_row))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!footer.contains("/etc/app.conf"));
        assert!(
            app.areas
                .buttons
                .iter()
                .all(|(rect, _)| rect.right() < width)
        );
        if width == 120 {
            assert_eq!(first_row, 22);
            assert!(
                app.areas
                    .buttons
                    .iter()
                    .filter(|(_, action)| !matches!(action, Action::Help))
                    .all(|(rect, _)| rect.y == first_row)
            );
            assert!(app.areas.buttons[6].0.right() >= width - 2);
        } else {
            assert!(app.areas.buttons[6].0.y > first_row);
        }
    }

    app.filter_edit = true;
    app.filter = "app".into();
    let text = render(&mut app, 120, 24);
    assert!(text.lines().nth(21).unwrap().contains("/ app▏"));
    app.clear_filter();
    render(&mut app, 120, 24);
    assert_eq!(app.areas.buttons[0].0.y, 22);
}

#[test]
fn mouse_wheel_moves_one_row_in_each_direction() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    render(&mut app, 120, 36);
    app.files.select(Some(3));
    app.command_limit = 10;
    app.command_scroll = 3;

    for point in [app.areas.files, app.areas.command, app.areas.layers] {
        for kind in [MouseEventKind::ScrollDown, MouseEventKind::ScrollUp] {
            input::handle(
                &mut app,
                Event::Mouse(MouseEvent {
                    kind,
                    column: point.x,
                    row: point.y,
                    modifiers: KeyModifiers::NONE,
                }),
            );
            let down = kind == MouseEventKind::ScrollDown;
            match app.focus {
                Focus::Files => assert_eq!(app.files.selected(), Some(if down { 4 } else { 3 })),
                Focus::Command => assert_eq!(app.command_scroll, if down { 4 } else { 3 }),
                Focus::Layers => assert_eq!(app.layer, if down { 1 } else { 0 }),
            }
        }
    }
}

#[test]
fn footer_keeps_compact_groups_and_one_clickable_help_next_to_quit() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    let text = render(&mut app, 120, 24);
    let controls = &app.areas.buttons;
    assert!(matches!(controls[0].1, Action::Compare));
    assert_eq!(controls[0].0.x, 1);
    for pair in controls[..3].windows(2).chain(controls[3..7].windows(2)) {
        assert_eq!(pair[1].0.x, pair[0].0.right() + 2);
    }
    assert!(controls[3].0.x > controls[2].0.right() + 3);
    assert_eq!(controls[6].0.right(), 119);

    let footer = text.lines().skip(22).collect::<Vec<_>>().join("\n");
    assert_eq!(footer.matches("help").count(), 1);
    assert!(text.lines().last().unwrap().contains("? help  q quit"));
    let help = controls
        .iter()
        .find(|(_, action)| matches!(action, Action::Help))
        .unwrap()
        .0;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: help.x,
            row: help.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(app.help);
}

#[test]
fn loading_screen_shows_stage_and_cancellation_without_explorer_controls() {
    for (width, height) in [(120, 36), (40, 12), (20, 8)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for cancelling in [false, true] {
            terminal
                .draw(|frame| {
                    view::draw_loading(
                        frame,
                        &Theme::detect(),
                        "ubuntu:latest",
                        "Exporting image archive",
                        std::time::Duration::from_millis(1200),
                        cancelling,
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text = (0..height)
                .flat_map(|y| (0..width).map(move |x| buffer[(x, y)].symbol()))
                .collect::<String>();
            assert!(text.contains("PEEK"));
            assert!(text.contains("ubuntu:latest"));
            assert!(!text.contains("Added"));
            if width >= 40 {
                assert!(text.contains(if cancelling {
                    "Cancelling image loading"
                } else {
                    "Exporting image archive"
                }));
                assert!(text.contains("1.2s elapsed"));
            }
        }
    }
}

#[test]
fn tab_cycles_main_panes_and_command_has_separate_keyboard_and_mouse_focus() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    for code in [KeyCode::Tab, KeyCode::BackTab] {
        app.focus = Focus::Layers;
        key(&mut app, code);
        assert_eq!(app.focus, Focus::Files);
        key(&mut app, code);
        assert_eq!(app.focus, Focus::Layers);
    }

    for focus in [Focus::Layers, Focus::Files] {
        app.focus = focus;
        input::handle(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
        );
        assert_eq!(app.focus, Focus::Command);
        app.command_limit = 10;
        app.command_scroll = 0;
        key(&mut app, KeyCode::Down);
        assert_eq!(app.command_scroll, 1);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.focus, Focus::Layers);
    }

    render(&mut app, 120, 36);
    let point = app.areas.command;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: point.x,
            row: point.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(app.focus, Focus::Command);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(app.focus, Focus::Layers);

    input::handle(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
    );
    let text = render(&mut app, 80, 24);
    assert!(text.contains("Ctrl+↓ to focus"));
    assert!(!app.areas.command.is_empty());
}

#[test]
fn sorting_cycles_all_orders_and_preserves_selected_file() {
    let analysis = fixture();
    let mut app = App::new(&analysis);
    app.files
        .select(app.visible.iter().position(|path| path == "/etc/app.conf"));

    for (order, paths) in [
        (SortOrder::NameAscending, ["/etc/app.conf", "/etc/keep"]),
        (SortOrder::NameDescending, ["/etc/keep", "/etc/app.conf"]),
        (SortOrder::SizeAscending, ["/etc/keep", "/etc/app.conf"]),
        (SortOrder::SizeDescending, ["/etc/app.conf", "/etc/keep"]),
    ] {
        assert_eq!(app.sort, order);
        let children: Vec<_> = app
            .visible
            .iter()
            .filter(|path| path.starts_with("/etc/"))
            .map(String::as_str)
            .collect();
        assert_eq!(children, paths);
        assert_eq!(app.selected_path(), Some("/etc/app.conf"));
        assert!(render(&mut app, 120, 36).contains(order.label()));
        key(&mut app, KeyCode::Char('s'));
    }
    assert_eq!(app.sort, SortOrder::NameAscending);

    let point = app
        .areas
        .buttons
        .iter()
        .find(|(_, action)| matches!(action, Action::Sort))
        .unwrap()
        .0;
    input::handle(
        &mut app,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: point.x,
            row: point.y,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert_eq!(app.sort, SortOrder::NameDescending);
}

#[test]
fn footer_shortcut_keys_are_bold_and_descriptions_are_not() {
    use ratatui::style::Modifier;

    let analysis = fixture();
    let mut app = App::new(&analysis);
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| view::draw(frame, &mut app, &Theme::detect()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    for (rect, action) in &app.areas.buttons {
        let offset = if matches!(action, Action::Category(_)) {
            2
        } else {
            0
        };
        let key = (rect.x + offset, rect.y);
        assert!(buffer[key].modifier.contains(Modifier::BOLD));
        assert!(!buffer[(key.0 + 2, key.1)].modifier.contains(Modifier::BOLD));
    }

    let footer_y = 35;
    for x in 1..=3 {
        assert!(buffer[(x, footer_y)].modifier.contains(Modifier::BOLD));
    }
    assert!(!buffer[(5, footer_y)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(112, footer_y)].modifier.contains(Modifier::BOLD));
    assert!(!buffer[(114, footer_y)].modifier.contains(Modifier::BOLD));
}

#[test]
fn loading_shortcut_keys_are_bold_and_quit_label_is_not() {
    use ratatui::style::Modifier;

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| {
            view::draw_loading(
                frame,
                &Theme::detect(),
                "ubuntu",
                "Loading image",
                std::time::Duration::ZERO,
                false,
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    // The 12-cell hint is centered on the last loading-screen row.
    for x in [14, 18, 19, 20] {
        assert!(buffer[(x, 9)].modifier.contains(Modifier::BOLD));
    }
    for x in [16, 22, 23, 24, 25] {
        assert!(!buffer[(x, 9)].modifier.contains(Modifier::BOLD));
    }
}
