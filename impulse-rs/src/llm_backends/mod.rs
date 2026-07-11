//! LLM provider abstraction (Anthropic, OpenAI, Minimax).
//!
//! Defines the [`LlmProvider`] trait and chat interface types ([`Message`],
//! [`ChatRequest`], [`ChatResponse`]). Provider implementations live in
//! [`anthropic`]. Phase 2 API surface — not yet wired to production paths.
//!
//! **Tool-calling (TUI_SPEC.md T9):** [`ToolDefinition`] carries an
//! Anthropic tool-use schema (`{name, description, input_schema}`, matching
//! `ion_repl::tools::ReplTool::json_schema`); [`ToolCall`]/[`ToolResult`]
//! carry one round trip's `tool_use`/`tool_result` content blocks;
//! [`StopReason`] distinguishes a plain-text reply from a tool-use request.
//! [`Agent::chat_with_tools`] drives the request/execute/tool_result loop
//! against an abstract [`ToolExecutor`] so this module never depends on
//! `ion_repl` or `src/tooling` types — the ion REPL supplies the executor
//! (see `ion_repl::chat`).

pub use crate::error::AgentResult;
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};

use crate::error::AgentError;

// Re-export all providers from consolidated anthropic.rs
pub mod anthropic;
pub use anthropic::{AnthropicProvider, MinimaxProvider, OpenAiProvider};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// `tool_use` blocks this (assistant) message requested. Empty for
    /// plain text messages -- see [`Message::text`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// `tool_result` blocks this (user) message is reporting back to the
    /// model. Empty for plain text messages -- see [`Message::text`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
}

impl Message {
    /// A plain text message -- the common case, used everywhere before T9.
    pub fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        }
    }

    /// An assistant message that requested one or more tool calls
    /// (`stop_reason: ToolUse`). `content` carries any text the model
    /// emitted alongside the tool-use blocks (often empty).
    pub fn assistant_tool_use(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls,
            tool_results: Vec::new(),
        }
    }

    /// A user message carrying `tool_result` blocks for one or more prior
    /// `tool_use` calls, sent back to the model to continue the turn.
    pub fn tool_results(tool_results: Vec<ToolResult>) -> Self {
        Self {
            role: Role::User,
            content: String::new(),
            tool_calls: Vec::new(),
            tool_results,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One tool the model may call, in Anthropic's tool-use schema shape
/// (`{name, description, input_schema}`). Provider-agnostic: non-Anthropic
/// providers may ignore `ChatRequest::tools` until they add support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// One `tool_use` content block: the model asking the caller to run
/// `name(input)` and report back via a matching [`ToolResult`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// One `tool_result` content block: the caller reporting the outcome of a
/// prior [`ToolCall`] back to the model, matched by `tool_use_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

/// Why the provider stopped generating. `ToolUse` is the only variant that
/// should ever coincide with a non-empty `ChatResponse::tool_calls`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Tools the model may call this turn. Empty means no tool-use
    /// (existing pre-T9 behavior — omitted from the wire request entirely).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
    #[serde(default)]
    pub stop_reason: StopReason,
    /// `tool_use` blocks the model emitted this turn. Non-empty only when
    /// `stop_reason == ToolUse`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse>;
    fn supported_models(&self) -> Vec<&str>;
}

/// Executes one named tool with JSON input and reports back text content
/// for a `tool_result` block. Abstracts over the concrete tool registry so
/// [`Agent::chat_with_tools`] (a generic LLM-backend concern) never depends
/// on `ion_repl`/`src/tooling` types -- the ion REPL supplies an adapter
/// (`ion_repl::chat::ReplToolExecutor`) that dispatches through its own
/// `ReplToolRegistry`.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, input: serde_json::Value) -> ToolExecutionResult;
}

/// Outcome of one [`ToolExecutor::execute`] call, ready to fold into a
/// [`ToolResult`] (the caller supplies `tool_use_id`).
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub content: String,
    pub is_error: bool,
}

