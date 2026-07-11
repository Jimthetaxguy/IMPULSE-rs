//! Chat-turn wiring for the ion REPL (TUI_SPEC.md T8/T9).
//!
//! TUI_SPEC.md section 2.3 describes free-text turns going through
//! "`llm_backends::ChatSession`" — no literal `ChatSession` type exists in
//! `llm_backends`; [`crate::llm_backends::Agent`] is the substrate that
//! matches the description (`.chat(&mut self, msg) -> AgentResult<String>`
//! with conversation `history` + `.clear_history()`, provider-agnostic via
//! `Box<dyn LlmProvider>`). [`ChatState`] wraps one `Agent` instance, built
//! once per `ReplSession` (`ion_repl::mod::ReplSession::new`) and held
//! mutably across turns so history survives between REPL lines instead of
//! being rebuilt (and lost) on every line.
//!
//! **Missing-key handling:** unlike `handlers::system::handle_chat` (which
//! pre-checks `ANTHROPIC_API_KEY` with an `anyhow::bail!` before ever
//! constructing a provider), `ChatState::from_env` always constructs an
//! `AnthropicProvider` — even with an empty key string — matching the typed
//! `AgentError::MissingApiKey` path used by `ImpulseAgent::new`
//! (`src/agent/mod.rs`). `AnthropicProvider::chat` calls
//! `BaseProvider::check_api_key()` first and returns
//! `Err(AgentError::MissingApiKey { .. })` without ever opening a network
//! connection, so `ChatState::turn` naturally surfaces that typed error for
//! the REPL to render as a one-line notice (`ion_repl::mod::respond`) — no
//! panic, no special-cased startup check needed.
//!
//! **Tool-calling (T9):** every `turn()` now runs through
//! [`Agent::chat_with_tools`], with the REPL's own [`ReplToolRegistry`]
//! exposed as Anthropic tool-use schemas (`ReplTool::json_schema`, already
//! shaped as `{name, description, input_schema}` — see `tools.rs`).
//! [`ReplToolExecutor`] adapts the registry (plus the per-session
//! `ReplContext`) to the provider-agnostic `llm_backends::ToolExecutor`
//! trait, so `llm_backends` itself never depends on `ion_repl` types. A
//! registry with no tools degrades to an empty `tools: []` request, which
//! behaves identically to the pre-T9 plain-chat path (no behavior change
//! for `ReplToolRegistry::new()`-based tests, other than the new required
//! parameters).
//!
//! **Confirmation gate (T9 adversarial-review follow-up):** before T9,
//! `file_write`/`bash_exec` were registered in `ReplToolRegistry` but had no
//! reachable execution path — only `/verify` was wired to a tool call, and
//! that's a hand-typed, user-initiated command. T9 made every registered
//! tool reachable from raw model output for the first time, with no
//! confirmation step -- unlike `claude`/`codex`, which prompt before
//! write/bash actions by default (auto-accept/yolo mode is opt-in, not the
//! default). [`CONFIRMATION_REQUIRED_TOOLS`] gates `bash_exec`/`file_write`
//! specifically (mutating capabilities) behind [`ReplToolExecutor::confirm`]
//! -- a declined confirmation short-circuits before `ReplTool::run` is ever
//! called, so nothing executes. `ion_verify`/`file_read` are read-only and
//! stay auto-approved, matching `/verify`'s existing ungated behavior.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::AgentResult;
use crate::llm_backends::{Agent, LlmProvider, ToolDefinition, ToolExecutionResult, ToolExecutor};

use super::registry::ReplToolRegistry;
use super::tools::ToolOutcome;
use super::ReplContext;

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const SYSTEM_PROMPT: &str =
    "You are ion, an interactive coding-agent assistant running in a terminal REPL.";

/// A confirmation hook: given a tool name and its input, returns whether the
/// call is approved. `Box<dyn Fn(&str, &Value) -> bool + Send + Sync>` named
/// to satisfy `clippy::type_complexity` and give the concept a name.
type ConfirmFn = Box<dyn Fn(&str, &Value) -> bool + Send + Sync>;

/// Holds the `Agent` (provider + model + conversation history) backing free
/// text chat turns in the ion REPL, plus the confirmation hook gating
/// mutating tool calls (see module doc comment).
pub struct ChatState {
    agent: Agent,
    confirm: ConfirmFn,
}

