use crate::intent::types::{Activity, AgentIntent, IntentCategory};
use std::env;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("API key not configured")]
    ApiKeyMissing,

    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),
}

/// Result type for intent detection
pub type IntentResult<T> = Result<T, ProviderError>;

/// AI provider trait for intent classification
pub trait IntentProvider: Send + Sync {
    /// Classify intent using AI
    fn classify(&self, activity: &Activity) -> IntentResult<AgentIntent>;

    /// Check if provider is available (API key configured, etc.)
    fn is_available(&self) -> bool;

    /// Provider name
    fn name(&self) -> &'static str;

    /// Extract intent from agent output
    fn extract_goal(&self, output: &str) -> IntentResult<String>;
}

/// Claude API provider
pub struct ClaudeProvider {
    api_key: Option<String>,
    model: String,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        let api_key = env::var("ANTHROPIC_API_KEY").ok();
        Self {
            api_key,
            model: "claude-3-haiku-20240307".to_string(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    #[allow(dead_code)]
    fn build_prompt(&self, activity: &Activity) -> String {
        format!(
            r#"Analyze this AI agent activity and classify the intent.

Agent ID: {}
Agent Type: {}
Activity Type: {}
Target: {}

Details:
{}

Respond with a JSON object containing:
- "category": one of [refactoring, implementing, testing, debugging, documenting, analyzing, configuring, deploying]
- "goal": a brief description of what the agent is trying to accomplish
- "confidence": a float between 0 and 1 indicating classification confidence
- "complexity": one of [low, medium, high]

Example response:
{{"category": "refactoring", "goal": "Simplify token validation logic", "confidence": 0.85, "complexity": "medium"}}"#,
            activity.agent_id,
            activity.agent_type.as_str(),
            activity.activity_type.as_str(),
            activity.target.as_deref().unwrap_or("none"),
            activity.details.join("\n")
        )
    }
}

impl IntentProvider for ClaudeProvider {
    fn classify(&self, activity: &Activity) -> IntentResult<AgentIntent> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        // For now, return a placeholder - actual API call would go here
        // This is a placeholder for the full implementation
        let mut intent = AgentIntent::new(activity.agent_id.clone(), activity.agent_type);

        intent.goal = "AI-classified intent (placeholder)".to_string();
        intent.confidence = 0.8;
        intent.intent_category = IntentCategory::Unknown;

        Ok(intent)
    }

    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    fn name(&self) -> &'static str {
        "claude"
    }

    fn extract_goal(&self, output: &str) -> IntentResult<String> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        // Placeholder - actual implementation would call Claude API
        Ok(output.lines().next().unwrap_or("").to_string())
    }
}

/// OpenAI API provider
pub struct OpenAIProvider {
    api_key: Option<String>,
    model: String,
}

impl Default for OpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAIProvider {
    pub fn new() -> Self {
        let api_key = env::var("OPENAI_API_KEY").ok();
        Self {
            api_key,
            model: "gpt-4o-mini".to_string(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

impl IntentProvider for OpenAIProvider {
    fn classify(&self, activity: &Activity) -> IntentResult<AgentIntent> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        let mut intent = AgentIntent::new(activity.agent_id.clone(), activity.agent_type);

        intent.goal = "AI-classified intent (placeholder)".to_string();
        intent.confidence = 0.8;
        intent.intent_category = IntentCategory::Unknown;

        Ok(intent)
    }

    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    fn name(&self) -> &'static str {
        "openai"
    }

    fn extract_goal(&self, output: &str) -> IntentResult<String> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        Ok(output.lines().next().unwrap_or("").to_string())
    }
}

/// Minimax API provider
pub struct MinimaxProvider {
    api_key: Option<String>,
    model: String,
}

impl Default for MinimaxProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MinimaxProvider {
    pub fn new() -> Self {
        let api_key = env::var("MINIMAX_API_KEY").ok();
        Self {
            api_key,
            model: "abab6.5s-chat".to_string(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

impl IntentProvider for MinimaxProvider {
    fn classify(&self, activity: &Activity) -> IntentResult<AgentIntent> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        let mut intent = AgentIntent::new(activity.agent_id.clone(), activity.agent_type);

        intent.goal = "AI-classified intent (placeholder)".to_string();
        intent.confidence = 0.8;
        intent.intent_category = IntentCategory::Unknown;

        Ok(intent)
    }

    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }

    fn name(&self) -> &'static str {
        "minimax"
    }

    fn extract_goal(&self, output: &str) -> IntentResult<String> {
        if !self.is_available() {
            return Err(ProviderError::ApiKeyMissing);
        }

        Ok(output.lines().next().unwrap_or("").to_string())
    }
}

/// Provider factory for creating configured providers
pub struct ProviderFactory;

impl ProviderFactory {
    /// Create all available providers
    pub fn create_all() -> Vec<Box<dyn IntentProvider>> {
        vec![
            Box::new(ClaudeProvider::new()),
            Box::new(OpenAIProvider::new()),
            Box::new(MinimaxProvider::new()),
        ]
    }

    /// Create the best available provider
    pub fn create_best() -> Box<dyn IntentProvider> {
        // Try in order of preference
        if let Ok(provider) = std::env::var("PREFERRED_INTENT_PROVIDER") {
            let providers = Self::create_all();
            for p in providers {
                if p.name() == provider {
                    return p;
                }
            }
        }

        // Default: first available
        for provider in Self::create_all() {
            if provider.is_available() {
                return provider;
            }
        }

        // Fallback: return Claude provider even if unavailable;
        // callers check is_available() before using it.
        Box::new(ClaudeProvider::new())
    }

    /// Get list of available provider names
    pub fn available() -> Vec<&'static str> {
        Self::create_all()
            .into_iter()
            .filter(|p| p.is_available())
            .map(|p| p.name())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_availability() {
        let claude = ClaudeProvider::new();
        let openai = OpenAIProvider::new();
        let minimax = MinimaxProvider::new();

        // These will be unavailable without API keys
        // The test just checks the method works
        let _ = claude.is_available();
        let _ = openai.is_available();
        let _ = minimax.is_available();
    }

    #[test]
    fn test_provider_names() {
        let claude = ClaudeProvider::new();
        let openai = OpenAIProvider::new();
        let minimax = MinimaxProvider::new();

        assert_eq!(claude.name(), "claude");
        assert_eq!(openai.name(), "openai");
        assert_eq!(minimax.name(), "minimax");
    }
}
