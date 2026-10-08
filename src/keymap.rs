use crate::keys::{Action, Outcome};
use crate::layout::Dir;
use crate::layout::Split;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One key, as written in the config: `M-space`, `C-a`, `Tab`, `%`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

/// What the option key produces on a US macOS layout when it is not configured
/// as meta. Without this, `M-h` would simply not arrive.
pub fn macos_option_glyph(c: char) -> Option<char> {
    Some(match c.to_ascii_lowercase() {
        ' ' => '\u{00a0}',
        'a' => 'å',
        'b' => '∫',
        'c' => 'ç',
        'd' => '∂',
        'e' => '´',
        'f' => 'ƒ',
        'g' => '©',
        'h' => '˙',
        'i' => 'ˆ',
        'j' => '∆',
        'k' => '˚',
        'l' => '¬',
        'm' => 'µ',
        'n' => '˜',
        'o' => 'ø',
        'p' => 'π',
        'q' => 'œ',
        'r' => '®',
        's' => 'ß',
        't' => '†',
        'u' => '¨',
        'v' => '√',
        'w' => '∑',
        'x' => '≈',
        'y' => '¥',
        'z' => 'Ω',
        _ => return None,
    })
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name.to_ascii_lowercase().as_str() {
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "btab" | "backtab" => KeyCode::BackTab,
        "enter" | "return" => KeyCode::Enter,
        "escape" | "esc" => KeyCode::Esc,
        "bspace" | "backspace" => KeyCode::Backspace,
        "delete" | "dc" => KeyCode::Delete,
        "insert" | "ic" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "ppage" => KeyCode::PageUp,
        "pagedown" | "npage" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        other => {
            let n: u8 = other.strip_prefix('f')?.parse().ok()?;
            if (1..=12).contains(&n) {
                KeyCode::F(n)
            } else {
                return None;
            }
        }
    })
}

impl KeySpec {
    pub fn parse(text: &str) -> Result<KeySpec, String> {
        let mut rest = text;
        let (mut ctrl, mut alt) = (false, false);
        loop {
            // Modifier prefixes only count when something follows them.
            let stripped = match rest {
                r if r.len() > 2 && (r.starts_with("C-") || r.starts_with("c-")) => {
                    ctrl = true;
                    &r[2..]
                }
                r if r.len() > 2 && (r.starts_with("M-") || r.starts_with("m-")) => {
                    alt = true;
                    &r[2..]
                }
                _ => break,
            };
            rest = stripped;
        }

        if rest.is_empty() {
            return Err(format!("empty key: {text:?}"));
        }
        let code = if rest.chars().count() == 1 {
            KeyCode::Char(rest.chars().next().unwrap())
        } else {
            named_key(rest).ok_or_else(|| format!("unknown key: {text:?}"))?
        };
        Ok(KeySpec { code, ctrl, alt })
    }

    /// True if this key press is the one described.
    pub fn matches(&self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.code == key.code && self.ctrl == ctrl && self.alt == alt {
            return true;
        }
        // An alt binding also answers to the glyph macOS sends for it.
        if self.alt
            && !self.ctrl
            && !alt
            && !ctrl
            && let (KeyCode::Char(want), KeyCode::Char(got)) = (self.code, key.code)
        {
            return macos_option_glyph(want) == Some(got);
        }
        false
    }

    /// How to write this key in help text.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("C-");
        }
        if self.alt {
            out.push_str("M-");
        }
        out.push_str(&match self.code {
            KeyCode::Char(' ') => "space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "BTab".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Escape".to_string(),
            KeyCode::Backspace => "BSpace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Insert => "Insert".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::F(n) => format!("F{n}"),
            other => format!("{other:?}"),
        });
        out
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    pub prefix: KeySpec,
    /// Bindings reached after the prefix.
    pub prefixed: Vec<(KeySpec, Action)>,
    /// Bindings that need no prefix (`bind -n`).
    pub root: Vec<(KeySpec, Action)>,
}

