//! Impulse Agent — a coordinating AI that augments other agents' coding progress.
//!
//! The Impulse Agent can operate in two modes:
//! 1. **API mode**: Direct LLM API calls (Anthropic, OpenAI, Minimax) using an API key
//! 2. **Harness mode**: Delegates to a CLI harness (Claude Code, OpenCode, Codex, Gemini) via subprocess
//!
//! The agent reads context from all panes via the context lifecycle extractor,
//! detects coordination needs, and generates actionable recommendations.

pub mod coordinator;
pub mod harness;
pub mod prompts;

use std::time::Duration;

use serde::{Deserialize, Serialize};

// Re-export LLM types for backward compatibility.
use crate::context_lifecycle::types::ExtractedInsight;
use crate::error::{AgentError, AgentResult};
pub use crate::llm_backends::anthropic::{AnthropicProvider, MinimaxProvider, OpenAiProvider};
pub use crate::llm_backends::{Agent, LlmProvider};

/// Wall-clock budget for a single harness CLI subprocess call
/// (`harness::harness_query_structured`'s `tokio::process::Command::output()`).
///
/// Mirrors `tooling::builtin::bash_exec::DEFAULT_TIMEOUT_SECS` (30s) and
/// `llm_backends::DEFAULT_TOOL_LOOP_TIMEOUT` (180s) as prior art for bounding
/// subprocess/provider calls in this codebase, but set higher than
/// `bash_exec`'s default: a harness invocation (`claude --print "..."`,
/// `codex exec "..."`, etc.) round-trips through a full LLM turn plus that
/// harness's own tool use, which is plausibly much slower than an arbitrary
/// shell command. 120s was chosen as a middle ground between `bash_exec`'s
/// 30s and the REPL tool loop's 180s wall-clock budget.
pub const DEFAULT_HARNESS_TIMEOUT: Duration = Duration::from_secs(120);
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
    Codex,
    Gemini,
}

