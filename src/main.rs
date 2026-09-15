use std::io::stdout;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use k9mux::app::{App, Mode, k9s_launch};
use k9mux::config::Config;
use k9mux::fleet::Fleet;
use k9mux::keys::Outcome;
use k9mux::kubeconfig;
use k9mux::ui;

/// Short enough that output from k9s appears without visible lag.
const TICK: Duration = Duration::from_millis(16);

const HELP: &str = "\
k9mux - run k9s against several clusters at once

k9mux does not reimplement k9s. Each pane is a real k9s process; k9mux only
arranges the panes and points each one at a different cluster.

USAGE:
    k9mux [--kubeconfig <paths>]

OPTIONS:
    --kubeconfig <paths>  kubeconfig file(s), colon separated
    -h, --help            print this help

KEYS:
    Every key goes to the k9s in the focused pane. Pane management sits
    behind a prefix, option-space by default. Run with --keys to print the
    default bindings in config syntax.

    Windows and panes work like tmux: option-space c opens a window,
    option-space , renames it, option-space % and \" split it into panes.

    Quitting k9s in a pane (:q) closes that pane; closing a window's last
    pane closes the window, and closing the last window quits k9mux.

KUBECONFIG:
    Read in this order, first match wins:
      1. --kubeconfig
      2. $KUBECONFIG          (colon separated, merged)
      3. kubeconfigs in ~/.config/k9mux/config.toml
      4. ~/.kube/config

CONFIG:
    ~/.config/k9mux/config, one directive per line, in the style of tmux.conf.
    With no `cluster` line, each kubeconfig file becomes one cluster named
    after the file.

        set prefix M-space

        bind '\"' split-horizontal
        bind %   split-vertical
        bind -n M-h focus-left
        unbind x

        kubeconfig ~/.kube/config-prod
        cluster prod-us use1-a use1-b

    Commands: split-vertical split-horizontal close-pane focus-left
    focus-right focus-up focus-down focus-next next-cluster prev-cluster
    restart help quit
";

fn build() -> Result<App, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--keys") {
        print!("{}", k9mux::keymap::DEFAULTS);
        std::process::exit(0);
    }

    let flag = args
        .iter()
        .position(|a| a == "--kubeconfig")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let env = std::env::var("KUBECONFIG").ok();
    let config = Config::load()?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);

    let paths = kubeconfig::resolve_paths(
        flag.as_deref(),
        env.as_deref(),
        &config.kubeconfigs,
        home.as_deref(),
    );
    let loaded = kubeconfig::load(&paths).map_err(|e| {
        format!(
            "{e}\n\nlooked in:\n{}\n\nset KUBECONFIG, pass --kubeconfig, or list files in {}",
            paths
                .iter()
                .map(|p| format!("  {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
            Config::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        )
    })?;

    let kubeconfig_env = loaded.kubeconfig_env();
    let grouping = if config.clusters.is_empty() {
        loaded.grouping
    } else {
        config.grouping()
    };
    Ok(App::with_keymap(
        Fleet::from_contexts(&loaded.contexts, &grouping),
        k9s_launch(&kubeconfig_env),
        config.keymap,
    ))
}

fn main() {
    let mut app = match build() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("k9mux: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = run(&mut app) {
        eprintln!("k9mux: {e}");
        std::process::exit(1);
    }
}

fn run(app: &mut App) -> Result<(), String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture).map_err(|e| e.to_string())?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;

    let result = event_loop(app, &mut term);

    disable_raw_mode().ok();
    execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    term.show_cursor().ok();
    result
}

fn event_loop<B: ratatui::backend::Backend>(
    app: &mut App,
    term: &mut Terminal<B>,
) -> Result<(), String> {
    loop {
        // Panes are sized from the frame, then the children are told to match.
        let size = term.size().map_err(|e| e.to_string())?;
        let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
        app.set_area(ui::pane_body(app, area));
        let sizes = ui::pane_inner_sizes(app, area);
        app.resize_panes(&sizes);

        term.draw(|f| ui::draw(f, app)).map_err(|e| e.to_string())?;

        if event::poll(TICK).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                // Naming a window takes the keyboard until it is done.
                Event::Key(k)
                    if k.kind == KeyEventKind::Press && matches!(app.mode, Mode::Rename(_)) =>
                {
                    app.rename_key(k);
                }
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    match app.keymap.resolve(k, app.armed) {
                        Outcome::Armed => app.armed = true,
                        Outcome::Do(action) => {
                            app.armed = false;
                            app.apply(action);
                        }
                        Outcome::Forward(bytes) => {
                            app.armed = false;
                            // The help overlay is k9mux', so any key dismisses it.
                            if app.help {
                                app.help = false;
                            } else {
                                app.forward(&bytes);
                            }
                        }
                        Outcome::Ignore => app.armed = false,
                    }
                }
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    match ui::sidebar_hit(app, m.column, m.row) {
                        Some(ui::Row::Window(w)) => app.select_window(w),
                        Some(ui::Row::Pane { window, pane }) => {
                            app.select_window(window);
                            app.set_focus(pane);
                        }
                        None => {
                            if let Some(idx) = ui::pane_hit(app, area, m.column, m.row) {
                                app.set_focus(idx);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // A pane whose k9s has quit goes away, like a shell exiting in tmux.
        app.reap();

        if app.should_quit {
            return Ok(());
        }
    }
}
