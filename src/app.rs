use crate::fleet::{Fleet, Health};
use crate::keymap::Keymap;
use crate::keys::Action;
use crate::layout::{Dir, Node, PaneId, Path, Split};
use crate::session::{Launch, Session};

/// A pane narrower or shorter than this is not worth creating, so a split
/// that would produce one is refused - the same rule tmux applies.
pub const MIN_PANE: (u16, u16) = (4, 10);
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
    pub id: PaneId,
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
    /// Which pane is focused, by id.
    pub focus: PaneId,
    /// How the panes are carved up.
    pub layout: Node,
    /// When set, this pane fills the window and the rest are hidden.
    pub zoomed: Option<PaneId>,
    next_id: PaneId,
}

impl Window {
    fn new(name: String, instance: String) -> Window {
        Window {
            name,
            panes: vec![Pane {
                id: 1,
                instance,
                session: None,
                error: None,
            }],
            focus: 1,
            layout: Node::leaf(1),
            zoomed: None,
            next_id: 2,
        }
    }
    pub fn index_of(&self, id: PaneId) -> Option<usize> {
        self.panes.iter().position(|p| p.id == id)
    }
    /// Panes and their rects, in layout order. A zoomed pane takes the lot
    /// and the others are not placed at all.
    pub fn rects(&self, area: ratatui::layout::Rect) -> Vec<(PaneId, ratatui::layout::Rect)> {
        match self.zoomed {
            Some(id) if self.index_of(id).is_some() => vec![(id, area)],
            _ => self.layout.rects(area),
        }
    }
    /// True when this pane is hidden behind a zoom.
    pub fn hidden(&self, id: PaneId) -> bool {
        self.zoomed.is_some_and(|z| z != id)
    }
}

