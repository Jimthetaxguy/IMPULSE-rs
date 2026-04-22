//! Structured JSON protocol for harness-mode agent communication.
//!
//! When Impulse delegates a query to a CLI harness (Claude Code, OpenCode), this
//! module defines the structured request and response format. The protocol is:
//!
//! 1. Serialize a [`HarnessRequest`] to a temp file.
//! 2. Set `IMPULSE_HARNESS_REQUEST` env var pointing to that file.
//! 3. Run the harness CLI with `--print <user_prompt>`.
//! 4. Attempt to parse stdout as [`HarnessResponse`] JSON.
//! 5. **Fallback**: if parsing fails, treat the entire stdout as plain-text content.
//!
//! The fallback guarantees backward compatibility: if the harness doesn't understand
//! the structured protocol, plain string output still works exactly as before.

use serde::{Deserialize, Serialize};

use crate::context_lifecycle::types::ExtractedInsight;

/// Structured request passed to a CLI harness via temp file.
///
/// The harness can read `IMPULSE_HARNESS_REQUEST` to get this JSON. If the env
/// var is absent or the file is unreadable, the harness falls back to the
/// plain-text `--print` argument — so this is purely additive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessRequest {
    /// System-level instruction for the LLM.
    pub system_prompt: String,
    /// User-level prompt / question.
    pub user_prompt: String,
    /// Cross-pane context insights extracted by the context lifecycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ExtractedInsight>,
    /// Optional token budget for the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Structured response returned by a harness that supports the protocol.
///
/// If the harness stdout is valid JSON matching this shape, we use it.
/// Otherwise, we fall back to [`HarnessResponse::plain`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessResponse {
    /// The main text content returned by the harness.
    pub content: String,
    /// Which model the harness used (if reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Token usage statistics (if reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<HarnessUsage>,
}

/// Token usage statistics from a harness invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessUsage {
    /// Tokens consumed by the input (prompt + context).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// Tokens produced in the output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
}

impl HarnessResponse {
    /// Create a plain-text fallback response from raw stdout.
    ///
    /// Used when the harness output is not valid JSON — the entire string
    /// becomes the `content` field with no model or usage metadata.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            content: text.into(),
            model: None,
            usage: None,
        }
    }

    /// Attempt to parse stdout as structured JSON, falling back to plain text.
    ///
    /// This is the core of the fallback protocol:
    /// - If `stdout` is valid JSON that deserializes to [`HarnessResponse`], use it.
    /// - Otherwise, return `HarnessResponse::plain(stdout)`.
    pub fn parse_or_plain(stdout: &str) -> Self {
        let trimmed = stdout.trim();
        if trimmed.starts_with('{') {
            serde_json::from_str::<HarnessResponse>(trimmed).unwrap_or_else(|_| Self::plain(stdout))
        } else {
            Self::plain(stdout)
        }
    }

    /// Whether this response was parsed from structured JSON (has model or usage).
    pub fn is_structured(&self) -> bool {
        self.model.is_some() || self.usage.is_some()
    }
}

