use crate::fleet::{Fleet, Health};
use crate::keymap::Keymap;
use crate::keys::{Action, Dir};
use crate::layout::Split;
use crate::session::{Launch, Session};

pub const MAX_PANES: usize = 4;
/// What a window is called until it is renamed.
pub const DEFAULT_WINDOW_NAME: &str = "k9mux";

/// Builds the command for a pane. Production runs k9s; tests run a stub.
pub type Spawner = Box<dyn Fn(&str) -> Launch + Send + Sync>;

/// The k9s invocation for a context, with k9mux' kubeconfig handed down.
pub fn k9s_launch(kubeconfig: &str) -> Spawner {
    let kubeconfig = kubeconfig.to_string();
    Box::new(move |context: &str| {
        Launch::new("k9s")
            .arg("--context")
            .arg(context)
            .env("KUBECONFIG", &kubeconfig)
            .env("TERM", "xterm-256color")
    })
}

pub struct Pane {
    pub instance: String,
    pub session: Option<Session>,
    /// Set when the session could not be started at all.
    pub error: Option<String>,
}

impl Pane {
    /// True once k9s has exited, so the pane can say so instead of freezing.
    pub fn exited(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.alive())
    }
    /// Exit code, once the child is gone.
    pub fn exit_code(&self) -> Option<i32> {
        self.session.as_ref().and_then(|s| s.exit_code())
    }
}

/// A named group of panes, like a tmux window.
pub struct Window {
    pub name: String,
    pub panes: Vec<Pane>,
    pub focus: usize,
    pub split: Split,
}

/// Typing a window name takes over the keyboard until it is finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Rename(String),
}

pub struct App {
    pub fleet: Fleet,
    pub windows: Vec<Window>,
    pub current: usize,
    pub keymap: Keymap,
    pub armed: bool,
    pub help: bool,
    /// False when the sidebar is collapsed to give k9s the whole width.
    pub sidebar: bool,
    pub mode: Mode,
    pub should_quit: bool,
    spawn: Spawner,
    /// Size last used for a pane, so new panes start out sensible.
    pane_size: (u16, u16),
}

impl App {
    pub fn new(fleet: Fleet, spawn: Spawner) -> App {
        App::with_keymap(fleet, spawn, Keymap::default())
    }

    pub fn with_keymap(fleet: Fleet, spawn: Spawner, keymap: Keymap) -> App {
        let first = fleet
            .instances()
            .first()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let mut app = App {
            fleet,
            windows: vec![Window {
                name: DEFAULT_WINDOW_NAME.to_string(),
                panes: vec![Pane {
                    instance: first,
                    session: None,
                    error: None,
                }],
                focus: 0,
                split: Split::Vertical,
            }],
            current: 0,
            keymap,
            armed: false,
            help: false,
            sidebar: true,
            mode: Mode::Normal,
            should_quit: false,
            spawn,
            pane_size: (24, 80),
        };
        app.start(0, 0);
        app
    }

    pub fn window(&self) -> &Window {
        &self.windows[self.current]
    }
    fn window_mut(&mut self) -> &mut Window {
        let i = self.current;
        &mut self.windows[i]
    }
    pub fn panes(&self) -> &[Pane] {
        &self.window().panes
    }
    pub fn focus(&self) -> usize {
        self.window().focus
    }
    pub fn set_focus(&mut self, idx: usize) {
        let n = self.panes().len();
        if idx < n {
            self.window_mut().focus = idx;
        }
    }
    pub fn split(&self) -> Split {
        self.window().split
    }
    pub fn pane(&self) -> &Pane {
        let w = self.window();
        &w.panes[w.focus]
    }

    /// Starts (or restarts) the child for a pane of a window.
    fn start(&mut self, win: usize, idx: usize) {
        let context = self
            .fleet
            .find(&self.windows[win].panes[idx].instance)
            .map(|i| i.context.clone())
            .unwrap_or_else(|| self.windows[win].panes[idx].instance.clone());
        let launch = (self.spawn)(&context);
        let (rows, cols) = self.pane_size;
        let name = self.windows[win].panes[idx].instance.clone();
        match Session::spawn(&launch, rows, cols) {
            Ok(s) => {
                self.windows[win].panes[idx].session = Some(s);
                self.windows[win].panes[idx].error = None;
                self.fleet.set_health(&name, Health::Ok);
            }
            Err(e) => {
                self.windows[win].panes[idx].session = None;
                self.windows[win].panes[idx].error = Some(e);
                self.fleet.set_health(&name, Health::Unreachable);
            }
        }
    }

