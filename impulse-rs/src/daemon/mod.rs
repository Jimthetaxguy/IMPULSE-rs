//! Daemon IPC server — long-running Unix socket process.
//!
//! Accepts JSON-line messages over [`DaemonRequest`] / [`DaemonResponse`] protocol.
//! Owns in-memory [`crate::state::State`] with dirty-flag sync. Handles session
//! lifecycle, file tracking, conflict detection, chat, and tool invocation.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Notify, RwLock};

use crate::injection::{run_injection, InjectionMode, InjectionSurface};
use crate::llm_backends::{AnthropicProvider, ChatRequest, LlmProvider, Message, Role};
use crate::state::SharedState;

const SOCKET_NAME: &str = "impulse.sock";

fn build_remote_tool_context(
    impulse_dir: &std::path::Path,
    config: &crate::state::Config,
) -> crate::tooling::ToolContext {
    crate::handlers::build_tool_context(
        impulse_dir,
        config,
        crate::tooling::ExecutionOrigin::Daemon,
        false,
        None,
    )
}

pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub state: SharedState,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    Ping,
    Status,
    CreateSession {
        name: String,
        platform: Option<String>,
    },
    EndSession {
        session_id: String,
        summary: String,
    },
    TrackFile {
        session_id: String,
        file_path: String,
    },
    TrackTool {
        session_id: String,
        tool_name: String,
    },
    GetSession {
        session_id: String,
    },
    ListSessions,
    Chat {
        session_id: String,
        message: String,
        #[serde(default)]
        inject_mode: Option<String>,
        #[serde(default)]
        inject_explain: bool,
    },
    StewardStatus,
    StewardProposals {
        action: String,
        id: Option<String>,
    },
    StewardMemory,
    /// List all available tools (for agent discovery)
    ListTools {
        #[serde(default)]
        category: Option<String>,
    },
    /// Get a tool's descriptor (params, capabilities)
    DescribeTool {
        name: String,
    },
    /// Invoke a tool by name with JSON params
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Export tool schemas in Claude tool-calling format
    ToolSchema,
    /// Fetch the workbench snapshot used by the egui operator console
    GetOpsSnapshot,
    /// Poll for workbench events and a reconciled snapshot
    SubscribeOps {
        #[serde(default)]
        since_seq: Option<u64>,
    },
    /// Publish live terminal telemetry from the egui workbench
    PublishTerminalOps {
        report: impulse_ops::TerminalOpsReport,
    },
    /// Read the effective supervisor permissions for the egui control plane
    GetSupervisorPermissions,
    /// Structured supervisor chat for the egui control plane
    SupervisorChat {
        prompt: String,
        context: Option<String>,
    },
    /// Run a structured supervisor action with daemon-side policy enforcement
    RunSupervisorAction {
        action: impulse_ops::SupervisorAction,
    },
    /// List project-scoped artifacts for the egui workbench
    ListArtifacts {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Get a single project artifact by ID
    GetArtifact {
        artifact_id: String,
    },
    /// Run an artifact action for the egui workbench
    RunArtifactAction {
        artifact_id: String,
        action_id: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Request AI coordination assistance via the Impulse Agent
    AgentAssist {
        prompt: String,
        context: Option<String>,
    },
    /// Evaluate an action against guardrail rules
    GuardEvaluate {
        target: String,
        action: String,
    },
    /// List active guardrail rules
    GuardList,
    /// Check if a file is being modified by another session
    CheckConflict {
        session_id: String,
        file_path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonResponse {
    Ok {
        result: serde_json::Value,
    },
    Error {
        message: String,
    },
    AgentAssistResult {
        success: bool,
        response: String,
    },
    ConflictCheck {
        has_conflict: bool,
        conflicting_sessions: Vec<String>,
    },
}

pub struct Daemon {
    config: DaemonConfig,
    shutdown_flag: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    tool_registry: Arc<crate::tooling::ToolRegistry>,
    tool_context: crate::tooling::ToolContext,
    terminal_telemetry: Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
}

impl Daemon {
    pub fn new(state: SharedState) -> Self {
        let socket_path = state
            .storage()
            .base_path()
            .join("sockets")
            .join(SOCKET_NAME);
        let project_root = state
            .storage()
            .base_path()
            .parent()
            .unwrap_or_else(|| state.storage().base_path());
        let config_snapshot = state.config_snapshot().unwrap_or_default();
        let external_tools_dir = config_snapshot.resolved_external_tools_dir_from(project_root);
        let tool_registry = crate::tooling::ToolRegistry::with_runtime(
            state.storage().base_path(),
            &external_tools_dir,
        )
        .unwrap_or_else(|_| crate::tooling::ToolRegistry::with_defaults());
        if let Err(err) = crate::agent_discovery::write_capabilities_manifest(
            state.storage().base_path(),
            &tool_registry,
        ) {
            tracing::warn!("failed to refresh capabilities manifest: {}", err);
        }
        let tool_context = build_remote_tool_context(state.storage().base_path(), &config_snapshot);

        Self {
            config: DaemonConfig {
                socket_path: socket_path.clone(),
                state,
            },
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
            tool_registry: Arc::new(tool_registry),
            tool_context,
            terminal_telemetry: Arc::new(RwLock::new(
                crate::ops_workbench::TerminalOpsTelemetryStore::default(),
            )),
            supervisor_session_override: Arc::new(RwLock::new(None)),
        }
    }

    #[allow(dead_code)]
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    pub async fn start(&self) -> Result<()> {
        let socket_dir = self
            .config
            .socket_path
            .parent()
            .context("Invalid socket path")?;
        tokio::fs::create_dir_all(socket_dir)
            .await
            .context("Failed to create socket directory")?;

        if let Err(e) = tokio::fs::remove_file(&self.config.socket_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e).context("Failed to remove old socket");
            }
        }

        let listener =
            UnixListener::bind(&self.config.socket_path).context("Failed to bind socket")?;

        println!("Daemon listening on {}", self.config.socket_path.display());

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state = self.config.state.clone();
                            let shutdown = self.shutdown_flag.clone();
                            let notify = self.shutdown_notify.clone();
                            let registry = self.tool_registry.clone();
                            let tool_context = self.tool_context.clone();
                            let terminal_telemetry = self.terminal_telemetry.clone();
                            let supervisor_session_override =
                                self.supervisor_session_override.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(
                                    stream,
                                    state,
                                    shutdown,
                                    notify,
                                    registry,
                                    tool_context,
                                    terminal_telemetry,
                                    supervisor_session_override,
                                )
                                .await
                                {
                                    eprintln!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error: {}", e);
                        }
                    }
                }
                _ = self.shutdown_notify.notified() => {
                    println!("Shutting down daemon...");
                    break;
                }
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        self.shutdown_notify.notify_waiters();

        let _ = tokio::fs::remove_file(&self.config.socket_path).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
    _notify: Arc<Notify>,
    registry: Arc<crate::tooling::ToolRegistry>,
    tool_context: crate::tooling::ToolContext,
    terminal_telemetry: Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024; // 10MB limit per request
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 0 {
        if line.len() > MAX_REQUEST_SIZE {
            let err_response = DaemonResponse::Error {
                message: format!(
                    "Request too large ({} bytes, max {})",
                    line.len(),
                    MAX_REQUEST_SIZE
                ),
            };
            let response_json = serde_json::to_string(&err_response)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            line.clear();
            continue;
        }
        let request: DaemonRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = DaemonResponse::Error {
                    message: format!("Failed to parse request: {}", e),
                };
                writer
                    .write_all(serde_json::to_string(&response)?.as_bytes())
                    .await?;
                writer.write_all(b"\n").await?;
                line.clear();
                continue;
            }
        };

        let response = process_request(
            request,
            state.clone(),
            &registry,
            &tool_context,
            &terminal_telemetry,
            &supervisor_session_override,
        )
        .await;

        writer
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        line.clear();

        if shutdown.load(Ordering::SeqCst) {
            break;
        }
    }

    Ok(())
}

