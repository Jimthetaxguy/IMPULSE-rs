//! Impulse Agent — a coordinating AI that augments other agents' coding progress.
//!
//! The Impulse Agent can operate in two modes:
//! 1. **API mode**: Direct LLM API calls (Anthropic, OpenAI, Minimax) using an API key
//! 2. **Harness mode**: Delegates to a CLI harness (Claude Code, OpenCode) via subprocess
//!
//! The agent reads context from all panes via the context lifecycle extractor,
//! detects coordination needs, and generates actionable recommendations.

pub mod coordinator;
pub mod prompts;

use serde::{Deserialize, Serialize};

// Re-export LLM types for backward compatibility.
use crate::context_lifecycle::types::ExtractedInsight;
use crate::error::{AgentError, AgentResult};
pub use crate::llm_backends::anthropic::{AnthropicProvider, MinimaxProvider, OpenAiProvider};
pub use crate::llm_backends::{Agent, LlmProvider};
use coordinator::Recommendation;

/// The LLM provider to use for the Impulse Agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImpulseProvider {
    Anthropic,
    OpenAi,
    Minimax,
}

impl ImpulseProvider {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "gpt" => Some(Self::OpenAi),
            "minimax" => Some(Self::Minimax),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Minimax => "minimax",
        }
    }

    /// Default model for this provider.
    ///
    /// Checks `IMPULSE_MODEL` env var first (user override), then falls back
    /// to a compiled default. Returns an owned `String` because the env var
    /// path produces heap-allocated data.
    pub fn default_model(self) -> String {
        if let Ok(model) = std::env::var("IMPULSE_MODEL") {
            if !model.is_empty() {
                return model;
            }
        }
        match self {
            Self::Anthropic => "claude-sonnet-4-6".to_string(),
            Self::OpenAi => "gpt-4o".to_string(),
            Self::Minimax => "abab6.5s-chat".to_string(),
        }
    }

    /// Resolve an API key from config or environment variables.
    pub fn resolve_api_key(self, configured_key: Option<&str>) -> Option<String> {
        if let Some(key) = configured_key {
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
        // Fall back to environment variables
        match self {
            Self::Anthropic => std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("CLAUDE_API_KEY"))
                .ok(),
            Self::OpenAi => std::env::var("OPENAI_API_KEY").ok(),
            Self::Minimax => std::env::var("MINIMAX_API_KEY").ok(),
        }
    }
}

/// The harness mode — delegates to a CLI coding agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImpulseHarness {
    ClaudeCode,
    OpenCode,
}

impl ImpulseHarness {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "opencode" | "open-code" => Some(Self::OpenCode),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::OpenCode => "opencode",
        }
    }
}

/// Operating mode for the Impulse Agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AgentMode {
    /// Direct API calls to an LLM provider.
    Api {
        provider: ImpulseProvider,
        model: Option<String>,
    },
    /// Delegate to a CLI harness (Claude Code, OpenCode).
    Harness { harness: ImpulseHarness },
    /// Agent is disabled.
    #[default]
    Disabled,
}

/// Configuration for the Impulse Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpulseAgentConfig {
    /// Operating mode.
    pub mode: AgentMode,
    /// API key (for API mode). If None, falls back to env vars.
    #[serde(default, skip_serializing)]
    pub api_key: Option<String>,
    /// Whether to automatically review cross-pane activity.
    pub auto_review: bool,
    /// Whether to automatically coordinate cross-pane conflicts.
    pub auto_coordinate: bool,
    /// Minimum number of insights before triggering a review.
    pub review_threshold: usize,
    /// Maximum tokens to use per agent request.
    pub max_tokens: u32,
    /// Temperature for LLM requests.
    pub temperature: f32,
}

impl Default for ImpulseAgentConfig {
    fn default() -> Self {
        Self {
            mode: AgentMode::Disabled,
            api_key: None,
            auto_review: false,
            auto_coordinate: false,
            review_threshold: 5,
            max_tokens: 2048,
            temperature: 0.3,
        }
    }
}

impl ImpulseAgentConfig {
    /// Create a config for API mode with a specific provider.
    pub fn api(provider: ImpulseProvider) -> Self {
        Self {
            mode: AgentMode::Api {
                provider,
                model: None,
            },
            ..Default::default()
        }
    }

    /// Create a config for harness mode.
    pub fn harness(harness: ImpulseHarness) -> Self {
        Self {
            mode: AgentMode::Harness { harness },
            ..Default::default()
        }
    }

