use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use std::sync::Arc;

use super::{
    ChatRequest, ChatResponse, LlmProvider, Message, Role, StopReason, ToolCall, ToolDefinition,
    Usage,
};
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

/// MiniMax's `chatcompletion_v2` endpoint expects `max_tokens` on every
/// request, so a request that does not set one still sends this fallback.
const MINIMAX_DEFAULT_MAX_TOKENS: u32 = 4096;

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

/// The wire shape one provider expects a chat request to take.
///
/// Every provider in this module renders messages through
/// [`format_messages_for`] with its own variant. There is deliberately no
/// shared "plain text only" formatter any more: the previous
/// `BaseProvider::format_messages` silently dropped `tool_calls` and
/// `tool_results` (it only ever read `Message::content`), so a tool-use turn
/// sent through a non-Anthropic provider reached the model as an empty
/// assistant message followed by an empty user message — an unpaired,
/// meaningless exchange that the provider had no way to reject usefully.
/// Making the format an explicit parameter means a new provider has to pick
/// a wire shape that carries tool blocks, rather than inheriting one that
/// discards them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// Anthropic Messages API: `content` becomes a block array carrying
    /// `tool_use` / `tool_result` blocks, and the system prompt is lifted to
    /// a top-level request field.
    Anthropic,
    /// OpenAI chat-completions: the assistant message carries a `tool_calls`
    /// array of `{id, type: "function", function: {name, arguments}}`, and
    /// each result comes back as its own `{role: "tool", tool_call_id, ...}`
    /// message. MiniMax's `chatcompletion_v2` endpoint speaks the same shape
    /// (see [`MinimaxProvider`]).
    OpenAi,
}

impl WireFormat {
    /// Every wire shape, so a measurement that must not under-count can take
    /// the widest. Adding a variant automatically joins this list.
    pub const ALL: [WireFormat; 2] = [WireFormat::Anthropic, WireFormat::OpenAi];

    /// Characters one tool call's `input` occupies in this wire shape.
    ///
    /// The two shapes differ by more than formatting. Anthropic sends the
    /// input as a JSON **object**, so it costs its canonical serialization.
    /// OpenAI sends it as `function.arguments`, a JSON **string** *holding*
    /// that serialization — so every quote and backslash inside is escaped a
    /// second time, and an escape-heavy input costs materially more on the
    /// OpenAI wire than on Anthropic's.
    ///
    /// Canonical JSON is used as the inner form so the measurement never
    /// drifts with map ordering.
    pub fn tool_input_chars(self, input: &serde_json::Value) -> usize {
        let canonical = crate::loop_contract::canonical_json(input);
        match self {
            WireFormat::Anthropic => canonical.chars().count(),
            WireFormat::OpenAi => serde_json::Value::String(canonical)
                .to_string()
                .chars()
                .count(),
        }
    }

    /// The largest [`WireFormat::tool_input_chars`] across every wire shape.
    ///
    /// The context budget measures with this rather than with the running
    /// provider's own shape (review round 1): reading the shape off the
    /// provider would mean a `LlmProvider` trait method, and a trait method
    /// with a default is exactly how the under-measurement it fixes would
    /// come back — a future provider that forgets to override it silently
    /// measures its own traffic short. Taking the widest can never
    /// under-count for any provider, and over-counting only spends the
    /// budget's deliberate headroom slightly sooner.
    pub fn widest_tool_input_chars(input: &serde_json::Value) -> usize {
        WireFormat::ALL
            .into_iter()
            .map(|format| format.tool_input_chars(input))
            .max()
            .unwrap_or(0)
    }
}

/// Renders `messages` in the wire shape `format` expects, preserving every
/// tool block. One [`Message`] may expand into more than one wire message
/// (OpenAI wants one `role: "tool"` message per result), which is why this
/// returns a fresh `Vec` rather than mapping one-to-one.
pub fn format_messages_for(format: WireFormat, messages: &[Message]) -> Vec<serde_json::Value> {
    match format {
        WireFormat::Anthropic => format_anthropic_messages(messages),
        WireFormat::OpenAi => format_openai_messages(messages),
    }
}