impl ImpulseHarness {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "opencode" | "open-code" => Some(Self::OpenCode),
            "codex" => Some(Self::Codex),
            "gemini" | "antigravity" => Some(Self::Gemini),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    /// Leading CLI args that put the harness into non-interactive
    /// (single-prompt, print-to-stdout) mode. The combined prompt is appended
    /// as the final positional argument by the caller.
    ///
    /// Each agent CLI has its own entry point for this:
    /// - Claude Code: `claude --print "<prompt>"`
    /// - OpenCode:    `opencode run "<prompt>"`
    /// - Codex:       `codex exec "<prompt>"`
    /// - Gemini:      `gemini -p "<prompt>"`
    pub fn invocation_args(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &["--print"],
            Self::OpenCode => &["run"],
            Self::Codex => &["exec"],
            Self::Gemini => &["-p"],
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
    /// Delegate to a CLI harness (Claude Code, OpenCode, Codex, Gemini).
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

/// Maximum number of session history turns to retain.
/// Keeps memory bounded — each turn is a (prompt, response) pair.
const MAX_SESSION_HISTORY: usize = 5;

/// Maximum length for a prompt stored in session history.
const MAX_HISTORY_PROMPT_LEN: usize = 200;

/// Maximum length for a response stored in session history.
const MAX_HISTORY_RESPONSE_LEN: usize = 500;

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a char boundary at or before max_len to avoid splitting multi-byte chars
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// The Impulse Agent — coordinates across agent panes using LLM intelligence.
pub struct ImpulseAgent {
    config: ImpulseAgentConfig,
    /// Internal Agent instance (for API mode).
    inner: Option<Agent>,
    /// Recent recommendations generated.
    recommendations: Vec<Recommendation>,
    /// Latest pane summaries from full coordination.
    pane_summaries: Vec<(String, Vec<String>)>,
    /// Session conversation history (bounded to last N turns).
    /// Each entry is a (prompt, response) pair, truncated for memory safety.
    session_history: Vec<(String, String)>,
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
            pane_summaries: Vec::new(),
            session_history: Vec::new(),
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

    /// Get latest pane summaries from the most recent coordination run.
    pub fn pane_summaries(&self) -> &[(String, Vec<String>)] {
        &self.pane_summaries
    }

    /// Get the current session history.
    pub fn session_history(&self) -> &[(String, String)] {
        &self.session_history
    }

    /// Build a context section from previous conversation turns.
    ///
    /// Returns an empty string when no history exists, so callers can
    /// cheaply skip prepending.
    fn build_history_context(&self) -> String {
        if self.session_history.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("\n## Previous Context\n");
        for (prompt, response) in &self.session_history {
            ctx.push_str(&format!(
                "Q: {}\nA: {}\n\n",
                truncate(prompt, MAX_HISTORY_PROMPT_LEN),
                truncate(response, MAX_HISTORY_RESPONSE_LEN),
            ));
        }
        ctx
    }

    /// Record a turn in session history, evicting the oldest if over capacity.
    fn record_turn(&mut self, prompt: &str, response: &str) {
        self.session_history.push((
            truncate(prompt, MAX_HISTORY_PROMPT_LEN),
            truncate(response, MAX_HISTORY_RESPONSE_LEN),
        ));
        if self.session_history.len() > MAX_SESSION_HISTORY {
            let drain_count = self.session_history.len() - MAX_SESSION_HISTORY;
            self.session_history.drain(..drain_count);
        }
    }

    /// Clear session history for an explicit reset.
    pub fn clear_session(&mut self) {
        self.session_history.clear();
    }

    /// Return the number of turns currently stored in session history.
    pub fn session_turn_count(&self) -> usize {
        self.session_history.len()
    }

    /// Run local coordination checks (no LLM needed).
    pub fn coordinate_local(&mut self, insights: &[ExtractedInsight]) -> Vec<Recommendation> {
        let recs = coordinator::run_local_coordination(insights);
        self.recommendations.extend(recs.iter().cloned());
        // Keep only last 50 recommendations
        if self.recommendations.len() > 50 {
            let drain_count = self.recommendations.len() - 50;
            self.recommendations.drain(..drain_count);
        }
        recs
    }

    /// Run full coordination: recommendations (conflicts + errors + delegations)
    /// plus pane summaries. Stores both in the agent for later retrieval.
    pub fn coordinate_full(
        &mut self,
        insights: &[ExtractedInsight],
    ) -> coordinator::CoordinationResult {
        let result = coordinator::run_full_coordination(insights);
        self.recommendations
            .extend(result.recommendations.iter().cloned());
        // Keep only last 50 recommendations
        if self.recommendations.len() > 50 {
            let drain_count = self.recommendations.len() - 50;
            self.recommendations.drain(..drain_count);
        }
        self.pane_summaries = result.pane_summaries.clone();
        result
    }

    /// Request a code review via the LLM, enriched with cross-pane context.
    ///
    /// Uses `query_with_context()` so extracted insights are prepended to
    /// the user prompt as structured context. Wired to `AgentReviewCode` IPC.
    pub async fn review_code(
        &mut self,
        pane_name: &str,
        insights_text: &[String],
        extracted_insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        let user_msg = prompts::build_review_prompt(pane_name, insights_text);
        self.query_with_context(prompts::CODE_REVIEW_SYSTEM, &user_msg, extracted_insights)
            .await
    }

    /// Request error analysis via the LLM, enriched with cross-pane context.
    ///
    /// Uses `query_with_context()` so extracted insights are prepended to
    /// the user prompt as structured context. Wired to `AgentAnalyzeError` IPC.
    pub async fn analyze_error(
        &mut self,
        pane_name: &str,
        error_text: &str,
        extracted_insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        let user_msg = prompts::build_error_prompt(pane_name, error_text);
        self.query_with_context(
            prompts::ERROR_ANALYSIS_SYSTEM,
            &user_msg,
            extracted_insights,
        )
        .await
    }

    /// Request cross-pane coordination analysis via the LLM.
    ///
    /// Uses `query_with_context()` so extracted insights are prepended to
    /// the user prompt as structured context.
    pub async fn coordinate_llm(
        &mut self,
        pane_summaries: &[(String, Vec<String>)],
        extracted_insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        let user_msg = prompts::build_coordination_prompt(pane_summaries);
        self.query_with_context(prompts::COORDINATION_SYSTEM, &user_msg, extracted_insights)
            .await
    }

    /// Request a pane summary via the LLM, enriched with cross-pane context.
    ///
    /// Uses `query_with_context()` so extracted insights are prepended to
    /// the user prompt as structured context. Wired to `AgentSummarizePane` IPC.
    pub async fn summarize_pane(
        &mut self,
        pane_name: &str,
        raw_output: &str,
        extracted_insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        let user_msg = prompts::build_summary_prompt(pane_name, raw_output);
        self.query_with_context(prompts::SUMMARIZE_SYSTEM, &user_msg, extracted_insights)
            .await
    }

    /// Run a CLI harness command and return the output (plain string).
    ///
    /// This is the legacy entry point that passes a simple string prompt.
    /// Internally wraps the prompt in a [`harness::HarnessRequest`] and
    /// routes through the structured JSON protocol.
    pub async fn harness_query(&self, prompt: &str) -> AgentResult<String> {
        let resp = self.harness_query_structured("", prompt, &[], None).await?;
        Ok(resp.content)
    }

    /// Run a CLI harness command using the structured JSON protocol.
    ///
    /// **Protocol:**
    /// 1. Serialize a [`harness::HarnessRequest`] to a temp file with separated
    ///    `system_prompt`, `user_prompt`, `context`, and `max_tokens` fields.
    /// 2. Set `IMPULSE_HARNESS_REQUEST` env var pointing to that file.
    /// 3. Invoke `<harness> --print <combined_prompt>` where `combined_prompt`
    ///    is `system_prompt + "\n\n" + user_prompt` (backward-compat fallback).
    /// 4. Parse stdout as [`harness::HarnessResponse`] JSON if possible.
    /// 5. **Fallback:** treat raw stdout as plain text content.
    ///
    /// The `--print` argument carries a combined prompt so that harnesses that
    /// do not read `IMPULSE_HARNESS_REQUEST` still get the full context as a
    /// plain string. The structured request file carries the separated fields
    /// for protocol-aware harnesses.
    pub async fn harness_query_structured(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        context: &[ExtractedInsight],
        max_tokens: Option<u32>,
    ) -> AgentResult<harness::HarnessResponse> {
        self.harness_query_structured_with_timeout(
            system_prompt,
            user_prompt,
            context,
            max_tokens,
            DEFAULT_HARNESS_TIMEOUT,
        )
        .await
    }

    /// Test/DI seam for [`harness_query_structured`](Self::harness_query_structured):
    /// same behavior with an overridable timeout, so tests can prove the
    /// timeout path fires without waiting out the full production budget.
    /// Mirrors `llm_backends::Agent::chat_with_tools_capped_timeout`'s
    /// pattern (a private `_with_timeout`/`_timeout` seam behind the public,
    /// fixed-default entry point).
    async fn harness_query_structured_with_timeout(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        context: &[ExtractedInsight],
        max_tokens: Option<u32>,
        timeout: Duration,
    ) -> AgentResult<harness::HarnessResponse> {
        let harness_kind = match &self.config.mode {
            AgentMode::Harness { harness } => *harness,
            _ => {
                return Err(AgentError::InvalidRequest(
                    "Agent not in harness mode".to_string(),
                ))
            }
        };

        // Build the structured request with separated fields
        let request = harness::HarnessRequest {
            system_prompt: system_prompt.to_string(),
            user_prompt: user_prompt.to_string(),
            context: context.to_vec(),
            max_tokens,
        };

        // Write request to temp file (persists until _request_file is dropped)
        let _request_file = harness::write_request_file(&request).map_err(|e| {
            AgentError::ApiRequest(format!("Failed to write harness request file: {e}"))
        })?;

        // Combine system + user prompt into a single positional argument so
        // harnesses that ignore IMPULSE_HARNESS_REQUEST still receive the full
        // context. The leading args (e.g. `--print`, `run`, `exec`) are
        // per-harness; the prompt is always the trailing positional.
        let print_arg = if system_prompt.is_empty() {
            user_prompt.to_string()
        } else {
            format!("{}\n\n{}", system_prompt, user_prompt)
        };

        let mut cmd = tokio::process::Command::new(harness_kind.command());
        cmd.args(harness_kind.invocation_args()).arg(&print_arg);

        // Set env var pointing to the structured request file
        cmd.env(
            "IMPULSE_HARNESS_REQUEST",
            _request_file.path().to_string_lossy().as_ref(),
        );

        // If the timeout below fires, the `cmd.output()` future (and the
        // `Child` it owns) is dropped. Without `kill_on_drop`, that would
        // leave the harness CLI running as an orphaned process instead of
        // being killed -- the same bug class already fixed in
        // `tooling::builtin::bash_exec::BashExecTool::execute` and
        // `impulse_ion::pi_adapter`'s watchdog. `kill_on_drop(true)` makes a
        // dropped future send SIGKILL to the child instead.
        cmd.kill_on_drop(true);

        // `kill_on_drop`/SIGKILL only reaches the *direct* child. Harness
        // CLIs are typically native binaries (exec-replaced directly), but a
        // harness that's itself a wrapper script forks a grandchild (e.g.
        // `sh script.sh` running `sleep`/the real work as a child of `sh`,
        // rather than exec-replacing it the way `sh -c "cmd"` does for a
        // single simple command) -- SIGKILL-ing just `sh` leaves that
        // grandchild running, orphaned. Regression test found this exact gap
        // (`test_harness_query_kills_hung_child_instead_of_orphaning`).
        // Fixed the same way `impulse_ion::pi_adapter`'s watchdog already
        // does: put the child in its own process group at spawn
        // (`process_group(0)`, pgid == the child's own pid) so a timeout can
        // kill the whole group, not just the direct child.
        #[cfg(unix)]
        {
            // tokio::process::Command exposes `process_group` natively
            // (unlike `pi_adapter.rs`'s std::process::Command, which needs
            // the `std::os::unix::process::CommandExt` trait in scope).
            cmd.process_group(0);
        }

        // Previously this call had no timeout at all: a hung harness CLI
        // (network stall, waiting on stdin, an unattended auth prompt) meant
        // `.output().await` never returned. Combined with the daemon holding
        // the `cached_agent` mutex across this same await (see
        // `daemon/handlers.rs`'s `checkout_agent`/`checkin_agent` doc), a
        // single wedged child could freeze the whole daemon's agent IPC
        // surface indefinitely. `tokio::time::timeout` bounds this call to
        // `timeout`, returning a typed `AgentError::HarnessTimedOut` instead
        // of hanging.
        let child = cmd.spawn().map_err(|e| {
            AgentError::ApiRequest(format!("Failed to spawn {}: {}", harness_kind.command(), e))
        })?;
        // Captured before `child` is consumed by `wait_with_output()`, so a
        // timeout can still target the process group after the future
        // (and the `Child` it owns) is dropped.
        let pgid = child.id();

        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(result) => result.map_err(|e| {
                AgentError::ApiRequest(format!("Failed to run {}: {}", harness_kind.command(), e))
            })?,
            Err(_elapsed) => {
                // `kill_on_drop` already SIGKILLed the direct child as the
                // dropped future's `Child` handle goes out of scope; this
                // explicit group-kill catches any grandchild it forked (see
                // the process_group comment above). Best-effort: shells out
                // to `kill` rather than pulling in `libc` for one syscall,
                // matching `pi_adapter::kill_process_group`'s precedent.
                #[cfg(unix)]
                if let Some(pgid) = pgid {
                    let _ = tokio::process::Command::new("kill")
                        .arg("-KILL")
                        .arg("--")
                        .arg(format!("-{pgid}"))
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await;
                }
                return Err(AgentError::HarnessTimedOut {
                    command: harness_kind.command().to_string(),
                    seconds: timeout.as_secs(),
                });
            }
        };

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(harness::HarnessResponse::parse_or_plain(&stdout))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(AgentError::ApiResponse(format!(
                "{} exited with {}: {}",
                harness_kind.command(),
                output.status,
                stderr
            )))
        }
    }

