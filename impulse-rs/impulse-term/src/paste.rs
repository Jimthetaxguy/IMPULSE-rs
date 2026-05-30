/// Wrap pasted text in bracketed paste escape sequences.
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
    fn test_bracketed_paste() {
        let bytes = bracketed_paste("hello");
        assert!(bytes.starts_with(b"\x1b[200~"));
        assert!(bytes.ends_with(b"\x1b[201~"));
        assert!(bytes.windows(5).any(|window| window == b"hello"));
    }
}