    /// Tells the visible panes how big they are now.
    pub fn resize_panes(&mut self, sizes: &[(u16, u16)]) {
        if let Some(first) = sizes.first() {
            self.pane_size = *first;
        }
        let i = self.current;
        for (pane, (rows, cols)) in self.windows[i].panes.iter_mut().zip(sizes) {
            if let Some(s) = pane.session.as_mut() {
                s.resize(*rows, *cols);
            }
        }
    }

    /// Sends typed bytes to the focused k9s.
    pub fn forward(&mut self, bytes: &[u8]) {
        let (w, f) = (self.current, self.focus());
        if let Some(s) = self.windows[w].panes[f].session.as_mut() {
            s.write(bytes);
        }
    }

    pub fn attach(&mut self, instance: &str) {
        if self.fleet.find(instance).is_none() {
            return;
        }
        let (w, f) = (self.current, self.focus());
        self.windows[w].panes[f].instance = instance.to_string();
        self.windows[w].panes[f].session = None;
        self.start(w, f);
    }

    /// Contexts open in any window, so a new pane picks something else.
    fn in_use(&self) -> Vec<String> {
        self.windows
            .iter()
            .flat_map(|w| w.panes.iter().map(|p| p.instance.clone()))
            .collect()
    }

    fn fresh_instance(&self, fallback: &str) -> String {
        self.fleet
            .first_unused(&self.in_use())
            .map(|i| i.name.clone())
            .unwrap_or_else(|| fallback.to_string())
    }

    pub fn new_window(&mut self) {
        let first = self
            .fleet
            .instances()
            .first()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let instance = self.fresh_instance(&first);
        self.windows.push(Window {
            name: DEFAULT_WINDOW_NAME.to_string(),
            panes: vec![Pane {
                instance,
                session: None,
                error: None,
            }],
            focus: 0,
            split: Split::Vertical,
        });
        self.current = self.windows.len() - 1;
        let w = self.current;
        self.start(w, 0);
    }

    pub fn select_window(&mut self, idx: usize) {
        if idx < self.windows.len() {
            self.current = idx;
        }
    }

    fn close_window(&mut self, idx: usize) {
        self.windows.remove(idx);
        if self.windows.is_empty() {
            self.should_quit = true;
            self.current = 0;
        } else {
            self.current = self.current.min(self.windows.len() - 1);
        }
    }

