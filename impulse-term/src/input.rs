//! Input handler — translates egui keyboard events into terminal escape sequences.
//!
//! Maps egui `Key` + `Modifiers` to VT100/xterm escape bytes that are written
//! to the PTY master, where the child process reads them as terminal input.

use eframe::egui;

/// Convert an egui key event to PTY-compatible bytes.
///
/// Returns `None` if the key shouldn't produce terminal output (e.g., modifier-only
/// keys, or shortcuts handled by the GUI itself).
///
/// `app_cursor` — whether the terminal is in application cursor mode (DECCKM).
pub fn key_to_pty_bytes(
    key: &egui::Key,
    modifiers: &egui::Modifiers,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    // Ctrl+Shift combinations — reserved for GUI shortcuts.
    if modifiers.ctrl && modifiers.shift {
        return None;
    }

    // Ctrl+letter → control character (0x01-0x1A).
    if modifiers.ctrl {
        if let Some(byte) = ctrl_key_byte(key) {
            if modifiers.alt {
                return Some(vec![0x1B, byte]);
            }
            return Some(vec![byte]);
        }
    }

    // Alt wraps the key in an ESC prefix.
    if modifiers.alt && !modifiers.ctrl {
        if let Some(ch) = key_to_char(key) {
            let mut bytes = vec![0x1B];
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            bytes.extend_from_slice(s.as_bytes());
            return Some(bytes);
        }
    }

    // Special keys.
    match key {
        egui::Key::Enter => return Some(vec![0x0D]),
        egui::Key::Tab => return Some(vec![0x09]),
        egui::Key::Backspace => return Some(vec![0x7F]),
        egui::Key::Escape => return Some(vec![0x1B]),
        egui::Key::Delete => return Some(b"\x1b[3~".to_vec()),

        // Arrow keys — different sequences for normal vs application cursor mode.
        egui::Key::ArrowUp => return Some(arrow_bytes(b'A', modifiers, app_cursor)),
        egui::Key::ArrowDown => return Some(arrow_bytes(b'B', modifiers, app_cursor)),
        egui::Key::ArrowRight => return Some(arrow_bytes(b'C', modifiers, app_cursor)),
        egui::Key::ArrowLeft => return Some(arrow_bytes(b'D', modifiers, app_cursor)),

        egui::Key::Home => return Some(b"\x1b[H".to_vec()),
        egui::Key::End => return Some(b"\x1b[F".to_vec()),
        egui::Key::PageUp => return Some(b"\x1b[5~".to_vec()),
        egui::Key::PageDown => return Some(b"\x1b[6~".to_vec()),
        egui::Key::Insert => return Some(b"\x1b[2~".to_vec()),

        // Function keys.
        egui::Key::F1 => return Some(b"\x1bOP".to_vec()),
        egui::Key::F2 => return Some(b"\x1bOQ".to_vec()),
        egui::Key::F3 => return Some(b"\x1bOR".to_vec()),
        egui::Key::F4 => return Some(b"\x1bOS".to_vec()),
        egui::Key::F5 => return Some(b"\x1b[15~".to_vec()),
        egui::Key::F6 => return Some(b"\x1b[17~".to_vec()),
        egui::Key::F7 => return Some(b"\x1b[18~".to_vec()),
        egui::Key::F8 => return Some(b"\x1b[19~".to_vec()),
        egui::Key::F9 => return Some(b"\x1b[20~".to_vec()),
        egui::Key::F10 => return Some(b"\x1b[21~".to_vec()),
        egui::Key::F11 => return Some(b"\x1b[23~".to_vec()),
        egui::Key::F12 => return Some(b"\x1b[24~".to_vec()),

        _ => {}
    }

    // Printable characters are handled exclusively by Event::Text in panel.rs.
    // Processing them here would cause doubled keystrokes since egui fires
    // both Event::Key and Event::Text for every printable keypress.
    None
}