/// Default cap on tool-use round trips within one [`Agent::chat_with_tools`]
/// call (TUI_SPEC.md T9) -- bounds a misbehaving model that keeps
/// requesting tools instead of ever returning a plain-text stop reason.
pub const DEFAULT_MAX_TOOL_ROUNDS: usize = 10;

pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: Box<dyn LlmProvider>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub history: Vec<Message>,
}

impl Agent {
    pub fn new(
        id: String,
        name: String,
        provider: Box<dyn LlmProvider>,
        model: Option<String>,
        system_prompt: Option<String>,
    ) -> Self {
        let model = model.unwrap_or_else(|| provider.default_model().to_string());
        Self {
            id,
            name,
            provider,
            model,
            system_prompt,
            history: Vec::new(),
        }
    }

    pub async fn chat(&mut self, user_message: &str) -> AgentResult<String> {
        let mut messages = Vec::new();
        if let Some(ref system) = self.system_prompt {
            messages.push(Message::text(Role::System, system.clone()));
        }
        messages.extend(self.history.clone());
        messages.push(Message::text(Role::User, user_message));

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: 0.7,
            max_tokens: Some(4096),
            tools: Vec::new(),
        };
        let response = self.provider.chat(request).await?;

        self.history.push(Message::text(Role::User, user_message));
        self.history
            .push(Message::text(Role::Assistant, response.content.clone()));