    /// Drops panes whose k9s has quit cleanly, the way a terminal multiplexer
    /// closes a pane when its shell exits. A crash is kept on screen so the
    /// reason stays readable. Returns true if anything changed.
    pub fn reap(&mut self) -> bool {
        let mut changed = false;
        for w in (0..self.windows.len()).rev() {
            let doomed: Vec<usize> = self.windows[w]
                .panes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.exit_code() == Some(0))
                .map(|(i, _)| i)
                .collect();
            if doomed.is_empty() {
                continue;
            }
            changed = true;
            for i in doomed.into_iter().rev() {
                self.windows[w].panes.remove(i);
            }
            if self.windows[w].panes.is_empty() {
                self.close_window(w);
            } else {
                let last = self.windows[w].panes.len() - 1;
                self.windows[w].focus = self.windows[w].focus.min(last);
            }
        }
        changed
    }

    /// Moves focus geometrically, so hjkl means what it looks like whether the
    /// panes are in a row, a column, or a 2x2 grid.
    fn focus_dir(&mut self, dir: Dir) {
        let g = crate::layout::grid(self.panes().len(), self.split());
        let cols = g.cols as usize;
        let (row, col) = (self.focus() / cols, self.focus() % cols);
        let (dr, dc) = match dir {
            Dir::Left => (0i32, -1i32),
            Dir::Right => (0, 1),
            Dir::Up => (-1, 0),
            Dir::Down => (1, 0),
        };
        let (r, c) = (row as i32 + dr, col as i32 + dc);
        if r < 0 || c < 0 || c >= cols as i32 || r >= g.rows as i32 {
            return;
        }
        let next = r as usize * cols + c as usize;
        if next < self.panes().len() {
            self.window_mut().focus = next;
        }
    }

    /// Handles a key while a window name is being typed. Returns true when the
    /// rename is over, either way.
    pub fn rename_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        let Mode::Rename(text) = &mut self.mode else {
            return true;
        };
        match key.code {
            KeyCode::Char(c) => {
                text.push(c);
                false
            }
            KeyCode::Backspace => {
                text.pop();
                false
            }
            KeyCode::Enter => {
                let name = text.trim().to_string();
                if !name.is_empty() {
                    self.window_mut().name = name;
                }
                self.mode = Mode::Normal;
                true
            }
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                true
            }
            _ => false,
        }
    }

    pub fn apply(&mut self, action: Action) {
        let idx = self.focus();
        let w = self.current;
        match action {
            Action::Quit => self.should_quit = true,
            Action::ToggleHelp => self.help = !self.help,
            Action::ToggleSidebar => self.sidebar = !self.sidebar,
            Action::Restart => self.start(w, idx),

            Action::NewWindow => self.new_window(),
            Action::RenameWindow => {
                let name = self.window().name.clone();
                self.mode = Mode::Rename(name);
            }
            Action::NextWindow => {
                self.current = (self.current + 1) % self.windows.len();
            }
            Action::PrevWindow => {
                self.current = (self.current + self.windows.len() - 1) % self.windows.len();
            }
            Action::SelectWindow(n) => self.select_window(n),

            Action::Split(dir) => {
                self.window_mut().split = dir;
                if self.panes().len() >= MAX_PANES {
                    return;
                }
                let fallback = self.windows[w].panes[idx].instance.clone();
                let next = self.fresh_instance(&fallback);
                self.windows[w].panes.insert(
                    idx + 1,
                    Pane {
                        instance: next,
                        session: None,
                        error: None,
                    },
                );
                self.windows[w].focus = idx + 1;
                self.start(w, idx + 1);
            }
            Action::ClosePane => {
                if self.panes().len() == 1 {
                    // Closing the only pane closes the window, as in tmux.
                    self.close_window(w);
                    return;
                }
                self.windows[w].panes.remove(idx);
                let last = self.windows[w].panes.len() - 1;
                self.windows[w].focus = idx.min(last);
            }
            Action::FocusNext => {
                let n = self.panes().len();
                self.window_mut().focus = (idx + 1) % n;
            }
            Action::Focus(dir) => self.focus_dir(dir),

            Action::NextInstance | Action::PrevInstance => {
                let step = if action == Action::NextInstance {
                    1
                } else {
                    -1
                };
                if let Some(next) = self
                    .fleet
                    .step(&self.windows[w].panes[idx].instance, step)
                    .map(|i| i.name.clone())
                {
                    self.attach(&next);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::Grouping;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::time::{Duration, Instant};

    fn fleet() -> Fleet {
        Fleet::from_contexts(
            &["use1-a", "use1-b", "usw2-b", "euc1-a"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            &Grouping {
                clusters: vec![
                    ("prod-us".into(), vec!["use1-a".into(), "use1-b".into()]),
                    ("prod-eu".into(), vec!["usw2-b".into(), "euc1-a".into()]),
                ],
            },
        )
    }

    /// Stands in for k9s: prints its context, then waits for input, quitting
    /// on `:q` exactly as k9s does.
    fn stub() -> Spawner {
        Box::new(|ctx: &str| {
            Launch::new("sh")
                .arg("-c")
                .arg(
                    "printf 'k9s ctx=%s\\n' \"$K9MUX_CTX\"; \
                     while read line; do [ \"$line\" = ':q' ] && exit 0; done",
                )
                .env("K9MUX_CTX", ctx)
        })
    }

    fn app() -> App {
        App::new(fleet(), stub())
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
    fn typ(a: &mut App, s: &str) {
        for c in s.chars() {
            a.rename_key(key(c));
        }
    }

    /// Clears whatever is in the rename prompt, whatever its length.
    fn clear_name(a: &mut App) {
        while !matches!(&a.mode, Mode::Rename(t) if t.is_empty()) {
            a.rename_key(code(KeyCode::Backspace));
        }
    }

    fn wait_for(app: &App, win: usize, idx: usize, f: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = app.windows[win].panes[idx]
                .session
                .as_ref()
                .map(|s| s.text())
                .unwrap_or_default();
            if f(&text) {
                return text;
            }
            assert!(Instant::now() < deadline, "timed out; pane showed:\n{text}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_until(mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < deadline, "condition never held");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // ---- windows ----

    #[test]
    fn it_opens_one_window_on_the_first_cluster() {
        let a = app();
        assert_eq!(a.windows.len(), 1);
        assert_eq!(a.current, 0);
        assert_eq!(a.window().name, "k9mux");
        assert_eq!(a.panes().len(), 1);
        assert_eq!(a.pane().instance, "use1-a");
        wait_for(&a, 0, 0, |t| t.contains("k9s ctx=use1-a"));
    }

    #[test]
    fn a_new_window_starts_its_own_k9s_and_becomes_current() {
        let mut a = app();
        a.apply(Action::NewWindow);
        assert_eq!(a.windows.len(), 2);
        assert_eq!(a.current, 1);
        assert_eq!(a.window().name, "k9mux");
        assert_eq!(a.panes().len(), 1);
        assert_eq!(
            a.pane().instance,
            "use1-b",
            "picks a cluster not already open"
        );
        wait_for(&a, 1, 0, |t| t.contains("k9s ctx=use1-b"));
    }

    #[test]
    fn windows_keep_their_own_panes() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.panes().len(), 2);
        a.apply(Action::NewWindow);
        assert_eq!(a.panes().len(), 1, "a new window starts with one pane");
        a.select_window(0);
        assert_eq!(a.panes().len(), 2, "the first window still has two");
    }

    #[test]
    fn windows_keep_their_own_split_direction() {
        let mut a = app();
        a.apply(Action::Split(Split::Horizontal));
        a.apply(Action::NewWindow);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.split(), Split::Vertical);
        a.select_window(0);
        assert_eq!(a.split(), Split::Horizontal);
    }

    #[test]
    fn windows_cycle_both_ways() {
        let mut a = app();
        a.apply(Action::NewWindow);
        a.apply(Action::NewWindow);
        assert_eq!(a.current, 2);
        a.apply(Action::NextWindow);
        assert_eq!(a.current, 0);
        a.apply(Action::PrevWindow);
        assert_eq!(a.current, 2);
    }

    #[test]
    fn a_window_can_be_selected_by_number() {
        let mut a = app();
        a.apply(Action::NewWindow);
        a.apply(Action::SelectWindow(0));
        assert_eq!(a.current, 0);
        a.apply(Action::SelectWindow(9));
        assert_eq!(a.current, 0, "out of range is ignored");
    }

    // ---- renaming ----

    #[test]
    fn renaming_starts_from_the_current_name() {
        let mut a = app();
        a.apply(Action::RenameWindow);
        assert_eq!(a.mode, Mode::Rename("k9mux".into()));
    }

    #[test]
    fn a_typed_name_is_committed_on_enter() {
        let mut a = app();
        a.apply(Action::RenameWindow);
        clear_name(&mut a);
        typ(&mut a, "prod");
        assert_eq!(a.mode, Mode::Rename("prod".into()));
        assert!(a.rename_key(code(KeyCode::Enter)));
        assert_eq!(a.window().name, "prod");
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn escape_leaves_the_name_alone() {
        let mut a = app();
        a.apply(Action::RenameWindow);
        typ(&mut a, "-staging");
        assert!(a.rename_key(code(KeyCode::Esc)));
        assert_eq!(a.window().name, "k9mux");
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn an_empty_name_is_refused() {
        let mut a = app();
        a.apply(Action::RenameWindow);
        clear_name(&mut a);
        a.rename_key(code(KeyCode::Enter));
        assert_eq!(a.window().name, "k9mux", "should keep the old name");
    }

    #[test]
    fn renaming_only_touches_the_current_window() {
        let mut a = app();
        a.apply(Action::NewWindow);
        a.apply(Action::RenameWindow);
        clear_name(&mut a);
        typ(&mut a, "second");
        a.rename_key(code(KeyCode::Enter));
        assert_eq!(a.windows[1].name, "second");
        assert_eq!(a.windows[0].name, "k9mux");
    }

    #[test]
    fn keys_typed_during_a_rename_do_not_reach_k9s() {
        let mut a = app();
        wait_for(&a, 0, 0, |t| t.contains("ctx="));
        a.apply(Action::RenameWindow);
        typ(&mut a, "qqq");
        assert!(!a.should_quit, "q must not quit while renaming");
        assert_eq!(a.mode, Mode::Rename("k9muxqqq".into()));
    }

    // ---- panes within a window ----

    #[test]
    fn splitting_starts_a_second_child_on_another_cluster() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.panes().len(), 2);
        assert_eq!(a.focus(), 1);
        assert_eq!(a.panes()[1].instance, "use1-b");
        wait_for(&a, 0, 1, |t| t.contains("k9s ctx=use1-b"));
    }

    #[test]
    fn splitting_stops_at_the_pane_limit() {
        let mut a = app();
        for _ in 0..8 {
            a.apply(Action::Split(Split::Vertical));
        }
        assert_eq!(a.panes().len(), MAX_PANES);
    }

    #[test]
    fn closing_the_only_pane_closes_the_window() {
        let mut a = app();
        a.apply(Action::NewWindow);
        assert_eq!(a.windows.len(), 2);
        a.apply(Action::ClosePane);
        assert_eq!(a.windows.len(), 1);
        assert_eq!(a.current, 0);
        assert!(!a.should_quit);
    }

    #[test]
    fn closing_the_last_pane_of_the_last_window_quits() {
        let mut a = app();
        a.apply(Action::ClosePane);
        assert!(a.should_quit);
    }

    #[test]
    fn hjkl_navigates_a_two_by_two_grid() {
        let mut a = app();
        for _ in 0..3 {
            a.apply(Action::Split(Split::Vertical));
        }
        a.set_focus(0);
        a.apply(Action::Focus(Dir::Right));
        assert_eq!(a.focus(), 1);
        a.apply(Action::Focus(Dir::Down));
        assert_eq!(a.focus(), 3);
        a.apply(Action::Focus(Dir::Left));
        assert_eq!(a.focus(), 2);
        a.apply(Action::Focus(Dir::Up));
        assert_eq!(a.focus(), 0);
    }

    #[test]
    fn focus_stops_at_the_edges_instead_of_wrapping() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Focus(Dir::Left));
        a.apply(Action::Focus(Dir::Left));
        assert_eq!(a.focus(), 0);
        a.apply(Action::Focus(Dir::Up));
        assert_eq!(a.focus(), 0);
    }

    #[test]
    fn typing_reaches_only_the_focused_child() {
        let mut a = App::new(
            fleet(),
            Box::new(|ctx: &str| {
                Launch::new("sh")
                    .arg("-c")
                    .arg("read line; printf '%s got:%s\\n' \"$K9MUX_CTX\" \"$line\"; sleep 20")
                    .env("K9MUX_CTX", ctx)
            }),
        );
        a.apply(Action::Split(Split::Vertical));
        a.forward(b"hello\r");
        wait_for(&a, 0, 1, |t| t.contains("got:hello"));
        let other = a.panes()[0].session.as_ref().unwrap().text();
        assert!(!other.contains("got:"), "unfocused pane received input");
    }

    // ---- clusters ----

    #[test]
    fn the_real_launcher_runs_k9s_with_the_context_and_kubeconfig() {
        let launch = k9s_launch("/tmp/kc-a:/tmp/kc-b")("use1-a");
        assert_eq!(launch.program, "k9s");
        assert_eq!(launch.args, vec!["--context", "use1-a"]);
        assert!(
            launch
                .env
                .contains(&("KUBECONFIG".into(), "/tmp/kc-a:/tmp/kc-b".into()))
        );
        assert!(launch.env.iter().any(|(k, _)| k == "TERM"));
    }

    #[test]
    fn attaching_swaps_the_cluster_in_the_focused_pane() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.attach("euc1-a");
        assert_eq!(a.panes()[1].instance, "euc1-a");
        assert_eq!(a.panes()[0].instance, "use1-a");
        wait_for(&a, 0, 1, |t| t.contains("k9s ctx=euc1-a"));
    }

    #[test]
    fn attaching_an_unknown_instance_is_ignored() {
        let mut a = app();
        a.attach("nope");
        assert_eq!(a.pane().instance, "use1-a");
    }

    #[test]
    fn walking_the_fleet_wraps_around() {
        let mut a = app();
        a.apply(Action::PrevInstance);
        assert_eq!(a.pane().instance, "euc1-a");
        a.apply(Action::NextInstance);
        assert_eq!(a.pane().instance, "use1-a");
    }

    // ---- lifecycle ----

    #[test]
    fn quitting_k9s_in_a_split_closes_that_pane() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.forward(b":q\r");
        wait_until(|| a.panes()[1].exit_code() == Some(0));
        assert!(a.reap());
        assert_eq!(a.panes().len(), 1);
        assert_eq!(a.focus(), 0);
        assert!(!a.should_quit);
    }

    #[test]
    fn quitting_the_last_k9s_in_a_window_closes_the_window() {
        let mut a = app();
        a.apply(Action::NewWindow);
        a.forward(b":q\r");
        wait_until(|| a.windows[1].panes[0].exit_code() == Some(0));
        assert!(a.reap());
        assert_eq!(a.windows.len(), 1);
        assert!(!a.should_quit);
    }

    #[test]
    fn quitting_the_only_k9s_quits_k9mux() {
        let mut a = App::new(
            fleet(),
            Box::new(|_: &str| Launch::new("sh").arg("-c").arg("exit 0")),
        );
        wait_until(|| a.pane().exit_code() == Some(0));
        assert!(a.reap());
        assert!(a.should_quit);
    }

    #[test]
    fn a_crashed_k9s_keeps_its_pane_so_you_can_read_why() {
        let mut a = App::new(
            fleet(),
            Box::new(|_: &str| Launch::new("sh").arg("-c").arg("exit 1")),
        );
        wait_until(|| a.pane().exited());
        assert_eq!(a.pane().exit_code(), Some(1));
        assert!(!a.reap(), "a failure must stay on screen");
        assert_eq!(a.panes().len(), 1);
    }

    #[test]
    fn a_missing_binary_is_reported_on_the_pane() {
        let a = App::new(
            fleet(),
            Box::new(|_: &str| Launch::new("k9mux-no-such-binary-xyz")),
        );
        assert!(a.pane().error.is_some());
        assert_eq!(a.fleet.find("use1-a").unwrap().health, Health::Unreachable);
    }

    #[test]
    fn restarting_gives_the_pane_a_fresh_child() {
        let mut a = app();
        wait_for(&a, 0, 0, |t| t.contains("k9s ctx=use1-a"));
        a.apply(Action::Restart);
        assert!(a.pane().session.is_some());
        wait_for(&a, 0, 0, |t| t.contains("k9s ctx=use1-a"));
    }

    #[test]
    fn panes_are_resized_to_their_share_of_the_screen() {
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.resize_panes(&[(30, 60), (30, 61)]);
        assert_eq!(a.panes()[0].session.as_ref().unwrap().size(), (30, 60));
        assert_eq!(a.panes()[1].session.as_ref().unwrap().size(), (30, 61));
    }

    #[test]
    fn a_new_pane_starts_at_the_current_pane_size() {
        let mut a = app();
        a.resize_panes(&[(40, 100)]);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.panes()[1].session.as_ref().unwrap().size(), (40, 100));
    }

    #[test]
    fn the_sidebar_can_be_collapsed_and_brought_back() {
        let mut a = app();
        assert!(a.sidebar, "shown by default");
        a.apply(Action::ToggleSidebar);
        assert!(!a.sidebar);
        a.apply(Action::ToggleSidebar);
        assert!(a.sidebar);
    }

    #[test]
    fn quitting_and_help_toggle() {
        let mut a = app();
        a.apply(Action::ToggleHelp);
        assert!(a.help);
        a.apply(Action::ToggleHelp);
        assert!(!a.help);
        a.apply(Action::Quit);
        assert!(a.should_quit);
    }
}
