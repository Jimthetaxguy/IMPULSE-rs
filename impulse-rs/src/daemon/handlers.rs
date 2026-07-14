//! Daemon request handlers — dispatches each [`DaemonRequest`] variant to focused sub-handlers.
//!
//! Contains the main `process_request` dispatcher plus grouped handlers for session,
//! chat, tool, steward, ops, supervisor, agent, guard, plugin, and debug requests.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::RwLock;

use super::protocol::{
    request_type_name, respond_err, respond_ok, DaemonRequest, DaemonResponse, PROTOCOL_VERSION,
};
use crate::injection::{run_injection, InjectionMode, InjectionSurface};
use crate::llm_backends::{AnthropicProvider, ChatRequest, LlmProvider, Message, Role};
use crate::state::SharedState;

// ── Telemetry helpers ──��───────────────────────────────────────────────────

pub(crate) async fn load_terminal_reports(
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
) -> Vec<impulse_ops::TerminalOpsReport> {
    let project = crate::ops_workbench::project_summary(state);
    terminal_telemetry
        .write()
        .await
        .fresh_reports(&project.id, chrono::Utc::now())
}

pub(crate) async fn build_ops_snapshot(
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
) -> Result<impulse_ops::ProjectOpsSnapshot> {
    let reports = load_terminal_reports(state, terminal_telemetry).await;
    crate::ops_workbench::build_snapshot(state, &reports).await
}

// ── Supervisor helpers ─��───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ParsedSupervisorChatResponse {
    response: String,
    #[serde(default)]
    proposals: Vec<impulse_ops::SupervisorProposal>,
}

pub(crate) async fn build_supervisor_permission_state(
    state: &SharedState,
    supervisor_session_override: &Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
) -> Result<impulse_ops::SupervisorPermissionState> {
    let config = state.config_snapshot()?;
    let session_override = supervisor_session_override.read().await.clone();
    Ok(impulse_ops::SupervisorPermissionState::resolve(
        config.impulse_agent_permissions,
        session_override,
    ))
}

fn required_tool_capabilities_for_action(
    action: &impulse_ops::SupervisorAction,
) -> Vec<impulse_ops::ToolCapabilityId> {
    use impulse_ops::SupervisorAction as Action;
    use impulse_ops::ToolCapabilityId as Cap;

    match action {
        Action::InjectContext { .. }
        | Action::CleanupContext { .. }
        | Action::HandoffContext { .. } => vec![Cap::FileSystemRead, Cap::FileSystemWrite],
        Action::OpenArtifactReview { .. } => vec![Cap::FileSystemRead],
        Action::SearchMemory { .. } => vec![Cap::FileSystemRead],
        _ => Vec::new(),
    }
}

fn supervisor_action_label(action: &impulse_ops::SupervisorAction) -> String {
    match action {
        impulse_ops::SupervisorAction::FocusAgent { .. } => "Focus Agent".to_string(),
        impulse_ops::SupervisorAction::SendInput { .. } => "Send Input".to_string(),
        impulse_ops::SupervisorAction::InjectContext { .. } => "Stage Injection Review".to_string(),
        impulse_ops::SupervisorAction::CleanupContext { .. } => "Create Cleanup Review".to_string(),
        impulse_ops::SupervisorAction::HandoffContext { .. } => "Create Handoff".to_string(),
        impulse_ops::SupervisorAction::OpenArtifactReview { .. } => "Open Review".to_string(),
        impulse_ops::SupervisorAction::SearchMemory { .. } => "Search Memory".to_string(),
        impulse_ops::SupervisorAction::ModifyPermissions { scope, .. } => match scope {
            impulse_ops::PermissionChangeScope::SessionOverride => "Allow This Session".to_string(),
            impulse_ops::PermissionChangeScope::PersistentDefault => "Save Default".to_string(),
        },
        impulse_ops::SupervisorAction::ClearSessionOverride { .. } => {
            "Clear Session Override".to_string()
        }
        impulse_ops::SupervisorAction::ResetBaselinePermissions { .. } => {
            "Reset Baseline".to_string()
        }
    }
}

fn hydrate_supervisor_proposal(
    proposal: &mut impulse_ops::SupervisorProposal,
    permission_state: &impulse_ops::SupervisorPermissionState,
) {
    proposal.requires_confirmation = permission_state
        .effective
        .requires_confirmation(proposal.action.permission());
    proposal.missing_actions = if permission_state
        .effective
        .allows_action(proposal.action.permission())
    {
        Vec::new()
    } else {
        vec![proposal.action.permission()]
    };
    proposal.missing_tool_capabilities = required_tool_capabilities_for_action(&proposal.action)
        .into_iter()
        .filter(|capability| {
            !permission_state
                .effective
                .allows_tool_capability(*capability)
        })
        .collect();
    if proposal.action_label.trim().is_empty() {
        proposal.action_label = supervisor_action_label(&proposal.action);
    }
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    raw.get(start..=end)
}

fn parse_supervisor_chat_response(
    raw: &str,
    permission_state: &impulse_ops::SupervisorPermissionState,
) -> impulse_ops::SupervisorChatResult {
    let parsed = serde_json::from_str::<ParsedSupervisorChatResponse>(raw)
        .ok()
        .or_else(|| extract_json_object(raw).and_then(|json| serde_json::from_str(json).ok()));

    if let Some(mut parsed) = parsed {
        for proposal in &mut parsed.proposals {
            hydrate_supervisor_proposal(proposal, permission_state);
        }
        impulse_ops::SupervisorChatResult {
            response: parsed.response,
            proposals: parsed.proposals,
            permission_state: permission_state.clone(),
        }
    } else {
        impulse_ops::SupervisorChatResult {
            response: raw.trim().to_string(),
            proposals: Vec::new(),
            permission_state: permission_state.clone(),
        }
    }
}

fn build_supervisor_prompt(
    snapshot: &impulse_ops::ProjectOpsSnapshot,
    permission_state: &impulse_ops::SupervisorPermissionState,
    prompt: &str,
    context: Option<&str>,
) -> String {
    let compact_snapshot = serde_json::json!({
        "project": {
            "id": snapshot.project.id,
            "name": snapshot.project.name,
        },
        "agents": snapshot.agents.iter().map(|agent| serde_json::json!({
            "id": agent.id,
            "label": agent.label,
            "session_id": agent.session_id,
            "status": agent.status,
            "backend_kind": agent.backend_kind,
            "context_tier": agent.context.tier,
            "current_task": agent.current_task,
            "warnings": agent.warnings,
        })).collect::<Vec<_>>(),
        "interventions": snapshot.interventions.iter().map(|item| serde_json::json!({
            "id": item.id,
            "title": item.title,
            "severity": item.severity,
            "action_kind": item.action_kind,
            "target_agent_id": item.target_agent_id,
        })).collect::<Vec<_>>(),
        "artifacts": snapshot.artifacts.iter().take(8).map(|artifact| serde_json::json!({
            "id": artifact.id,
            "kind": artifact.kind,
            "title": artifact.title,
            "status": artifact.status,
        })).collect::<Vec<_>>(),
    });

    let extra_context = context.unwrap_or("").trim();
    format!(
        "Supervisor permission state:\n{}\n\nWorkspace snapshot:\n{}\n\nOperator context:\n{}\n\nOperator request:\n{}",
        serde_json::to_string_pretty(permission_state).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string_pretty(&compact_snapshot).unwrap_or_else(|_| "{}".to_string()),
        if extra_context.is_empty() {
            "(none provided)"
        } else {
            extra_context
        },
        prompt
    )
}

struct SupervisorArtifactInput {
    project_id: String,
    agent_id: String,
    kind: String,
    title: String,
    summary: String,
    payload: serde_json::Value,
    related_files: Vec<impulse_ops::ArtifactFileRef>,
    actions: Vec<impulse_ops::ArtifactAction>,
}

fn save_supervisor_artifact(state: &SharedState, input: SupervisorArtifactInput) -> Result<String> {
    let SupervisorArtifactInput {
        project_id,
        agent_id,
        kind,
        title,
        summary,
        payload,
        related_files,
        actions,
    } = input;

    let artifact_id = impulse_ops::sanitize_id(&format!(
        "{}-{}-{}",
        kind,
        agent_id,
        chrono::Utc::now().timestamp_millis()
    ));
    let artifact = impulse_ops::ArtifactEnvelope {
        id: artifact_id.clone(),
        project_id,
        agent_id,
        session_id: None,
        kind: kind.clone(),
        schema: format!("impulse.{}.v1", kind),
        title,
        summary,
        payload,
        view_hints: vec![
            impulse_ops::ArtifactViewHint::SummaryCard,
            impulse_ops::ArtifactViewHint::Markdown,
            impulse_ops::ArtifactViewHint::RawJson,
        ],
        actions,
        status: impulse_ops::ArtifactStatus::Staged,
        created_at: impulse_ops::now_rfc3339(),
        related_files,
        metadata: serde_json::json!({
            "source": "supervisor_action",
        }),
    };
    impulse_ops::save_artifact(state.storage().base_path(), &artifact)?;
    Ok(artifact_id)
}

fn resolve_supervisor_target<'a>(
    snapshot: &'a impulse_ops::ProjectOpsSnapshot,
    agent_id: Option<&str>,
    session_id: Option<&str>,
) -> Option<&'a impulse_ops::AgentRuntime> {
    session_id
        .and_then(|target| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.session_id.as_deref() == Some(target))
        })
        .or_else(|| {
            agent_id.and_then(|target| snapshot.agents.iter().find(|agent| agent.id == target))
        })
}

fn guard_target_for_action(action: &impulse_ops::SupervisorAction) -> &'static str {
    match action {
        impulse_ops::SupervisorAction::SendInput { .. } => "bash",
        impulse_ops::SupervisorAction::InjectContext { .. }
        | impulse_ops::SupervisorAction::CleanupContext { .. }
        | impulse_ops::SupervisorAction::HandoffContext { .. }
        | impulse_ops::SupervisorAction::OpenArtifactReview { .. } => "file-write",
        _ => "any",
    }
}

fn guard_string_for_action(action: &impulse_ops::SupervisorAction) -> String {
    match action {
        impulse_ops::SupervisorAction::SendInput { content, .. } => content.clone(),
        _ => action.summary(),
    }
}