    /// Set the API key.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set the model (API mode only).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        if let AgentMode::Api {
            model: ref mut model_field,
            ..
        } = self.mode
        {
            *model_field = Some(model.into());
        }
        self
    }

    /// Enable auto-review.
    pub fn with_auto_review(mut self) -> Self {
        self.auto_review = true;
        self
    }

    /// Enable auto-coordination.
    pub fn with_auto_coordinate(mut self) -> Self {
        self.auto_coordinate = true;
        self
    }

    /// Check if the agent is enabled.
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, AgentMode::Disabled)
    }
}

/// The Impulse Agent — coordinates across agent panes using LLM intelligence.
pub struct ImpulseAgent {
    config: ImpulseAgentConfig,
    /// Internal Agent instance (for API mode).
    inner: Option<Agent>,
    /// Recent recommendations generated.
    recommendations: Vec<Recommendation>,
}

impl ImpulseAgent {
    /// Create a new ImpulseAgent from configuration.
    pub fn new(config: ImpulseAgentConfig) -> AgentResult<Self> {
        let inner = match &config.mode {
            AgentMode::Api { provider, model } => {
                let api_key = provider
                    .resolve_api_key(config.api_key.as_deref())
                    .ok_or_else(|| AgentError::MissingApiKey {
                        provider: provider.as_str().to_string(),
                    })?;

                let llm_provider: Box<dyn LlmProvider> = match provider {
                    ImpulseProvider::Anthropic => Box::new(AnthropicProvider::new(api_key)),
                    ImpulseProvider::OpenAi => Box::new(OpenAiProvider::new(api_key)),
                    ImpulseProvider::Minimax => Box::new(MinimaxProvider::new(api_key)),
                };

                let model_name = model.clone().unwrap_or_else(|| provider.default_model());

                Some(Agent::new(
                    "impulse-agent".to_string(),
                    "Impulse Agent".to_string(),
                    llm_provider,
                    Some(model_name),
                    None, // System prompt set per-request
                ))
            }
            AgentMode::Harness { .. } => None, // CLI harness doesn't use Agent
            AgentMode::Disabled => None,
        };

        Ok(Self {
            config,
            inner,
            recommendations: Vec::new(),
        })
    }

    /// Check if the agent is enabled and ready.
    pub fn is_ready(&self) -> bool {
        match &self.config.mode {
            AgentMode::Api { .. } => self.inner.is_some(),
            AgentMode::Harness { harness } => {
                // Check if the CLI command exists
                which::which(harness.command()).is_ok()
            }
            AgentMode::Disabled => false,
        }
    }

    /// Get the agent's configuration.
    pub fn config(&self) -> &ImpulseAgentConfig {
        &self.config
    }

    /// Get recent recommendations.
    pub fn recommendations(&self) -> &[Recommendation] {
        &self.recommendations
    }

    /// Run local coordination checks (no LLM needed).
    pub fn coordinate_local(&mut self, insights: &[ExtractedInsight]) -> Vec<Recommendation> {
        let recs = coordinator::run_local_coordination(insights);
        self.recommendations.extend(recs.clone());
        // Keep only last 50 recommendations
        if self.recommendations.len() > 50 {
            let drain_count = self.recommendations.len() - 50;
            self.recommendations.drain(..drain_count);
        }
        recs
    }

    /// Request a code review via the LLM (API mode).
    pub async fn review_code(
        &mut self,
        pane_name: &str,
        insights: &[String],
    ) -> AgentResult<String> {
        let agent = self
            .inner
            .as_mut()
            .ok_or_else(|| AgentError::InvalidRequest("Agent not in API mode".to_string()))?;

        agent.system_prompt = Some(prompts::CODE_REVIEW_SYSTEM.to_string());
        let user_msg = prompts::build_review_prompt(pane_name, insights);
        agent.chat(&user_msg).await
    }

    /// Request error analysis via the LLM (API mode).
    pub async fn analyze_error(
        &mut self,
        pane_name: &str,
        error_text: &str,
    ) -> AgentResult<String> {
        let agent = self
            .inner
            .as_mut()
            .ok_or_else(|| AgentError::InvalidRequest("Agent not in API mode".to_string()))?;

        agent.system_prompt = Some(prompts::ERROR_ANALYSIS_SYSTEM.to_string());
        let user_msg = prompts::build_error_prompt(pane_name, error_text);
        agent.chat(&user_msg).await
    }

    /// Request cross-pane coordination analysis via the LLM (API mode).
    pub async fn coordinate_llm(
        &mut self,
        pane_summaries: &[(String, Vec<String>)],
    ) -> AgentResult<String> {
        let agent = self
            .inner
            .as_mut()
            .ok_or_else(|| AgentError::InvalidRequest("Agent not in API mode".to_string()))?;

        agent.system_prompt = Some(prompts::COORDINATION_SYSTEM.to_string());
        let user_msg = prompts::build_coordination_prompt(pane_summaries);
        agent.chat(&user_msg).await
    }

