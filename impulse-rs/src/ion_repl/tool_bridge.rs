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
//! **Seam note (Stage 1 sandbox roots,
//! `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`):**
//! `DynamicTool::required_capabilities()` / `ToolContext` model *filesystem
//! sandboxing* (`allowed_read_roots`/`allowed_write_roots`) as well as
//! capability grants; `ReplTool` has no equivalent concept of its own. T7
//! ran every adapted tool with `ToolContext::with_all_capabilities()`
//! (unrestricted roots, all capabilities granted) -- fine for a spawned
//! y/N-gated call in the moment, but once approved it left `file_write`/
//! `bash_exec` able to touch anywhere on the host with no further check.
//! `run()` now builds its `ToolContext` from `ctx.sandbox_tool_context()`
//! (`ion_repl::ReplContext`): all capabilities are still granted (`ion` is
//! a CLI-launched coding agent, matching `ToolContext::with_all_capabilities`'s
//! existing precedent), but the filesystem roots are narrowed to the
//! session's `repo_root` for writes and `repo_root` plus any `/allow`-granted
//! paths for reads. This is the *second* layer of the sandbox --
//! `ion_repl::chat::ReplToolExecutor` checks the same roots before a gated
//! call ever reaches this bridge, so a denial is visible at confirmation
//! time; this layer is what makes the boundary real even if a future
//! caller reaches a bridged tool by some other path.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::tooling::{ExecutionOrigin, ToolRegistry};

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
        let tool_ctx = ctx.sandbox_tool_context();
        debug_assert_eq!(tool_ctx.execution_origin, ExecutionOrigin::Cli);

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
// clippy: some tests here hold `impulse_home_env_lock()`/`env_lock()`
// across an `.await` (must span the whole IMPULSE_HOME-dependent async
// call so a concurrent test can't mutate the env var mid-call); see
// ion_repl::mod's identical justification for the same pattern.
#[allow(clippy::await_holding_lock)]
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

    // ------------------------------------------------------------------
    // Stage 1: sandbox roots (write limited to repo_root, read extended by
    // /allow grants) -- proves the bridge itself enforces the boundary,
    // independent of the confirmation-layer check in `ion_repl::chat`.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_file_write_inside_repo_root_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_write", "n/a");
        let ctx = ReplContext {
            repo_root: dir.path().to_path_buf(),
            ..ReplContext::default()
        };

        let target = dir.path().join("inside.txt");
        let outcome = bridge
            .run(
                serde_json::json!({"path": target.display().to_string(), "content": "hi"}),
                &ctx,
            )
            .await
            .expect("write inside repo_root should succeed");

        assert!(outcome.ok);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hi");
    }

    #[tokio::test]
    async fn run_file_write_outside_repo_root_is_refused() {
        // The core sandbox invariant: even with all capabilities granted,
        // a write outside the session's repo_root must be denied by the
        // ToolContext roots this bridge now builds, not merely by a
        // higher-layer confirmation prompt.
        let repo_root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_write", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        let target = outside.path().join("escape.txt");
        let result = bridge
            .run(
                serde_json::json!({"path": target.display().to_string(), "content": "hi"}),
                &ctx,
            )
            .await;

        assert!(result.is_err(), "write outside repo_root must be refused");
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn run_file_read_outside_repo_root_is_refused_without_an_allow_grant() {
        let repo_root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "top secret").unwrap();
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_read", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        let result = bridge
            .run(
                serde_json::json!({"path": secret.display().to_string()}),
                &ctx,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_file_read_outside_repo_root_succeeds_once_allowed() {
        // /allow extends the READ roots only -- this proves the grant
        // actually reaches the ToolContext this bridge builds.
        let repo_root = tempfile::tempdir().expect("tempdir");
        let granted = tempfile::tempdir().expect("tempdir");
        let target = granted.path().join("doc.txt");
        std::fs::write(&target, "granted content").unwrap();
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_read", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            allowed_read_roots: vec![granted.path().to_path_buf()],
        };

        let outcome = bridge
            .run(
                serde_json::json!({"path": target.display().to_string()}),
                &ctx,
            )
            .await
            .expect("read of a /allow-granted path should succeed");

        assert!(outcome.ok);
        assert!(outcome.rendered.contains("granted content"));
    }

    #[tokio::test]
    async fn run_file_write_to_an_allow_granted_path_still_refused() {
        // A read grant must never widen the write sandbox -- write stays
        // pinned to repo_root regardless of /allow.
        let repo_root = tempfile::tempdir().expect("tempdir");
        let granted = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_write", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            allowed_read_roots: vec![granted.path().to_path_buf()],
        };

        let target = granted.path().join("should-not-write.txt");
        let result = bridge
            .run(
                serde_json::json!({"path": target.display().to_string(), "content": "no"}),
                &ctx,
            )
            .await;

        assert!(result.is_err());
        assert!(!target.exists());
    }

    // ------------------------------------------------------------------
    // Review round 2, P3: an EXPLICITLY-supplied out-of-sandbox
    // `impulse_dir` must be refused for memory_search/genome_read, the
    // same way file_read's `path` is refused above. The review round 1
    // tests for these two tools called `tool.execute(...)` directly,
    // bypassing `ToolRegistry::execute`'s `validate_paths` step entirely --
    // they proved the ctx-default selection logic, not that an
    // out-of-sandbox explicit value is actually denied end to end through
    // the bridge. These go through the real path: DynamicToolBridge::run
    // -> ctx.sandbox_tool_context() -> ToolRegistry::execute ->
    // validate_paths.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_memory_search_with_an_out_of_sandbox_impulse_dir_is_refused() {
        let repo_root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "memory_search", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        let result = bridge
            .run(
                serde_json::json!({
                    "query": "auth",
                    "impulse_dir": outside.path().display().to_string()
                }),
                &ctx,
            )
            .await;

        assert!(
            result.is_err(),
            "an out-of-sandbox explicit impulse_dir must be refused"
        );
    }

    #[tokio::test]
    async fn run_genome_read_with_an_out_of_sandbox_impulse_dir_is_refused() {
        let repo_root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            outside.path().join("GENOME.md"),
            "# should not be reachable",
        )
        .unwrap();
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "genome_read", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        let result = bridge
            .run(
                serde_json::json!({"impulse_dir": outside.path().display().to_string()}),
                &ctx,
            )
            .await;

        assert!(
            result.is_err(),
            "an out-of-sandbox explicit impulse_dir must be refused"
        );
    }

    #[tokio::test]
    async fn run_memory_search_with_an_impulse_dir_inside_an_allow_grant_is_still_refused() {
        // Review round 5, P1 Codex, item 3 (behavior change from round 2):
        // an `/allow` grant no longer extends what `memory_search`'s/
        // `genome_read`'s `impulse_dir` may reach at all -- these two tools
        // validate `impulse_dir` against a CLOSED set (`ctx.impulse_dir`,
        // or an explicitly-configured `IMPULSE_HOME`), never the shared
        // `allowed_read_roots` `/allow` extends. This intentionally
        // supersedes round 2's `run_memory_search_with_an_impulse_dir_
        // inside_the_allow_grant_succeeds`, which assumed the OLD model
        // where `impulse_dir` was a `ParamType::FilePath` checked against
        // those shared roots.
        let repo_root = tempfile::tempdir().expect("tempdir");
        let granted = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "memory_search", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            allowed_read_roots: vec![granted.path().to_path_buf()],
        };

        let result = bridge
            .run(
                serde_json::json!({
                    "query": "auth",
                    "impulse_dir": granted.path().display().to_string()
                }),
                &ctx,
            )
            .await;

        assert!(
            result.is_err(),
            "an /allow grant must not widen memory_search's impulse_dir reach"
        );
    }

    // ------------------------------------------------------------------
    // Review round 5, P1 Codex, items 2/3's exact acceptance test: an
    // explicitly-configured IMPULSE_HOME outside repo_root is reachable by
    // the two memory tools (tool-scoped validation) but NOT by file_read
    // (never added to the shared allowed_read_roots) -- proving the two
    // authorization surfaces are genuinely independent, not "IMPULSE_HOME
    // happens to work for everything because it's on some shared list".
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_file_read_on_an_explicitly_configured_impulse_home_is_denied() {
        let _guard = crate::test_support::impulse_home_env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        let home_dir = tempfile::tempdir().expect("tempdir");
        let history_path = home_dir.path().join("ion_history");
        std::fs::write(&history_path, "/help\n").unwrap();
        std::env::set_var("IMPULSE_HOME", home_dir.path());

        let repo_root = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "file_read", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        let result = bridge
            .run(
                serde_json::json!({"path": history_path.display().to_string()}),
                &ctx,
            )
            .await;

        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }

        assert!(
            result.is_err(),
            "file_read must never gain access to IMPULSE_HOME just because the memory tools can \
             read it -- the shared allowed_read_roots must stay untouched"
        );
    }

    #[tokio::test]
    async fn run_genome_read_with_the_same_explicitly_configured_impulse_home_succeeds() {
        let _guard = crate::test_support::impulse_home_env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        let home_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            home_dir.path().join("GENOME.md"),
            "# from the configured IMPULSE_HOME",
        )
        .unwrap();
        std::env::set_var("IMPULSE_HOME", home_dir.path());

        let repo_root = tempfile::tempdir().expect("tempdir");
        let registry = Arc::new(ToolRegistry::with_defaults());
        let bridge = DynamicToolBridge::new(registry, "genome_read", "n/a");
        let ctx = ReplContext {
            repo_root: repo_root.path().to_path_buf(),
            ..ReplContext::default()
        };

        // Omitting `impulse_dir` proves the DEFAULT (`ctx.impulse_dir`,
        // which `sandbox_tool_context` set from IMPULSE_HOME here) already
        // resolves correctly -- the same path `file_read` was just denied
        // above, reached a completely different way.
        let outcome = bridge
            .run(serde_json::json!({}), &ctx)
            .await
            .expect("genome_read must reach the same IMPULSE_HOME file_read was denied");

        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }

        assert!(outcome.ok);
        assert!(outcome
            .rendered
            .contains("from the configured IMPULSE_HOME"));
    }
}
