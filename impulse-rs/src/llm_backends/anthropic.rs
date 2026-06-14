use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;

use super::{ChatRequest, ChatResponse, LlmProvider, Message, Role, Usage};
use crate::error::{AgentError, AgentResult};

/// Total request timeout for an LLM call. Long enough for slow completions,
/// bounded so a hung connection can never block the daemon indefinitely.
const HTTP_REQUEST_TIMEOUT_SECS: u64 = 120;
/// Connection-establishment timeout (fail fast when the endpoint is unreachable).
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Build the shared HTTP client with bounded timeouts. Falls back to a default
/// client if the builder fails (it never does for static timeout config).
fn build_http_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
        .connect_timeout(std::time::Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Common provider structure - shared by all LLM providers
pub struct BaseProvider {
    api_key: String,
    http_client: Arc<Client>,
    provider_name: &'static str,
    default_model: String,
}

impl Clone for BaseProvider {
    fn clone(&self) -> Self {
        Self {
            api_key: self.api_key.clone(),
            // Share the connection pool (and its timeout config) rather than
            // spinning up a fresh client per clone.
            http_client: Arc::clone(&self.http_client),
            provider_name: self.provider_name,
            default_model: self.default_model.clone(),
        }
    }
}

impl BaseProvider {
    pub fn new(provider_name: &'static str, api_key: String, default_model: &'static str) -> Self {
        Self {
            api_key,
            http_client: Arc::new(build_http_client()),
            provider_name,
            default_model: default_model.to_string(),
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.default_model = model.to_string();
        self
    }

    pub fn check_api_key(&self) -> AgentResult<()> {
        if self.api_key.is_empty() {
            return Err(AgentError::MissingApiKey {
                provider: self.provider_name.to_string(),
            });
        }
        Ok(())
    }

    /// Convert messages to provider format
    pub fn format_messages(messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    "content": m.content
                })
            })
            .collect()
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}

// =============================================================================
// Anthropic Provider
// =============================================================================

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    // dead_code: deserialized from Anthropic API response; retained for logging and debugging
    #[allow(dead_code)]
    id: String,
    model: String,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    // dead_code: deserialized from Anthropic API response; required by serde to round-trip the "type" field
    #[allow(dead_code)]
    _type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

pub struct AnthropicProvider(BaseProvider);

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self(BaseProvider::new("anthropic", api_key, "claude-sonnet-4-6"))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> &str {
        &self.0.default_model
    }

    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
        self.0.check_api_key()?;

        let system = request
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.clone());
        let non_system: Vec<_> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .cloned()
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "temperature": request.temperature,
            "messages": BaseProvider::format_messages(&non_system),
        });

        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys);
        }

        let response = self
            .0
            .http_client()
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.0.api_key())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ApiRequest(e.to_string()))?;

        // Check status and get error details if failed
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                429 => AgentError::RateLimited,
                401 => AgentError::Authentication(text),
                _ => AgentError::ApiRequest(format!("{} - {}", status, text)),
            });
        }

        let resp: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ApiResponse(e.to_string()))?;

        let content = resp
            .content
            .iter()
            .filter_map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("");

        Ok(ChatResponse {
            content,
            model: resp.model,
            usage: Usage {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
            },
        })
    }

    fn supported_models(&self) -> Vec<&str> {
        vec![
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "claude-opus-4-5-20250514",
            "claude-sonnet-4-20250514",
            "claude-3-5-sonnet-20241022",
        ]
    }
}

// =============================================================================
// OpenAI Provider
// =============================================================================

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: String,
    usage: OpenAiUsage,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    // dead_code: deserialized from OpenAI API response; kept for future cost-tracking and diagnostic use
    #[allow(dead_code)]
    total_tokens: u32,
}

pub struct OpenAiProvider(BaseProvider);

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        Self(BaseProvider::new("openai", api_key, "gpt-4o"))
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> &str {
        &self.0.default_model
    }

    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
        self.0.check_api_key()?;

        let messages = BaseProvider::format_messages(&request.messages);
        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        let response = self
            .0
            .http_client()
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.0.api_key()))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ApiRequest(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                429 => AgentError::RateLimited,
                401 => AgentError::Authentication(text),
                _ => AgentError::ApiRequest(format!("{} - {}", status, text)),
            });
        }

        let resp: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ApiResponse(e.to_string()))?;

        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(ChatResponse {
            content,
            model: resp.model,
            usage: Usage {
                input_tokens: resp.usage.prompt_tokens,
                output_tokens: resp.usage.completion_tokens,
            },
        })
    }

    fn supported_models(&self) -> Vec<&str> {
        vec![
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4-turbo",
            "gpt-4",
            "gpt-3.5-turbo",
        ]
    }
}