pub fn parse_command(text: &str) -> Option<Action> {
    Some(match text.trim() {
        "split-vertical" => Action::Split(Split::Vertical),
        "split-horizontal" => Action::Split(Split::Horizontal),
        "close-pane" => Action::ClosePane,
        "focus-left" => Action::Focus(Dir::Left),
        "focus-right" => Action::Focus(Dir::Right),
        "focus-up" => Action::Focus(Dir::Up),
        "focus-down" => Action::Focus(Dir::Down),
        "focus-next" => Action::FocusNext,
        "resize-left" => Action::Resize(Dir::Left),
        "resize-right" => Action::Resize(Dir::Right),
        "resize-up" => Action::Resize(Dir::Up),
        "resize-down" => Action::Resize(Dir::Down),
        "next-cluster" => Action::NextInstance,
        "prev-cluster" => Action::PrevInstance,
        "new-window" => Action::NewWindow,
        "rename-window" => Action::RenameWindow,
        "next-window" => Action::NextWindow,
        "prev-window" => Action::PrevWindow,
        "restart" => Action::Restart,
        "help" => Action::ToggleHelp,
        "toggle-sidebar" => Action::ToggleSidebar,
        "zoom" => Action::ToggleZoom,
        "choose-cluster" => Action::ClusterPicker,
        "spread-clusters" => Action::SpreadClusters,
        "quit" => Action::Quit,
        other => {
            // select-window-0 .. select-window-9
            let n = other.strip_prefix("select-window-")?.parse().ok()?;
            Action::SelectWindow(n)
        }
    })
}

/// The name a command is written under in the config.
pub fn command_name(action: &Action) -> &'static str {
    match action {
        Action::Split(Split::Vertical) => "split-vertical",
        Action::Split(Split::Horizontal) => "split-horizontal",
        Action::ClosePane => "close-pane",
        Action::Focus(Dir::Left) => "focus-left",
        Action::Focus(Dir::Right) => "focus-right",
        Action::Focus(Dir::Up) => "focus-up",
        Action::Focus(Dir::Down) => "focus-down",
        Action::FocusNext => "focus-next",
        Action::Resize(Dir::Left) => "resize-left",
        Action::Resize(Dir::Right) => "resize-right",
        Action::Resize(Dir::Up) => "resize-up",
        Action::Resize(Dir::Down) => "resize-down",
        Action::NextInstance => "next-cluster",
        Action::PrevInstance => "prev-cluster",
        Action::NewWindow => "new-window",
        Action::RenameWindow => "rename-window",
        Action::NextWindow => "next-window",
        Action::PrevWindow => "prev-window",
        Action::SelectWindow(_) => "select-window",
        Action::Restart => "restart",
        Action::ToggleHelp => "help",
        Action::ToggleSidebar => "toggle-sidebar",
        Action::ToggleZoom => "zoom",
        Action::ClusterPicker => "choose-cluster",
        Action::SpreadClusters => "spread-clusters",
        Action::Quit => "quit",
    }
}

/// The bindings k9mux ships with, in config syntax.
pub const DEFAULTS: &str = "\
set prefix M-space

bind c   new-window
bind ,   rename-window
bind n   next-window
bind p   prev-window
bind 1   select-window-0
bind 2   select-window-1
bind 3   select-window-2
bind 4   select-window-3

bind '\"' split-horizontal
bind %   split-vertical
bind x   close-pane
bind Tab focus-next
bind H   resize-left
bind J   resize-down
bind K   resize-up
bind L   resize-right
bind '<' prev-cluster
bind '>' next-cluster
bind z   zoom
bind w   choose-cluster
bind g   spread-clusters
bind r   restart
bind e   toggle-sidebar
bind ?   help
bind q   quit

bind -n M-h focus-left
bind -n M-j focus-down
bind -n M-k focus-up
bind -n M-l focus-right
";

