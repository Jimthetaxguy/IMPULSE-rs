//! Forward-looking `ReplTool` scaffolding (TUI_SPEC.md T6/section 2.3).
//!
//! Nothing implements this trait yet — T6 only ships the deterministic
//! slash-command surface in `router.rs`, with stub responses for `/verify`
//! and free-text chat rendered by `ion_repl::mod`. The trait and
//! [`ToolOutcome`] are defined now, matching the shape sketched in
//! TUI_SPEC.md section 2.3, so T7 (`ion_verify` as the first `ReplTool`) and
//! T9 (exposing `ReplTool`s as Anthropic tool-use schemas) can register real
//! tools without a rewrite of the router/session plumbing. Deliberately
//! mirrors `src/tooling::DynamicTool` so tools can be adapted from the
//! existing capability-based registry instead of inventing a third
//! abstraction (TUI_SPEC.md section 2.2).

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use super::ReplContext;

/// One callable capability inside the ion chat loop.
#[async_trait]
pub trait ReplTool: Send + Sync {
    /// Stable tool name, e.g. `"ion_verify"`.
    fn name(&self) -> &'static str;
    /// One-line usage shown by `/help` and used as the LLM tool-use description (T9).
    fn usage(&self) -> &'static str;
    /// Anthropic tool-use input schema (T9).
    fn json_schema(&self) -> Value;
    /// Executes the tool against `ctx` with the given (already-parsed) arguments.
    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome>;
}

/// Result of running a [`ReplTool`].
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// Human-readable text for the REPL transcript.
    pub rendered: String,
    /// Structured result (e.g. `HarnessResponse` for `ion_verify`).
    pub payload: Value,
    /// Whether the outcome should be treated as success (verify:
    /// `response.passed()`) — the render layer branches on this, never on
    /// `rendered` prose (spec-a invariant, TUI_SPEC.md section 2.3).
    pub ok: bool,
}
