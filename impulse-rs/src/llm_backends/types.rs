//! Flexible Agent Types
//!
//! Supports both API-based LLM providers and CLI-based agents (Claude Code, OpenCode)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The backing type for an agent - either a remote API or a local CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentBackend {
    /// Direct API call to an LLM provider (Anthropic, OpenAI, etc.)
    Api,
    /// Spawn a CLI subprocess (Claude Code, OpenCode, etc.)
    Cli,
}

impl Default for AgentBackend {
    fn default() -> Self {
        Self::Cli
    }
}

/// The specific agent type to use
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentType {
    /// Claude Code CLI (`claude` command)
    ClaudeCode,
    /// OpenCode CLI (`opencode` command)
    OpenCode,
    /// Anthropic API (direct API call)
    Anthropic,
    /// OpenAI API (direct API call)
    OpenAi,
    /// Minimax API (direct API call)
    Minimax,
    /// Custom API endpoint
    Custom,
}

impl Default for AgentType {
    fn default() -> Self {
        Self::ClaudeCode
    }
}

impl AgentType {
    /// Get the default backend for this agent type
    pub fn default_backend(&self) -> AgentBackend {
        match self {
            Self::ClaudeCode | Self::OpenCode => AgentBackend::Cli,
            Self::Anthropic | Self::OpenAi | Self::Minimax | Self::Custom => AgentBackend::Api,
        }
    }

    /// Get the CLI command name for CLI-based agents
    pub fn cli_command(&self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::OpenCode => Some("opencode"),
            _ => None,
        }
    }

    /// Check if this is a CLI-based agent
    pub fn is_cli(&self) -> bool {
        matches!(self, Self::ClaudeCode | Self::OpenCode)
    }

    /// Check if this is an API-based agent
    pub fn is_api(&self) -> bool {
        !self.is_cli()
    }
}

/// Configuration for a flexible agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for this agent
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// The type of agent (Claude Code, OpenAI API, etc.)
    pub agent_type: AgentType,
    /// The backend to use (Cli or Api)
    #[serde(default)]
    pub backend: AgentBackend,
    /// Model to use (for API providers)
    #[serde(default)]
    pub model: Option<String>,
    /// API key (for API providers)
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Custom API endpoint (for Custom type)
    #[serde(default)]
    pub api_endpoint: Option<String>,
    /// Working directory for CLI agents
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables for CLI agents
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// System prompt for the agent
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Whether to enable verbose output
    #[serde(default)]
    pub verbose: bool,
}

impl AgentConfig {
    /// Create a new agent config with defaults
    pub fn new(id: impl Into<String>, name: impl Into<String>, agent_type: AgentType) -> Self {
        let backend = agent_type.default_backend();
        Self {
            id: id.into(),
            name: name.into(),
            agent_type,
            backend,
            model: None,
            api_key: None,
            api_endpoint: None,
            working_dir: None,
            env: std::collections::HashMap::new(),
            system_prompt: None,
            verbose: false,
        }
    }

    /// Create config for Claude Code
    pub fn claude_code(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(id, name, AgentType::ClaudeCode)
    }

    /// Create config for OpenCode
    pub fn opencode(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self::new(id, name, AgentType::OpenCode)
    }

    /// Create config for Anthropic API
    pub fn anthropic(
        id: impl Into<String>,
        name: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self::new(id, name, AgentType::Anthropic)
            .with_api_key(api_key)
            .with_model("claude-sonnet-4-20250514")
    }

    /// Create config for OpenAI API
    pub fn openai(
        id: impl Into<String>,
        name: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self::new(id, name, AgentType::OpenAi)
            .with_api_key(api_key)
            .with_model("gpt-4o")
    }

    /// Set the API key
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set the model
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the working directory
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    /// Set the system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Enable verbose mode
    pub fn with_verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Set custom API endpoint
    pub fn with_api_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.api_endpoint = Some(endpoint.into());
        self
    }

    /// Add environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// Session state for a CLI agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSession {
    /// The agent config
    pub config: AgentConfig,
    /// Whether the session is active
    pub active: bool,
    /// Process ID of the CLI subprocess
    #[serde(default)]
    pub pid: Option<u32>,
    /// Session ID (for tracking)
    #[serde(default)]
    pub session_id: Option<String>,
}

impl CliSession {
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            active: false,
            pid: None,
            session_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_defaults() {
        let config = AgentConfig::claude_code("test", "Test Agent");
        assert_eq!(config.agent_type, AgentType::ClaudeCode);
        assert_eq!(config.backend, AgentBackend::Cli);

        let config = AgentConfig::anthropic("test", "Anthropic", "key123");
        assert_eq!(config.agent_type, AgentType::Anthropic);
        assert_eq!(config.backend, AgentBackend::Api);
    }

    #[test]
    fn test_agent_type_helpers() {
        assert!(AgentType::ClaudeCode.is_cli());
        assert!(AgentType::OpenCode.is_cli());
        assert!(!AgentType::Anthropic.is_cli());

        assert!(AgentType::Anthropic.is_api());
        assert!(!AgentType::ClaudeCode.is_api());

        assert_eq!(AgentType::ClaudeCode.cli_command(), Some("claude"));
        assert_eq!(AgentType::Anthropic.cli_command(), None);
    }

    #[test]
    fn test_agent_config_builder() {
        let config = AgentConfig::claude_code("my-claude", "My Claude")
            .with_working_dir(PathBuf::from("/tmp"))
            .with_verbose()
            .with_system_prompt("You are a helpful coding assistant");

        assert_eq!(config.id, "my-claude");
        assert_eq!(config.name, "My Claude");
        assert!(config.verbose);
        assert_eq!(config.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(
            config.system_prompt,
            Some("You are a helpful coding assistant".to_string())
        );
    }
}