fn openai_role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// Renders messages for OpenAI-style chat completions (OpenAI and MiniMax).
///
/// Three shapes:
/// - a message carrying `tool_results` becomes one `{"role": "tool",
///   "tool_call_id", "content"}` message per result, because OpenAI pairs a
///   result to its call by id and accepts exactly one id per message;
/// - a message carrying `tool_calls` becomes an assistant message with a
///   `tool_calls` array, each call's `input` re-encoded as the JSON *string*
///   the API expects in `function.arguments`;
/// - anything else is the plain `{"role", "content"}` pair.
///
/// [`super::ToolResult::is_error`] has no OpenAI counterpart on the wire, so the
/// error text is sent as the tool message's content verbatim rather than
/// wrapped in a marker this module would have invented. The loop breaker
/// still sees the flag (it reads the executor's result, not the wire form),
/// so same-error detection is unaffected.
fn format_openai_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut wire = Vec::with_capacity(messages.len());
    for message in messages {
        if !message.tool_results.is_empty() {
            for result in &message.tool_results {
                wire.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": result.tool_use_id,
                    "content": result.content,
                }));
            }
            // A tool-result message normally carries no prose, but if it
            // does, it must not be dropped along the way.
            if !message.content.is_empty() {
                wire.push(serde_json::json!({
                    "role": openai_role(message.role),
                    "content": message.content,
                }));
            }
            continue;
        }

        if !message.tool_calls.is_empty() {
            let calls: Vec<serde_json::Value> = message
                .tool_calls
                .iter()
                .map(|call| {
                    serde_json::json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.input.to_string(),
                        },
                    })
                })
                .collect();
            let mut assistant = serde_json::json!({
                "role": "assistant",
                "tool_calls": calls,
            });
            if !message.content.is_empty() {
                assistant["content"] = serde_json::Value::String(message.content.clone());
            }
            wire.push(assistant);
            continue;
        }

        wire.push(serde_json::json!({
            "role": openai_role(message.role),
            "content": message.content,
        }));
    }
    wire
}

/// Renders [`ToolDefinition`]s as OpenAI function tools. The definition's
/// `input_schema` is the JSON Schema both APIs want; only the envelope
/// differs (`{type, function{name, description, parameters}}` here against
/// Anthropic's flat `{name, description, input_schema}`).
fn openai_tools_value(tools: &[ToolDefinition]) -> serde_json::Value {
    serde_json::Value::Array(
        tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    },
                })
            })
            .collect(),
    )
}

/// Renders messages for the Anthropic Messages API. Plain text messages use
/// the simple `{"role", "content": "..."}` shape; messages carrying
/// `tool_calls` or `tool_results` (TUI_SPEC.md T9) render `content` as a
/// block array per Anthropic's tool-use protocol instead. Reached through
/// [`format_messages_for`] with [`WireFormat::Anthropic`]; the OpenAI and
/// MiniMax shape is [`format_openai_messages`].
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

/// Whether a fresh [`AnthropicProvider`] marks its system-and-tools prefix
/// cacheable. On by default: the prefix is identical on every round of a
/// tool loop, so caching it is the single cheapest saving available, and a
/// cache miss costs nothing but the write.
const DEFAULT_CACHE_SYSTEM_AND_TOOLS: bool = true;

pub struct AnthropicProvider {
    base: BaseProvider,
    cache_system_and_tools: bool,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            base: BaseProvider::new("anthropic", api_key, "claude-sonnet-4-6"),
            cache_system_and_tools: DEFAULT_CACHE_SYSTEM_AND_TOOLS,
        }
    }

    /// Turns the ephemeral `cache_control` breakpoint on the
    /// system-and-tools prefix on or off. Defaults to on
    /// ([`DEFAULT_CACHE_SYSTEM_AND_TOOLS`]).
    pub fn with_prompt_cache(mut self, enabled: bool) -> Self {
        self.cache_system_and_tools = enabled;
        self
    }

    /// Whether this provider marks its system-and-tools prefix cacheable.
    pub fn prompt_cache_enabled(&self) -> bool {
        self.cache_system_and_tools
    }

    fn endpoint(&self) -> String {
        self.base.endpoint(
            ANTHROPIC_BASE_URL_ENV,
            ANTHROPIC_DEFAULT_BASE_URL,
            "/v1/messages",
        )
    }
}

/// The ephemeral cache breakpoint Anthropic reads on the last element of the
/// cacheable prefix.
fn ephemeral_cache_control() -> serde_json::Value {
    serde_json::json!({"type": "ephemeral"})
}