async fn run_supervisor_action(
    state: &SharedState,
    action: impulse_ops::SupervisorAction,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: &Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
) -> Result<impulse_ops::SupervisorActionResult> {
    let snapshot = build_ops_snapshot(state, terminal_telemetry).await?;
    let permission_state =
        build_supervisor_permission_state(state, supervisor_session_override).await?;
    let permission = action.permission();

    if !permission_state.effective.allows_action(permission) {
        anyhow::bail!(
            "Permission '{}' is not currently allowed for the supervisor",
            permission.as_str()
        );
    }

    let missing_caps = required_tool_capabilities_for_action(&action)
        .into_iter()
        .filter(|capability| {
            !permission_state
                .effective
                .allows_tool_capability(*capability)
        })
        .collect::<Vec<_>>();
    if !missing_caps.is_empty() {
        anyhow::bail!(
            "Missing tool capabilities: {}",
            missing_caps
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    if permission_state.effective.requires_confirmation(permission) && !action.confirmed() {
        anyhow::bail!(
            "Action '{}' requires explicit confirmation",
            permission.as_str()
        );
    }

    let config = state.config_snapshot()?;
    let guard_results = crate::guardrail::evaluate_action(
        &guard_string_for_action(&action),
        guard_target_for_action(&action),
        &config.guardrails,
    )
    .map_err(anyhow::Error::msg)?;
    if crate::guardrail::GuardEngine::has_blocking(&guard_results) {
        let reason = guard_results
            .first()
            .map(|result| result.reason.clone())
            .unwrap_or_else(|| "Blocked by guardrail rule".to_string());
        anyhow::bail!(reason);
    }

    let project = snapshot.project.clone();

    match action.clone() {
        impulse_ops::SupervisorAction::FocusAgent { .. }
        | impulse_ops::SupervisorAction::SendInput { .. }
        | impulse_ops::SupervisorAction::SearchMemory { .. } => {
            Ok(impulse_ops::SupervisorActionResult {
                status: "dispatch_local".to_string(),
                message: format!("Approved {}", action.summary()),
                local_action: Some(action),
                permission_state: Some(permission_state),
                artifact_id: None,
                payload: None,
            })
        }
        impulse_ops::SupervisorAction::OpenArtifactReview { artifact_id } => {
            let review = crate::ops_workbench::run_artifact_action(
                state.storage().base_path(),
                &project.id,
                &artifact_id,
                "review",
                &serde_json::Value::Null,
            )?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: review.message,
                local_action: None,
                permission_state: Some(permission_state),
                artifact_id: Some(artifact_id),
                payload: review.payload,
            })
        }
        impulse_ops::SupervisorAction::InjectContext {
            agent_id,
            session_id,
            query,
            ..
        } => {
            let target =
                resolve_supervisor_target(&snapshot, agent_id.as_deref(), session_id.as_deref());
            let mut query_parts = vec![query.clone()];
            if let Some(target) = target {
                query_parts.push(target.label.clone());
                if let Some(task) = target.current_task.clone() {
                    query_parts.push(task);
                }
                if !target.recent_files.is_empty() {
                    query_parts.push(target.recent_files.join(" "));
                }
            }
            let injection = run_injection(
                state.storage().base_path(),
                &config,
                InjectionSurface::DaemonChat,
                Some(InjectionMode::Review),
                &query_parts,
            );
            let artifact_id = injection.artifact_path.as_ref().and_then(|path| {
                std::path::Path::new(path).file_stem().map(|stem| {
                    impulse_ops::sanitize_id(&format!("legacy-{}", stem.to_string_lossy()))
                })
            });
            Ok(impulse_ops::SupervisorActionResult {
                status: if injection.artifact_path.is_some() {
                    "executed".to_string()
                } else {
                    "no_candidates".to_string()
                },
                message: if let Some(path) = injection.artifact_path.as_ref() {
                    format!("Injection review staged at {}", path)
                } else {
                    injection
                        .skipped_reason
                        .clone()
                        .unwrap_or_else(|| "No injection candidates found".to_string())
                },
                local_action: None,
                permission_state: Some(permission_state),
                artifact_id,
                payload: Some(serde_json::to_value(&injection)?),
            })
        }
        impulse_ops::SupervisorAction::CleanupContext {
            agent_id,
            session_id,
            goal,
            ..
        } => {
            let target =
                resolve_supervisor_target(&snapshot, agent_id.as_deref(), session_id.as_deref())
                    .ok_or_else(|| anyhow::anyhow!("Target agent not found"))?;
            let artifact_id = save_supervisor_artifact(
                state,
                SupervisorArtifactInput {
                    project_id: project.id.clone(),
                    agent_id: "impulse-supervisor".to_string(),
                    kind: "context_cleanup_review".to_string(),
                    title: format!("Cleanup Review: {}", target.label),
                    summary: format!("Reviewable cleanup context prepared for {}", target.label),
                    payload: serde_json::json!({
                        "markdown": format!(
                            "# Cleanup Review\n\n## Target\n- Agent: {}\n- Session: {}\n- Context Tier: {}\n\n## Goal\n{}\n\n## Recent Files\n{}\n\n## Current Task\n{}\n\n## Warnings\n{}\n",
                            target.label,
                            target.session_id.clone().unwrap_or_else(|| "none".to_string()),
                            target.context.tier,
                            goal.clone().unwrap_or_else(|| "Prepare a compact reviewable context bundle.".to_string()),
                            if target.recent_files.is_empty() {
                                "- (none tracked)".to_string()
                            } else {
                                target.recent_files.iter().map(|file| format!("- {}", file)).collect::<Vec<_>>().join("\n")
                            },
                            target.current_task.clone().unwrap_or_else(|| "unknown".to_string()),
                            if target.warnings.is_empty() {
                                "- (none)".to_string()
                            } else {
                                target.warnings.iter().map(|warning| format!("- {}", warning)).collect::<Vec<_>>().join("\n")
                            },
                        ),
                        "target_agent_id": target.id,
                        "target_session_id": target.session_id,
                        "goal": goal,
                    }),
                    related_files: Vec::new(),
                    actions: vec![
                        impulse_ops::ArtifactAction {
                            id: "review".to_string(),
                            label: "Review".to_string(),
                            kind: "review".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                        impulse_ops::ArtifactAction {
                            id: "apply".to_string(),
                            label: "Apply To Active Agent".to_string(),
                            kind: "apply".to_string(),
                            requires_confirmation: true,
                            params_schema: serde_json::Value::Null,
                        },
                        impulse_ops::ArtifactAction {
                            id: "acknowledge".to_string(),
                            label: "Acknowledge".to_string(),
                            kind: "acknowledge".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                    ],
                },
            )?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: format!("Cleanup review created for {}", target.label),
                local_action: None,
                permission_state: Some(permission_state),
                artifact_id: Some(artifact_id),
                payload: None,
            })
        }
        impulse_ops::SupervisorAction::HandoffContext {
            session_id,
            target_tool,
            task,
            notes,
            ..
        } => {
            let session = match session_id.as_deref() {
                Some(value) => state.get_session(value).await?,
                None => None,
            };
            let handoff_path = crate::orchestration::write_handoff(
                state.storage().base_path(),
                &target_tool,
                &task,
                notes.as_deref(),
                session.as_ref(),
            )?;
            let markdown = std::fs::read_to_string(&handoff_path).unwrap_or_default();
            let artifact_id = save_supervisor_artifact(
                state,
                SupervisorArtifactInput {
                    project_id: project.id.clone(),
                    agent_id: "impulse-supervisor".to_string(),
                    kind: "handoff_review".to_string(),
                    title: format!("Handoff: {}", target_tool),
                    summary: format!("Supervisor handoff prepared for {}", target_tool),
                    payload: serde_json::json!({
                        "markdown": markdown,
                        "target_tool": target_tool,
                        "task": task,
                        "notes": notes,
                        "source_path": handoff_path.display().to_string(),
                    }),
                    related_files: vec![impulse_ops::ArtifactFileRef {
                        path: handoff_path.display().to_string(),
                        label: Some("Generated handoff file".to_string()),
                    }],
                    actions: vec![
                        impulse_ops::ArtifactAction {
                            id: "review".to_string(),
                            label: "Review".to_string(),
                            kind: "review".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                        impulse_ops::ArtifactAction {
                            id: "open_file".to_string(),
                            label: "Open File Ref".to_string(),
                            kind: "open_file".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                        impulse_ops::ArtifactAction {
                            id: "acknowledge".to_string(),
                            label: "Acknowledge".to_string(),
                            kind: "acknowledge".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                    ],
                },
            )?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: format!("Handoff created for {}", target_tool),
                local_action: None,
                permission_state: Some(permission_state),
                artifact_id: Some(artifact_id),
                payload: Some(serde_json::json!({
                    "path": handoff_path.display().to_string(),
                })),
            })
        }
        impulse_ops::SupervisorAction::ModifyPermissions {
            scope,
            grant_actions,
            grant_tool_capabilities,
            ..
        } => {
            let mut updated_policy = match scope {
                impulse_ops::PermissionChangeScope::SessionOverride => {
                    permission_state.effective.clone()
                }
                impulse_ops::PermissionChangeScope::PersistentDefault => {
                    permission_state.baseline.clone()
                }
            };
            for permission in grant_actions {
                updated_policy.grant_action(permission);
            }
            for capability in grant_tool_capabilities {
                updated_policy.grant_tool_capability(capability);
            }
            updated_policy.normalize();

            let next_state = match scope {
                impulse_ops::PermissionChangeScope::SessionOverride => {
                    *supervisor_session_override.write().await = Some(updated_policy.clone());
                    build_supervisor_permission_state(state, supervisor_session_override).await?
                }
                impulse_ops::PermissionChangeScope::PersistentDefault => {
                    state.update_impulse_agent_permissions(updated_policy.clone())?;
                    *supervisor_session_override.write().await = None;
                    build_supervisor_permission_state(state, supervisor_session_override).await?
                }
            };
            let artifact_id = save_supervisor_artifact(
                state,
                SupervisorArtifactInput {
                    project_id: project.id.clone(),
                    agent_id: "impulse-supervisor".to_string(),
                    kind: "permission_change".to_string(),
                    title: "Supervisor Permission Change".to_string(),
                    summary: format!("Supervisor permissions updated for {:?}", scope),
                    payload: serde_json::json!({
                        "scope": scope,
                        "allowed_actions": next_state.effective.allowed_actions,
                        "allowed_tool_capabilities": next_state.effective.allowed_tool_capabilities,
                        "require_confirmation_actions": next_state.effective.require_confirmation_actions,
                    }),
                    related_files: Vec::new(),
                    actions: vec![
                        impulse_ops::ArtifactAction {
                            id: "review".to_string(),
                            label: "Review".to_string(),
                            kind: "review".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                        impulse_ops::ArtifactAction {
                            id: "acknowledge".to_string(),
                            label: "Acknowledge".to_string(),
                            kind: "acknowledge".to_string(),
                            requires_confirmation: false,
                            params_schema: serde_json::Value::Null,
                        },
                    ],
                },
            )?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: "Supervisor permissions updated".to_string(),
                local_action: None,
                permission_state: Some(next_state),
                artifact_id: Some(artifact_id),
                payload: None,
            })
        }
        impulse_ops::SupervisorAction::ClearSessionOverride { .. } => {
            *supervisor_session_override.write().await = None;
            let next_state =
                build_supervisor_permission_state(state, supervisor_session_override).await?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: "Session override cleared".to_string(),
                local_action: None,
                permission_state: Some(next_state),
                artifact_id: None,
                payload: None,
            })
        }
        impulse_ops::SupervisorAction::ResetBaselinePermissions { .. } => {
            state.update_impulse_agent_permissions(
                impulse_ops::SupervisorPermissionPolicy::default(),
            )?;
            *supervisor_session_override.write().await = None;
            let next_state =
                build_supervisor_permission_state(state, supervisor_session_override).await?;
            Ok(impulse_ops::SupervisorActionResult {
                status: "executed".to_string(),
                message: "Baseline permissions reset to defaults".to_string(),
                local_action: None,
                permission_state: Some(next_state),
                artifact_id: None,
                payload: None,
            })
        }
    }
}

// ── Main dispatcher ──────────���─────────────────────────────────────────────

pub(crate) struct ProcessRequestContext<'a> {
    pub state: SharedState,
    pub registry: &'a crate::tooling::ToolRegistry,
    pub tool_context: &'a crate::tooling::ToolContext,
    pub terminal_telemetry: &'a Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    pub supervisor_session_override:
        &'a Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
    pub conflict_resolver: &'a Arc<RwLock<crate::agent::coordinator::ConflictResolver>>,
    pub delegation_tracker: &'a Arc<RwLock<crate::delegation::DelegationTracker>>,
    pub cached_agent: &'a Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
}

