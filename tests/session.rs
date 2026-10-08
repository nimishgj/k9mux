//! End-to-end: real key events, into real child processes on real ptys, out to
//! a rendered terminal buffer. The child is a stub rather than k9s so the tests
//! need no cluster, but every layer in between is the real one.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use k9mux::app::{App, Spawner};
use k9mux::config::Config;
use k9mux::fleet::Fleet;
use k9mux::keys::Outcome;
use k9mux::session::Launch;
use k9mux::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};

fn fleet() -> Fleet {
    let (contexts, grouping) = (
        vec![
            "use1-a".to_string(),
            "use1-b".to_string(),
            "euc1-a".to_string(),
        ],
        k9mux::fleet::Grouping {
            clusters: vec![
                ("prod-us".into(), vec!["use1-a".into(), "use1-b".into()]),
                ("prod-eu".into(), vec!["euc1-a".into()]),
            ],
        },
    );
    Fleet::from_contexts(&contexts, &grouping)
}

/// A stand-in for k9s: announces its context, echoes whatever is typed, and
/// quits on `:q` just as k9s does.
fn stub() -> Spawner {
    Box::new(|ctx: &str| {
        Launch::new("sh")
            .arg("-c")
            .arg(
                "printf 'k9s@%s ready\\n' \"$K9MUX_CTX\"; \
                 while read line; do \
                   [ \"$line\" = ':q' ] && exit 0; \
                   printf '%s typed:%s\\n' \"$K9MUX_CTX\" \"$line\"; \
                 done",
            )
            .env("K9MUX_CTX", ctx)
    })
}

fn session() -> App {
    App::new(fleet(), stub())
}

fn screen(app: &App) -> String {
    let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
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

fn wait_screen(app: &App, f: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let s = screen(app);
        if f(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "timed out; screen was:\n{s}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Drives the app exactly as the event loop does, prefix state and all.
fn press(app: &mut App, key: KeyEvent) {
    if matches!(app.mode, k9mux::app::Mode::Rename(_)) {
        app.rename_key(key);
        return;
    }
    match app.keymap.resolve(key, app.armed) {
        Outcome::Armed => app.armed = true,
        Outcome::Do(a) => {
            app.armed = false;
            app.apply(a);
        }
        Outcome::Forward(b) => {
            app.armed = false;
            app.forward(&b);
        }
        Outcome::Ignore => app.armed = false,
    }
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        let key = match c {
            '\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            '\t' => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        };
        press(app, key);
    }
}

/// The prefix, then a command key.
fn prefix(app: &mut App, c: char) {
    press(app, KeyEvent::new(KeyCode::Char(' '), KeyModifiers::ALT));
    // A terminal sends Tab, not a literal tab character.
    let code = if c == '\t' {
        KeyCode::Tab
    } else {
        KeyCode::Char(c)
    };
    press(app, KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn a_pane_runs_a_real_child_process() {
    let app = session();
    wait_screen(&app, |s| s.contains("k9s@use1-a ready"));
}

#[test]
fn two_clusters_run_side_by_side() {
    let mut app = session();
    prefix(&mut app, '%');
    assert_eq!(app.panes().len(), 2);
    wait_screen(&app, |s| {
        s.contains("k9s@use1-a ready") && s.contains("k9s@use1-b ready")
    });
}

#[test]
fn typing_goes_to_the_focused_child_and_nowhere_else() {
    let mut app = session();
    prefix(&mut app, '%');
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));

    typ(&mut app, "hello\n");
    wait_screen(&app, |s| s.contains("use1-b typed:hello"));
    assert!(
        !screen(&app).contains("use1-a typed:"),
        "input leaked into the unfocused pane"
    );
}

#[test]
fn keys_that_k9mux_used_to_own_now_reach_k9s() {
    let mut app = session();
    // ':', '|', 'd', '0' and ctrl-a are all k9s keys; none may be intercepted.
    typ(&mut app, ":pods|d0%\n");
    wait_screen(&app, |s| s.contains("typed::pods|d0%"));
    assert_eq!(
        app.panes().len(),
        1,
        "unprefixed '%' must not split the view"
    );

    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
    );
    assert!(!app.armed, "ctrl-a is no longer the prefix");
}

#[test]
fn quitting_k9s_in_a_split_closes_that_pane() {
    let mut app = session();
    prefix(&mut app, '%');
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));
    assert_eq!(app.panes().len(), 2);

    typ(&mut app, ":q\n");

    let deadline = Instant::now() + Duration::from_secs(15);
    while app.panes().len() > 1 {
        app.reap();
        assert!(Instant::now() < deadline, "the pane never closed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.panes().len(), 1);
    assert_eq!(app.focus(), 0);
    assert_eq!(app.pane().instance, "use1-a");
    assert!(!app.should_quit, "k9mux should stay open");
    wait_screen(&app, |s| s.contains("k9s@use1-a ready"));
}

#[test]
fn quitting_the_last_k9s_quits_k9mux() {
    let mut app = session();
    wait_screen(&app, |s| s.contains("k9s@use1-a ready"));
    typ(&mut app, ":q\n");

    let deadline = Instant::now() + Duration::from_secs(15);
    while !app.should_quit {
        app.reap();
        assert!(Instant::now() < deadline, "k9mux never quit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.windows.is_empty());
}

#[test]
fn the_prefix_is_what_drives_the_panes() {
    let mut app = session();
    prefix(&mut app, '%');
    assert_eq!(app.panes().len(), 2);
    prefix(&mut app, '"');
    assert_eq!(app.panes().len(), 3);
    prefix(&mut app, 'x');
    assert_eq!(app.panes().len(), 2);
}

#[test]
fn focus_moves_with_the_prefix_and_typing_follows_it() {
    let mut app = session();
    prefix(&mut app, '%');
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));
    assert_eq!(app.focus(), 1);

    // option+h, no prefix needed
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT),
    );
    assert_eq!(app.focus(), 0);
    typ(&mut app, "first\n");
    wait_screen(&app, |s| s.contains("use1-a typed:first"));
}

