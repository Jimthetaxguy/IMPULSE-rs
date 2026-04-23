//! Toolkit-neutral keyboard input → PTY bytes.
//!
//! Each renderer (egui, Dioxus, ratatui) translates its toolkit's key event
//! type into `TermKey` + `TermModifiers`, then calls `key_to_pty_bytes` to
//! get the bytes to write to the PTY master.
//!
//! Why this lives here, not in each renderer crate:
//! - The escape-sequence rules (xterm modifier encoding, app-cursor mode,
//!   bold-bright promotion of Ctrl+letter to control bytes 0x01–0x1A) are
//!   universal and complex; duplicating them across renderer crates is a
//!   recipe for renderer-specific bugs.
//! - Tests cover the byte sequences once, and every renderer benefits.
//!
//! Renderer crates only need to map their toolkit's `Key` enum to `TermKey`
//! — a mechanical, ~50-line lookup that is hard to get wrong.

/// Toolkit-neutral key identifier. Renderers map their toolkit's key type
/// into this enum before calling `key_to_pty_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermKey {
    // ---- Letters (always lowercase here; Shift handled separately) ----
    Letter(char),
    // ---- Digits ----
    Digit(char),
    // ---- Whitespace / control ----
    Enter,
    Tab,
    Backspace,
    Escape,
    Delete,
    Space,
    // ---- Cursor navigation ----
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    // ---- Function keys ----
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // ---- Punctuation / symbols (raw character) ----
    Char(char),
}

impl TermKey {
    /// Returns the lowercase letter character if this is a `Letter`.
    pub fn as_letter(self) -> Option<char> {
        match self {
            TermKey::Letter(c) => Some(c),
            _ => None,
        }
    }

    /// Returns the printable character used for Alt+key combos and similar.
    /// Special keys (Enter, ArrowUp, etc.) return `None`.
    pub fn as_printable(self) -> Option<char> {
        match self {
            TermKey::Letter(c) | TermKey::Digit(c) | TermKey::Char(c) => Some(c),
            TermKey::Space => Some(' '),
            _ => None,
        }
    }
}

/// Toolkit-neutral keyboard modifier state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TermModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// macOS Cmd / Windows Super. Currently unused by the byte mapping but
    /// recorded so renderers can route Cmd-shortcuts to the GUI instead of
    /// the terminal.
    pub meta: bool,
}

impl TermModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    pub fn ctrl() -> Self {
        Self {
            ctrl: true,
            ..Self::NONE
        }
    }

    pub fn alt() -> Self {
        Self {
            alt: true,
            ..Self::NONE
        }
    }

    pub fn shift() -> Self {
        Self {
            shift: true,
            ..Self::NONE
        }
    }
}