    /// Query the agent with additional context from the context lifecycle.
    ///
    /// In **harness mode**, the insights are passed both as structured data in
    /// the [`harness::HarnessRequest`] `context` field (for protocol-aware
    /// harnesses) *and* as a text block in the `--print` argument (for
    /// backward compatibility with plain-text harnesses).
    ///
    /// In **API mode**, builds a text context block from insights and prepends
    /// it to the user prompt.
    pub async fn query_with_context(
        &mut self,
        system_prompt: &str,
        user_prompt: &str,
        insights: &[ExtractedInsight],
    ) -> AgentResult<String> {
        // Build text-enriched prompt (used for both API and --print fallback)
        let context_block = prompts::build_context_prompt(insights);
        let enriched_prompt = if context_block.is_empty() {
            user_prompt.to_string()
        } else {
            format!("{context_block}\n\n{user_prompt}")
        };

        match &self.config.mode {
            AgentMode::Harness { .. } => {
                // Prepend session history for conversational continuity
                let history_ctx = self.build_history_context();
                let final_prompt = if history_ctx.is_empty() {
                    enriched_prompt
                } else {
                    format!("{history_ctx}\n{enriched_prompt}")
                };

                // Pass structured insights + system prompt through the protocol
                let resp = self
                    .harness_query_structured(
                        system_prompt,
                        &final_prompt,
                        insights,
                        Some(self.config.max_tokens),
                    )
                    .await?;

                self.record_turn(user_prompt, &resp.content);
                Ok(resp.content)
            }
            _ => {
                // API and Disabled modes: text-only enrichment via query()
                self.query(system_prompt, &enriched_prompt).await
            }
        }
    }