impl Default for Keymap {
    fn default() -> Keymap {
        let mut km = Keymap {
            prefix: KeySpec::parse("M-space").expect("valid default prefix"),
            prefixed: Vec::new(),
            root: Vec::new(),
        };
        for (key, cmd) in [
            ("c", "new-window"),
            (",", "rename-window"),
            ("n", "next-window"),
            ("p", "prev-window"),
            ("1", "select-window-0"),
            ("2", "select-window-1"),
            ("3", "select-window-2"),
            ("4", "select-window-3"),
            ("\"", "split-horizontal"),
            ("%", "split-vertical"),
            ("x", "close-pane"),
            ("Tab", "focus-next"),
            ("H", "resize-left"),
            ("J", "resize-down"),
            ("K", "resize-up"),
            ("L", "resize-right"),
            ("<", "prev-cluster"),
            (">", "next-cluster"),
            ("z", "zoom"),
            ("w", "choose-cluster"),
            ("g", "spread-clusters"),
            ("r", "restart"),
            ("e", "toggle-sidebar"),
            ("?", "help"),
            ("q", "quit"),
        ] {
            km.bind(
                KeySpec::parse(key).expect("valid default key"),
                parse_command(cmd).expect("valid default command"),
                false,
            );
        }
        for (key, cmd) in [
            ("M-h", "focus-left"),
            ("M-j", "focus-down"),
            ("M-k", "focus-up"),
            ("M-l", "focus-right"),
        ] {
            km.bind(
                KeySpec::parse(key).expect("valid default key"),
                parse_command(cmd).expect("valid default command"),
                true,
            );
        }
        km
    }
}