impl ChatState {
    /// Builds chat state from `ANTHROPIC_API_KEY`/`CLAUDE_API_KEY` (same
    /// fallback order as `handlers::system::handle_chat`) and `IMPULSE_MODEL`
    /// (override, else `claude-sonnet-4-6`). Never fails and never requires
    /// the key to be present — see the module doc comment for why a missing
    /// key is handled lazily, on first `turn()`, instead of here.
    pub fn from_env() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .unwrap_or_default();
        let model = std::env::var("IMPULSE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let provider: Box<dyn LlmProvider> = Box::new(
            crate::llm_backends::anthropic::AnthropicProvider::new(api_key),
        );
        Self::with_provider(provider, model)
    }

    /// Test/DI seam: build chat state with an arbitrary provider (e.g. a
    /// fake `LlmProvider` from [`test_support`]) instead of the real
    /// Anthropic backend. Also usable by future providers (OpenAI, Minimax).
    /// Uses the real stdin confirmation prompt for mutating tools; tests
    /// that need to drive `bash_exec`/`file_write` through a full `turn()`
    /// without blocking on stdin should use [`ChatState::with_confirm`].
    pub fn with_provider(provider: Box<dyn LlmProvider>, model: String) -> Self {
        Self {
            agent: Agent::new(
                "ion-repl".to_string(),
                "ion".to_string(),
                provider,
                Some(model),
                Some(SYSTEM_PROMPT.to_string()),
            ),
            confirm: Box::new(confirm_via_stdin),
        }
    }

    /// Test-only seam: override the confirmation hook (default:
    /// [`confirm_via_stdin`], which blocks on real stdin and is unusable in
    /// an automated test). Consumes and returns `self` for chaining onto
    /// [`ChatState::with_provider`].
    #[cfg(test)]
    pub(crate) fn with_confirm(
        mut self,
        confirm: impl Fn(&str, &Value) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.confirm = Box::new(confirm);
        self
    }

    /// Sends one chat turn through the underlying `Agent`, with `tools`
    /// exposed to the model as Anthropic tool-use schemas (T9). Requested
    /// tool calls are executed against `tools`/`ctx` and their results sent
    /// back to the model automatically (`Agent::chat_with_tools`, capped at
    /// `llm_backends::DEFAULT_MAX_TOOL_ROUNDS` round trips and, as of the
    /// same-day Opus adversarial-review follow-up (finding S2),
    /// `llm_backends::DEFAULT_TOOL_LOOP_TIMEOUT` wall-clock) -- mutating
    /// calls (`bash_exec`/`file_write`) are gated behind `self.confirm`
    /// first (see module doc comment). Returns the assistant's final reply
    /// text, or the `AgentError` the provider or loop cap failed with
    /// (including `AgentError::MissingApiKey` for an absent key,
    /// `AgentError::ToolLoopLimitExceeded` if the round cap is hit, and
    /// `AgentError::ToolLoopTimedOut` if the wall-clock budget elapses
    /// first) — never panics, matching every other `Result`-returning path
    /// in this crate.
    ///
    /// **Timeout caveat (fresh Opus sweep, finding G1):** the wall-clock
    /// timeout bounds every `.await` point in the loop -- provider calls,
    /// tool execution -- but a blocking `confirm` prompt
    /// (`confirm_via_stdin`) is a synchronous `stdin().read_line()`, not an
    /// `.await`, so `tokio::time::timeout` cannot cancel it. If a user is
    /// asked `Allow? [y/N]` and walks away, that specific turn blocks
    /// indefinitely regardless of `DEFAULT_TOOL_LOOP_TIMEOUT` -- it does not
    /// deadlock the runtime (the default multi-thread `#[tokio::main]`
    /// flavor just parks the one worker thread), but the REPL itself won't
    /// proceed until the prompt is answered. Accepted as-is: this is a
    /// human explicitly being asked to respond, not a hang in automated
    /// logic, and forcing it through `spawn_blocking` wouldn't make it
    /// cancellable either (the read would keep blocking on its own thread
    /// after an abandoned timeout, risking a stray stdin read racing a
    /// later prompt) -- it would just add complexity without fixing the
    /// underlying property. Full interruptibility (Ctrl-C mid-round) would
    /// need cancellation tokens threaded through the REPL's readline/event
    /// loop, a separate and larger change.
    pub async fn turn(
        &mut self,
        text: &str,
        tools: &ReplToolRegistry,
        ctx: &ReplContext,
    ) -> AgentResult<String> {
        let tool_defs = tool_definitions(tools);
        let executor = ReplToolExecutor {
            tools,
            ctx,
            confirm: self.confirm.as_ref(),
        };
        self.agent
            .chat_with_tools(text, &tool_defs, &executor)
            .await
    }

    /// Clears conversation history on the same `Agent` instance (T8 wires
    /// the previously-stubbed `/clear` command to this).
    pub fn clear(&mut self) {
        self.agent.clear_history();
    }

    /// Number of messages (user + assistant) currently held in history.
    /// `#[cfg(test)]`-only accessor used to prove `/clear` actually empties
    /// history rather than merely printing a confirmation string.
    #[cfg(test)]
    pub(crate) fn history_len(&self) -> usize {
        self.agent.history.len()
    }
}