#[tracing::instrument(skip_all, fields(request_type = request_type_name(&request)))]
pub(crate) async fn process_request(
    request: DaemonRequest,
    context: ProcessRequestContext<'_>,
) -> DaemonResponse {
    let ProcessRequestContext {
        state,
        registry,
        tool_context,
        terminal_telemetry,
        supervisor_session_override,
        conflict_resolver,
        delegation_tracker,
        cached_agent,
    } = context;

    // ── Boundary validation ─��───────────────────────────────────────────────
    // Validate user-supplied IDs before dispatch to catch malformed input early.
    if let DaemonRequest::EndSession { ref session_id, .. }
    | DaemonRequest::GetSession { ref session_id }
    | DaemonRequest::TrackFile { ref session_id, .. }
    | DaemonRequest::TrackTool { ref session_id, .. }
    | DaemonRequest::CheckConflict { ref session_id, .. }
    | DaemonRequest::Chat { ref session_id, .. } = request
    {
        if let Err(e) = crate::validate::validate_session_id(session_id) {
            return respond_err(e);
        }
    }
    if let DaemonRequest::DescribeTool { ref name } | DaemonRequest::InvokeTool { ref name, .. } =
        request
    {
        if let Err(e) = crate::validate::validate_tool_id(name) {
            return respond_err(e);
        }
    }
    if let DaemonRequest::InvokePlugin { ref name, .. } = request {
        if let Err(e) = crate::validate::reject_control_chars(name, "plugin_name") {
            return respond_err(e);
        }
    }
    if let DaemonRequest::CreateSession { ref name, .. } = request {
        if let Err(e) = crate::validate::reject_control_chars(name, "name") {
            return respond_err(e);
        }
    }
    if let DaemonRequest::GetArtifact { ref artifact_id }
    | DaemonRequest::RunArtifactAction {
        ref artifact_id, ..
    } = request
    {
        if let Err(e) = crate::validate::reject_control_chars(artifact_id, "artifact_id") {
            return respond_err(e);
        }
    }
    if let DaemonRequest::GetGovernedTask { ref project_id, .. }
    | DaemonRequest::ListGovernedTasks { ref project_id }
    | DaemonRequest::SubmitGovernedClaim {
        request: impulse_ops::governed_task::GovernedClaimRequest { ref project_id, .. },
    }
    | DaemonRequest::RunGovernedVerification {
        request: impulse_ops::governed_task::GovernedVerificationRequest { ref project_id, .. },
    }
    | DaemonRequest::RunGovernedSupervisorReview {
        request: impulse_ops::governed_task::GovernedSupervisorReviewRequest { ref project_id, .. },
    } = request
    {
        if let Err(error) = crate::validate::reject_control_chars(project_id, "project_id") {
            return respond_err(error);
        }
    }

    match request {
        DaemonRequest::Ping => DaemonResponse::Ok {
            result: serde_json::json!({"pong": true, "protocol_version": PROTOCOL_VERSION}),
        },
        DaemonRequest::Status => handle_status(&state).await,

        // Session group
        DaemonRequest::CreateSession { .. }
        | DaemonRequest::EndSession { .. }
        | DaemonRequest::TrackFile { .. }
        | DaemonRequest::TrackTool { .. }
        | DaemonRequest::CheckConflict { .. }
        | DaemonRequest::GetSession { .. }
        | DaemonRequest::ListSessions => handle_session_request(request, &state).await,

        // Chat
        DaemonRequest::Chat { .. } => handle_chat_request(request, &state).await,

        // Tool group
        DaemonRequest::ListTools { .. }
        | DaemonRequest::DescribeTool { .. }
        | DaemonRequest::InvokeTool { .. }
        | DaemonRequest::ToolSchema => handle_tool_request(request, registry, tool_context).await,

        // Steward group
        DaemonRequest::StewardStatus
        | DaemonRequest::StewardProposals { .. }
        | DaemonRequest::StewardMemory => handle_steward_request(request, &state).await,

        // Ops group
        DaemonRequest::GetOpsSnapshot
        | DaemonRequest::SubscribeOps { .. }
        | DaemonRequest::PublishTerminalOps { .. }
        | DaemonRequest::ListArtifacts { .. }
        | DaemonRequest::GetArtifact { .. }
        | DaemonRequest::RunArtifactAction { .. } => {
            handle_ops_request(request, &state, terminal_telemetry).await
        }

        // Daemon-owned governed task lifecycle. Disk I/O and the serialized
        // compare-and-set mutation run off Tokio's async worker threads.
        DaemonRequest::RegisterGovernedTask { .. }
        | DaemonRequest::GetGovernedTask { .. }
        | DaemonRequest::ListGovernedTasks { .. }
        | DaemonRequest::MutateGovernedTask { .. } => {
            handle_governed_task_request(request, &state).await
        }

        // Closed-loop callers trigger work but never supply derived actor,
        // subject, command-evidence, or verdict payloads.
        DaemonRequest::SubmitGovernedClaim { .. }
        | DaemonRequest::RunGovernedVerification { .. } => {
            handle_governed_producer_request(request, &state).await
        }

        // Supervisor group
        DaemonRequest::GetSupervisorPermissions
        | DaemonRequest::SupervisorChat { .. }
        | DaemonRequest::RunSupervisorAction { .. }
        | DaemonRequest::RunGovernedSupervisorReview { .. } => {
            handle_supervisor_request(
                request,
                &state,
                terminal_telemetry,
                supervisor_session_override,
                cached_agent,
            )
            .await
        }

        // Agent assist
        DaemonRequest::AgentAssist { .. } => {
            handle_agent_request(request, &state, cached_agent).await
        }

        // Agent specialized endpoints (Task 23)
        DaemonRequest::AgentReviewCode { .. }
        | DaemonRequest::AgentAnalyzeError { .. }
        | DaemonRequest::AgentSummarizePane { .. } => {
            handle_agent_specialized_request(request, &state, cached_agent).await
        }

        // Guard group
        DaemonRequest::GuardEvaluate { .. } | DaemonRequest::GuardList => {
            handle_guard_request(request, &state).await
        }

        // Debug
        DaemonRequest::DebugSnapshot => handle_debug_snapshot(&state, registry).await,

        // Plugin group
        DaemonRequest::ListPlugins | DaemonRequest::InvokePlugin { .. } => {
            handle_plugin_request(request).await
        }

        // Delegation group (Phase 1B — backed by the shared DelegationTracker)
        DaemonRequest::RegisterDelegation { .. }
        | DaemonRequest::CompleteDelegation { .. }
        | DaemonRequest::ListDelegations => {
            handle_delegation_request(request, delegation_tracker).await
        }

        // Conflict resolver group (Task 20)
        DaemonRequest::GetConflictHistory => {
            let resolver = conflict_resolver.read().await;
            let history = resolver.get_resolution_history();
            respond_ok(&history)
        }
        DaemonRequest::ClearResolvedConflicts => {
            let mut resolver = conflict_resolver.write().await;
            resolver.clear_resolved();
            respond_ok(&serde_json::json!({"cleared": true}))
        }

        // Agent pool (Phase 2B — returns sessions grouped by role)
        DaemonRequest::GetAgentPool => match state.list_sessions().await {
            Ok(sessions) => DaemonResponse::Ok {
                result: serde_json::json!({
                    "agents": sessions.iter().map(|s| {
                        serde_json::json!({
                            "id": s.id,
                            "name": s.name,
                            "status": format!("{:?}", s.status),
                            "role": s.role,
                            "parent_session_id": s.parent_session_id,
                            "delegation_id": s.delegation_id,
                            "target": s.target,
                        })
                    }).collect::<Vec<_>>(),
                }),
            },
            Err(e) => respond_err(e),
        },
    }
}

// ── Sub-handlers ────���───────────────────────────────────────────────────────

pub(crate) async fn handle_governed_task_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    // Generic lifecycle mutations and daemon-owned producer mutations share
    // one per-task serialization boundary. Without this guard, a runtime-exit
    // update can advance the revision while a verifier or Supervisor side
    // effect is in flight, causing that side effect to be repeated on retry.
    let _producer_guard = match &request {
        DaemonRequest::MutateGovernedTask { request } => {
            Some(state.acquire_governed_producer_lock(&request.task_id).await)
        }
        _ => None,
    };
    let state = state.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        match request {
            DaemonRequest::RegisterGovernedTask { registration } => {
                if registration.verification_profile.is_some() {
                    registration.validate()?;
                    let assignment = registration
                        .role_assignment
                        .as_ref()
                        .context("profiled governed task lost its Builder assignment")?;
                    let supplied_compatibility = registration
                        .role_compatibility
                        .as_ref()
                        .context("profiled governed task lost its runtime compatibility")?;
                    let platform = impulse_ops::agent_registry::AgentPlatformId::try_new(
                        registration.runtime_id.clone(),
                    )?;
                    let observed_compatibility =
                        impulse_ops::agent_registry::AgentRegistry::registry_for_runtime()?
                            .evaluate_role_compatibility(&platform, assignment)?;
                    if supplied_compatibility != &observed_compatibility {
                        anyhow::bail!(
                            "profiled governed task compatibility must equal the daemon-observed runtime capability result"
                        );
                    }
                    let observed = crate::governed_producers::observe_clean_git_subject(
                        std::path::Path::new(&registration.workspace_root),
                        None,
                    )?;
                    if registration.initial_subject_revision.as_deref() != Some(observed.as_str()) {
                        anyhow::bail!(
                            "profiled governed task initial revision must equal daemon-observed clean HEAD"
                        );
                    }
                }
                serde_json::to_value(state.register_governed_task(registration)?)
                    .context("Failed to serialize registered governed task")
            }
            DaemonRequest::GetGovernedTask {
                project_id,
                task_id,
            } => serde_json::to_value(state.get_governed_task(&project_id, &task_id)?)
                .context("Failed to serialize governed task"),
            DaemonRequest::ListGovernedTasks { project_id } => {
                serde_json::to_value(state.list_governed_tasks(&project_id)?)
                    .context("Failed to serialize governed task list")
            }
            DaemonRequest::MutateGovernedTask { request } => {
                if matches!(
                    request.mutation,
                    impulse_ops::governed_task::GovernedTaskMutation::SubmitClaim { .. }
                        | impulse_ops::governed_task::GovernedTaskMutation::RecordVerification { .. }
                        | impulse_ops::governed_task::GovernedTaskMutation::RecordSupervisorVerdict { .. }
                ) {
                    let task = state
                        .get_governed_task(&request.project_id, &request.task_id)?
                        .ok_or_else(|| {
                            anyhow::anyhow!("governed task `{}` was not found", request.task_id)
                        })?;
                    if task.verification_profile.is_some() {
                        anyhow::bail!(
                            "profiled governed tasks require daemon-owned claim, verification, and Supervisor producer requests"
                        );
                    }
                }
                serde_json::to_value(state.mutate_governed_task(request)?)
                    .context("Failed to serialize governed task mutation")
            }
            _ => anyhow::bail!("Internal routing error: not a governed task request"),
        }
    })
    .await;

    match result {
        Ok(Ok(result)) => DaemonResponse::Ok { result },
        Ok(Err(error)) => respond_err(error),
        Err(error) => respond_err(format!("Governed task worker failed: {error}")),
    }
}

fn require_current_governed_task(
    state: &SharedState,
    project_id: &str,
    task_id: &impulse_ops::governed_task::GovernedTaskId,
) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
    state
        .get_governed_task(project_id, task_id)?
        .ok_or_else(|| anyhow::anyhow!("governed task `{task_id}` was not found"))
}

async fn persist_governed_mutation(
    state: &SharedState,
    request: impulse_ops::governed_task::GovernedTaskMutationRequest,
) -> Result<impulse_ops::governed_task::GovernedTaskRun> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || state.mutate_governed_task(request))
        .await
        .context("governed producer persistence worker panicked")?
}

fn require_producer_request_state(
    state: &SharedState,
    task: &impulse_ops::governed_task::GovernedTaskRun,
    request_id: &impulse_ops::governed_task::GovernedRequestId,
    expected_revision: u64,
) -> Result<bool> {
    let replay = state.governed_producer_request_is_replay(request_id, &task.id)?;
    if replay {
        if task.revision <= expected_revision {
            anyhow::bail!(
                "governed producer replay receipt does not advance beyond expected revision {expected_revision}"
            );
        }
        return Ok(true);
    }
    if task.revision != expected_revision {
        anyhow::bail!(
            "governed task revision conflict: expected {expected_revision}, current {}",
            task.revision
        );
    }
    Ok(false)
}

fn preflight_claim(
    task: &impulse_ops::governed_task::GovernedTaskRun,
    request: &impulse_ops::governed_task::GovernedClaimRequest,
) -> Result<()> {
    request.validate()?;
    if !matches!(
        task.execution_state,
        impulse_ops::governed_task::GovernedExecutionState::Running
            | impulse_ops::governed_task::GovernedExecutionState::RuntimeExited
    ) {
        anyhow::bail!("governed claim requires a launched or exited runtime");
    }
    if !matches!(
        task.review_state,
        impulse_ops::governed_task::GovernedReviewState::AwaitingClaim
            | impulse_ops::governed_task::GovernedReviewState::ChangesRequested
            | impulse_ops::governed_task::GovernedReviewState::VerificationFailed
    ) {
        anyhow::bail!("governed task review state does not accept a worker claim");
    }
    if task.claims.len() >= impulse_ops::governed_task::MAX_GOVERNED_RECORDS_PER_KIND {
        anyhow::bail!("governed worker claim capacity is exhausted");
    }
    Ok(())
}

fn preflight_verification(task: &impulse_ops::governed_task::GovernedTaskRun) -> Result<()> {
    if task.review_state != impulse_ops::governed_task::GovernedReviewState::AwaitingVerification {
        anyhow::bail!("governed verification requires an awaiting-verification task");
    }
    if task.latest_claim().is_none() {
        anyhow::bail!("governed verification requires a current worker claim");
    }
    if task.verifications.len() >= impulse_ops::governed_task::MAX_GOVERNED_RECORDS_PER_KIND {
        anyhow::bail!("governed verification capacity is exhausted");
    }
    Ok(())
}

fn preflight_supervisor_review(task: &impulse_ops::governed_task::GovernedTaskRun) -> Result<()> {
    if !matches!(
        task.review_state,
        impulse_ops::governed_task::GovernedReviewState::AwaitingSupervisor
            | impulse_ops::governed_task::GovernedReviewState::VerificationFailed
    ) {
        anyhow::bail!("governed Supervisor review requires current verification");
    }
    if task.latest_verification().is_none() {
        anyhow::bail!("governed Supervisor review requires a verification record");
    }
    if task.supervisor_verdicts.len() >= impulse_ops::governed_task::MAX_GOVERNED_RECORDS_PER_KIND {
        anyhow::bail!("governed Supervisor verdict capacity is exhausted");
    }
    Ok(())
}

fn replay_claim_input(
    task: &impulse_ops::governed_task::GovernedTaskRun,
    request: &impulse_ops::governed_task::GovernedClaimRequest,
) -> Result<impulse_ops::governed_task::WorkerCompletionClaimInput> {
    let claim = task
        .claims
        .iter()
        .find(|claim| claim.based_on_revision == request.expected_revision)
        .context("stale governed claim request has no record at its expected revision")?;
    if claim.summary != request.summary || claim.artifact_ids != request.artifact_ids {
        anyhow::bail!("replayed governed claim request changed its summary or artifact IDs");
    }
    Ok(impulse_ops::governed_task::WorkerCompletionClaimInput {
        actor: claim.actor.clone(),
        summary: claim.summary.clone(),
        subject_revision: claim.subject_revision.clone(),
        artifact_ids: claim.artifact_ids.clone(),
        diff_ref: claim.diff_ref.clone(),
    })
}

fn replay_verification_input(
    task: &impulse_ops::governed_task::GovernedTaskRun,
    expected_revision: u64,
) -> Result<impulse_ops::governed_task::GovernedVerificationInput> {
    let verification = task
        .verifications
        .iter()
        .find(|verification| verification.based_on_revision == expected_revision)
        .context("stale governed verification request has no record at its expected revision")?;
    Ok(impulse_ops::governed_task::GovernedVerificationInput {
        actor: verification.actor.clone(),
        claim_id: verification.claim_id.clone(),
        subject_revision: verification.subject_revision.clone(),
        policy: verification.policy.clone(),
        outcome: verification.outcome,
        commands: verification.commands.clone(),
        artifact_ids: verification.artifact_ids.clone(),
        notes: verification.notes.clone(),
    })
}

fn replay_supervisor_input(
    task: &impulse_ops::governed_task::GovernedTaskRun,
    expected_revision: u64,
) -> Result<impulse_ops::governed_task::SupervisorVerdictInput> {
    let verdict = task
        .supervisor_verdicts
        .iter()
        .find(|verdict| verdict.based_on_revision == expected_revision)
        .context("stale governed Supervisor request has no verdict at its expected revision")?;
    Ok(impulse_ops::governed_task::SupervisorVerdictInput {
        actor: verdict.actor.clone(),
        verification_id: verdict.verification_id.clone(),
        verdict: verdict.verdict,
        rationale: verdict.rationale.clone(),
    })
}

