use crate::app::{App, Mode, Pane};
use crate::fleet::Health;
use crate::layout;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tui_term::widget::PseudoTerminal;

pub const SIDEBAR_WIDTH: u16 = 30;

/// How much room the sidebar takes right now; zero when collapsed.
pub fn sidebar_width(app: &App) -> u16 {
    if app.sidebar { SIDEBAR_WIDTH } else { 0 }
}

const ORANGE: Color = Color::Rgb(255, 140, 66);
const KEYCOL: Color = Color::Rgb(95, 175, 255);
const DIM: Color = Color::Rgb(107, 114, 128);
const SELECT: Color = Color::Rgb(13, 92, 99);

/// Commands worth advertising in the sidebar, in display order.
const SUMMARY: &[(&str, &str)] = &[
    ("new-window", "new window"),
    ("rename-window", "rename window"),
    ("next-window", "next window"),
    ("split-vertical", "split vertical"),
    ("split-horizontal", "split horizontal"),
    ("focus-left", "focus left"),
    ("focus-right", "focus right"),
    ("next-cluster", "next cluster"),
    ("prev-cluster", "prev cluster"),
    ("close-pane", "close pane"),
    ("restart", "restart k9s"),
    ("toggle-sidebar", "hide this sidebar"),
    ("help", "help"),
    ("quit", "quit"),
];

/// Key hints built from whatever the config actually bound.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    let km = &app.keymap;
    let prefix = km.prefix.label();
    SUMMARY
        .iter()
        .filter_map(|(cmd, desc)| {
            let action = crate::keymap::parse_command(cmd)?;
            if let Some((key, _)) = km.root.iter().find(|(_, a)| *a == action) {
                return Some((key.label(), *desc));
            }
            let (key, _) = km.prefixed.iter().find(|(_, a)| *a == action)?;
            Some((format!("{prefix} {}", key.label()), *desc))
        })
        .collect()
}

/// Where each pane of the current window is drawn.
pub fn pane_areas(app: &App, area: Rect) -> Vec<Rect> {
    let rail = sidebar_width(app);
    let body = Rect::new(
        rail.min(area.width),
        area.y,
        area.width.saturating_sub(rail),
        area.height,
    );
    layout::areas(body, app.panes().len(), app.split())
}

/// Rows and columns available to each child, i.e. inside the borders.
pub fn pane_inner_sizes(app: &App, area: Rect) -> Vec<(u16, u16)> {
    pane_areas(app, area)
        .into_iter()
        .map(|r| {
            (
                r.height.saturating_sub(2).max(1),
                r.width.saturating_sub(2).max(1),
            )
        })
        .collect()
}

/// What a sidebar row refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Window(usize),
    Pane { window: usize, pane: usize },
}

/// The sidebar contents as (screen row, target) pairs. Panes are listed under
/// the current window only, the way tmux shows the active window's layout.
pub fn sidebar_rows(app: &App) -> Vec<(u16, Row)> {
    let mut y = 2; // title, blank
    let mut out = Vec::new();
    for (w, win) in app.windows.iter().enumerate() {
        out.push((y, Row::Window(w)));
        y += 1;
        if w == app.current {
            for p in 0..win.panes.len() {
                out.push((y, Row::Pane { window: w, pane: p }));
                y += 1;
            }
        }
    }
    out
}

pub fn sidebar_hit(app: &App, x: u16, y: u16) -> Option<Row> {
    if !app.sidebar || x >= SIDEBAR_WIDTH {
        return None;
    }
    sidebar_rows(app)
        .into_iter()
        .find(|(row, _)| *row == y)
        .map(|(_, target)| target)
}

pub fn pane_hit(app: &App, area: Rect, x: u16, y: u16) -> Option<usize> {
    if x < sidebar_width(app) {
        return None;
    }
    pane_areas(app, area)
        .into_iter()
        .position(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    if app.sidebar {
        let [rail, _] =
            Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)]).areas(area);
        draw_sidebar(f, app, rail);
    }
    for (i, r) in pane_areas(app, area).into_iter().enumerate() {
        draw_pane(f, app, i, r);
    }
    if app.help {
        draw_help(f, app, area);
    }
}

