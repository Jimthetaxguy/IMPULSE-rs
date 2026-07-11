//! Error types for the Impulse agent and LLM backend layer.
//!
//! These types define the error contract for LLM provider integrations
//! (Anthropic, OpenAI, Minimax). Currently used by `llm_backends::LlmProvider`
//! trait; production wiring planned for Phase 2 daemon chat.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("API request failed: {0}")]
    ApiRequest(String),

    #[error("API returned invalid response: {0}")]
    ApiResponse(String),

    #[error("No API key configured for {provider}")]
    MissingApiKey { provider: String },

    // dead_code: Phase 2 — will be used by model management IPC endpoints
    #[allow(dead_code)]
    #[error("Model {model} not found")]
    ModelNotFound { model: String },

    #[error("Rate limited by provider")]
    RateLimited,

    #[error("Authentication failed: {0}")]
    Authentication(String),

    // dead_code: Phase 2 — will be used by agent lifecycle IPC endpoints
    #[allow(dead_code)]
    #[error("Agent not found: {id}")]
    AgentNotFound { id: String },

    // dead_code: Phase 2 — will be used by session lookup IPC endpoints
    #[allow(dead_code)]
    #[error("Session not found: {id}")]
    SessionNotFound { id: String },

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Surfaced by `llm_backends::Agent::chat_with_tools` (TUI_SPEC.md T9)
    /// when a model keeps requesting tool calls without ever returning a
    /// plain-text stop reason. Bounds a misbehaving model instead of looping
    /// forever; the caller (ion REPL) renders this as a one-line chat error,
    /// matching every other `AgentError` variant's rendering path.
    #[error("Tool-use loop exceeded the maximum of {rounds} round trip(s) without a final reply")]
    ToolLoopLimitExceeded { rounds: usize },

    /// Surfaced by `llm_backends::Agent::chat_with_tools`/
    /// `chat_with_tools_capped` (same-day Opus adversarial-review follow-up
    /// to TUI_SPEC.md T9, finding S2) when the *entire* multi-round
    /// tool-use exchange exceeds its wall-clock budget
    /// (`llm_backends::DEFAULT_TOOL_LOOP_TIMEOUT`), regardless of how many
    /// rounds it took. Bounds a hung provider request or slow tool
    /// execution so the REPL always regains control instead of blocking
    /// indefinitely with no way to abort; history is left untouched on this
    /// path, same as `ToolLoopLimitExceeded`.
    #[error("Tool-use loop exceeded its {seconds}s wall-clock budget without a final reply")]
    ToolLoopTimedOut { seconds: u64 },

    /// Surfaced by `agent::ImpulseAgent::harness_query_structured` (same-day
    /// Opus sweep, freeze-bug fix) when the spawned harness CLI subprocess
    /// (`claude`/`codex`/`gemini`, etc.) doesn't exit within
    /// `agent::DEFAULT_HARNESS_TIMEOUT`. Previously this `.output().await`
    /// had no timeout at all; combined with the daemon holding the
    /// `cached_agent` mutex across the same await (see
    /// `daemon/handlers.rs`'s `checkout_agent`/`checkin_agent`), a single
    /// hung harness process could freeze the entire daemon's agent IPC
    /// surface indefinitely. This bounds that to a fixed wall-clock budget
    /// instead.
    #[error("Harness command '{command}' did not complete within {seconds}s")]
    HarnessTimedOut { command: String, seconds: u64 },
}

pub type AgentResult<T> = Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_api_request_display() {
        let err = AgentError::ApiRequest("connection refused".into());
        assert!(format!("{err}").contains("connection refused"));
        assert!(format!("{err}").contains("API request failed"));
    }

    #[test]
    fn test_agent_error_api_response_display() {
        let err = AgentError::ApiResponse("invalid JSON".into());
        assert!(format!("{err}").contains("invalid JSON"));
    }

    #[test]
    fn test_agent_error_missing_api_key_display() {
        let err = AgentError::MissingApiKey {
            provider: "Anthropic".into(),
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("Anthropic"),
            "expected provider name in: {msg}"
        );
        assert!(
            msg.contains("No API key"),
            "expected error prefix in: {msg}"
        );
    }

    #[test]
    fn test_agent_error_model_not_found_display() {
        let err = AgentError::ModelNotFound {
            model: "gpt-5".into(),
        };
        assert!(format!("{err}").contains("gpt-5"));
    }

    #[test]
    fn test_agent_error_rate_limited_display() {
        let err = AgentError::RateLimited;
        assert!(format!("{err}").contains("Rate limited"));
    }

    #[test]
    fn test_agent_error_authentication_display() {
        let err = AgentError::Authentication("invalid token".into());
        assert!(format!("{err}").contains("invalid token"));
    }

    #[test]
    fn test_agent_error_agent_not_found_display() {
        let err = AgentError::AgentNotFound {
            id: "agent-42".into(),
        };
        assert!(format!("{err}").contains("agent-42"));
    }

    #[test]
    fn test_agent_error_session_not_found_display() {
        let err = AgentError::SessionNotFound {
            id: "sess-abc".into(),
        };
        assert!(format!("{err}").contains("sess-abc"));
    }

    #[test]
    fn test_agent_error_invalid_request_display() {
        let err = AgentError::InvalidRequest("empty query".into());
        assert!(format!("{err}").contains("empty query"));
        assert!(format!("{err}").contains("Invalid request"));
    }

    #[test]
    fn test_agent_error_tool_loop_limit_exceeded_display() {
        let err = AgentError::ToolLoopLimitExceeded { rounds: 10 };
        let msg = format!("{err}");
        assert!(msg.contains("10"), "expected round count in: {msg}");
        assert!(
            msg.to_lowercase().contains("tool-use loop"),
            "expected tool-use loop wording in: {msg}"
        );
    }

    #[test]
    fn test_agent_error_tool_loop_timed_out_display() {
        let err = AgentError::ToolLoopTimedOut { seconds: 180 };
        let msg = format!("{err}");
        assert!(msg.contains("180"), "expected timeout seconds in: {msg}");
        assert!(
            msg.to_lowercase().contains("tool-use loop"),
            "expected tool-use loop wording in: {msg}"
        );
        assert!(
            msg.to_lowercase().contains("wall-clock"),
            "expected wall-clock wording in: {msg}"
        );
    }

    #[test]
    fn test_agent_error_harness_timed_out_display() {
        let err = AgentError::HarnessTimedOut {
            command: "claude".to_string(),
            seconds: 120,
        };
        let msg = format!("{err}");
        assert!(msg.contains("claude"), "expected command name in: {msg}");
        assert!(msg.contains("120"), "expected timeout seconds in: {msg}");
    }
}
