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
}
