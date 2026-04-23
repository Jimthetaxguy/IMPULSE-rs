//! Egui input shim — converts `egui::Key` + `egui::Modifiers` to the
//! toolkit-neutral `TermKey` + `TermModifiers` and delegates to
//! `impulse_term_core::input::key_to_pty_bytes` for byte translation.
//!
//! All escape-sequence logic lives in `impulse-term-core`. This module is
//! intentionally a mechanical mapping — the byte-level rules are identical
//! across renderer crates.

use eframe::egui;
use impulse_term_core::input as core_input;

pub use impulse_term_core::escape::bracketed_paste;
pub use impulse_term_core::input::{TermKey, TermModifiers};

/// Convert an egui key event to PTY-compatible bytes.
///
/// Public signature unchanged for back-compat with `impulse-gui` and any
/// other consumers — internally delegates to the toolkit-neutral core.
pub fn key_to_pty_bytes(
    key: &egui::Key,
    modifiers: &egui::Modifiers,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let term_key = egui_key_to_term(*key)?;
    let term_mods = egui_mods_to_term(modifiers);
    core_input::key_to_pty_bytes(&term_key, &term_mods, app_cursor)
}

/// Map an `egui::Key` to a `TermKey`. Returns `None` for keys that have
/// no terminal-input meaning (modifier keys, GUI-specific keys, etc.).
pub fn egui_key_to_term(key: egui::Key) -> Option<TermKey> {
    Some(match key {
        // Letters (lowercase canonical form).
        egui::Key::A => TermKey::Letter('a'),
        egui::Key::B => TermKey::Letter('b'),
        egui::Key::C => TermKey::Letter('c'),
        egui::Key::D => TermKey::Letter('d'),
        egui::Key::E => TermKey::Letter('e'),
        egui::Key::F => TermKey::Letter('f'),
        egui::Key::G => TermKey::Letter('g'),
        egui::Key::H => TermKey::Letter('h'),
        egui::Key::I => TermKey::Letter('i'),
        egui::Key::J => TermKey::Letter('j'),
        egui::Key::K => TermKey::Letter('k'),
        egui::Key::L => TermKey::Letter('l'),
        egui::Key::M => TermKey::Letter('m'),
        egui::Key::N => TermKey::Letter('n'),
        egui::Key::O => TermKey::Letter('o'),
        egui::Key::P => TermKey::Letter('p'),
        egui::Key::Q => TermKey::Letter('q'),
        egui::Key::R => TermKey::Letter('r'),
        egui::Key::S => TermKey::Letter('s'),
        egui::Key::T => TermKey::Letter('t'),
        egui::Key::U => TermKey::Letter('u'),
        egui::Key::V => TermKey::Letter('v'),
        egui::Key::W => TermKey::Letter('w'),
        egui::Key::X => TermKey::Letter('x'),
        egui::Key::Y => TermKey::Letter('y'),
        egui::Key::Z => TermKey::Letter('z'),
        // Digits.
        egui::Key::Num0 => TermKey::Digit('0'),
        egui::Key::Num1 => TermKey::Digit('1'),
        egui::Key::Num2 => TermKey::Digit('2'),
        egui::Key::Num3 => TermKey::Digit('3'),
        egui::Key::Num4 => TermKey::Digit('4'),
        egui::Key::Num5 => TermKey::Digit('5'),
        egui::Key::Num6 => TermKey::Digit('6'),
        egui::Key::Num7 => TermKey::Digit('7'),
        egui::Key::Num8 => TermKey::Digit('8'),
        egui::Key::Num9 => TermKey::Digit('9'),
        // Whitespace / control.
        egui::Key::Enter => TermKey::Enter,
        egui::Key::Tab => TermKey::Tab,
        egui::Key::Backspace => TermKey::Backspace,
        egui::Key::Escape => TermKey::Escape,
        egui::Key::Delete => TermKey::Delete,
        egui::Key::Space => TermKey::Space,
        // Cursor navigation.
        egui::Key::ArrowUp => TermKey::ArrowUp,
        egui::Key::ArrowDown => TermKey::ArrowDown,
        egui::Key::ArrowLeft => TermKey::ArrowLeft,
        egui::Key::ArrowRight => TermKey::ArrowRight,
        egui::Key::Home => TermKey::Home,
        egui::Key::End => TermKey::End,
        egui::Key::PageUp => TermKey::PageUp,
        egui::Key::PageDown => TermKey::PageDown,
        egui::Key::Insert => TermKey::Insert,
        // Function keys.
        egui::Key::F1 => TermKey::F1,
        egui::Key::F2 => TermKey::F2,
        egui::Key::F3 => TermKey::F3,
        egui::Key::F4 => TermKey::F4,
        egui::Key::F5 => TermKey::F5,
        egui::Key::F6 => TermKey::F6,
        egui::Key::F7 => TermKey::F7,
        egui::Key::F8 => TermKey::F8,
        egui::Key::F9 => TermKey::F9,
        egui::Key::F10 => TermKey::F10,
        egui::Key::F11 => TermKey::F11,
        egui::Key::F12 => TermKey::F12,
        // Punctuation / symbols.
        egui::Key::Minus => TermKey::Char('-'),
        egui::Key::Plus | egui::Key::Equals => TermKey::Char('='),
        egui::Key::OpenBracket => TermKey::Char('['),
        egui::Key::CloseBracket => TermKey::Char(']'),
        egui::Key::Backslash => TermKey::Char('\\'),
        egui::Key::Semicolon => TermKey::Char(';'),
        egui::Key::Colon => TermKey::Char(':'),
        egui::Key::Comma => TermKey::Char(','),
        egui::Key::Period => TermKey::Char('.'),
        egui::Key::Slash => TermKey::Char('/'),
        egui::Key::Backtick => TermKey::Char('`'),
        // Anything else (modifier-only keys, media keys, etc.) — no terminal mapping.
        _ => return None,
    })
}