/// Builds the Anthropic Messages API request body for `request`.
///
/// Extracted from [`AnthropicProvider::chat`] so the exact wire JSON — in
/// particular where the single `cache_control` breakpoint lands — is
/// assertable without a network call.
///
/// **Prompt caching.** Anthropic's cacheable prefix is ordered `tools`, then
/// `system`, then `messages`, and a breakpoint caches everything up to and
/// including the element it sits on. So exactly one breakpoint, placed at the
/// *end* of the system-and-tools prefix, caches the whole prefix:
///
/// - with a system prompt, `system` is rendered as a one-element block array
///   and the breakpoint goes on that block, covering the tools before it;
/// - with tools but no system prompt, the breakpoint goes on the last tool;
/// - with neither, there is no prefix to cache and no breakpoint is emitted.
///
/// The breakpoint is never placed inside `messages`: the conversation grows
/// (and, under a context budget, gets compacted) every round, so caching it
/// would write a new entry per round for a prefix that rarely repeats.
fn build_anthropic_body(
    request: &ChatRequest,
    cache_system_and_tools: bool,
) -> AgentResult<serde_json::Value> {
    let (system_msgs, non_system): (Vec<&Message>, Vec<&Message>) = request
        .messages
        .iter()
        .partition(|m| m.role == Role::System);
    let non_system: Vec<Message> = non_system.into_iter().cloned().collect();
    let system = system_msgs.first().map(|m| m.content.clone());

    let mut body = serde_json::json!({
        "model": request.model,
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "temperature": request.temperature,
        "messages": format_messages_for(WireFormat::Anthropic, &non_system),
    });

    let mut tools = if request.tools.is_empty() {
        None
    } else {
        Some(
            serde_json::to_value(&request.tools)
                .map_err(|e| AgentError::InvalidRequest(format!("failed to encode tools: {e}")))?,
        )
    };

    match (system, cache_system_and_tools) {
        (Some(sys), true) => {
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": sys,
                "cache_control": ephemeral_cache_control(),
            }]);
        }
        (Some(sys), false) => {
            body["system"] = serde_json::Value::String(sys);
        }
        (None, true) => {
            // No system prompt: the prefix ends at the last tool.
            if let Some(serde_json::Value::Array(entries)) = tools.as_mut() {
                if let Some(serde_json::Value::Object(last)) = entries.last_mut() {
                    last.insert("cache_control".to_string(), ephemeral_cache_control());
                }
            }
        }
        (None, false) => {}
    }

    if let Some(tools) = tools {
        body["tools"] = tools;
    }

    Ok(body)
}

/// Message used when a provider refuses without saying anything.
const UNEXPLAINED_REFUSAL: &str = "the provider refused the request";

