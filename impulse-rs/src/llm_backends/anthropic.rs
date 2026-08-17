use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use std::sync::Arc;

use super::{ChatRequest, ChatResponse, LlmProvider, Message, Role, StopReason, ToolCall, Usage};
use crate::error::{AgentError, AgentResult};

/// Total request timeout for an LLM call. Long enough for slow completions,
/// bounded so a hung connection can never block the daemon indefinitely.
const HTTP_REQUEST_TIMEOUT_SECS: u64 = 120;
/// Connection-establishment timeout (fail fast when the endpoint is unreachable).
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Build the shared HTTP client with bounded defaults. Every provider request
/// also applies the same request-local timeout, so the fallback client cannot
/// silently become unbounded if platform TLS/client initialization rejects the
/// configured builder.
fn build_http_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
        .connect_timeout(std::time::Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn bounded_request(request: RequestBuilder) -> RequestBuilder {
    request.timeout(std::time::Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
}

/// Canonical API origins. Each provider appends its own request path, so these
/// are scheme + host only — never a full endpoint.
const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com";
const MINIMAX_DEFAULT_BASE_URL: &str = "https://api.minimax.chat";

/// Env vars that redirect a provider at a different origin. The override exists
/// so an eval harness or local proxy can intercept provider traffic without a
/// rebuild; it is deliberately origin-only so a redirect cannot rewrite the
/// request path.
const ANTHROPIC_BASE_URL_ENV: &str = "ANTHROPIC_BASE_URL";
const OPENAI_BASE_URL_ENV: &str = "OPENAI_BASE_URL";
const MINIMAX_BASE_URL_ENV: &str = "MINIMAX_BASE_URL";

/// Resolve a base URL from, in precedence order: explicit config, environment
/// override, canonical default. An override that is blank or carries no
/// `http(s)` scheme is discarded rather than used, so a typo degrades to the
/// real API instead of to an unresolvable endpoint.
fn select_base_url(explicit: Option<&str>, from_env: Option<&str>, default: &str) -> String {
    explicit
        .or(from_env)
        .map(str::trim)
        .filter(|candidate| candidate.starts_with("http://") || candidate.starts_with("https://"))
        .unwrap_or(default)
        .trim_end_matches('/')
        .to_string()
}

/// How a resolved origin relates to the provider's canonical default.
///
/// Any `http(s)` origin is accepted today — there is no host allowlist. The
/// classification exists so an active override is observable (never the API
/// key) and so cleartext HTTP to a non-loopback host is a warning, not silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseUrlKind {
    Canonical,
    OverrideHttps,
    OverrideLoopbackHttp,
    OverrideCleartextHttp,
}

fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    if let Some(rest) = rest.strip_prefix('[') {
        return rest.split(']').next();
    }
    rest.split([':', '/']).next()
}

fn host_is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "::1" || host.starts_with("127.")
}

fn classify_base_url(resolved: &str, default: &str) -> BaseUrlKind {
    let canonical = default.trim_end_matches('/');
    if resolved == canonical {
        return BaseUrlKind::Canonical;
    }
    if resolved.starts_with("https://") {
        return BaseUrlKind::OverrideHttps;
    }
    if resolved.starts_with("http://") {
        if origin_host(resolved).is_some_and(host_is_loopback) {
            return BaseUrlKind::OverrideLoopbackHttp;
        }
        return BaseUrlKind::OverrideCleartextHttp;
    }
    BaseUrlKind::Canonical
}

fn log_base_url_override(provider_name: &str, origin: &str, kind: BaseUrlKind) {
    match kind {
        BaseUrlKind::Canonical => {}
        BaseUrlKind::OverrideHttps | BaseUrlKind::OverrideLoopbackHttp => {
            tracing::info!(
                provider = provider_name,
                origin,
                "LLM provider base-URL override active"
            );
        }
        BaseUrlKind::OverrideCleartextHttp => {
            tracing::warn!(
                provider = provider_name,
                origin,
                "LLM provider base-URL override uses cleartext HTTP to a non-loopback origin"
            );
        }
    }
}

