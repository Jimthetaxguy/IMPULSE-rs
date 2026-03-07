//! Agent Factory
//!
//! Creates the appropriate agent type based on configuration

use async_trait::async_trait;

use super::anthropic::{AnthropicProvider, MinimaxProvider, OpenAiProvider};
use super::cli::CliAgent;
use super::types::{AgentBackend, AgentConfig, AgentType};
use super::{AgentResult, LlmProvider};
use crate::error::AgentError;

/// Unified agent trait that abstracts over both CLI and API agents
#[async_trait]
pub trait UnifiedAgent: Send + Sync {
    /// Get the agent's name
    fn name(&self) -> &str;

    /// Get the agent's ID
    fn id(&self) -> &str;

    /// Get the agent type
    fn agent_type(&self) -> AgentType;

    /// Check if the agent is available (CLI command exists or API is reachable)
    async fn is_available(&self) -> bool;

    /// Send a message and get a response
    async fn send(&self, message: &str) -> AgentResult<String>;
}

/// API-backed agent wrapping LlmProvider
pub struct ApiAgent {
    config: AgentConfig,
    provider: Box<dyn LlmProvider>,
}

impl ApiAgent {
    pub fn new(config: AgentConfig, provider: Box<dyn LlmProvider>) -> Self {
        Self { config, provider }
    }
}

#[async_trait]
impl UnifiedAgent for ApiAgent {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn id(&self) -> &str {
        &self.config.id
    }

    fn agent_type(&self) -> AgentType {
        self.config.agent_type.clone()
    }

    async fn is_available(&self) -> bool {
        // For API agents, check if we have an API key
        self.config.api_key.is_some()
    }

    async fn send(&self, message: &str) -> AgentResult<String> {
        use super::Agent;

        let mut agent = Agent::new(
            self.config.id.clone(),
            self.config.name.clone(),
            self.provider.box_clone(),
            self.config.model.clone(),
            self.config.system_prompt.clone(),
        );

        agent.chat(message).await
    }
}

/// CLI-backed agent (Claude Code, OpenCode)
pub struct CliUnifiedAgent {
    agent: CliAgent,
}

impl CliUnifiedAgent {
    pub fn new(agent: CliAgent) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl UnifiedAgent for CliUnifiedAgent {
    fn name(&self) -> &str {
        &self.agent.config.name
    }

    fn id(&self) -> &str {
        &self.agent.config.id
    }

    fn agent_type(&self) -> AgentType {
        self.agent.config.agent_type.clone()
    }

    async fn is_available(&self) -> bool {
        self.agent.check_availability().await.unwrap_or(false)
    }

    async fn send(&self, message: &str) -> AgentResult<String> {
        self.agent.send_message(message).await.map(|response| response.content)
    }
}

/// Agent manager - handles agent lifecycle and provides unified access
pub struct AgentManager {
    /// Active agents by ID
    agents: std::collections::HashMap<String, AgentConfig>,
    /// API keys by provider
    api_keys: std::collections::HashMap<String, String>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            agents: std::collections::HashMap::new(),
            api_keys: std::collections::HashMap::new(),
        }
    }

    /// Register an API key for a provider
    pub fn register_api_key(&mut self, provider: impl Into<String>, key: impl Into<String>) {
        self.api_keys.insert(provider.into(), key.into());
    }

    /// Register an agent configuration
    pub fn register_agent(&mut self, config: AgentConfig) {
        self.agents.insert(config.id.clone(), config);
    }

    /// Get an agent configuration
    pub fn get_agent(&self, id: &str) -> Option<&AgentConfig> {
        self.agents.get(id)
    }

    /// List all registered agents
    pub fn list_agents(&self) -> Vec<&AgentConfig> {
        self.agents.values().collect()
    }

    /// Create an agent instance based on its configuration
    pub fn create_agent(&self, id: &str) -> AgentResult<Box<dyn UnifiedAgent>> {
        let config = self
            .agents
            .get(id)
            .ok_or_else(|| AgentError::ApiRequest(format!("Agent not found: {}", id)))?;

        match config.backend {
            AgentBackend::Cli => {
                let cli_agent = CliAgent::new(config.clone())?;
                Ok(Box::new(CliUnifiedAgent::new(cli_agent)))
            }
            AgentBackend::Api => {
                let provider = self.create_provider(config)?;
                Ok(Box::new(ApiAgent::new(config.clone(), provider)))
            }
        }
    }

    /// Create an LLM provider based on config
    fn create_provider(&self, config: &AgentConfig) -> AgentResult<Box<dyn LlmProvider>> {
        let api_key = config
            .api_key
            .as_ref()
            .or_else(|| self.api_keys.get(config.agent_type.name()).map(|s| s.as_str()))
            .ok_or_else(|| AgentError::MissingApiKey {
                provider: config.agent_type.name().to_string(),
            })?;

        let provider: Box<dyn LlmProvider> = match config.agent_type {
            AgentType::Anthropic => Box::new(AnthropicProvider::new(api_key.to_string())),
            AgentType::OpenAi => Box::new(OpenAiProvider::new(api_key.to_string())),
            AgentType::Minimax => Box::new(MinimaxProvider::new(api_key.to_string())),
            _ => {
                return Err(AgentError::ApiRequest(format!(
                    "Unsupported API agent type: {:?}",
                    config.agent_type
                )))
            }
        };

        Ok(provider)
    }

    /// Check availability of all agents
    pub async fn check_availability(&self) -> Vec<(String, bool)> {
        let mut results = Vec::new();

        for (id, config) in &self.agents {
            let available = match config.backend {
                AgentBackend::Cli => {
                    let agent = CliAgent::new(config.clone());
                    match agent {
                        Ok(a) => a.check_availability().await.unwrap_or(false),
                        Err(_) => false,
                    }
                }
                AgentBackend::Api => config.api_key.is_some(),
            };
            results.push((id.clone(), available));
        }

        results
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Extended AgentType with name helper
impl AgentType {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
            Self::GenericCli => "generic-cli",
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Minimax => "minimax",
            Self::Custom => "custom",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_manager() {
        let mut manager = AgentManager::new();

        // Register an API key
        manager.register_api_key("anthropic", "test-key-123");

        // Register a CLI agent
        manager.register_agent(AgentConfig::claude_code("my-claude", "My Claude"));

        // Register an API agent (key will come from registered keys)
        let mut config = AgentConfig::anthropic("my-anthropic", "My Anthropic", "");
        config.api_key = None; // Will use registered key
        manager.register_agent(config);

        // List agents
        let agents = manager.list_agents();
        assert_eq!(agents.len(), 2);

        // Check availability
        // Note: This would fail if Claude isn't installed
    }

    #[test]
    fn test_agent_type_name() {
        assert_eq!(AgentType::ClaudeCode.name(), "claude-code");
        assert_eq!(AgentType::OpenAi.name(), "openai");
        assert_eq!(AgentType::Anthropic.name(), "anthropic");
    }
}