fn health_dot(h: Health) -> Span<'static> {
    let (sym, color) = match h {
        Health::Ok => ("●", Color::Green),
        Health::Degraded => ("◐", Color::Yellow),
        Health::Unreachable => ("○", Color::Red),
        Health::Unknown => ("●", DIM),
    };
    Span::styled(sym, Style::new().fg(color))
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(Style::new().fg(DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let n = app.windows.len();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "k9mux",
                Style::new().fg(ORANGE).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{n} window{}", if n == 1 { "" } else { "s" }),
                Style::new().fg(DIM),
            ),
        ]),
        Line::raw(""),
    ];

    for (w, win) in app.windows.iter().enumerate() {
        let current = w == app.current;
        let renaming = current && matches!(app.mode, Mode::Rename(_));
        let name = match &app.mode {
            Mode::Rename(text) if current => format!("{text}▏"),
            _ => win.name.clone(),
        };
        let style = if renaming {
            Style::new().bg(ORANGE).fg(Color::Black)
        } else if current {
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(DIM)
        };
        lines.push(
            Line::from(format!(
                "{}:{name}{}",
                w + 1,
                if current && !renaming { "*" } else { "" }
            ))
            .style(style),
        );

        if !current {
            continue;
        }
        for (p, pane) in win.panes.iter().enumerate() {
            let health = app
                .fleet
                .find(&pane.instance)
                .map(|i| i.health)
                .unwrap_or(Health::Unknown);
            let style = if p == win.focus {
                Style::new().bg(SELECT).fg(Color::White)
            } else {
                Style::new()
            };
            lines.push(
                Line::from(vec![
                    Span::raw("  "),
                    health_dot(health),
                    Span::raw(" "),
                    Span::raw(pane.instance.clone()),
                ])
                .style(style),
            );
        }
    }

    let hint_list = hints(app);
    let hint_height = (hint_list.len() as u16 + 1).min(inner.height.saturating_sub(1));
    let list_height = inner.height - hint_height;
    let [list, hints_area] =
        Layout::vertical([Constraint::Length(list_height), Constraint::Min(0)]).areas(inner);
    f.render_widget(Paragraph::new(lines), list);

    let mut hint_lines = vec![Line::styled(
        if app.armed {
            format!("{} …", app.keymap.prefix.label())
        } else {
            String::new()
        },
        Style::new().fg(ORANGE),
    )];
    let width = hint_list.iter().map(|(k, _)| k.len()).max().unwrap_or(0) + 1;
    hint_lines.extend(hint_list.iter().map(|(k, d)| {
        Line::from(vec![
            Span::styled(format!("{k:<width$}"), Style::new().fg(KEYCOL)),
            Span::styled(*d, Style::new().fg(DIM)),
        ])
    }));
    f.render_widget(Paragraph::new(hint_lines), hints_area);
}

