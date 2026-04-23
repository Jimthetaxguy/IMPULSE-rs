//! Dioxus `KeyboardEvent` → `TermKey` + `TermModifiers` shim.
//!
//! Mirror of the egui-side `egui_key_to_term` shim that lives in
//! `impulse-term/src/input.rs`. Both shims are deliberately mechanical —
//! the actual escape-sequence rules live in `impulse_term_core::input`.
//!
//! # Why this is a separate module from `pty_view.rs`
//!
//! Tests for the key mapping want to construct `Key` values without
//! spinning up a Dioxus runtime. Keeping the mapping pure (no Signals,
//! no components) lets us cover all key variants via direct unit tests.

use dioxus::events::Modifiers as DxModifiers;
use dioxus::html::input_data::keyboard_types::Key as DxKey;
use impulse_term_core::input::{TermKey, TermModifiers};

/// Map a Dioxus `Key` to a `TermKey`.
///
/// Returns `None` for keys that have no terminal-input meaning (modifier
/// keys held alone, dead keys, media keys, etc.).
///
/// Letters are normalized to lowercase. Shift state is conveyed via
/// `TermModifiers.shift`.
pub fn dx_key_to_term(key: &DxKey) -> Option<TermKey> {
    Some(match key {
        // Single character → Letter / Digit / Char.
        DxKey::Character(s) => match s.as_str() {
            "" => return None,
            single if single.chars().count() == 1 => {
                let c = single.chars().next().unwrap();
                if c.is_ascii_alphabetic() {
                    TermKey::Letter(c.to_ascii_lowercase())
                } else if c.is_ascii_digit() {
                    TermKey::Digit(c)
                } else if c == ' ' {
                    TermKey::Space
                } else {
                    TermKey::Char(c)
                }
            }
            _ => return None, // multi-char input handled by IME / text path
        },
        DxKey::Enter => TermKey::Enter,
        DxKey::Tab => TermKey::Tab,
        DxKey::Backspace => TermKey::Backspace,
        DxKey::Escape => TermKey::Escape,
        DxKey::Delete => TermKey::Delete,
        DxKey::ArrowUp => TermKey::ArrowUp,
        DxKey::ArrowDown => TermKey::ArrowDown,
        DxKey::ArrowLeft => TermKey::ArrowLeft,
        DxKey::ArrowRight => TermKey::ArrowRight,
        DxKey::Home => TermKey::Home,
        DxKey::End => TermKey::End,
        DxKey::PageUp => TermKey::PageUp,
        DxKey::PageDown => TermKey::PageDown,
        DxKey::Insert => TermKey::Insert,
        DxKey::F1 => TermKey::F1,
        DxKey::F2 => TermKey::F2,
        DxKey::F3 => TermKey::F3,
        DxKey::F4 => TermKey::F4,
        DxKey::F5 => TermKey::F5,
        DxKey::F6 => TermKey::F6,
        DxKey::F7 => TermKey::F7,
        DxKey::F8 => TermKey::F8,
        DxKey::F9 => TermKey::F9,
        DxKey::F10 => TermKey::F10,
        DxKey::F11 => TermKey::F11,
        DxKey::F12 => TermKey::F12,
        // Modifier-only or unmapped keys (Shift, Control, Alt, Meta,
        // CapsLock, AudioVolumeUp, ContextMenu, etc.) → no terminal byte.
        _ => return None,
    })
}