/// Common provider structure - shared by all LLM providers
pub struct BaseProvider {
    api_key: String,
    http_client: Arc<Client>,
    provider_name: &'static str,
    default_model: String,
    base_url: Option<String>,
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
            base_url: self.base_url.clone(),
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
            base_url: None,
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.default_model = model.to_string();
        self
    }

    /// Pin this provider to a specific API origin, outranking any env override.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Full request URL for one provider endpoint, honoring the configured or
    /// env-supplied origin. `path` must start with `/`.
    pub fn endpoint(&self, env_var: &str, default_base_url: &str, path: &str) -> String {
        let from_env = std::env::var(env_var).ok();
        let base = select_base_url(
            self.base_url.as_deref(),
            from_env.as_deref(),
            default_base_url,
        );
        log_base_url_override(
            self.provider_name,
            &base,
            classify_base_url(&base, default_base_url),
        );
        format!("{base}{path}")
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
    /// Anthropic's stop reason string (`"end_turn"`, `"tool_use"`,
    /// `"max_tokens"`, `"stop_sequence"`, ...) — mapped to [`StopReason`] in
    /// `AnthropicProvider::chat`. Absent on some malformed/legacy responses,
    /// hence `Option`.
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
    // Present on `tool_use` blocks only.
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Renders messages for the Anthropic Messages API. Plain text messages use
/// the simple `{"role", "content": "..."}` shape (matching
/// `BaseProvider::format_messages`); messages carrying `tool_calls` or
/// `tool_results` (TUI_SPEC.md T9) render `content` as a block array per
/// Anthropic's tool-use protocol instead. Kept Anthropic-specific rather
/// than folded into `BaseProvider::format_messages` because OpenAI/Minimax
/// don't support tool-use blocks yet.
fn format_anthropic_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                // System messages are filtered out before this is called
                // (Anthropic takes `system` as a top-level request field),
                // but map defensively to "user" rather than panic if one
                // ever slips through.
                Role::System | Role::User => "user",
                Role::Assistant => "assistant",
            };

            if !m.tool_calls.is_empty() {
                let mut blocks = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                }
                for call in &m.tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": call.input,
                    }));
                }
                serde_json::json!({"role": role, "content": blocks})
            } else if !m.tool_results.is_empty() {
                let blocks: Vec<serde_json::Value> = m
                    .tool_results
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": r.tool_use_id,
                            "content": r.content,
                            "is_error": r.is_error,
                        })
                    })
                    .collect();
                serde_json::json!({"role": role, "content": blocks})
            } else {
                serde_json::json!({"role": role, "content": m.content})
            }
        })
        .collect()
}

