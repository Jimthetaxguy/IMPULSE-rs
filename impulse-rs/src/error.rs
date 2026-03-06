//! Error types for the Impulse agent and LLM backend layer.
//!
//! These types define the error contract for LLM provider integrations
//! (Anthropic, OpenAI, Minimax). Currently used by `llm_backends::LlmProvider`
//! trait; production wiring planned for Phase 2 daemon chat.

#![allow(dead_code)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("API request failed: {0}")]
    ApiRequest(String),

    #[error("API returned invalid response: {0}")]
    ApiResponse(String),

    #[error("No API key configured for {provider}")]
    MissingApiKey { provider: String },

    #[error("Model {model} not found")]
    ModelNotFound { model: String },

    #[error("Rate limited by provider")]
    RateLimited,

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Agent not found: {id}")]
    AgentNotFound { id: String },

    #[error("Session not found: {id}")]
    SessionNotFound { id: String },

    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

pub type AgentResult<T> = Result<T, AgentError>;