    /// Generic query: routes to API or harness depending on mode.
    ///
    /// Automatically prepends session history context and records
    /// the (prompt, response) turn for future queries.
    ///
    /// In harness mode, passes the system prompt as a structured field in the
    /// [`harness::HarnessRequest`] and as a combined `--print` argument for
    /// backward compatibility.
    pub async fn query(&mut self, system_prompt: &str, user_prompt: &str) -> AgentResult<String> {
        // Prepend session history so the LLM has conversational continuity
        let history_ctx = self.build_history_context();
        let enriched_prompt = if history_ctx.is_empty() {
            user_prompt.to_string()
        } else {
            format!("{history_ctx}\n{user_prompt}")
        };

        let result = match &self.config.mode {
            AgentMode::Api { .. } => {
                let agent = self.inner.as_mut().ok_or_else(|| {
                    AgentError::InvalidRequest("Agent not initialized".to_string())
                })?;
                agent.system_prompt = Some(system_prompt.to_string());
                agent.chat(&enriched_prompt).await
            }
            AgentMode::Harness { .. } => {
                // Route through structured protocol with system prompt separated
                let resp = self
                    .harness_query_structured(
                        system_prompt,
                        &enriched_prompt,
                        &[],
                        Some(self.config.max_tokens),
                    )
                    .await?;
                Ok(resp.content)
            }
            AgentMode::Disabled => Err(AgentError::InvalidRequest("Agent is disabled".to_string())),
        };

        // On success, record the turn (using original user_prompt, not enriched)
        if let Ok(ref response) = result {
            self.record_turn(user_prompt, response);
        }

        result
    }