impl Keymap {
    pub fn bind(&mut self, key: KeySpec, action: Action, root: bool) {
        let table = if root {
            &mut self.root
        } else {
            &mut self.prefixed
        };
        table.retain(|(k, _)| *k != key);
        table.push((key, action));
    }
    pub fn unbind(&mut self, key: KeySpec, root: bool) {
        let table = if root {
            &mut self.root
        } else {
            &mut self.prefixed
        };
        table.retain(|(k, _)| *k != key);
    }
    /// Decides what a key press means. `armed` is true just after the prefix.
    pub fn resolve(&self, key: KeyEvent, armed: bool) -> Outcome {
        if armed {
            // A second prefix means "I meant it literally".
            if self.prefix.matches(key) {
                return Outcome::Forward(crate::keys::encode(key));
            }
            return match self.prefixed.iter().find(|(k, _)| k.matches(key)) {
                Some((_, action)) => Outcome::Do(action.clone()),
                None => Outcome::Ignore,
            };
        }
        if self.prefix.matches(key) {
            return Outcome::Armed;
        }
        match self.root.iter().find(|(k, _)| k.matches(key)) {
            Some((_, action)) => Outcome::Do(action.clone()),
            None => Outcome::Forward(crate::keys::encode(key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn a_plain_character_parses() {
        let s = KeySpec::parse("%").unwrap();
        assert_eq!(s.code, KeyCode::Char('%'));
        assert!(!s.ctrl && !s.alt);
        assert!(s.matches(k('%')));
    }

    #[test]
    fn modifiers_parse_the_way_tmux_writes_them() {
        let m = KeySpec::parse("M-space").unwrap();
        assert_eq!(m.code, KeyCode::Char(' '));
        assert!(m.alt && !m.ctrl);

        let c = KeySpec::parse("C-a").unwrap();
        assert_eq!(c.code, KeyCode::Char('a'));
        assert!(c.ctrl && !c.alt);

        let both = KeySpec::parse("C-M-x").unwrap();
        assert!(both.ctrl && both.alt);
    }

    #[test]
    fn named_keys_parse_case_insensitively() {
        assert_eq!(KeySpec::parse("Tab").unwrap().code, KeyCode::Tab);
        assert_eq!(KeySpec::parse("tab").unwrap().code, KeyCode::Tab);
        assert_eq!(KeySpec::parse("Enter").unwrap().code, KeyCode::Enter);
        assert_eq!(KeySpec::parse("Escape").unwrap().code, KeyCode::Esc);
        assert_eq!(KeySpec::parse("Space").unwrap().code, KeyCode::Char(' '));
        assert_eq!(KeySpec::parse("BSpace").unwrap().code, KeyCode::Backspace);
        assert_eq!(KeySpec::parse("Left").unwrap().code, KeyCode::Left);
        assert_eq!(KeySpec::parse("F5").unwrap().code, KeyCode::F(5));
    }

    #[test]
    fn nonsense_keys_are_rejected() {
        assert!(KeySpec::parse("").is_err());
        assert!(KeySpec::parse("Nope").is_err());
        assert!(KeySpec::parse("M-").is_err());
    }

    #[test]
    fn a_key_matches_only_with_its_own_modifiers() {
        let m = KeySpec::parse("M-h").unwrap();
        assert!(m.matches(alt('h')));
        assert!(!m.matches(k('h')), "plain h must still reach k9s");
        assert!(!m.matches(ctrl('h')));
    }

    #[test]
    fn an_alt_binding_also_matches_the_macos_glyph() {
        assert!(KeySpec::parse("M-h").unwrap().matches(k('˙')));
        assert!(KeySpec::parse("M-l").unwrap().matches(k('¬')));
        assert!(KeySpec::parse("M-space").unwrap().matches(k('\u{a0}')));
    }

    #[test]
    fn a_plain_binding_does_not_match_the_glyph() {
        assert!(!KeySpec::parse("h").unwrap().matches(k('˙')));
    }

    #[test]
    fn shift_is_ignored_because_the_character_already_carries_it() {
        let pct = KeySpec::parse("%").unwrap();
        assert!(pct.matches(KeyEvent::new(KeyCode::Char('%'), KeyModifiers::SHIFT)));
    }

    #[test]
    fn keys_are_labelled_the_way_they_were_written() {
        assert_eq!(KeySpec::parse("M-space").unwrap().label(), "M-space");
        assert_eq!(KeySpec::parse("C-a").unwrap().label(), "C-a");
        assert_eq!(KeySpec::parse("%").unwrap().label(), "%");
        assert_eq!(KeySpec::parse("Tab").unwrap().label(), "Tab");
    }

    #[test]
    fn every_command_name_round_trips() {
        for a in [
            Action::Quit,
            Action::Split(Split::Vertical),
            Action::Split(Split::Horizontal),
            Action::ClosePane,
            Action::Focus(Dir::Left),
            Action::Focus(Dir::Right),
            Action::Focus(Dir::Up),
            Action::Focus(Dir::Down),
            Action::FocusNext,
            Action::Resize(Dir::Left),
            Action::Resize(Dir::Right),
            Action::Resize(Dir::Up),
            Action::Resize(Dir::Down),
            Action::NextInstance,
            Action::PrevInstance,
            Action::NewWindow,
            Action::RenameWindow,
            Action::NextWindow,
            Action::PrevWindow,
            Action::Restart,
            Action::ToggleHelp,
            Action::ToggleSidebar,
            Action::ToggleZoom,
            Action::ClusterPicker,
            Action::SpreadClusters,
        ] {
            let name = command_name(&a);
            assert_eq!(parse_command(name), Some(a.clone()), "{name}");
        }
    }

    #[test]
    fn unknown_commands_are_rejected() {
        assert_eq!(parse_command("do-a-barrel-roll"), None);
    }

    #[test]
    fn the_defaults_are_the_documented_bindings() {
        let km = Keymap::default();
        assert!(km.prefix.matches(alt(' ')));

        assert_eq!(km.resolve(alt(' '), false), Outcome::Armed);
        assert_eq!(
            km.resolve(k('"'), true),
            Outcome::Do(Action::Split(Split::Horizontal))
        );
        assert_eq!(
            km.resolve(k('%'), true),
            Outcome::Do(Action::Split(Split::Vertical))
        );
        assert_eq!(km.resolve(k('x'), true), Outcome::Do(Action::ClosePane));
        assert_eq!(km.resolve(k('e'), true), Outcome::Do(Action::ToggleSidebar));
        assert_eq!(km.resolve(k('z'), true), Outcome::Do(Action::ToggleZoom));
        assert_eq!(
            km.resolve(k('L'), true),
            Outcome::Do(Action::Resize(Dir::Right))
        );
        assert_eq!(
            km.resolve(k('J'), true),
            Outcome::Do(Action::Resize(Dir::Down))
        );
        assert_eq!(km.resolve(k('w'), true), Outcome::Do(Action::ClusterPicker));
        assert_eq!(
            km.resolve(k('g'), true),
            Outcome::Do(Action::SpreadClusters)
        );
        assert_eq!(km.resolve(k('c'), true), Outcome::Do(Action::NewWindow));
        assert_eq!(km.resolve(k(','), true), Outcome::Do(Action::RenameWindow));
        assert_eq!(km.resolve(k('n'), true), Outcome::Do(Action::NextWindow));
        assert_eq!(km.resolve(k('p'), true), Outcome::Do(Action::PrevWindow));
        assert_eq!(km.resolve(k('>'), true), Outcome::Do(Action::NextInstance));
        assert_eq!(
            km.resolve(k('1'), true),
            Outcome::Do(Action::SelectWindow(0))
        );
        assert_eq!(
            km.resolve(alt('h'), false),
            Outcome::Do(Action::Focus(Dir::Left))
        );
    }

    #[test]
    fn unbound_keys_go_to_k9s() {
        let km = Keymap::default();
        assert_eq!(km.resolve(k('j'), false), Outcome::Forward(b"j".to_vec()));
        assert_eq!(km.resolve(ctrl('a'), false), Outcome::Forward(vec![0x01]));
    }

    #[test]
    fn an_unbound_key_after_the_prefix_is_dropped() {
        assert_eq!(Keymap::default().resolve(k('Z'), true), Outcome::Ignore);
    }

    #[test]
    fn the_prefix_twice_forwards_it() {
        let km = Keymap::default();
        assert_eq!(
            km.resolve(alt(' '), true),
            Outcome::Forward(b"\x1b ".to_vec())
        );
    }

    #[test]
    fn a_rebound_prefix_takes_over() {
        let km = Keymap {
            prefix: KeySpec::parse("C-b").unwrap(),
            ..Keymap::default()
        };
        assert_eq!(km.resolve(ctrl('b'), false), Outcome::Armed);
        assert_eq!(
            km.resolve(alt(' '), false),
            Outcome::Forward(b"\x1b ".to_vec()),
            "the old prefix becomes an ordinary key"
        );
    }

    #[test]
    fn binding_replaces_whatever_that_key_did() {
        let mut km = Keymap::default();
        km.bind(KeySpec::parse("s").unwrap(), Action::ClosePane, false);
        assert_eq!(km.resolve(k('s'), true), Outcome::Do(Action::ClosePane));

        km.bind(KeySpec::parse("s").unwrap(), Action::Quit, false);
        assert_eq!(km.resolve(k('s'), true), Outcome::Do(Action::Quit));
    }

    #[test]
    fn unbinding_hands_the_key_back_to_k9s() {
        let mut km = Keymap::default();
        km.unbind(KeySpec::parse("M-h").unwrap(), true);
        assert_eq!(
            km.resolve(alt('h'), false),
            Outcome::Forward(b"\x1bh".to_vec())
        );
    }

    #[test]
    fn root_bindings_win_over_forwarding_but_not_over_the_prefix() {
        let mut km = Keymap::default();
        km.bind(KeySpec::parse("M-space").unwrap(), Action::Quit, true);
        // The prefix is checked first, so it still arms.
        assert_eq!(km.resolve(alt(' '), false), Outcome::Armed);
    }
}
