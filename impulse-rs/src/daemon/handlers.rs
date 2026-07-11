//! Daemon request handlers — dispatches each [`DaemonRequest`] variant to focused sub-handlers.
//!
//! Contains the main `process_request` dispatcher plus grouped handlers for session,
//! chat, tool, steward, ops, supervisor, agent, guard, plugin, and debug requests.

use std::sync::Arc;

use anyhow::Result;
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

        // Supervisor group
        DaemonRequest::GetSupervisorPermissions
        | DaemonRequest::SupervisorChat { .. }
        | DaemonRequest::RunSupervisorAction { .. } => {
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

/// Check the daemon-level agent cache out for the duration of a single
/// request, lazily initializing it from config on first use, and release
/// the lock immediately rather than holding it across the caller's
/// subsequent `.await` on the agent.
///
/// **Why this exists (freeze bug, same-day Opus sweep):** the previous
/// version of this helper returned a live `MutexGuard<Option<ImpulseAgent>>`
/// that callers kept in scope across `agent.query(...).await`. `query()`
/// eventually reaches `harness::harness_query_structured`'s
/// `tokio::process::Command::output().await` in harness mode (`agent/mod.rs`),
/// which had no timeout — if the spawned `claude`/`codex`/`gemini` CLI hung
/// (network stall, waiting on stdin, an auth prompt), that `.await` never
/// returned, and the still-held `MutexGuard` meant the mutex itself was
/// locked indefinitely. Every *other* agent-related daemon request
/// (`SupervisorChat`, `AgentAssist`, `AgentReviewCode`/`AgentAnalyzeError`/
/// `AgentSummarizePane`) calls this same helper and would then block on
/// `cached_agent.lock().await` forever — one wedged child process froze the
/// entire daemon's agent IPC surface, not just the one hung request.
///
/// This helper now `take()`s the `ImpulseAgent` out of the `Option` and
/// returns it *owned*, dropping the `MutexGuard` before returning. Callers
/// run their (possibly slow) query on the owned value with no lock held,
/// then must call [`checkin_agent`] to put it back — see that function's
/// doc for why every exit path, including error/not-ready paths, must call
/// it. Paired with `agent/mod.rs`'s new subprocess timeout, a hung harness
/// now blocks at most one request for a bounded duration instead of
/// freezing the daemon indefinitely.
///
/// **Accepted tradeoff (fresh Opus review, same day):** while the cache
/// slot is empty (between one request's checkout and its checkin), a
/// concurrent request sees `None` here and re-initializes a second, fully
/// independent `ImpulseAgent` with empty history rather than waiting. Both
/// requests' agents check in afterward via plain `*guard = Some(agent)` —
/// last write wins. This can discard the *original* agent's accumulated
/// session history, not just the freshly-reinitialized one's (empty)
/// history: if the reinitialized instance happens to check in after the
/// original, the original's history is the one silently lost. Accepted
/// because `session_history` is in-memory, already bounded/truncated, and
/// already lost on daemon restart — the alternative (serializing every
/// agent request on this mutex across slow LLM/subprocess calls) is
/// exactly the freeze this fix exists to prevent.
async fn checkout_agent(
    cached_agent: &tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>,
    state: &SharedState,
) -> Option<crate::agent::ImpulseAgent> {
    let mut guard = cached_agent.lock().await;
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
    guard.take()
}

/// Put an agent checked out via [`checkout_agent`] back into the daemon
/// cache, re-acquiring the mutex only for the instant it takes to store it.
///
/// Must be called on **every** exit path after a successful `checkout_agent`
/// — including "not configured"/"not ready" early returns and query
/// errors — not just the success path. Skipping it on any branch would
/// silently drop the cached agent (losing session history and forcing a
/// full re-init, including re-resolving config, on the next request) or, if
/// skipped on a hot path repeatedly, leave the cache permanently `None`.
async fn checkin_agent(
    cached_agent: &tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>,
    agent: crate::agent::ImpulseAgent,
) {
    let mut guard = cached_agent.lock().await;
    *guard = Some(agent);
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
            let mut agent = match checkout_agent(cached_agent, state).await {
                Some(a) => a,
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
                checkin_agent(cached_agent, agent).await;
                return DaemonResponse::Ok {
                    result: serde_json::to_value(fallback)
                        .unwrap_or_else(|_| serde_json::json!({})),
                };
            }

            let full_prompt =
                build_supervisor_prompt(&snapshot, &permission_state, &prompt, context.as_deref());

            // The mutex is NOT held across this `.await` — `agent` is an
            // owned value checked out above, so a slow/hung harness
            // subprocess here blocks only this request, not every other
            // agent-related daemon request. See `checkout_agent`'s doc.
            let query_result = agent
                .query(crate::agent::prompts::SUPERVISOR_SYSTEM, &full_prompt)
                .await;

            checkin_agent(cached_agent, agent).await;

            match query_result {
                Ok(response) => {
                    let parsed = parse_supervisor_chat_response(&response, &permission_state);
                    respond_ok(&parsed)
                }
                Err(e) => respond_err(format!("Supervisor chat failed: {}", e)),
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

    let mut agent = match checkout_agent(cached_agent, state).await {
        Some(a) => a,
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
        checkin_agent(cached_agent, agent).await;
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
    // with structured cross-pane context from the context lifecycle. Neither
    // branch holds the cache mutex across its `.await` -- `agent` is owned,
    // checked out above. See `checkout_agent`'s doc for why this matters.
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

    checkin_agent(cached_agent, agent).await;

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
    let mut agent = match checkout_agent(cached_agent, state).await {
        Some(a) => a,
        None => {
            return respond_err(
                "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY",
            )
        }
    };

    if !agent.is_ready() {
        checkin_agent(cached_agent, agent).await;
        return respond_err(
            "Impulse Agent is configured but not ready (check API key or harness installation)",
        );
    }

    // None of the branches below hold the cache mutex across their
    // `.await` -- `agent` is an owned value checked out above via
    // `checkout_agent`, and is checked back in once the match completes
    // (success or error). See `checkout_agent`'s doc for why this matters.
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

    checkin_agent(cached_agent, agent).await;

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
mod agent_lock_release_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{checkin_agent, checkout_agent};

    fn test_state() -> (tempfile::TempDir, crate::state::SharedState) {
        let tmp = tempfile::TempDir::new().unwrap();
        let st = crate::state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    /// Regression test for the freeze bug (same-day Opus sweep): before the
    /// fix, `get_or_init_agent` returned a live `MutexGuard` that daemon
    /// handlers kept in scope across `agent.query(...).await`, so the
    /// `cached_agent` mutex stayed locked for the query's entire duration.
    /// A slow-but-not-hung query (or a genuinely hung one) meant every
    /// *other* agent-related request would block on
    /// `cached_agent.lock().await` until that one query finished. This test
    /// proves `checkout_agent`/`checkin_agent` no longer has that shape: the
    /// mutex is only held for the instant it takes to check the agent out,
    /// not across the caller's subsequent work with it.
    ///
    /// Exercises the locking primitive directly (`checkout_agent`/
    /// `checkin_agent`) rather than a full `handle_agent_request` +
    /// real/fake harness subprocess round trip -- the subprocess timeout
    /// itself is covered separately by
    /// `agent::tests::test_harness_query_times_out_instead_of_hanging_forever`;
    /// what matters here is specifically that the *mutex* isn't held across
    /// whatever the caller does with the checked-out agent, which is a
    /// property of these two functions independent of what runs in between.
    #[tokio::test]
    async fn test_checkout_agent_releases_lock_before_a_slow_query_completes() {
        let (_tmp, state) = test_state();
        let agent = crate::agent::ImpulseAgent::new(crate::agent::ImpulseAgentConfig::default())
            .expect("disabled-mode agent should construct");
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));

        let cached_agent_a = cached_agent.clone();
        let state_a = state.clone();
        let task_a = tokio::spawn(async move {
            let agent = checkout_agent(cached_agent_a.as_ref(), &state_a)
                .await
                .expect("agent should be checked out");
            // Simulate a slow-but-not-hung query in progress. The mutex
            // must NOT be held across this sleep -- the pre-fix code held
            // the equivalent `MutexGuard` across `agent.query(...).await`
            // for exactly this long.
            tokio::time::sleep(Duration::from_millis(300)).await;
            checkin_agent(cached_agent_a.as_ref(), agent).await;
        });

        // Give task A time to complete its checkout (and start its
        // simulated slow query) before task B tries its own checkout.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let start = std::time::Instant::now();
        let checked_out_b = checkout_agent(cached_agent.as_ref(), &state).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(250),
            "checkout_agent blocked for {elapsed:?} waiting on task A's in-flight query -- \
             the cache mutex must be released before the query runs, not held across it"
        );
        // Task A already took the only cached agent out, so B's cache slot
        // is empty; `state`'s config has no provider/harness configured, so
        // `resolve_from_config` legitimately returns `None` here rather
        // than fabricating a second agent. This is the documented tradeoff
        // of this locking shape (see `checkout_agent`'s doc): concurrent
        // requests get independent agent instances rather than serializing
        // on one -- whichever instance checks in LAST wins the cache slot,
        // which can discard the OTHER instance's session history (not just
        // a fresh instance's empty history) when they race.
        assert!(checked_out_b.is_none());

        task_a.await.expect("task A should complete");
        assert!(
            cached_agent.lock().await.is_some(),
            "task A must have checked its agent back in"
        );
    }
}
