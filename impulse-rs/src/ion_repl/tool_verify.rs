//! `ion_verify` as a `ReplTool` (TUI_SPEC.md T7, section 2.3).
//!
//! Thin wrapper around `handlers::ion::run_ion_verify` -- the same pure
//! function backing `ion verify` / `impulse-rs ion-verify` (TUI_SPEC.md T3):
//! `HarnessRequest::verify(..)` -> `spawn_blocking(PiAdapter::verify)` with
//! timeout (T1/T2) -> rendered via `handlers::ion::format_response_text`
//! (the same text the CLI prints, extracted so this tool doesn't duplicate
//! or shell back out to it).
//!
//! **Spec-a invariant carried over from TUI_SPEC.md section 2.3:** this tool
//! branches only on `response.passed()` -- never NLP on prose -- and the
//! underlying `HarnessRequest` stays on the closed, read-only
//! `VERIFY_CAPABILITY_ALLOWLIST` enforced by `HarnessRequest::validate()`
//! (spec-a). That write-denial-by-omission rule is scoped to *this* tool's
//! gate call; it does not apply to the other `ReplTool`s in the registry
//! (`file_write`, `bash_exec` via `tool_bridge::DynamicToolBridge`), which
//! is exactly the "two capability universes" split TUI_SPEC.md calls for.

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::handlers::ion::{format_response_text, run_ion_verify};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;

const DEFAULT_DIFF_REF: &str = "HEAD~1..HEAD";
const DEFAULT_DESCRIPTION: &str = "Verify the pending diff.";

pub struct IonVerifyTool;

#[async_trait]
impl ReplTool for IonVerifyTool {
    fn name(&self) -> &'static str {
        "ion_verify"
    }

    fn usage(&self) -> &'static str {
        "/verify [--repo PATH] [--diff-ref REF] [description...] -- run the Ion \
         verification gate (read-only, spec-a) against a diff"
    }

    fn json_schema(&self) -> Value {
        serde_json::json!({
            "name": "ion_verify",
            "description": "Run the Ion verification gate (harness #2, Pi on MiniMax) \
                against a repo diff. Read-only per the spec-a contract.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "repo": {
                        "type": "string",
                        "description": "Repo path (defaults to the REPL's launch directory)"
                    },
                    "diff_ref": {
                        "type": "string",
                        "description": "Git ref range, e.g. HEAD~1..HEAD",
                        "default": DEFAULT_DIFF_REF
                    },
                    "description": {
                        "type": "string",
                        "description": "Task description passed to the gate",
                        "default": DEFAULT_DESCRIPTION
                    }
                },
                "required": []
            }
        })
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let repo = args
            .get("repo")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                if ctx.repo_root.as_os_str().is_empty() {
                    None
                } else {
                    Some(ctx.repo_root.display().to_string())
                }
            });

        // Review round 1, P1: this gate ships the resolved repo's diff to an
        // external API (the Ion Pi gate on MiniMax) -- a model-supplied
        // `repo` argument pointing outside the session's read sandbox must
        // be refused before ever reaching run_ion_verify, the same as any
        // other path a bridged tool would check. The default (no `repo`
        // argument -- falls back to ctx.repo_root itself) always resolves
        // inside the sandbox trivially, so this check is universal rather
        // than conditioned on whether the model supplied the argument.
        if let Some(repo_str) = &repo {
            let tool_ctx = ctx.sandbox_tool_context();
            let resolved = tool_ctx.resolve_path(repo_str);
            if !tool_ctx.is_path_allowed(&resolved, false) {
                anyhow::bail!(
                    "ion_verify: repo '{repo_str}' resolves outside the session's read \
                     sandbox (repo root plus any /allow grants); this gate ships the \
                     repo's diff to an external API, so it refuses to run against an \
                     unsandboxed path"
                );
            }
        }

        let diff_ref = args
            .get("diff_ref")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_DIFF_REF)
            .to_string();
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_DESCRIPTION)
            .to_string();

        let response = run_ion_verify(repo, diff_ref, description)
            .await
            .context("ion_verify tool failed")?;

        // Mirror the CLI's exit-code logic exactly (`handlers::ion::handle_ion_verify`):
        // `passed()` alone misses `validate()`-only violations like
        // MissingCommandsRun, which can fire even when the verdict is
        // Approve with no CRITICAL finding.
        let contract_violation = response
            .validate()
            .err()
            .map(|violation| violation.to_string());
        let ok = response.passed() && contract_violation.is_none();
        let rendered = format_response_text(&response);
        let mut payload = serde_json::to_value(&response)
            .context("failed to serialize HarnessResponse to JSON")?;
        if let Value::Object(ref mut map) = payload {
            map.insert(
                "contract_violation".to_string(),
                serde_json::to_value(&contract_violation).unwrap_or(Value::Null),
            );
        }

        Ok(ToolOutcome {
            rendered,
            payload,
            ok,
        })
    }
}

