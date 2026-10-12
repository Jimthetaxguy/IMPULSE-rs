use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod agent_registry;
pub mod governed_task;
pub mod memory_candidate;
pub mod operator_capability;
pub mod role_assignment;

/// Shared daemon protocol version for GUI/operator workbench compatibility.
///
/// v7 (ADR-0018) adds the connection-scoped `PresentOperatorCapability`
/// request and the operator-class requirement on `RecordOperatorDecision`.
pub const DAEMON_PROTOCOL_VERSION: u32 = 8;

#[derive(Debug, thiserror::Error)]
pub enum OpsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing artifact parent directory for {0}")]
    MissingParent(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum WorkbenchDaemonRequest {
    Ping,
    Status,
    /// Present this daemon run's operator capability (ADR-0018).
    ///
    /// Connection-scoped: an operator surface sends it once immediately after
    /// connecting, before any request that mints acceptance. Governed panes
    /// never receive the capability, so a launched runtime cannot send this.
    PresentOperatorCapability(crate::operator_capability::OperatorCapabilityPresentation),
    ListSessions,
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
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    ToolSchema,
    GetOpsSnapshot,
    SubscribeOps {
        #[serde(default)]
        since_seq: Option<u64>,
    },
    PublishTerminalOps {
        report: TerminalOpsReport,
    },
    GetSupervisorPermissions,
    SupervisorChat {
        prompt: String,
        context: Option<String>,
    },
    RunSupervisorAction {
        action: SupervisorAction,
    },
    RunArtifactAction {
        artifact_id: String,
        action_id: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    RegisterGovernedTask {
        registration: governed_task::GovernedTaskRegistration,
    },
    GetGovernedTask {
        project_id: String,
        task_id: governed_task::GovernedTaskId,
    },
    ListGovernedTasks {
        project_id: String,
    },
    MutateGovernedTask {
        request: governed_task::GovernedTaskMutationRequest,
    },
    SubmitGovernedClaim {
        request: governed_task::GovernedClaimRequest,
    },
    RunGovernedVerification {
        request: governed_task::GovernedVerificationRequest,
    },
    RunGovernedSupervisorReview {
        request: governed_task::GovernedSupervisorReviewRequest,
    },
    GuardList,
    GetConflictHistory,
    ClearResolvedConflicts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum WorkbenchDaemonResponse {
    Ok {
        result: serde_json::Value,
    },
    Error {
        message: String,
    },
    Busy {
        resource: DaemonBusyResource,
        retry_after_ms: u64,
    },
    ConflictCheck {
        has_conflict: bool,
        conflicting_sessions: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonBusyResource {
    AgentTurn,
}

pub fn sanitize_id(input: &str) -> String {
    let trimmed = input.trim();
    let candidate = trimmed
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>();
    let collapsed = candidate
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "unknown".to_string()
    } else {
        collapsed
    }
}

pub fn atomic_write_path(path: &Path, content: &[u8]) -> Result<(), OpsError> {
    let parent = path
        .parent()
        .ok_or_else(|| OpsError::MissingParent(path.to_path_buf()))?;
    std::fs::create_dir_all(parent)?;
    let tmp_path = parent.join(format!(
        ".tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = File::create(&tmp_path)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub impulse_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MemorySummary {
    pub active_sessions: usize,
    pub history_entries: usize,
    pub genome_decisions: usize,
    pub last_genome_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RetrievalSummary {
    pub mode: String,
    pub backend: String,
    pub vector_enabled: bool,
    pub semantic_strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct InsightRecord {
    pub timestamp: Option<String>,
    pub agent_label: String,
    pub kind: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ContextHealthSummary {
    pub tier: String,
    pub usage_fraction: f32,
    pub estimated_tokens: usize,
    pub window_tokens: usize,
    pub compaction_count: u32,
    pub injection_count: u32,
    pub pending_review_count: usize,
    pub recent_insights: Vec<InsightRecord>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapabilityId {
    FileSystemRead,
    FileSystemWrite,
    Network,
    PythonExec,
    SystemInfo,
}

impl ToolCapabilityId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FileSystemRead => "filesystem_read",
            Self::FileSystemWrite => "filesystem_write",
            Self::Network => "network",
            Self::PythonExec => "python_exec",
            Self::SystemInfo => "system_info",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorActionPermission {
    MonitorAgents,
    FocusAgent,
    OpenReview,
    SearchMemory,
    ModifyPermissions,
    SendInput,
    InjectContext,
    CleanupContext,
    HandoffContext,
}

impl SupervisorActionPermission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MonitorAgents => "monitor_agents",
            Self::FocusAgent => "focus_agent",
            Self::OpenReview => "open_review",
            Self::SearchMemory => "search_memory",
            Self::ModifyPermissions => "modify_permissions",
            Self::SendInput => "send_input",
            Self::InjectContext => "inject_context",
            Self::CleanupContext => "cleanup_context",
            Self::HandoffContext => "handoff_context",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionChangeScope {
    SessionOverride,
    PersistentDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SupervisorPermissionPolicy {
    pub allowed_actions: Vec<SupervisorActionPermission>,
    pub allowed_tool_capabilities: Vec<ToolCapabilityId>,
    pub require_confirmation_actions: Vec<SupervisorActionPermission>,
}

impl Default for SupervisorPermissionPolicy {
    fn default() -> Self {
        Self {
            allowed_actions: vec![
                SupervisorActionPermission::MonitorAgents,
                SupervisorActionPermission::FocusAgent,
                SupervisorActionPermission::OpenReview,
                SupervisorActionPermission::SearchMemory,
                SupervisorActionPermission::ModifyPermissions,
            ],
            allowed_tool_capabilities: vec![
                ToolCapabilityId::FileSystemRead,
                ToolCapabilityId::SystemInfo,
            ],
            require_confirmation_actions: vec![
                SupervisorActionPermission::SendInput,
                SupervisorActionPermission::InjectContext,
                SupervisorActionPermission::CleanupContext,
                SupervisorActionPermission::HandoffContext,
                SupervisorActionPermission::ModifyPermissions,
            ],
        }
    }
}

impl SupervisorPermissionPolicy {
    pub fn allows_action(&self, permission: SupervisorActionPermission) -> bool {
        self.allowed_actions.contains(&permission)
    }

    pub fn allows_tool_capability(&self, capability: ToolCapabilityId) -> bool {
        self.allowed_tool_capabilities.contains(&capability)
    }

    pub fn requires_confirmation(&self, permission: SupervisorActionPermission) -> bool {
        self.require_confirmation_actions.contains(&permission)
    }

    pub fn grant_action(&mut self, permission: SupervisorActionPermission) {
        if !self.allowed_actions.contains(&permission) {
            self.allowed_actions.push(permission);
        }
    }

    pub fn grant_tool_capability(&mut self, capability: ToolCapabilityId) {
        if !self.allowed_tool_capabilities.contains(&capability) {
            self.allowed_tool_capabilities.push(capability);
        }
    }

    pub fn normalize(&mut self) {
        dedupe_vec(&mut self.allowed_actions);
        dedupe_vec(&mut self.allowed_tool_capabilities);
        dedupe_vec(&mut self.require_confirmation_actions);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorPermissionState {
    pub baseline: SupervisorPermissionPolicy,
    #[serde(default)]
    pub session_override: Option<SupervisorPermissionPolicy>,
    pub effective: SupervisorPermissionPolicy,
}

impl SupervisorPermissionState {
    pub fn resolve(
        mut baseline: SupervisorPermissionPolicy,
        session_override: Option<SupervisorPermissionPolicy>,
    ) -> Self {
        baseline.normalize();
        let session_override = session_override.map(|mut policy| {
            policy.normalize();
            policy
        });

        let mut effective = baseline.clone();
        if let Some(override_policy) = &session_override {
            for permission in &override_policy.allowed_actions {
                effective.grant_action(*permission);
            }
            for capability in &override_policy.allowed_tool_capabilities {
                effective.grant_tool_capability(*capability);
            }
            for permission in &override_policy.require_confirmation_actions {
                if !effective.require_confirmation_actions.contains(permission) {
                    effective.require_confirmation_actions.push(*permission);
                }
            }
        }
        effective.normalize();

        Self {
            baseline,
            session_override,
            effective,
        }
    }

    pub fn session_override_active(&self) -> bool {
        self.session_override.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupervisorAction {
    FocusAgent {
        agent_id: String,
        #[serde(default)]
        session_id: Option<String>,
    },
    SendInput {
        agent_id: String,
        #[serde(default)]
        session_id: Option<String>,
        content: String,
        #[serde(default)]
        confirmed: bool,
    },
    InjectContext {
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        query: String,
        #[serde(default)]
        confirmed: bool,
    },
    CleanupContext {
        #[serde(default)]
        agent_id: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        goal: Option<String>,
        #[serde(default)]
        confirmed: bool,
    },
    HandoffContext {
        #[serde(default)]
        session_id: Option<String>,
        target_tool: String,
        task: String,
        #[serde(default)]
        notes: Option<String>,
        #[serde(default)]
        confirmed: bool,
    },
    OpenArtifactReview {
        artifact_id: String,
    },
    SearchMemory {
        query: String,
    },
    ModifyPermissions {
        scope: PermissionChangeScope,
        #[serde(default)]
        grant_actions: Vec<SupervisorActionPermission>,
        #[serde(default)]
        grant_tool_capabilities: Vec<ToolCapabilityId>,
        #[serde(default)]
        confirmed: bool,
    },
    ClearSessionOverride {
        #[serde(default)]
        confirmed: bool,
    },
    ResetBaselinePermissions {
        #[serde(default)]
        confirmed: bool,
    },
}

impl SupervisorAction {
    pub fn permission(&self) -> SupervisorActionPermission {
        match self {
            Self::FocusAgent { .. } => SupervisorActionPermission::FocusAgent,
            Self::SendInput { .. } => SupervisorActionPermission::SendInput,
            Self::InjectContext { .. } => SupervisorActionPermission::InjectContext,
            Self::CleanupContext { .. } => SupervisorActionPermission::CleanupContext,
            Self::HandoffContext { .. } => SupervisorActionPermission::HandoffContext,
            Self::OpenArtifactReview { .. } => SupervisorActionPermission::OpenReview,
            Self::SearchMemory { .. } => SupervisorActionPermission::SearchMemory,
            Self::ModifyPermissions { .. }
            | Self::ClearSessionOverride { .. }
            | Self::ResetBaselinePermissions { .. } => {
                SupervisorActionPermission::ModifyPermissions
            }
        }
    }

    pub fn confirmed(&self) -> bool {
        match self {
            Self::FocusAgent { .. }
            | Self::OpenArtifactReview { .. }
            | Self::SearchMemory { .. } => true,
            Self::SendInput { confirmed, .. }
            | Self::InjectContext { confirmed, .. }
            | Self::CleanupContext { confirmed, .. }
            | Self::HandoffContext { confirmed, .. }
            | Self::ModifyPermissions { confirmed, .. }
            | Self::ClearSessionOverride { confirmed }
            | Self::ResetBaselinePermissions { confirmed } => *confirmed,
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::FocusAgent { agent_id, .. } => format!("focus agent {}", agent_id),
            Self::SendInput {
                agent_id, content, ..
            } => format!("send input to {}: {}", agent_id, content),
            Self::InjectContext {
                agent_id,
                session_id,
                query,
                ..
            } => format!(
                "inject context for {}{}: {}",
                agent_id.as_deref().unwrap_or("active-agent"),
                session_id
                    .as_ref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default(),
                query
            ),
            Self::CleanupContext {
                agent_id,
                session_id,
                goal,
                ..
            } => format!(
                "cleanup context for {}{}{}",
                agent_id.as_deref().unwrap_or("active-agent"),
                session_id
                    .as_ref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default(),
                goal.as_ref()
                    .map(|value| format!(": {}", value))
                    .unwrap_or_default()
            ),
            Self::HandoffContext {
                session_id,
                target_tool,
                task,
                ..
            } => format!(
                "handoff context to {}{}: {}",
                target_tool,
                session_id
                    .as_ref()
                    .map(|value| format!(" ({})", value))
                    .unwrap_or_default(),
                task
            ),
            Self::OpenArtifactReview { artifact_id } => {
                format!("open artifact review {}", artifact_id)
            }
            Self::SearchMemory { query } => format!("search memory: {}", query),
            Self::ModifyPermissions {
                scope,
                grant_actions,
                grant_tool_capabilities,
                ..
            } => format!(
                "modify permissions ({:?}) actions={} tools={}",
                scope,
                grant_actions.len(),
                grant_tool_capabilities.len()
            ),
            Self::ClearSessionOverride { .. } => "clear session override".to_string(),
            Self::ResetBaselinePermissions { .. } => "reset baseline permissions".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorProposal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub action_label: String,
    pub action: SupervisorAction,
    #[serde(default)]
    pub missing_actions: Vec<SupervisorActionPermission>,
    #[serde(default)]
    pub missing_tool_capabilities: Vec<ToolCapabilityId>,
    #[serde(default)]
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorChatResult {
    pub response: String,
    #[serde(default)]
    pub proposals: Vec<SupervisorProposal>,
    pub permission_state: SupervisorPermissionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorActionResult {
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub local_action: Option<SupervisorAction>,
    #[serde(default)]
    pub permission_state: Option<SupervisorPermissionState>,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// Structured status for an agent runtime, replacing plain String.
/// Tracks the agent's current operational state for UI display and coordination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentStatus {
    Starting,
    #[default]
    Idle,
    Working {
        task: String,
    },
    Blocked {
        reason: String,
    },
    Interrupted,
    Completed,
}

impl AgentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Starting => "starting",
            Self::Idle => "idle",
            Self::Working { .. } => "working",
            Self::Blocked { .. } => "blocked",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
        }
    }

    /// Convert to legacy String representation for backward compatibility.
    pub fn to_legacy_string(&self) -> String {
        match self {
            Self::Starting => "starting".to_string(),
            Self::Idle => "idle".to_string(),
            Self::Working { task } => format!("working: {}", task),
            Self::Blocked { reason } => format!("blocked: {}", reason),
            Self::Interrupted => "interrupted".to_string(),
            Self::Completed => "completed".to_string(),
        }
    }
}

/// Legacy topology role in a coordinator/worker delegation pattern.
///
/// This is intentionally distinct from [`role_assignment::AgentRoleId`], which
/// identifies an open product-role contract independent of pane topology.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Coordinator,
    Worker { parent_pane_id: usize },
}

/// A record of a tool invocation observed in agent output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolInvocationRecord {
    pub kind: String,
    pub target: String,
    pub timestamp: Option<String>,
}

/// Summary of diff changes observed in agent output.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// Summary of a delegation task for cross-crate use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelegationSummary {
    pub id: String,
    pub task: String,
    pub state: String,
    pub coordinator_pane_id: usize,
    pub worker_pane_id: Option<usize>,
    pub created_at: String,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub tool_invocations: Vec<ToolInvocationRecord>,
    #[serde(default)]
    pub diff_summary: Option<DiffSummary>,
}

/// Target machine where an agent operates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MachineTarget {
    Local {
        workdir: String,
    },
    Remote {
        user: String,
        host: String,
        workdir: String,
        #[serde(default)]
        session_name: Option<String>,
    },
}

impl Default for MachineTarget {
    fn default() -> Self {
        Self::Local {
            workdir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AgentRuntime {
    pub id: String,
    pub label: String,
    pub backend_kind: String,
    pub session_id: Option<String>,
    /// Durable daemon-owned task identity, distinct from agent/session routing ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governed_task_id: Option<governed_task::GovernedTaskId>,
    /// Last daemon lifecycle revision observed by this runtime surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governed_task_revision: Option<u64>,
    #[serde(default)]
    pub ephemeral: bool,
    pub working_directory: String,
    pub status: String,
    pub current_task: Option<String>,
    pub active: bool,
    pub context: ContextHealthSummary,
    pub recent_files: Vec<String>,
    pub recent_tools: Vec<String>,
    pub warnings: Vec<String>,
    /// Structured agent status (parallel to legacy `status` string).
    #[serde(default)]
    pub agent_status: AgentStatus,
    /// Role in coordinator/worker pattern.
    #[serde(default)]
    pub role: Option<AgentRole>,
    /// Explicit product-role contract assigned to this agent runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_assignment: Option<role_assignment::AgentRoleAssignment>,
    /// Runtime capability evaluation captured for the product-role assignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_compatibility: Option<role_assignment::RoleCompatibility>,
    /// Grouping label for agent pool display.
    #[serde(default)]
    pub group: Option<String>,
    /// Tool invocations observed during this agent's current task.
    #[serde(default)]
    pub tool_invocations: Vec<ToolInvocationRecord>,
    /// Diff summary for the current work.
    #[serde(default)]
    pub diff_summary: Option<DiffSummary>,
    /// Target machine where the agent operates.
    #[serde(default)]
    pub target: Option<MachineTarget>,
}

fn dedupe_vec<T: Copy + PartialEq>(values: &mut Vec<T>) {
    let mut deduped = Vec::with_capacity(values.len());
    for value in values.iter().copied() {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    *values = deduped;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct InterventionRecommendation {
    pub id: String,
    pub title: String,
    pub description: String,
    pub severity: String,
    pub action_kind: String,
    pub action_label: String,
    pub target_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TerminalOpsReport {
    pub source_id: String,
    pub published_at: String,
    #[serde(default)]
    pub agents: Vec<AgentRuntime>,
    #[serde(default)]
    pub context: ContextHealthSummary,
    #[serde(default)]
    pub interventions: Vec<InterventionRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactViewHint {
    SummaryCard,
    Table,
    Timeline,
    Diff,
    Log,
    Markdown,
    RawJson,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    #[default]
    Ready,
    Staged,
    Pending,
    Applied,
    Acknowledged,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ArtifactFileRef {
    pub path: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ArtifactAction {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub params_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ArtifactEnvelope {
    pub id: String,
    pub project_id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub kind: String,
    pub schema: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub view_hints: Vec<ArtifactViewHint>,
    #[serde(default)]
    pub actions: Vec<ArtifactAction>,
    #[serde(default)]
    pub status: ArtifactStatus,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub related_files: Vec<ArtifactFileRef>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct OpsEvent {
    pub seq: u64,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub agent_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectOpsSnapshot {
    pub generated_at: String,
    pub project: ProjectSummary,
    pub agents: Vec<AgentRuntime>,
    pub interventions: Vec<InterventionRecommendation>,
    pub context: ContextHealthSummary,
    pub memory: MemorySummary,
    pub retrieval: RetrievalSummary,
    pub artifacts: Vec<ArtifactEnvelope>,
    /// Active and recent delegations for the agent pool view.
    #[serde(default)]
    pub delegations: Vec<DelegationSummary>,
    /// Daemon-owned governed task truth. Legacy snapshots omit this field.
    #[serde(default)]
    pub governed_tasks: Vec<governed_task::GovernedTaskSnapshot>,
    /// Review-only semantic proposals derived from accepted governed runs.
    /// Legacy protocol payloads omit this field.
    #[serde(default)]
    pub memory_candidates: Vec<memory_candidate::AcceptedRunMemoryCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct OpsSubscription {
    pub snapshot: ProjectOpsSnapshot,
    pub events: Vec<OpsEvent>,
    pub next_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ArtifactActionResult {
    pub status: String,
    pub message: String,
    pub artifact: Option<ArtifactEnvelope>,
    pub payload: Option<serde_json::Value>,
}

pub fn artifact_store_root(impulse_dir: &Path, project_id: &str) -> PathBuf {
    impulse_dir.join("projects").join(project_id).join("agents")
}

pub fn artifact_file_path(
    impulse_dir: &Path,
    project_id: &str,
    agent_id: &str,
    artifact_id: &str,
) -> PathBuf {
    artifact_store_root(impulse_dir, project_id)
        .join(agent_id)
        .join("artifacts")
        .join(format!("{}.json", artifact_id))
}

pub fn save_artifact(impulse_dir: &Path, artifact: &ArtifactEnvelope) -> Result<PathBuf, OpsError> {
    let path = artifact_file_path(
        impulse_dir,
        &artifact.project_id,
        &artifact.agent_id,
        &artifact.id,
    );
    let content = serde_json::to_vec_pretty(artifact)?;
    atomic_write_path(&path, &content)?;
    Ok(path)
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn file_modified_to_rfc3339(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sanitize_id_collapses_noise() {
        assert_eq!(
            sanitize_id(" Claude Code / Agent #1 "),
            "claude-code-agent-1"
        );
    }

    #[test]
    fn test_save_artifact_writes_json() {
        let temp = TempDir::new().unwrap();
        let artifact = ArtifactEnvelope {
            id: "artifact-1".into(),
            project_id: "project-a".into(),
            agent_id: "agent-a".into(),
            title: "Artifact".into(),
            created_at: now_rfc3339(),
            ..Default::default()
        };

        let path = save_artifact(temp.path(), &artifact).unwrap();
        assert!(path.exists());
        let saved: ArtifactEnvelope =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved.id, "artifact-1");
    }

    #[test]
    fn test_supervisor_permission_state_resolve_layers_session_override() {
        let baseline = SupervisorPermissionPolicy::default();
        let session_override = SupervisorPermissionPolicy {
            allowed_actions: vec![SupervisorActionPermission::InjectContext],
            allowed_tool_capabilities: vec![ToolCapabilityId::FileSystemWrite],
            require_confirmation_actions: vec![SupervisorActionPermission::InjectContext],
        };

        let state = SupervisorPermissionState::resolve(baseline.clone(), Some(session_override));

        assert!(state
            .effective
            .allows_action(SupervisorActionPermission::MonitorAgents));
        assert!(state
            .effective
            .allows_action(SupervisorActionPermission::InjectContext));
        assert!(state
            .effective
            .allows_tool_capability(ToolCapabilityId::SystemInfo));
        assert!(state
            .effective
            .allows_tool_capability(ToolCapabilityId::FileSystemWrite));
        assert!(state
            .effective
            .requires_confirmation(SupervisorActionPermission::InjectContext));
    }

    #[test]
    fn test_supervisor_permission_state_without_override_matches_baseline() {
        let baseline = SupervisorPermissionPolicy::default();
        let state = SupervisorPermissionState::resolve(baseline.clone(), None);

        assert_eq!(state.baseline, baseline);
        assert_eq!(state.effective, baseline);
        assert!(!state.session_override_active());
    }

    #[test]
    fn test_workbench_busy_response_roundtrip() {
        let response = WorkbenchDaemonResponse::Busy {
            resource: DaemonBusyResource::AgentTurn,
            retry_after_ms: 250,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""type":"Busy""#));
        assert!(json.contains(r#""resource":"agent_turn""#));
        assert_eq!(
            serde_json::from_str::<WorkbenchDaemonResponse>(&json).unwrap(),
            response
        );
    }
}
