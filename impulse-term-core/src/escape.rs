//! Toolkit-neutral terminal escape sequence helpers.
//!
//! These helpers don't depend on any GUI toolkit's key types — they operate on
//! plain bytes and strings. Used by both the context bridge (programmatic
//! injection) and the per-toolkit input modules (`impulse-term-egui::input`,
//! `impulse-term-dioxus::input`) which translate their toolkit's key events
//! into PTY bytes.

/// Wrap pasted text in bracketed paste escape sequences (DEC private mode 2004).
///
/// The terminal child process must have bracketed-paste mode enabled (which
/// modern shells, editors, and TUI apps do by default) for these markers to
/// be interpreted as paste boundaries rather than typed characters.
pub fn bracketed_paste(text: &str) -> Vec<u8> {
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bracketed_paste_wraps_with_markers() {
        let bytes = bracketed_paste("hello");
        assert_eq!(&bytes[0..6], b"\x1b[200~");
        assert_eq!(&bytes[6..11], b"hello");
        assert_eq!(&bytes[11..], b"\x1b[201~");
    }

    #[test]
    fn test_bracketed_paste_empty() {
        let bytes = bracketed_paste("");
        assert_eq!(&bytes[0..6], b"\x1b[200~");
        assert_eq!(&bytes[6..], b"\x1b[201~");
        assert_eq!(bytes.len(), 12);
    }

    #[test]
    fn test_bracketed_paste_preserves_unicode() {
        let bytes = bracketed_paste("héllo 🦀");
        let inner = &bytes[6..bytes.len() - 6];
        assert_eq!(inner, "héllo 🦀".as_bytes());
    }
}