#[test]
fn a_pane_can_be_pointed_at_another_cluster() {
    let mut app = session();
    prefix(&mut app, '>');
    assert_eq!(app.pane().instance, "use1-b");
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));
}

#[test]
fn clicking_a_pane_in_the_sidebar_focuses_it() {
    let mut app = session();
    prefix(&mut app, '%');
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));

    // Clicking the first pane row in the sidebar focuses that pane.
    let (row, target) = ui::sidebar_rows(&app)
        .into_iter()
        .find(|(_, t)| matches!(t, ui::Row::Pane { pane: 0, .. }))
        .unwrap();
    assert_eq!(ui::sidebar_hit(&app, 4, row), Some(target));
    app.set_focus(0);
    typ(&mut app, "first\n");
    wait_screen(&app, |s| s.contains("use1-a typed:first"));
}

#[test]
fn a_literal_prefix_can_be_sent_through_to_k9s() {
    let mut app = session();
    let opt_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::ALT);
    press(&mut app, opt_space);
    assert!(app.armed);
    press(&mut app, opt_space);
    assert!(!app.armed);
    assert_eq!(
        app.panes().len(),
        1,
        "the second option-space was sent, not acted on"
    );
}

#[test]
fn a_non_breaking_space_also_arms_the_prefix() {
    // What macOS sends when option is not configured as meta.
    let mut app = session();
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('\u{a0}'), KeyModifiers::NONE),
    );
    assert!(app.armed);
    press(
        &mut app,
        KeyEvent::new(KeyCode::Char('%'), KeyModifiers::NONE),
    );
    assert_eq!(app.panes().len(), 2);
}

#[test]
fn children_are_resized_to_match_their_panes() {
    let mut app = session();
    prefix(&mut app, '%');
    let area = Rect::new(0, 0, 160, 44);
    let sizes = ui::pane_inner_sizes(&app, area);
    app.resize_panes(&sizes);
    for (i, expected) in &sizes {
        assert_eq!(app.panes()[*i].session.as_ref().unwrap().size(), *expected);
    }
}

#[test]
fn a_pane_survives_its_child_exiting() {
    let mut app = App::new(
        fleet(),
        Box::new(|_: &str| Launch::new("sh").arg("-c").arg("exit 0")),
    );
    wait_screen(&app, |s| s.contains("k9s exited"));
    prefix(&mut app, 'r');
    assert!(app.pane().session.is_some());
}

#[test]
fn the_pane_limit_and_the_last_pane_rule_hold() {
    let mut app = session();
    for _ in 0..8 {
        prefix(&mut app, '%');
    }
    assert!(app.panes().len() > 1);
    while !app.should_quit {
        prefix(&mut app, 'x');
    }
    assert!(app.windows.is_empty(), "closing every pane quits k9mux");
}

#[test]
fn grouping_from_config_drives_the_fleet() {
    let cfg = Config::parse(
        "cluster east use1-a
\
         cluster west usw2-a
",
    )
    .unwrap();
    let contexts = ["use1-a", "usw2-a", "loose-ctx"]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let app = App::new(Fleet::from_contexts(&contexts, &cfg.grouping()), stub());
    let names: Vec<_> = app.fleet.groups.iter().map(|g| g.name.clone()).collect();
    assert_eq!(names, vec!["east", "west", "contexts"]);
    assert_eq!(app.pane().instance, "use1-a", "opens the first cluster");
}

#[test]
fn windows_are_created_named_and_switched() {
    let mut app = session();
    assert_eq!(app.window().name, "k9mux");

    prefix(&mut app, 'c');
    assert_eq!(app.windows.len(), 2);
    wait_screen(&app, |s| s.contains("k9s@use1-b ready"));

    prefix(&mut app, ',');
    while !matches!(&app.mode, k9mux::app::Mode::Rename(t) if t.is_empty()) {
        press(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
    }
    typ(
        &mut app, "staging
",
    );
    assert_eq!(app.window().name, "staging");
    assert!(screen(&app).contains("2:staging"), "{}", screen(&app));

    prefix(&mut app, 'n');
    assert_eq!(app.current, 0);
    assert_eq!(app.window().name, "k9mux");
}