#[cfg(test)]
// See handlers::ion's test module for why holding env_lock() across .await
// here is intentional (must span the whole gate-launcher round trip) and
// safe (test-only std::sync::Mutex<()>, never contended by production code).
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the process-global `ION_GATE_LAUNCHER`
    /// env var, shared with `handlers::ion` and `ion_repl` via
    /// `crate::test_support` (see that module's doc comment for why a
    /// per-file lock is insufficient).
    use crate::test_support::init_git_repo;
    use crate::test_support::ion_gate_launcher_env_lock as env_lock;

    #[test]
    fn test_name_and_usage() {
        let tool = IonVerifyTool;
        assert_eq!(tool.name(), "ion_verify");
        assert!(tool.usage().contains("/verify"));
    }

    #[test]
    fn test_json_schema_declares_repo_diff_ref_description() {
        let schema = IonVerifyTool.json_schema();
        let props = &schema["input_schema"]["properties"];
        assert!(props["repo"].is_object());
        assert!(props["diff_ref"].is_object());
        assert!(props["description"].is_object());
    }

    #[tokio::test]
    async fn run_against_stub_gate_returns_passing_outcome() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate.sh");
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            ..ReplContext::default()
        };
        let outcome = IonVerifyTool
            .run(serde_json::json!({"diff_ref": "HEAD"}), &ctx)
            .await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        let outcome = outcome.expect("run should succeed against the stub gate");
        assert!(outcome.ok);
        assert!(outcome.rendered.contains("Approve"));
        assert_eq!(outcome.payload["verdict"], "APPROVE");
    }

    #[tokio::test]
    async fn run_against_changes_requested_stub_gate_returns_not_ok() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate-changes-requested.sh");
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            ..ReplContext::default()
        };
        let outcome = IonVerifyTool
            .run(serde_json::json!({"diff_ref": "HEAD"}), &ctx)
            .await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        let outcome = outcome.expect("run should succeed even for a failing verdict");
        assert!(!outcome.ok);
        assert!(outcome.rendered.to_lowercase().contains("changesrequested"));
    }

    #[tokio::test]
    async fn run_with_empty_repo_root_falls_back_to_dot() {
        // ReplContext::default() has an empty repo_root; the tool must not
        // pass an empty string through to run_ion_verify (which would fail
        // to canonicalize) -- it should fall back to `run_ion_verify`'s own
        // "." default by omitting `repo` entirely.
        let ctx = ReplContext::default();
        let result = IonVerifyTool
            .run(serde_json::json!({"diff_ref": "HEAD"}), &ctx)
            .await;
        // No stub gate configured here, so this either fails on git
        // rev-parse (no repo at ".") or on the adapter -- either way it must
        // not panic, and it must not be an empty-path canonicalize error.
        // (Both branches are asserted so the test cannot pass vacuously.)
        match result {
            Err(err) => {
                let message = format!("{err:#}");
                assert!(
                    !message.contains("Failed to resolve repo path: \n"),
                    "must not pass an empty repo path through: {message}"
                );
            }
            Ok(outcome) => {
                assert!(
                    !outcome.rendered.is_empty(),
                    "unexpected success must still produce rendered output"
                );
            }
        }
    }

    #[tokio::test]
    async fn run_refuses_a_model_supplied_repo_outside_the_sandbox() {
        // Review round 1, P1: ion_verify ships the repo's diff to an
        // external API -- a repo argument outside the session's read
        // sandbox must be refused before run_ion_verify is ever called
        // (proven here by the absence of any git-repo-not-found style
        // message: the sandbox message must be the one that surfaces).
        let repo = init_git_repo();
        let outside = init_git_repo();
        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            ..ReplContext::default()
        };

        let outcome = IonVerifyTool
            .run(
                serde_json::json!({
                    "repo": outside.path().display().to_string(),
                    "diff_ref": "HEAD",
                }),
                &ctx,
            )
            .await;

        let err = outcome.expect_err("a repo outside the sandbox must be refused");
        let message = format!("{err:#}");
        assert!(
            message.contains("outside the session's read sandbox"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn run_accepts_a_model_supplied_repo_granted_via_allow() {
        let repo = init_git_repo();
        let granted = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate.sh");
        let _guard = env_lock();
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            allowed_read_roots: vec![granted.path().to_path_buf()],
        };
        let outcome = IonVerifyTool
            .run(
                serde_json::json!({
                    "repo": granted.path().display().to_string(),
                    "diff_ref": "HEAD",
                }),
                &ctx,
            )
            .await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        let outcome = outcome.expect("a /allow-granted repo should be accepted");
        assert!(outcome.ok);
    }

    #[tokio::test]
    async fn run_ok_is_false_when_validate_fails_even_though_passed_is_true() {
        // Regression test: an earlier version set `ok = response.passed()`
        // only, diverging from the CLI's `!passed() || validate().is_err()`
        // exit-code logic (handlers::ion::handle_ion_verify). A verdict of
        // Approve with no CRITICAL finding makes `passed()` true, but an
        // empty `commands_run` makes `validate()` fail with
        // MissingCommandsRun -- the tool must report `ok: false` in that
        // case, and must surface `contract_violation` in the payload.
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate-approve-no-commands.sh");
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            ..ReplContext::default()
        };
        let outcome = IonVerifyTool
            .run(serde_json::json!({"diff_ref": "HEAD"}), &ctx)
            .await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        let outcome = outcome.expect("run should succeed against the stub gate");
        assert!(
            !outcome.ok,
            "validate() failure must make ok false even when passed() is true"
        );
        assert!(!outcome.payload["contract_violation"].is_null());
    }
}
