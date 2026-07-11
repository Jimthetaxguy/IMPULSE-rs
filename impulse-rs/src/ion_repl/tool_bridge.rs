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

        Ok(ToolOutcome {
            rendered,
            payload: result.output,
            ok: true,
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