/// Typing a window name takes over the keyboard until it is finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Rename(String),
    /// Choosing a cluster for the focused pane.
    Picker {
        query: String,
        selected: usize,
    },
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
    /// The border the mouse has hold of, while a drag is in progress.
    pub drag: Option<Path>,
    pub should_quit: bool,
    spawn: Spawner,
    /// Size last used for a pane, so new panes start out sensible.
    pane_size: (u16, u16),
    /// The area the panes were last laid out in, for geometry decisions.
    last_area: ratatui::layout::Rect,
    /// Watches whether the clusters are actually answering.
    prober: Option<crate::probe::Prober>,
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
            windows: vec![Window::new(DEFAULT_WINDOW_NAME.to_string(), first)],
            current: 0,
            keymap,
            armed: false,
            help: false,
            sidebar: true,
            mode: Mode::Normal,
            drag: None,
            should_quit: false,
            spawn,
            pane_size: (24, 80),
            last_area: ratatui::layout::Rect::new(0, 0, 80, 24),
            prober: None,
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
    /// Index of the focused pane within `panes()`.
    pub fn focus(&self) -> usize {
        let w = self.window();
        w.index_of(w.focus).unwrap_or(0)
    }
    /// Focus a pane by its position in `panes()`.
    pub fn set_focus(&mut self, idx: usize) {
        if let Some(id) = self.panes().get(idx).map(|p| p.id) {
            self.window_mut().focus = id;
        }
    }
    pub fn pane(&self) -> &Pane {
        let w = self.window();
        &w.panes[w.index_of(w.focus).unwrap_or(0)]
    }
    /// The panes actually on screen and where they sit: every pane normally,
    /// or just the zoomed one.
    pub fn visible_panes(&self) -> Vec<(usize, ratatui::layout::Rect)> {
        let w = self.window();
        w.rects(self.last_area)
            .into_iter()
            .filter_map(|(id, r)| w.index_of(id).map(|i| (i, r)))
            .collect()
    }

    /// Where the panes of the current window sit inside `area`, in the same
    /// order as `panes()`.
    pub fn pane_rects(&self, area: ratatui::layout::Rect) -> Vec<ratatui::layout::Rect> {
        let w = self.window();
        let placed = w.rects(area);
        w.panes
            .iter()
            .map(|p| {
                placed
                    .iter()
                    .find(|(id, _)| *id == p.id)
                    .map(|(_, r)| *r)
                    .unwrap_or(area)
            })
            .collect()
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
                // Whether the cluster is healthy is the prober's call, not
                // something a started process can tell us.
                let _ = &name;
            }
            Err(e) => {
                self.windows[win].panes[idx].session = None;
                self.windows[win].panes[idx].error = Some(e);
                self.fleet.set_health(&name, Health::Unreachable);
            }
        }
    }

    /// Starts watching these clusters for reachability.
    pub fn watch(&mut self, targets: Vec<crate::probe::Target>) {
        self.prober = Some(crate::probe::Prober::spawn(
            targets,
            std::time::Duration::from_secs(15),
            std::time::Duration::from_secs(3),
        ));
    }

    /// Applies any probe results. Returns true if a cluster changed state.
    pub fn poll_health(&mut self) -> bool {
        let Some(p) = self.prober.as_ref() else {
            return false;
        };
        let mut changed = false;
        for (instance, ok) in p.drain() {
            let want = if ok { Health::Ok } else { Health::Unreachable };
            if self.fleet.find(&instance).map(|i| i.health) != Some(want) {
                self.fleet.set_health(&instance, want);
                changed = true;
            }
        }
        changed
    }

    /// Moves a border the mouse has hold of. The path was worked out from the
    /// layout the last frame was drawn from, so one that has since gone stale
    /// is simply ignored.
    pub fn drag_border(&mut self, path: &Path, ratio: u16) -> bool {
        // A zoomed pane already owns the window; there is no border on screen.
        if self.window().zoomed.is_some() {
            return false;
        }
        self.window_mut().layout.set_ratio(path, ratio)
    }

    /// Remembers the region the panes are drawn in.
    pub fn set_area(&mut self, area: ratatui::layout::Rect) {
        self.last_area = area;
    }

    /// Tells the on-screen panes how big they are now, by pane index. Panes
    /// hidden behind a zoom keep the size they had.
    pub fn resize_panes(&mut self, sizes: &[(usize, (u16, u16))]) {
        if let Some((_, first)) = sizes.first() {
            self.pane_size = *first;
        }
        let w = self.current;
        for (idx, (rows, cols)) in sizes {
            if let Some(pane) = self.windows[w].panes.get_mut(*idx)
                && let Some(s) = pane.session.as_mut()
            {
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
        self.windows
            .push(Window::new(DEFAULT_WINDOW_NAME.to_string(), instance));
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
                let gone = self.windows[w].panes.remove(i).id;
                let layout = std::mem::replace(&mut self.windows[w].layout, Node::leaf(0));
                if let Some(tree) = layout.remove_leaf(gone) {
                    self.windows[w].layout = tree;
                }
            }
            if self.windows[w].panes.is_empty() {
                self.close_window(w);
            } else if self.windows[w].index_of(self.windows[w].focus).is_none() {
                self.windows[w].focus = self.windows[w].panes[0].id;
            }
        }
        changed
    }

    /// Moves focus geometrically, so hjkl means what it looks like whether the
    /// panes are in a row, a column, or a 2x2 grid.
    fn focus_dir(&mut self, dir: Dir) {
        let area = self.last_area;
        let w = self.window();
        if let Some(next) = crate::layout::neighbour(&w.rects(area), w.focus, dir) {
            self.window_mut().focus = next;
        }
    }

    /// Clusters offered by the picker, already filtered by what was typed.
    pub fn picker_matches(&self) -> Vec<String> {
        let Mode::Picker { query, .. } = &self.mode else {
            return Vec::new();
        };
        let all: Vec<String> = self
            .fleet
            .instances()
            .iter()
            .map(|i| i.name.clone())
            .collect();
        crate::picker::rank(&all, query)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Handles a key while the cluster picker is open. Returns true once it
    /// has closed.
    pub fn picker_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        let hits = self.picker_matches();
        let Mode::Picker { query, selected } = &mut self.mode else {
            return true;
        };
        match key.code {
            KeyCode::Char(c) => {
                query.push(c);
                *selected = 0;
                false
            }
            KeyCode::Backspace => {
                query.pop();
                *selected = 0;
                false
            }
            KeyCode::Down => {
                *selected = (*selected + 1).min(hits.len().saturating_sub(1));
                false
            }
            KeyCode::Up => {
                *selected = selected.saturating_sub(1);
                false
            }
            KeyCode::Enter => {
                let chosen = hits.get(*selected).cloned();
                self.mode = Mode::Normal;
                if let Some(name) = chosen {
                    self.attach(&name);
                }
                true
            }
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                true
            }
            _ => false,
        }
    }

    /// Opens a window per cluster group, each holding a pane per context in
    /// that group. This is the thing a plain multiplexer cannot do for you,
    /// because it has no idea what a cluster is.
    pub fn spread_clusters(&mut self) {
        let groups: Vec<(String, Vec<String>)> = self
            .fleet
            .groups
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.instances.iter().map(|i| i.name.clone()).collect(),
                )
            })
            .filter(|(_, members): &(String, Vec<String>)| !members.is_empty())
            .collect();

        for (name, members) in groups {
            let mut iter = members.into_iter();
            let Some(first) = iter.next() else { continue };
            self.windows.push(Window::new(name, first));
            self.current = self.windows.len() - 1;
            let w = self.current;
            self.start(w, 0);

            // Fill the window with the rest, alternating the split so the
            // panes stay roughly square, and stopping when there is no room.
            for (n, instance) in iter.enumerate() {
                let dir = if n % 2 == 0 {
                    Split::Horizontal
                } else {
                    Split::Vertical
                };
                if !self.split_focused_with(dir, Some(instance)) {
                    break;
                }
            }
        }
    }

    /// Splits the focused pane, optionally forcing which cluster the new pane
    /// runs. Returns false when there was no room.
    fn split_focused_with(&mut self, dir: Split, instance: Option<String>) -> bool {
        let w = self.current;
        let focused = self.window().focus;
        let Some(idx) = self.window().index_of(focused) else {
            return false;
        };
        let Some(rect) = self
            .window()
            .rects(self.last_area)
            .into_iter()
            .find(|(id, _)| *id == focused)
            .map(|(_, r)| r)
        else {
            return false;
        };
        let fits = match dir {
            Split::Vertical => rect.width / 2 >= MIN_PANE.1,
            Split::Horizontal => rect.height / 2 >= MIN_PANE.0,
        };
        if !fits {
            return false;
        }
        let next = match instance {
            Some(i) => i,
            None => {
                let fallback = self.windows[w].panes[idx].instance.clone();
                self.fresh_instance(&fallback)
            }
        };
        let id = self.windows[w].next_id;
        self.windows[w].next_id += 1;
        self.windows[w].panes.insert(
            idx + 1,
            Pane {
                id,
                instance: next,
                session: None,
                error: None,
            },
        );
        self.windows[w].layout.split_leaf(focused, dir, id);
        self.windows[w].focus = id;
        self.windows[w].zoomed = None;
        self.start(w, idx + 1);
        true
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
            Action::ToggleZoom => {
                let focused = self.window().focus;
                let w = self.window_mut();
                w.zoomed = match w.zoomed {
                    Some(_) => None,
                    None => Some(focused),
                };
            }
            Action::ClusterPicker => {
                self.mode = Mode::Picker {
                    query: String::new(),
                    selected: 0,
                };
            }
            Action::SpreadClusters => self.spread_clusters(),
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

            // Only the focused pane is divided; everything else keeps its shape.
            Action::Split(dir) => {
                self.split_focused_with(dir, None);
            }
            Action::ClosePane => {
                if self.panes().len() == 1 {
                    // Closing the only pane closes the window, as in tmux.
                    self.close_window(w);
                    return;
                }
                let gone = self.windows[w].focus;
                self.windows[w].panes.remove(idx);
                let layout = std::mem::replace(&mut self.windows[w].layout, Node::leaf(0));
                if let Some(tree) = layout.remove_leaf(gone) {
                    self.windows[w].layout = tree;
                }
                let last = self.windows[w].panes.len() - 1;
                self.windows[w].focus = self.windows[w].panes[idx.min(last)].id;
                self.windows[w].zoomed = None;
            }
            Action::FocusNext => {
                let n = self.panes().len();
                let next = (idx + 1) % n;
                self.windows[w].focus = self.windows[w].panes[next].id;
            }
            Action::Focus(dir) => self.focus_dir(dir),
            Action::Resize(dir) => {
                // A zoomed pane already owns the window; there is no border.
                if self.window().zoomed.is_none() {
                    let focused = self.window().focus;
                    self.window_mut()
                        .layout
                        .resize(focused, dir, crate::layout::RESIZE_STEP);
                }
            }

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
    use ratatui::layout::Rect;
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
    fn windows_keep_their_own_layout() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::Split(Split::Horizontal));
        a.apply(Action::NewWindow);
        assert_eq!(a.panes().len(), 1, "a new window starts fresh");
        a.select_window(0);
        assert_eq!(a.panes().len(), 2, "the first window kept its split");
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
    fn splitting_is_refused_when_there_is_no_room_left() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        for _ in 0..20 {
            a.apply(Action::Split(Split::Vertical));
        }
        let widths: Vec<u16> = a
            .pane_rects(Rect::new(0, 0, 80, 24))
            .iter()
            .map(|r| r.width)
            .collect();
        assert!(
            widths.iter().all(|w| *w >= MIN_PANE.1),
            "panes should never be squeezed below the minimum: {widths:?}"
        );
    }

    #[test]
    fn a_split_divides_only_the_focused_pane() {
        // Two side by side; focus the right one and stack it. The left pane
        // must keep the full height.
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.focus(), 1);

        a.apply(Action::Split(Split::Horizontal));
        assert_eq!(a.panes().len(), 3);

        let r = a.pane_rects(area);
        assert_eq!(r[0], Rect::new(0, 0, 40, 24), "left pane untouched");
        assert_eq!(r[1], Rect::new(40, 0, 40, 12), "top right");
        assert_eq!(r[2], Rect::new(40, 12, 40, 12), "bottom right");
    }

    #[test]
    fn closing_a_pane_gives_its_space_back_to_its_sibling() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Split(Split::Horizontal));
        a.apply(Action::ClosePane);
        assert_eq!(a.panes().len(), 2);
        let r = a.pane_rects(area);
        assert_eq!(r[0], Rect::new(0, 0, 40, 24));
        assert_eq!(
            r[1],
            Rect::new(40, 0, 40, 24),
            "sibling reclaimed the space"
        );
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
    fn hjkl_follows_the_actual_layout() {
        // left pane full height, right pane split into top and bottom
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Split(Split::Horizontal));
        assert_eq!(a.focus(), 2, "the new bottom-right pane has focus");

        a.apply(Action::Focus(Dir::Up));
        assert_eq!(a.focus(), 1, "top right");
        a.apply(Action::Focus(Dir::Left));
        assert_eq!(a.focus(), 0, "the tall left pane");
        a.apply(Action::Focus(Dir::Right));
        assert_eq!(a.focus(), 1);
        a.apply(Action::Focus(Dir::Down));
        assert_eq!(a.focus(), 2);
    }

    #[test]
    fn focus_stops_at_the_edges_instead_of_wrapping() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
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
        a.resize_panes(&[(0, (30, 60)), (1, (30, 61))]);
        assert_eq!(a.panes()[0].session.as_ref().unwrap().size(), (30, 60));
        assert_eq!(a.panes()[1].session.as_ref().unwrap().size(), (30, 61));
    }

    #[test]
    fn a_new_pane_starts_at_the_current_pane_size() {
        let mut a = app();
        a.resize_panes(&[(0, (40, 100))]);
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

    // ---- manual resize ----

    #[test]
    fn resizing_moves_the_border_between_two_panes() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.pane_rects(area)[0].width, 40);

        // focus is on the right pane, so growing it left takes from the left
        a.apply(Action::Resize(Dir::Left));
        let r = a.pane_rects(area);
        assert!(r[1].width > 40, "focused pane grew: {r:?}");
        assert_eq!(r[0].width + r[1].width, 80);
    }

    #[test]
    fn resizing_is_repeatable_and_then_stops() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        for _ in 0..40 {
            a.apply(Action::Resize(Dir::Left));
        }
        let r = a.pane_rects(area);
        assert!(r[0].width >= 1, "the other pane survives: {r:?}");
        assert_eq!(r[0].width + r[1].width, 80);
    }

    #[test]
    fn a_resize_sticks_across_a_redraw() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Resize(Dir::Left));
        let after = a.pane_rects(area);
        assert_eq!(a.pane_rects(area), after, "layout is stable");
    }

    #[test]
    fn resizing_a_zoomed_pane_does_nothing() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        let before = a.pane_rects(area);
        a.apply(Action::ToggleZoom);
        a.apply(Action::Resize(Dir::Left));
        a.apply(Action::ToggleZoom);
        assert_eq!(
            a.pane_rects(area),
            before,
            "the layout underneath is untouched"
        );
    }

    #[test]
    fn resizing_a_lone_pane_is_harmless() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Resize(Dir::Left));
        assert_eq!(a.pane_rects(area)[0], area);
    }

    #[test]
    fn the_child_is_told_about_its_new_size() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Resize(Dir::Left));

        let sizes: Vec<(usize, (u16, u16))> = a
            .visible_panes()
            .into_iter()
            .map(|(i, r)| (i, (r.height - 2, r.width - 2)))
            .collect();
        a.resize_panes(&sizes);
        for (i, expected) in &sizes {
            assert_eq!(a.panes()[*i].session.as_ref().unwrap().size(), *expected);
        }
    }

    // ---- zoom ----

    #[test]
    fn zooming_gives_the_focused_pane_the_whole_window() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.pane_rects(area)[1], Rect::new(40, 0, 40, 24));

        a.apply(Action::ToggleZoom);
        assert_eq!(a.window().zoomed, Some(a.window().focus));
        assert_eq!(a.visible_panes().len(), 1, "only the zoomed pane is shown");
        assert_eq!(a.visible_panes()[0].1, area, "and it fills the window");
    }

    #[test]
    fn unzooming_puts_the_layout_back_exactly() {
        let mut a = app();
        let area = Rect::new(0, 0, 80, 24);
        a.set_area(area);
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::Split(Split::Horizontal));
        let before = a.pane_rects(area);

        a.apply(Action::ToggleZoom);
        a.apply(Action::ToggleZoom);
        assert_eq!(a.window().zoomed, None);
        assert_eq!(a.pane_rects(area), before);
    }

    #[test]
    fn a_zoomed_pane_still_takes_the_keys() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::Split(Split::Vertical));
        let focused = a.focus();
        a.apply(Action::ToggleZoom);
        assert_eq!(a.focus(), focused);
    }

    #[test]
    fn splitting_while_zoomed_unzooms_first() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::ToggleZoom);
        a.apply(Action::Split(Split::Vertical));
        assert_eq!(a.window().zoomed, None, "tmux unzooms when you split");
        assert_eq!(a.panes().len(), 2);
    }

    #[test]
    fn closing_a_pane_clears_the_zoom() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::ToggleZoom);
        a.apply(Action::ClosePane);
        assert_eq!(a.window().zoomed, None);
        assert_eq!(a.visible_panes().len(), 1);
    }

    #[test]
    fn zoom_belongs_to_its_window() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 80, 24));
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::ToggleZoom);
        a.apply(Action::NewWindow);
        assert_eq!(a.window().zoomed, None);
        a.select_window(0);
        assert!(
            a.window().zoomed.is_some(),
            "the first window is still zoomed"
        );
    }

    // ---- cluster picker ----

    #[test]
    fn the_picker_offers_every_cluster_until_you_type() {
        let mut a = app();
        a.apply(Action::ClusterPicker);
        assert_eq!(a.picker_matches().len(), a.fleet.instances().len());
    }

    #[test]
    fn typing_narrows_the_picker() {
        let mut a = app();
        a.apply(Action::ClusterPicker);
        a.picker_key(key('e'));
        a.picker_key(key('u'));
        let hits = a.picker_matches();
        assert!(hits.contains(&"euc1-a".to_string()), "{hits:?}");
        assert!(!hits.contains(&"use1-b".to_string()), "{hits:?}");
    }

    #[test]
    fn enter_attaches_the_chosen_cluster_to_the_focused_pane() {
        let mut a = app();
        a.apply(Action::ClusterPicker);
        for c in "euc".chars() {
            a.picker_key(key(c));
        }
        assert!(a.picker_key(code(KeyCode::Enter)));
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.pane().instance, "euc1-a");
        wait_for(&a, 0, 0, |t| t.contains("k9s ctx=euc1-a"));
    }

    #[test]
    fn the_picker_can_be_walked_with_the_arrows() {
        let mut a = app();
        a.apply(Action::ClusterPicker);
        let second = a.picker_matches()[1].clone();
        a.picker_key(code(KeyCode::Down));
        a.picker_key(code(KeyCode::Enter));
        assert_eq!(a.pane().instance, second);
    }

    #[test]
    fn escape_closes_the_picker_without_changing_anything() {
        let mut a = app();
        let before = a.pane().instance.clone();
        a.apply(Action::ClusterPicker);
        a.picker_key(key('e'));
        assert!(a.picker_key(code(KeyCode::Esc)));
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.pane().instance, before);
    }

    #[test]
    fn enter_on_an_empty_result_does_nothing() {
        let mut a = app();
        let before = a.pane().instance.clone();
        a.apply(Action::ClusterPicker);
        for c in "zzzz".chars() {
            a.picker_key(key(c));
        }
        assert!(a.picker_matches().is_empty());
        a.picker_key(code(KeyCode::Enter));
        assert_eq!(a.pane().instance, before);
        assert_eq!(a.mode, Mode::Normal);
    }

    #[test]
    fn picker_keys_do_not_reach_k9s() {
        let mut a = app();
        a.apply(Action::ClusterPicker);
        a.picker_key(key('q'));
        assert!(!a.should_quit);
    }

    // ---- a window per cluster group ----

    #[test]
    fn spreading_opens_a_window_per_cluster_group() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 200, 60));
        a.apply(Action::SpreadClusters);

        let names: Vec<String> = a.windows.iter().map(|w| w.name.clone()).collect();
        assert!(names.contains(&"prod-us".to_string()), "{names:?}");
        assert!(names.contains(&"prod-eu".to_string()), "{names:?}");
    }

    #[test]
    fn each_spread_window_holds_that_groups_clusters() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 200, 60));
        a.apply(Action::SpreadClusters);

        let us = a.windows.iter().find(|w| w.name == "prod-us").unwrap();
        let open: Vec<String> = us.panes.iter().map(|p| p.instance.clone()).collect();
        assert_eq!(open, vec!["use1-a", "use1-b"]);

        let eu = a.windows.iter().find(|w| w.name == "prod-eu").unwrap();
        let open: Vec<String> = eu.panes.iter().map(|p| p.instance.clone()).collect();
        assert_eq!(open, vec!["usw2-b", "euc1-a"]);
    }

    #[test]
    fn spreading_into_a_tiny_terminal_stops_rather_than_squeezing() {
        let mut a = app();
        a.set_area(Rect::new(0, 0, 20, 6));
        a.apply(Action::SpreadClusters);
        for w in &a.windows {
            for (_, r) in w.rects(Rect::new(0, 0, 20, 6)) {
                assert!(r.width >= MIN_PANE.1 || w.panes.len() == 1, "{r:?}");
            }
        }
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

    // ---- mouse drag ----

    #[test]
    fn a_drag_moves_a_border_of_the_current_window() {
        let area = Rect::new(0, 0, 80, 24);
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let b = a.window().layout.borders(area)[0].clone();

        assert!(a.drag_border(&b.path, b.ratio_at(24)));
        assert_eq!(a.pane_rects(area)[0].width, 24);
        assert_eq!(a.pane_rects(area)[1].width, 56);
    }

    #[test]
    fn a_drag_leaves_the_other_windows_alone() {
        let area = Rect::new(0, 0, 80, 24);
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        a.apply(Action::NewWindow);
        a.apply(Action::Split(Split::Vertical));
        let b = a.window().layout.borders(area)[0].clone();
        a.drag_border(&b.path, b.ratio_at(20));

        assert_eq!(a.pane_rects(area)[0].width, 20);
        a.apply(Action::PrevWindow);
        assert_eq!(a.pane_rects(area)[0].width, 40, "still an even split");
    }

    #[test]
    fn a_drag_on_a_layout_that_has_moved_on_is_ignored() {
        let area = Rect::new(0, 0, 80, 24);
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let b = a.window().layout.borders(area)[0].clone();
        a.apply(Action::ClosePane); // the split the drag refers to is gone
        assert!(!a.drag_border(&b.path, 80));
        assert_eq!(a.pane_rects(area)[0], area);
    }

    #[test]
    fn a_zoomed_pane_cannot_be_dragged() {
        let area = Rect::new(0, 0, 80, 24);
        let mut a = app();
        a.apply(Action::Split(Split::Vertical));
        let b = a.window().layout.borders(area)[0].clone();
        a.apply(Action::ToggleZoom);
        assert!(!a.drag_border(&b.path, b.ratio_at(20)));
        assert_eq!(a.pane_rects(area)[0], area, "the zoom still owns it all");
    }
}