/// Write a [`HarnessRequest`] to a temp file and return the path.
///
/// The temp file is created with a `.json` suffix for clarity. The caller is
/// responsible for cleanup (the file persists until the [`tempfile::NamedTempFile`]
/// handle is dropped or the process exits).
pub fn write_request_file(request: &HarnessRequest) -> std::io::Result<tempfile::NamedTempFile> {
    use std::io::Write;

    let mut file = tempfile::Builder::new()
        .prefix("impulse-harness-")
        .suffix(".json")
        .tempfile()?;

    let json = serde_json::to_string(request)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    file.write_all(json.as_bytes())?;
    file.flush()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_lifecycle::types::{AgentKind, InsightType};

    // --- HarnessRequest serialization ---

    #[test]
    fn test_harness_request_serialization_minimal() {
        let req = HarnessRequest {
            system_prompt: "You are a helpful assistant.".to_string(),
            user_prompt: "What is Rust?".to_string(),
            context: vec![],
            max_tokens: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("system_prompt"));
        assert!(json.contains("user_prompt"));
        // Empty context and None max_tokens should be omitted
        assert!(!json.contains("context"));
        assert!(!json.contains("max_tokens"));
    }

    #[test]
    fn test_harness_request_serialization_full() {
        let req = HarnessRequest {
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            context: vec![ExtractedInsight {
                pane_id: 1,
                agent_kind: AgentKind::ClaudeCode,
                timestamp: chrono::Utc::now(),
                insight_type: InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            }],
            max_tokens: Some(4096),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("context"));
        assert!(json.contains("max_tokens"));
        assert!(json.contains("4096"));
        assert!(json.contains("src/main.rs"));
    }

    #[test]
    fn test_harness_request_roundtrip() {
        let req = HarnessRequest {
            system_prompt: "sys".to_string(),
            user_prompt: "usr".to_string(),
            context: vec![],
            max_tokens: Some(1024),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: HarnessRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.system_prompt, "sys");
        assert_eq!(deserialized.user_prompt, "usr");
        assert_eq!(deserialized.max_tokens, Some(1024));
    }

    // --- HarnessResponse deserialization ---

    #[test]
    fn test_harness_response_structured_full() {
        let json = r#"{
            "content": "Rust is a systems programming language.",
            "model": "claude-sonnet-4-6",
            "usage": {
                "input_tokens": 150,
                "output_tokens": 42
            }
        }"#;
        let resp: HarnessResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content, "Rust is a systems programming language.");
        assert_eq!(resp.model.as_deref(), Some("claude-sonnet-4-6"));
        assert!(resp.is_structured());
        let usage = resp.usage.clone().unwrap();
        assert_eq!(usage.input_tokens, Some(150));
        assert_eq!(usage.output_tokens, Some(42));
    }

    #[test]
    fn test_harness_response_structured_content_only() {
        let json = r#"{"content": "hello"}"#;
        let resp: HarnessResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content, "hello");
        assert!(resp.model.is_none());
        assert!(resp.usage.is_none());
        assert!(!resp.is_structured());
    }

    #[test]
    fn test_harness_response_plain() {
        let resp = HarnessResponse::plain("Some raw text output");
        assert_eq!(resp.content, "Some raw text output");
        assert!(resp.model.is_none());
        assert!(resp.usage.is_none());
        assert!(!resp.is_structured());
    }

    // --- Fallback behavior ---

    #[test]
    fn test_parse_or_plain_with_valid_json() {
        let json = r#"{"content": "structured response", "model": "gpt-4o"}"#;
        let resp = HarnessResponse::parse_or_plain(json);
        assert_eq!(resp.content, "structured response");
        assert_eq!(resp.model.as_deref(), Some("gpt-4o"));
        assert!(resp.is_structured());
    }

    #[test]
    fn test_parse_or_plain_with_valid_json_whitespace() {
        let json = r#"  {"content": "padded", "model": "m"}  "#;
        let resp = HarnessResponse::parse_or_plain(json);
        assert_eq!(resp.content, "padded");
        assert_eq!(resp.model.as_deref(), Some("m"));
    }

    #[test]
    fn test_parse_or_plain_with_plain_text() {
        let text = "This is just plain text from the CLI";
        let resp = HarnessResponse::parse_or_plain(text);
        assert_eq!(resp.content, text);
        assert!(resp.model.is_none());
        assert!(!resp.is_structured());
    }

    #[test]
    fn test_parse_or_plain_with_invalid_json() {
        // Starts with '{' but is not valid HarnessResponse JSON
        let text = r#"{"error": "something went wrong"}"#;
        let resp = HarnessResponse::parse_or_plain(text);
        // Missing required "content" field, so falls back to plain
        assert_eq!(resp.content, text);
        assert!(!resp.is_structured());
    }

    #[test]
    fn test_parse_or_plain_with_partial_json() {
        let text = r#"{"content": "hello", broken"#;
        let resp = HarnessResponse::parse_or_plain(text);
        assert_eq!(resp.content, text);
        assert!(!resp.is_structured());
    }

    #[test]
    fn test_parse_or_plain_with_empty_string() {
        let resp = HarnessResponse::parse_or_plain("");
        assert_eq!(resp.content, "");
        assert!(!resp.is_structured());
    }

    #[test]
    fn test_parse_or_plain_with_json_array() {
        // JSON array, not object — should fall back to plain
        let text = r#"[{"content": "item"}]"#;
        let resp = HarnessResponse::parse_or_plain(text);
        assert_eq!(resp.content, text);
        assert!(!resp.is_structured());
    }

    // --- write_request_file ---

    #[test]
    fn test_write_request_file_creates_readable_json() {
        let req = HarnessRequest {
            system_prompt: "test system".to_string(),
            user_prompt: "test user".to_string(),
            context: vec![],
            max_tokens: Some(512),
        };
        let file = write_request_file(&req).unwrap();
        let path = file.path();
        assert!(path.exists());

        let contents = std::fs::read_to_string(path).unwrap();
        let parsed: HarnessRequest = serde_json::from_str(&contents).unwrap();
        assert_eq!(parsed.system_prompt, "test system");
        assert_eq!(parsed.user_prompt, "test user");
        assert_eq!(parsed.max_tokens, Some(512));
    }

    #[test]
    fn test_write_request_file_has_json_suffix() {
        let req = HarnessRequest {
            system_prompt: String::new(),
            user_prompt: String::new(),
            context: vec![],
            max_tokens: None,
        };
        let file = write_request_file(&req).unwrap();
        let path_str = file.path().to_string_lossy().to_string();
        assert!(
            path_str.ends_with(".json"),
            "temp file should have .json suffix"
        );
    }

    // --- HarnessUsage ---

    #[test]
    fn test_harness_usage_partial() {
        let json = r#"{"input_tokens": 100}"#;
        let usage: HarnessUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, None);
    }

    #[test]
    fn test_harness_usage_empty() {
        let json = r#"{}"#;
        let usage: HarnessUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
    }

    // --- HarnessResponse roundtrip ---

    #[test]
    fn test_harness_response_roundtrip() {
        let resp = HarnessResponse {
            content: "answer".to_string(),
            model: Some("claude-sonnet-4-6".to_string()),
            usage: Some(HarnessUsage {
                input_tokens: Some(200),
                output_tokens: Some(50),
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed = HarnessResponse::parse_or_plain(&json);
        assert_eq!(parsed.content, "answer");
        assert_eq!(parsed.model.as_deref(), Some("claude-sonnet-4-6"));
        assert!(parsed.is_structured());
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.input_tokens, Some(200));
        assert_eq!(usage.output_tokens, Some(50));
    }
}
