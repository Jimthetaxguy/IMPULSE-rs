//! `handoff` — one workflow tool for findings handoffs and shared-artifact writes.
//!
//! Agents call this tool instead of a separate endpoint per check. The body
//! is [`crate::handoff::dispatch`]: a refusal comes back as `state: pending`
//! with `accepted: false` and the exact failures, not as a pass with a warning.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;

use crate::handoff::{self, CheckKind, GateFailure, HandoffError, NextStep, WorkflowResult};
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

pub struct HandoffTool;

#[async_trait]
impl DynamicTool for HandoffTool {
    fn id(&self) -> &str {
        "handoff"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "handoff".into(),
            name: "Handoff".into(),
            description: "Submit or inspect a findings handoff, or read and write a shared artifact. \
                One action per call: record_fetch, submit, status, read, read_shared, write_shared. \
                The result is always a state snapshot plus the allowed next step. \
                A failed check returns state pending and accepted false with the exact failures. \
                It is not accepted with a warning."
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
                    description: "Action parameters. submit: {id, from_agent, to_agent, workspace, \
                        claims:[{id, statement, subject, source}]}. source is {kind:file, path}, \
                        {kind:commit, hash}, or {kind:url, url}. record_fetch: {sender, url, status, \
                        body_digest}. read_shared: {artifact_id, reader}. write_shared: \
                        {artifact_id, writer, based_on_generation, content}."
                        .into(),
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

    async fn execute(&self, mut params: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if let Some(workspace) = params
            .get("payload")
            .and_then(|payload| payload.get("workspace"))
            .and_then(Value::as_str)
        {
            let workspace = workspace.trim();
            if workspace.is_empty() {
                return tool_result(refusal("workspace must not be empty".into())?);
            }
            let path = ctx.resolve_path(workspace);
            if !ctx.is_path_allowed(&path, false) {
                return Err(ToolError::PathNotAllowed(path.display().to_string()));
            }
            // Dispatch must use precisely the path that passed the read check.
            params["payload"]["workspace"] = Value::String(path.to_string_lossy().into_owned());
        }
        let ledger_dir = &ctx.project_impulse_dir;
        if !ctx.is_path_allowed(ledger_dir, false) || !ctx.is_path_allowed(ledger_dir, true) {
            return Err(ToolError::PathNotAllowed(ledger_dir.display().to_string()));
        }

        let action = params.get("action").and_then(Value::as_str).unwrap_or("");
        let payload = params.get("payload").cloned().unwrap_or(Value::Null);
        match handoff::dispatch(action, &payload, ledger_dir, Utc::now()) {
            Ok(result) => tool_result(result),
            Err(HandoffError::Invalid(message)) => tool_result(refusal(message)?),
            Err(error) => Err(ToolError::ExecutionFailed(error.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead, Capability::FileSystemWrite]
    }
}

fn refusal(message: String) -> Result<WorkflowResult, ToolError> {
    WorkflowResult::pending(
        serde_json::json!({"kind": "handoff_tool", "phase": "pending"}),
        vec![GateFailure {
            target: "request".into(),
            check: CheckKind::Request,
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
            project_impulse_dir: impulse_dir.clone(),
            impulse_dir,
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

    #[test]
    fn test_handoff_tool_requires_read_and_write_capabilities() {
        let caps = HandoffTool.required_capabilities();
        assert!(caps.contains(&Capability::FileSystemRead));
        assert!(caps.contains(&Capability::FileSystemWrite));
    }
    #[tokio::test]
    async fn test_handoff_tool_rejects_ledger_outside_read_roots() {
        let project = tempfile::tempdir().unwrap();
        let readable = tempfile::tempdir().unwrap();
        let ledger = project.path().join(".impulse");
        let mut ctx = context(ledger.clone());
        ctx.allowed_write_roots = vec![project.path().to_path_buf()];
        ctx.allowed_read_roots = vec![readable.path().to_path_buf()];
        let result = HandoffTool.execute(serde_json::json!({
            "action": "read_shared", "payload": {"artifact_id": "notes", "reader": "agent-a"}
        }), &ctx).await;
        assert!(matches!(result, Err(ToolError::PathNotAllowed(_))));
        assert!(!ledger.exists(), "denial must precede all ledger I/O");
    }

    #[tokio::test]
    async fn test_handoff_tool_uses_project_state_with_external_home() {
        let project = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let ledger = project.path().join(".impulse");
        let mut ctx = context(external.path().join(".impulse"));
        ctx.project_impulse_dir = ledger.clone();
        ctx.allowed_write_roots = vec![project.path().to_path_buf()];
        ctx.allowed_read_roots = ctx.allowed_write_roots.clone();
        let result = HandoffTool.execute(serde_json::json!({
            "action": "write_shared", "payload": {
                "artifact_id": "notes", "writer": "agent-a", "based_on_generation": 0, "content": "first"
            }
        }), &ctx).await.unwrap();
        assert_eq!(result.output["accepted"], true);
        assert!(ledger.join(crate::handoff::HANDOFF_GATE_FILE).is_file());
        assert!(!ctx.impulse_dir.exists());
    }

    #[tokio::test]
    async fn test_handoff_tool_rejects_padded_absolute_workspace_outside_read_roots() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut ctx = context(project.path().join(".impulse"));
        ctx.allowed_read_roots = vec![project.path().to_path_buf()];
        ctx.allowed_write_roots = ctx.allowed_read_roots.clone();
        for padding in [" ", "\n", "\t"] {
            let result = HandoffTool
                .execute(
                    serde_json::json!({
                        "action": "submit", "payload": {
                            "workspace": format!("{padding}{} ", outside.path().display())
                        }
                    }),
                    &ctx,
                )
                .await;
            assert!(matches!(result, Err(ToolError::PathNotAllowed(_))));
        }
        assert!(!ctx.project_impulse_dir.exists());
    }
    #[tokio::test]
    async fn test_handoff_tool_dispatches_the_normalized_allowed_workspace() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("source.txt"),
            "impulse-subject: refunds",
        )
        .unwrap();
        let mut ctx = context(project.path().join(".impulse"));
        ctx.allowed_read_roots = vec![project.path().to_path_buf()];
        ctx.allowed_write_roots = ctx.allowed_read_roots.clone();
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit", "payload": {
                        "id": "report", "from_agent": "a", "to_agent": "b",
                        "workspace": format!(" \n{}\t", project.path().display()),
                        "claims": [{"id": "claim", "statement": "Refunds need approval.",
                            "subject": "refunds", "source": {"kind": "file", "path": "source.txt"}}]
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["accepted"], true);
        assert_eq!(
            result.output["snapshot"]["workspace"],
            project.path().to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn test_handoff_tool_refuses_blank_workspace_before_ledger_io() {
        let project = tempfile::tempdir().unwrap();
        let ctx = context(project.path().join(".impulse"));
        let result = HandoffTool
            .execute(
                serde_json::json!({
                    "action": "submit", "payload": {"workspace": " \n\t"}
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