pub(crate) async fn handle_governed_producer_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    let result = async {
        let (_producer_guard, mutation_request) = match request {
            DaemonRequest::SubmitGovernedClaim { request } => {
                let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
                let task =
                    require_current_governed_task(state, &request.project_id, &request.task_id)?;
                if task.verification_profile.is_none() {
                    anyhow::bail!("governed claim producer requires a closed-loop task profile");
                }
                let replay = require_producer_request_state(
                    state,
                    &task,
                    &request.request_id,
                    request.expected_revision,
                )?;
                let claim = if replay {
                    replay_claim_input(&task, &request)?
                } else {
                    preflight_claim(&task, &request)?;
                    let task_for_git = task.clone();
                    let request_for_git = request.clone();
                    tokio::task::spawn_blocking(move || {
                        crate::governed_producers::derive_claim(&task_for_git, &request_for_git)
                    })
                    .await
                    .context("governed claim Git observer panicked")??
                };
                let mutation_request = impulse_ops::governed_task::GovernedTaskMutationRequest {
                    request_id: request.request_id,
                    project_id: request.project_id,
                    task_id: request.task_id,
                    expected_revision: request.expected_revision,
                    mutation: impulse_ops::governed_task::GovernedTaskMutation::SubmitClaim {
                        claim,
                    },
                };
                (_producer_guard, mutation_request)
            }
            DaemonRequest::RunGovernedVerification { request } => {
                let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
                let task =
                    require_current_governed_task(state, &request.project_id, &request.task_id)?;
                if task.verification_profile.is_none() {
                    anyhow::bail!(
                        "governed verification producer requires a closed-loop task profile"
                    );
                }
                let replay = require_producer_request_state(
                    state,
                    &task,
                    &request.request_id,
                    request.expected_revision,
                )?;
                let verification = if replay {
                    replay_verification_input(&task, request.expected_revision)?
                } else {
                    preflight_verification(&task)?;
                    crate::governed_producers::run_verification(&task).await?
                };
                let mutation_request = impulse_ops::governed_task::GovernedTaskMutationRequest {
                    request_id: request.request_id,
                    project_id: request.project_id,
                    task_id: request.task_id,
                    expected_revision: request.expected_revision,
                    mutation:
                        impulse_ops::governed_task::GovernedTaskMutation::RecordVerification {
                            verification,
                        },
                };
                (_producer_guard, mutation_request)
            }
            _ => anyhow::bail!("Internal routing error: not a governed producer request"),
        };

        persist_governed_mutation(state, mutation_request).await
    }
    .await;

    match result {
        Ok(task) => respond_ok(&task),
        Err(error) => respond_err(error),
    }
}

pub(crate) async fn handle_status(state: &SharedState) -> DaemonResponse {
    match state.list_sessions().await {
        Ok(sessions) => DaemonResponse::Ok {
            result: serde_json::json!({
                "sessions": sessions.len(),
                "active": sessions.iter().filter(|s| s.status == crate::state::SessionStatus::Active).count(),
                "protocol_version": PROTOCOL_VERSION,
            }),
        },
        Err(e) => respond_err(e),
    }
}

pub(crate) async fn handle_session_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    match request {
        DaemonRequest::CreateSession { name, platform } => {
            let platform = platform.and_then(|p| crate::state::Platform::from_str_name(&p));
            match state.create_session(name, platform).await {
                Ok(session) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "session_id": session.id,
                        "name": session.name
                    }),
                },
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::EndSession {
            session_id,
            summary,
        } => match state.end_session(&session_id, summary).await {
            Ok(Some(entry)) => DaemonResponse::Ok {
                result: serde_json::json!({
                    "session_id": entry.session_id,
                    "ended_at": entry.ended_at
                }),
            },
            Ok(None) => respond_err("Session not found"),
            Err(e) => respond_err(e),
        },
        DaemonRequest::TrackFile {
            session_id,
            file_path,
        } => match state.track_file(&session_id, &file_path).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => respond_err(e),
        },
        DaemonRequest::TrackTool {
            session_id,
            tool_name,
        } => match state.track_tool(&session_id, &tool_name).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => respond_err(e),
        },
        DaemonRequest::CheckConflict {
            session_id,
            file_path,
        } => match state.check_file_conflict(&session_id, &file_path).await {
            Ok(conflicting) => DaemonResponse::ConflictCheck {
                has_conflict: !conflicting.is_empty(),
                conflicting_sessions: conflicting,
            },
            Err(e) => respond_err(format!("Conflict check failed: {}", e)),
        },
        DaemonRequest::GetSession { session_id } => match state.get_session(&session_id).await {
            Ok(Some(session)) => respond_ok(&session),
            Ok(None) => respond_err("Session not found"),
            Err(e) => respond_err(e),
        },
        DaemonRequest::ListSessions => match state.list_sessions().await {
            Ok(sessions) => respond_ok(&sessions),
            Err(e) => respond_err(e),
        },
        _ => respond_err("Internal routing error: not a session request"),
    }
}

pub(crate) async fn handle_chat_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    let DaemonRequest::Chat {
        session_id,
        message,
        inject_mode,
        inject_explain,
    } = request
    else {
        return respond_err("Internal routing error: not a chat request");
    };

    let config = match state.config_snapshot() {
        Ok(c) => c,
        Err(e) => return respond_err(format!("Failed to read config: {}", e)),
    };
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("CLAUDE_API_KEY"))
        .unwrap_or_else(|_| "".to_string());

    #[cfg(debug_assertions)]
    let test_mode = std::env::var("IMPULSE_TEST_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    #[cfg(not(debug_assertions))]
    let test_mode = false;

    if api_key.is_empty() && !test_mode {
        return respond_err("ANTHROPIC_API_KEY or CLAUDE_API_KEY not set");
    }

    let session_context = state.get_session(&session_id).await.ok().flatten();

    let mut context_prompt = if let Some(session) = &session_context {
        let files_list = session.active_files.join(", ");
        let tools_list = session.recent_tools.join(", ");
        format!(
            "Session Context:\n- Session: {} (ID: {})\n- Files touched: {}\n- Recent tools: {}\n\nUser question: {}",
            session.name, session.id,
            if files_list.is_empty() { "none".to_string() } else { files_list },
            if tools_list.is_empty() { "none".to_string() } else { tools_list },
            message
        )
    } else {
        message.clone()
    };

    let mode_override = inject_mode.as_deref().and_then(InjectionMode::parse);
    if inject_mode.is_some() && mode_override.is_none() {
        return respond_err("Invalid inject_mode. Use off|review|apply");
    }
    let mut injection_query_parts = vec![message.clone()];
    if let Some(session) = &session_context {
        injection_query_parts.push(session.name.clone());
        if !session.active_files.is_empty() {
            injection_query_parts.push(session.active_files.join(" "));
        }
        if !session.recent_tools.is_empty() {
            injection_query_parts.push(session.recent_tools.join(" "));
        }
    }

    let injection_result = run_injection(
        state.storage().base_path(),
        &config,
        InjectionSurface::DaemonChat,
        mode_override,
        &injection_query_parts,
    );

    if injection_result.applied {
        if let Some(block) = &injection_result.injected_block {
            context_prompt = format!("{}\n\n{}", block, context_prompt);
        }
    }

    if test_mode {
        return DaemonResponse::Ok {
            result: serde_json::json!({
                "response": format!("TEST_MODE_RESPONSE: {}", message),
                "session_id": session_id,
                "model": "test-mode",
                "context_included": session_context.is_some(),
                "injection": if inject_explain {
                    serde_json::to_value(&injection_result).unwrap_or_else(|_| serde_json::json!({"status": "serialization_error"}))
                } else {
                    serde_json::json!({
                        "requested_mode": injection_result.requested_mode,
                        "effective_mode": injection_result.effective_mode,
                        "applied": injection_result.applied,
                        "artifact_path": injection_result.artifact_path,
                        "fallback_code": injection_result.explain.fallback_code,
                    })
                }
            }),
        };
    }

    let provider = AnthropicProvider::new(api_key);
    let model = std::env::var("IMPULSE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
    let request = ChatRequest {
        model,
        messages: vec![Message::text(Role::User, context_prompt)],
        temperature: 0.7,
        max_tokens: Some(4096),
        tools: Vec::new(),
    };

    match provider.chat(request).await {
        Ok(response) => DaemonResponse::Ok {
            result: serde_json::json!({
                "response": response.content,
                "session_id": session_id,
                "model": response.model,
                "context_included": session_context.is_some(),
                "injection": if inject_explain {
                    serde_json::to_value(&injection_result).unwrap_or_else(|_| serde_json::json!({"status": "serialization_error"}))
                } else {
                    serde_json::json!({
                        "requested_mode": injection_result.requested_mode,
                        "effective_mode": injection_result.effective_mode,
                        "applied": injection_result.applied,
                        "artifact_path": injection_result.artifact_path,
                        "fallback_code": injection_result.explain.fallback_code,
                    })
                }
            }),
        },
        Err(e) => respond_err(e),
    }
}

pub(crate) async fn handle_tool_request(
    request: DaemonRequest,
    registry: &crate::tooling::ToolRegistry,
    tool_context: &crate::tooling::ToolContext,
) -> DaemonResponse {
    match request {
        DaemonRequest::ListTools { category } => {
            let descriptors = if let Some(cat) = category {
                let cat = match cat.as_str() {
                    "system" => crate::tooling::ToolCategory::System,
                    "utility" => crate::tooling::ToolCategory::Utility,
                    "analysis" => crate::tooling::ToolCategory::Analysis,
                    "document" => crate::tooling::ToolCategory::Document,
                    _ => {
                        return respond_err(format!(
                            "Unknown category: {}. Use system, utility, analysis, document",
                            cat
                        ))
                    }
                };
                registry.list_by_category(cat)
            } else {
                registry.list()
            };
            let out: Vec<_> = descriptors
                .iter()
                .map(|d| {
                    let source = registry
                        .source(&d.id)
                        .map(|s| s.as_str())
                        .unwrap_or("builtin");
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "description": d.description,
                        "category": format!("{}", d.category),
                        "params": d.params.len(),
                        "source": source,
                    })
                })
                .collect();
            DaemonResponse::Ok {
                result: serde_json::json!({"tools": out, "count": out.len()}),
            }
        }
        DaemonRequest::DescribeTool { name } => match registry.get(&name) {
            Some(tool) => {
                let desc = tool.descriptor();
                DaemonResponse::Ok {
                    result: serde_json::json!({
                        "id": desc.id,
                        "name": desc.name,
                        "description": desc.description,
                        "version": desc.version,
                        "category": format!("{}", desc.category),
                        "params": desc.params.iter().map(|p| serde_json::json!({
                            "name": p.name,
                            "description": p.description,
                            "type": p.param_type.as_str(),
                            "required": p.required,
                            "default": p.default,
                        })).collect::<Vec<_>>(),
                        "capabilities": tool.required_capabilities().iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>(),
                        "source": registry.source(&desc.id).map(|s| s.as_str()).unwrap_or("builtin"),
                    }),
                }
            }
            None => respond_err(format!("Tool not found: {}", name)),
        },
        DaemonRequest::InvokeTool { name, params } => {
            match registry.execute(&name, params, tool_context).await {
                Ok(result) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "tool": name,
                        "output": result.output,
                        "metadata": result.metadata,
                    }),
                },
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::ToolSchema => DaemonResponse::Ok {
            result: serde_json::json!({"tools": registry.schema_json()}),
        },
        _ => respond_err("Internal routing error: not a tool request"),
    }
}

