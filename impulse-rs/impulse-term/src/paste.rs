/// Wrap pasted text in bracketed paste escape sequences.
///
/// Every ESC in `text`, and the one-character CSI (U+009B), is removed first.
/// Pasted text used to be able to end the paste early with its own
/// `ESC[201~`, after which the rest reached the program as typed input (a
/// `\r` there runs a command). Removing only that sequence would not do: the
/// removal itself can join the text around it into a new one.
pub fn bracketed_paste(text: &str) -> Vec<u8> {
    let mut bytes = b"\x1b[200~".to_vec();
    for ch in text.chars().filter(|&c| c != '\x1b' && c != '\u{9b}') {
        let mut utf8 = [0u8; 4];
        bytes.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bracketed_paste() {
        let bytes = bracketed_paste("hello");
        assert!(bytes.starts_with(b"\x1b[200~"));
        assert!(bytes.ends_with(b"\x1b[201~"));
        assert!(bytes.windows(5).any(|window| window == b"hello"));
    }

    /// Review finding: pasted text ended the paste with its own terminator,
    /// so the shell ran the rest as typed input.
    #[test]
    fn test_bracketed_paste_text_cannot_end_the_paste() {
        for text in [
            "echo harmless\x1b[201~; touch /tmp/impulse-pwned\r",
            // Removing only the inner terminator would join this into one.
            "\x1b[20\x1b[201~1~ rest\r",
            "csi\u{9b}201~ rest\r",
        ] {
            let bytes = bracketed_paste(text);
            let escapes = bytes.iter().filter(|&&b| b == 0x1b).count();
            assert_eq!(escapes, 2, "{text:?} -> {bytes:?}");
            assert!(bytes.starts_with(b"\x1b[200~") && bytes.ends_with(b"\x1b[201~"));
            let inner = String::from_utf8(bytes[6..bytes.len() - 6].to_vec()).unwrap();
            assert!(!inner.contains('\u{9b}'), "{inner:?}");
        }
    }
}