pub struct AnthropicProvider(BaseProvider);

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self(BaseProvider::new("anthropic", api_key, "claude-sonnet-4-6"))
    }

    fn endpoint(&self) -> String {
        self.0.endpoint(
            ANTHROPIC_BASE_URL_ENV,
            ANTHROPIC_DEFAULT_BASE_URL,
            "/v1/messages",
        )
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

        let (system_msgs, non_system): (Vec<_>, Vec<_>) = request
            .messages
            .into_iter()
            .partition(|m| m.role == Role::System);

        let system = system_msgs.into_iter().next().map(|m| m.content);

        let mut body = serde_json::json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "temperature": request.temperature,
            "messages": format_anthropic_messages(&non_system),
        });

        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::to_value(&request.tools)
                .map_err(|e| AgentError::InvalidRequest(format!("failed to encode tools: {e}")))?;
        }

        let response = bounded_request(self.0.http_client().post(self.endpoint()))
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
            .filter(|c| c.block_type == "text")
            .filter_map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("");

        let tool_calls: Vec<ToolCall> = resp
            .content
            .iter()
            .filter(|c| c.block_type == "tool_use")
            .map(|c| ToolCall {
                id: c.id.clone().unwrap_or_default(),
                name: c.name.clone().unwrap_or_default(),
                input: c.input.clone().unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let stop_reason = match resp.stop_reason.as_deref() {
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("end_turn") | Some("stop_sequence") => StopReason::EndTurn,
            _ => StopReason::Other,
        };

        Ok(ChatResponse {
            content,
            model: resp.model,
            usage: Usage {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
            },
            stop_reason,
            tool_calls,
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

    fn endpoint(&self) -> String {
        self.0.endpoint(
            OPENAI_BASE_URL_ENV,
            OPENAI_DEFAULT_BASE_URL,
            "/v1/chat/completions",
        )
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

        let response = bounded_request(self.0.http_client().post(self.endpoint()))
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
            // OpenAI tool-calling isn't wired up yet -- always a plain reply.
            stop_reason: StopReason::EndTurn,
            tool_calls: Vec::new(),
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

    fn endpoint(&self) -> String {
        self.0.endpoint(
            MINIMAX_BASE_URL_ENV,
            MINIMAX_DEFAULT_BASE_URL,
            "/v1/text/chatcompletion_v2",
        )
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

        let response = bounded_request(self.0.http_client().post(self.endpoint()))
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
            // Minimax tool-calling isn't wired up yet -- always a plain reply.
            stop_reason: StopReason::EndTurn,
            tool_calls: Vec::new(),
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
    fn test_bounded_request_applies_timeout_even_to_fallback_client() {
        let request = bounded_request(Client::new().get("http://127.0.0.1/"))
            .build()
            .unwrap();
        assert_eq!(
            request.timeout().copied(),
            Some(std::time::Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
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
            Message::text(Role::System, "You are helpful"),
            Message::text(Role::User, "Hello"),
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

    /// Serializes the tests that mutate process-wide environment variables
    /// against the ones asserting the unset-env defaults.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_select_base_url_falls_back_to_default_when_nothing_set() {
        assert_eq!(
            select_base_url(None, None, "https://api.anthropic.com"),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn test_select_base_url_env_overrides_default() {
        assert_eq!(
            select_base_url(
                None,
                Some("http://127.0.0.1:8080"),
                "https://api.anthropic.com"
            ),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn test_select_base_url_explicit_wins_over_env() {
        assert_eq!(
            select_base_url(
                Some("http://explicit.test"),
                Some("http://from-env.test"),
                "https://api.anthropic.com"
            ),
            "http://explicit.test"
        );
    }

    #[test]
    fn test_select_base_url_blank_override_falls_back_to_default() {
        assert_eq!(
            select_base_url(Some("   "), Some(""), "https://api.anthropic.com"),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn test_select_base_url_override_without_http_scheme_falls_back_to_default() {
        assert_eq!(
            select_base_url(None, Some("api.anthropic.com"), "https://api.anthropic.com"),
            "https://api.anthropic.com"
        );
    }

    #[test]
    fn test_select_base_url_strips_trailing_slash() {
        assert_eq!(
            select_base_url(
                None,
                Some("http://127.0.0.1:8080/"),
                "https://api.anthropic.com"
            ),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn test_classify_base_url_canonical_default_is_canonical() {
        assert_eq!(
            classify_base_url("https://api.anthropic.com", "https://api.anthropic.com"),
            BaseUrlKind::Canonical
        );
    }

    #[test]
    fn test_classify_base_url_https_override_is_https() {
        assert_eq!(
            classify_base_url("https://proxy.example.test", "https://api.anthropic.com"),
            BaseUrlKind::OverrideHttps
        );
    }

    #[test]
    fn test_classify_base_url_loopback_http_is_loopback() {
        assert_eq!(
            classify_base_url("http://127.0.0.1:4010", "https://api.anthropic.com"),
            BaseUrlKind::OverrideLoopbackHttp
        );
        assert_eq!(
            classify_base_url("http://localhost:4010", "https://api.anthropic.com"),
            BaseUrlKind::OverrideLoopbackHttp
        );
        assert_eq!(
            classify_base_url("http://[::1]:4010", "https://api.anthropic.com"),
            BaseUrlKind::OverrideLoopbackHttp
        );
    }

    #[test]
    fn test_classify_base_url_non_loopback_http_is_cleartext_warning() {
        assert_eq!(
            classify_base_url("http://proxy.example.test", "https://api.anthropic.com"),
            BaseUrlKind::OverrideCleartextHttp
        );
    }

    #[test]
    fn test_base_provider_endpoint_reads_env_var() {
        let _guard = env_guard();
        let var = "IMPULSE_TEST_PROVIDER_BASE_URL";
        std::env::set_var(var, "http://127.0.0.1:9999");
        let provider = BaseProvider::new("test", "api_key".to_string(), "gpt-4");
        let url = provider.endpoint(var, "https://api.example.com", "/v1/messages");
        std::env::remove_var(var);
        assert_eq!(url, "http://127.0.0.1:9999/v1/messages");
    }

    #[test]
    fn test_base_provider_with_base_url_beats_env_var() {
        let _guard = env_guard();
        let var = "IMPULSE_TEST_PROVIDER_BASE_URL_EXPLICIT";
        std::env::set_var(var, "http://from-env.test");
        let provider = BaseProvider::new("test", "api_key".to_string(), "gpt-4")
            .with_base_url("http://explicit.test");
        let url = provider.endpoint(var, "https://api.example.com", "/v1/messages");
        std::env::remove_var(var);
        assert_eq!(url, "http://explicit.test/v1/messages");
    }

    #[test]
    fn test_anthropic_endpoint_defaults_to_canonical_url() {
        let _guard = env_guard();
        std::env::remove_var(ANTHROPIC_BASE_URL_ENV);
        let provider = AnthropicProvider::new("test_key".to_string());
        assert_eq!(provider.endpoint(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn test_anthropic_endpoint_honors_env_override() {
        let _guard = env_guard();
        std::env::set_var(ANTHROPIC_BASE_URL_ENV, "http://127.0.0.1:4010");
        let provider = AnthropicProvider::new("test_key".to_string());
        let url = provider.endpoint();
        std::env::remove_var(ANTHROPIC_BASE_URL_ENV);
        assert_eq!(url, "http://127.0.0.1:4010/v1/messages");
    }

    #[test]
    fn test_openai_endpoint_defaults_to_canonical_url() {
        let _guard = env_guard();
        std::env::remove_var(OPENAI_BASE_URL_ENV);
        let provider = OpenAiProvider::new("test_key".to_string());
        assert_eq!(
            provider.endpoint(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_openai_endpoint_honors_env_override() {
        let _guard = env_guard();
        std::env::set_var(OPENAI_BASE_URL_ENV, "http://127.0.0.1:4011");
        let provider = OpenAiProvider::new("test_key".to_string());
        let url = provider.endpoint();
        std::env::remove_var(OPENAI_BASE_URL_ENV);
        assert_eq!(url, "http://127.0.0.1:4011/v1/chat/completions");
    }

    #[test]
    fn test_minimax_endpoint_defaults_to_canonical_url() {
        let _guard = env_guard();
        std::env::remove_var(MINIMAX_BASE_URL_ENV);
        let provider = MinimaxProvider::new("test_key".to_string());
        assert_eq!(
            provider.endpoint(),
            "https://api.minimax.chat/v1/text/chatcompletion_v2"
        );
    }

    #[test]
    fn test_minimax_endpoint_honors_env_override() {
        let _guard = env_guard();
        std::env::set_var(MINIMAX_BASE_URL_ENV, "http://127.0.0.1:4012");
        let provider = MinimaxProvider::new("test_key".to_string());
        let url = provider.endpoint();
        std::env::remove_var(MINIMAX_BASE_URL_ENV);
        assert_eq!(url, "http://127.0.0.1:4012/v1/text/chatcompletion_v2");
    }

    #[test]
    fn test_format_anthropic_messages_plain_text_uses_string_content() {
        let messages = vec![Message::text(Role::User, "hello")];
        let formatted = format_anthropic_messages(&messages);
        assert_eq!(formatted[0]["role"], "user");
        assert_eq!(formatted[0]["content"], "hello");
    }

    #[test]
    fn test_format_anthropic_messages_tool_use_renders_block_array() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "ion_verify".into(),
            input: serde_json::json!({"diff_ref": "HEAD"}),
        };
        let messages = vec![Message::assistant_tool_use("", vec![call])];
        let formatted = format_anthropic_messages(&messages);
        assert_eq!(formatted[0]["role"], "assistant");
        let blocks = formatted[0]["content"].as_array().expect("block array");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_use");
        assert_eq!(blocks[0]["id"], "call_1");
        assert_eq!(blocks[0]["name"], "ion_verify");
    }

    #[test]
    fn test_format_anthropic_messages_tool_result_renders_block_array() {
        let result = crate::llm_backends::ToolResult {
            tool_use_id: "call_1".into(),
            content: "Approve".into(),
            is_error: false,
        };
        let messages = vec![Message::tool_results(vec![result])];
        let formatted = format_anthropic_messages(&messages);
        assert_eq!(formatted[0]["role"], "user");
        let blocks = formatted[0]["content"].as_array().expect("block array");
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "call_1");
        assert_eq!(blocks[0]["content"], "Approve");
        assert_eq!(blocks[0]["is_error"], false);
    }
}