/// Wrap pasted text in bracketed paste escape sequences.
pub fn bracketed_paste(text: &str) -> Vec<u8> {
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

/// Map Ctrl+key to the corresponding control character byte.
fn ctrl_key_byte(key: &egui::Key) -> Option<u8> {
    match key {
        egui::Key::A => Some(0x01),
        egui::Key::B => Some(0x02),
        egui::Key::C => Some(0x03),
        egui::Key::D => Some(0x04),
        egui::Key::E => Some(0x05),
        egui::Key::F => Some(0x06),
        egui::Key::G => Some(0x07),
        egui::Key::H => Some(0x08),
        egui::Key::I => Some(0x09),
        egui::Key::J => Some(0x0A),
        egui::Key::K => Some(0x0B),
        egui::Key::L => Some(0x0C),
        egui::Key::M => Some(0x0D),
        egui::Key::N => Some(0x0E),
        egui::Key::O => Some(0x0F),
        egui::Key::P => Some(0x10),
        egui::Key::Q => Some(0x11),
        egui::Key::R => Some(0x12),
        egui::Key::S => Some(0x13),
        egui::Key::T => Some(0x14),
        egui::Key::U => Some(0x15),
        egui::Key::V => Some(0x16),
        egui::Key::W => Some(0x17),
        egui::Key::X => Some(0x18),
        egui::Key::Y => Some(0x19),
        egui::Key::Z => Some(0x1A),
        _ => None,
    }
}

/// Generate arrow key bytes with optional modifier encoding.
fn arrow_bytes(direction: u8, modifiers: &egui::Modifiers, app_cursor: bool) -> Vec<u8> {
    // Modified arrows: ESC[1;{mod}X
    let modifier_code = modifier_param(modifiers);
    if modifier_code > 1 {
        return format!("\x1b[1;{}{}", modifier_code, direction as char).into_bytes();
    }

    // Unmodified arrows.
    if app_cursor {
        vec![0x1B, b'O', direction]
    } else {
        vec![0x1B, b'[', direction]
    }
}

/// xterm modifier parameter: 1=none, 2=Shift, 3=Alt, 5=Ctrl, etc.
fn modifier_param(modifiers: &egui::Modifiers) -> u8 {
    let mut code: u8 = 1;
    if modifiers.shift {
        code += 1;
    }
    if modifiers.alt {
        code += 2;
    }
    if modifiers.ctrl {
        code += 4;
    }
    code
}

/// Map an egui Key to a printable character (lowercase).
///
/// Used ONLY for Alt+key combos (ESC prefix) — NOT for direct printable output.
/// Direct printable input is handled by Event::Text in panel.rs.
fn key_to_char(key: &egui::Key) -> Option<char> {
    match key {
        egui::Key::A => Some('a'),
        egui::Key::B => Some('b'),
        egui::Key::C => Some('c'),
        egui::Key::D => Some('d'),
        egui::Key::E => Some('e'),
        egui::Key::F => Some('f'),
        egui::Key::G => Some('g'),
        egui::Key::H => Some('h'),
        egui::Key::I => Some('i'),
        egui::Key::J => Some('j'),
        egui::Key::K => Some('k'),
        egui::Key::L => Some('l'),
        egui::Key::M => Some('m'),
        egui::Key::N => Some('n'),
        egui::Key::O => Some('o'),
        egui::Key::P => Some('p'),
        egui::Key::Q => Some('q'),
        egui::Key::R => Some('r'),
        egui::Key::S => Some('s'),
        egui::Key::T => Some('t'),
        egui::Key::U => Some('u'),
        egui::Key::V => Some('v'),
        egui::Key::W => Some('w'),
        egui::Key::X => Some('x'),
        egui::Key::Y => Some('y'),
        egui::Key::Z => Some('z'),
        egui::Key::Num0 => Some('0'),
        egui::Key::Num1 => Some('1'),
        egui::Key::Num2 => Some('2'),
        egui::Key::Num3 => Some('3'),
        egui::Key::Num4 => Some('4'),
        egui::Key::Num5 => Some('5'),
        egui::Key::Num6 => Some('6'),
        egui::Key::Num7 => Some('7'),
        egui::Key::Num8 => Some('8'),
        egui::Key::Num9 => Some('9'),
        egui::Key::Space => Some(' '),
        egui::Key::Minus => Some('-'),
        egui::Key::Plus => Some('='),
        egui::Key::OpenBracket => Some('['),
        egui::Key::CloseBracket => Some(']'),
        egui::Key::Backslash => Some('\\'),
        egui::Key::Semicolon => Some(';'),
        egui::Key::Colon => Some(':'),
        egui::Key::Comma => Some(','),
        egui::Key::Period => Some('.'),
        egui::Key::Slash => Some('/'),
        egui::Key::Backtick => Some('`'),
        egui::Key::Equals => Some('='),
        _ => None,
    }
}

// NOTE: shift_char() was removed — shift handling for printable characters
// is done by the OS/egui via Event::Text, not by our key mapping.

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mods() -> egui::Modifiers {
        egui::Modifiers::NONE
    }

    fn ctrl() -> egui::Modifiers {
        egui::Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    fn alt() -> egui::Modifiers {
        egui::Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    fn shift() -> egui::Modifiers {
        egui::Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_enter() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Enter, &no_mods(), false),
            Some(vec![0x0D])
        );
    }

    #[test]
    fn test_tab() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Tab, &no_mods(), false),
            Some(vec![0x09])
        );
    }

    #[test]
    fn test_backspace() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Backspace, &no_mods(), false),
            Some(vec![0x7F])
        );
    }

    #[test]
    fn test_escape() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Escape, &no_mods(), false),
            Some(vec![0x1B])
        );
    }

    #[test]
    fn test_ctrl_c() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::C, &ctrl(), false),
            Some(vec![0x03])
        );
    }

    #[test]
    fn test_ctrl_d() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::D, &ctrl(), false),
            Some(vec![0x04])
        );
    }

    #[test]
    fn test_arrow_normal_mode() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowUp, &no_mods(), false),
            Some(vec![0x1B, b'[', b'A'])
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowDown, &no_mods(), false),
            Some(vec![0x1B, b'[', b'B'])
        );
    }

    #[test]
    fn test_arrow_app_cursor_mode() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowUp, &no_mods(), true),
            Some(vec![0x1B, b'O', b'A'])
        );
    }

    #[test]
    fn test_printable_char_returns_none() {
        // Printable characters must return None from key_to_pty_bytes —
        // they are handled by Event::Text in panel.rs to avoid doubled input.
        assert_eq!(key_to_pty_bytes(&egui::Key::A, &no_mods(), false), None);
        assert_eq!(key_to_pty_bytes(&egui::Key::A, &shift(), false), None);
        assert_eq!(key_to_pty_bytes(&egui::Key::Num5, &no_mods(), false), None);
    }

    #[test]
    fn test_alt_char_still_works() {
        // Alt+key produces ESC prefix — this is a modifier combo, not printable text,
        // so it must still be handled in key_to_pty_bytes.
        assert_eq!(
            key_to_pty_bytes(&egui::Key::A, &alt(), false),
            Some(vec![0x1B, b'a'])
        );
    }

    #[test]
    fn test_ctrl_shift_reserved() {
        let mods = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(key_to_pty_bytes(&egui::Key::C, &mods, false), None);
    }

    #[test]
    fn test_bracketed_paste() {
        let bytes = bracketed_paste("hello");
        assert!(bytes.starts_with(b"\x1b[200~"));
        assert!(bytes.ends_with(b"\x1b[201~"));
        assert!(bytes.windows(5).any(|w| w == b"hello"));
    }

    #[test]
    fn test_function_keys() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::F1, &no_mods(), false),
            Some(b"\x1bOP".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::F12, &no_mods(), false),
            Some(b"\x1b[24~".to_vec())
        );
    }
}