fn draw_pane(f: &mut Frame, app: &App, idx: usize, area: Rect) {
    if area.width < 3 || area.height < 3 {
        return;
    }
    let pane: &Pane = &app.panes()[idx];
    let focused = idx == app.focus();
    let border = if focused { Color::Cyan } else { DIM };

    let block = Block::bordered()
        .border_style(Style::new().fg(border))
        .title(Line::from(vec![
            Span::styled(pane.instance.clone(), Style::new().fg(ORANGE)),
            Span::styled(
                format!(" [{}]", idx + 1),
                Style::new().fg(if focused { Color::Cyan } else { DIM }),
            ),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if let Some(err) = &pane.error {
        f.render_widget(
            Paragraph::new(vec![
                Line::raw(""),
                Line::styled(format!(" {err}"), Style::new().fg(Color::Red)),
                Line::raw(""),
                Line::styled(
                    format!(
                        " is k9s on your PATH?  {} r to retry",
                        app.keymap.prefix.label()
                    ),
                    Style::new().fg(DIM),
                ),
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }

    let Some(session) = pane.session.as_ref() else {
        return;
    };
    let parser = session.parser().lock().unwrap();
    let screen = parser.screen();
    f.render_widget(PseudoTerminal::new(screen), inner);

    // A clean exit closes the pane, so anything still showing here failed.
    if pane.exited() {
        let note = Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1);
        let code = pane
            .exit_code()
            .map(|c| format!(" (exit {c})"))
            .unwrap_or_default();
        f.render_widget(Clear, note);
        f.render_widget(
            Paragraph::new(Line::styled(
                format!(
                    "  k9s exited{code} — {} r to restart  ",
                    app.keymap.prefix.label()
                ),
                Style::new().fg(Color::Black).bg(Color::Yellow),
            )),
            note,
        );
        return;
    }

    // Let the real cursor sit where the focused k9s put it.
    if focused && app.mode == Mode::Normal && !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        if row < inner.height && col < inner.width {
            f.set_cursor_position((inner.x + col, inner.y + row));
        }
    }
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let prefix = app.keymap.prefix.label();
    let hint_list = hints(app);
    let width = hint_list
        .iter()
        .map(|(k, _)| k.len())
        .max()
        .unwrap_or(0)
        .max(prefix.len() * 2 + 1)
        + 2;
    let lines: Vec<Line> = std::iter::once(Line::styled(
        format!("k9mux — {prefix} is the prefix, everything else goes to k9s"),
        Style::new().fg(ORANGE),
    ))
    .chain(std::iter::once(Line::raw("")))
    .chain(hint_list.iter().map(|(k, d)| {
        Line::from(vec![
            Span::styled(format!("  {k:<width$}"), Style::new().fg(KEYCOL)),
            Span::styled(*d, Style::new().fg(Color::White)),
        ])
    }))
    .chain(std::iter::once(Line::from(vec![
        Span::styled(
            format!("  {:<width$}", format!("{prefix} {prefix}")),
            Style::new().fg(KEYCOL),
        ),
        Span::styled(
            "send a literal prefix to k9s",
            Style::new().fg(Color::White),
        ),
    ])))
    .chain(std::iter::once(Line::raw("")))
    .chain(std::iter::once(Line::styled(
        "  click a window or pane in the sidebar to go to it",
        Style::new().fg(DIM),
    )))
    .chain(std::iter::once(Line::styled(
        format!(
            "  {} e hides the sidebar and gives k9s the full width",
            app.keymap.prefix.label()
        ),
        Style::new().fg(DIM),
    )))
    .chain(std::iter::once(Line::styled(
        format!(
            "  configure in {}",
            crate::config::Config::path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config/k9mux/config".into())
        ),
        Style::new().fg(DIM),
    )))
    .collect();

    let h = (lines.len() as u16 + 2).min(area.height);
    let w = 66.min(area.width);
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(w)) / 2,
        area.y + (area.height.saturating_sub(h)) / 2,
        w,
        h,
    );
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::new().fg(Color::Cyan))
                .title("help"),
        ),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Spawner;
    use crate::fleet::{Fleet, Grouping};
    use crate::keys::Action;
    use crate::layout::Split;
    use crate::session::Launch;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::time::{Duration, Instant};

    fn fleet() -> Fleet {
        Fleet::from_contexts(
            &["use1-a", "use1-b", "euc1-a"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            &Grouping {
                clusters: vec![
                    ("prod-us".into(), vec!["use1-a".into(), "use1-b".into()]),
                    ("prod-eu".into(), vec!["euc1-a".into()]),
                ],
            },
        )
    }

    fn stub() -> Spawner {
        Box::new(|ctx: &str| {
            Launch::new("sh")
                .arg("-c")
                .arg("printf 'PANE OF %s\\n' \"$K9MUX_CTX\"; sleep 30")
                .env("K9MUX_CTX", ctx)
        })
    }

    fn app() -> App {
        App::new(fleet(), stub())
    }

    fn screen_of(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn screen(app: &App) -> String {
        screen_of(app, 140, 40)
    }

    fn sidebar(app: &App) -> String {
        screen(app)
            .lines()
            .map(|l| l.chars().take(SIDEBAR_WIDTH as usize).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_screen(app: &App, f: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = screen(app);
            if f(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out; screen was:\n{s}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_pane_shows_what_the_child_printed() {
        let a = app();
        wait_screen(&a, |s| s.contains("PANE OF use1-a"));
    }

    #[test]
    fn each_pane_shows_its_own_child() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        wait_screen(&a, |s| {
            s.contains("PANE OF use1-a") && s.contains("PANE OF use1-b")
        });
    }

    #[test]
    fn the_sidebar_lists_windows_by_name() {
        let mut a = app();
        let s = sidebar(&a);
        assert!(s.contains("1:k9mux*"), "current window marked:\n{s}");
        a.apply(Action::NewWindow);
        let s = sidebar(&a);
        assert!(s.contains("1:k9mux"), "{s}");
        assert!(s.contains("2:k9mux*"), "{s}");
    }

    #[test]
    fn the_current_windows_panes_are_listed_under_it() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let s = sidebar(&a);
        assert!(s.contains("use1-a"), "{s}");
        assert!(s.contains("use1-b"), "{s}");
    }

    #[test]
    fn only_the_current_window_shows_its_panes() {
        let mut a = app();
        a.apply(Action::NewWindow); // window 2 on use1-b
        let s = sidebar(&a);
        assert!(s.contains("use1-b"), "current window's pane:\n{s}");
        assert!(!s.contains("use1-a"), "other window's panes hidden:\n{s}");
    }

    #[test]
    fn a_rename_in_progress_is_shown_in_the_sidebar() {
        let mut a = app();
        a.apply(Action::RenameWindow);
        while !matches!(&a.mode, Mode::Rename(t) if t.is_empty()) {
            a.rename_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Backspace,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        for c in "prod".chars() {
            a.rename_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        let s = sidebar(&a);
        assert!(s.contains("1:prod"), "the name being typed:\n{s}");
    }

    #[test]
    fn the_sidebar_shows_the_window_bindings() {
        let s = sidebar(&app());
        assert!(s.contains("M-space c"), "{s}");
        assert!(s.contains("new window"), "{s}");
        assert!(s.contains("rename window"), "{s}");
        assert!(
            s.contains("M-h"),
            "root bindings shown without a prefix:\n{s}"
        );
    }

    #[test]
    fn the_hints_follow_whatever_the_config_bound() {
        let km = crate::config::Config::parse(
            "set prefix C-b\nbind s split-vertical\nunbind %\nunbind -n M-h",
        )
        .unwrap()
        .keymap;
        let a = App::with_keymap(fleet(), stub(), km);
        let s = sidebar(&a);
        assert!(s.contains("C-b s"), "rebound key not shown:\n{s}");
        assert!(!s.contains("M-space"), "old prefix still advertised:\n{s}");
        assert!(!s.contains("M-h"), "unbound key still advertised:\n{s}");
    }

    #[test]
    fn the_focused_pane_is_visibly_distinct() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        term.draw(|f| draw(f, &a)).unwrap();
        let buf = term.backend().buffer().clone();
        let border = buf.area.height - 1;
        let unfocused = buf[(SIDEBAR_WIDTH + 5, border)].style().fg;
        let focused = buf[(buf.area.width - 5, border)].style().fg;
        assert_ne!(focused, unfocused);
        assert_eq!(focused, Some(Color::Cyan));
    }

    #[test]
    fn a_pane_that_could_not_start_says_why() {
        let a = App::new(
            fleet(),
            Box::new(|_: &str| Launch::new("k9mux-no-such-binary-xyz")),
        );
        let s = screen(&a);
        assert!(s.contains("is k9s on your PATH"), "{s}");
    }

    #[test]
    fn a_pane_whose_k9s_crashed_shows_the_exit_code() {
        let a = App::new(
            fleet(),
            Box::new(|_: &str| Launch::new("sh").arg("-c").arg("exit 3")),
        );
        wait_screen(&a, |s| s.contains("k9s exited (exit 3)"));
    }

    #[test]
    fn help_can_be_shown_over_the_panes() {
        let mut a = app();
        assert!(!screen(&a).contains("literal prefix"));
        a.apply(Action::ToggleHelp);
        let s = screen(&a);
        assert!(s.contains("literal prefix"), "{s}");
        assert!(s.contains("M-space is the prefix"), "{s}");
    }

    #[test]
    fn inner_sizes_exclude_the_borders() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let area = Rect::new(0, 0, 140, 40);
        let panes = pane_areas(&a, area);
        let sizes = pane_inner_sizes(&a, area);
        assert_eq!(sizes.len(), 2);
        for (r, (rows, cols)) in panes.iter().zip(&sizes) {
            assert_eq!(*rows, r.height - 2);
            assert_eq!(*cols, r.width - 2);
        }
    }

    #[test]
    fn the_child_is_resized_to_the_pane_it_is_drawn_in() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let area = Rect::new(0, 0, 140, 40);
        let sizes = pane_inner_sizes(&a, area);
        a.resize_panes(&sizes);
        for (i, expected) in sizes.iter().enumerate() {
            assert_eq!(a.panes()[i].session.as_ref().unwrap().size(), *expected);
        }
    }

    #[test]
    fn sidebar_rows_line_up_with_what_is_drawn() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::NewWindow);
        let s = screen(&a);
        for (y, target) in sidebar_rows(&a) {
            let line = s.lines().nth(y as usize).unwrap();
            let expected = match target {
                Row::Window(w) => a.windows[w].name.clone(),
                Row::Pane { window, pane } => a.windows[window].panes[pane].instance.clone(),
            };
            assert!(line.contains(&expected), "row {y} should show {expected:?}");
            assert_eq!(sidebar_hit(&a, 4, y), Some(target));
        }
    }

    #[test]
    fn clicks_outside_the_sidebar_hit_panes_instead() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let area = Rect::new(0, 0, 140, 40);
        assert_eq!(pane_hit(&a, area, SIDEBAR_WIDTH + 2, 10), Some(0));
        assert_eq!(pane_hit(&a, area, 138, 10), Some(1));
        assert_eq!(pane_hit(&a, area, 4, 10), None);
        assert_eq!(sidebar_hit(&a, 40, 3), None);
    }

    #[test]
    fn collapsing_the_sidebar_gives_the_space_to_the_panes() {
        let mut a = app();
        let area = Rect::new(0, 0, 140, 40);
        let before = pane_areas(&a, area)[0];
        assert_eq!(before.x, SIDEBAR_WIDTH);

        a.apply(Action::ToggleSidebar);
        let after = pane_areas(&a, area)[0];
        assert_eq!(after.x, 0, "panes should start at the left edge");
        assert_eq!(after.width, before.width + SIDEBAR_WIDTH);
        assert_eq!(pane_inner_sizes(&a, area)[0].1, after.width - 2);
    }

    #[test]
    fn a_collapsed_sidebar_is_not_drawn() {
        let mut a = app();
        assert!(sidebar(&a).contains("1:k9mux"));
        a.apply(Action::ToggleSidebar);
        let s = screen(&a);
        assert!(!s.contains("1:k9mux"), "sidebar still drawn:\n{s}");
        assert!(!s.contains("new window"), "hints still drawn:\n{s}");
    }

    #[test]
    fn the_panes_still_render_with_the_sidebar_hidden() {
        let mut a = app();
        a.apply(Action::ToggleSidebar);
        wait_screen(&a, |s| s.contains("PANE OF use1-a"));
    }

    #[test]
    fn clicks_land_correctly_with_the_sidebar_hidden() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::ToggleSidebar);
        let area = Rect::new(0, 0, 140, 40);
        assert_eq!(sidebar_hit(&a, 4, 3), None, "nothing to click there now");
        assert_eq!(
            pane_hit(&a, area, 4, 10),
            Some(0),
            "the left edge is a pane"
        );
        assert_eq!(pane_hit(&a, area, 138, 10), Some(1));
    }

    #[test]
    fn the_sidebar_advertises_how_to_hide_it() {
        let s = sidebar(&app());
        assert!(s.contains("M-space e"), "{s}");
        assert!(s.contains("hide this sidebar"), "{s}");
    }

    #[test]
    fn the_help_says_how_to_bring_it_back() {
        let mut a = app();
        a.apply(Action::ToggleSidebar);
        a.apply(Action::ToggleHelp);
        let s = screen(&a);
        assert!(s.contains("hides the sidebar"), "{s}");
    }

    #[test]
    fn it_survives_a_tiny_terminal() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Split(Split::Horizontal));
        a.apply(Action::NewWindow);
        for (w, h) in [
            (200u16, 60u16),
            (80, 24),
            (40, 12),
            (20, 6),
            (10, 3),
            (4, 2),
        ] {
            let _ = screen_of(&a, w, h);
        }
    }
}
