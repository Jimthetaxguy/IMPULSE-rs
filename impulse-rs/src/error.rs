use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[allow(dead_code)]
    #[error("API request failed: {0}")]
    ApiRequest(String),

    #[allow(dead_code)]
    #[error("API returned invalid response: {0}")]
    ApiResponse(String),

    #[allow(dead_code)]
    #[error("No API key configured for {provider}")]
    MissingApiKey { provider: String },

    #[allow(dead_code)]
    #[error("Model {model} not found")]
    ModelNotFound { model: String },

    #[allow(dead_code)]
    #[error("Rate limited by provider")]
    RateLimited,

    #[allow(dead_code)]
    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[allow(dead_code)]
    #[error("Agent not found: {id}")]
    AgentNotFound { id: String },

    #[allow(dead_code)]
    #[error("Session not found: {id}")]
    SessionNotFound { id: String },

    #[allow(dead_code)]
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