        Ok(response.content)
    }

    /// Sends one user turn with `tools` available for the model to call
    /// (TUI_SPEC.md T9), looping on `tool_use` stop reasons -- executing
    /// each requested call via `executor` and sending the results back --
    /// until the model returns a plain-text reply or [`DEFAULT_MAX_TOOL_ROUNDS`]
    /// is hit. Conversation history is only committed on a successful
    /// (non-error) return, matching [`Agent::chat`]'s error-path behavior.
    pub async fn chat_with_tools(
        &mut self,
        user_message: &str,
        tools: &[ToolDefinition],
        executor: &dyn ToolExecutor,
    ) -> AgentResult<String> {
        self.chat_with_tools_capped(user_message, tools, executor, DEFAULT_MAX_TOOL_ROUNDS)
            .await
    }

    /// Same as [`Agent::chat_with_tools`] with an explicit round cap --
    /// split out so tests can exercise the cap-hit error path without
    /// looping [`DEFAULT_MAX_TOOL_ROUNDS`] times.
    pub async fn chat_with_tools_capped(
        &mut self,
        user_message: &str,
        tools: &[ToolDefinition],
        executor: &dyn ToolExecutor,
        max_rounds: usize,
    ) -> AgentResult<String> {
        let mut working = self.history.clone();
        working.push(Message::text(Role::User, user_message));

        for _round in 0..max_rounds {
            let mut messages = Vec::new();
            if let Some(ref system) = self.system_prompt {
                messages.push(Message::text(Role::System, system.clone()));
            }
            messages.extend(working.clone());

            let request = ChatRequest {
                model: self.model.clone(),
                messages,
                temperature: 0.7,
                max_tokens: Some(4096),
                tools: tools.to_vec(),
            };
            let response = self.provider.chat(request).await?;

            if response.stop_reason == StopReason::ToolUse && !response.tool_calls.is_empty() {
                working.push(Message::assistant_tool_use(
                    response.content.clone(),
                    response.tool_calls.clone(),
                ));

                let mut results = Vec::with_capacity(response.tool_calls.len());
                for call in &response.tool_calls {
                    let outcome = executor.execute(&call.name, call.input.clone()).await;
                    results.push(ToolResult {
                        tool_use_id: call.id.clone(),
                        content: outcome.content,
                        is_error: outcome.is_error,
                    });
                }
                working.push(Message::tool_results(results));
                continue;
            }

            working.push(Message::text(Role::Assistant, response.content.clone()));
            self.history = working;
            return Ok(response.content);
        }

        Err(AgentError::ToolLoopLimitExceeded { rounds: max_rounds })
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedReplyProvider {
        content: &'static str,
    }

    #[async_trait]
    impl LlmProvider for FixedReplyProvider {
        fn name(&self) -> &str {
            "fixed-fake"
        }
        fn default_model(&self) -> &str {
            "fixed-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            Ok(ChatResponse {
                content: self.content.to_string(),
                model: request.model,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
                tool_calls: Vec::new(),
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["fixed-fake-model"]
        }
    }

    /// Always asks the model to call `echo_tool` on round 1, then returns a
    /// fixed final reply on every subsequent round. Call count is tracked
    /// with a sync `Mutex` since `LlmProvider::chat` takes `&self`.
    struct OneShotToolProvider {
        calls: std::sync::Mutex<usize>,
    }

    impl OneShotToolProvider {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for OneShotToolProvider {
        fn name(&self) -> &str {
            "one-shot-tool-fake"
        }
        fn default_model(&self) -> &str {
            "one-shot-tool-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let mut calls = self.calls.lock().expect("lock is never poisoned in tests");
            *calls += 1;
            if *calls == 1 {
                Ok(ChatResponse {
                    content: String::new(),
                    model: request.model,
                    usage: Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    stop_reason: StopReason::ToolUse,
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: serde_json::json!({"msg": "hi"}),
                    }],
                })
            } else {
                Ok(ChatResponse {
                    content: "final answer".to_string(),
                    model: request.model,
                    usage: Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    stop_reason: StopReason::EndTurn,
                    tool_calls: Vec::new(),
                })
            }
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["one-shot-tool-fake-model"]
        }
    }

    /// Always asks the model to call `echo_tool`, every round -- used to
    /// prove the round cap actually fires instead of looping forever.
    struct AlwaysToolUseProvider;

    #[async_trait]
    impl LlmProvider for AlwaysToolUseProvider {
        fn name(&self) -> &str {
            "always-tool-fake"
        }
        fn default_model(&self) -> &str {
            "always-tool-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            Ok(ChatResponse {
                content: String::new(),
                model: request.model,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::ToolUse,
                tool_calls: vec![ToolCall {
                    id: "call".to_string(),
                    name: "echo_tool".to_string(),
                    input: serde_json::Value::Null,
                }],
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["always-tool-fake-model"]
        }
    }

    struct EchoExecutor {
        invocations: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl EchoExecutor {
        fn new() -> Self {
            Self {
                invocations: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolExecutor for EchoExecutor {
        async fn execute(&self, name: &str, input: serde_json::Value) -> ToolExecutionResult {
            self.invocations
                .lock()
                .expect("lock is never poisoned in tests")
                .push((name.to_string(), input.clone()));
            ToolExecutionResult {
                content: format!("echoed:{input}"),
                is_error: false,
            }
        }
    }

    fn test_agent(provider: impl LlmProvider + 'static) -> Agent {
        Agent::new(
            "test-agent".to_string(),
            "test".to_string(),
            Box::new(provider),
            Some("test-model".to_string()),
            Some("system prompt".to_string()),
        )
    }

    #[test]
    fn test_role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn test_message_construction() {
        let msg = Message::text(Role::User, "hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
        assert!(msg.tool_calls.is_empty());
        assert!(msg.tool_results.is_empty());
    }

    #[test]
    fn test_message_assistant_tool_use_constructor() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "echo".into(),
            input: serde_json::json!({"x": 1}),
        };
        let msg = Message::assistant_tool_use("thinking...", vec![call.clone()]);
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "thinking...");
        assert_eq!(msg.tool_calls, vec![call]);
        assert!(msg.tool_results.is_empty());
    }

    #[test]
    fn test_message_tool_results_constructor() {
        let result = ToolResult {
            tool_use_id: "call_1".into(),
            content: "ok".into(),
            is_error: false,
        };
        let msg = Message::tool_results(vec![result.clone()]);
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "");
        assert!(msg.tool_calls.is_empty());
        assert_eq!(msg.tool_results, vec![result]);
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest {
            model: "claude-3".into(),
            messages: vec![Message::text(Role::User, "hi")],
            temperature: 0.7,
            max_tokens: Some(4096),
            tools: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "claude-3");
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.max_tokens, Some(4096));
        assert!(parsed.tools.is_empty());
    }

    #[test]
    fn test_chat_response_serialization() {
        let resp = ChatResponse {
            content: "Hello!".into(),
            model: "claude-3".into(),
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
            stop_reason: StopReason::EndTurn,
            tool_calls: Vec::new(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: ChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "Hello!");
        assert_eq!(parsed.usage.input_tokens, 10);
        assert_eq!(parsed.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_role_rename_all_lowercase() {
        let json = serde_json::to_string(&Role::System).unwrap();
        assert_eq!(json, "\"system\"");
        let json = serde_json::to_string(&Role::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
    }

    #[test]
    fn round_trip_tool_definition() {
        let original = ToolDefinition {
            name: "ion_verify".into(),
            description: "runs the gate".into(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_tool_call() {
        let original = ToolCall {
            id: "call_1".into(),
            name: "bash_exec".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_tool_result() {
        let original = ToolResult {
            tool_use_id: "call_1".into(),
            content: "done".into(),
            is_error: true,
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_stop_reason() {
        for reason in [
            StopReason::EndTurn,
            StopReason::ToolUse,
            StopReason::MaxTokens,
            StopReason::Other,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let recovered: StopReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, recovered);
        }
    }

    #[test]
    fn round_trip_message_with_tool_calls() {
        let original = Message::assistant_tool_use(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: "echo".into(),
                input: serde_json::Value::Null,
            }],
        );
        let json = serde_json::to_string(&original).unwrap();
        let recovered: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.tool_calls, original.tool_calls);
        assert_eq!(recovered.role, original.role);
    }

    #[test]
    fn round_trip_chat_request_with_tools() {
        let original = ChatRequest {
            model: "claude-3".into(),
            messages: Vec::new(),
            temperature: 0.5,
            max_tokens: None,
            tools: vec![ToolDefinition {
                name: "ion_verify".into(),
                description: "d".into(),
                input_schema: serde_json::json!({}),
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.tools.len(), 1);
        assert_eq!(recovered.tools[0].name, "ion_verify");
    }

    #[tokio::test]
    async fn test_chat_with_tools_executes_tool_and_returns_final_reply() {
        let mut agent = test_agent(OneShotToolProvider::new());
        let executor = EchoExecutor::new();

        let reply = agent
            .chat_with_tools("do the thing", &[], &executor)
            .await
            .expect("tool loop should resolve to a final reply");

        assert_eq!(reply, "final answer");
        let invocations = executor.invocations.lock().unwrap();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].0, "echo_tool");

        // History committed: user, assistant(tool_use), user(tool_results), assistant(final).
        assert_eq!(agent.history.len(), 4);
        assert!(!agent.history[1].tool_calls.is_empty());
        assert!(!agent.history[2].tool_results.is_empty());
        assert_eq!(agent.history[3].content, "final answer");
    }

    #[tokio::test]
    async fn test_chat_with_tools_no_tool_use_behaves_like_plain_chat() {
        let mut agent = test_agent(FixedReplyProvider {
            content: "hi there",
        });
        let executor = EchoExecutor::new();

        let reply = agent
            .chat_with_tools("hello", &[], &executor)
            .await
            .expect("no tool_use means an immediate reply");

        assert_eq!(reply, "hi there");
        assert_eq!(agent.history.len(), 2);
        assert!(executor.invocations.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_chat_with_tools_capped_returns_error_when_cap_hit() {
        let mut agent = test_agent(AlwaysToolUseProvider);
        let executor = EchoExecutor::new();

        let result = agent
            .chat_with_tools_capped("do the thing", &[], &executor, 2)
            .await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopLimitExceeded { rounds: 2 })
        ));
        // History must be left untouched on the error path, matching
        // Agent::chat's existing error-path behavior.
        assert!(agent.history.is_empty());
    }
}