/// Builds Anthropic tool-use schemas from every tool in `registry`
/// (`ReplTool::json_schema` already returns `{name, description,
/// input_schema}` — see `tools.rs`). An empty registry yields an empty
/// `Vec`, which `Agent::chat_with_tools` treats as "no tools this turn"
/// (`ChatRequest::tools` empty -> omitted from the wire request entirely).
fn tool_definitions(registry: &ReplToolRegistry) -> Vec<ToolDefinition> {
    registry
        .list()
        .into_iter()
        .map(|tool| {
            let schema = tool.json_schema();
            ToolDefinition {
                name: schema
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| tool.name())
                    .to_string(),
                description: schema
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                input_schema: schema
                    .get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
            }
        })
        .collect()
}

/// Tool names whose execution mutates state (filesystem writes, shell
/// commands) and therefore require an interactive confirmation before
/// running when triggered by model-generated `tool_use` output. `ion_verify`
/// (read-only, spec-a gate) and `file_read` are deliberately not gated:
/// `ion_verify` is already ungated when hand-typed via `/verify`, and
/// `file_read` cannot mutate anything.
const CONFIRMATION_REQUIRED_TOOLS: &[&str] = &["bash_exec", "file_write"];

/// Adapts a [`ReplToolRegistry`] (plus the per-session [`ReplContext`]) to
/// `llm_backends::ToolExecutor`, so `Agent::chat_with_tools` can execute
/// tool calls the model requests without `llm_backends` depending on
/// `ion_repl` types. Borrows both for the lifetime of one `turn()` call.
/// `confirm` is consulted before any tool in [`CONFIRMATION_REQUIRED_TOOLS`]
/// runs; a `false` short-circuits before `ReplTool::run` is ever called, so
/// a declined call has no side effects.
struct ReplToolExecutor<'a> {
    tools: &'a ReplToolRegistry,
    ctx: &'a ReplContext,
    confirm: &'a (dyn Fn(&str, &Value) -> bool + Send + Sync),
}

#[async_trait]
impl ToolExecutor for ReplToolExecutor<'_> {
    async fn execute(&self, name: &str, input: Value) -> ToolExecutionResult {
        if CONFIRMATION_REQUIRED_TOOLS.contains(&name) && !(self.confirm)(name, &input) {
            return ToolExecutionResult {
                content: format!(
                    "User declined to run '{name}'. Ask before assuming it happened, and \
                     do not retry without explicit approval."
                ),
                is_error: true,
            };
        }
        match self.tools.get(name) {
            Some(tool) => match tool.run(input, self.ctx).await {
                Ok(outcome) => ToolExecutionResult {
                    content: tool_result_content(&outcome),
                    is_error: !outcome.ok,
                },
                Err(err) => ToolExecutionResult {
                    content: format!("{err:#}"),
                    is_error: true,
                },
            },
            None => ToolExecutionResult {
                content: format!("Tool '{name}' is not registered."),
                is_error: true,
            },
        }
    }
}

