//! `handoff` — one workflow tool for findings handoffs and shared-artifact writes.
//!
//! Agents call this tool instead of a separate endpoint per check. The body
//! is [`crate::handoff::dispatch`]: a refusal comes back as `state: pending`
//! with `accepted: false` and the exact failures, not as a pass with a warning.

use std::path::PathBuf;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use crate::handoff::{
    self, action_schemas, CheckKind, GateFailure, HandoffError, NextStep, WorkflowResult,
};
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

pub struct HandoffTool;

#[async_trait]
impl DynamicTool for HandoffTool {
    fn id(&self) -> &str {
        "handoff"
    }

    fn descriptor(&self) -> ToolDescriptor {
        let schemas = action_schemas();
        let schema_text = serde_json::to_string(&schemas).unwrap_or_default();
        ToolDescriptor {
            id: "handoff".into(),
            name: "Handoff".into(),
            description: "Submit or inspect a findings handoff, or read and write a shared artifact. \
                One action per call: record_fetch, submit, status, read, read_shared, write_shared. \
                The result is always a state snapshot plus the allowed next step. \
                A failed check returns state pending and accepted false with the exact failures. \
                It is not accepted with a warning. Session and origin are taken from the tool context."
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "action".into(),
                    description: "record_fetch, submit, status, read, read_shared, or write_shared"
                        .into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "payload".into(),
                    description: format!(
                        "Action parameters. The canonical per-action JSON Schema follows the marker. \
                        Do not send session_id, execution_origin, or recorded_via; those are stamped \
                        from the tool context. Drifted names such as `from` are not aliases. \
                        ACTION_SCHEMAS={schema_text}"
                    ),
                    param_type: ParamType::Json,
                    required: true,
                    default: None,
                },
            ],
        }
    }

    fn validate_params(&self, params: &Value) -> Result<(), ToolError> {
        if params.is_object() {
            Ok(())
        } else {
            Err(ToolError::InvalidParams(
                "handoff parameters must be a JSON object with `action` and `payload`".into(),
            ))
        }
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let payload = params.get("payload").cloned().unwrap_or(Value::Null);
        let workspace = match workspace_from_payload(&payload) {
            Ok(path) => path,
            Err(message) => return tool_result(refusal(Some("workspace"), message)?),
        };
        if let Some(path) = &workspace {
            if !ctx.is_path_allowed(path, false) {
                return Err(ToolError::PathNotAllowed(path.display().to_string()));
            }
        }
        // The ledger is the project's own `.impulse/`, not `IMPULSE_HOME`.
        // Reading it back (`read`, `read_shared`) requires the read root as
        // well as the write root.
        let ledger = &ctx.project_impulse_dir;
        if !ctx.is_path_allowed(ledger, false) || !ctx.is_path_allowed(ledger, true) {
            return Err(ToolError::PathNotAllowed(ledger.display().to_string()));
        }

        let action = params.get("action").and_then(Value::as_str).unwrap_or("");
        let mut admission = handoff::AdmissionContext::from_tool(
            ctx.execution_origin.as_str(),
            ctx.session_id.as_deref(),
        );
        admission.fetch_max_age = handoff::parse_fetch_max_age_secs(
            std::env::var("IMPULSE_HANDOFF_FETCH_MAX_AGE_SECS")
                .ok()
                .as_deref(),
        );
        match handoff::dispatch_for(
            action,
            &payload,
            ledger,
            Utc::now(),
            workspace.as_deref(),
            &admission,
        ) {
            Ok(result) => tool_result(result),
            Err(HandoffError::Invalid(message)) => tool_result(refusal(None, message)?),
            Err(error) => Err(ToolError::ExecutionFailed(error.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead, Capability::FileSystemWrite]
    }
}

/// Normalize `payload.workspace` once. `None` means the action has no
/// workspace field. The returned path is the one the sandbox checks and the
/// one `submit` uses for file reads and `git -C`.
fn workspace_from_payload(payload: &Value) -> Result<Option<PathBuf>, String> {
    let Some(raw) = payload.get("workspace").and_then(Value::as_str) else {
        return Ok(None);
    };
    handoff::normalize_workspace(raw)
        .map(Some)
        .map_err(|error| match error {
            HandoffError::Invalid(message) => message,
            other => other.to_string(),
        })
}

fn refusal(field: Option<&str>, message: String) -> Result<WorkflowResult, ToolError> {
    WorkflowResult::pending(
        serde_json::json!({"kind": "handoff_tool", "phase": "pending"}),
        vec![GateFailure {
            target: field.unwrap_or("request").to_string(),
            check: CheckKind::Request,
            field: field.map(str::to_string),
            fix: message,
        }],
        NextStep {
            action: "submit".into(),
            detail: "Fix the request named in failures and call handoff again.".into(),
        },
    )
    .map_err(|error| ToolError::ExecutionFailed(error.to_string()))
}

fn tool_result(workflow: WorkflowResult) -> Result<ToolResult, ToolError> {
    let state = workflow.state.as_str().to_string();
    let accepted = workflow.accepted.to_string();
    let mut result = ToolResult::json(
        serde_json::to_value(&workflow)
            .map_err(|error| ToolError::ExecutionFailed(error.to_string()))?,
    );
    result.metadata.insert("state".into(), state);
    result.metadata.insert("accepted".into(), accepted);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(impulse_dir: std::path::PathBuf) -> ToolContext {
        ToolContext {
            impulse_dir: impulse_dir.clone(),
            project_impulse_dir: impulse_dir,
            ..ToolContext::with_all_capabilities()
        }
    }

    fn rooted(root: &std::path::Path, ledger: &std::path::Path) -> ToolContext {
        ToolContext {
            impulse_dir: ledger.to_path_buf(),
            project_impulse_dir: ledger.to_path_buf(),
            allowed_read_roots: vec![root.to_path_buf()],
            allowed_write_roots: vec![root.to_path_buf()],
            ..ToolContext::with_all_capabilities()
        }
    }

    #[tokio::test]
    async fn test_handoff_tool_returns_pending_for_a_different_domain() {
        let temp = tempfile::tempdir().unwrap();
        let impulse = temp.path().join(".impulse");
        let ctx = context(impulse);
        let tool = HandoffTool;
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        tool.execute(
            serde_json::json!({
                "action": "record_fetch",
                "payload": {
                    "sender": "agent-a",
                    "url": "https://auth.example.com/docs/refunds",
                    "status": 200,
                    "body_digest": digest
                }
            }),
            &ctx,
        )
        .await
        .unwrap();

        let result = tool
            .execute(
                serde_json::json!({
                    "action": "submit",
                    "payload": {
                        "id": "refund-report",
                        "from_agent": "agent-a",
                        "to_agent": "agent-b",
                        "workspace": temp.path(),
                        "claims": [{
                            "id": "refund-cap",
                            "statement": "Refunds over $50 need a second approver.",
                            "subject": "billing.example.com",
                            "source": {
                                "kind": "url",
                                "url": "https://auth.example.com/docs/refunds"
                            }
                        }]
                    }
                }),
                &ctx,
            )
            .await
            .expect("a refusal is a workflow result, not a tool error");

        assert_eq!(result.output["accepted"], false);
        assert_eq!(result.output["state"], "pending");
        assert_eq!(result.output["failures"][0]["check"], "subject_matches");
        let fix = result.output["failures"][0]["fix"].as_str().unwrap();
        assert!(fix.contains("billing.example.com"));
        assert!(fix.contains("auth.example.com"));
        assert_eq!(result.output["next_step"]["action"], "submit");
        assert_eq!(
            result.metadata.get("accepted").map(String::as_str),
            Some("false")
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_refuses_a_padded_absolute_workspace_outside_read_roots() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let ledger = root.join(".impulse");
        std::fs::create_dir_all(&root).unwrap();
        let ctx = rooted(&root, &ledger);
        let tool = HandoffTool;
        for padded in [" /etc", "\n/etc", " /etc "] {
            let error = tool
                .execute(
                    serde_json::json!({
                        "action": "submit",
                        "payload": {
                            "id": "escape",
                            "from_agent": "agent-a",
                            "to_agent": "agent-b",
                            "workspace": padded,
                            "claims": [{
                                "id": "host-file",
                                "statement": "The host passwd file is the source.",
                                "subject": "billing.example.com",
                                "source": {"kind": "file", "path": "passwd"}
                            }]
                        }
                    }),
                    &ctx,
                )
                .await
                .expect_err("a padded absolute workspace must not pass the read sandbox");
            match error {
                ToolError::PathNotAllowed(path) => {
                    let canonical = std::fs::canonicalize("/etc").unwrap();
                    assert_eq!(
                        std::path::PathBuf::from(&path),
                        canonical,
                        "sandbox must see the trimmed workspace, not {padded:?}"
                    );
                    assert!(!path.contains("passwd"));
                }
                other => panic!("expected PathNotAllowed, got {other}"),
            }
        }
        assert!(
            !ledger.join("HANDOFF_GATE.json").exists(),
            "a refused workspace must not write a ledger"
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_refuses_a_workspace_path_that_contains_parent_segments() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join("billing.md"),
            "impulse-subject: billing.example.com\nINSIDE-MARKER\n",
        )
        .unwrap();
        let ledger = root.join(".impulse");
        let ctx = rooted(&root, &ledger);
        let workspace = format!("{}/sub/..", root.display());
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit",
                    "payload": {
                        "id": "refund-report",
                        "from_agent": "agent-a",
                        "to_agent": "agent-b",
                        "workspace": workspace,
                        "claims": [{
                            "id": "refund-cap",
                            "statement": "Refunds over $50 need a second approver.",
                            "subject": "billing.example.com",
                            "source": {"kind": "file", "path": "billing.md"}
                        }]
                    }
                }),
                &ctx,
            )
            .await
            .expect("a `..` workspace is a pending refusal, not a sandbox error");
        assert_eq!(result.output["accepted"], false);
        assert_eq!(result.output["state"], "pending");
        let fix = result.output["failures"][0]["fix"].as_str().unwrap();
        assert!(fix.contains(".."), "the refusal must name `..`, got {fix}");
        let rendered = result.output.to_string();
        assert!(
            !rendered.contains("INSIDE-MARKER"),
            "a `..` workspace must be rejected before the claim file is read"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_handoff_tool_refuses_a_file_claim_that_symlinks_outside_the_workspace() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("secret.md"),
            "impulse-subject: billing.example.com\nOUTSIDE-SECRET-MARKER\n",
        )
        .unwrap();
        symlink(outside.join("secret.md"), root.join("secret.md")).unwrap();
        let ledger = root.join(".impulse");
        let ctx = rooted(&root, &ledger);
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit",
                    "payload": {
                        "id": "refund-report",
                        "from_agent": "agent-a",
                        "to_agent": "agent-b",
                        "workspace": root,
                        "claims": [{
                            "id": "refund-cap",
                            "statement": "Refunds over $50 need a second approver.",
                            "subject": "billing.example.com",
                            "source": {"kind": "file", "path": "secret.md"}
                        }]
                    }
                }),
                &ctx,
            )
            .await
            .expect("a symlink escape is a pending refusal");
        assert_eq!(result.output["accepted"], false);
        assert_eq!(result.output["state"], "pending");
        assert_eq!(result.output["failures"][0]["check"], "source_resolves");
        let fix = result.output["failures"][0]["fix"].as_str().unwrap();
        assert!(
            fix.contains("outside the workspace"),
            "the refusal must say the symlink left the workspace, got {fix}"
        );
        assert!(
            !result.output.to_string().contains("OUTSIDE-SECRET-MARKER"),
            "the outside file's contents must not be returned"
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_refuses_a_ledger_dir_outside_the_read_roots() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let ledger = repo.join(".impulse");
        std::fs::create_dir_all(&ledger).unwrap();
        let readable = temp.path().join("readable");
        std::fs::create_dir_all(&readable).unwrap();
        let ctx = ToolContext {
            impulse_dir: ledger.clone(),
            project_impulse_dir: ledger.clone(),
            allowed_read_roots: vec![readable],
            allowed_write_roots: vec![repo],
            ..ToolContext::with_all_capabilities()
        };
        let error = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "read",
                    "payload": {"id": "refund-report"}
                }),
                &ctx,
            )
            .await
            .expect_err("a write-only ledger directory must not be readable");
        match error {
            ToolError::PathNotAllowed(path) => {
                assert!(
                    path.contains(".impulse"),
                    "the refusal must name the ledger directory, got {path}"
                );
            }
            other => panic!("expected PathNotAllowed, got {other}"),
        }
    }

    #[tokio::test]
    async fn test_handoff_tool_uses_project_impulse_dir_when_impulse_dir_is_outside_the_repo() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let project_impulse = repo.join(".impulse");
        std::fs::create_dir_all(&repo).unwrap();
        let external = temp.path().join("impulse-home");
        std::fs::create_dir_all(&external).unwrap();
        let ctx = ToolContext {
            impulse_dir: external.clone(),
            project_impulse_dir: project_impulse.clone(),
            allowed_read_roots: vec![repo.clone()],
            allowed_write_roots: vec![repo],
            ..ToolContext::with_all_capabilities()
        };
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "record_fetch",
                    "payload": {
                        "sender": "agent-a",
                        "url": "https://billing.example.com/docs/refunds",
                        "status": 200,
                        "body_digest": digest
                    }
                }),
                &ctx,
            )
            .await
            .expect("project-local ledger is inside the read and write roots");
        assert_eq!(result.output["state"], "recorded");
        assert_eq!(result.output["accepted"], false);
        assert!(
            project_impulse.join("HANDOFF_GATE.json").is_file(),
            "the ledger must be written under the project .impulse directory"
        );
        assert!(
            !external.join("HANDOFF_GATE.json").exists(),
            "IMPULSE_HOME must not receive the handoff ledger"
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_stamps_context_and_ignores_payload_identity() {
        let temp = tempfile::tempdir().unwrap();
        let impulse = temp.path().join(".impulse");
        let mut ctx = context(impulse.clone());
        ctx.session_id = Some("from-context".into());
        ctx.execution_origin = ExecutionOrigin::Mcp;
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "record_fetch",
                    "payload": {
                        "sender": "agent-a",
                        "url": "https://billing.example.com/docs/refunds",
                        "status": 200,
                        "body_digest": digest,
                        "session_id": "from-payload",
                        "execution_origin": "cli"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["state"], "recorded");
        assert_eq!(
            result.output["snapshot"]["recorded_via"]["session_id"],
            "from-context"
        );
        assert_eq!(
            result.output["snapshot"]["recorded_via"]["execution_origin"],
            "mcp"
        );
        let ledger = std::fs::read_to_string(impulse.join("HANDOFF_GATE.json")).unwrap();
        assert!(ledger.contains("from-context"));
        assert!(!ledger.contains("from-payload"));
    }

    #[test]
    fn test_handoff_tool_requires_read_and_write_capabilities() {
        let caps = HandoffTool.required_capabilities();
        assert!(caps.contains(&Capability::FileSystemRead));
        assert!(caps.contains(&Capability::FileSystemWrite));
    }

    #[tokio::test]
    async fn test_handoff_tool_dispatches_the_normalized_allowed_workspace() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("source.txt"),
            "impulse-subject: refunds\n",
        )
        .unwrap();
        let mut ctx = context(project.path().join(".impulse"));
        ctx.allowed_read_roots = vec![project.path().to_path_buf()];
        ctx.allowed_write_roots = ctx.allowed_read_roots.clone();
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit",
                    "payload": {
                        "id": "report",
                        "from_agent": "a",
                        "to_agent": "b",
                        "workspace": format!(" \n{}\t", project.path().display()),
                        "claims": [{
                            "id": "claim",
                            "statement": "Refunds need approval.",
                            "subject": "refunds",
                            "source": {"kind": "file", "path": "source.txt"}
                        }]
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["accepted"], true);
        let canonical = project.path().canonicalize().unwrap();
        assert_eq!(
            result.output["snapshot"]["workspace"],
            canonical.to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_refuses_blank_workspace_before_ledger_io() {
        let project = tempfile::tempdir().unwrap();
        let ctx = context(project.path().join(".impulse"));
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit",
                    "payload": {"workspace": " \n\t"}
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["accepted"], false);
        assert_eq!(result.output["state"], "pending");
        assert!(result.output["failures"][0]["fix"]
            .as_str()
            .unwrap()
            .contains("workspace"));
        assert!(!ctx.project_impulse_dir.exists());
    }
}