// =============================================================================
// Minimax Provider
// =============================================================================

#[derive(Debug, Deserialize)]
struct MinimaxResponse {
    choices: Vec<MinimaxChoice>,
    usage: MinimaxUsage,
}

#[derive(Debug, Deserialize)]
struct MinimaxChoice {
    message: MinimaxMessage,
}

#[derive(Debug, Deserialize)]
struct MinimaxMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct MinimaxUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    // dead_code: deserialized from Minimax API response; kept for future cost-tracking and diagnostic use
    #[allow(dead_code)]
    total_tokens: u32,
}

pub struct MinimaxProvider(BaseProvider);

impl MinimaxProvider {
    pub fn new(api_key: String) -> Self {
        Self(BaseProvider::new("minimax", api_key, "abab6.5s-chat"))
    }
}

#[async_trait]
impl LlmProvider for MinimaxProvider {
    fn name(&self) -> &str {
        "minimax"
    }

    fn default_model(&self) -> &str {
        &self.0.default_model
    }

    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
        self.0.check_api_key()?;

        let messages = BaseProvider::format_messages(&request.messages);
        let body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

        let response = self
            .0
            .http_client()
            .post("https://api.minimax.chat/v1/text/chatcompletion_v2")
            .header("Authorization", format!("Bearer {}", self.0.api_key()))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::ApiRequest(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                429 => AgentError::RateLimited,
                401 => AgentError::Authentication(text),
                _ => AgentError::ApiRequest(format!("{} - {}", status, text)),
            });
        }

        let resp: MinimaxResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ApiResponse(e.to_string()))?;

        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(ChatResponse {
            content,
            model: request.model,
            usage: Usage {
                input_tokens: resp.usage.prompt_tokens,
                output_tokens: resp.usage.completion_tokens,
            },
        })
    }

    fn supported_models(&self) -> Vec<&str> {
        vec!["abab6.5s-chat", "abab6.5g-chat", "abab5.5s-chat"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_provider_new() {
        let provider = BaseProvider::new("test", "api_key123".to_string(), "gpt-4");
        assert_eq!(provider.api_key(), "api_key123");
    }

    #[test]
    fn test_base_provider_clone_shares_http_client() {
        // Cloning must reuse the same (timeout-configured) connection pool
        // rather than building a fresh client each time.
        let provider = BaseProvider::new("test", "api_key".to_string(), "gpt-4");
        let cloned = provider.clone();
        assert!(
            Arc::ptr_eq(&provider.http_client, &cloned.http_client),
            "clone should share the http client Arc"
        );
    }

    #[test]
    fn test_base_provider_with_model() {
        let provider = BaseProvider::new("test", "api_key".to_string(), "gpt-4");
        let provider = provider.with_model("gpt-4o");
        assert_eq!(provider.default_model, "gpt-4o");
    }

    #[test]
    fn test_base_provider_check_api_key_valid() {
        let provider = BaseProvider::new("test", "api_key123".to_string(), "gpt-4");
        assert!(provider.check_api_key().is_ok());
    }

    #[test]
    fn test_base_provider_check_api_key_empty() {
        let provider = BaseProvider::new("test", "".to_string(), "gpt-4");
        let result = provider.check_api_key();
        assert!(result.is_err());
    }

    #[test]
    fn test_format_messages() {
        let messages = vec![
            Message {
                role: Role::System,
                content: "You are helpful".to_string(),
            },
            Message {
                role: Role::User,
                content: "Hello".to_string(),
            },
        ];

        let formatted = BaseProvider::format_messages(&messages);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(formatted[0]["content"], "You are helpful");
        assert_eq!(formatted[1]["role"], "user");
        assert_eq!(formatted[1]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_provider_new() {
        let provider = AnthropicProvider::new("test_key".to_string());
        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn test_openai_provider_new() {
        let provider = OpenAiProvider::new("test_key".to_string());
        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn test_minimax_provider_new() {
        let provider = MinimaxProvider::new("test_key".to_string());
        assert_eq!(provider.name(), "minimax");
    }
}