    /// Clear conversation history (API mode) and session history.
    pub fn clear_history(&mut self) {
        if let Some(agent) = &mut self.inner {
            agent.clear_history();
        }
        self.session_history.clear();
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
        return match ImpulseAgent::new(config) {
            Ok(agent) => Some(agent),
            Err(err) => {
                tracing::error!(
                    "failed to create harness agent for harness '{}': {}",
                    h.as_str(),
                    err
                );
                None
            }
        };
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
        return match ImpulseAgent::new(config) {
            Ok(agent) => Some(agent),
            Err(err) => {
                tracing::error!(
                    "failed to create API agent for provider '{}': {}",
                    p.as_str(),
                    err
                );
                None
            }
        };
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
        assert_eq!(ImpulseHarness::parse("codex"), Some(ImpulseHarness::Codex));
        assert_eq!(
            ImpulseHarness::parse("gemini"),
            Some(ImpulseHarness::Gemini)
        );
        assert_eq!(
            ImpulseHarness::parse("antigravity"),
            Some(ImpulseHarness::Gemini)
        );
        assert_eq!(ImpulseHarness::parse("invalid"), None);
    }

    #[test]
    fn test_impulse_harness_invocation_args() {
        // Each harness has its own non-interactive entry point; the prompt is
        // appended as the trailing positional by the caller.
        assert_eq!(ImpulseHarness::ClaudeCode.invocation_args(), &["--print"]);
        assert_eq!(ImpulseHarness::OpenCode.invocation_args(), &["run"]);
        assert_eq!(ImpulseHarness::Codex.invocation_args(), &["exec"]);
        assert_eq!(ImpulseHarness::Gemini.invocation_args(), &["-p"]);
        // command() returns the binary name for `which` lookups.
        assert_eq!(ImpulseHarness::Codex.command(), "codex");
        assert_eq!(ImpulseHarness::Codex.as_str(), "codex");
        assert_eq!(ImpulseHarness::Gemini.command(), "gemini");
        assert_eq!(ImpulseHarness::Gemini.as_str(), "gemini");
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
        // With env override — all providers return the override value.
        std::env::set_var("IMPULSE_MODEL", "custom-model");
        assert_eq!(ImpulseProvider::Anthropic.default_model(), "custom-model");
        assert_eq!(ImpulseProvider::OpenAi.default_model(), "custom-model");
        assert_eq!(ImpulseProvider::Minimax.default_model(), "custom-model");

        // Without env override — compiled defaults.
        std::env::remove_var("IMPULSE_MODEL");
        assert_eq!(
            ImpulseProvider::Anthropic.default_model(),
            "claude-sonnet-4-6"
        );
        assert_eq!(ImpulseProvider::OpenAi.default_model(), "gpt-4o");
        assert_eq!(ImpulseProvider::Minimax.default_model(), "abab6.5s-chat");
    }