    /// Request a task summary via the LLM (API mode).
    pub async fn summarize_pane(
        &mut self,
        pane_name: &str,
        raw_output: &str,
    ) -> AgentResult<String> {
        let agent = self
            .inner
            .as_mut()
            .ok_or_else(|| AgentError::InvalidRequest("Agent not in API mode".to_string()))?;

        agent.system_prompt = Some(prompts::SUMMARIZE_SYSTEM.to_string());
        let user_msg = prompts::build_summary_prompt(pane_name, raw_output);
        agent.chat(&user_msg).await
    }

    /// Run a CLI harness command and return the output.
    pub async fn harness_query(&self, prompt: &str) -> AgentResult<String> {
        let harness = match &self.config.mode {
            AgentMode::Harness { harness } => *harness,
            _ => {
                return Err(AgentError::InvalidRequest(
                    "Agent not in harness mode".to_string(),
                ))
            }
        };

        let output = tokio::process::Command::new(harness.command())
            .arg("--print")
            .arg(prompt)
            .output()
            .await
            .map_err(|e| {
                AgentError::ApiRequest(format!("Failed to run {}: {}", harness.command(), e))
            })?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(AgentError::ApiResponse(format!(
                "{} exited with {}: {}",
                harness.command(),
                output.status,
                stderr
            )))
        }
    }

    /// Generic query: routes to API or harness depending on mode.
    pub async fn query(&mut self, system_prompt: &str, user_prompt: &str) -> AgentResult<String> {
        match &self.config.mode {
            AgentMode::Api { .. } => {
                let agent = self.inner.as_mut().ok_or_else(|| {
                    AgentError::InvalidRequest("Agent not initialized".to_string())
                })?;
                agent.system_prompt = Some(system_prompt.to_string());
                agent.chat(user_prompt).await
            }
            AgentMode::Harness { .. } => {
                // Harness mode: combine system and user prompt into a single message
                let combined = format!("{}\n\n{}", system_prompt, user_prompt);
                self.harness_query(&combined).await
            }
            AgentMode::Disabled => Err(AgentError::InvalidRequest("Agent is disabled".to_string())),
        }
    }

    /// Clear conversation history (API mode only).
    pub fn clear_history(&mut self) {
        if let Some(agent) = &mut self.inner {
            agent.clear_history();
        }
    }

    /// Get a status summary of the agent.
    pub fn status_summary(&self) -> String {
        match &self.config.mode {
            AgentMode::Api { provider, model } => {
                let default = provider.default_model();
                let model_name = model.as_deref().unwrap_or(&default);
                let ready = if self.is_ready() {
                    "ready"
                } else {
                    "no API key"
                };
                format!("API ({}, {}) [{}]", provider.as_str(), model_name, ready)
            }
            AgentMode::Harness { harness } => {
                let ready = if self.is_ready() {
                    "available"
                } else {
                    "not found"
                };
                format!("Harness ({}) [{}]", harness.as_str(), ready)
            }
            AgentMode::Disabled => "Disabled".to_string(),
        }
    }
}

