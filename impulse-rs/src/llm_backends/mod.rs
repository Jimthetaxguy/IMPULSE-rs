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

use std::time::Duration;

use crate::agent::step_model::{resolve_step_model, HarnessStepContext};
use crate::error::AgentError;
use crate::loop_contract::{
    error_signature, CallOutcome, LoopBreaker, LoopContract, LoopReport, LoopTermination, LoopTrip,
};

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
/// Sourced from the Ion loop contract (ADR-0017) so the constant and
/// [`LoopContract::ion_tool_loop`] can never disagree.
pub const DEFAULT_MAX_TOOL_ROUNDS: usize = crate::loop_contract::ION_DEFAULT_MAX_ROUNDS;

/// Overall wall-clock budget for one [`Agent::chat_with_tools`]/
/// [`Agent::chat_with_tools_capped`] call -- the *entire* multi-round
/// exchange, not any single round (same-day Opus adversarial-review
/// follow-up to TUI_SPEC.md T9, finding S2). [`DEFAULT_MAX_TOOL_ROUNDS`]
/// rounds, each potentially waiting on a 30s `bash_exec` timeout plus
/// network latency for the LLM call itself, could otherwise block the REPL
/// for several minutes with no way to abort (Ctrl-C is only handled around
/// `readline()`, not mid-`.await` inside the tool loop -- full
/// interruptibility is a separate, larger change involving cancellation
/// tokens threaded through the REPL's event loop). This timeout is a
/// narrower, immediately-actionable mitigation: it guarantees the loop
/// always returns control to the REPL, even if a provider or tool call
/// hangs outright. Sourced from the Ion loop contract (ADR-0017), which
/// also stops the loop earlier on repeated identical calls, repeated
/// identical batches, and same-error streaks (`AgentError::ToolLoopStalled`).
pub const DEFAULT_TOOL_LOOP_TIMEOUT: Duration = crate::loop_contract::ION_DEFAULT_WALL_CLOCK;

pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: Box<dyn LlmProvider>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub history: Vec<Message>,
    /// Harness step facts for [`resolve_step_model`]. Defaults to an Ion/API
    /// Worker context with no review/verification state (v0 identity).
    pub step_context: HarnessStepContext,
    /// The budget every [`Agent::chat_with_tools`] run is bounded by
    /// (ADR-0017). Defaults to [`LoopContract::ion_tool_loop`]. Private so
    /// it can only be replaced through the validating
    /// [`Agent::with_loop_contract`]; the effective contract is validated
    /// again at the execution boundary regardless.
    loop_contract: LoopContract,
    /// Typed evidence from the most recent [`Agent::chat_with_tools`] run,
    /// whether it completed, tripped, or failed. `None` until the first run
    /// and while a run is in progress, so a stale report can never describe
    /// a later turn.
    last_loop_report: Option<LoopReport>,
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
        let step_context = HarnessStepContext::ion_api(model.clone());
        Self {
            id,
            name,
            provider,
            model,
            system_prompt,
            history: Vec::new(),
            step_context,
            loop_contract: LoopContract::ion_tool_loop(),
            last_loop_report: None,
        }
    }

    /// Replaces the loop contract every subsequent [`Agent::chat_with_tools`]
    /// run is bounded by. Rejects a contract that could never run.
    pub fn with_loop_contract(
        mut self,
        contract: LoopContract,
    ) -> Result<Self, crate::loop_contract::LoopContractError> {
        contract.validate()?;
        self.loop_contract = contract;
        Ok(self)
    }

    /// The contract every [`Agent::chat_with_tools`] run is bounded by.
    pub fn loop_contract(&self) -> &LoopContract {
        &self.loop_contract
    }

    /// Typed evidence from the most recent [`Agent::chat_with_tools`] run.
    pub fn last_loop_report(&self) -> Option<&LoopReport> {
        self.last_loop_report.as_ref()
    }

    fn request_model(&self, tool_round: usize) -> String {
        let mut ctx = self.step_context.clone();
        ctx.current_model = self.model.clone();
        ctx.tool_round = tool_round;
        resolve_step_model(&ctx, &self.model, None)
    }

    pub async fn chat(&mut self, user_message: &str) -> AgentResult<String> {
        let mut messages = Vec::new();
        if let Some(ref system) = self.system_prompt {
            messages.push(Message::text(Role::System, system.clone()));
        }
        messages.extend(self.history.clone());
        messages.push(Message::text(Role::User, user_message));

        let request = ChatRequest {
            model: self.request_model(0),
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
    /// until the model returns a plain-text reply or the agent's
    /// [`LoopContract`] trips (round cap, wall clock, repeated identical
    /// call, or same-error streak; ADR-0017). Conversation history is only
    /// committed on a successful (non-error) return, matching
    /// [`Agent::chat`]'s error-path behavior. [`Agent::last_loop_report`]
    /// holds the typed termination evidence afterwards either way.
    pub async fn chat_with_tools(
        &mut self,
        user_message: &str,
        tools: &[ToolDefinition],
        executor: &dyn ToolExecutor,
    ) -> AgentResult<String> {
        let max_rounds = self.loop_contract.budget.max_rounds;
        self.chat_with_tools_capped(user_message, tools, executor, max_rounds)
            .await
    }

    /// Same as [`Agent::chat_with_tools`] with an explicit round cap --
    /// split out so tests can exercise the cap-hit error path without
    /// looping [`DEFAULT_MAX_TOOL_ROUNDS`] times. Uses the contract's
    /// wall-clock budget; see [`Agent::chat_with_tools_capped_timeout`] for
    /// the test-only seam that overrides it. A cap of zero is not a run that
    /// hits its limit immediately: it is an invalid effective contract and is
    /// rejected with `AgentError::InvalidRequest` before any model round.
    pub async fn chat_with_tools_capped(
        &mut self,
        user_message: &str,
        tools: &[ToolDefinition],
        executor: &dyn ToolExecutor,
        max_rounds: usize,
    ) -> AgentResult<String> {
        let wall_clock = self.loop_contract.budget.wall_clock;
        self.chat_with_tools_capped_timeout(user_message, tools, executor, max_rounds, wall_clock)
            .await
    }

    /// Same as [`Agent::chat_with_tools_capped`] with an explicit wall-clock
    /// timeout override -- split out so tests can exercise the timeout error
    /// path with a short duration instead of waiting on
    /// [`DEFAULT_TOOL_LOOP_TIMEOUT`]. Wraps the *entire* multi-round
    /// exchange in [`tokio::time::timeout`], not any single round: the
    /// round-loop body lives in the free fn [`run_tool_loop`], which takes
    /// only the borrows it needs (never `&mut self`) so the `.await` inside
    /// `tokio::time::timeout` doesn't hold a long-lived mutable borrow of
    /// `self`. History is only committed via `self.history = working` on the
    /// success path -- both a round-cap error and a timeout leave
    /// `self.history` exactly as it was before this call, matching
    /// [`Agent::chat`]'s error-path invariant.
    async fn chat_with_tools_capped_timeout(
        &mut self,
        user_message: &str,
        tools: &[ToolDefinition],
        executor: &dyn ToolExecutor,
        max_rounds: usize,
        timeout_duration: Duration,
    ) -> AgentResult<String> {
        // A new run starts with no evidence: whatever happens below, the
        // report a caller reads afterwards describes this run or nothing.
        self.last_loop_report = None;

        // Validate the *effective* contract, after the round and wall-clock
        // overrides, at the execution boundary. `with_loop_contract` already
        // validates stored contracts, but the overrides can still produce a
        // budget that could never run.
        let mut contract = self.loop_contract.clone();
        contract.budget.max_rounds = max_rounds;
        contract.budget.wall_clock = timeout_duration;
        contract
            .validate()
            .map_err(|err| AgentError::InvalidRequest(format!("loop contract rejected: {err}")))?;

        let mut working = self.history.clone();
        working.push(Message::text(Role::User, user_message));

        let mut step_context = self.step_context.clone();
        step_context.current_model = self.model.clone();
        let mut breaker = LoopBreaker::new(contract);
        let loop_future = run_tool_loop(
            self.provider.as_ref(),
            &step_context,
            &self.system_prompt,
            working,
            tools,
            executor,
            &mut breaker,
        );

        let outcome = tokio::time::timeout(timeout_duration, loop_future).await;
        // The loop future is consumed by the timeout above, so the breaker
        // is free again here: it carries the run's counts either way.
        match outcome {
            Ok(Ok((reply, working))) => {
                self.last_loop_report = Some(breaker.report(LoopTermination::Completed));
                self.history = working;
                Ok(reply)
            }
            Ok(Err(LoopExit::Tripped(trip))) => {
                self.last_loop_report =
                    Some(breaker.report(LoopTermination::Tripped { trip: trip.clone() }));
                Err(match trip {
                    LoopTrip::RoundCap { rounds } => AgentError::ToolLoopLimitExceeded { rounds },
                    LoopTrip::WallClock { millis } => AgentError::ToolLoopTimedOut {
                        seconds: millis / 1_000,
                    },
                    other => AgentError::ToolLoopStalled { trip: other },
                })
            }
            Ok(Err(LoopExit::Failed(err))) => {
                self.last_loop_report = Some(breaker.report(LoopTermination::Failed {
                    error: error_signature(&err.to_string()),
                }));
                Err(err)
            }
            Err(_elapsed) => {
                let trip = LoopTrip::WallClock {
                    millis: u64::try_from(timeout_duration.as_millis()).unwrap_or(u64::MAX),
                };
                self.last_loop_report =
                    Some(breaker.report(LoopTermination::Tripped { trip: trip.clone() }));
                Err(AgentError::ToolLoopTimedOut {
                    seconds: timeout_duration.as_secs(),
                })
            }
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}

/// The round-loop body behind [`Agent::chat_with_tools_capped_timeout`],
/// extracted to a free fn that borrows only `provider`/`model`/
/// `system_prompt` (never `&mut Agent`) so its returned future can be
/// wrapped in `tokio::time::timeout` without holding a mutable borrow of the
/// `Agent` across the `.await`. Returns the final reply and the full
/// `working` history (system prompt excluded, matching `Agent::history`'s
/// existing shape) on success, so the caller can decide whether to commit it.
/// Every round is admitted by `breaker` and every executed tool call is
/// reported to it, so a trip stops the loop with typed evidence (ADR-0017).
async fn run_tool_loop(
    provider: &dyn LlmProvider,
    step_context: &HarnessStepContext,
    system_prompt: &Option<String>,
    mut working: Vec<Message>,
    tools: &[ToolDefinition],
    executor: &dyn ToolExecutor,
    breaker: &mut LoopBreaker,
) -> Result<(String, Vec<Message>), LoopExit> {
    loop {
        let tool_round = breaker.begin_round().map_err(LoopExit::Tripped)?;
        let mut messages = Vec::new();
        if let Some(system) = system_prompt {
            messages.push(Message::text(Role::System, system.clone()));
        }
        messages.extend(working.clone());

        let mut ctx = step_context.clone();
        ctx.tool_round = tool_round;
        let request = ChatRequest {
            model: resolve_step_model(&ctx, &step_context.current_model, None),
            messages,
            temperature: 0.7,
            max_tokens: Some(4096),
            tools: tools.to_vec(),
        };
        let response = provider.chat(request).await.map_err(LoopExit::Failed)?;

        if response.stop_reason == StopReason::ToolUse && !response.tool_calls.is_empty() {
            working.push(Message::assistant_tool_use(
                response.content.clone(),
                response.tool_calls.clone(),
            ));

            let mut results = Vec::with_capacity(response.tool_calls.len());
            for call in &response.tool_calls {
                breaker.dispatch_call();
                let outcome = executor.execute(&call.name, call.input.clone()).await;
                let trip = breaker.observe_call(
                    &call.name,
                    &call.input,
                    CallOutcome {
                        is_error: outcome.is_error,
                        content: &outcome.content,
                    },
                );
                results.push(ToolResult {
                    tool_use_id: call.id.clone(),
                    content: outcome.content,
                    is_error: outcome.is_error,
                });
                if let Some(trip) = trip {
                    // The breaker is open: no further call in this batch may
                    // run, even though the model requested it. The caller
                    // discards `working` on this path, so the partial batch
                    // never reaches history.
                    return Err(LoopExit::Tripped(trip));
                }
            }
            working.push(Message::tool_results(results));
            if let Some(trip) = breaker.end_round() {
                return Err(LoopExit::Tripped(trip));
            }
            continue;
        }

        working.push(Message::text(Role::Assistant, response.content.clone()));
        return Ok((response.content, working));
    }
}

/// Why [`run_tool_loop`] returned without a final reply: the contract
/// tripped, or a provider call failed outright.
#[derive(Debug)]
enum LoopExit {
    Tripped(LoopTrip),
    Failed(AgentError),
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

    struct RecordingModelProvider {
        models: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tool_first: bool,
        calls: std::sync::Mutex<usize>,
    }

    impl RecordingModelProvider {
        fn new() -> Self {
            Self {
                models: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                tool_first: false,
                calls: std::sync::Mutex::new(0),
            }
        }

        fn with_one_tool_round() -> Self {
            Self {
                models: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                tool_first: true,
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for RecordingModelProvider {
        fn name(&self) -> &str {
            "recording-model-fake"
        }
        fn default_model(&self) -> &str {
            "recording-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            self.models
                .lock()
                .expect("lock is never poisoned in tests")
                .push(request.model.clone());
            let mut calls = self.calls.lock().expect("lock is never poisoned in tests");
            *calls += 1;
            if self.tool_first && *calls == 1 {
                return Ok(ChatResponse {
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
                        input: serde_json::json!({}),
                    }],
                });
            }
            Ok(ChatResponse {
                content: "ok".to_string(),
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
            vec!["recording-fake-model"]
        }
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
        let report = agent.last_loop_report().expect("cap trip leaves a report");
        assert_eq!(
            report.termination,
            LoopTermination::Tripped {
                trip: LoopTrip::RoundCap { rounds: 2 }
            }
        );
        assert_eq!(report.rounds_used, 2);
        assert_eq!(report.tool_calls, 2);
    }

    /// Always sleeps longer than any sane test timeout before returning a
    /// plain-text reply -- used to prove the wall-clock timeout actually
    /// fires instead of waiting on a hung provider forever.
    struct SlowProvider {
        delay: std::time::Duration,
    }

    #[async_trait]
    impl LlmProvider for SlowProvider {
        fn name(&self) -> &str {
            "slow-fake"
        }
        fn default_model(&self) -> &str {
            "slow-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            tokio::time::sleep(self.delay).await;
            Ok(ChatResponse {
                content: "eventually replied".to_string(),
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
            vec!["slow-fake-model"]
        }
    }

    #[tokio::test]
    async fn test_chat_with_tools_capped_timeout_returns_error_when_timeout_hit() {
        let mut agent = test_agent(SlowProvider {
            delay: Duration::from_secs(30),
        });
        let executor = EchoExecutor::new();

        let result = agent
            .chat_with_tools_capped_timeout(
                "do the thing",
                &[],
                &executor,
                DEFAULT_MAX_TOOL_ROUNDS,
                Duration::from_millis(100),
            )
            .await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopTimedOut { seconds: 0 })
        ));
        // History must be left untouched on the timeout path too -- the
        // provider's slow `chat()` call never gets to commit `working` back
        // onto `self.history`, matching the round-cap-exceeded invariant
        // above.
        assert!(agent.history.is_empty());
        let report = agent.last_loop_report().expect("timeout leaves a report");
        assert_eq!(
            report.termination,
            LoopTermination::Tripped {
                trip: LoopTrip::WallClock { millis: 100 }
            }
        );
        assert_eq!(report.rounds_used, 1, "the first round had begun");
        assert_eq!(report.tool_calls, 0);
    }

    #[tokio::test]
    async fn test_chat_calls_decide_step_model_identity() {
        let provider = RecordingModelProvider::new();
        let recorded = std::sync::Arc::clone(&provider.models);
        let mut agent = test_agent(provider);
        let reply = agent.chat("hello").await.expect("chat should succeed");
        assert_eq!(reply, "ok");
        assert_eq!(recorded.lock().unwrap().as_slice(), ["test-model"]);
    }

    #[tokio::test]
    async fn test_chat_after_verifier_failure_sends_escalate_model() {
        use impulse_ops::governed_task::GovernedVerificationOutcome;

        let provider = RecordingModelProvider::new();
        let recorded = std::sync::Arc::clone(&provider.models);
        let mut agent = test_agent(provider);
        agent.step_context.latest_verification = Some(GovernedVerificationOutcome::Failed);
        agent.step_context.escalate_model = Some("escalate-model".to_string());
        agent.chat("retry").await.expect("chat should succeed");
        assert_eq!(recorded.lock().unwrap().as_slice(), ["escalate-model"]);
    }

    #[tokio::test]
    async fn test_run_tool_loop_calls_decide_step_model_each_round() {
        use impulse_ops::governed_task::GovernedVerificationOutcome;

        let provider = RecordingModelProvider::with_one_tool_round();
        let recorded = std::sync::Arc::clone(&provider.models);
        let mut agent = test_agent(provider);
        agent.step_context.latest_verification = Some(GovernedVerificationOutcome::Failed);
        agent.step_context.escalate_model = Some("escalate-model".to_string());
        let executor = EchoExecutor::new();
        let reply = agent
            .chat_with_tools("do it", &[], &executor)
            .await
            .expect("tool loop should finish");
        assert_eq!(reply, "ok");
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            ["escalate-model", "escalate-model"]
        );
    }

    /// Always requests `echo_tool`, but with a different input every call,
    /// so the repeated-call detector never fires and only the same-error
    /// detector can trip.
    struct VaryingToolProvider {
        calls: std::sync::Mutex<usize>,
    }

    #[async_trait]
    impl LlmProvider for VaryingToolProvider {
        fn name(&self) -> &str {
            "varying-tool-fake"
        }
        fn default_model(&self) -> &str {
            "varying-tool-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let mut calls = self.calls.lock().expect("lock is never poisoned in tests");
            *calls += 1;
            Ok(ChatResponse {
                content: String::new(),
                model: request.model,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::ToolUse,
                tool_calls: vec![ToolCall {
                    id: format!("call_{}", *calls),
                    name: "echo_tool".to_string(),
                    input: serde_json::json!({"n": *calls}),
                }],
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["varying-tool-fake-model"]
        }
    }

    /// Fails every call the same way, with trailing detail that differs so
    /// the signature (first line) is what must match.
    struct FailingExecutor {
        calls: std::sync::Mutex<usize>,
    }

    #[async_trait]
    impl ToolExecutor for FailingExecutor {
        async fn execute(&self, _name: &str, _input: serde_json::Value) -> ToolExecutionResult {
            let mut calls = self.calls.lock().expect("lock is never poisoned in tests");
            *calls += 1;
            ToolExecutionResult {
                content: format!("permission denied: /etc/shadow\nattempt {}", *calls),
                is_error: true,
            }
        }
    }

    #[test]
    fn test_default_loop_constants_match_ion_contract() {
        let contract = LoopContract::ion_tool_loop();
        assert_eq!(DEFAULT_MAX_TOOL_ROUNDS, contract.budget.max_rounds);
        assert_eq!(DEFAULT_TOOL_LOOP_TIMEOUT, contract.budget.wall_clock);
        assert_eq!(
            test_agent(FixedReplyProvider { content: "x" }).loop_contract(),
            &contract
        );
    }

    #[tokio::test]
    async fn test_chat_with_tools_trips_on_repeated_identical_calls() {
        // AlwaysToolUseProvider re-issues the exact same call every round;
        // the default Ion contract trips on the third identical call, well
        // before the ten-round cap.
        let mut agent = test_agent(AlwaysToolUseProvider);
        let executor = EchoExecutor::new();

        let result = agent.chat_with_tools("do the thing", &[], &executor).await;

        match result {
            Err(AgentError::ToolLoopStalled {
                trip: LoopTrip::RepeatedCall { tool, streak },
            }) => {
                assert_eq!(tool, "echo_tool");
                assert_eq!(streak, 3);
            }
            other => panic!("expected RepeatedCall stall, got: {other:?}"),
        }
        assert_eq!(executor.invocations.lock().unwrap().len(), 3);
        assert!(agent.history.is_empty(), "history untouched on stall");
        let report = agent.last_loop_report().expect("stall leaves a report");
        assert_eq!(report.contract, "ion_tool_loop");
        assert_eq!(report.rounds_used, 3);
        assert_eq!(report.tool_calls, 3);
        assert_eq!(report.tool_errors, 0);
        assert!(matches!(
            report.termination,
            LoopTermination::Tripped {
                trip: LoopTrip::RepeatedCall { .. }
            }
        ));
    }

    #[tokio::test]
    async fn test_chat_with_tools_trips_on_same_error_streak() {
        let mut agent = test_agent(VaryingToolProvider {
            calls: std::sync::Mutex::new(0),
        });
        let executor = FailingExecutor {
            calls: std::sync::Mutex::new(0),
        };

        let result = agent.chat_with_tools("do the thing", &[], &executor).await;

        match result {
            Err(AgentError::ToolLoopStalled {
                trip:
                    LoopTrip::SameError {
                        tool,
                        streak,
                        signature,
                    },
            }) => {
                assert_eq!(tool, "echo_tool");
                assert_eq!(streak, 3);
                assert_eq!(signature, "permission denied: /etc/shadow");
            }
            other => panic!("expected SameError stall, got: {other:?}"),
        }
        assert_eq!(*executor.calls.lock().unwrap(), 3);
        assert!(agent.history.is_empty());
        let report = agent.last_loop_report().expect("stall leaves a report");
        assert_eq!(report.tool_calls, 3);
        assert_eq!(report.tool_errors, 3);
    }

    #[tokio::test]
    async fn test_chat_with_tools_success_records_completed_report() {
        let mut agent = test_agent(OneShotToolProvider::new());
        let executor = EchoExecutor::new();

        let reply = agent
            .chat_with_tools("do the thing", &[], &executor)
            .await
            .expect("one tool round then a final reply");

        assert_eq!(reply, "final answer");
        let report = agent.last_loop_report().expect("success leaves a report");
        assert_eq!(report.termination, LoopTermination::Completed);
        assert_eq!(report.rounds_used, 2);
        assert_eq!(report.tool_calls, 1);
        assert_eq!(report.tool_errors, 0);
    }

    #[tokio::test]
    async fn test_with_loop_contract_bounds_subsequent_runs() {
        let mut contract = LoopContract::ion_tool_loop();
        contract.name = "tight".to_string();
        contract.budget.max_rounds = 1;
        let mut agent = test_agent(OneShotToolProvider::new())
            .with_loop_contract(contract)
            .expect("a one-round contract is valid");
        let executor = EchoExecutor::new();

        let result = agent.chat_with_tools("do the thing", &[], &executor).await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopLimitExceeded { rounds: 1 })
        ));
        assert_eq!(agent.last_loop_report().unwrap().contract, "tight");
    }

    #[test]
    fn test_with_loop_contract_rejects_invalid_budget() {
        let mut contract = LoopContract::ion_tool_loop();
        contract.budget.max_rounds = 0;
        let result = test_agent(FixedReplyProvider { content: "x" }).with_loop_contract(contract);
        assert!(matches!(
            result,
            Err(crate::loop_contract::LoopContractError::ZeroRounds { .. })
        ));
    }

    /// Requests four tool calls in one response: the same call three times,
    /// then a different one. With the default streak limit of 3 the third
    /// call trips mid-batch, so the fourth must never execute.
    struct BatchedRepeatProvider;

    #[async_trait]
    impl LlmProvider for BatchedRepeatProvider {
        fn name(&self) -> &str {
            "batched-repeat-fake"
        }
        fn default_model(&self) -> &str {
            "batched-repeat-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let same = serde_json::json!({"command": "ls"});
            let mut tool_calls: Vec<ToolCall> = (1..=3)
                .map(|i| ToolCall {
                    id: format!("call_{i}"),
                    name: "echo_tool".to_string(),
                    input: same.clone(),
                })
                .collect();
            tool_calls.push(ToolCall {
                id: "call_4".to_string(),
                name: "echo_tool".to_string(),
                input: serde_json::json!({"command": "pwd"}),
            });
            Ok(ChatResponse {
                content: String::new(),
                model: request.model,
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::ToolUse,
                tool_calls,
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["batched-repeat-fake-model"]
        }
    }

    /// Fails every model round outright, as a provider outage would.
    struct ErroringProvider;

    #[async_trait]
    impl LlmProvider for ErroringProvider {
        fn name(&self) -> &str {
            "erroring-fake"
        }
        fn default_model(&self) -> &str {
            "erroring-fake-model"
        }
        async fn chat(&self, _request: ChatRequest) -> AgentResult<ChatResponse> {
            Err(AgentError::ApiRequest(
                "boom: upstream 503\ndetail".to_string(),
            ))
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["erroring-fake-model"]
        }
    }

    #[tokio::test]
    async fn test_batched_tool_calls_stop_executing_once_breaker_trips() {
        let mut agent = test_agent(BatchedRepeatProvider);
        let executor = EchoExecutor::new();

        let result = agent.chat_with_tools("do the thing", &[], &executor).await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopStalled {
                trip: LoopTrip::RepeatedCall { streak: 3, .. }
            })
        ));
        // Three calls ran (the third tripped); the fourth, distinct call in
        // the same batch never executed.
        let invocations = executor.invocations.lock().unwrap();
        assert_eq!(invocations.len(), 3);
        assert!(invocations
            .iter()
            .all(|(_, input)| input == &serde_json::json!({"command": "ls"})));
        let report = agent.last_loop_report().expect("stall leaves a report");
        assert_eq!(report.tool_calls, 3);
        assert_eq!(report.rounds_used, 1);
        assert!(agent.history.is_empty());
    }

    #[tokio::test]
    async fn test_provider_failure_replaces_stale_loop_report() {
        let mut agent = test_agent(OneShotToolProvider::new());
        let executor = EchoExecutor::new();
        agent
            .chat_with_tools("first", &[], &executor)
            .await
            .expect("first run completes");
        assert_eq!(
            agent.last_loop_report().unwrap().termination,
            LoopTermination::Completed
        );

        agent.provider = Box::new(ErroringProvider);
        let result = agent.chat_with_tools("second", &[], &executor).await;

        assert!(matches!(result, Err(AgentError::ApiRequest(_))));
        let report = agent
            .last_loop_report()
            .expect("a failed run leaves a report");
        match &report.termination {
            LoopTermination::Failed { error } => {
                assert!(error.contains("boom"), "{error}");
                assert!(!error.contains("detail"), "only the first line: {error}");
            }
            other => panic!("expected Failed termination, got {other:?}"),
        }
        assert_eq!(report.rounds_used, 1);
        assert_eq!(report.tool_calls, 0);
        // History still holds only the first, successful exchange.
        assert_eq!(agent.history.len(), 4);
    }

    #[tokio::test]
    async fn test_invalid_effective_contract_is_rejected_before_the_loop_runs() {
        // Seed a completed run first, so the assertions below prove the
        // stale report is cleared by the rejection and that no further
        // model call is made -- not merely that a fresh agent has nothing.
        let provider = RecordingModelProvider::with_one_tool_round();
        let recorded = std::sync::Arc::clone(&provider.models);
        let mut agent = test_agent(provider);
        let executor = EchoExecutor::new();
        agent
            .chat_with_tools("warm up", &[], &executor)
            .await
            .expect("seed run completes");
        assert!(agent.last_loop_report().is_some());
        let model_calls_before = recorded.lock().unwrap().len();
        let invocations_before = executor.invocations.lock().unwrap().len();
        let history_before = agent.history.len();

        let result = agent.chat_with_tools_capped("go", &[], &executor, 0).await;
        assert!(
            matches!(result, Err(AgentError::InvalidRequest(ref msg)) if msg.contains("at least one round")),
            "{result:?}"
        );
        assert!(
            agent.last_loop_report().is_none(),
            "a rejected request must not leave the previous run's report"
        );
        assert_eq!(recorded.lock().unwrap().len(), model_calls_before);
        assert_eq!(
            executor.invocations.lock().unwrap().len(),
            invocations_before
        );
        assert_eq!(agent.history.len(), history_before);

        let result = agent
            .chat_with_tools_capped_timeout("go", &[], &executor, 3, Duration::ZERO)
            .await;
        assert!(
            matches!(result, Err(AgentError::InvalidRequest(ref msg)) if msg.contains("wall-clock")),
            "{result:?}"
        );
        assert_eq!(recorded.lock().unwrap().len(), model_calls_before);
    }

    /// Fails every call with a pretty-printed JSON payload, the shape a
    /// bridged dynamic tool such as `bash_exec` produces, where the first
    /// line is just `{` and the command differs per input.
    struct JsonFailureExecutor;

    #[async_trait]
    impl ToolExecutor for JsonFailureExecutor {
        async fn execute(&self, _name: &str, input: serde_json::Value) -> ToolExecutionResult {
            ToolExecutionResult {
                content: format!(
                    "{{\n  \"command\": \"cmd-{}\",\n  \"exit_code\": 1,\n  \"success\": false\n}}",
                    input["n"]
                ),
                is_error: true,
            }
        }
    }

    #[tokio::test]
    async fn test_distinct_json_failures_do_not_trip_same_error() {
        let mut contract = LoopContract::ion_tool_loop();
        contract.budget.max_rounds = 4;
        let mut agent = test_agent(VaryingToolProvider {
            calls: std::sync::Mutex::new(0),
        })
        .with_loop_contract(contract)
        .expect("valid contract");

        let result = agent.chat_with_tools("go", &[], &JsonFailureExecutor).await;

        // Four different commands failed four different ways: that is the
        // round cap, not a same-error stall.
        assert!(
            matches!(result, Err(AgentError::ToolLoopLimitExceeded { rounds: 4 })),
            "{result:?}"
        );
        let report = agent.last_loop_report().unwrap();
        assert_eq!(report.tool_calls, 4);
        assert_eq!(report.tool_errors, 4);
    }

    /// Requests the same two-call batch on every round, the normal shape of
    /// a parallel tool-use response, so the per-call detector alone never
    /// sees a repeat.
    struct SameBatchProvider;

    #[async_trait]
    impl LlmProvider for SameBatchProvider {
        fn name(&self) -> &str {
            "same-batch-fake"
        }
        fn default_model(&self) -> &str {
            "same-batch-fake-model"
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
                tool_calls: vec![
                    ToolCall {
                        id: "a".to_string(),
                        name: "file_read".to_string(),
                        input: serde_json::json!({"path": "a"}),
                    },
                    ToolCall {
                        id: "b".to_string(),
                        name: "file_read".to_string(),
                        input: serde_json::json!({"path": "b"}),
                    },
                ],
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["same-batch-fake-model"]
        }
    }

    #[tokio::test]
    async fn test_repeated_batch_trips_after_three_identical_rounds() {
        let mut agent = test_agent(SameBatchProvider);
        let executor = EchoExecutor::new();

        let result = agent.chat_with_tools("go", &[], &executor).await;

        assert!(
            matches!(
                result,
                Err(AgentError::ToolLoopStalled {
                    trip: LoopTrip::RepeatedRound {
                        calls: 2,
                        streak: 3
                    }
                })
            ),
            "{result:?}"
        );
        assert_eq!(executor.invocations.lock().unwrap().len(), 6);
        let report = agent.last_loop_report().unwrap();
        assert_eq!(report.rounds_used, 3);
        assert_eq!(report.tool_calls, 6);
        assert!(agent.history.is_empty());
    }

    /// Never finishes executing within any test timeout.
    struct HangingExecutor;

    #[async_trait]
    impl ToolExecutor for HangingExecutor {
        async fn execute(&self, _name: &str, _input: serde_json::Value) -> ToolExecutionResult {
            tokio::time::sleep(Duration::from_secs(30)).await;
            ToolExecutionResult {
                content: "never".to_string(),
                is_error: false,
            }
        }
    }

    #[tokio::test]
    async fn test_wall_clock_cutoff_mid_tool_call_reports_the_call_as_interrupted() {
        let mut agent = test_agent(AlwaysToolUseProvider);

        let result = agent
            .chat_with_tools_capped_timeout(
                "go",
                &[],
                &HangingExecutor,
                DEFAULT_MAX_TOOL_ROUNDS,
                Duration::from_millis(100),
            )
            .await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopTimedOut { seconds: 0 })
        ));
        let report = agent.last_loop_report().unwrap();
        assert_eq!(
            report.termination,
            LoopTermination::Tripped {
                trip: LoopTrip::WallClock { millis: 100 }
            }
        );
        assert_eq!(report.rounds_used, 1);
        assert_eq!(report.tool_calls, 0, "the call never completed");
        assert_eq!(report.tool_calls_interrupted, 1, "but it was dispatched");
    }

    #[tokio::test]
    async fn test_disabled_detectors_fall_through_to_round_cap() {
        let mut contract = LoopContract::ion_tool_loop();
        contract.budget.max_rounds = 4;
        contract.budget.max_repeated_call_streak = None;
        contract.budget.max_same_error_streak = None;
        let mut agent = test_agent(AlwaysToolUseProvider)
            .with_loop_contract(contract)
            .expect("valid contract");
        let executor = EchoExecutor::new();

        let result = agent.chat_with_tools("do the thing", &[], &executor).await;

        assert!(matches!(
            result,
            Err(AgentError::ToolLoopLimitExceeded { rounds: 4 })
        ));
        assert_eq!(executor.invocations.lock().unwrap().len(), 4);
    }
}
