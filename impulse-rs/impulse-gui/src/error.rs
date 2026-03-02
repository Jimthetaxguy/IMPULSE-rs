//! Typed error enum for the Impulse GUI crate.
//!
//! Replaces `Box<dyn std::error::Error>` across identity, global_config,
//! and project_scaffold modules with a concrete error type that supports
//! `#[from]` auto-conversion and pattern matching.

use thiserror::Error;

/// Errors produced by impulse-gui operations (config, identity, scaffold).
#[derive(Debug, Error)]
pub enum GuiError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("scaffold error: {0}")]
    Scaffold(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_io_error() {
        let err = GuiError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_display_json_error() {
        let json_err: Result<serde_json::Value, _> = serde_json::from_str("{invalid");
        let err: GuiError = json_err.unwrap_err().into();
        assert!(err.to_string().contains("JSON error"));
    }

    #[test]
    fn test_display_scaffold_error() {
        let err = GuiError::Scaffold("no parent directory".to_string());
        assert_eq!(err.to_string(), "scaffold error: no parent directory");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let gui_err: GuiError = io_err.into();
        assert!(matches!(gui_err, GuiError::Io(_)));
    }

    #[test]
    fn test_from_json_error() {
        let json_result: Result<serde_json::Value, _> = serde_json::from_str("not json");
        let gui_err: GuiError = json_result.unwrap_err().into();
        assert!(matches!(gui_err, GuiError::Json(_)));
    }
}
