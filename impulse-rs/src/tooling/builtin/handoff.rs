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

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if let Some(workspace) = params
            .get("payload")
            .and_then(|payload| payload.get("workspace"))
            .and_then(Value::as_str)
        {
            let path = ctx.resolve_path(workspace);
            if !ctx.is_path_allowed(&path, false) {
                return Err(ToolError::PathNotAllowed(path.display().to_string()));
            }
        }
        if !ctx.is_path_allowed(&ctx.impulse_dir, true) {
            return Err(ToolError::PathNotAllowed(
                ctx.impulse_dir.display().to_string(),
            ));
        }

        let action = params.get("action").and_then(Value::as_str).unwrap_or("");
        let payload = params.get("payload").cloned().unwrap_or(Value::Null);
        match handoff::dispatch(action, &payload, &ctx.impulse_dir, Utc::now()) {
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
}