/// Real confirmation prompt: prints the pending tool call and blocks on
/// stdin for a `y`/`yes` response (default: decline). Blocking is
/// consistent with the rest of the REPL's interaction model (`rustyline`'s
/// `readline()` already blocks the same way in `ReplSession::run`) — `ion`
/// is a single-user, single-session terminal REPL, not a server handling
/// concurrent requests, so there is no other work this could stall.
fn confirm_via_stdin(name: &str, input: &Value) -> bool {
    use std::io::Write;
    println!("ion wants to run '{name}' with arguments: {input}");
    print!("Allow? [y/N] ");
    if std::io::stdout().flush().is_err() {
        return false;
    }
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Text sent back to the model for a `tool_result` block: the tool's own
/// rendered transcript text when present, else its structured payload.
fn tool_result_content(outcome: &ToolOutcome) -> String {
    if outcome.rendered.is_empty() {
        outcome.payload.to_string()
    } else {
        outcome.rendered.clone()
    }
}

/// Fake `LlmProvider`s for T8 tests. This codebase has no pre-existing test
/// double for `LlmProvider` (`handlers::system::handle_chat`'s tests
/// exercise the real `AnthropicProvider` and accept "may fail with API key
/// or network" rather than mocking — see that module's `mod tests`), so a
/// minimal one is added here, shared by this module's own tests and
/// `ion_repl::mod`'s routing tests, rather than duplicated per test module.
#[cfg(test)]
pub(crate) mod test_support {
    use async_trait::async_trait;

    use crate::error::{AgentError, AgentResult};
    use crate::llm_backends::{
        ChatRequest, ChatResponse, LlmProvider, StopReason, ToolCall, Usage,
    };

    /// Echoes the last user message back with a fixed prefix, so tests can
    /// assert the exact text sent by `ChatState::turn` reached the provider.
    /// Always returns `StopReason::EndTurn` (no tool_calls) — a plain-text
    /// reply on the first tool-loop round, matching pre-T9 behavior.
    pub(crate) struct EchoProvider {
        pub prefix: &'static str,
    }

    #[async_trait]
    impl LlmProvider for EchoProvider {
        fn name(&self) -> &str {
            "echo-fake"
        }
        fn default_model(&self) -> &str {
            "echo-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let last_user = request
                .messages
                .iter()
                .rev()
                .find(|m| m.role == crate::llm_backends::Role::User)
                .map(|m| m.content.clone())
                .unwrap_or_default();
            Ok(ChatResponse {
                content: format!("{}{}", self.prefix, last_user),
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
            vec!["echo-fake-model"]
        }
    }

    /// Requests `echo_tool` on the first call, then returns a fixed final
    /// reply on every subsequent call — proves `ChatState::turn` actually
    /// drives `Agent::chat_with_tools`'s round trip (execute -> tool_result
    /// -> final reply), not just a single-shot request. Call count uses a
    /// sync `Mutex` since `LlmProvider::chat` takes `&self`.
    pub(crate) struct ScriptedToolProvider {
        calls: std::sync::Mutex<usize>,
    }

    impl ScriptedToolProvider {
        pub(crate) fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ScriptedToolProvider {
        fn name(&self) -> &str {
            "scripted-tool-fake"
        }
        fn default_model(&self) -> &str {
            "scripted-tool-fake-model"
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
                        name: "ion_verify".to_string(),
                        input: serde_json::json!({"diff_ref": "HEAD"}),
                    }],
                })
            } else {
                Ok(ChatResponse {
                    content: "final reply after tool use".to_string(),
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
            vec!["scripted-tool-fake-model"]
        }
    }

    /// Requests `bash_exec` with `command` on the first call, then returns a
    /// fixed final reply on every subsequent call -- used to prove a
    /// declined confirmation reaches all the way through `ChatState::turn`'s
    /// tool loop without ever running the command.
    pub(crate) struct ScriptedBashExecProvider {
        pub command: String,
    }

    #[async_trait]
    impl LlmProvider for ScriptedBashExecProvider {
        fn name(&self) -> &str {
            "scripted-bash-fake"
        }
        fn default_model(&self) -> &str {
            "scripted-bash-fake-model"
        }
        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let already_has_tool_result =
                request.messages.iter().any(|m| !m.tool_results.is_empty());
            if already_has_tool_result {
                return Ok(ChatResponse {
                    content: "final reply after decline".to_string(),
                    model: request.model,
                    usage: Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    stop_reason: StopReason::EndTurn,
                    tool_calls: Vec::new(),
                });
            }
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
                    name: "bash_exec".to_string(),
                    input: serde_json::json!({"command": self.command}),
                }],
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec!["scripted-bash-fake-model"]
        }
    }

    /// Always fails with `AgentError::MissingApiKey`, for tests that want to
    /// exercise the missing-key rendering path without depending on ambient
    /// `ANTHROPIC_API_KEY` env state (which is process-global and shared
    /// across concurrently-running tests).
    pub(crate) struct MissingKeyProvider;

    #[async_trait]
    impl LlmProvider for MissingKeyProvider {
        fn name(&self) -> &str {
            "missing-key-fake"
        }
        fn default_model(&self) -> &str {
            "missing-key-fake-model"
        }
        async fn chat(&self, _request: ChatRequest) -> AgentResult<ChatResponse> {
            Err(AgentError::MissingApiKey {
                provider: "missing-key-fake".to_string(),
            })
        }
        fn supported_models(&self) -> Vec<&str> {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        EchoProvider, MissingKeyProvider, ScriptedBashExecProvider, ScriptedToolProvider,
    };
    use super::*;
    use crate::error::AgentError;

    #[tokio::test]
    async fn test_turn_sends_message_and_returns_provider_reply() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        let tools = ReplToolRegistry::new();
        let ctx = ReplContext::default();
        let reply = chat
            .turn("hello", &tools, &ctx)
            .await
            .expect("fake provider succeeds");
        assert_eq!(reply, "echo:hello");
    }

    #[tokio::test]
    async fn test_turn_accumulates_history_across_calls() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        let tools = ReplToolRegistry::new();
        let ctx = ReplContext::default();
        chat.turn("first", &tools, &ctx)
            .await
            .expect("first turn succeeds");
        assert_eq!(chat.history_len(), 2); // user + assistant
        chat.turn("second", &tools, &ctx)
            .await
            .expect("second turn succeeds");
        assert_eq!(chat.history_len(), 4);
    }

    #[tokio::test]
    async fn test_clear_resets_history_to_empty() {
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".to_string(),
        );
        let tools = ReplToolRegistry::new();
        let ctx = ReplContext::default();
        chat.turn("hi", &tools, &ctx).await.expect("turn succeeds");
        assert_eq!(chat.history_len(), 2);
        chat.clear();
        assert_eq!(chat.history_len(), 0);
    }

    #[tokio::test]
    async fn test_turn_surfaces_missing_api_key_error_without_panicking() {
        let mut chat = ChatState::with_provider(
            Box::new(MissingKeyProvider),
            "missing-key-fake-model".to_string(),
        );
        let tools = ReplToolRegistry::new();
        let ctx = ReplContext::default();
        let result = chat.turn("hello", &tools, &ctx).await;
        assert!(matches!(result, Err(AgentError::MissingApiKey { .. })));
    }

    #[test]
    fn test_from_env_constructs_without_a_key_present() {
        // Must not panic even when ANTHROPIC_API_KEY/CLAUDE_API_KEY are
        // unset -- construction always succeeds; only `turn()` can fail.
        let _chat = ChatState::from_env();
    }

    #[test]
    fn test_tool_definitions_maps_replool_schema_to_tool_definition() {
        let registry = ReplToolRegistry::with_defaults();
        let defs = tool_definitions(&registry);
        assert_eq!(defs.len(), registry.len());
        let verify = defs
            .iter()
            .find(|d| d.name == "ion_verify")
            .expect("ion_verify tool definition present");
        assert!(!verify.description.is_empty());
        assert!(verify.input_schema.is_object());
    }

    #[test]
    fn test_tool_definitions_empty_registry_yields_empty_vec() {
        let registry = ReplToolRegistry::new();
        assert!(tool_definitions(&registry).is_empty());
    }

    #[tokio::test]
    async fn test_bash_exec_is_gated_and_a_decline_prevents_the_command_running() {
        // Regression test (Opus adversarial review of T9, finding S1): T9
        // made file_write/bash_exec reachable from raw model output for the
        // first time, with no confirmation gate -- unlike claude/codex,
        // which prompt before write/bash by default. Proves a decline is a
        // true short-circuit: the shell command must never actually run.
        let tools = ReplToolRegistry::with_defaults();
        let ctx = ReplContext::default();
        let asked: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let confirm = |name: &str, _input: &Value| {
            asked.lock().unwrap().push(name.to_string());
            false // always decline
        };
        let executor = ReplToolExecutor {
            tools: &tools,
            ctx: &ctx,
            confirm: &confirm,
        };

        let marker = std::env::temp_dir().join(format!(
            "ion-confirm-gate-test-{}-{}",
            std::process::id(),
            "bash-exec"
        ));
        let _ = std::fs::remove_file(&marker);
        let marker_display = marker.display().to_string();

        let result = executor
            .execute(
                "bash_exec",
                serde_json::json!({"command": format!("touch {marker_display}")}),
            )
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("declined"));
        assert!(
            !marker.exists(),
            "declined bash_exec must never have run the shell command"
        );
        assert_eq!(asked.lock().unwrap().as_slice(), &["bash_exec".to_string()]);
    }

    #[tokio::test]
    async fn test_read_only_tools_are_not_gated_behind_confirmation() {
        let tools = ReplToolRegistry::with_defaults();
        let ctx = ReplContext::default();
        let asked: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let confirm = |name: &str, _input: &Value| {
            asked.lock().unwrap().push(name.to_string());
            false
        };
        let executor = ReplToolExecutor {
            tools: &tools,
            ctx: &ctx,
            confirm: &confirm,
        };

        // file_read: not in CONFIRMATION_REQUIRED_TOOLS, so confirm must
        // never be invoked -- even though the path doesn't exist and the
        // call fails for an unrelated reason, that failure must not be the
        // "declined" message.
        let result = executor
            .execute(
                "file_read",
                serde_json::json!({"path": "/definitely/does/not/exist/ion-test"}),
            )
            .await;
        assert!(result.is_error);
        assert!(!result.content.contains("declined"));
        assert!(asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_turn_with_confirm_declined_does_not_execute_bash_exec() {
        // End-to-end: drives the gate through ChatState::turn (not just the
        // executor directly), proving the confirmation hook installed via
        // with_confirm is actually the one consulted mid-tool-loop.
        let marker =
            std::env::temp_dir().join(format!("ion-confirm-gate-e2e-{}-turn", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let marker_display = marker.display().to_string();

        let mut chat = ChatState::with_provider(
            Box::new(ScriptedBashExecProvider {
                command: format!("touch {marker_display}"),
            }),
            "scripted-bash-fake-model".to_string(),
        )
        .with_confirm(|_name, _input| false);

        let tools = ReplToolRegistry::with_defaults();
        let ctx = ReplContext::default();
        let reply = chat
            .turn("please touch that file", &tools, &ctx)
            .await
            .expect("tool loop resolves even when the tool call is declined");

        assert_eq!(reply, "final reply after decline");
        assert!(!marker.exists(), "declined bash_exec must never have run");
    }

    #[tokio::test]
    // See handlers::ion's test module for why holding env_lock() across
    // .await here is intentional (must span the whole gate-launcher round
    // trip) and safe (test-only std::sync::Mutex<()>, never contended by
    // production code).
    #[allow(clippy::await_holding_lock)]
    async fn test_turn_with_tools_executes_tool_call_and_returns_final_reply() {
        // Proves ChatState::turn actually drives the tool-use round trip
        // (request -> tool_use -> execute against the real ion_verify
        // ReplTool via the stub gate -> tool_result -> final reply), not
        // just a single-shot request/response.
        let _guard = crate::test_support::ion_gate_launcher_env_lock();
        let repo = init_git_repo_for_tool_test();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate.sh");
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let mut chat = ChatState::with_provider(
            Box::new(ScriptedToolProvider::new()),
            "scripted-tool-fake-model".to_string(),
        );
        let tools = ReplToolRegistry::with_defaults();
        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
        };
        let reply = chat.turn("verify my diff", &tools, &ctx).await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        let reply = reply.expect("tool loop should resolve to the scripted final reply");
        assert_eq!(reply, "final reply after tool use");
        // user, assistant(tool_use), user(tool_results), assistant(final).
        assert_eq!(chat.history_len(), 4);
    }

    fn init_git_repo_for_tool_test() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("failed to create tempdir");
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .expect("failed to run git");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["commit", "--allow-empty", "--quiet", "-m", "init"]);
        dir
    }
}