/// Resolve an ImpulseAgent from state config values.
/// Returns None if the agent is disabled or cannot be created.
pub fn resolve_from_config(
    provider_str: Option<&str>,
    api_key: Option<&str>,
    model: Option<&str>,
    harness_str: Option<&str>,
) -> Option<ImpulseAgent> {
    // Harness takes priority if specified
    if let Some(h) = harness_str.and_then(ImpulseHarness::parse) {
        let config = ImpulseAgentConfig::harness(h);
        return ImpulseAgent::new(config).ok();
    }

    // Then try API mode
    if let Some(p) = provider_str.and_then(ImpulseProvider::parse) {
        let mut config = ImpulseAgentConfig::api(p);
        if let Some(key) = api_key {
            config = config.with_api_key(key);
        }
        if let Some(m) = model {
            config = config.with_model(m);
        }
        return ImpulseAgent::new(config).ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_impulse_provider_parse() {
        assert_eq!(
            ImpulseProvider::parse("anthropic"),
            Some(ImpulseProvider::Anthropic)
        );
        assert_eq!(
            ImpulseProvider::parse("claude"),
            Some(ImpulseProvider::Anthropic)
        );
        assert_eq!(
            ImpulseProvider::parse("openai"),
            Some(ImpulseProvider::OpenAi)
        );
        assert_eq!(ImpulseProvider::parse("gpt"), Some(ImpulseProvider::OpenAi));
        assert_eq!(
            ImpulseProvider::parse("minimax"),
            Some(ImpulseProvider::Minimax)
        );
        assert_eq!(ImpulseProvider::parse("invalid"), None);
    }

    #[test]
    fn test_impulse_harness_parse() {
        assert_eq!(
            ImpulseHarness::parse("claude-code"),
            Some(ImpulseHarness::ClaudeCode)
        );
        assert_eq!(
            ImpulseHarness::parse("claude"),
            Some(ImpulseHarness::ClaudeCode)
        );
        assert_eq!(
            ImpulseHarness::parse("opencode"),
            Some(ImpulseHarness::OpenCode)
        );
        assert_eq!(ImpulseHarness::parse("invalid"), None);
    }

    #[test]
    fn test_agent_mode_default_is_disabled() {
        let mode = AgentMode::default();
        assert_eq!(mode, AgentMode::Disabled);
    }

    #[test]
    fn test_config_api() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic)
            .with_api_key("test-key")
            .with_model("claude-opus-4-5-20250514");
        assert!(config.is_enabled());
        match &config.mode {
            AgentMode::Api { provider, model } => {
                assert_eq!(*provider, ImpulseProvider::Anthropic);
                assert_eq!(model.as_deref(), Some("claude-opus-4-5-20250514"));
            }
            _ => panic!("Expected API mode"),
        }
    }

    #[test]
    fn test_config_harness() {
        let config = ImpulseAgentConfig::harness(ImpulseHarness::ClaudeCode);
        assert!(config.is_enabled());
        match &config.mode {
            AgentMode::Harness { harness } => {
                assert_eq!(*harness, ImpulseHarness::ClaudeCode);
            }
            _ => panic!("Expected Harness mode"),
        }
    }

    #[test]
    fn test_config_disabled() {
        let config = ImpulseAgentConfig::default();
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_agent_new_disabled() {
        let config = ImpulseAgentConfig::default();
        let agent = ImpulseAgent::new(config).unwrap();
        assert!(!agent.is_ready());
        assert_eq!(agent.status_summary(), "Disabled");
    }

    #[test]
    fn test_agent_new_api_no_key() {
        // Without env var set, should fail
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLAUDE_API_KEY");
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic);
        let result = ImpulseAgent::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_new_api_with_key() {
        let config =
            ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key-123");
        let agent = ImpulseAgent::new(config).unwrap();
        assert!(agent.is_ready());
        assert!(agent.status_summary().contains("anthropic"));
        assert!(agent.status_summary().contains("ready"));
    }

    #[test]
    fn test_agent_local_coordination() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        let insights = vec![
            ExtractedInsight {
                pane_id: 1,
                agent_kind: crate::context_lifecycle::types::AgentKind::ClaudeCode,
                timestamp: chrono::Utc::now(),
                insight_type: crate::context_lifecycle::types::InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
            ExtractedInsight {
                pane_id: 2,
                agent_kind: crate::context_lifecycle::types::AgentKind::OpenCode,
                timestamp: chrono::Utc::now(),
                insight_type: crate::context_lifecycle::types::InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
        ];

        let recs = agent.coordinate_local(&insights);
        assert_eq!(recs.len(), 1);
        assert_eq!(
            recs[0].recommendation_type,
            coordinator::RecommendationType::FileConflict
        );
    }

    #[test]
    fn test_resolve_from_config_disabled() {
        let agent = resolve_from_config(None, None, None, None);
        assert!(agent.is_none());
    }

    #[test]
    fn test_resolve_from_config_harness() {
        let agent = resolve_from_config(None, None, None, Some("claude-code"));
        assert!(agent.is_some());
    }

    #[test]
    fn test_resolve_from_config_api() {
        let agent = resolve_from_config(Some("anthropic"), Some("test-key"), None, None);
        assert!(agent.is_some());
        let agent = agent.unwrap();
        assert!(agent.is_ready());
    }

    #[test]
    fn test_provider_default_model() {
        // When IMPULSE_MODEL is not set, returns the compiled default.
        std::env::remove_var("IMPULSE_MODEL");
        assert_eq!(
            ImpulseProvider::Anthropic.default_model(),
            "claude-sonnet-4-6"
        );
        assert_eq!(ImpulseProvider::OpenAi.default_model(), "gpt-4o");
        assert_eq!(ImpulseProvider::Minimax.default_model(), "abab6.5s-chat");
    }

    #[test]
    fn test_provider_default_model_env_override() {
        // IMPULSE_MODEL env var takes priority over compiled default.
        std::env::set_var("IMPULSE_MODEL", "claude-opus-4-6");
        let model = ImpulseProvider::Anthropic.default_model();
        std::env::remove_var("IMPULSE_MODEL");
        assert_eq!(model, "claude-opus-4-6");
    }

    #[test]
    fn test_harness_command() {
        assert_eq!(ImpulseHarness::ClaudeCode.command(), "claude");
        assert_eq!(ImpulseHarness::OpenCode.command(), "opencode");
    }
}
