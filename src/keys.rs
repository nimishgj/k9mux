use crate::layout::{Dir, Split};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Split(Split),
    ClosePane,
    Focus(Dir),
    Resize(Dir),
    FocusNext,
    NextInstance,
    PrevInstance,
    NewWindow,
    RenameWindow,
    NextWindow,
    PrevWindow,
    SelectWindow(usize),
    Restart,
    ToggleHelp,
    ToggleSidebar,
    ToggleZoom,
    ClusterPicker,
    SpreadClusters,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Swallowed by k9mux.
    Do(Action),
    /// Send these bytes to the focused k9s.
    Forward(Vec<u8>),
    /// Prefix seen; the next key is a command.
    Armed,
    /// Nothing to do.
    Ignore,
}

/// Encodes a key press the way a terminal would send it to a child process.
pub fn encode(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let mut out: Vec<u8> = match key.code {
        KeyCode::Char(c) if ctrl => {
            // Control codes: ctrl-a is 0x01, ctrl-z is 0x1a.
            let b = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a' - 1);
            match c {
                'a'..='z' => vec![b],
                ' ' | '@' => vec![0],
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                '^' => vec![0x1e],
                '_' | '?' => vec![0x1f],
                _ => c.to_string().into_bytes(),
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6..=10 => format!("\x1b[{}~", n + 11).into_bytes(),
            11 | 12 => format!("\x1b[{}~", n + 12).into_bytes(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    if alt && !out.is_empty() {
        out.insert(0, 0x1b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn control_keys_are_encoded_for_the_child() {
        assert_eq!(encode(ctrl('c')), vec![0x03]);
        assert_eq!(encode(ctrl('d')), vec![0x04]);
        assert_eq!(encode(ctrl('z')), vec![0x1a]);
        assert_eq!(encode(ctrl('a')), vec![0x01]);
    }

    #[test]
    fn special_keys_use_the_sequences_a_terminal_would_send() {
        assert_eq!(encode(code(KeyCode::Enter)), b"\r");
        assert_eq!(encode(code(KeyCode::Tab)), b"\t");
        assert_eq!(encode(code(KeyCode::Backspace)), vec![0x7f]);
        assert_eq!(encode(code(KeyCode::Esc)), vec![0x1b]);
        assert_eq!(encode(code(KeyCode::Up)), b"\x1b[A");
        assert_eq!(encode(code(KeyCode::Down)), b"\x1b[B");
        assert_eq!(encode(code(KeyCode::Right)), b"\x1b[C");
        assert_eq!(encode(code(KeyCode::Left)), b"\x1b[D");
        assert_eq!(encode(code(KeyCode::Home)), b"\x1b[H");
        assert_eq!(encode(code(KeyCode::End)), b"\x1b[F");
        assert_eq!(encode(code(KeyCode::PageUp)), b"\x1b[5~");
        assert_eq!(encode(code(KeyCode::PageDown)), b"\x1b[6~");
        assert_eq!(encode(code(KeyCode::Delete)), b"\x1b[3~");
        assert_eq!(encode(code(KeyCode::Insert)), b"\x1b[2~");
        assert_eq!(encode(code(KeyCode::BackTab)), b"\x1b[Z");
    }

    #[test]
    fn function_keys_are_encoded() {
        assert_eq!(encode(code(KeyCode::F(1))), b"\x1bOP");
        assert_eq!(encode(code(KeyCode::F(5))), b"\x1b[15~");
    }

    #[test]
    fn alt_prefixes_the_key_with_escape() {
        let alt_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT);
        assert_eq!(encode(alt_x), b"\x1bx");
    }

    #[test]
    fn ordinary_and_multibyte_characters_are_sent_as_typed() {
        assert_eq!(encode(k('j')), b"j");
        assert_eq!(encode(k(':')), b":");
        assert_eq!(encode(k(' ')), b" ");
        assert_eq!(encode(k('é')), "é".as_bytes());
    }
}
