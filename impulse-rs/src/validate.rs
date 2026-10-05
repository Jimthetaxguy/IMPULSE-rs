//! Centralized input validation chokepoint (ATCC v1).
//!
//! All user/agent-supplied strings pass through this module before being used
//! as filesystem paths, SQL components, IDs, or URL segments. This is the
//! single enforcement point for the "treat agent inputs as adversarial" rule.

/// Validation error with machine-readable `kind` for envelope error payloads.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("control characters in input: {field}")]
    ControlChars { field: &'static str },

    #[error("path traversal detected: {path}")]
    PathTraversal { path: String },

    #[error("percent-encoded input rejected: {field}")]
    PercentEncoded { field: &'static str },

    #[error("invalid resource name (contains '{ch}'): {name}")]
    InvalidResourceName { name: String, ch: char },

    #[error("input too long ({len} > {max}): {field}")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },

    #[error("empty input: {field}")]
    Empty { field: &'static str },
}

impl ValidationError {
    /// Machine-readable error kind for envelope payloads.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ControlChars { .. } => "control_chars",
            Self::PathTraversal { .. } => "path_traversal",
            Self::PercentEncoded { .. } => "percent_encoded",
            Self::InvalidResourceName { .. } => "invalid_resource_name",
            Self::TooLong { .. } => "too_long",
            Self::Empty { .. } => "empty_input",
        }
    }

    /// Whether the agent could retry with corrected input.
    pub fn retryable(&self) -> bool {
        true
    }
}

// ─── Validators ─────────────────────────────────────────────────────────────

/// Reject ASCII control characters (< 0x20 except \n, \r, \t).
pub fn reject_control_chars(input: &str, field: &'static str) -> Result<(), ValidationError> {
    for ch in input.chars() {
        if ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t' {
            return Err(ValidationError::ControlChars { field });
        }
    }
    Ok(())
}

/// Unicode bidirectional controls, which reorder how text displays
/// ("Trojan Source"): a path or name can read differently from what it is.
fn is_bidi_control(ch: char) -> bool {
    matches!(ch, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Reject any control character, line breaks and tabs included, and bidi
/// controls: for one-line values such as a file path or a tool name.
pub fn validate_single_line_text(input: &str, field: &'static str) -> Result<(), ValidationError> {
    if input
        .chars()
        .any(|ch| ch.is_control() || is_bidi_control(ch))
    {
        return Err(ValidationError::ControlChars { field });
    }
    Ok(())
}

/// [`reject_control_chars`] plus bidi controls: for free text that may span
/// lines, such as a session summary.
pub fn validate_multiline_text(input: &str, field: &'static str) -> Result<(), ValidationError> {
    reject_control_chars(input, field)?;
    if input.chars().any(is_bidi_control) {
        return Err(ValidationError::ControlChars { field });
    }
    Ok(())
}

/// Reject pre-percent-encoded strings when we encode later (avoids double-encode).
pub fn reject_percent_encoded(input: &str, field: &'static str) -> Result<(), ValidationError> {
    let bytes = input.as_bytes();
    if bytes
        .windows(3)
        .any(|w| w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit())
    {
        return Err(ValidationError::PercentEncoded { field });
    }
    Ok(())
}

/// Reject `?` and `#` in resource names/IDs (prevents embedded query/fragment).
pub fn validate_resource_name(name: &str, field: &'static str) -> Result<(), ValidationError> {
    for ch in ['?', '#', '\0', '/', '\\'] {
        if name.contains(ch) {
            return Err(ValidationError::InvalidResourceName {
                name: name.to_string(),
                ch,
            });
        }
    }
    reject_control_chars(name, field)?;
    Ok(())
}

/// Reject inputs longer than `max` bytes.
pub fn validate_length(
    input: &str,
    field: &'static str,
    max: usize,
) -> Result<(), ValidationError> {
    if input.len() > max {
        return Err(ValidationError::TooLong {
            field,
            len: input.len(),
            max,
        });
    }
    Ok(())
}

/// Reject empty inputs.
pub fn reject_empty(input: &str, field: &'static str) -> Result<(), ValidationError> {
    if input.trim().is_empty() {
        return Err(ValidationError::Empty { field });
    }
    Ok(())
}

/// Validate a session ID: non-empty, no control chars, no query/fragment chars, max 256.
pub fn validate_session_id(id: &str) -> Result<(), ValidationError> {
    reject_empty(id, "session_id")?;
    validate_length(id, "session_id", 256)?;
    validate_resource_name(id, "session_id")?;
    Ok(())
}

/// Validate a tool ID: non-empty, no control chars, no query/fragment chars, max 128.
pub fn validate_tool_id(id: &str) -> Result<(), ValidationError> {
    reject_empty(id, "tool_id")?;
    validate_length(id, "tool_id", 128)?;
    validate_resource_name(id, "tool_id")?;
    Ok(())
}

/// Validate a file path argument: non-empty, no control chars.
pub fn validate_file_arg(path: &str) -> Result<(), ValidationError> {
    reject_empty(path, "file")?;
    reject_control_chars(path, "file")?;
    validate_length(path, "file", 4096)?;
    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_chars_rejected() {
        assert!(reject_control_chars("hello\x00world", "test").is_err());
        assert!(reject_control_chars("hello\x07world", "test").is_err());
        // Newline, tab allowed
        assert!(reject_control_chars("hello\nworld", "test").is_ok());
        assert!(reject_control_chars("hello\tworld", "test").is_ok());
    }

    #[test]
    fn percent_encoded_rejected() {
        assert!(reject_percent_encoded("hello%20world", "test").is_err());
        assert!(reject_percent_encoded("%2F..%2Fetc", "test").is_err());
        // Literal percent without hex is fine
        assert!(reject_percent_encoded("100% done", "test").is_ok());
        assert!(reject_percent_encoded("hello%GGworld", "test").is_ok());
    }

    #[test]
    fn resource_name_rejects_query_fragment() {
        assert!(validate_resource_name("id?fields=all", "test").is_err());
        assert!(validate_resource_name("id#section", "test").is_err());
        assert!(validate_resource_name("id\0null", "test").is_err());
        assert!(validate_resource_name("valid-id-123", "test").is_ok());
    }

    #[test]
    fn length_enforced() {
        assert!(validate_length("ab", "test", 5).is_ok());
        assert!(validate_length("abcdef", "test", 5).is_err());
    }

    #[test]
    fn empty_rejected() {
        assert!(reject_empty("", "test").is_err());
        assert!(reject_empty("   ", "test").is_err());
        assert!(reject_empty("x", "test").is_ok());
    }

    #[test]
    fn session_id_validation() {
        assert!(validate_session_id("my-session-123").is_ok());
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id("id?q=1").is_err());
        assert!(validate_session_id(&"x".repeat(257)).is_err());
    }

    #[test]
    fn tool_id_validation() {
        assert!(validate_tool_id("file-reader").is_ok());
        assert!(validate_tool_id("").is_err());
        assert!(validate_tool_id(&"x".repeat(129)).is_err());
    }

    #[test]
    fn validation_error_kind() {
        let e = ValidationError::ControlChars { field: "x" };
        assert_eq!(e.kind(), "control_chars");
        assert!(e.retryable());
    }
}