    #[test]
    fn test_harness_command() {
        assert_eq!(ImpulseHarness::ClaudeCode.command(), "claude");
        assert_eq!(ImpulseHarness::OpenCode.command(), "opencode");
    }

    #[tokio::test]
    async fn test_query_with_context_disabled_agent_returns_error() {
        let config = ImpulseAgentConfig::default(); // Disabled
        let mut agent = ImpulseAgent::new(config).unwrap();

        let insights = vec![ExtractedInsight {
            pane_id: 1,
            agent_kind: crate::context_lifecycle::types::AgentKind::ClaudeCode,
            timestamp: chrono::Utc::now(),
            insight_type: crate::context_lifecycle::types::InsightType::FileModified,
            content: "src/main.rs".to_string(),
            intent: None,
        }];

        let result = agent
            .query_with_context("system", "user prompt", &insights)
            .await;
        assert!(result.is_err(), "disabled agent should return error");
    }

    #[tokio::test]
    async fn test_query_with_context_empty_insights_delegates_to_query() {
        // With empty insights, query_with_context should behave identically to query.
        // A disabled agent returns an error either way.
        let config = ImpulseAgentConfig::default();
        let mut agent = ImpulseAgent::new(config).unwrap();

        let result = agent.query_with_context("system", "user prompt", &[]).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_coordinate_full_stores_pane_summaries() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        assert!(
            agent.pane_summaries().is_empty(),
            "should start with no summaries"
        );

        let now = chrono::Utc::now();
        let insights = vec![
            ExtractedInsight {
                pane_id: 1,
                agent_kind: crate::context_lifecycle::types::AgentKind::ClaudeCode,
                timestamp: now,
                insight_type: crate::context_lifecycle::types::InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
            ExtractedInsight {
                pane_id: 2,
                agent_kind: crate::context_lifecycle::types::AgentKind::OpenCode,
                timestamp: now,
                insight_type: crate::context_lifecycle::types::InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
            ExtractedInsight {
                pane_id: 2,
                agent_kind: crate::context_lifecycle::types::AgentKind::OpenCode,
                timestamp: now,
                insight_type: crate::context_lifecycle::types::InsightType::ErrorEncountered,
                content: "build error".to_string(),
                intent: None,
            },
        ];

        let result = agent.coordinate_full(&insights);

        // Result should contain both recommendations and pane summaries
        assert!(
            !result.recommendations.is_empty(),
            "should have recommendations"
        );
        assert_eq!(
            result.pane_summaries.len(),
            2,
            "should have 2 pane summaries"
        );

        // Agent should store the summaries for later retrieval
        assert_eq!(agent.pane_summaries().len(), 2);
        assert_eq!(agent.pane_summaries()[0].0, "pane-1");
        assert_eq!(agent.pane_summaries()[1].0, "pane-2");

        // Agent should also accumulate recommendations
        assert!(!agent.recommendations().is_empty());
    }

    // ── Session history tests ────────────────────────────────────────

    #[test]
    fn test_session_history_grows_with_record_turn() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        assert_eq!(agent.session_turn_count(), 0);
        assert!(agent.session_history().is_empty());

        agent.record_turn("prompt 1", "response 1");
        assert_eq!(agent.session_turn_count(), 1);

        agent.record_turn("prompt 2", "response 2");
        assert_eq!(agent.session_turn_count(), 2);

        // Verify stored content
        assert_eq!(agent.session_history()[0].0, "prompt 1");
        assert_eq!(agent.session_history()[1].1, "response 2");
    }

    #[test]
    fn test_session_history_bounded_to_max() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        // Push MAX_SESSION_HISTORY + 2 turns
        for i in 0..(MAX_SESSION_HISTORY + 2) {
            agent.record_turn(&format!("prompt {i}"), &format!("response {i}"));
        }

        assert_eq!(
            agent.session_turn_count(),
            MAX_SESSION_HISTORY,
            "history should be capped at MAX_SESSION_HISTORY"
        );

        // Oldest entries should have been evicted
        let first = &agent.session_history()[0];
        assert_eq!(first.0, "prompt 2", "oldest entries should be evicted");
    }