/// Convert egui modifier state to toolkit-neutral form.
pub fn egui_mods_to_term(m: &egui::Modifiers) -> TermModifiers {
    TermModifiers {
        ctrl: m.ctrl,
        alt: m.alt,
        shift: m.shift,
        meta: m.mac_cmd || m.command,
    }
}

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
    fn test_egui_key_a_to_letter() {
        assert_eq!(egui_key_to_term(egui::Key::A), Some(TermKey::Letter('a')));
        assert_eq!(egui_key_to_term(egui::Key::Z), Some(TermKey::Letter('z')));
    }

    #[test]
    fn test_egui_key_num_to_digit() {
        assert_eq!(egui_key_to_term(egui::Key::Num0), Some(TermKey::Digit('0')));
        assert_eq!(egui_key_to_term(egui::Key::Num9), Some(TermKey::Digit('9')));
    }

    #[test]
    fn test_egui_key_special() {
        assert_eq!(egui_key_to_term(egui::Key::Enter), Some(TermKey::Enter));
        assert_eq!(egui_key_to_term(egui::Key::F5), Some(TermKey::F5));
        assert_eq!(egui_key_to_term(egui::Key::ArrowUp), Some(TermKey::ArrowUp));
    }

    #[test]
    fn test_egui_punctuation() {
        assert_eq!(egui_key_to_term(egui::Key::Minus), Some(TermKey::Char('-')));
        assert_eq!(
            egui_key_to_term(egui::Key::OpenBracket),
            Some(TermKey::Char('['))
        );
        assert_eq!(egui_key_to_term(egui::Key::Slash), Some(TermKey::Char('/')));
    }

    #[test]
    fn test_modifiers_round_trip() {
        let m = egui::Modifiers {
            ctrl: true,
            alt: true,
            shift: false,
            mac_cmd: false,
            command: false,
        };
        let t = egui_mods_to_term(&m);
        assert!(t.ctrl);
        assert!(t.alt);
        assert!(!t.shift);
        assert!(!t.meta);
    }

    #[test]
    fn test_modifiers_meta_from_command_or_mac_cmd() {
        let m = egui::Modifiers {
            command: true,
            ..Default::default()
        };
        assert!(egui_mods_to_term(&m).meta);
        let m = egui::Modifiers {
            mac_cmd: true,
            ..Default::default()
        };
        assert!(egui_mods_to_term(&m).meta);
    }

    // Smoke tests: existing impulse-term::input::key_to_pty_bytes still works
    // via delegation. The exhaustive byte-level coverage now lives in core.
    #[test]
    fn test_enter_via_shim() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Enter, &no_mods(), false),
            Some(vec![0x0D])
        );
    }

    #[test]
    fn test_ctrl_c_via_shim() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::C, &ctrl(), false),
            Some(vec![0x03])
        );
    }

    #[test]
    fn test_arrow_via_shim() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowUp, &no_mods(), false),
            Some(vec![0x1B, b'[', b'A'])
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowUp, &no_mods(), true),
            Some(vec![0x1B, b'O', b'A'])
        );
    }

    #[test]
    fn test_alt_letter_via_shim() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::A, &alt(), false),
            Some(vec![0x1B, b'a'])
        );
    }

    #[test]
    fn test_printable_returns_none_via_shim() {
        assert_eq!(key_to_pty_bytes(&egui::Key::A, &no_mods(), false), None);
        assert_eq!(key_to_pty_bytes(&egui::Key::A, &shift(), false), None);
    }

    #[test]
    fn test_bracketed_paste_via_shim() {
        let bytes = bracketed_paste("hi");
        assert!(bytes.starts_with(b"\x1b[200~"));
        assert!(bytes.ends_with(b"\x1b[201~"));
    }
}