/// Converts an Anthropic Messages API response into the provider-neutral
/// [`ChatResponse`].
///
/// Extracted from [`AnthropicProvider::chat`] so the mapping — in particular
/// the refusal arm — is assertable without a network call, mirroring
/// [`openai_style_chat_response`].
///
/// **Refusals are errors, not empty replies (review round 3 follow-up).**
/// Anthropic reports a declined completion as `stop_reason: "refusal"`, which
/// previously fell into the catch-all `StopReason::Other` arm and was returned
/// as an ordinary reply — typically with no text at all, so the loop committed
/// an empty assistant message and reported success. This is the same defect
/// fixed on the OpenAI path in review round 3, and it closes here on the same
/// terms: [`AgentError::ProviderRefusal`], history untouched. `StopReason::
/// Other` now means only what its name says — a stop reason this code does not
/// recognize — and such a response still completes normally.
fn anthropic_chat_response(resp: AnthropicResponse, provider: &str) -> AgentResult<ChatResponse> {
    let content = resp
        .content
        .iter()
        .filter(|c| c.block_type == "text")
        .filter_map(|c| c.text.clone())
        .collect::<Vec<_>>()
        .join("");

    if resp.stop_reason.as_deref() == Some("refusal") {
        let message = if content.trim().is_empty() {
            UNEXPLAINED_REFUSAL.to_string()
        } else {
            content
        };
        return Err(AgentError::ProviderRefusal {
            provider: provider.to_string(),
            message,
        });
    }

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

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> &str {
        &self.base.default_model
    }

    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
        self.base.check_api_key()?;

        let body = build_anthropic_body(&request, self.cache_system_and_tools)?;

        let response = bounded_request(self.base.http_client().post(self.endpoint()))
            .header("x-api-key", self.base.api_key())
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

        anthropic_chat_response(resp, self.name())
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

/// One OpenAI-style chat-completions response.
///
/// Shared by [`OpenAiProvider`] and [`MinimaxProvider`]: MiniMax's
/// `chatcompletion_v2` endpoint returns the same envelope, minus a `model`
/// field, which is why `model` is optional here and each provider supplies
/// its own fallback.
#[derive(Debug, Deserialize)]
struct OpenAiStyleResponse {
    #[serde(default)]
    choices: Vec<OpenAiStyleChoice>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: OpenAiStyleUsage,
}

#[derive(Debug, Deserialize)]
struct OpenAiStyleChoice {
    message: OpenAiStyleMessage,
    /// `"stop"`, `"tool_calls"`, `"length"`, ... Absent on some MiniMax
    /// responses, hence `Option`.
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStyleMessage {
    /// Null on a pure tool-call turn, and on a refusal, hence `Option`.
    #[serde(default)]
    content: Option<String>,
    /// The model's own refusal text. Present (with `content` null) when the
    /// model declines to answer; see [`AgentError::ProviderRefusal`].
    #[serde(default)]
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OpenAiStyleToolCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStyleToolCall {
    #[serde(default)]
    id: String,
    function: OpenAiStyleFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiStyleFunction {
    #[serde(default)]
    name: String,
    /// A JSON *string* holding the call's arguments object, not an object.
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiStyleUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// Builds the request body for an OpenAI-style chat-completions endpoint.
///
/// `default_max_tokens` preserves the two providers' existing difference:
/// OpenAI passes `None` and omits `max_tokens` unless the request set one,
/// while MiniMax passes a fallback because its endpoint expects the field.
///
/// Extracted from the providers so the wire JSON — the `tools` envelope and
/// the `role: "tool"` messages in particular — is assertable without a
/// network call.
fn build_openai_style_body(
    request: &ChatRequest,
    default_max_tokens: Option<u32>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": format_messages_for(WireFormat::OpenAi, &request.messages),
        "temperature": request.temperature,
    });
    if let Some(max_tokens) = request.max_tokens.or(default_max_tokens) {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if !request.tools.is_empty() {
        body["tools"] = openai_tools_value(&request.tools);
    }
    body
}

/// Converts an OpenAI-style response into the provider-neutral
/// [`ChatResponse`], including any `tool_calls` the model requested.
///
/// `fallback_model` is used when the response omits `model` (MiniMax).
/// A `function.arguments` payload that is not valid JSON is a malformed
/// response, not an empty call: it fails with [`AgentError::ApiResponse`]
/// rather than being silently turned into an empty input object that the
/// tool would then execute with.
///
/// **Refusals are errors, not empty replies (review round 3).** A model that
/// declines to answer sends `message.content: null` with the explanation in
/// `message.refusal`; a filtered completion reports
/// `finish_reason: "content_filter"`. Both used to fall through
/// `unwrap_or_default()` into an empty-string reply that the loop committed as
/// a successful turn, so the user saw a blank answer and nothing said the model
/// had refused. Both now return [`AgentError::ProviderRefusal`], leaving
/// history untouched like every other error path.
fn openai_style_chat_response(
    resp: OpenAiStyleResponse,
    provider: &str,
    fallback_model: &str,
) -> AgentResult<ChatResponse> {
    let choice = resp.choices.first();
    let content = choice
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();

    // A structured refusal is unambiguous: the model said why it will not
    // answer, and that text is the outcome.
    if let Some(refusal) = choice
        .and_then(|c| c.message.refusal.as_deref())
        .map(str::trim)
        .filter(|refusal| !refusal.is_empty())
    {
        return Err(AgentError::ProviderRefusal {
            provider: provider.to_string(),
            message: refusal.to_string(),
        });
    }

    // A filtered completion is also a refusal, whether or not any partial text
    // survived it: returning that text as an ordinary reply would present a
    // blocked completion as a finished answer.
    if choice.and_then(|c| c.finish_reason.as_deref()) == Some("content_filter") {
        let message = if content.trim().is_empty() {
            "the provider filtered this completion".to_string()
        } else {
            content
        };
        debug_assert!(!message.is_empty(), "a refusal always carries a reason");
        return Err(AgentError::ProviderRefusal {
            provider: provider.to_string(),
            message,
        });
    }

    let mut tool_calls = Vec::new();
    if let Some(choice) = choice {
        for call in &choice.message.tool_calls {
            let input = if call.function.arguments.trim().is_empty() {
                serde_json::Value::Object(serde_json::Map::new())
            } else {
                serde_json::from_str(&call.function.arguments).map_err(|e| {
                    AgentError::ApiResponse(format!(
                        "tool call '{}' carried arguments that are not valid JSON: {e}",
                        call.function.name
                    ))
                })?
            };
            tool_calls.push(ToolCall {
                id: call.id.clone(),
                name: call.function.name.clone(),
                input,
            });
        }
    }

    // A response that carries tool calls is a tool-use turn even when the
    // provider omitted `finish_reason` — the calls themselves are the signal
    // the loop acts on, and `StopReason::ToolUse` is what pairs with them.
    let stop_reason = match choice.and_then(|c| c.finish_reason.as_deref()) {
        Some("tool_calls") | Some("function_call") => StopReason::ToolUse,
        Some("length") | Some("max_tokens") => StopReason::MaxTokens,
        Some("stop") => StopReason::EndTurn,
        None if !tool_calls.is_empty() => StopReason::ToolUse,
        None => StopReason::EndTurn,
        Some(_) if !tool_calls.is_empty() => StopReason::ToolUse,
        Some(_) => StopReason::Other,
    };

    Ok(ChatResponse {
        content,
        model: resp.model.unwrap_or_else(|| fallback_model.to_string()),
        usage: Usage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
        },
        stop_reason,
        tool_calls,
    })
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

        let body = build_openai_style_body(&request, None);

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

        let resp: OpenAiStyleResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ApiResponse(e.to_string()))?;

        openai_style_chat_response(resp, self.name(), &request.model)
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

// MiniMax's `chatcompletion_v2` endpoint is OpenAI-shaped on both the
// request and the response side, so it reuses `build_openai_style_body`,
// `OpenAiStyleResponse`, and `openai_style_chat_response` rather than
// carrying a second near-identical set of structs. The only two differences
// are handled by parameters: MiniMax wants `max_tokens` always present, and
// its response omits `model`.

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

        let body = build_openai_style_body(&request, Some(MINIMAX_DEFAULT_MAX_TOKENS));

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

        let resp: OpenAiStyleResponse = response
            .json()
            .await
            .map_err(|e| AgentError::ApiResponse(e.to_string()))?;

        openai_style_chat_response(resp, self.name(), &request.model)
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
    fn test_format_messages_for_openai_keeps_plain_text_pairs() {
        let messages = vec![
            Message::text(Role::System, "You are helpful"),
            Message::text(Role::User, "Hello"),
        ];

        let formatted = format_messages_for(WireFormat::OpenAi, &messages);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0]["role"], "system");
        assert_eq!(formatted[0]["content"], "You are helpful");
        assert_eq!(formatted[1]["role"], "user");
        assert_eq!(formatted[1]["content"], "Hello");
    }

    // ---------------------------------------------------------------
    // Provider-neutral tool calls (Stage 1b-A)
    // ---------------------------------------------------------------

    use super::super::{ToolDefinition, ToolResult};

    /// One assistant `tool_use` message followed by its matching
    /// `tool_result` -- the pair every provider formatter has to carry
    /// through intact, since a call without its result (or the reverse) is
    /// rejected or misread by both APIs.
    fn tool_use_pair() -> Vec<Message> {
        vec![
            Message::text(Role::User, "read the file"),
            Message::assistant_tool_use(
                "let me look",
                vec![ToolCall {
                    id: "call_abc".to_string(),
                    name: "file_read".to_string(),
                    input: serde_json::json!({"path": "README.md"}),
                }],
            ),
            Message::tool_results(vec![ToolResult {
                tool_use_id: "call_abc".to_string(),
                content: "# Title".to_string(),
                is_error: false,
            }]),
        ]
    }

    fn sample_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "file_read".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            ToolDefinition {
                name: "bash_exec".to_string(),
                description: "Run a command".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ]
    }

    fn sample_request(messages: Vec<Message>, tools: Vec<ToolDefinition>) -> ChatRequest {
        ChatRequest {
            model: "test-model".to_string(),
            messages,
            temperature: 0.7,
            max_tokens: Some(1024),
            tools,
        }
    }

    #[test]
    fn test_anthropic_formatter_keeps_the_tool_use_result_pair() {
        let wire = format_messages_for(WireFormat::Anthropic, &tool_use_pair());
        let rendered = serde_json::to_string(&wire).unwrap();
        assert!(rendered.contains("\"tool_use\""), "got: {rendered}");
        assert!(rendered.contains("\"tool_result\""), "got: {rendered}");
        assert_eq!(rendered.matches("call_abc").count(), 2, "got: {rendered}");
        assert!(rendered.contains("file_read"), "got: {rendered}");
    }

    #[test]
    fn test_openai_formatter_keeps_the_tool_use_result_pair() {
        let wire = format_messages_for(WireFormat::OpenAi, &tool_use_pair());
        let rendered = serde_json::to_string(&wire).unwrap();
        // The call survives as an assistant `tool_calls` entry...
        assert_eq!(wire[1]["role"], "assistant");
        assert_eq!(wire[1]["tool_calls"][0]["id"], "call_abc");
        assert_eq!(wire[1]["tool_calls"][0]["type"], "function");
        assert_eq!(wire[1]["tool_calls"][0]["function"]["name"], "file_read");
        // ...with its input re-encoded as the JSON string OpenAI expects.
        let args = wire[1]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .expect("arguments must be a JSON string");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(args).unwrap(),
            serde_json::json!({"path": "README.md"})
        );
        // ...and the result survives as its own `role: "tool"` message.
        assert_eq!(wire[2]["role"], "tool");
        assert_eq!(wire[2]["tool_call_id"], "call_abc");
        assert_eq!(wire[2]["content"], "# Title");
        assert_eq!(rendered.matches("call_abc").count(), 2, "got: {rendered}");
    }

    #[test]
    fn test_openai_formatter_emits_one_tool_message_per_result() {
        let messages = vec![Message::tool_results(vec![
            ToolResult {
                tool_use_id: "a".to_string(),
                content: "first".to_string(),
                is_error: false,
            },
            ToolResult {
                tool_use_id: "b".to_string(),
                content: "second".to_string(),
                is_error: true,
            },
        ])];
        let wire = format_messages_for(WireFormat::OpenAi, &messages);
        assert_eq!(wire.len(), 2, "one wire message per result");
        assert_eq!(wire[0]["tool_call_id"], "a");
        assert_eq!(wire[1]["tool_call_id"], "b");
        // `is_error` has no OpenAI counterpart: the text is sent verbatim.
        assert_eq!(wire[1]["content"], "second");
    }

    #[test]
    fn test_openai_formatter_keeps_prose_that_rides_with_tool_results() {
        let mut message = Message::tool_results(vec![ToolResult {
            tool_use_id: "a".to_string(),
            content: "done".to_string(),
            is_error: false,
        }]);
        message.content = "also, note this".to_string();
        let wire = format_messages_for(WireFormat::OpenAi, &[message]);
        assert_eq!(wire.len(), 2);
        assert_eq!(wire[1]["content"], "also, note this");
    }

    #[test]
    fn test_openai_body_renders_tools_as_function_schemas() {
        let body = build_openai_style_body(&sample_request(tool_use_pair(), sample_tools()), None);
        let tools = body["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "file_read");
        assert_eq!(tools[0]["function"]["description"], "Read a file");
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn test_openai_body_omits_tools_when_none_are_offered() {
        let body = build_openai_style_body(&sample_request(tool_use_pair(), Vec::new()), None);
        assert!(body.get("tools").is_none(), "got: {body}");
    }

    #[test]
    fn test_openai_body_omits_max_tokens_but_minimax_supplies_a_default() {
        let mut request = sample_request(tool_use_pair(), Vec::new());
        request.max_tokens = None;
        assert!(build_openai_style_body(&request, None)
            .get("max_tokens")
            .is_none());
        assert_eq!(
            build_openai_style_body(&request, Some(MINIMAX_DEFAULT_MAX_TOKENS))["max_tokens"],
            MINIMAX_DEFAULT_MAX_TOKENS
        );
    }

    #[test]
    fn test_openai_style_response_parses_tool_calls_and_stop_reason() {
        let raw = serde_json::json!({
            "model": "gpt-4o",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "file_read", "arguments": "{\"path\":\"a.txt\"}"}
                    }]
                }
            }],
            "usage": {"prompt_tokens": 11, "completion_tokens": 3}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let parsed = openai_style_chat_response(resp, "openai", "fallback-model").unwrap();
        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.content, "");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_1");
        assert_eq!(parsed.tool_calls[0].name, "file_read");
        assert_eq!(
            parsed.tool_calls[0].input,
            serde_json::json!({"path": "a.txt"})
        );
        assert_eq!(parsed.usage.input_tokens, 11);
        assert_eq!(parsed.usage.output_tokens, 3);
    }

    #[test]
    fn test_openai_style_response_falls_back_to_the_request_model() {
        // MiniMax omits `model` from its response envelope.
        let raw = serde_json::json!({
            "choices": [{"finish_reason": "stop", "message": {"content": "hi"}}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let parsed = openai_style_chat_response(resp, "openai", "abab6.5s-chat").unwrap();
        assert_eq!(parsed.model, "abab6.5s-chat");
        assert_eq!(parsed.content, "hi");
        assert_eq!(parsed.stop_reason, StopReason::EndTurn);
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn test_openai_style_response_infers_tool_use_without_a_finish_reason() {
        let raw = serde_json::json!({
            "choices": [{"message": {"tool_calls": [{
                "id": "c", "function": {"name": "t", "arguments": ""}
            }]}}],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let parsed = openai_style_chat_response(resp, "openai", "m").unwrap();
        assert_eq!(parsed.stop_reason, StopReason::ToolUse);
        // Empty arguments mean "no input", not malformed input.
        assert_eq!(parsed.tool_calls[0].input, serde_json::json!({}));
    }

    #[test]
    fn test_openai_style_response_maps_length_to_max_tokens() {
        let raw = serde_json::json!({
            "choices": [{"finish_reason": "length", "message": {"content": "cut"}}],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(
            openai_style_chat_response(resp, "openai", "m")
                .unwrap()
                .stop_reason,
            StopReason::MaxTokens
        );
    }

    #[test]
    fn test_openai_style_response_maps_an_unknown_reason_to_other() {
        // A reason this code does not know is `Other` -- not an error. Only
        // the refusal shapes below are errors.
        let raw = serde_json::json!({
            "choices": [{"finish_reason": "some_future_reason", "message": {"content": "partial"}}],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let parsed = openai_style_chat_response(resp, "openai", "m").unwrap();
        assert_eq!(parsed.stop_reason, StopReason::Other);
        assert_eq!(parsed.content, "partial");
    }

    #[test]
    fn test_openai_style_response_surfaces_a_structured_refusal_as_an_error() {
        // Review round 3: `content` is null and the explanation lives in
        // `refusal`. This used to become an empty-string reply that the loop
        // committed as a successful turn -- a blank answer with nothing
        // saying the model had refused.
        let raw = serde_json::json!({
            "model": "gpt-4o",
            "choices": [{
                "finish_reason": "stop",
                "message": {"content": null, "refusal": "I can't help with that request."}
            }],
            "usage": {"prompt_tokens": 9, "completion_tokens": 0}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let err = openai_style_chat_response(resp, "openai", "m")
            .expect_err("a refusal must not be a successful empty reply");
        match err {
            AgentError::ProviderRefusal {
                ref provider,
                ref message,
            } => {
                assert_eq!(provider, "openai");
                assert_eq!(message, "I can't help with that request.");
            }
            other => panic!("expected ProviderRefusal, got: {other:?}"),
        }
    }

    #[test]
    fn test_openai_style_response_surfaces_a_content_filter_as_an_error() {
        let raw = serde_json::json!({
            "choices": [{"finish_reason": "content_filter", "message": {"content": null}}],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let err = openai_style_chat_response(resp, "minimax", "m")
            .expect_err("a filtered completion must not be a successful empty reply");
        match err {
            AgentError::ProviderRefusal {
                ref provider,
                ref message,
            } => {
                assert_eq!(provider, "minimax");
                assert!(message.contains("filtered"), "got: {message}");
            }
            other => panic!("expected ProviderRefusal, got: {other:?}"),
        }
    }

    #[test]
    fn test_openai_style_response_content_filter_keeps_any_partial_text_in_the_error() {
        // A filtered completion is a refusal whether or not partial text
        // survived it: returning that text as an ordinary reply would present
        // a blocked completion as a finished answer.
        let raw = serde_json::json!({
            "choices": [{
                "finish_reason": "content_filter",
                "message": {"content": "here is how to"}
            }],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let err = openai_style_chat_response(resp, "openai", "m")
            .expect_err("a filtered completion is never a success");
        assert!(format!("{err}").contains("here is how to"), "got: {err}");
    }

    #[test]
    fn test_openai_style_response_ignores_an_empty_refusal_field() {
        // Many ordinary replies carry `refusal: null`, and some carry an
        // empty string; neither is a refusal.
        for refusal in [serde_json::Value::Null, serde_json::json!("   ")] {
            let raw = serde_json::json!({
                "choices": [{
                    "finish_reason": "stop",
                    "message": {"content": "a real answer", "refusal": refusal}
                }],
                "usage": {}
            });
            let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
            let parsed = openai_style_chat_response(resp, "openai", "m")
                .expect("an ordinary reply must not be mistaken for a refusal");
            assert_eq!(parsed.content, "a real answer");
        }
    }

    #[test]
    fn test_openai_style_response_rejects_unparseable_tool_arguments() {
        let raw = serde_json::json!({
            "choices": [{"finish_reason": "tool_calls", "message": {"tool_calls": [{
                "id": "c", "function": {"name": "bash_exec", "arguments": "{not json"}
            }]}}],
            "usage": {}
        });
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let err = openai_style_chat_response(resp, "openai", "m").unwrap_err();
        assert!(
            matches!(err, AgentError::ApiResponse(_)),
            "expected ApiResponse, got: {err:?}"
        );
        assert!(format!("{err}").contains("bash_exec"), "got: {err}");
    }

    #[test]
    fn test_openai_style_response_with_no_choices_is_an_empty_reply() {
        let raw = serde_json::json!({"choices": [], "usage": {}});
        let resp: OpenAiStyleResponse = serde_json::from_value(raw).unwrap();
        let parsed = openai_style_chat_response(resp, "openai", "m").unwrap();
        assert_eq!(parsed.content, "");
        assert!(parsed.tool_calls.is_empty());
    }

    // ---------------------------------------------------------------
    // Anthropic refusals (review round 3 follow-up)
    // ---------------------------------------------------------------

    fn anthropic_response(stop_reason: &str, text: &str) -> AnthropicResponse {
        let content = if text.is_empty() {
            serde_json::json!([])
        } else {
            serde_json::json!([{"type": "text", "text": text}])
        };
        serde_json::from_value(serde_json::json!({
            "id": "msg_1",
            "model": "claude-sonnet-4-6",
            "stop_reason": stop_reason,
            "content": content,
            "usage": {"input_tokens": 3, "output_tokens": 0}
        }))
        .expect("the fixture is a valid Anthropic response")
    }

    #[test]
    fn test_anthropic_refusal_with_text_surfaces_the_models_words() {
        let resp = anthropic_response("refusal", "I won't help with that.");
        let err = anthropic_chat_response(resp, "anthropic")
            .expect_err("a refusal must not be a successful reply");
        match err {
            AgentError::ProviderRefusal {
                ref provider,
                ref message,
            } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(message, "I won't help with that.");
            }
            other => panic!("expected ProviderRefusal, got: {other:?}"),
        }
    }

    #[test]
    fn test_anthropic_refusal_without_text_falls_back_to_a_fixed_reason() {
        // The empty-content case is the one that used to commit an empty
        // assistant message and report success.
        let resp = anthropic_response("refusal", "");
        let err = anthropic_chat_response(resp, "anthropic")
            .expect_err("a refusal must not be a successful empty reply");
        match err {
            AgentError::ProviderRefusal {
                ref provider,
                ref message,
            } => {
                assert_eq!(provider, "anthropic");
                assert_eq!(message, UNEXPLAINED_REFUSAL);
                assert!(!message.is_empty());
            }
            other => panic!("expected ProviderRefusal, got: {other:?}"),
        }
    }

    #[test]
    fn test_anthropic_unknown_stop_reason_with_text_still_completes() {
        // `Other` now means only what its name says. A stop reason this code
        // does not recognize is not a refusal and must still return the
        // model's answer.
        let resp = anthropic_response("some_future_reason", "still an answer");
        let parsed = anthropic_chat_response(resp, "anthropic")
            .expect("an unknown stop reason is not an error");
        assert_eq!(parsed.stop_reason, StopReason::Other);
        assert_eq!(parsed.content, "still an answer");
        assert_eq!(parsed.model, "claude-sonnet-4-6");
    }

    #[test]
    fn test_anthropic_known_stop_reasons_map_as_before() {
        for (raw, expected) in [
            ("end_turn", StopReason::EndTurn),
            ("stop_sequence", StopReason::EndTurn),
            ("max_tokens", StopReason::MaxTokens),
            ("tool_use", StopReason::ToolUse),
        ] {
            let parsed = anthropic_chat_response(anthropic_response(raw, "hi"), "anthropic")
                .unwrap_or_else(|err| panic!("{raw} must not error: {err}"));
            assert_eq!(parsed.stop_reason, expected, "for {raw}");
        }
    }

    // ---------------------------------------------------------------
    // Anthropic prompt-cache breakpoint (Stage 1b-A)
    // ---------------------------------------------------------------

    fn cache_control_count(body: &serde_json::Value) -> usize {
        serde_json::to_string(body)
            .unwrap()
            .matches("cache_control")
            .count()
    }

    #[test]
    fn test_anthropic_body_caches_the_prefix_exactly_once_at_the_system_block() {
        let mut messages = vec![Message::text(Role::System, "You are Ion")];
        messages.extend(tool_use_pair());
        let body = build_anthropic_body(&sample_request(messages, sample_tools()), true).unwrap();

        assert_eq!(
            cache_control_count(&body),
            1,
            "exactly one breakpoint per request: {body}"
        );
        // System is the last element of the tools-then-system prefix, so the
        // breakpoint sits there and covers the tools before it.
        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["system"][0]["text"], "You are Ion");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(
            body["tools"][1].get("cache_control").is_none(),
            "tools must not carry a second breakpoint: {body}"
        );
        // Never inside the conversation, which changes every round.
        assert!(
            !serde_json::to_string(&body["messages"])
                .unwrap()
                .contains("cache_control"),
            "got: {body}"
        );
    }

    #[test]
    fn test_anthropic_body_caches_the_last_tool_when_there_is_no_system_prompt() {
        let body =
            build_anthropic_body(&sample_request(tool_use_pair(), sample_tools()), true).unwrap();
        assert_eq!(cache_control_count(&body), 1, "got: {body}");
        assert!(body.get("system").is_none());
        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["tools"][1]["name"], "bash_exec");
    }

    #[test]
    fn test_anthropic_body_emits_no_breakpoint_without_a_prefix() {
        let body =
            build_anthropic_body(&sample_request(tool_use_pair(), Vec::new()), true).unwrap();
        assert_eq!(cache_control_count(&body), 0, "got: {body}");
    }

    #[test]
    fn test_anthropic_body_omits_the_breakpoint_when_caching_is_disabled() {
        let mut messages = vec![Message::text(Role::System, "You are Ion")];
        messages.extend(tool_use_pair());
        let body = build_anthropic_body(&sample_request(messages, sample_tools()), false).unwrap();
        assert_eq!(cache_control_count(&body), 0, "got: {body}");
        // System falls back to the plain string form.
        assert_eq!(body["system"], "You are Ion");
        assert_eq!(body["tools"][0]["name"], "file_read");
    }

    #[test]
    fn test_anthropic_body_keeps_the_tool_use_pair_and_request_fields() {
        let body =
            build_anthropic_body(&sample_request(tool_use_pair(), Vec::new()), true).unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["max_tokens"], 1024);
        let messages = serde_json::to_string(&body["messages"]).unwrap();
        assert!(messages.contains("tool_use"), "got: {messages}");
        assert!(messages.contains("tool_result"), "got: {messages}");
    }

    #[test]
    fn test_anthropic_provider_enables_the_prompt_cache_by_default() {
        let provider = AnthropicProvider::new("k".to_string());
        assert!(provider.prompt_cache_enabled());
        assert!(!provider.with_prompt_cache(false).prompt_cache_enabled());
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