/// Convert a key event to PTY-compatible bytes.
///
/// Returns `None` if the key shouldn't produce terminal output (e.g.,
/// modifier-only keys, plain printable characters which the renderer must
/// emit through its text-input path to avoid double-input, or Ctrl+Shift
/// combinations which are reserved for GUI shortcuts).
///
/// `app_cursor` — whether the terminal is in DECCKM application cursor mode.
pub fn key_to_pty_bytes(
    key: &TermKey,
    modifiers: &TermModifiers,
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
        if let Some(ch) = key.as_printable() {
            let mut bytes = vec![0x1B];
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            bytes.extend_from_slice(s.as_bytes());
            return Some(bytes);
        }
    }

    // Special keys.
    match key {
        TermKey::Enter => return Some(vec![0x0D]),
        TermKey::Tab => return Some(vec![0x09]),
        TermKey::Backspace => return Some(vec![0x7F]),
        TermKey::Escape => return Some(vec![0x1B]),
        TermKey::Delete => return Some(b"\x1b[3~".to_vec()),

        // Arrow keys — different sequences for normal vs application cursor mode.
        TermKey::ArrowUp => return Some(arrow_bytes(b'A', modifiers, app_cursor)),
        TermKey::ArrowDown => return Some(arrow_bytes(b'B', modifiers, app_cursor)),
        TermKey::ArrowRight => return Some(arrow_bytes(b'C', modifiers, app_cursor)),
        TermKey::ArrowLeft => return Some(arrow_bytes(b'D', modifiers, app_cursor)),

        TermKey::Home => return Some(b"\x1b[H".to_vec()),
        TermKey::End => return Some(b"\x1b[F".to_vec()),
        TermKey::PageUp => return Some(b"\x1b[5~".to_vec()),
        TermKey::PageDown => return Some(b"\x1b[6~".to_vec()),
        TermKey::Insert => return Some(b"\x1b[2~".to_vec()),

        // Function keys — DEC/xterm SS3 + CSI ~ encoding.
        TermKey::F1 => return Some(b"\x1bOP".to_vec()),
        TermKey::F2 => return Some(b"\x1bOQ".to_vec()),
        TermKey::F3 => return Some(b"\x1bOR".to_vec()),
        TermKey::F4 => return Some(b"\x1bOS".to_vec()),
        TermKey::F5 => return Some(b"\x1b[15~".to_vec()),
        TermKey::F6 => return Some(b"\x1b[17~".to_vec()),
        TermKey::F7 => return Some(b"\x1b[18~".to_vec()),
        TermKey::F8 => return Some(b"\x1b[19~".to_vec()),
        TermKey::F9 => return Some(b"\x1b[20~".to_vec()),
        TermKey::F10 => return Some(b"\x1b[21~".to_vec()),
        TermKey::F11 => return Some(b"\x1b[23~".to_vec()),
        TermKey::F12 => return Some(b"\x1b[24~".to_vec()),

        // Printable keys: handled by the renderer's text-input path to avoid
        // double-input. We return None here.
        TermKey::Letter(_) | TermKey::Digit(_) | TermKey::Char(_) | TermKey::Space => {}
    }

    None
}

fn ctrl_key_byte(key: &TermKey) -> Option<u8> {
    let c = key.as_letter()?;
    if c.is_ascii_lowercase() {
        // 'a' (0x61) → 0x01, 'z' (0x7A) → 0x1A.
        Some((c as u8) - b'a' + 1)
    } else {
        None
    }
}

fn arrow_bytes(direction: u8, modifiers: &TermModifiers, app_cursor: bool) -> Vec<u8> {
    let modifier_code = modifier_param(modifiers);
    if modifier_code > 1 {
        return format!("\x1b[1;{}{}", modifier_code, direction as char).into_bytes();
    }
    if app_cursor {
        vec![0x1B, b'O', direction]
    } else {
        vec![0x1B, b'[', direction]
    }
}

