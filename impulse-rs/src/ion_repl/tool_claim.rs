//! Native Ion bridge for daemon-owned governed completion claims.
//!
//! The model supplies only a summary and optional artifact ids; a summary
//! over the claim limit is spilled to the blackboard (ADR-0023). Task
//! routing comes from the launch environment; current revision, actor, Git
//! subject, and diff truth are resolved by the daemon.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use serde_json::Value;

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;

pub struct GovernedSubmitClaimTool;

#[async_trait]
impl ReplTool for GovernedSubmitClaimTool {
    fn name(&self) -> &'static str {
        "governed_submit_claim"
    }

    fn usage(&self) -> &'static str {
        "governed_submit_claim {\"summary\": \"...\", \"artifact_ids\": []} -- submit this launched Builder's completion claim"
    }

    fn json_schema(&self) -> Value {
        serde_json::json!({
            "name": "governed_submit_claim",
            "description": "Submit a completion claim for the current governed Builder task. Impulse derives actor identity and the clean Git subject.",
            "input_schema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Concise description of the completed work"
                    },
                    "artifact_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional project-local artifact identifiers"
                    }
                },
                "required": ["summary"]
            }
        })
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let socket_path = required_launch_env("IMPULSE_SOCKET_PATH")?;
        let project_id = required_launch_env("IMPULSE_PROJECT_ID")?;
        let task_id = impulse_ops::governed_task::GovernedTaskId::try_new(required_launch_env(
            "IMPULSE_GOVERNED_TASK_ID",
        )?)
        .context("invalid governed task id in launch environment")?;
        let summary = args
            .get("summary")
            .and_then(Value::as_str)
            .context("governed_submit_claim requires a string summary")?
            .to_string();
        let artifact_ids = args
            .get("artifact_ids")
            .map(|value| {
                value
                    .as_array()
                    .context("artifact_ids must be an array")?
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(str::to_string)
                            .context("artifact_ids entries must be strings")
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();

        let blackboard_dir =
            super::tool_blackboard::claim_blackboard_dir(std::path::Path::new(&socket_path), ctx);
        let client = crate::client::DaemonClient::new(PathBuf::from(socket_path));
        let current = client
            .get_governed_task(project_id.clone(), task_id.clone())
            .await?
            .context("governed task was not found by the project daemon")?;
        // ADR-0023: a summary over the claim limit is kept whole in the
        // canonical project's blackboard and submitted as a preview plus a
        // `blackboard:` artifact reference, instead of being refused by the
        // daemon's size check. Done after the daemon lookup, and undone below
        // unless the daemon records the claim, so no failed or refused
        // submission leaves a non-expiring entry behind.
        let spilled = super::tool_blackboard::spill_claim_summary(
            ctx.blackboard.as_ref(),
            &blackboard_dir,
            task_id.as_str(),
            summary,
            artifact_ids,
        )?;
        let stored_key = spilled.stored_key.clone();
        let result = submit_claim(
            &client,
            project_id,
            task_id,
            current.revision,
            spilled.summary,
            spilled.artifact_ids,
        )
        .await;
        let recorded = matches!(&result, Ok(outcome) if outcome.ok);
        if let (false, Some(key)) = (recorded, stored_key) {
            if let Err(err) = super::tool_blackboard::undo_claim_spill(&blackboard_dir, &key) {
                tracing::warn!("could not remove unrecorded claim summary {key}: {err:#}");
            }
        }
        result
    }
}

async fn submit_claim(
    client: &crate::client::DaemonClient,
    project_id: String,
    task_id: impulse_ops::governed_task::GovernedTaskId,
    expected_revision: u64,
    summary: String,
    artifact_ids: Vec<String>,
) -> Result<ToolOutcome> {
    let request = impulse_ops::governed_task::GovernedClaimRequest {
        request_id: impulse_ops::governed_task::GovernedRequestId::try_new(format!(
            "ion-claim-{}",
            uuid::Uuid::new_v4()
        ))?,
        project_id,
        task_id,
        expected_revision,
        summary,
        artifact_ids,
    };
    request.validate()?;
    match client.submit_governed_claim(request).await? {
        crate::client::GovernedProducerOutcome::Recorded(acknowledged) => {
            let payload = serde_json::to_value(&acknowledged)
                .context("failed to serialize governed claim acknowledgment")?;
            Ok(ToolOutcome {
                rendered: format!(
                    "Governed completion claim acknowledged at task revision {}.",
                    acknowledged.revision
                ),
                payload,
                ok: true,
            })
        }
        // ADR-0019 rule 13: the daemon refused to run Git in this staged
        // worktree. Nothing was recorded and retrying changes nothing, so
        // the Builder is told the reason and the remedy rather than a
        // failure it might loop on.
        crate::client::GovernedProducerOutcome::StagedConfigRefused(refusal) => {
            let payload = serde_json::to_value(&refusal)
                .context("failed to serialize governed claim refusal")?;
            Ok(ToolOutcome {
                rendered: format!(
                    "Governed completion claim refused: {}. {}.",
                    refusal.reason, refusal.remedy
                ),
                payload,
                ok: false,
            })
        }
    }
}

fn required_launch_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("{name} is missing; launch Ion through a governed Impulse pane"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_exposes_only_claim_inputs() {
        let schema = GovernedSubmitClaimTool.json_schema();
        let properties = &schema["input_schema"]["properties"];
        assert!(properties["summary"].is_object());
        assert!(properties["artifact_ids"].is_object());
        for forbidden in ["actor", "subject_revision", "commands", "verdict"] {
            assert!(properties.get(forbidden).is_none());
        }
        assert_eq!(schema["input_schema"]["additionalProperties"], false);
    }

    #[test]
    fn missing_launch_context_is_explicit() {
        let error = required_launch_env("IMPULSE_TEST_REQUIRED_MISSING_CONTEXT")
            .expect_err("test-only launch variable must be absent");
        assert!(error
            .to_string()
            .contains("launch Ion through a governed Impulse pane"));
    }
}