/// Map a Dioxus `Modifiers` bitflag set to `TermModifiers`.
pub fn dx_mods_to_term(m: &DxModifiers) -> TermModifiers {
    TermModifiers {
        ctrl: m.contains(DxModifiers::CONTROL),
        alt: m.contains(DxModifiers::ALT),
        shift: m.contains(DxModifiers::SHIFT),
        meta: m.contains(DxModifiers::META) || m.contains(DxModifiers::SUPER),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_letter_normalized_to_lowercase() {
        assert_eq!(
            dx_key_to_term(&DxKey::Character("A".into())),
            Some(TermKey::Letter('a'))
        );
        assert_eq!(
            dx_key_to_term(&DxKey::Character("z".into())),
            Some(TermKey::Letter('z'))
        );
    }

    #[test]
    fn test_character_digit() {
        assert_eq!(
            dx_key_to_term(&DxKey::Character("0".into())),
            Some(TermKey::Digit('0'))
        );
        assert_eq!(
            dx_key_to_term(&DxKey::Character("9".into())),
            Some(TermKey::Digit('9'))
        );
    }

    #[test]
    fn test_character_space() {
        assert_eq!(
            dx_key_to_term(&DxKey::Character(" ".into())),
            Some(TermKey::Space)
        );
    }

    #[test]
    fn test_character_punctuation_falls_through_to_char() {
        assert_eq!(
            dx_key_to_term(&DxKey::Character("@".into())),
            Some(TermKey::Char('@'))
        );
        assert_eq!(
            dx_key_to_term(&DxKey::Character("/".into())),
            Some(TermKey::Char('/'))
        );
    }

    #[test]
    fn test_empty_character_returns_none() {
        assert_eq!(dx_key_to_term(&DxKey::Character("".into())), None);
    }

    #[test]
    fn test_multi_char_returns_none() {
        // IME composition would feed multi-char strings; those go through
        // a separate text-input path, not key-to-bytes.
        assert_eq!(dx_key_to_term(&DxKey::Character("ñá".into())), None);
    }

    #[test]
    fn test_special_keys_map_through() {
        assert_eq!(dx_key_to_term(&DxKey::Enter), Some(TermKey::Enter));
        assert_eq!(dx_key_to_term(&DxKey::Tab), Some(TermKey::Tab));
        assert_eq!(dx_key_to_term(&DxKey::Backspace), Some(TermKey::Backspace));
        assert_eq!(dx_key_to_term(&DxKey::Escape), Some(TermKey::Escape));
        assert_eq!(dx_key_to_term(&DxKey::Delete), Some(TermKey::Delete));
    }

    #[test]
    fn test_arrow_keys() {
        assert_eq!(dx_key_to_term(&DxKey::ArrowUp), Some(TermKey::ArrowUp));
        assert_eq!(dx_key_to_term(&DxKey::ArrowDown), Some(TermKey::ArrowDown));
        assert_eq!(dx_key_to_term(&DxKey::ArrowLeft), Some(TermKey::ArrowLeft));
        assert_eq!(
            dx_key_to_term(&DxKey::ArrowRight),
            Some(TermKey::ArrowRight)
        );
    }

    #[test]
    fn test_navigation_keys() {
        assert_eq!(dx_key_to_term(&DxKey::Home), Some(TermKey::Home));
        assert_eq!(dx_key_to_term(&DxKey::End), Some(TermKey::End));
        assert_eq!(dx_key_to_term(&DxKey::PageUp), Some(TermKey::PageUp));
        assert_eq!(dx_key_to_term(&DxKey::PageDown), Some(TermKey::PageDown));
        assert_eq!(dx_key_to_term(&DxKey::Insert), Some(TermKey::Insert));
    }

    #[test]
    fn test_function_keys_full_range() {
        assert_eq!(dx_key_to_term(&DxKey::F1), Some(TermKey::F1));
        assert_eq!(dx_key_to_term(&DxKey::F12), Some(TermKey::F12));
    }

    #[test]
    fn test_modifier_only_keys_return_none() {
        assert_eq!(dx_key_to_term(&DxKey::Shift), None);
        assert_eq!(dx_key_to_term(&DxKey::Control), None);
        assert_eq!(dx_key_to_term(&DxKey::Alt), None);
        assert_eq!(dx_key_to_term(&DxKey::Meta), None);
        assert_eq!(dx_key_to_term(&DxKey::CapsLock), None);
    }

    #[test]
    fn test_dx_mods_to_term_basic() {
        let mods = DxModifiers::CONTROL | DxModifiers::SHIFT;
        let t = dx_mods_to_term(&mods);
        assert!(t.ctrl);
        assert!(t.shift);
        assert!(!t.alt);
        assert!(!t.meta);
    }

    #[test]
    fn test_dx_mods_meta_from_meta_or_super() {
        let mods = DxModifiers::META;
        assert!(dx_mods_to_term(&mods).meta);

        let mods = DxModifiers::SUPER;
        assert!(dx_mods_to_term(&mods).meta);
    }

    #[test]
    fn test_dx_mods_empty() {
        let mods = DxModifiers::empty();
        let t = dx_mods_to_term(&mods);
        assert!(!t.ctrl);
        assert!(!t.alt);
        assert!(!t.shift);
        assert!(!t.meta);
    }

    /// Smoke: combine the shim + core to assert end-to-end byte production.
    #[test]
    fn test_end_to_end_ctrl_c() {
        use impulse_term_core::input::key_to_pty_bytes;

        let key = dx_key_to_term(&DxKey::Character("c".into())).unwrap();
        let mods = dx_mods_to_term(&DxModifiers::CONTROL);
        assert_eq!(key_to_pty_bytes(&key, &mods, false), Some(vec![0x03]));
    }

    #[test]
    fn test_end_to_end_arrow_up_app_cursor() {
        use impulse_term_core::input::key_to_pty_bytes;

        let key = dx_key_to_term(&DxKey::ArrowUp).unwrap();
        let mods = dx_mods_to_term(&DxModifiers::empty());
        assert_eq!(
            key_to_pty_bytes(&key, &mods, true),
            Some(vec![0x1B, b'O', b'A'])
        );
    }
}