fn modifier_param(modifiers: &TermModifiers) -> u8 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Enter, &TermModifiers::NONE, false),
            Some(vec![0x0D])
        );
    }

    #[test]
    fn test_tab() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Tab, &TermModifiers::NONE, false),
            Some(vec![0x09])
        );
    }

    #[test]
    fn test_backspace() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Backspace, &TermModifiers::NONE, false),
            Some(vec![0x7F])
        );
    }

    #[test]
    fn test_escape() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Escape, &TermModifiers::NONE, false),
            Some(vec![0x1B])
        );
    }

    #[test]
    fn test_delete() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Delete, &TermModifiers::NONE, false),
            Some(b"\x1b[3~".to_vec())
        );
    }

    #[test]
    fn test_ctrl_c_and_d() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('c'), &TermModifiers::ctrl(), false),
            Some(vec![0x03])
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('d'), &TermModifiers::ctrl(), false),
            Some(vec![0x04])
        );
    }

    #[test]
    fn test_ctrl_a_through_z() {
        // Spot-check the boundary cases of the lowercase-letter mapping.
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('a'), &TermModifiers::ctrl(), false),
            Some(vec![0x01])
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('z'), &TermModifiers::ctrl(), false),
            Some(vec![0x1A])
        );
    }

    #[test]
    fn test_arrow_normal_mode() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::ArrowUp, &TermModifiers::NONE, false),
            Some(vec![0x1B, b'[', b'A'])
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::ArrowDown, &TermModifiers::NONE, false),
            Some(vec![0x1B, b'[', b'B'])
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::ArrowLeft, &TermModifiers::NONE, false),
            Some(vec![0x1B, b'[', b'D'])
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::ArrowRight, &TermModifiers::NONE, false),
            Some(vec![0x1B, b'[', b'C'])
        );
    }

    #[test]
    fn test_arrow_app_cursor_mode() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::ArrowUp, &TermModifiers::NONE, true),
            Some(vec![0x1B, b'O', b'A'])
        );
    }

    #[test]
    fn test_arrow_with_modifier_uses_csi_form() {
        // Shift+ArrowUp → ESC[1;2A
        let bytes = key_to_pty_bytes(&TermKey::ArrowUp, &TermModifiers::shift(), false).unwrap();
        assert_eq!(bytes, b"\x1b[1;2A".to_vec());
    }

    #[test]
    fn test_printable_returns_none() {
        // Plain printable keys are handled by text-input path, not here.
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('a'), &TermModifiers::NONE, false),
            None
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('a'), &TermModifiers::shift(), false),
            None
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Digit('5'), &TermModifiers::NONE, false),
            None
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Char('@'), &TermModifiers::NONE, false),
            None
        );
    }

    #[test]
    fn test_alt_letter_emits_esc_prefix() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('a'), &TermModifiers::alt(), false),
            Some(vec![0x1B, b'a'])
        );
    }

    #[test]
    fn test_alt_digit_emits_esc_prefix() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Digit('1'), &TermModifiers::alt(), false),
            Some(vec![0x1B, b'1'])
        );
    }

    #[test]
    fn test_ctrl_shift_reserved_for_gui() {
        let mods = TermModifiers {
            ctrl: true,
            shift: true,
            ..TermModifiers::NONE
        };
        assert_eq!(key_to_pty_bytes(&TermKey::Letter('c'), &mods, false), None);
    }

    #[test]
    fn test_ctrl_alt_letter_emits_esc_then_control() {
        // Ctrl+Alt+C → ESC + 0x03
        let mods = TermModifiers {
            ctrl: true,
            alt: true,
            ..TermModifiers::NONE
        };
        assert_eq!(
            key_to_pty_bytes(&TermKey::Letter('c'), &mods, false),
            Some(vec![0x1B, 0x03])
        );
    }

    #[test]
    fn test_function_keys_full_range() {
        let f_keys: &[(TermKey, &[u8])] = &[
            (TermKey::F1, b"\x1bOP"),
            (TermKey::F2, b"\x1bOQ"),
            (TermKey::F3, b"\x1bOR"),
            (TermKey::F4, b"\x1bOS"),
            (TermKey::F5, b"\x1b[15~"),
            (TermKey::F12, b"\x1b[24~"),
        ];
        for (key, expected) in f_keys {
            assert_eq!(
                key_to_pty_bytes(key, &TermModifiers::NONE, false),
                Some(expected.to_vec()),
                "f-key {key:?}"
            );
        }
    }

    #[test]
    fn test_navigation_keys() {
        assert_eq!(
            key_to_pty_bytes(&TermKey::Home, &TermModifiers::NONE, false),
            Some(b"\x1b[H".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::End, &TermModifiers::NONE, false),
            Some(b"\x1b[F".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::PageUp, &TermModifiers::NONE, false),
            Some(b"\x1b[5~".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::PageDown, &TermModifiers::NONE, false),
            Some(b"\x1b[6~".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&TermKey::Insert, &TermModifiers::NONE, false),
            Some(b"\x1b[2~".to_vec())
        );
    }

    #[test]
    fn test_term_key_as_printable() {
        assert_eq!(TermKey::Letter('a').as_printable(), Some('a'));
        assert_eq!(TermKey::Digit('7').as_printable(), Some('7'));
        assert_eq!(TermKey::Char('@').as_printable(), Some('@'));
        assert_eq!(TermKey::Space.as_printable(), Some(' '));
        assert_eq!(TermKey::Enter.as_printable(), None);
        assert_eq!(TermKey::F5.as_printable(), None);
    }
}