pub(crate) async fn handle_steward_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    use crate::stewardship;

    match request {
        DaemonRequest::StewardStatus => {
            let base = state.storage().base_path();
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => return respond_err(format!("Failed to read config: {}", e)),
            };
            let stew_config = stewardship::StewardshipConfig::from_config(&config);
            let proposals = stewardship::approval::list_pending(base).unwrap_or_default();
            let cross = stewardship::cross_project::load_cross_project(base).unwrap_or_default();

            DaemonResponse::Ok {
                result: serde_json::json!({
                    "mode": stew_config.mode.as_str(),
                    "thresholds": {
                        "monitor": stew_config.monitor_threshold,
                        "surgical": stew_config.surgical_threshold,
                        "thoughtful": stew_config.thoughtful_threshold,
                        "emergency": stew_config.emergency_threshold,
                    },
                    "context_window_tokens": stew_config.context_window_tokens,
                    "pending_proposals": proposals.len(),
                    "cross_project_patterns": cross.patterns.len(),
                    "cross_project_learnings": cross.learnings.len(),
                }),
            }
        }
        DaemonRequest::StewardProposals { action, id } => {
            let base = state.storage().base_path();
            match action.as_str() {
                "list" => match stewardship::approval::list_pending(base) {
                    Ok(proposals) => {
                        let out: Vec<_> = proposals
                            .iter()
                            .map(|p| {
                                serde_json::json!({
                                    "id": p.id,
                                    "strategy": p.strategy.as_str(),
                                    "threshold": p.threshold.as_str(),
                                    "estimated_tokens_freed": p.estimated_tokens_freed,
                                    "regions": p.regions.len(),
                                })
                            })
                            .collect();
                        DaemonResponse::Ok {
                            result: serde_json::json!(out),
                        }
                    }
                    Err(e) => respond_err(e),
                },
                "approve" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => return respond_err("id required for approve"),
                    };
                    match stewardship::approval::approve_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"approved": pid}),
                        },
                        Ok(false) => respond_err(format!("Proposal {} not found", pid)),
                        Err(e) => respond_err(e),
                    }
                }
                "reject" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => return respond_err("id required for reject"),
                    };
                    match stewardship::approval::reject_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"rejected": pid}),
                        },
                        Ok(false) => respond_err(format!("Proposal {} not found", pid)),
                        Err(e) => respond_err(e),
                    }
                }
                _ => respond_err(format!(
                    "Unknown action: {}. Use list, approve, reject",
                    action
                )),
            }
        }
        DaemonRequest::StewardMemory => {
            let base = state.storage().base_path();
            match stewardship::cross_project::load_cross_project(base) {
                Ok(cross) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "version": cross.version,
                        "updated": cross.updated.to_rfc3339(),
                        "patterns": cross.patterns.iter().map(|p| serde_json::json!({
                            "id": p.id,
                            "type": p.pattern_type,
                            "description": p.description,
                            "occurrences": p.occurrences,
                            "projects": p.projects,
                            "insight": p.insight,
                        })).collect::<Vec<_>>(),
                        "learnings": cross.learnings,
                        "stats": {
                            "total_patterns": cross.stats.total_patterns,
                            "total_sessions": cross.stats.total_sessions_analyzed,
                            "total_learnings": cross.stats.total_learnings,
                        },
                    }),
                },
                Err(e) => respond_err(e),
            }
        }
        _ => respond_err("Internal routing error: not a steward request"),
    }
}

pub(crate) async fn handle_ops_request(
    request: DaemonRequest,
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
) -> DaemonResponse {
    match request {
        DaemonRequest::GetOpsSnapshot => {
            match build_ops_snapshot(state, terminal_telemetry).await {
                Ok(snapshot) => respond_ok(&snapshot),
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::SubscribeOps { since_seq } => {
            let reports = load_terminal_reports(state, terminal_telemetry).await;
            match crate::ops_workbench::subscribe_ops(state, since_seq, &reports).await {
                Ok(subscription) => respond_ok(&subscription),
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::PublishTerminalOps { report } => {
            let project = crate::ops_workbench::project_summary(state);
            terminal_telemetry
                .write()
                .await
                .publish(&project.id, report);
            DaemonResponse::Ok {
                result: serde_json::json!({"accepted": true}),
            }
        }
        DaemonRequest::ListArtifacts { limit } => {
            match build_ops_snapshot(state, terminal_telemetry).await {
                Ok(snapshot) => {
                    let mut artifacts = snapshot.artifacts;
                    if let Some(limit) = limit {
                        artifacts.truncate(limit);
                    }
                    respond_ok(&artifacts)
                }
                Err(e) => respond_err(format!("Failed to list artifacts: {}", e)),
            }
        }
        DaemonRequest::GetArtifact { artifact_id } => {
            match build_ops_snapshot(state, terminal_telemetry).await {
                Ok(snapshot) => match crate::ops_workbench::get_artifact(
                    state.storage().base_path(),
                    &snapshot.project.id,
                    &artifact_id,
                ) {
                    Ok(Some(artifact)) => respond_ok(&artifact),
                    Ok(None) => respond_err(format!("Artifact not found: {}", artifact_id)),
                    Err(e) => respond_err(format!("Failed to read artifact: {}", e)),
                },
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::RunArtifactAction {
            artifact_id,
            action_id,
            params,
        } => match build_ops_snapshot(state, terminal_telemetry).await {
            Ok(snapshot) => match crate::ops_workbench::run_artifact_action(
                state.storage().base_path(),
                &snapshot.project.id,
                &artifact_id,
                &action_id,
                &params,
            ) {
                Ok(result) => respond_ok(&result),
                Err(e) => respond_err(format!("Artifact action failed: {}", e)),
            },
            Err(e) => respond_err(format!("Failed to resolve artifact action context: {}", e)),
        },
        _ => respond_err("Internal routing error: not an ops request"),
    }
}

const AGENT_BUSY_RETRY_AFTER_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentTurnBusy;

fn agent_turn_busy_response() -> DaemonResponse {
    DaemonResponse::Busy {
        resource: impulse_ops::DaemonBusyResource::AgentTurn,
        retry_after_ms: AGENT_BUSY_RETRY_AFTER_MS,
    }
}

/// Try to acquire the daemon's one cached agent for a complete logical turn.
///
/// The returned Tokio mutex guard is intentionally held across the caller's
/// agent query. A concurrent request receives a typed `Busy` response instead
/// of queueing beyond its client lifetime or forking the cached state. This
/// keeps conversation history on one `ImpulseAgent` and makes cache state
/// cancellation-safe: aborting the handler drops the guard but leaves the agent
/// in the cache. The previous
/// checkout/checkin ownership transfer allowed concurrent re-initialization,
/// last-writer-wins history loss, and permanent cache loss when a task was
/// cancelled between checkout and checkin.
///
/// Holding this asynchronous mutex does not block unrelated daemon requests;
/// only agent handlers acquire it. Harness and API calls are independently
/// bounded by their provider timeouts, so an in-flight turn cannot retain the
/// guard forever. Fail-fast acquisition also guarantees a request cannot wait
/// behind another full provider timeout and later commit a turn after its
/// client has disconnected.
fn try_lock_agent_for_turn<'a>(
    cached_agent: &'a tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>,
    state: &SharedState,
) -> Result<tokio::sync::MutexGuard<'a, Option<crate::agent::ImpulseAgent>>, AgentTurnBusy> {
    let mut guard = cached_agent.try_lock().map_err(|_| AgentTurnBusy)?;
    if guard.is_none() {
        if let Ok(config) = state.config_snapshot() {
            *guard = crate::agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            );
        }
    }
    Ok(guard)
}

pub(crate) async fn handle_supervisor_request(
    request: DaemonRequest,
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: &Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
    cached_agent: &Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
) -> DaemonResponse {
    match request {
        DaemonRequest::GetSupervisorPermissions => {
            match build_supervisor_permission_state(state, supervisor_session_override).await {
                Ok(permission_state) => respond_ok(&permission_state),
                Err(e) => respond_err(e),
            }
        }
        DaemonRequest::SupervisorChat { prompt, context } => {
            let snapshot = match build_ops_snapshot(state, terminal_telemetry).await {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    return respond_err(format!("Failed to build supervisor snapshot: {}", e))
                }
            };
            let permission_state =
                match build_supervisor_permission_state(state, supervisor_session_override).await {
                    Ok(state) => state,
                    Err(e) => {
                        return respond_err(format!(
                            "Failed to resolve supervisor permissions: {}",
                            e
                        ))
                    }
                };
            let mut agent_guard = match try_lock_agent_for_turn(cached_agent, state) {
                Ok(guard) => guard,
                Err(_) => return agent_turn_busy_response(),
            };
            let agent = match agent_guard.as_mut() {
                Some(agent) => agent,
                None => {
                    let fallback = impulse_ops::SupervisorChatResult {
                        response: "Impulse Agent not configured. Configure a provider or harness to enable supervisor chat.".to_string(),
                        proposals: Vec::new(),
                        permission_state,
                    };
                    return DaemonResponse::Ok {
                        result: serde_json::to_value(fallback)
                            .unwrap_or_else(|_| serde_json::json!({})),
                    };
                }
            };

            if !agent.is_ready() {
                let fallback = impulse_ops::SupervisorChatResult {
                    response: "Impulse Agent is configured but not ready. Check the API key or harness installation.".to_string(),
                    proposals: Vec::new(),
                    permission_state,
                };
                return DaemonResponse::Ok {
                    result: serde_json::to_value(fallback)
                        .unwrap_or_else(|_| serde_json::json!({})),
                };
            }

            let full_prompt =
                build_supervisor_prompt(&snapshot, &permission_state, &prompt, context.as_deref());

            // The agent-turn mutex is held across this bounded query so a
            // second agent request receives Busy instead of forking session
            // history. Unrelated daemon request groups do not acquire it.
            let query_result = agent
                .query(crate::agent::prompts::SUPERVISOR_SYSTEM, &full_prompt)
                .await;

            match query_result {
                Ok(response) => {
                    let parsed = parse_supervisor_chat_response(&response, &permission_state);
                    respond_ok(&parsed)
                }
                Err(e) => respond_err(format!("Supervisor chat failed: {}", e)),
            }
        }
        DaemonRequest::RunGovernedSupervisorReview { request } => {
            let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
            let task =
                match require_current_governed_task(state, &request.project_id, &request.task_id) {
                    Ok(task) => task,
                    Err(error) => return respond_err(error),
                };
            if task.verification_profile.is_none() {
                return respond_err(
                    "governed Supervisor producer requires a closed-loop task profile",
                );
            }
            let replay = match require_producer_request_state(
                state,
                &task,
                &request.request_id,
                request.expected_revision,
            ) {
                Ok(replay) => replay,
                Err(error) => return respond_err(error),
            };

            let verdict = if replay {
                match replay_supervisor_input(&task, request.expected_revision) {
                    Ok(verdict) => verdict,
                    Err(error) => return respond_err(error),
                }
            } else {
                if let Err(error) = preflight_supervisor_review(&task) {
                    return respond_err(error);
                }
                let (system_prompt, user_prompt) =
                    match crate::governed_producers::supervisor_review_prompt(&task) {
                        Ok(prompt) => prompt,
                        Err(error) => return respond_err(error),
                    };
                let mut agent_guard = match try_lock_agent_for_turn(cached_agent, state) {
                    Ok(guard) => guard,
                    Err(_) => return agent_turn_busy_response(),
                };
                let agent = match agent_guard.as_mut() {
                    Some(agent) if agent.is_ready() => agent,
                    Some(_) => return respond_err(
                        "Impulse Agent is configured but not ready for governed Supervisor review",
                    ),
                    None => {
                        return respond_err(
                            "Impulse Agent must be configured before governed Supervisor review",
                        )
                    }
                };
                let supervisor_actor = agent.governed_review_actor();
                let response = match agent.query_stateless(&system_prompt, &user_prompt).await {
                    Ok(response) => response,
                    Err(error) => {
                        return respond_err(format!(
                            "governed Supervisor review turn failed: {error}"
                        ))
                    }
                };
                match crate::governed_producers::bind_supervisor_review(
                    &task,
                    &response,
                    supervisor_actor,
                ) {
                    Ok(verdict) => verdict,
                    Err(error) => return respond_err(error),
                }
            };

            let mutation_request = impulse_ops::governed_task::GovernedTaskMutationRequest {
                request_id: request.request_id,
                project_id: request.project_id,
                task_id: request.task_id,
                expected_revision: request.expected_revision,
                mutation:
                    impulse_ops::governed_task::GovernedTaskMutation::RecordSupervisorVerdict {
                        verdict,
                    },
            };
            match persist_governed_mutation(state, mutation_request).await {
                Ok(task) => respond_ok(&task),
                Err(error) => respond_err(error),
            }
        }
        DaemonRequest::RunSupervisorAction { action } => {
            match run_supervisor_action(
                state,
                action,
                terminal_telemetry,
                supervisor_session_override,
            )
            .await
            {
                Ok(result) => respond_ok(&result),
                Err(e) => respond_err(format!("Supervisor action failed: {}", e)),
            }
        }
        _ => respond_err("Internal routing error: not a supervisor request"),
    }
}

pub(crate) async fn handle_agent_request(
    request: DaemonRequest,
    state: &SharedState,
    cached_agent: &Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
) -> DaemonResponse {
    let DaemonRequest::AgentAssist {
        prompt,
        context,
        insights,
    } = request
    else {
        return respond_err("Internal routing error: not an agent request");
    };

    let mut agent_guard = match try_lock_agent_for_turn(cached_agent, state) {
        Ok(guard) => guard,
        Err(_) => return agent_turn_busy_response(),
    };
    let agent = match agent_guard.as_mut() {
        Some(agent) => agent,
        None => {
            return DaemonResponse::AgentAssistResult {
                success: false,
                response: "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY".to_string(),
                recommendations: Vec::new(),
                pane_summaries: Vec::new(),
            }
        }
    };

    if !agent.is_ready() {
        return DaemonResponse::AgentAssistResult {
            success: false,
            response:
                "Impulse Agent is configured but not ready (check API key or harness installation)"
                    .to_string(),
            recommendations: Vec::new(),
            pane_summaries: Vec::new(),
        };
    }

    // Run full coordination pipeline: recommendations + pane summaries.
    // This activates detect_file_conflicts, detect_cross_pane_errors,
    // detect_delegation_events, and aggregate_pane_summaries.
    let coordination = agent.coordinate_full(&insights);

    let full_prompt = match context {
        Some(ctx) => format!("Context:\n{}\n\nRequest:\n{}", ctx, prompt),
        None => prompt,
    };

    // Use query_with_context when insights are available to enrich the prompt
    // with structured cross-pane context from the context lifecycle. The
    // agent-turn mutex remains held so history updates are ordered.
    let result = if insights.is_empty() {
        agent
            .query(crate::agent::prompts::COORDINATION_SYSTEM, &full_prompt)
            .await
    } else {
        agent
            .query_with_context(
                crate::agent::prompts::COORDINATION_SYSTEM,
                &full_prompt,
                &insights,
            )
            .await
    };

    match result {
        Ok(response) => DaemonResponse::AgentAssistResult {
            success: true,
            response,
            recommendations: coordination.recommendations,
            pane_summaries: coordination.pane_summaries,
        },
        Err(e) => DaemonResponse::AgentAssistResult {
            success: false,
            response: format!("Agent query failed: {}", e),
            recommendations: coordination.recommendations,
            pane_summaries: coordination.pane_summaries,
        },
    }
}

/// Handle specialized agent IPC requests (Task 23): review_code, analyze_error, summarize_pane.
///
/// Uses the daemon-level cached agent so session history persists across requests.
pub(crate) async fn handle_agent_specialized_request(
    request: DaemonRequest,
    state: &SharedState,
    cached_agent: &Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
) -> DaemonResponse {
    let mut agent_guard = match try_lock_agent_for_turn(cached_agent, state) {
        Ok(guard) => guard,
        Err(_) => return agent_turn_busy_response(),
    };
    let agent = match agent_guard.as_mut() {
        Some(agent) => agent,
        None => {
            return respond_err(
                "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY",
            )
        }
    };

    if !agent.is_ready() {
        return respond_err(
            "Impulse Agent is configured but not ready (check API key or harness installation)",
        );
    }

    // All branches run under the same agent-turn guard so the one cached
    // agent's conversation history cannot fork or be overwritten.
    let response = match request {
        DaemonRequest::AgentReviewCode {
            file_path,
            diff,
            insights,
        } => {
            let insight_lines: Vec<String> = if diff.is_empty() {
                vec![format!("File: {}", file_path)]
            } else {
                vec![format!("File: {}", file_path), format!("Diff:\n{}", diff)]
            };
            let pane_label = format!("review-{}", file_path);
            match agent
                .review_code(&pane_label, &insight_lines, &insights)
                .await
            {
                Ok(response) => DaemonResponse::AgentSpecializedResult {
                    success: true,
                    response,
                },
                Err(e) => DaemonResponse::AgentSpecializedResult {
                    success: false,
                    response: format!("review_code failed: {}", e),
                },
            }
        }
        DaemonRequest::AgentAnalyzeError {
            error_text,
            context,
            insights,
        } => {
            let pane_label = if context.is_empty() {
                "error-analysis".to_string()
            } else {
                context.clone()
            };
            match agent
                .analyze_error(&pane_label, &error_text, &insights)
                .await
            {
                Ok(response) => DaemonResponse::AgentSpecializedResult {
                    success: true,
                    response,
                },
                Err(e) => DaemonResponse::AgentSpecializedResult {
                    success: false,
                    response: format!("analyze_error failed: {}", e),
                },
            }
        }
        DaemonRequest::AgentSummarizePane {
            pane_id,
            raw_output,
            insights,
        } => {
            let pane_label = format!("pane-{}", pane_id);
            match agent
                .summarize_pane(&pane_label, &raw_output, &insights)
                .await
            {
                Ok(response) => DaemonResponse::AgentSpecializedResult {
                    success: true,
                    response,
                },
                Err(e) => DaemonResponse::AgentSpecializedResult {
                    success: false,
                    response: format!("summarize_pane failed: {}", e),
                },
            }
        }
        _ => respond_err("Internal routing error: not a specialized agent request"),
    };

    response
}

pub(crate) async fn handle_debug_snapshot(
    state: &SharedState,
    registry: &crate::tooling::ToolRegistry,
) -> DaemonResponse {
    use crate::state::SessionStatus;

    let sessions = state.list_sessions().await.unwrap_or_default();
    let active_count = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Active)
        .count();

    let tool_count = registry.list().len();

    let config = state.config_snapshot().ok();

    let guardrail_count = config
        .as_ref()
        .map(|c| crate::guardrail::config::merge_rules(&c.guardrails).len())
        .unwrap_or(0);

    let r = crate::plugin::registry::global_registry();
    let providers = r.list_context_providers().unwrap_or_default().len();
    let handlers = r.list_action_handlers().unwrap_or_default().len();
    let plugins = serde_json::json!({
        "context_providers": providers,
        "action_handlers": handlers,
    });

    let base_path = state.storage().base_path().display().to_string();

    DaemonResponse::Ok {
        result: serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "pid": std::process::id(),
            "base_path": base_path,
            "sessions": {
                "total": sessions.len(),
                "active": active_count,
            },
            "tools": {
                "registered": tool_count,
            },
            "guardrails": {
                "rules": guardrail_count,
            },
            "plugins": plugins,
            "config": config.as_ref().map(|c| serde_json::json!({
                "default_platform": &c.default_platform,
            })).unwrap_or(serde_json::json!(null)),
        }),
    }
}

