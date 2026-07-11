//! Bridge from `src/tooling::DynamicTool` (the existing capability-gated
//! registry) to the REPL's `ReplTool` trait (TUI_SPEC.md T7, section 2.3:
//! "mirrors `src/tooling::Tool` so tools can be adapted from the existing
//! registry" and the "Scope clarification" -- `ion` needs real write/bash
//! capability, not just the read-only verify gate).
//!
//! This is deliberately a thin seam, not a reimplementation: `run()`
//! delegates straight to `ToolRegistry::execute`, which still enforces the
//! full `exists -> capability -> param-validate -> execute` pipeline
//! (`src/tooling/registry.rs`, `src/tooling/executor.rs`). The bridge only
//! translates between the two trait shapes (`ReplTool::run` returning
//! `ToolOutcome` vs `DynamicTool::execute` returning `ToolResult`) and
//! supplies the `ToolContext` the REPL runs its tools under.
//!
//! **Seam note:** `DynamicTool::required_capabilities()` / `ToolContext`
//! model *filesystem sandboxing* (`allowed_read_roots`/`allowed_write_roots`)
//! as well as capability grants; `ReplTool` has no equivalent concept yet.
//! For T7 the bridge runs every adapted tool with
//! `ToolContext::with_all_capabilities()` (unrestricted roots, all
//! capabilities granted) -- the same context shape already used for CLI
//! direct-invocation elsewhere in this crate (`ToolContext::with_all_capabilities`
//! doc comment: "for CLI direct invocation"). `ion` is a CLI-launched coding
//! agent, so this matches existing precedent rather than inventing new
//! sandboxing policy; scoping `ReplContext` to a narrower per-session
//! `ToolContext` (e.g. write roots under `ctx.repo_root`) is a natural T9+
//! follow-up once the REPL exposes a `/sandbox` or similar control surface.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::tooling::{ExecutionOrigin, ToolContext, ToolRegistry};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;

/// Adapts one `DynamicTool` (looked up by `tool_id` in `registry` at call
/// time) into a `ReplTool`.
pub struct DynamicToolBridge {
    registry: Arc<ToolRegistry>,
    tool_id: &'static str,
    usage: &'static str,
}

impl DynamicToolBridge {
    /// `usage` is supplied by the caller (rather than derived from the
    /// dynamic tool's `ToolDescriptor`, which owns `String`s) because
    /// `ReplTool::usage` returns `&'static str` — see `tools.rs`'s trait
    /// definition, mirrored from TUI_SPEC.md section 2.3.
    pub fn new(registry: Arc<ToolRegistry>, tool_id: &'static str, usage: &'static str) -> Self {
        Self {
            registry,
            tool_id,
            usage,
        }
    }
}

#[async_trait]
impl ReplTool for DynamicToolBridge {
    fn name(&self) -> &'static str {
        self.tool_id
    }

    fn usage(&self) -> &'static str {
        self.usage
    }

    fn json_schema(&self) -> Value {
        self.registry
            .schema_json()
            .into_iter()
            .find(|schema| schema["name"] == self.tool_id)
            .unwrap_or_else(|| serde_json::json!({"name": self.tool_id}))
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let _ = &ctx.repo_root; // reserved for future per-session sandboxing (see module doc)

        let tool_ctx = ToolContext {
            execution_origin: ExecutionOrigin::Cli,
            ..ToolContext::with_all_capabilities()
        };

        let result = self
            .registry
            .execute(self.tool_id, args, &tool_ctx)
            .await
            .with_context(|| format!("tool '{}' failed", self.tool_id))?;

        let rendered = serde_json::to_string_pretty(&result.output)
            .unwrap_or_else(|_| result.output.to_string());

        // `DynamicTool::execute` only returns `Err` for a failure to run the
        // tool at all (spawn failure, invalid params, capability denial --
        // all already mapped to an `Err` above via `?`). A tool that ran
        // successfully but produced a *logically* failing result (e.g.
        // bash_exec's command exiting non-zero) reports that via its own
        // JSON payload, not via `Err` -- so `ok` must be derived from
        // `output["success"]` when the tool's payload shape declares one,
        // rather than hardcoded `true`. Tools with no `success` field in
        // their payload (file_read, file_write) default to `true`, matching
        // this bridge's pre-existing behavior for them.
        let ok = result
            .output
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(ToolOutcome {
            rendered,
            payload: result.output,
            ok,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_delegates_to_the_underlying_dynamic_tool() {
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "system_info", "system_info -- report info");
        let ctx = ReplContext::default();

        let outcome = bridge
            .run(serde_json::json!({}), &ctx)
            .await
            .expect("system_info should succeed");

        assert!(outcome.ok);
        assert!(!outcome.rendered.is_empty());
    }

    #[tokio::test]
    async fn run_surfaces_an_error_for_an_unregistered_tool_id() {
        let registry = Arc::new(ToolRegistry::new());
        let bridge = DynamicToolBridge::new(registry, "does_not_exist", "n/a");
        let ctx = ReplContext::default();

        let result = bridge.run(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_reports_ok_false_when_the_tools_own_payload_declares_failure() {
        // Regression test (fresh Opus sweep, finding G2): DynamicToolBridge
        // used to hardcode `ok: true` regardless of the tool's own logical
        // result. bash_exec doesn't return Err on a non-zero exit -- it
        // returns Ok with `"success": false` in its JSON payload -- so a
        // failing command must surface as `ok: false` to the model, not get
        // silently reported as a success.
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "bash_exec", "bash_exec -- run a command");
        let ctx = ReplContext::default();

        let outcome = bridge
            .run(serde_json::json!({"command": "exit 1"}), &ctx)
            .await
            .expect("bash_exec running a failing command is still an Ok ToolOutcome");

        assert!(
            !outcome.ok,
            "a non-zero exit_code must report ok: false, not the previous hardcoded true"
        );
        assert_eq!(outcome.payload["success"], false);
    }

    #[tokio::test]
    async fn run_reports_ok_true_for_a_tool_with_no_success_field_in_its_payload() {
        // file_read has no "success" field in its output -- the default
        // (true) must still apply so this bridge's behavior for tools like
        // file_read/file_write is unchanged by the G2 fix.
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "system_info", "system_info -- report info");
        let ctx = ReplContext::default();

        let outcome = bridge
            .run(serde_json::json!({}), &ctx)
            .await
            .expect("system_info should succeed");

        assert!(outcome.payload.get("success").is_none());
        assert!(outcome.ok);
    }

    #[test]
    fn name_and_usage_reflect_the_configured_tool_id() {
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_read", "file_read -- read a file");
        assert_eq!(bridge.name(), "file_read");
        assert_eq!(bridge.usage(), "file_read -- read a file");
    }

    #[test]
    fn json_schema_looks_up_the_matching_dynamic_tool_schema() {
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_read", "n/a");
        let schema = bridge.json_schema();
        assert_eq!(schema["name"], "file_read");
        assert!(schema["input_schema"].is_object());
    }
}