async fn load_terminal_reports(
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
) -> Vec<impulse_ops::TerminalOpsReport> {
    let project = crate::ops_workbench::project_summary(state);
    terminal_telemetry
        .write()
        .await
        .fresh_reports(&project.id, chrono::Utc::now())
}

async fn build_ops_snapshot(
    state: &SharedState,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
) -> Result<impulse_ops::ProjectOpsSnapshot> {
    let reports = load_terminal_reports(state, terminal_telemetry).await;
    crate::ops_workbench::build_snapshot(state, &reports).await
}

#[derive(Debug, Deserialize)]
struct ParsedSupervisorChatResponse {
    response: String,
    #[serde(default)]
    proposals: Vec<impulse_ops::SupervisorProposal>,
}

async fn build_supervisor_permission_state(
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

#[allow(clippy::too_many_arguments)]
fn save_supervisor_artifact(
    state: &SharedState,
    project_id: &str,
    agent_id: &str,
    kind: &str,
    title: String,
    summary: String,
    payload: serde_json::Value,
    related_files: Vec<impulse_ops::ArtifactFileRef>,
    actions: Vec<impulse_ops::ArtifactAction>,
) -> Result<String> {
    let artifact_id = impulse_ops::sanitize_id(&format!(
        "{}-{}-{}",
        kind,
        agent_id,
        chrono::Utc::now().timestamp_millis()
    ));
    let artifact = impulse_ops::ArtifactEnvelope {
        id: artifact_id.clone(),
        project_id: project_id.to_string(),
        agent_id: agent_id.to_string(),
        session_id: None,
        kind: kind.to_string(),
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
                &project.id,
                "impulse-supervisor",
                "context_cleanup_review",
                format!("Cleanup Review: {}", target.label),
                format!("Reviewable cleanup context prepared for {}", target.label),
                serde_json::json!({
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
                Vec::new(),
                vec![
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
                &project.id,
                "impulse-supervisor",
                "handoff_review",
                format!("Handoff: {}", target_tool),
                format!("Supervisor handoff prepared for {}", target_tool),
                serde_json::json!({
                    "markdown": markdown,
                    "target_tool": target_tool,
                    "task": task,
                    "notes": notes,
                    "source_path": handoff_path.display().to_string(),
                }),
                vec![impulse_ops::ArtifactFileRef {
                    path: handoff_path.display().to_string(),
                    label: Some("Generated handoff file".to_string()),
                }],
                vec![
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
                &project.id,
                "impulse-supervisor",
                "permission_change",
                "Supervisor Permission Change".to_string(),
                format!("Supervisor permissions updated for {:?}", scope),
                serde_json::json!({
                    "scope": scope,
                    "allowed_actions": next_state.effective.allowed_actions,
                    "allowed_tool_capabilities": next_state.effective.allowed_tool_capabilities,
                    "require_confirmation_actions": next_state.effective.require_confirmation_actions,
                }),
                Vec::new(),
                vec![
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

async fn process_request(
    request: DaemonRequest,
    state: SharedState,
    registry: &crate::tooling::ToolRegistry,
    tool_context: &crate::tooling::ToolContext,
    terminal_telemetry: &Arc<RwLock<crate::ops_workbench::TerminalOpsTelemetryStore>>,
    supervisor_session_override: &Arc<RwLock<Option<impulse_ops::SupervisorPermissionPolicy>>>,
) -> DaemonResponse {
    match request {
        DaemonRequest::Ping => DaemonResponse::Ok {
            result: serde_json::json!({"pong": true}),
        },

        DaemonRequest::Status => match state.list_sessions().await {
            Ok(sessions) => DaemonResponse::Ok {
                result: serde_json::json!({
                    "sessions": sessions.len(),
                    "active": sessions.iter().filter(|s| s.status == crate::state::SessionStatus::Active).count()
                }),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::CreateSession { name, platform } => {
            let platform = platform.and_then(|p| crate::state::Platform::from_str_name(&p));

            match state.create_session(name, platform).await {
                Ok(session) => DaemonResponse::Ok {
                    result: serde_json::json!({
                        "session_id": session.id,
                        "name": session.name
                    }),
                },
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
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
            Ok(None) => DaemonResponse::Error {
                message: "Session not found".to_string(),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::TrackFile {
            session_id,
            file_path,
        } => match state.track_file(&session_id, &file_path).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::TrackTool {
            session_id,
            tool_name,
        } => match state.track_tool(&session_id, &tool_name).await {
            Ok(_) => DaemonResponse::Ok {
                result: serde_json::json!({"tracked": true}),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::CheckConflict {
            session_id,
            file_path,
        } => match state.check_file_conflict(&session_id, &file_path).await {
            Ok(conflicting) => DaemonResponse::ConflictCheck {
                has_conflict: !conflicting.is_empty(),
                conflicting_sessions: conflicting,
            },
            Err(e) => DaemonResponse::Error {
                message: format!("Conflict check failed: {}", e),
            },
        },

        DaemonRequest::GetSession { session_id } => match state.get_session(&session_id).await {
            Ok(Some(session)) => match serde_json::to_value(session) {
                Ok(result) => DaemonResponse::Ok { result },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to serialize session: {}", e),
                },
            },
            Ok(None) => DaemonResponse::Error {
                message: "Session not found".to_string(),
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::ListSessions => match state.list_sessions().await {
            Ok(sessions) => match serde_json::to_value(sessions) {
                Ok(result) => DaemonResponse::Ok { result },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to serialize sessions: {}", e),
                },
            },
            Err(e) => DaemonResponse::Error {
                message: e.to_string(),
            },
        },

        DaemonRequest::Chat {
            session_id,
            message,
            inject_mode,
            inject_explain,
        } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
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
                return DaemonResponse::Error {
                    message: "ANTHROPIC_API_KEY or CLAUDE_API_KEY not set".to_string(),
                };
            }

            // Build session context from state
            let session_context = state.get_session(&session_id).await.ok().flatten();

            // Create context prompt with session history
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
                return DaemonResponse::Error {
                    message: "Invalid inject_mode. Use off|review|apply".to_string(),
                };
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
            let model = std::env::var("IMPULSE_MODEL")
                .or_else(|_| std::env::var("COCKPIT_MODEL"))
                .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
            let request = ChatRequest {
                model,
                messages: vec![Message {
                    role: Role::User,
                    content: context_prompt,
                }],
                temperature: 0.7,
                max_tokens: Some(4096),
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
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        DaemonRequest::StewardStatus => {
            use crate::stewardship;

            let base = state.storage().base_path();
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
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
            use crate::stewardship;

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
                    Err(e) => DaemonResponse::Error {
                        message: e.to_string(),
                    },
                },
                "approve" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => {
                            return DaemonResponse::Error {
                                message: "id required for approve".to_string(),
                            }
                        }
                    };
                    match stewardship::approval::approve_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"approved": pid}),
                        },
                        Ok(false) => DaemonResponse::Error {
                            message: format!("Proposal {} not found", pid),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                "reject" => {
                    let pid = match id {
                        Some(pid) => pid,
                        None => {
                            return DaemonResponse::Error {
                                message: "id required for reject".to_string(),
                            }
                        }
                    };
                    match stewardship::approval::reject_proposal(base, &pid) {
                        Ok(true) => DaemonResponse::Ok {
                            result: serde_json::json!({"rejected": pid}),
                        },
                        Ok(false) => DaemonResponse::Error {
                            message: format!("Proposal {} not found", pid),
                        },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                _ => DaemonResponse::Error {
                    message: format!("Unknown action: {}. Use list, approve, reject", action),
                },
            }
        }

        DaemonRequest::ListTools { category } => {
            let descriptors = if let Some(cat) = category {
                let cat = match cat.as_str() {
                    "system" => crate::tooling::ToolCategory::System,
                    "utility" => crate::tooling::ToolCategory::Utility,
                    "analysis" => crate::tooling::ToolCategory::Analysis,
                    "document" => crate::tooling::ToolCategory::Document,
                    _ => {
                        return DaemonResponse::Error {
                            message: format!(
                                "Unknown category: {}. Use system, utility, analysis, document",
                                cat
                            ),
                        }
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
            None => DaemonResponse::Error {
                message: format!("Tool not found: {}", name),
            },
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
                Err(e) => DaemonResponse::Error {
                    message: format!("{}", e),
                },
            }
        }

        DaemonRequest::ToolSchema => DaemonResponse::Ok {
            result: serde_json::json!({"tools": registry.schema_json()}),
        },

        DaemonRequest::GetOpsSnapshot => match build_ops_snapshot(&state, terminal_telemetry).await
        {
            Ok(snapshot) => match serde_json::to_value(snapshot) {
                Ok(result) => DaemonResponse::Ok { result },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to serialize ops snapshot: {}", e),
                },
            },
            Err(e) => DaemonResponse::Error {
                message: format!("Failed to build ops snapshot: {}", e),
            },
        },

        DaemonRequest::SubscribeOps { since_seq } => {
            let reports = load_terminal_reports(&state, terminal_telemetry).await;
            match crate::ops_workbench::subscribe_ops(&state, since_seq, &reports).await {
                Ok(subscription) => match serde_json::to_value(subscription) {
                    Ok(result) => DaemonResponse::Ok { result },
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to serialize ops subscription: {}", e),
                    },
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to subscribe ops state: {}", e),
                },
            }
        }

        DaemonRequest::PublishTerminalOps { report } => {
            let project = crate::ops_workbench::project_summary(&state);
            terminal_telemetry
                .write()
                .await
                .publish(&project.id, report);
            DaemonResponse::Ok {
                result: serde_json::json!({"accepted": true}),
            }
        }

        DaemonRequest::GetSupervisorPermissions => {
            match build_supervisor_permission_state(&state, supervisor_session_override).await {
                Ok(permission_state) => match serde_json::to_value(permission_state) {
                    Ok(result) => DaemonResponse::Ok { result },
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to serialize supervisor permission state: {}", e),
                    },
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to resolve supervisor permissions: {}", e),
                },
            }
        }

        DaemonRequest::SupervisorChat { prompt, context } => {
            let snapshot = match build_ops_snapshot(&state, terminal_telemetry).await {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to build supervisor snapshot: {}", e),
                    }
                }
            };
            let permission_state = match build_supervisor_permission_state(
                &state,
                supervisor_session_override,
            )
            .await
            {
                Ok(state) => state,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to resolve supervisor permissions: {}", e),
                    }
                }
            };
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to load config: {}", e),
                    }
                }
            };

            let mut agent = match crate::agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            ) {
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
                return DaemonResponse::Ok {
                    result: serde_json::to_value(fallback)
                        .unwrap_or_else(|_| serde_json::json!({})),
                };
            }

            let full_prompt =
                build_supervisor_prompt(&snapshot, &permission_state, &prompt, context.as_deref());

            match agent
                .query(crate::agent::prompts::SUPERVISOR_SYSTEM, &full_prompt)
                .await
            {
                Ok(response) => {
                    let parsed = parse_supervisor_chat_response(&response, &permission_state);
                    match serde_json::to_value(parsed) {
                        Ok(result) => DaemonResponse::Ok { result },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Failed to serialize supervisor chat result: {}", e),
                        },
                    }
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Supervisor chat failed: {}", e),
                },
            }
        }

        DaemonRequest::RunSupervisorAction { action } => {
            match run_supervisor_action(
                &state,
                action,
                terminal_telemetry,
                supervisor_session_override,
            )
            .await
            {
                Ok(result) => match serde_json::to_value(result) {
                    Ok(result) => DaemonResponse::Ok { result },
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to serialize supervisor action result: {}", e),
                    },
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("Supervisor action failed: {}", e),
                },
            }
        }

        DaemonRequest::ListArtifacts { limit } => {
            match build_ops_snapshot(&state, terminal_telemetry).await {
                Ok(snapshot) => {
                    let mut artifacts = snapshot.artifacts;
                    if let Some(limit) = limit {
                        artifacts.truncate(limit);
                    }
                    match serde_json::to_value(artifacts) {
                        Ok(result) => DaemonResponse::Ok { result },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Failed to serialize artifacts: {}", e),
                        },
                    }
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to list artifacts: {}", e),
                },
            }
        }

        DaemonRequest::GetArtifact { artifact_id } => {
            match build_ops_snapshot(&state, terminal_telemetry).await {
                Ok(snapshot) => match crate::ops_workbench::get_artifact(
                    state.storage().base_path(),
                    &snapshot.project.id,
                    &artifact_id,
                ) {
                    Ok(Some(artifact)) => match serde_json::to_value(artifact) {
                        Ok(result) => DaemonResponse::Ok { result },
                        Err(e) => DaemonResponse::Error {
                            message: format!("Failed to serialize artifact: {}", e),
                        },
                    },
                    Ok(None) => DaemonResponse::Error {
                        message: format!("Artifact not found: {}", artifact_id),
                    },
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to read artifact: {}", e),
                    },
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("Failed to resolve project artifact store: {}", e),
                },
            }
        }

        DaemonRequest::RunArtifactAction {
            artifact_id,
            action_id,
            params,
        } => match build_ops_snapshot(&state, terminal_telemetry).await {
            Ok(snapshot) => match crate::ops_workbench::run_artifact_action(
                state.storage().base_path(),
                &snapshot.project.id,
                &artifact_id,
                &action_id,
                &params,
            ) {
                Ok(result) => match serde_json::to_value(result) {
                    Ok(result) => DaemonResponse::Ok { result },
                    Err(e) => DaemonResponse::Error {
                        message: format!("Failed to serialize artifact action result: {}", e),
                    },
                },
                Err(e) => DaemonResponse::Error {
                    message: format!("Artifact action failed: {}", e),
                },
            },
            Err(e) => DaemonResponse::Error {
                message: format!("Failed to resolve artifact action context: {}", e),
            },
        },

        DaemonRequest::StewardMemory => {
            use crate::stewardship;

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
                Err(e) => DaemonResponse::Error {
                    message: e.to_string(),
                },
            }
        }

        DaemonRequest::AgentAssist { prompt, context } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::AgentAssistResult {
                        success: false,
                        response: format!("Failed to load config: {}", e),
                    }
                }
            };

            let mut agent = match crate::agent::resolve_from_config(
                config.impulse_agent_provider.as_deref(),
                config.impulse_agent_api_key.as_deref(),
                config.impulse_agent_model.as_deref(),
                config.impulse_agent_harness.as_deref(),
            ) {
                Some(a) => a,
                None => {
                    return DaemonResponse::AgentAssistResult {
                        success: false,
                        response: "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY".to_string(),
                    }
                }
            };

            if !agent.is_ready() {
                return DaemonResponse::AgentAssistResult {
                    success: false,
                    response: "Impulse Agent is configured but not ready (check API key or harness installation)".to_string(),
                };
            }

            // Build the full prompt, incorporating optional context
            let full_prompt = match context {
                Some(ctx) => format!("Context:\n{}\n\nRequest:\n{}", ctx, prompt),
                None => prompt,
            };

            match agent
                .query(crate::agent::prompts::COORDINATION_SYSTEM, &full_prompt)
                .await
            {
                Ok(response) => DaemonResponse::AgentAssistResult {
                    success: true,
                    response,
                },
                Err(e) => DaemonResponse::AgentAssistResult {
                    success: false,
                    response: format!("Agent query failed: {}", e),
                },
            }
        }

        DaemonRequest::GuardEvaluate { target, action } => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
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
                Err(e) => DaemonResponse::Error {
                    message: format!("Guardrail evaluation failed: {}", e),
                },
            }
        }

        DaemonRequest::GuardList => {
            let config = match state.config_snapshot() {
                Ok(c) => c,
                Err(e) => {
                    return DaemonResponse::Error {
                        message: format!("Failed to read config: {}", e),
                    }
                }
            };
            let rules = crate::guardrail::list_active_rules(&config.guardrails);
            DaemonResponse::Ok {
                result: serde_json::json!({ "rules": rules }),
            }
        }
    }
}

#[cfg(test)]
mod tests;
