use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod agent_registry;
pub mod governed_task;
pub mod governed_wiring;
pub mod memory_candidate;
/// ADR-0020 promotion/dismissal wire contract (request types only).
pub mod memory_wiring;
pub mod operator_capability;
pub mod role_assignment;

/// Shared daemon protocol version for GUI/operator workbench compatibility.
///
/// v7 (ADR-0018) adds the connection-scoped `PresentOperatorCapability`
/// request and the operator-class requirement on `RecordOperatorDecision`.
/// v8 (ADR-0019) adds the staged-worktree and promotion mutations plus the
/// `world_scope` registration field.
/// v9 (this lane) adds the `PromoteGovernedOutcome` and
/// `DiscardGovernedStagedWorktree` requests, staged materialization inside
/// `RegisterGovernedTask`, and the flattened producer acknowledgements that
/// carry `replayed` plus a reservation's `pending_rerun_reason`.
pub const DAEMON_PROTOCOL_VERSION: u32 = 9;

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
    /// Fast-forward the canonical branch onto an accepted staged outcome
    /// (ADR-0019). Operator-class only.
    PromoteGovernedOutcome {
        request: governed_wiring::GovernedPromotionRequest,
    },
    /// Reclaim a finished staged worktree (ADR-0019). Operator-class only.
    DiscardGovernedStagedWorktree {
        request: governed_wiring::GovernedStagedWorktreeDiscardRequest,
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

static TEMP_FILE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn atomic_write_path(path: &Path, content: &[u8]) -> Result<(), OpsError> {
    let parent = path
        .parent()
        .ok_or_else(|| OpsError::MissingParent(path.to_path_buf()))?;
    std::fs::create_dir_all(parent)?;
    // The target's name, the pid and a per-process sequence number make the
    // temp name unique. It used to be the pid and the clock alone, shared by
    // every file in the directory, and macOS clock nanoseconds are whole
    // microseconds: two artifacts saved in one microsecond shared a temp
    // file, so one could be renamed into place under the other's name.
    let tmp_path = parent.join(format!(
        ".{}.tmp.{}.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("write"),
        std::process::id(),
        TEMP_FILE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
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
    // `as_str` (and so the daemon's "Missing tool capabilities" error)
    // spells these without the inner underscore; accept that spelling back
    // too, so a client can grant the capability the error named. The wire
    // name written stays `file_system_*`.
    #[serde(alias = "filesystem_read")]
    FileSystemRead,
    #[serde(alias = "filesystem_write")]
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

    /// Whether this action changes the supervisor's permission policy.
    ///
    /// These take effect for every later supervisor action, so the daemon
    /// accepts them only from an operator-class connection (ADR-0018); the
    /// `confirmed` field is the client's own claim and authorizes nothing.
    pub fn changes_permissions(&self) -> bool {
        matches!(
            self,
            Self::ModifyPermissions { .. }
                | Self::ClearSessionOverride { .. }
                | Self::ResetBaselinePermissions { .. }
        )
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
    fn test_atomic_write_path_keeps_concurrent_files_apart() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().to_path_buf();
        let handles: Vec<_> = (0..4)
            .map(|thread| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    (0..200)
                        .filter_map(|i| {
                            let name = format!("{thread}-{i}.json");
                            atomic_write_path(&dir.join(&name), name.as_bytes()).err()
                        })
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let failures: Vec<String> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        assert!(
            failures.is_empty(),
            "{} failed: {:?}",
            failures.len(),
            failures.first()
        );
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with('.'), "temp file left: {name}");
            assert_eq!(std::fs::read_to_string(entry.path()).unwrap(), name);
            seen += 1;
        }
        assert_eq!(seen, 800);
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

    // ---- Wire contract shared with impulse-desktop -------------------------
    //
    // These models cross the daemon socket into the Dioxus host, so the JSON
    // shape is a cross-process contract: tags and field names are pinned, and
    // JSON from an older peer that omits defaulted fields must still parse.

    /// Serialize, deserialize, compare, and hand back the JSON for pinning.
    fn round_trip<T>(value: &T) -> serde_json::Value
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).unwrap();
        let decoded: T = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, value, "round trip changed the value: {json}");
        serde_json::from_str(&json).unwrap()
    }

    fn context_health() -> ContextHealthSummary {
        ContextHealthSummary {
            tier: "warning".into(),
            usage_fraction: 0.5,
            estimated_tokens: 64_000,
            window_tokens: 128_000,
            compaction_count: 2,
            injection_count: 3,
            pending_review_count: 1,
            recent_insights: vec![InsightRecord {
                timestamp: Some("2026-10-23T10:00:00Z".into()),
                agent_label: "Codex".into(),
                kind: "decision".into(),
                content: "use atomic writes".into(),
            }],
        }
    }

    fn tool_invocation() -> ToolInvocationRecord {
        ToolInvocationRecord {
            kind: "edit".into(),
            target: "src/lib.rs".into(),
            timestamp: Some("2026-10-23T10:01:00Z".into()),
        }
    }

    fn diff_summary() -> DiffSummary {
        DiffSummary {
            files_changed: 2,
            lines_added: 30,
            lines_removed: 4,
        }
    }

    fn agent_runtime() -> AgentRuntime {
        let assignment = role_assignment::canonical_governed_builder_assignment();
        let compatibility = role_assignment::evaluate_role_compatibility(
            &agent_registry::AgentPlatformId::try_new("codex").unwrap(),
            &[],
            &assignment,
        )
        .unwrap();
        AgentRuntime {
            id: "agent-1".into(),
            label: "Codex".into(),
            backend_kind: "codex".into(),
            session_id: Some("session-1".into()),
            governed_task_id: Some(governed_task::GovernedTaskId::try_new("task-7").unwrap()),
            governed_task_revision: Some(4),
            ephemeral: true,
            working_directory: "/workspace".into(),
            status: "working: wire tests".into(),
            current_task: Some("wire tests".into()),
            active: true,
            context: context_health(),
            recent_files: vec!["src/lib.rs".into()],
            recent_tools: vec!["Edit".into()],
            warnings: vec!["context at 50%".into()],
            agent_status: AgentStatus::Working {
                task: "wire tests".into(),
            },
            role: Some(AgentRole::Worker { parent_pane_id: 3 }),
            role_assignment: Some(assignment),
            role_compatibility: Some(compatibility),
            group: Some("builders".into()),
            tool_invocations: vec![tool_invocation()],
            diff_summary: Some(diff_summary()),
            target: Some(MachineTarget::Remote {
                user: "james".into(),
                host: "build-box".into(),
                workdir: "/srv/work".into(),
                session_name: Some("impulse".into()),
            }),
        }
    }

    fn intervention() -> InterventionRecommendation {
        InterventionRecommendation {
            id: "intervention-1".into(),
            title: "Pause pane 2".into(),
            description: "Two agents edit src/lib.rs".into(),
            severity: "high".into(),
            action_kind: "focus_agent".into(),
            action_label: "Focus".into(),
            target_agent_id: Some("agent-2".into()),
        }
    }

    fn terminal_ops_report() -> TerminalOpsReport {
        TerminalOpsReport {
            source_id: "desktop".into(),
            published_at: "2026-10-23T10:02:00Z".into(),
            agents: vec![agent_runtime()],
            context: context_health(),
            interventions: vec![intervention()],
        }
    }

    fn artifact_envelope() -> ArtifactEnvelope {
        ArtifactEnvelope {
            id: "artifact-1".into(),
            project_id: "project-a".into(),
            agent_id: "agent-1".into(),
            session_id: Some("session-1".into()),
            kind: "review".into(),
            schema: "impulse.review.v1".into(),
            title: "Review".into(),
            summary: "Two findings".into(),
            payload: serde_json::json!({"findings": 2}),
            view_hints: vec![ArtifactViewHint::SummaryCard, ArtifactViewHint::Diff],
            actions: vec![ArtifactAction {
                id: "apply".into(),
                label: "Apply".into(),
                kind: "apply_patch".into(),
                requires_confirmation: true,
                params_schema: serde_json::json!({"type": "object"}),
            }],
            status: ArtifactStatus::Staged,
            created_at: "2026-10-23T10:03:00Z".into(),
            related_files: vec![ArtifactFileRef {
                path: "src/lib.rs".into(),
                label: Some("library".into()),
            }],
            metadata: serde_json::json!({"source": "test"}),
        }
    }

    fn delegation_summary() -> DelegationSummary {
        DelegationSummary {
            id: "delegation-1".into(),
            task: "write wire tests".into(),
            state: "completed".into(),
            coordinator_pane_id: 1,
            worker_pane_id: Some(2),
            created_at: "2026-10-23T09:00:00Z".into(),
            completed_at: Some("2026-10-23T09:30:00Z".into()),
            tool_invocations: vec![tool_invocation()],
            diff_summary: Some(diff_summary()),
        }
    }

    fn ops_snapshot() -> ProjectOpsSnapshot {
        ProjectOpsSnapshot {
            generated_at: "2026-10-23T10:04:00Z".into(),
            project: ProjectSummary {
                id: "project-a".into(),
                name: "Project A".into(),
                root_path: "/workspace".into(),
                impulse_path: "/workspace/.impulse".into(),
            },
            agents: vec![agent_runtime()],
            interventions: vec![intervention()],
            context: context_health(),
            memory: MemorySummary {
                active_sessions: 2,
                history_entries: 40,
                genome_decisions: 7,
                last_genome_update: Some("2026-10-22T08:00:00Z".into()),
            },
            retrieval: RetrievalSummary {
                mode: "hybrid".into(),
                backend: "sqlite".into(),
                vector_enabled: true,
                semantic_strategy: "rrf".into(),
            },
            artifacts: vec![artifact_envelope()],
            delegations: vec![delegation_summary()],
            governed_tasks: Vec::new(),
            memory_candidates: Vec::new(),
        }
    }

    fn every_supervisor_action() -> Vec<(&'static str, SupervisorAction)> {
        vec![
            (
                "focus_agent",
                SupervisorAction::FocusAgent {
                    agent_id: "agent-1".into(),
                    session_id: Some("session-1".into()),
                },
            ),
            (
                "send_input",
                SupervisorAction::SendInput {
                    agent_id: "agent-1".into(),
                    session_id: Some("session-1".into()),
                    content: "continue".into(),
                    confirmed: true,
                },
            ),
            (
                "inject_context",
                SupervisorAction::InjectContext {
                    agent_id: Some("agent-1".into()),
                    session_id: Some("session-1".into()),
                    query: "atomic writes".into(),
                    confirmed: true,
                },
            ),
            (
                "cleanup_context",
                SupervisorAction::CleanupContext {
                    agent_id: Some("agent-1".into()),
                    session_id: Some("session-1".into()),
                    goal: Some("drop stale logs".into()),
                    confirmed: true,
                },
            ),
            (
                "handoff_context",
                SupervisorAction::HandoffContext {
                    session_id: Some("session-1".into()),
                    target_tool: "codex".into(),
                    task: "finish review".into(),
                    notes: Some("see PR".into()),
                    confirmed: true,
                },
            ),
            (
                "open_artifact_review",
                SupervisorAction::OpenArtifactReview {
                    artifact_id: "artifact-1".into(),
                },
            ),
            (
                "search_memory",
                SupervisorAction::SearchMemory {
                    query: "serde".into(),
                },
            ),
            (
                "modify_permissions",
                SupervisorAction::ModifyPermissions {
                    scope: PermissionChangeScope::PersistentDefault,
                    grant_actions: vec![SupervisorActionPermission::SendInput],
                    grant_tool_capabilities: vec![ToolCapabilityId::Network],
                    confirmed: true,
                },
            ),
            (
                "clear_session_override",
                SupervisorAction::ClearSessionOverride { confirmed: true },
            ),
            (
                "reset_baseline_permissions",
                SupervisorAction::ResetBaselinePermissions { confirmed: true },
            ),
        ]
    }

    fn permission_state() -> SupervisorPermissionState {
        SupervisorPermissionState::resolve(
            SupervisorPermissionPolicy::default(),
            Some(SupervisorPermissionPolicy {
                allowed_actions: vec![SupervisorActionPermission::InjectContext],
                allowed_tool_capabilities: vec![ToolCapabilityId::PythonExec],
                require_confirmation_actions: vec![SupervisorActionPermission::FocusAgent],
            }),
        )
    }

    #[test]
    fn test_workbench_request_every_variant_round_trips_with_its_type_tag() {
        use governed_task as gt;
        let request_id = |id: &str| gt::GovernedRequestId::try_new(id).unwrap();
        let task_id = || gt::GovernedTaskId::try_new("task-7").unwrap();
        let registration = gt::GovernedTaskRegistration::builder(
            "request-register",
            "task-7",
            "demo",
            "/tmp/demo",
            "pin the wire contract",
            "agent-1",
            "codex",
        )
        .build()
        .unwrap();
        let requests = vec![
            WorkbenchDaemonRequest::Ping,
            WorkbenchDaemonRequest::Status,
            WorkbenchDaemonRequest::PresentOperatorCapability(
                operator_capability::OperatorCapabilityPresentation {
                    token: "f".repeat(64),
                },
            ),
            WorkbenchDaemonRequest::ListSessions,
            WorkbenchDaemonRequest::CreateSession {
                name: "wire".into(),
                platform: Some("codex".into()),
            },
            WorkbenchDaemonRequest::EndSession {
                session_id: "s-1".into(),
                summary: "done".into(),
            },
            WorkbenchDaemonRequest::TrackFile {
                session_id: "s-1".into(),
                file_path: "src/lib.rs".into(),
            },
            WorkbenchDaemonRequest::InvokeTool {
                name: "system_info".into(),
                params: serde_json::json!({"verbose": true}),
            },
            WorkbenchDaemonRequest::ToolSchema,
            WorkbenchDaemonRequest::GetOpsSnapshot,
            WorkbenchDaemonRequest::SubscribeOps { since_seq: Some(9) },
            WorkbenchDaemonRequest::PublishTerminalOps {
                report: terminal_ops_report(),
            },
            WorkbenchDaemonRequest::GetSupervisorPermissions,
            WorkbenchDaemonRequest::SupervisorChat {
                prompt: "status?".into(),
                context: Some("two panes".into()),
            },
            WorkbenchDaemonRequest::RunSupervisorAction {
                action: SupervisorAction::SearchMemory {
                    query: "serde".into(),
                },
            },
            WorkbenchDaemonRequest::RunArtifactAction {
                artifact_id: "artifact-1".into(),
                action_id: "apply".into(),
                params: serde_json::json!({"note": "ok"}),
            },
            WorkbenchDaemonRequest::RegisterGovernedTask { registration },
            WorkbenchDaemonRequest::GetGovernedTask {
                project_id: "demo".into(),
                task_id: task_id(),
            },
            WorkbenchDaemonRequest::ListGovernedTasks {
                project_id: "demo".into(),
            },
            WorkbenchDaemonRequest::MutateGovernedTask {
                request: gt::GovernedTaskMutationRequest {
                    request_id: request_id("request-mutate"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 1,
                    mutation: gt::GovernedTaskMutation::MarkRunning {
                        actor: gt::GovernedActor {
                            kind: gt::GovernedActorKind::System,
                            id: "desktop".into(),
                        },
                    },
                    fencing_generation: Some(1),
                },
            },
            WorkbenchDaemonRequest::SubmitGovernedClaim {
                request: gt::GovernedClaimRequest {
                    request_id: request_id("request-claim"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 2,
                    summary: "implemented".into(),
                    artifact_ids: vec!["artifact-1".into()],
                    generation: Some(1),
                },
            },
            WorkbenchDaemonRequest::RunGovernedVerification {
                request: gt::GovernedVerificationRequest {
                    request_id: request_id("request-verify"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 3,
                },
            },
            WorkbenchDaemonRequest::RunGovernedSupervisorReview {
                request: gt::GovernedSupervisorReviewRequest {
                    request_id: request_id("request-review"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 4,
                },
            },
            WorkbenchDaemonRequest::PromoteGovernedOutcome {
                request: governed_wiring::GovernedPromotionRequest {
                    request_id: request_id("request-promote"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 5,
                },
            },
            WorkbenchDaemonRequest::DiscardGovernedStagedWorktree {
                request: governed_wiring::GovernedStagedWorktreeDiscardRequest {
                    request_id: request_id("request-discard"),
                    project_id: "demo".into(),
                    task_id: task_id(),
                    expected_revision: 6,
                    reason: "rejected".into(),
                },
            },
            WorkbenchDaemonRequest::GuardList,
            WorkbenchDaemonRequest::GetConflictHistory,
            WorkbenchDaemonRequest::ClearResolvedConflicts,
        ];

        let mut seen = std::collections::BTreeSet::new();
        for request in &requests {
            // Exhaustive on purpose: a new variant fails to compile here until
            // its wire tag is pinned and a value is added above.
            let (tag, unit) = match request {
                WorkbenchDaemonRequest::Ping => ("Ping", true),
                WorkbenchDaemonRequest::Status => ("Status", true),
                WorkbenchDaemonRequest::PresentOperatorCapability(_) => {
                    ("PresentOperatorCapability", false)
                }
                WorkbenchDaemonRequest::ListSessions => ("ListSessions", true),
                WorkbenchDaemonRequest::CreateSession { .. } => ("CreateSession", false),
                WorkbenchDaemonRequest::EndSession { .. } => ("EndSession", false),
                WorkbenchDaemonRequest::TrackFile { .. } => ("TrackFile", false),
                WorkbenchDaemonRequest::InvokeTool { .. } => ("InvokeTool", false),
                WorkbenchDaemonRequest::ToolSchema => ("ToolSchema", true),
                WorkbenchDaemonRequest::GetOpsSnapshot => ("GetOpsSnapshot", true),
                WorkbenchDaemonRequest::SubscribeOps { .. } => ("SubscribeOps", false),
                WorkbenchDaemonRequest::PublishTerminalOps { .. } => ("PublishTerminalOps", false),
                WorkbenchDaemonRequest::GetSupervisorPermissions => {
                    ("GetSupervisorPermissions", true)
                }
                WorkbenchDaemonRequest::SupervisorChat { .. } => ("SupervisorChat", false),
                WorkbenchDaemonRequest::RunSupervisorAction { .. } => {
                    ("RunSupervisorAction", false)
                }
                WorkbenchDaemonRequest::RunArtifactAction { .. } => ("RunArtifactAction", false),
                WorkbenchDaemonRequest::RegisterGovernedTask { .. } => {
                    ("RegisterGovernedTask", false)
                }
                WorkbenchDaemonRequest::GetGovernedTask { .. } => ("GetGovernedTask", false),
                WorkbenchDaemonRequest::ListGovernedTasks { .. } => ("ListGovernedTasks", false),
                WorkbenchDaemonRequest::MutateGovernedTask { .. } => ("MutateGovernedTask", false),
                WorkbenchDaemonRequest::SubmitGovernedClaim { .. } => {
                    ("SubmitGovernedClaim", false)
                }
                WorkbenchDaemonRequest::RunGovernedVerification { .. } => {
                    ("RunGovernedVerification", false)
                }
                WorkbenchDaemonRequest::RunGovernedSupervisorReview { .. } => {
                    ("RunGovernedSupervisorReview", false)
                }
                WorkbenchDaemonRequest::PromoteGovernedOutcome { .. } => {
                    ("PromoteGovernedOutcome", false)
                }
                WorkbenchDaemonRequest::DiscardGovernedStagedWorktree { .. } => {
                    ("DiscardGovernedStagedWorktree", false)
                }
                WorkbenchDaemonRequest::GuardList => ("GuardList", true),
                WorkbenchDaemonRequest::GetConflictHistory => ("GetConflictHistory", true),
                WorkbenchDaemonRequest::ClearResolvedConflicts => ("ClearResolvedConflicts", true),
            };
            let json = round_trip(request);
            assert_eq!(json["type"], tag);
            assert_eq!(json.get("data").is_none(), unit, "data presence for {tag}");
            assert!(seen.insert(tag), "duplicate {tag}");
        }
        assert_eq!(
            seen.len(),
            28,
            "every WorkbenchDaemonRequest variant is listed"
        );
    }

    #[test]
    fn test_workbench_request_omitting_defaulted_fields_parses() {
        let parse = |value: serde_json::Value| {
            serde_json::from_value::<WorkbenchDaemonRequest>(value).unwrap()
        };
        assert_eq!(
            parse(serde_json::json!({"type": "InvokeTool", "data": {"name": "system_info"}})),
            WorkbenchDaemonRequest::InvokeTool {
                name: "system_info".into(),
                params: serde_json::Value::Null,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"type": "SubscribeOps", "data": {}})),
            WorkbenchDaemonRequest::SubscribeOps { since_seq: None }
        );
        assert_eq!(
            parse(serde_json::json!({
                "type": "RunArtifactAction",
                "data": {"artifact_id": "a", "action_id": "b"}
            })),
            WorkbenchDaemonRequest::RunArtifactAction {
                artifact_id: "a".into(),
                action_id: "b".into(),
                params: serde_json::Value::Null,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"type": "CreateSession", "data": {"name": "n"}})),
            WorkbenchDaemonRequest::CreateSession {
                name: "n".into(),
                platform: None,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"type": "SupervisorChat", "data": {"prompt": "p"}})),
            WorkbenchDaemonRequest::SupervisorChat {
                prompt: "p".into(),
                context: None,
            }
        );
        assert_eq!(
            parse(serde_json::json!({
                "type": "PublishTerminalOps",
                "data": {"report": {"source_id": "desktop", "published_at": "t"}}
            })),
            WorkbenchDaemonRequest::PublishTerminalOps {
                report: TerminalOpsReport {
                    source_id: "desktop".into(),
                    published_at: "t".into(),
                    ..Default::default()
                },
            }
        );
        assert!(serde_json::from_value::<WorkbenchDaemonRequest>(
            serde_json::json!({"type": "Nope"})
        )
        .is_err());
    }

    #[test]
    fn test_workbench_response_every_variant_round_trips_with_its_type_tag() {
        let responses = [
            (
                "Ok",
                WorkbenchDaemonResponse::Ok {
                    result: serde_json::json!({"sessions": 2}),
                },
            ),
            (
                "Error",
                WorkbenchDaemonResponse::Error {
                    message: "no such task".into(),
                },
            ),
            (
                "Busy",
                WorkbenchDaemonResponse::Busy {
                    resource: DaemonBusyResource::AgentTurn,
                    retry_after_ms: 250,
                },
            ),
            (
                "ConflictCheck",
                WorkbenchDaemonResponse::ConflictCheck {
                    has_conflict: true,
                    conflicting_sessions: vec!["s-1".into(), "s-2".into()],
                },
            ),
        ];
        for (tag, response) in &responses {
            let json = round_trip(response);
            assert_eq!(json["type"], *tag);
            assert!(json["data"].is_object());
        }
    }

    #[test]
    fn test_tool_capability_id_wire_names_and_as_str_spelling_both_parse() {
        let cases = [
            (ToolCapabilityId::FileSystemRead, "file_system_read"),
            (ToolCapabilityId::FileSystemWrite, "file_system_write"),
            (ToolCapabilityId::Network, "network"),
            (ToolCapabilityId::PythonExec, "python_exec"),
            (ToolCapabilityId::SystemInfo, "system_info"),
        ];
        for (capability, wire) in cases {
            assert_eq!(round_trip(&capability), wire);
        }
        // `as_str` (used in the daemon's error text) spells the filesystem
        // pair without the inner underscore; that spelling is accepted back
        // so a client can grant exactly what the error named.
        assert_eq!(ToolCapabilityId::FileSystemRead.as_str(), "filesystem_read");
        for capability in [
            ToolCapabilityId::FileSystemRead,
            ToolCapabilityId::FileSystemWrite,
        ] {
            let named_in_error = format!("\"{}\"", capability.as_str());
            assert_eq!(
                serde_json::from_str::<ToolCapabilityId>(&named_in_error).unwrap(),
                capability
            );
        }
        for capability in [
            ToolCapabilityId::Network,
            ToolCapabilityId::PythonExec,
            ToolCapabilityId::SystemInfo,
        ] {
            assert_eq!(round_trip(&capability), capability.as_str());
        }
    }

    #[test]
    fn test_supervisor_permission_wire_names_match_as_str() {
        let permissions = [
            SupervisorActionPermission::MonitorAgents,
            SupervisorActionPermission::FocusAgent,
            SupervisorActionPermission::OpenReview,
            SupervisorActionPermission::SearchMemory,
            SupervisorActionPermission::ModifyPermissions,
            SupervisorActionPermission::SendInput,
            SupervisorActionPermission::InjectContext,
            SupervisorActionPermission::CleanupContext,
            SupervisorActionPermission::HandoffContext,
        ];
        for permission in permissions {
            assert_eq!(round_trip(&permission), permission.as_str());
        }
        assert_eq!(
            round_trip(&PermissionChangeScope::SessionOverride),
            "session_override"
        );
        assert_eq!(
            round_trip(&PermissionChangeScope::PersistentDefault),
            "persistent_default"
        );
    }

    #[test]
    fn test_supervisor_permission_policy_round_trips_and_fills_missing_lists() {
        let policy = SupervisorPermissionPolicy {
            allowed_actions: vec![SupervisorActionPermission::SendInput],
            allowed_tool_capabilities: vec![ToolCapabilityId::Network],
            require_confirmation_actions: vec![],
        };
        round_trip(&policy);

        // Container-level `serde(default)`: each missing list keeps the default.
        assert_eq!(
            serde_json::from_str::<SupervisorPermissionPolicy>("{}").unwrap(),
            SupervisorPermissionPolicy::default()
        );
        let partial: SupervisorPermissionPolicy =
            serde_json::from_str(r#"{"allowed_actions":["send_input"]}"#).unwrap();
        assert_eq!(
            partial.allowed_actions,
            vec![SupervisorActionPermission::SendInput]
        );
        assert_eq!(
            partial.require_confirmation_actions,
            SupervisorPermissionPolicy::default().require_confirmation_actions
        );
    }

    #[test]
    fn test_supervisor_permission_state_round_trips_and_omitted_override_is_none() {
        let state = permission_state();
        assert!(state.session_override_active());
        round_trip(&state);

        let mut json = serde_json::to_value(&state).unwrap();
        json.as_object_mut().unwrap().remove("session_override");
        let decoded: SupervisorPermissionState = serde_json::from_value(json).unwrap();
        assert!(decoded.session_override.is_none());
        assert_eq!(decoded.effective, state.effective);
    }

    #[test]
    fn test_supervisor_action_every_kind_round_trips_and_matches_permission() {
        let actions = every_supervisor_action();
        assert_eq!(actions.len(), 10);
        for (kind, action) in &actions {
            let json = round_trip(action);
            assert_eq!(json["kind"], *kind);
            assert!(action.confirmed());
        }
    }

    #[test]
    fn test_supervisor_action_minimal_json_fills_defaults() {
        let parse =
            |value: serde_json::Value| serde_json::from_value::<SupervisorAction>(value).unwrap();
        assert_eq!(
            parse(serde_json::json!({"kind": "focus_agent", "agent_id": "a"})),
            SupervisorAction::FocusAgent {
                agent_id: "a".into(),
                session_id: None,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "send_input", "agent_id": "a", "content": "c"})),
            SupervisorAction::SendInput {
                agent_id: "a".into(),
                session_id: None,
                content: "c".into(),
                confirmed: false,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "inject_context", "query": "q"})),
            SupervisorAction::InjectContext {
                agent_id: None,
                session_id: None,
                query: "q".into(),
                confirmed: false,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "cleanup_context"})),
            SupervisorAction::CleanupContext {
                agent_id: None,
                session_id: None,
                goal: None,
                confirmed: false,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "handoff_context", "target_tool": "t", "task": "k"})),
            SupervisorAction::HandoffContext {
                session_id: None,
                target_tool: "t".into(),
                task: "k".into(),
                notes: None,
                confirmed: false,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "modify_permissions", "scope": "session_override"})),
            SupervisorAction::ModifyPermissions {
                scope: PermissionChangeScope::SessionOverride,
                grant_actions: vec![],
                grant_tool_capabilities: vec![],
                confirmed: false,
            }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "clear_session_override"})),
            SupervisorAction::ClearSessionOverride { confirmed: false }
        );
        assert_eq!(
            parse(serde_json::json!({"kind": "reset_baseline_permissions"})),
            SupervisorAction::ResetBaselinePermissions { confirmed: false }
        );
    }

    #[test]
    fn test_supervisor_results_round_trip_and_minimal_json_parses() {
        let proposal = SupervisorProposal {
            id: "proposal-1".into(),
            title: "Inject context".into(),
            description: "Give agent-1 the ADR".into(),
            action_label: "Inject".into(),
            action: every_supervisor_action().remove(2).1,
            missing_actions: vec![SupervisorActionPermission::InjectContext],
            missing_tool_capabilities: vec![ToolCapabilityId::FileSystemWrite],
            requires_confirmation: true,
        };
        round_trip(&proposal);
        round_trip(&SupervisorChatResult {
            response: "Two agents are active".into(),
            proposals: vec![proposal],
            permission_state: permission_state(),
        });
        round_trip(&SupervisorActionResult {
            status: "ok".into(),
            message: "focused".into(),
            local_action: Some(every_supervisor_action().remove(0).1),
            permission_state: Some(permission_state()),
            artifact_id: Some("artifact-1".into()),
            payload: Some(serde_json::json!({"focused": "agent-1"})),
        });

        let minimal_proposal: SupervisorProposal = serde_json::from_value(serde_json::json!({
            "id": "p", "title": "t", "description": "d", "action_label": "l",
            "action": {"kind": "search_memory", "query": "q"}
        }))
        .unwrap();
        assert!(minimal_proposal.missing_actions.is_empty());
        assert!(minimal_proposal.missing_tool_capabilities.is_empty());
        assert!(!minimal_proposal.requires_confirmation);

        let state = serde_json::to_value(permission_state()).unwrap();
        let minimal_chat: SupervisorChatResult =
            serde_json::from_value(serde_json::json!({"response": "r", "permission_state": state}))
                .unwrap();
        assert!(minimal_chat.proposals.is_empty());

        let minimal_result: SupervisorActionResult =
            serde_json::from_value(serde_json::json!({"status": "ok", "message": "m"})).unwrap();
        assert!(minimal_result.local_action.is_none());
        assert!(minimal_result.permission_state.is_none());
        assert!(minimal_result.artifact_id.is_none());
        assert!(minimal_result.payload.is_none());
    }

    #[test]
    fn test_agent_status_state_tag_matches_as_str_for_every_variant() {
        let statuses = [
            AgentStatus::Starting,
            AgentStatus::Idle,
            AgentStatus::Working { task: "t".into() },
            AgentStatus::Blocked { reason: "r".into() },
            AgentStatus::Interrupted,
            AgentStatus::Completed,
        ];
        for status in &statuses {
            let json = round_trip(status);
            assert_eq!(json["state"], status.as_str());
        }
        assert_eq!(
            round_trip(&AgentStatus::Working { task: "t".into() }),
            serde_json::json!({"state": "working", "task": "t"})
        );
        assert_eq!(
            round_trip(&AgentStatus::Blocked { reason: "r".into() }),
            serde_json::json!({"state": "blocked", "reason": "r"})
        );
    }

    #[test]
    fn test_agent_role_wire_shapes() {
        assert_eq!(round_trip(&AgentRole::Coordinator), "coordinator");
        assert_eq!(
            round_trip(&AgentRole::Worker { parent_pane_id: 3 }),
            serde_json::json!({"worker": {"parent_pane_id": 3}})
        );
    }

    #[test]
    fn test_machine_target_kinds_round_trip_and_session_name_is_optional() {
        assert_eq!(
            round_trip(&MachineTarget::Local {
                workdir: "/w".into()
            }),
            serde_json::json!({"kind": "local", "workdir": "/w"})
        );
        let remote = round_trip(&MachineTarget::Remote {
            user: "u".into(),
            host: "h".into(),
            workdir: "/w".into(),
            session_name: Some("s".into()),
        });
        assert_eq!(remote["kind"], "remote");
        let minimal: MachineTarget = serde_json::from_value(
            serde_json::json!({"kind": "remote", "user": "u", "host": "h", "workdir": "/w"}),
        )
        .unwrap();
        assert_eq!(
            minimal,
            MachineTarget::Remote {
                user: "u".into(),
                host: "h".into(),
                workdir: "/w".into(),
                session_name: None,
            }
        );
    }

    #[test]
    fn test_delegation_and_tool_records_round_trip_and_minimal_json_parses() {
        round_trip(&tool_invocation());
        round_trip(&diff_summary());
        round_trip(&delegation_summary());

        let minimal: DelegationSummary = serde_json::from_value(serde_json::json!({
            "id": "d", "task": "t", "state": "pending", "coordinator_pane_id": 1,
            "created_at": "c"
        }))
        .unwrap();
        assert!(minimal.worker_pane_id.is_none());
        assert!(minimal.completed_at.is_none());
        assert!(minimal.tool_invocations.is_empty());
        assert!(minimal.diff_summary.is_none());

        let record: ToolInvocationRecord =
            serde_json::from_value(serde_json::json!({"kind": "k", "target": "t"})).unwrap();
        assert!(record.timestamp.is_none());
    }

    #[test]
    fn test_agent_runtime_full_value_round_trips_and_omits_unset_governed_fields() {
        let runtime = agent_runtime();
        let json = round_trip(&runtime);
        assert_eq!(json["governed_task_id"], "task-7");
        assert_eq!(json["governed_task_revision"], 4);
        assert_eq!(json["agent_status"]["state"], "working");
        assert_eq!(json["target"]["kind"], "remote");

        let bare = serde_json::to_value(AgentRuntime::default()).unwrap();
        for key in [
            "governed_task_id",
            "governed_task_revision",
            "role_assignment",
            "role_compatibility",
        ] {
            assert!(bare.get(key).is_none(), "{key} should be skipped when None");
        }
    }

    #[test]
    fn test_summary_records_round_trip() {
        round_trip(&context_health());
        round_trip(&intervention());
        round_trip(&terminal_ops_report());
        let snapshot = ops_snapshot();
        round_trip(&snapshot.project);
        round_trip(&snapshot.memory);
        round_trip(&snapshot.retrieval);
        round_trip(&snapshot.context.recent_insights[0]);
    }

    #[test]
    fn test_artifact_enum_wire_names() {
        let hints = [
            (ArtifactViewHint::SummaryCard, "summary_card"),
            (ArtifactViewHint::Table, "table"),
            (ArtifactViewHint::Timeline, "timeline"),
            (ArtifactViewHint::Diff, "diff"),
            (ArtifactViewHint::Log, "log"),
            (ArtifactViewHint::Markdown, "markdown"),
            (ArtifactViewHint::RawJson, "raw_json"),
        ];
        for (hint, wire) in &hints {
            assert_eq!(round_trip(hint), *wire);
        }
        let statuses = [
            (ArtifactStatus::Ready, "ready"),
            (ArtifactStatus::Staged, "staged"),
            (ArtifactStatus::Pending, "pending"),
            (ArtifactStatus::Applied, "applied"),
            (ArtifactStatus::Acknowledged, "acknowledged"),
        ];
        for (status, wire) in &statuses {
            assert_eq!(round_trip(status), *wire);
        }
    }

    #[test]
    fn test_artifact_envelope_round_trips_and_minimal_json_parses() {
        let artifact = artifact_envelope();
        round_trip(&artifact);
        round_trip(&artifact.actions[0]);
        round_trip(&artifact.related_files[0]);
        round_trip(&ArtifactActionResult {
            status: "applied".into(),
            message: "patch applied".into(),
            artifact: Some(artifact),
            payload: Some(serde_json::json!({"files": 2})),
        });

        let minimal: ArtifactEnvelope = serde_json::from_value(serde_json::json!({
            "id": "a", "project_id": "p", "agent_id": "g", "kind": "k", "schema": "s"
        }))
        .unwrap();
        assert_eq!(minimal.status, ArtifactStatus::Ready);
        assert!(minimal.session_id.is_none());
        assert!(minimal.title.is_empty());
        assert!(minimal.payload.is_null());
        assert!(minimal.view_hints.is_empty());
        assert!(minimal.actions.is_empty());
        assert!(minimal.related_files.is_empty());
        assert!(minimal.metadata.is_null());

        let action: ArtifactAction =
            serde_json::from_value(serde_json::json!({"id": "i", "label": "l", "kind": "k"}))
                .unwrap();
        assert!(!action.requires_confirmation);
        assert!(action.params_schema.is_null());
    }

    #[test]
    fn test_ops_snapshot_and_subscription_round_trip() {
        let snapshot = ops_snapshot();
        let json = round_trip(&snapshot);
        assert_eq!(json["delegations"][0]["id"], "delegation-1");
        round_trip(&OpsSubscription {
            snapshot,
            events: vec![OpsEvent {
                seq: 12,
                kind: "agent_status".into(),
                severity: "info".into(),
                title: "agent-1 working".into(),
                detail: "wire tests".into(),
                agent_id: Some("agent-1".into()),
                created_at: "2026-10-23T10:05:00Z".into(),
            }],
            next_seq: 13,
        });

        // Snapshots from a peer that predates delegations still load.
        let mut legacy = serde_json::to_value(ops_snapshot()).unwrap();
        let object = legacy.as_object_mut().unwrap();
        for key in ["delegations", "governed_tasks", "memory_candidates"] {
            object.remove(key);
        }
        let decoded: ProjectOpsSnapshot = serde_json::from_value(legacy).unwrap();
        assert!(decoded.delegations.is_empty());
        assert!(decoded.governed_tasks.is_empty());
        assert!(decoded.memory_candidates.is_empty());
    }
}
