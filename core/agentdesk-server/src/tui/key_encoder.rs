//! Terminal keyboard input encoder.
//! Translates Crossterm `KeyEvent` structures into raw POSIX terminal byte sequences.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Convert a Crossterm `KeyEvent` into the raw byte sequence expected by a PTY.
pub fn encode_key_event(key: KeyEvent) -> Option<Vec<u8>> {
    // Ignore key release events
    if key.kind == KeyEventKind::Release {
        return None;
    }

    let mut bytes = Vec::new();

    // 1. Control key combinations
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char(c) => {
                let lower = c.to_ascii_lowercase();
                if (b'a'..=b'z').contains(&(lower as u8)) {
                    bytes.push(lower as u8 - b'a' + 1);
                } else if c == ' ' || c == '@' {
                    bytes.push(0x00);
                } else if c == '[' {
                    bytes.push(0x1B);
                } else if c == '\\' {
                    bytes.push(0x1C);
                } else if c == ']' {
                    bytes.push(0x1D);
                } else if c == '^' {
                    bytes.push(0x1E);
                } else if c == '_' {
                    bytes.push(0x1F);
                } else if c == '?' {
                    bytes.push(0x7F);
                } else {
                    bytes.extend_from_slice(c.to_string().as_bytes());
                }
            }
            KeyCode::Left => bytes.extend_from_slice(b"\x1b[1;5D"),
            KeyCode::Right => bytes.extend_from_slice(b"\x1b[1;5C"),
            KeyCode::Up => bytes.extend_from_slice(b"\x1b[1;5A"),
            KeyCode::Down => bytes.extend_from_slice(b"\x1b[1;5B"),
            _ => return None,
        }
        return Some(bytes);
    }

    // 2. Alt / Meta key combinations
    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.push(0x1B);
        match key.code {
            KeyCode::Char(c) => bytes.extend_from_slice(c.to_string().as_bytes()),
            KeyCode::Enter => bytes.push(b'\r'),
            KeyCode::Backspace => bytes.push(0x7F),
            _ => return None,
        }
        return Some(bytes);
    }

    // 3. Normal / un-modified keys
    match key.code {
        KeyCode::Char(c) => {
            bytes.extend_from_slice(c.to_string().as_bytes());
        }
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Backspace => bytes.push(0x7F),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::BackTab => bytes.extend_from_slice(b"\x1b[Z"),
        KeyCode::Esc => bytes.push(0x1B),
        KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(1) => bytes.extend_from_slice(b"\x1bOP"),
        KeyCode::F(2) => bytes.extend_from_slice(b"\x1bOQ"),
        KeyCode::F(3) => bytes.extend_from_slice(b"\x1bOR"),
        KeyCode::F(4) => bytes.extend_from_slice(b"\x1bOS"),
        KeyCode::F(5) => bytes.extend_from_slice(b"\x1b[15~"),
        KeyCode::F(6) => bytes.extend_from_slice(b"\x1b[17~"),
        KeyCode::F(7) => bytes.extend_from_slice(b"\x1b[18~"),
        KeyCode::F(8) => bytes.extend_from_slice(b"\x1b[19~"),
        KeyCode::F(9) => bytes.extend_from_slice(b"\x1b[20~"),
        KeyCode::F(10) => bytes.extend_from_slice(b"\x1b[21~"),
        KeyCode::F(11) => bytes.extend_from_slice(b"\x1b[23~"),
        KeyCode::F(12) => bytes.extend_from_slice(b"\x1b[24~"),
        _ => return None,
    }

    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_encode_ascii_and_symbols() {
        assert_eq!(encode_key_event(make_key(KeyCode::Char('a'), KeyModifiers::NONE)), Some(vec![b'a']));
        assert_eq!(encode_key_event(make_key(KeyCode::Char('Z'), KeyModifiers::SHIFT)), Some(vec![b'Z']));
        assert_eq!(encode_key_event(make_key(KeyCode::Char('/'), KeyModifiers::NONE)), Some(vec![b'/']));
    }

    #[test]
    fn test_encode_control_keys() {
        // Ctrl+C -> 0x03 (SIGINT)
        assert_eq!(encode_key_event(make_key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(vec![0x03]));
        // Ctrl+D -> 0x04 (EOF)
        assert_eq!(encode_key_event(make_key(KeyCode::Char('d'), KeyModifiers::CONTROL)), Some(vec![0x04]));
        // Ctrl+Z -> 0x1A (SIGTSTP)
        assert_eq!(encode_key_event(make_key(KeyCode::Char('z'), KeyModifiers::CONTROL)), Some(vec![0x1A]));
        // Ctrl+L -> 0x0C (Clear screen)
        assert_eq!(encode_key_event(make_key(KeyCode::Char('l'), KeyModifiers::CONTROL)), Some(vec![0x0C]));
    }

    #[test]
    fn test_encode_navigation_keys() {
        assert_eq!(encode_key_event(make_key(KeyCode::Enter, KeyModifiers::NONE)), Some(vec![b'\r']));
        assert_eq!(encode_key_event(make_key(KeyCode::Backspace, KeyModifiers::NONE)), Some(vec![0x7F]));
        assert_eq!(encode_key_event(make_key(KeyCode::Tab, KeyModifiers::NONE)), Some(vec![b'\t']));
        assert_eq!(encode_key_event(make_key(KeyCode::Esc, KeyModifiers::NONE)), Some(vec![0x1B]));
        assert_eq!(encode_key_event(make_key(KeyCode::Up, KeyModifiers::NONE)), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key_event(make_key(KeyCode::Down, KeyModifiers::NONE)), Some(b"\x1b[B".to_vec()));
        assert_eq!(encode_key_event(make_key(KeyCode::Right, KeyModifiers::NONE)), Some(b"\x1b[C".to_vec()));
        assert_eq!(encode_key_event(make_key(KeyCode::Left, KeyModifiers::NONE)), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn test_encode_function_keys() {
        assert_eq!(encode_key_event(make_key(KeyCode::F(1), KeyModifiers::NONE)), Some(b"\x1bOP".to_vec()));
        assert_eq!(encode_key_event(make_key(KeyCode::F(10), KeyModifiers::NONE)), Some(b"\x1b[21~".to_vec()));
    }
}