pub(crate) async fn handle_guard_request(
    request: DaemonRequest,
    state: &SharedState,
) -> DaemonResponse {
    match request {
        DaemonRequest::GuardEvaluate { target, action } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => return respond_err(format!("Failed to read config: {}", e)),
            };
            match crate::guardrail::evaluate_action(&action, &target, &config.guardrails) {
                Ok(results) => {
                    let has_block = crate::guardrail::GuardEngine::has_blocking(&results);
                    DaemonResponse::Ok {
                        result: serde_json::json!({
                            "blocked": has_block,
                            "results": results,
                        }),
                    }
                }
                Err(e) => respond_err(format!("Guardrail evaluation failed: {}", e)),
            }
        }
        DaemonRequest::GuardList => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => return respond_err(format!("Failed to read config: {}", e)),
            };
            let rules = crate::guardrail::list_active_rules(&config.guardrails);
            DaemonResponse::Ok {
                result: serde_json::json!({ "rules": rules }),
            }
        }
        _ => respond_err("Internal routing error: not a guard request"),
    }
}

/// Handle delegation lifecycle requests against the shared DelegationTracker
/// (Phase 1B). Backs RegisterDelegation / CompleteDelegation / ListDelegations.
pub(crate) async fn handle_delegation_request(
    request: DaemonRequest,
    delegation_tracker: &Arc<RwLock<crate::delegation::DelegationTracker>>,
) -> DaemonResponse {
    match request {
        DaemonRequest::RegisterDelegation {
            spec,
            coordinator_pane_id,
            context_snapshot,
        } => {
            // Daemon-registered delegations are top-level (depth 0); nested
            // depth tracking is an in-process concern.
            let mut tracker = delegation_tracker.write().await;
            match tracker.register(spec, coordinator_pane_id, context_snapshot, 0) {
                Some(id) => respond_ok(&serde_json::json!({ "delegation_id": id })),
                None => respond_err(format!(
                    "delegation rejected: max depth ({}) would be exceeded",
                    crate::delegation::types::MAX_DELEGATION_DEPTH
                )),
            }
        }
        DaemonRequest::CompleteDelegation {
            delegation_id,
            summary,
            tool_trace,
            diff_summary,
        } => {
            let mut tracker = delegation_tracker.write().await;
            if tracker.complete(&delegation_id, summary, tool_trace, diff_summary) {
                let handoff_prompt = tracker.build_handoff_prompt(&delegation_id);
                respond_ok(&serde_json::json!({
                    "completed": true,
                    "handoff_prompt": handoff_prompt,
                }))
            } else {
                respond_err(format!("delegation not found: {delegation_id}"))
            }
        }
        DaemonRequest::ListDelegations => {
            let tracker = delegation_tracker.read().await;
            respond_ok(&tracker.to_summaries())
        }
        _ => respond_err("Internal routing error: not a delegation request"),
    }
}

pub(crate) async fn handle_plugin_request(request: DaemonRequest) -> DaemonResponse {
    let registry = crate::plugin::registry::global_registry();

    match request {
        DaemonRequest::ListPlugins => {
            let context_providers = match registry.list_context_providers() {
                Ok(p) => p,
                Err(e) => return respond_err(format!("Failed to list context providers: {}", e)),
            };
            let action_handlers = match registry.list_action_handlers_metadata() {
                Ok(h) => h,
                Err(e) => return respond_err(format!("Failed to list action handlers: {}", e)),
            };
            DaemonResponse::Ok {
                result: serde_json::json!({
                    "context_providers": context_providers,
                    "action_handlers": action_handlers,
                }),
            }
        }
        DaemonRequest::InvokePlugin { name, input } => {
            let handler = match registry.get_action_handler(&name) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    return respond_err(format!("Plugin not found: {}", name));
                }
                Err(e) => return respond_err(format!("Registry error: {}", e)),
            };
            match handler.validate(&input) {
                Ok(()) => {}
                Err(e) => return respond_err(format!("Validation failed: {}", e)),
            }
            match handler.execute(&input) {
                Ok(output) => DaemonResponse::Ok {
                    result: serde_json::to_value(output).unwrap_or_default(),
                },
                Err(e) => respond_err(format!("Plugin execution failed: {}", e)),
            }
        }
        _ => respond_err("Internal routing error: not a plugin request"),
    }
}