    #[test]
    fn test_clear_session_empties_history() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        agent.record_turn("a", "b");
        agent.record_turn("c", "d");
        assert_eq!(agent.session_turn_count(), 2);

        agent.clear_session();
        assert_eq!(agent.session_turn_count(), 0);
        assert!(agent.session_history().is_empty());
    }

    #[test]
    fn test_build_history_context_empty_when_no_history() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let agent = ImpulseAgent::new(config).unwrap();

        assert!(
            agent.build_history_context().is_empty(),
            "no history should produce empty context"
        );
    }

    #[test]
    fn test_build_history_context_contains_previous_context_header() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        agent.record_turn("what files changed?", "src/main.rs was modified");
        let ctx = agent.build_history_context();

        assert!(
            ctx.contains("## Previous Context"),
            "context should include 'Previous Context' header"
        );
        assert!(
            ctx.contains("Q: what files changed?"),
            "context should include the prompt"
        );
        assert!(
            ctx.contains("A: src/main.rs was modified"),
            "context should include the response"
        );
    }

    #[test]
    fn test_session_history_truncates_long_entries() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        let long_prompt = "x".repeat(MAX_HISTORY_PROMPT_LEN + 100);
        let long_response = "y".repeat(MAX_HISTORY_RESPONSE_LEN + 100);

        agent.record_turn(&long_prompt, &long_response);

        let (stored_prompt, stored_response) = &agent.session_history()[0];
        // Truncated entries end with "..." and are shorter than the original
        assert!(
            stored_prompt.len() <= MAX_HISTORY_PROMPT_LEN + 3,
            "prompt should be truncated"
        );
        assert!(
            stored_prompt.ends_with("..."),
            "truncated prompt should end with ellipsis"
        );
        assert!(
            stored_response.len() <= MAX_HISTORY_RESPONSE_LEN + 3,
            "response should be truncated"
        );
        assert!(
            stored_response.ends_with("..."),
            "truncated response should end with ellipsis"
        );
    }

    #[test]
    fn test_truncate_respects_char_boundaries() {
        // Multibyte char: each e-acute is 2 bytes
        let s = "\u{00e9}".repeat(150); // 300 bytes, 150 chars
        let result = truncate(&s, 100);
        // Should not panic and should be valid UTF-8
        assert!(result.len() <= 103 + 3); // worst case: 100 bytes + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_session_turn_count_reflects_state() {
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key");
        let mut agent = ImpulseAgent::new(config).unwrap();

        assert_eq!(agent.session_turn_count(), 0);
        agent.record_turn("q1", "a1");
        assert_eq!(agent.session_turn_count(), 1);
        agent.record_turn("q2", "a2");
        assert_eq!(agent.session_turn_count(), 2);
        agent.clear_session();
        assert_eq!(agent.session_turn_count(), 0);
    }

    // ── Harness subprocess timeout / kill_on_drop regression tests
    //    (same-day Opus sweep: `harness_query_structured`'s `.output().await`
    //    previously had no timeout and no `kill_on_drop`) ──────────────────

    /// Serializes tests in this module that prepend a fake-binary directory
    /// to the process-global `PATH` env var. `cargo test` runs this crate's
    /// unit tests in one process across multiple threads, and no other test
    /// file in this crate mutates `PATH` (verified via
    /// `git grep 'set_var("PATH"'`), so a lock local to this module is
    /// sufficient — mirrors `tooling::builtin::bash_exec`'s
    /// `secret_env_lock` pattern for the same reason (a shared mutable
    /// process resource, not crate state).
    fn path_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// RAII guard that prepends `dir` to `PATH` for the duration of a test
    /// and restores the previous `PATH` on drop (even on panic/early
    /// return), so a fake `claude`/`codex`/etc. binary placed in `dir`
    /// shadows any real one on the developer's/CI machine without
    /// permanently mutating process state for sibling tests.
    struct PathPrependGuard {
        previous: Option<String>,
    }

    impl PathPrependGuard {
        fn new(dir: &std::path::Path) -> Self {
            let previous = std::env::var("PATH").ok();
            let new_path = match &previous {
                Some(p) => format!("{}:{}", dir.display(), p),
                None => dir.display().to_string(),
            };
            std::env::set_var("PATH", new_path);
            Self { previous }
        }
    }

    impl Drop for PathPrependGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("PATH", value),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// Writes an executable shell script named `claude` into a fresh temp
    /// directory that just sleeps, standing in for a hung harness CLI
    /// without depending on a real `claude`/`codex`/`gemini` binary being
    /// installed on the test machine.
    fn write_hanging_fake_harness_binary() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = dir.path().join("claude");
        std::fs::write(&script_path, "#!/bin/sh\nsleep 30\n").expect("write fake harness script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)
                .expect("stat fake harness script")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).expect("chmod fake harness script");
        }
        dir
    }

    #[tokio::test]
    // clippy: the lock is a test-only std::sync::Mutex<()> (never contended
    // by production code) and must span the whole `.await` call so no
    // sibling test in this module can mutate PATH mid-spawn — matches the
    // justified pattern in `bash_exec.rs`'s secret-env tests.
    #[allow(clippy::await_holding_lock)]
    async fn test_harness_query_times_out_instead_of_hanging_forever() {
        let _lock = path_env_lock();
        let fake_bin_dir = write_hanging_fake_harness_binary();
        let _path_guard = PathPrependGuard::new(fake_bin_dir.path());

        let config = ImpulseAgentConfig::harness(ImpulseHarness::ClaudeCode);
        let agent = ImpulseAgent::new(config).expect("harness agent should construct");

        let start = std::time::Instant::now();
        let result = agent
            .harness_query_structured_with_timeout(
                "system",
                "hello",
                &[],
                None,
                Duration::from_millis(300),
            )
            .await;
        let elapsed = start.elapsed();

        match result {
            Err(AgentError::HarnessTimedOut {
                command,
                seconds: _,
            }) => {
                assert_eq!(command, "claude");
            }
            other => panic!("expected HarnessTimedOut, got: {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "timeout should fire well before the fake harness's 30s sleep, took {elapsed:?}"
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_harness_query_kills_hung_child_instead_of_orphaning() {
        // Regression test for `kill_on_drop`: without it, a timed-out
        // harness subprocess kept running as an orphan after this function
        // returned. Uses a per-invocation unique sleep duration (mirrors
        // `bash_exec.rs`'s equivalent test) so a `pgrep -f` check here can't
        // be confused by an unrelated `sleep` from another concurrently
        // running test.
        //
        // Nanoseconds, not `std::process::id() % 1000` (fresh Fable review,
        // same day): `bash_exec.rs`'s equivalent test computed its marker
        // with the identical pid-based formula -- since `cargo test` runs
        // the whole crate's tests in one process, both files could produce
        // the literal same `sleep 9.NNN` command, and a concurrent run of
        // both tests could see one's `pgrep -f` catch the other's still-
        // legitimately-running sleep. Nanoseconds (matching
        // `storage::atomic_write_path`'s own PID+nanos uniqueness
        // precedent) are effectively unique per call, not just per process.
        let _lock = path_env_lock();
        let unique_duration = format!(
            "9.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
                % 1000
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = dir.path().join("claude");
        std::fs::write(
            &script_path,
            format!("#!/bin/sh\nsleep {unique_duration}\n"),
        )
        .expect("write fake harness script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)
                .expect("stat fake harness script")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).expect("chmod fake harness script");
        }
        let _path_guard = PathPrependGuard::new(dir.path());

        let config = ImpulseAgentConfig::harness(ImpulseHarness::ClaudeCode);
        let agent = ImpulseAgent::new(config).expect("harness agent should construct");

        let result = agent
            .harness_query_structured_with_timeout(
                "system",
                "hello",
                &[],
                None,
                Duration::from_millis(300),
            )
            .await;
        assert!(matches!(result, Err(AgentError::HarnessTimedOut { .. })));

        tokio::time::sleep(Duration::from_millis(200)).await;
        let check = tokio::process::Command::new("pgrep")
            .arg("-f")
            .arg(format!("sleep {unique_duration}"))
            .output()
            .await;
        if let Ok(check) = check {
            let stray = String::from_utf8_lossy(&check.stdout);
            assert!(
                stray.trim().is_empty(),
                "expected no orphaned harness process after timeout, found pids: {stray}"
            );
        }
    }
}