#[cfg(test)]
mod agent_cache_lifecycle_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use super::{handle_agent_request, handle_status, try_lock_agent_for_turn};
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
    use crate::llm_backends::{
        async_trait, AgentResult, ChatRequest, ChatResponse, LlmProvider, StopReason, Usage,
    };
    use tokio::sync::Notify;

    struct GatedProviderState {
        calls: AtomicUsize,
        requests: std::sync::Mutex<Vec<ChatRequest>>,
        blocked_call: usize,
        blocked_call_started: Notify,
        release_blocked_call: Notify,
    }

    impl GatedProviderState {
        fn new(blocked_call: usize) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                requests: std::sync::Mutex::new(Vec::new()),
                blocked_call,
                blocked_call_started: Notify::new(),
                release_blocked_call: Notify::new(),
            })
        }
    }

    struct GatedProvider {
        state: Arc<GatedProviderState>,
    }

    #[async_trait]
    impl LlmProvider for GatedProvider {
        fn name(&self) -> &str {
            "gated-test-provider"
        }

        fn default_model(&self) -> &str {
            "gated-test-model"
        }

        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            let call = self.state.calls.fetch_add(1, Ordering::SeqCst);
            self.state.requests.lock().unwrap().push(request);
            if call == self.state.blocked_call {
                self.state.blocked_call_started.notify_one();
                self.state.release_blocked_call.notified().await;
            }
            Ok(ChatResponse {
                content: format!("reply-{}", call + 1),
                model: self.default_model().to_string(),
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
                tool_calls: Vec::new(),
            })
        }

        fn supported_models(&self) -> Vec<&str> {
            vec![self.default_model()]
        }
    }

    fn test_state() -> (tempfile::TempDir, crate::state::SharedState) {
        let tmp = tempfile::TempDir::new().unwrap();
        let st = crate::state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    fn agent_assist(prompt: &str) -> DaemonRequest {
        DaemonRequest::AgentAssist {
            prompt: prompt.to_string(),
            context: None,
            insights: Vec::new(),
        }
    }

    fn assert_agent_success(response: DaemonResponse) {
        assert!(
            matches!(
                response,
                DaemonResponse::AgentAssistResult { success: true, .. }
            ),
            "agent turn should succeed: {response:?}"
        );
    }

    fn assert_agent_busy(response: DaemonResponse) {
        assert!(
            matches!(
                response,
                DaemonResponse::Busy {
                    resource: impulse_ops::DaemonBusyResource::AgentTurn,
                    retry_after_ms: 250,
                }
            ),
            "concurrent agent turn should return typed busy: {response:?}"
        );
    }

    async fn notified_within(notify: &Notify, message: &'static str) {
        tokio::time::timeout(Duration::from_secs(2), notify.notified())
            .await
            .expect(message);
    }

    #[tokio::test]
    async fn test_concurrent_agent_turn_fails_fast_without_forking_the_cache() {
        let (_tmp, state) = test_state();
        let agent = crate::agent::ImpulseAgent::new(crate::agent::ImpulseAgentConfig::default())
            .expect("disabled-mode agent should construct");
        let cached_agent = tokio::sync::Mutex::new(Some(agent));
        let first_turn = try_lock_agent_for_turn(&cached_agent, &state)
            .expect("first turn should acquire the cached agent");

        assert!(
            try_lock_agent_for_turn(&cached_agent, &state).is_err(),
            "a concurrent logical turn must fail fast instead of queueing or forking the cache"
        );
        drop(first_turn);
        assert!(
            try_lock_agent_for_turn(&cached_agent, &state)
                .expect("the next turn should acquire after release")
                .is_some(),
            "the next turn must receive the same cached agent after the first releases it"
        );
    }

    #[tokio::test]
    async fn test_aborted_agent_turn_preserves_the_cached_agent() {
        let (_tmp, state) = test_state();
        let agent = crate::agent::ImpulseAgent::new(crate::agent::ImpulseAgentConfig::default())
            .expect("disabled-mode agent should construct");
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));
        let turn_started = Arc::new(Notify::new());

        let in_flight = {
            let cached_agent = cached_agent.clone();
            let state = state.clone();
            let turn_started = turn_started.clone();
            tokio::spawn(async move {
                let guard = try_lock_agent_for_turn(cached_agent.as_ref(), &state)
                    .expect("turn should acquire the cached agent");
                assert!(guard.is_some(), "turn should receive the cached agent");
                turn_started.notify_one();
                std::future::pending::<()>().await;
            })
        };

        notified_within(&turn_started, "agent turn did not acquire the cache").await;
        in_flight.abort();
        let _ = in_flight.await;

        assert!(
            try_lock_agent_for_turn(cached_agent.as_ref(), &state)
                .expect("aborting the turn must release the async mutex")
                .is_some(),
            "cancelling a logical turn must not drop the daemon's only cached agent"
        );
    }

    #[tokio::test]
    async fn test_unrelated_daemon_work_is_not_serialized_by_an_agent_turn() {
        let (_tmp, state) = test_state();
        let agent = crate::agent::ImpulseAgent::new(crate::agent::ImpulseAgentConfig::default())
            .expect("disabled-mode agent should construct");
        let cached_agent = tokio::sync::Mutex::new(Some(agent));
        let _agent_turn = try_lock_agent_for_turn(&cached_agent, &state)
            .expect("agent turn should acquire the cache");

        tokio::time::timeout(Duration::from_millis(100), handle_status(&state))
            .await
            .expect("non-agent daemon work must remain responsive while an agent turn is held");
    }

    #[tokio::test]
    async fn test_busy_agent_request_can_retry_into_one_ordered_conversation() {
        let (_tmp, state) = test_state();
        let provider_state = GatedProviderState::new(0);
        let agent = crate::agent::ImpulseAgent::with_test_provider(Box::new(GatedProvider {
            state: provider_state.clone(),
        }));
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));

        let first_turn = {
            let state = state.clone();
            let cached_agent = cached_agent.clone();
            tokio::spawn(async move {
                handle_agent_request(agent_assist("first turn"), &state, &cached_agent).await
            })
        };
        notified_within(
            &provider_state.blocked_call_started,
            "first provider turn did not start",
        )
        .await;

        let mut second_turn = {
            let state = state.clone();
            let cached_agent = cached_agent.clone();
            tokio::spawn(async move {
                handle_agent_request(agent_assist("second turn"), &state, &cached_agent).await
            })
        };
        let busy_response = tokio::time::timeout(Duration::from_millis(100), &mut second_turn)
            .await
            .expect("a concurrent turn must fail fast before the client budget expires")
            .expect("second turn task should join");
        assert_agent_busy(busy_response);
        assert_eq!(
            provider_state.calls.load(Ordering::SeqCst),
            1,
            "a busy request must not reach the provider"
        );

        provider_state.release_blocked_call.notify_one();
        assert_agent_success(first_turn.await.expect("first turn task should join"));
        assert_agent_success(
            handle_agent_request(agent_assist("second turn"), &state, &cached_agent).await,
        );

        let second_wire_context = {
            let requests = provider_state.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            requests[1]
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(second_wire_context.contains("first turn"));
        assert!(second_wire_context.contains("reply-1"));
        assert!(second_wire_context.contains("second turn"));

        let guard = cached_agent.lock().await;
        let history = guard.as_ref().unwrap().session_history();
        assert_eq!(
            history,
            &[
                ("first turn".to_string(), "reply-1".to_string()),
                ("second turn".to_string(), "reply-2".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn test_aborted_provider_turn_preserves_history_and_allows_recovery() {
        let (_tmp, state) = test_state();
        let provider_state = GatedProviderState::new(1);
        let agent = crate::agent::ImpulseAgent::with_test_provider(Box::new(GatedProvider {
            state: provider_state.clone(),
        }));
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));

        assert_agent_success(
            handle_agent_request(agent_assist("baseline"), &state, &cached_agent).await,
        );

        let cancelled_turn = {
            let state = state.clone();
            let cached_agent = cached_agent.clone();
            tokio::spawn(async move {
                handle_agent_request(agent_assist("cancel me"), &state, &cached_agent).await
            })
        };
        notified_within(
            &provider_state.blocked_call_started,
            "cancelled provider turn did not start",
        )
        .await;
        cancelled_turn.abort();
        let _ = cancelled_turn.await;

        {
            let guard = tokio::time::timeout(Duration::from_millis(50), cached_agent.lock())
                .await
                .expect("aborting the provider turn must release the agent guard");
            assert_eq!(
                guard.as_ref().unwrap().session_history(),
                &[("baseline".to_string(), "reply-1".to_string())],
                "a cancelled provider turn must preserve committed history and omit the partial conversation turn"
            );
        }

        assert_agent_success(
            handle_agent_request(agent_assist("recovery"), &state, &cached_agent).await,
        );
        let guard = cached_agent.lock().await;
        assert_eq!(
            guard.as_ref().unwrap().session_history(),
            &[
                ("baseline".to_string(), "reply-1".to_string()),
                ("recovery".to_string(), "reply-3".to_string()),
            ]
        );
    }
}

#[cfg(test)]
mod governed_producer_handler_tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use impulse_ops::agent_registry::{AgentPlatformId, AgentRegistry};
    use impulse_ops::governed_task::{
        GovernedActor, GovernedActorKind, GovernedClaimRequest, GovernedCommandEvidence,
        GovernedExecutionState, GovernedRequestId, GovernedReviewState,
        GovernedSupervisorReviewRequest, GovernedTaskMutation, GovernedTaskMutationRequest,
        GovernedTaskRegistration, GovernedTaskRun, GovernedVerificationInput,
        GovernedVerificationOutcome, GovernedVerificationProfile, GovernedVerificationRequest,
        OperatorDecisionInput, OperatorDecisionKind, SupervisorVerdictInput, SupervisorVerdictKind,
        WorkerCompletionClaimInput,
    };
    use impulse_ops::role_assignment::{
        canonical_governed_builder_assignment, AgentRoleAssignment, EnforcementStrength,
        RoleCompatibility,
    };

    use super::{
        handle_governed_producer_request, handle_governed_task_request, handle_supervisor_request,
    };
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
    use crate::llm_backends::{
        async_trait, AgentResult, ChatRequest, ChatResponse, LlmProvider, Role, StopReason, Usage,
    };

    struct BoundSupervisorProvider {
        calls: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<ChatRequest>>>,
    }

    #[async_trait]
    impl LlmProvider for BoundSupervisorProvider {
        fn name(&self) -> &str {
            "bound-supervisor-test"
        }

        fn default_model(&self) -> &str {
            "bound-supervisor-model"
        }

        async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let user = request
                .messages
                .iter()
                .find(|message| message.role == Role::User)
                .expect("Supervisor request must have a user payload");
            let payload: serde_json::Value =
                serde_json::from_str(&user.content).expect("Supervisor payload must be JSON");
            let response = serde_json::json!({
                "contract_version": payload["contract_version"],
                "task_id": payload["task_id"],
                "task_revision": payload["task_revision"],
                "claim_id": payload["claim_id"],
                "verification_id": payload["verification_id"],
                "subject_revision": payload["subject_revision"],
                "acceptance_criteria_count": payload["acceptance_criteria_count"],
                "acceptance_criteria_digest": payload["acceptance_criteria_digest"],
                "verdict": "recommend_accept",
                "rationale": "every exact criterion is supported by daemon-observed evidence"
            });
            self.requests.lock().unwrap().push(request);
            Ok(ChatResponse {
                content: response.to_string(),
                model: self.default_model().to_string(),
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::EndTurn,
                tool_calls: Vec::new(),
            })
        }

        fn supported_models(&self) -> Vec<&str> {
            vec![self.default_model()]
        }
    }

    fn run_git(root: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("Git command must launch");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout)
            .expect("Git output must be UTF-8")
            .trim()
            .to_string()
    }

    fn rust_repo_state() -> (tempfile::TempDir, crate::state::SharedState, String, String) {
        let repo = tempfile::Builder::new()
            .prefix("impulse-governed-handler-")
            .tempdir()
            .unwrap();
        std::fs::create_dir(repo.path().join("src")).unwrap();
        std::fs::write(
            repo.path().join("Cargo.toml"),
            "[package]\nname = \"governed_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join("src/lib.rs"),
            "pub fn governed_fixture() -> bool {\n    true\n}\n",
        )
        .unwrap();
        std::fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();
        let lock_status = Command::new("cargo")
            .arg("generate-lockfile")
            .current_dir(repo.path())
            .status()
            .expect("Cargo lockfile generation must launch");
        assert!(lock_status.success(), "Cargo lockfile generation failed");
        run_git(repo.path(), &["init", "--quiet"]);
        run_git(repo.path(), &["config", "user.email", "test@example.com"]);
        run_git(repo.path(), &["config", "user.name", "Impulse Test"]);
        run_git(
            repo.path(),
            &[
                "add",
                ".gitignore",
                "Cargo.toml",
                "Cargo.lock",
                "src/lib.rs",
            ],
        );
        run_git(repo.path(), &["commit", "--quiet", "-m", "initial"]);
        let oid = run_git(repo.path(), &["rev-parse", "HEAD"]);
        let project_id = impulse_ops::sanitize_id(
            &repo
                .path()
                .file_name()
                .expect("temp repo has a name")
                .to_string_lossy(),
        );
        let state = Arc::new(
            crate::state::State::new(repo.path().join(".impulse"))
                .expect("project state must initialize"),
        );
        (repo, state, project_id, oid)
    }

    fn task_from_response(response: DaemonResponse) -> GovernedTaskRun {
        match response {
            DaemonResponse::Ok { result } => {
                serde_json::from_value(result).expect("response must contain a governed task")
            }
            other => panic!("expected governed task response, received {other:?}"),
        }
    }

    fn error_from_response(response: DaemonResponse) -> String {
        match response {
            DaemonResponse::Error { message } => message,
            other => panic!("expected daemon error, received {other:?}"),
        }
    }

    fn request_id(value: &str) -> GovernedRequestId {
        GovernedRequestId::try_new(value).unwrap()
    }

    fn profiled_role(runtime: &str) -> (AgentRoleAssignment, RoleCompatibility) {
        let assignment = canonical_governed_builder_assignment();
        let platform = AgentPlatformId::try_new(runtime).unwrap();
        let compatibility = AgentRegistry::builtin()
            .evaluate_role_compatibility(&platform, &assignment)
            .unwrap();
        (assignment, compatibility)
    }

    #[tokio::test]
    async fn profiled_registration_rejects_caller_forged_runtime_compatibility() {
        let (repo, state, project_id, oid) = rust_repo_state();
        let (assignment, mut compatibility) = profiled_role("ion");
        let optional = compatibility
            .checks
            .iter_mut()
            .find(|check| !check.mandatory)
            .expect("canonical Builder contract has an optional filesystem check");
        optional.available = EnforcementStrength::Structural;
        assert!(compatibility.launch_allowed());

        let registration = GovernedTaskRegistration::builder(
            "register-forged-profile-compatibility",
            "task-forged-profile-compatibility",
            project_id,
            repo.path().display().to_string(),
            "reject caller-authored capability strength",
            "worker-forged-profile-compatibility",
            "ion",
        )
        .acceptance_criteria(vec!["daemon recomputes runtime compatibility".to_string()])
        .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
        .initial_subject_revision(oid)
        .role_assignment(assignment)
        .role_compatibility(compatibility)
        .build()
        .unwrap();

        let error = error_from_response(
            handle_governed_task_request(
                DaemonRequest::RegisterGovernedTask { registration },
                &state,
            )
            .await,
        );
        assert!(error.contains("daemon-observed runtime capability result"));
    }

    #[tokio::test]
    async fn lifecycle_mutations_wait_for_the_task_producer_lock() {
        let (repo, state, project_id, oid) = rust_repo_state();
        let (assignment, compatibility) = profiled_role("ion");
        let registration = GovernedTaskRegistration::builder(
            "register-lock-boundary",
            "task-lock-boundary",
            project_id.clone(),
            repo.path().display().to_string(),
            "serialize lifecycle mutations with producer side effects",
            "worker-lock-boundary",
            "ion",
        )
        .acceptance_criteria(vec!["the shared lock boundary is enforced".to_string()])
        .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
        .initial_subject_revision(oid)
        .role_assignment(assignment)
        .role_compatibility(compatibility)
        .build()
        .unwrap();
        let registered = task_from_response(
            handle_governed_task_request(
                DaemonRequest::RegisterGovernedTask { registration },
                &state,
            )
            .await,
        );
        let running = task_from_response(
            handle_governed_task_request(
                DaemonRequest::MutateGovernedTask {
                    request: GovernedTaskMutationRequest {
                        request_id: request_id("running-lock-boundary"),
                        project_id: project_id.clone(),
                        task_id: registered.id.clone(),
                        expected_revision: registered.revision,
                        mutation: GovernedTaskMutation::MarkRunning {
                            actor: GovernedActor {
                                kind: GovernedActorKind::System,
                                id: "desktop-runtime".to_string(),
                            },
                        },
                    },
                },
                &state,
            )
            .await,
        );

        let producer_guard = state.acquire_governed_producer_lock(&running.id).await;
        let mutation_state = state.clone();
        let mut mutation = tokio::spawn(async move {
            handle_governed_task_request(
                DaemonRequest::MutateGovernedTask {
                    request: GovernedTaskMutationRequest {
                        request_id: request_id("runtime-exit-lock-boundary"),
                        project_id,
                        task_id: running.id,
                        expected_revision: running.revision,
                        mutation: GovernedTaskMutation::MarkRuntimeExited {
                            actor: GovernedActor {
                                kind: GovernedActorKind::System,
                                id: "desktop-runtime".to_string(),
                            },
                            reason: Some("fixture exited".to_string()),
                        },
                    },
                },
                &mutation_state,
            )
            .await
        });

        tokio::time::timeout(Duration::from_millis(50), &mut mutation)
            .await
            .expect_err("lifecycle mutation must wait while the producer lock is held");
        drop(producer_guard);
        let exited = task_from_response(mutation.await.unwrap());
        assert_eq!(
            exited.execution_state,
            GovernedExecutionState::RuntimeExited
        );
    }

    #[tokio::test]
    async fn daemon_owned_producers_complete_one_persistent_governed_run() {
        let (repo, state, project_id, oid) = rust_repo_state();
        let (assignment, compatibility) = profiled_role("ion");
        let registration = GovernedTaskRegistration::builder(
            "register-producer-flow",
            "task-producer-flow",
            project_id.clone(),
            repo.path().display().to_string(),
            "prove the daemon-owned producer lifecycle",
            "worker-1",
            "ion",
        )
        .acceptance_criteria(vec!["the Rust workspace gate passes".to_string()])
        .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
        .initial_subject_revision(oid.clone())
        .role_assignment(assignment)
        .role_compatibility(compatibility)
        .build()
        .unwrap();
        std::fs::write(repo.path().join("dirty-before-register.txt"), "dirty").unwrap();
        let dirty_registration = handle_governed_task_request(
            DaemonRequest::RegisterGovernedTask {
                registration: registration.clone(),
            },
            &state,
        )
        .await;
        assert!(error_from_response(dirty_registration).contains("dirty"));
        std::fs::remove_file(repo.path().join("dirty-before-register.txt")).unwrap();
        let registered = task_from_response(
            handle_governed_task_request(
                DaemonRequest::RegisterGovernedTask { registration },
                &state,
            )
            .await,
        );
        assert_eq!(
            registered.initial_subject_revision.as_deref(),
            Some(oid.as_str())
        );

        let running = task_from_response(
            handle_governed_task_request(
                DaemonRequest::MutateGovernedTask {
                    request: GovernedTaskMutationRequest {
                        request_id: request_id("running-producer-flow"),
                        project_id: project_id.clone(),
                        task_id: registered.id.clone(),
                        expected_revision: registered.revision,
                        mutation: GovernedTaskMutation::MarkRunning {
                            actor: GovernedActor {
                                kind: GovernedActorKind::System,
                                id: "desktop-runtime".to_string(),
                            },
                        },
                    },
                },
                &state,
            )
            .await,
        );
        assert_eq!(running.execution_state, GovernedExecutionState::Running);

        let fabricated = handle_governed_task_request(
            DaemonRequest::MutateGovernedTask {
                request: GovernedTaskMutationRequest {
                    request_id: request_id("fabricated-claim"),
                    project_id: project_id.clone(),
                    task_id: running.id.clone(),
                    expected_revision: running.revision,
                    mutation: GovernedTaskMutation::SubmitClaim {
                        claim: WorkerCompletionClaimInput {
                            actor: GovernedActor {
                                kind: GovernedActorKind::Worker,
                                id: "forged".to_string(),
                            },
                            summary: "forged".to_string(),
                            subject_revision: oid.clone(),
                            artifact_ids: Vec::new(),
                            diff_ref: None,
                        },
                    },
                },
            },
            &state,
        )
        .await;
        assert!(error_from_response(fabricated).contains("daemon-owned"));

        let claim_request = GovernedClaimRequest {
            request_id: request_id("claim-producer-flow"),
            project_id: project_id.clone(),
            task_id: running.id.clone(),
            expected_revision: running.revision,
            summary: "fixture implementation is complete".to_string(),
            artifact_ids: vec!["artifact-1".to_string()],
        };
        std::fs::write(repo.path().join("dirty.txt"), "dirty").unwrap();
        let dirty = handle_governed_producer_request(
            DaemonRequest::SubmitGovernedClaim {
                request: claim_request.clone(),
            },
            &state,
        )
        .await;
        assert!(error_from_response(dirty).contains("dirty"));
        assert_eq!(
            state
                .get_governed_task(&project_id, &running.id)
                .unwrap()
                .unwrap()
                .revision,
            running.revision
        );
        std::fs::remove_file(repo.path().join("dirty.txt")).unwrap();

        let claimed = task_from_response(
            handle_governed_producer_request(
                DaemonRequest::SubmitGovernedClaim {
                    request: claim_request.clone(),
                },
                &state,
            )
            .await,
        );
        let claim = claimed.latest_claim().unwrap();
        assert_eq!(claim.actor.kind, GovernedActorKind::Worker);
        assert_eq!(claim.actor.id, "worker-1");
        assert_eq!(claim.subject_revision, oid);
        assert_eq!(
            claimed.review_state,
            GovernedReviewState::AwaitingVerification
        );

        let fabricated_verification = handle_governed_task_request(
            DaemonRequest::MutateGovernedTask {
                request: GovernedTaskMutationRequest {
                    request_id: request_id("fabricated-verification"),
                    project_id: project_id.clone(),
                    task_id: claimed.id.clone(),
                    expected_revision: claimed.revision,
                    mutation: GovernedTaskMutation::RecordVerification {
                        verification: GovernedVerificationInput {
                            actor: GovernedActor {
                                kind: GovernedActorKind::Verifier,
                                id: "forged-verifier".to_string(),
                            },
                            claim_id: claim.id.clone(),
                            subject_revision: claim.subject_revision.clone(),
                            policy: "forged".to_string(),
                            outcome: GovernedVerificationOutcome::Passed,
                            commands: vec![GovernedCommandEvidence {
                                name: "forged".to_string(),
                                executable: "true".to_string(),
                                redacted_args: Vec::new(),
                                command_digest: format!("sha256:{}", "a".repeat(64)),
                                exit_code: Some(0),
                                success: true,
                                output_digest: format!("sha256:{}", "b".repeat(64)),
                                output_ref: None,
                                output_bytes: 0,
                                output_truncated: false,
                            }],
                            artifact_ids: Vec::new(),
                            notes: None,
                        },
                    },
                },
            },
            &state,
        )
        .await;
        assert!(error_from_response(fabricated_verification).contains("daemon-owned"));

        std::fs::write(repo.path().join("dirty.txt"), "dirty after receipt").unwrap();
        let replayed_claim = task_from_response(
            handle_governed_producer_request(
                DaemonRequest::SubmitGovernedClaim {
                    request: claim_request.clone(),
                },
                &state,
            )
            .await,
        );
        assert_eq!(replayed_claim, claimed);
        let mut changed_claim = claim_request;
        changed_claim.summary = "changed replay payload".to_string();
        let changed = handle_governed_producer_request(
            DaemonRequest::SubmitGovernedClaim {
                request: changed_claim,
            },
            &state,
        )
        .await;
        assert!(error_from_response(changed).contains("changed its summary"));
        std::fs::remove_file(repo.path().join("dirty.txt")).unwrap();

        let verification_request = GovernedVerificationRequest {
            request_id: request_id("verify-producer-flow"),
            project_id: project_id.clone(),
            task_id: claimed.id.clone(),
            expected_revision: claimed.revision,
        };
        let first_state = state.clone();
        let second_state = state.clone();
        let first_request = verification_request.clone();
        let second_request = verification_request.clone();
        let (first_response, second_response) = tokio::join!(
            async move {
                handle_governed_producer_request(
                    DaemonRequest::RunGovernedVerification {
                        request: first_request,
                    },
                    &first_state,
                )
                .await
            },
            async move {
                handle_governed_producer_request(
                    DaemonRequest::RunGovernedVerification {
                        request: second_request,
                    },
                    &second_state,
                )
                .await
            }
        );
        let verified = task_from_response(first_response);
        assert_eq!(task_from_response(second_response), verified);
        assert_eq!(
            crate::governed_producers::verification_execution_count(&verified.id),
            1
        );
        let verification = verified.latest_verification().unwrap();
        assert_eq!(verification.commands.len(), 4);
        assert!(verification.commands.iter().all(|command| command.success));
        assert_eq!(
            verified.review_state,
            GovernedReviewState::AwaitingSupervisor
        );

        std::fs::write(repo.path().join("dirty.txt"), "dirty after verify").unwrap();
        let replayed_verification = task_from_response(
            handle_governed_producer_request(
                DaemonRequest::RunGovernedVerification {
                    request: verification_request,
                },
                &state,
            )
            .await,
        );
        assert_eq!(replayed_verification, verified);
        let invalid_repeat = handle_governed_producer_request(
            DaemonRequest::RunGovernedVerification {
                request: GovernedVerificationRequest {
                    request_id: request_id("verify-invalid-repeat"),
                    project_id: project_id.clone(),
                    task_id: verified.id.clone(),
                    expected_revision: verified.revision,
                },
            },
            &state,
        )
        .await;
        assert!(error_from_response(invalid_repeat).contains("awaiting-verification"));
        std::fs::remove_file(repo.path().join("dirty.txt")).unwrap();

        let fabricated_verdict = handle_governed_task_request(
            DaemonRequest::MutateGovernedTask {
                request: GovernedTaskMutationRequest {
                    request_id: request_id("fabricated-verdict"),
                    project_id: project_id.clone(),
                    task_id: verified.id.clone(),
                    expected_revision: verified.revision,
                    mutation: GovernedTaskMutation::RecordSupervisorVerdict {
                        verdict: SupervisorVerdictInput {
                            actor: GovernedActor {
                                kind: GovernedActorKind::Supervisor,
                                id: "forged-supervisor".to_string(),
                            },
                            verification_id: verification.id.clone(),
                            verdict: SupervisorVerdictKind::RecommendAccept,
                            rationale: "forged".to_string(),
                        },
                    },
                },
            },
            &state,
        )
        .await;
        assert!(error_from_response(fabricated_verdict).contains("daemon-owned"));

        let calls = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let agent =
            crate::agent::ImpulseAgent::with_test_provider(Box::new(BoundSupervisorProvider {
                calls: calls.clone(),
                requests: requests.clone(),
            }));
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));
        let telemetry = Arc::new(tokio::sync::RwLock::new(
            crate::ops_workbench::TerminalOpsTelemetryStore::default(),
        ));
        let permissions = Arc::new(tokio::sync::RwLock::new(None));
        let review_request = GovernedSupervisorReviewRequest {
            request_id: request_id("review-producer-flow"),
            project_id: project_id.clone(),
            task_id: verified.id.clone(),
            expected_revision: verified.revision,
        };
        let reviewed = task_from_response(
            handle_supervisor_request(
                DaemonRequest::RunGovernedSupervisorReview {
                    request: review_request.clone(),
                },
                &state,
                &telemetry,
                &permissions,
                &cached_agent,
            )
            .await,
        );
        assert_eq!(reviewed.review_state, GovernedReviewState::AwaitingOperator);
        let supervisor_actor_id = &reviewed.latest_supervisor_verdict().unwrap().actor.id;
        assert!(supervisor_actor_id
            .starts_with("impulse-agent:api:anthropic:bound-supervisor-model:sha256-"));
        assert_eq!(supervisor_actor_id.rsplit('-').next().unwrap().len(), 64);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        {
            let provider_requests = requests.lock().unwrap();
            assert_eq!(provider_requests.len(), 1);
            assert_eq!(provider_requests[0].messages.len(), 2);
            assert_eq!(provider_requests[0].model, "bound-supervisor-model");
            assert!(provider_requests[0].tools.is_empty());
            assert_eq!(provider_requests[0].temperature, 0.0);
        }
        assert!(cached_agent
            .lock()
            .await
            .as_ref()
            .unwrap()
            .session_history()
            .is_empty());

        let replayed_review = task_from_response(
            handle_supervisor_request(
                DaemonRequest::RunGovernedSupervisorReview {
                    request: review_request,
                },
                &state,
                &telemetry,
                &permissions,
                &cached_agent,
            )
            .await,
        );
        assert_eq!(replayed_review, reviewed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let accepted = task_from_response(
            handle_governed_task_request(
                DaemonRequest::MutateGovernedTask {
                    request: GovernedTaskMutationRequest {
                        request_id: request_id("operator-accept-producer-flow"),
                        project_id: project_id.clone(),
                        task_id: reviewed.id.clone(),
                        expected_revision: reviewed.revision,
                        mutation: GovernedTaskMutation::RecordOperatorDecision {
                            decision: OperatorDecisionInput {
                                actor: GovernedActor {
                                    kind: GovernedActorKind::Operator,
                                    id: "local-operator-test".to_string(),
                                },
                                supervisor_verdict_id: reviewed
                                    .latest_supervisor_verdict()
                                    .unwrap()
                                    .id
                                    .clone(),
                                decision: OperatorDecisionKind::Approve,
                                rationale: "verified evidence accepted".to_string(),
                            },
                        },
                    },
                },
                &state,
            )
            .await,
        );
        assert_eq!(accepted.review_state, GovernedReviewState::Accepted);

        let reloaded = crate::state::State::new(repo.path().join(".impulse")).unwrap();
        let persisted = reloaded
            .get_governed_task(&project_id, &accepted.id)
            .unwrap()
            .unwrap();
        assert_eq!(persisted, accepted);
    }
}
