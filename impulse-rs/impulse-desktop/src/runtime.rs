use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};

pub use impulse_ops::agent_registry::AgentPlatformId;
use impulse_ops::governed_task::{
    GovernedTaskMutation, GovernedTaskMutationRequest, GovernedTaskRegistration, GovernedTaskRun,
};
use impulse_ops::role_assignment::{AgentRoleAssignment, EnforcementStrength, RoleCompatibility};
use impulse_ops::{AgentRole, AgentStatus, ContextHealthSummary, MachineTarget};
use impulse_term::TerminalBackend;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bridge::{
    DesktopBridgeError, TerminalBridge, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};

/// Tasks are copied into `IMPULSE_TASK`; keep that environment value bounded
/// and measure the wire-compatible UTF-8 representation rather than chars.
const MAX_GOVERNED_TASK_BYTES: usize = impulse_ops::governed_task::MAX_GOVERNED_TASK_BYTES;

/// Acknowledged daemon command seam used by the synchronous pre-PTY gate.
/// Implementations must return daemon-owned task state, never optimistic UI state.
pub trait GovernedTaskGateway: Send + Sync {
    fn register(&self, registration: GovernedTaskRegistration) -> Result<GovernedTaskRun, String>;

    fn mutate(&self, request: GovernedTaskMutationRequest) -> Result<GovernedTaskRun, String>;

    fn mutate_current(
        &self,
        project_id: &str,
        task_id: &impulse_ops::governed_task::GovernedTaskId,
        request_id: impulse_ops::governed_task::GovernedRequestId,
        mutation: GovernedTaskMutation,
    ) -> Result<GovernedTaskRun, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceTarget {
    pub root: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub project_notes: Option<String>,
}

impl WorkspaceTarget {
    pub fn from_root(root: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            label: Path::new(&root)
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string()),
            root,
            purpose: None,
            project_notes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuiltInMcpTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
}

impl BuiltInMcpTool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        capabilities: Vec<String>,
        requires_confirmation: bool,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            capabilities,
            requires_confirmation,
        }
    }
}

pub fn default_builtin_mcp_tools() -> Vec<BuiltInMcpTool> {
    vec![
        BuiltInMcpTool::new(
            "impulse.agent_spawn",
            "Start a terminal coding agent in an explicit workspace through the Rust PTY runtime.",
            vec!["terminal".to_string(), "workspace".to_string()],
            true,
        ),
        BuiltInMcpTool::new(
            "impulse.agent_write",
            "Send confirmed input bytes to a running terminal coding agent.",
            vec!["terminal".to_string()],
            true,
        ),
        BuiltInMcpTool::new(
            "impulse.search_memory",
            "Search Impulse memory and session history for context before agent action.",
            vec!["memory".to_string(), "read_only".to_string()],
            false,
        ),
        BuiltInMcpTool::new(
            "impulse.review_injection",
            "Stage retrieved context for review before injecting it into an agent terminal.",
            vec!["context".to_string(), "review".to_string()],
            true,
        ),
        BuiltInMcpTool::new(
            "impulse.review_decision",
            "Apply or skip a staged review payload with an audit receipt.",
            vec![
                "context".to_string(),
                "review".to_string(),
                "terminal".to_string(),
            ],
            true,
        ),
        BuiltInMcpTool::new(
            "impulse.project_context",
            "Read operator-authored context for a registered project workspace.",
            vec![
                "workspace".to_string(),
                "context".to_string(),
                "read_only".to_string(),
            ],
            false,
        ),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSpawnRequest {
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub platform: AgentPlatformId,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub workspace: Option<WorkspaceTarget>,
    #[serde(default)]
    pub mcp_tools: Vec<BuiltInMcpTool>,
    pub rows: u16,
    pub cols: u16,
    #[serde(default)]
    pub role: Option<AgentRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_assignment: Option<AgentRoleAssignment>,
    #[serde(default)]
    pub target: Option<MachineTarget>,
}

impl AgentSpawnRequest {
    pub fn terminal_harness(
        agent_id: impl Into<String>,
        platform: AgentPlatformId,
        workspace_root: impl Into<String>,
        rows: u16,
        cols: u16,
    ) -> Self {
        let agent_id = agent_id.into();
        let workspace = WorkspaceTarget::from_root(workspace_root.into());
        Self {
            agent_id: Some(agent_id.clone()),
            session_id: Some(format!("{agent_id}-session")),
            platform,
            command: None,
            args: Vec::new(),
            cwd: Some(workspace.root.clone()),
            env: HashMap::new(),
            workspace: Some(workspace),
            mcp_tools: default_builtin_mcp_tools(),
            rows,
            cols,
            role: None,
            task: None,
            role_assignment: None,
            target: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentWriteRequest {
    pub agent_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorLocalActionRequest {
    pub action: LocalSupervisorAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalSupervisorAction {
    FocusAgent {
        agent_id: String,
    },
    SendInput {
        agent_id: String,
        content: String,
        confirmed: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRuntimeSnapshot {
    pub agent_id: String,
    pub label: String,
    pub platform: AgentPlatformId,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub workspace: Option<WorkspaceTarget>,
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governed_task_id: Option<impulse_ops::governed_task::GovernedTaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governed_task_revision: Option<u64>,
    pub rows: u16,
    pub cols: u16,
    pub alive: bool,
    pub focused: bool,
    pub status: AgentStatus,
    pub current_task: Option<String>,
    pub role: Option<AgentRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_assignment: Option<AgentRoleAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_compatibility: Option<RoleCompatibility>,
    pub target: Option<MachineTarget>,
    pub mcp_tools: Vec<BuiltInMcpTool>,
    pub output_bytes: u64,
    pub output_lines: u64,
    pub context: ContextHealthSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum DesktopEvent {
    TerminalOutput {
        agent_id: String,
        data: Vec<u8>,
    },
    TerminalExit {
        agent_id: String,
    },
    AgentRuntimeUpdate {
        snapshot: Box<AgentRuntimeSnapshot>,
    },
    OpsUpdate {
        payload: serde_json::Value,
    },
    OpsConnectionUpdate {
        connected: bool,
        #[serde(default)]
        error: Option<String>,
    },
    SupervisorLocalAction {
        action: LocalSupervisorAction,
    },
}

impl DesktopEvent {
    pub const HOST_EVENT_NAMES: &'static [&'static str] = &[
        "terminal_output",
        "terminal_exit",
        "agent_runtime_update",
        "ops_update",
        "ops_connection_update",
        "supervisor_local_action",
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::TerminalOutput { .. } => Self::HOST_EVENT_NAMES[0],
            Self::TerminalExit { .. } => Self::HOST_EVENT_NAMES[1],
            Self::AgentRuntimeUpdate { .. } => Self::HOST_EVENT_NAMES[2],
            Self::OpsUpdate { .. } => Self::HOST_EVENT_NAMES[3],
            Self::OpsConnectionUpdate { .. } => Self::HOST_EVENT_NAMES[4],
            Self::SupervisorLocalAction { .. } => Self::HOST_EVENT_NAMES[5],
        }
    }
}

pub trait DesktopEventSink: Send + Sync {
    fn emit(&self, event: DesktopEvent);
}

#[derive(Debug, Default)]
struct NoopEventSink;

impl DesktopEventSink for NoopEventSink {
    fn emit(&self, _event: DesktopEvent) {}
}

#[derive(Default)]
struct LifecycleEventQueue {
    events: VecDeque<DesktopEvent>,
    dispatching: bool,
}

/// FIFO lifecycle delivery that permits a public sink to reenter the runtime.
/// Capture/enqueue happens under `DesktopRuntime::lifecycle_events`; delivery
/// never does, so a reentrant focus/resize call appends behind the event being
/// delivered instead of deadlocking on a non-reentrant mutex.
struct LifecycleEventDispatcher {
    sink: Arc<dyn DesktopEventSink>,
    queue: Mutex<LifecycleEventQueue>,
}

impl LifecycleEventDispatcher {
    fn new(sink: Arc<dyn DesktopEventSink>) -> Self {
        Self {
            sink,
            queue: Mutex::new(LifecycleEventQueue::default()),
        }
    }

    /// Returns true only to the caller that owns the current drain cycle.
    fn enqueue(&self, event: DesktopEvent) -> bool {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        queue.events.push_back(event);
        if queue.dispatching {
            false
        } else {
            queue.dispatching = true;
            true
        }
    }

    fn drain(&self) {
        let mut reset = DispatchReset {
            dispatcher: self,
            armed: true,
        };
        loop {
            let next = {
                let mut queue = self
                    .queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match queue.events.pop_front() {
                    Some(event) => Some(event),
                    None => {
                        queue.dispatching = false;
                        reset.armed = false;
                        None
                    }
                }
            };
            let Some(event) = next else {
                return;
            };
            self.sink.emit(event);
        }
    }
}

struct DispatchReset<'a> {
    dispatcher: &'a LifecycleEventDispatcher,
    armed: bool,
}

impl Drop for DispatchReset<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.dispatcher
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dispatching = false;
        }
    }
}

struct RuntimeRecord {
    platform: AgentPlatformId,
    label: String,
    command: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    workspace: Option<WorkspaceTarget>,
    session_id: Option<String>,
    governed_task: Option<GovernedTaskRun>,
    current_task: Option<String>,
    role: Option<AgentRole>,
    role_assignment: Option<AgentRoleAssignment>,
    role_compatibility: Option<RoleCompatibility>,
    target: Option<MachineTarget>,
    mcp_tools: Vec<BuiltInMcpTool>,
    focused: bool,
    status: AgentStatus,
    backend: TerminalBackend,
}

impl RuntimeRecord {
    fn snapshot(&self, agent_id: &str) -> AgentRuntimeSnapshot {
        let (cols, rows) = self.backend.size();
        let alive = self.backend.is_alive();
        let status = if alive {
            self.status.clone()
        } else {
            AgentStatus::Completed
        };
        AgentRuntimeSnapshot {
            agent_id: agent_id.to_string(),
            label: self.label.clone(),
            platform: self.platform.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            cwd: self.cwd.as_ref().map(|path| path.display().to_string()),
            workspace: self.workspace.clone(),
            session_id: self.session_id.clone(),
            governed_task_id: self.governed_task.as_ref().map(|task| task.id.clone()),
            governed_task_revision: self.governed_task.as_ref().map(|task| task.revision),
            rows,
            cols,
            alive,
            focused: self.focused,
            status,
            current_task: self.current_task.clone(),
            role: self.role.clone(),
            role_assignment: self.role_assignment.clone(),
            role_compatibility: self.role_compatibility.clone(),
            target: self.target.clone(),
            mcp_tools: self.mcp_tools.clone(),
            output_bytes: self.backend.output_bytes(),
            output_lines: self.backend.output_lines(),
            context: ContextHealthSummary::default(),
        }
    }
}

#[derive(Default)]
struct RuntimeState {
    agents: HashMap<String, RuntimeRecord>,
    /// Agent ids are event-routing addresses. Until lifecycle events carry an
    /// explicit incarnation, an id is one-use for this runtime so a delayed
    /// callback can never target a newer process.
    used_agent_ids: HashSet<String>,
}

#[derive(Default)]
struct LaunchCallbackGate {
    open: Mutex<bool>,
    ready: Condvar,
}

impl LaunchCallbackGate {
    fn wait(&self) {
        let open = self
            .open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _open = self
            .ready
            .wait_while(open, |is_open| !*is_open)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }

    fn open(&self) {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *open = true;
        self.ready.notify_all();
    }
}

struct DesktopRuntimeInner {
    state: Mutex<RuntimeState>,
    sink: Arc<dyn DesktopEventSink>,
    governed_task_gateway: Option<Arc<dyn GovernedTaskGateway>>,
    /// Serializes lifecycle publication with the PTY reader's exit callback.
    /// A snapshot is always captured while this lock is held, so a stored
    /// `alive=false` cannot be followed by an older `alive=true` event.
    lifecycle_events: Mutex<()>,
    lifecycle_dispatcher: LifecycleEventDispatcher,
}

impl Drop for DesktopRuntimeInner {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let records = state
            .agents
            .drain()
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        for record in records {
            if let Err(error) = record.backend.kill() {
                let message = format!(
                    "desktop shutdown could not confirm runtime termination for {}: {error}",
                    record
                        .governed_task
                        .as_ref()
                        .map(|task| task.id.as_str())
                        .unwrap_or(&record.command)
                );
                eprintln!("{message}");
                self.sink.emit(DesktopEvent::OpsConnectionUpdate {
                    connected: true,
                    error: Some(message),
                });
                continue;
            }
            if let (Some(task), Some(gateway)) =
                (record.governed_task.as_ref(), &self.governed_task_gateway)
            {
                if let Err(error) = gateway.mutate_current(
                    &task.project_id,
                    &task.id,
                    new_governed_request_id("runtime-drop"),
                    GovernedTaskMutation::MarkRuntimeExited {
                        actor: governed_system_actor("desktop-runtime"),
                        reason: Some("desktop runtime shut down".to_string()),
                    },
                ) {
                    eprintln!(
                        "failed to record governed runtime shutdown for {}: {error}",
                        task.id
                    );
                    self.sink.emit(DesktopEvent::OpsConnectionUpdate {
                        connected: true,
                        error: Some(format!(
                            "governed task {} runtime shutdown was not durably recorded: {error}",
                            task.id
                        )),
                    });
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct DesktopRuntime {
    inner: Arc<DesktopRuntimeInner>,
}

/// Non-owning runtime handle for event sinks that call back into lifecycle
/// methods. Keeping a strong [`DesktopRuntime`] inside its own sink would form
/// an ownership cycle; install this handle after building the runtime instead.
#[derive(Clone)]
pub struct WeakDesktopRuntime {
    inner: Weak<DesktopRuntimeInner>,
}

impl WeakDesktopRuntime {
    pub fn upgrade(&self) -> Option<DesktopRuntime> {
        self.inner.upgrade().map(|inner| DesktopRuntime { inner })
    }
}

impl Default for DesktopRuntime {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl DesktopRuntime {
    pub fn builder() -> DesktopRuntimeBuilder {
        DesktopRuntimeBuilder {
            sink: Arc::new(NoopEventSink),
            governed_task_gateway: None,
        }
    }

    pub fn downgrade(&self) -> WeakDesktopRuntime {
        WeakDesktopRuntime {
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub fn spawn_agent(
        &self,
        request: AgentSpawnRequest,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        let mut request = request;
        if request.role_assignment.is_some() {
            let task = request.task.as_deref().ok_or_else(|| {
                DesktopBridgeError::InvalidTerminalRequest {
                    message: "role assignment requires a nonblank task".to_string(),
                }
            })?;
            if task.trim().is_empty() {
                return Err(DesktopBridgeError::InvalidTerminalRequest {
                    message: "role assignment requires a nonblank task".to_string(),
                });
            }
            if task.contains('\0') {
                return Err(DesktopBridgeError::InvalidTerminalRequest {
                    message: "governed task must not contain NUL bytes".to_string(),
                });
            }
            if task.len() > MAX_GOVERNED_TASK_BYTES {
                return Err(DesktopBridgeError::InvalidTerminalRequest {
                    message: format!(
                        "governed task must be at most {MAX_GOVERNED_TASK_BYTES} UTF-8 bytes"
                    ),
                });
            }
        }
        validate_dimensions(request.rows, request.cols)?;
        bind_governed_workspace(&mut request)?;
        // Centralized registry load (symmetric to load_registry_for_tool in mcp.rs).
        // Keeps the error mapping for spawn in one place and makes future changes cheaper.
        let registry = load_registry_for_spawn()?;
        let command = impulse_ops::agent_registry::resolve_launch_command(
            &registry,
            &request.platform,
            request.command.as_deref(),
        )
        .map_err(|error| DesktopBridgeError::InvalidTerminalRequest {
            message: error.to_string(),
        })?;
        let descriptor = registry.resolve(request.platform.as_str()).ok_or_else(|| {
            DesktopBridgeError::InvalidTerminalRequest {
                message: format!("unknown agent platform `{}`", request.platform),
            }
        })?;
        let platform_label = descriptor.label.clone();
        request.platform = descriptor.id.clone();
        let role_compatibility = request
            .role_assignment
            .as_ref()
            .map(|assignment| {
                registry
                    .evaluate_role_compatibility(&request.platform, assignment)
                    .map_err(|error| DesktopBridgeError::InvalidTerminalRequest {
                        message: format!("role compatibility evaluation failed: {error}"),
                    })
            })
            .transpose()?;
        if let Some(compatibility) = role_compatibility
            .as_ref()
            .filter(|result| result.is_blocked())
        {
            let missing = compatibility
                .checks
                .iter()
                .filter(|check| check.mandatory && !check.is_satisfied())
                .map(|check| {
                    format!(
                        "{} (required={}, available={})",
                        check.capability,
                        enforcement_strength_label(check.required),
                        enforcement_strength_label(check.available)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(DesktopBridgeError::InvalidTerminalRequest {
                message: format!(
                    "role `{}` is incompatible with platform `{}`; mandatory capabilities not satisfied: {missing}; choose a compatible platform or adjust the role requirement",
                    compatibility.role, compatibility.platform
                ),
            });
        }
        let command =
            resolve_executable_command(&request.platform, &command, request.command.is_some());

        let agent_id = request
            .agent_id
            .as_deref()
            .map(sanitize_runtime_id)
            .unwrap_or_else(|| {
                sanitize_runtime_id(&format!("{}-{}", request.platform.as_str(), Uuid::new_v4()))
            });
        let cwd = request.cwd.as_ref().map(PathBuf::from);
        let workspace = request
            .workspace
            .clone()
            .or_else(|| request.cwd.clone().map(WorkspaceTarget::from_root));
        let mcp_tools = if request.mcp_tools.is_empty() {
            default_builtin_mcp_tools()
        } else {
            request.mcp_tools.clone()
        };
        let mut governed_task = if let Some(role_assignment) = request.role_assignment.as_ref() {
            let gateway = self.inner.governed_task_gateway.as_ref().ok_or_else(|| {
                DesktopBridgeError::GovernedTaskFailed {
                    message: "governed launch requires an acknowledged daemon task gateway"
                        .to_string(),
                }
            })?;
            let workspace_root = workspace
                .as_ref()
                .map(|target| target.root.clone())
                .ok_or_else(|| DesktopBridgeError::GovernedTaskFailed {
                    message: "governed launch lost its canonical workspace binding".to_string(),
                })?;
            let project_id = governed_project_id(&workspace_root)?;
            let task =
                request
                    .task
                    .clone()
                    .ok_or_else(|| DesktopBridgeError::GovernedTaskFailed {
                        message: "governed launch lost its validated task description".to_string(),
                    })?;
            let task_id = new_governed_task_id();
            let mut registration = GovernedTaskRegistration::builder(
                new_governed_request_id("register").to_string(),
                task_id.to_string(),
                project_id.clone(),
                workspace_root.clone(),
                task.clone(),
                agent_id.clone(),
                request.platform.as_str(),
            )
            .role_assignment(role_assignment.clone());
            if let Some(compatibility) = role_compatibility.clone() {
                registration = registration.role_compatibility(compatibility);
            }
            if let Some(session_id) = request.session_id.clone() {
                registration = registration.session_id(session_id);
            }
            let registration =
                registration
                    .build()
                    .map_err(|error| DesktopBridgeError::GovernedTaskFailed {
                        message: format!("build daemon registration: {error}"),
                    })?;
            let registered = match gateway.register(registration.clone()) {
                Ok(registered) => registered,
                Err(error) => {
                    let cleanup = record_launch_abort_bound(
                        gateway.as_ref(),
                        &project_id,
                        &task_id,
                        "registration acknowledgment failed before PTY creation",
                    )
                    .err();
                    return Err(DesktopBridgeError::GovernedTaskFailed {
                        message: match cleanup {
                            Some(cleanup) => format!(
                                "register before PTY creation: {error}; registration cleanup: {cleanup}"
                            ),
                            None => format!("register before PTY creation: {error}"),
                        },
                    });
                }
            };
            if let Err(validation_error) = validate_registered_task(&registered, &registration) {
                let abort_error = record_launch_abort_bound(
                    gateway.as_ref(),
                    &project_id,
                    &task_id,
                    "daemon returned an invalid registration acknowledgment",
                )
                .err();
                return Err(DesktopBridgeError::GovernedTaskFailed {
                    message: match abort_error {
                        Some(abort_error) => format!(
                            "{validation_error}; additionally failed to record launch rejection: {abort_error}"
                        ),
                        None => validation_error.to_string(),
                    },
                });
            }
            Some(registered)
        } else {
            None
        };
        {
            let mut state = self.lock_state();
            if !state.used_agent_ids.insert(agent_id.clone()) {
                if let (Some(task), Some(gateway)) = (
                    governed_task.as_ref(),
                    self.inner.governed_task_gateway.as_ref(),
                ) {
                    let _ = record_launch_abort(
                        gateway.as_ref(),
                        task,
                        "agent id was already used before PTY creation",
                    );
                }
                return Err(DesktopBridgeError::InvalidTerminalRequest {
                    message: format!(
                        "agent id `{agent_id}` has already been used by this runtime; choose a new id"
                    ),
                });
            }
        }
        let env_pairs = runtime_env(
            &agent_id,
            &request,
            &command,
            workspace.as_ref(),
            governed_task.as_ref(),
        );
        // PTY callbacks are per-launch gated until the daemon acknowledges
        // Running and the runtime record is installed. This keeps one slow
        // daemon round-trip from blocking lifecycle/output delivery for every
        // other agent behind the global ordering lock.
        let launch_callback_gate = Arc::new(LaunchCallbackGate::default());
        let output_runtime = Arc::downgrade(&self.inner);
        let output_agent_id = agent_id.clone();
        let output_launch_gate = Arc::clone(&launch_callback_gate);
        let output_callback = Arc::new(move |data: &[u8]| {
            output_launch_gate.wait();
            let Some(runtime) = output_runtime.upgrade() else {
                return;
            };
            let should_drain = {
                let _lifecycle_guard = runtime
                    .lifecycle_events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let active = runtime
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .agents
                    .contains_key(&output_agent_id);
                active
                    && runtime
                        .lifecycle_dispatcher
                        .enqueue(DesktopEvent::TerminalOutput {
                            agent_id: output_agent_id.clone(),
                            data: data.to_vec(),
                        })
            };
            if should_drain {
                runtime.lifecycle_dispatcher.drain();
            }
        });
        let exit_agent_id = agent_id.clone();
        let exit_runtime = Arc::downgrade(&self.inner);
        let exit_launch_gate = Arc::clone(&launch_callback_gate);
        let exit_governed_task = governed_task
            .as_ref()
            .map(|task| (task.project_id.clone(), task.id.clone()));
        let exit_governed_gateway = self.inner.governed_task_gateway.clone();
        let exit_callback = Arc::new(move || {
            exit_launch_gate.wait();
            let Some(runtime) = exit_runtime.upgrade() else {
                return;
            };
            let removed = {
                let _lifecycle_guard = runtime
                    .lifecycle_events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                runtime
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .agents
                    .remove(&exit_agent_id)
            };
            let Some(record) = removed else {
                return;
            };
            if let Err(error) = record.backend.kill() {
                let message = format!(
                    "terminal EOF observed for {exit_agent_id}, but its process could not be confirmed terminated: {error}"
                );
                let should_drain = {
                    let _lifecycle_guard = runtime
                        .lifecycle_events
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    runtime
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .agents
                        .insert(exit_agent_id.clone(), record);
                    runtime
                        .lifecycle_dispatcher
                        .enqueue(DesktopEvent::OpsConnectionUpdate {
                            connected: true,
                            error: Some(message),
                        })
                };
                if should_drain {
                    runtime.lifecycle_dispatcher.drain();
                }
                return;
            }

            let should_drain = {
                let _lifecycle_guard = runtime
                    .lifecycle_events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                runtime
                    .lifecycle_dispatcher
                    .enqueue(DesktopEvent::TerminalExit {
                        agent_id: exit_agent_id.clone(),
                    })
            };
            drop(record);
            if let (Some((project_id, task_id)), Some(gateway)) =
                (&exit_governed_task, &exit_governed_gateway)
            {
                if let Err(error) = gateway.mutate_current(
                    project_id,
                    task_id,
                    new_governed_request_id("runtime-exit"),
                    GovernedTaskMutation::MarkRuntimeExited {
                        actor: governed_system_actor("desktop-runtime"),
                        reason: None,
                    },
                ) {
                    eprintln!(
                        "failed to record governed runtime exit for {}: {error}",
                        task_id
                    );
                    runtime
                        .lifecycle_dispatcher
                        .enqueue(DesktopEvent::OpsConnectionUpdate {
                            connected: true,
                            error: Some(format!(
                                "governed task {task_id} runtime exit was not durably recorded: {error}"
                            )),
                        });
                }
            }
            if should_drain {
                runtime.lifecycle_dispatcher.drain();
            }
        });

        let backend = match TerminalBackend::spawn_with_callbacks(
            &command,
            &request.args,
            cwd.as_deref(),
            &env_pairs,
            request.rows,
            request.cols,
            None,
            Some(output_callback),
            Some(exit_callback),
        ) {
            Ok(backend) => backend,
            Err(error) => {
                self.lock_state().used_agent_ids.remove(&agent_id);
                launch_callback_gate.open();
                if let (Some(task), Some(gateway)) = (
                    governed_task.as_ref(),
                    self.inner.governed_task_gateway.as_ref(),
                ) {
                    if let Err(record_error) =
                        record_launch_abort(gateway.as_ref(), task, &error.to_string())
                    {
                        return Err(DesktopBridgeError::TerminalSpawnFailed {
                            message: format!(
                                "{error}; additionally failed to record governed launch failure: {record_error}"
                            ),
                        });
                    }
                }
                return Err(DesktopBridgeError::TerminalSpawnFailed {
                    message: error.to_string(),
                });
            }
        };

        if let (Some(task), Some(gateway)) = (
            governed_task.as_ref(),
            self.inner.governed_task_gateway.as_ref(),
        ) {
            let running = gateway.mutate(GovernedTaskMutationRequest {
                request_id: new_governed_request_id("running"),
                project_id: task.project_id.clone(),
                task_id: task.id.clone(),
                expected_revision: task.revision,
                mutation: GovernedTaskMutation::MarkRunning {
                    actor: governed_system_actor("desktop-runtime"),
                },
            });
            match running {
                Ok(updated) => {
                    if let Err(validation_error) = validate_running_task(&updated, task) {
                        if let Err(termination_error) = backend.kill() {
                            launch_callback_gate.open();
                            return Err(DesktopBridgeError::GovernedTaskFailed {
                                message: format!(
                                    "{validation_error}; PTY termination could not be confirmed, so no exit state was recorded: {termination_error}"
                                ),
                            });
                        }
                        let abort_error = record_launch_abort(
                            gateway.as_ref(),
                            task,
                            "daemon returned an invalid running acknowledgment",
                        )
                        .err();
                        launch_callback_gate.open();
                        return Err(DesktopBridgeError::GovernedTaskFailed {
                            message: match abort_error {
                                Some(abort_error) => format!(
                                    "{validation_error}; additionally failed to record runtime abort: {abort_error}"
                                ),
                                None => validation_error.to_string(),
                            },
                        });
                    }
                    governed_task = Some(updated);
                }
                Err(error) => {
                    if let Err(termination_error) = backend.kill() {
                        launch_callback_gate.open();
                        return Err(DesktopBridgeError::GovernedTaskFailed {
                            message: format!(
                                "mark task running after PTY creation: {error}; PTY termination could not be confirmed, so no exit state was recorded: {termination_error}"
                            ),
                        });
                    }
                    let abort_error = record_launch_abort(
                        gateway.as_ref(),
                        task,
                        "runtime started but daemon running acknowledgment failed",
                    )
                    .err();
                    launch_callback_gate.open();
                    return Err(DesktopBridgeError::GovernedTaskFailed {
                        message: match abort_error {
                            Some(abort_error) => format!(
                                "mark task running after PTY creation: {error}; abort record also failed: {abort_error}"
                            ),
                            None => format!("mark task running after PTY creation: {error}"),
                        },
                    });
                }
            }
        }

        let record = RuntimeRecord {
            platform: request.platform.clone(),
            label: platform_label,
            command,
            args: request.args,
            cwd,
            workspace,
            session_id: request.session_id,
            governed_task,
            current_task: request.task,
            role: request.role,
            role_assignment: request.role_assignment,
            role_compatibility,
            target: request.target,
            mcp_tools,
            focused: false,
            status: AgentStatus::Starting,
            backend,
        };
        let lifecycle_guard = self
            .inner
            .lifecycle_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = {
            let mut state = self.lock_state();
            let replaced = state.agents.insert(agent_id.clone(), record);
            debug_assert!(replaced.is_none(), "reserved agent id replaced a runtime");
            state
                .agents
                .get(&agent_id)
                .expect("inserted runtime record")
                .snapshot(&agent_id)
        };
        let should_drain =
            self.inner
                .lifecycle_dispatcher
                .enqueue(DesktopEvent::AgentRuntimeUpdate {
                    snapshot: Box::new(snapshot.clone()),
                });
        launch_callback_gate.open();
        drop(lifecycle_guard);
        if should_drain {
            self.inner.lifecycle_dispatcher.drain();
        }
        Ok(snapshot)
    }

    pub fn write_agent(&self, request: AgentWriteRequest) -> Result<(), DesktopBridgeError> {
        let state = self.lock_state();
        let record = state.agents.get(&request.agent_id).ok_or_else(|| {
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.agent_id.clone(),
            }
        })?;
        record
            .backend
            .write_queue()
            .write_user_input(&request.data)
            .map_err(|error| DesktopBridgeError::TerminalWriteFailed {
                message: error.to_string(),
            })
    }

    pub fn mutate_governed_task(
        &self,
        request: GovernedTaskMutationRequest,
    ) -> Result<GovernedTaskRun, DesktopBridgeError> {
        let gateway = self.inner.governed_task_gateway.as_ref().ok_or_else(|| {
            DesktopBridgeError::GovernedTaskFailed {
                message: "daemon task gateway is unavailable".to_string(),
            }
        })?;
        let expected_task_id = request.task_id.clone();
        let expected_project_id = request.project_id.clone();
        let expected_revision = request.expected_revision;
        let updated = gateway
            .mutate(request)
            .map_err(|message| DesktopBridgeError::GovernedTaskFailed { message })?;
        if updated.id != expected_task_id
            || updated.project_id != expected_project_id
            || updated.revision <= expected_revision
        {
            return Err(DesktopBridgeError::GovernedTaskFailed {
                message: "daemon mutation acknowledgment did not match the requested governed task"
                    .to_string(),
            });
        }

        let lifecycle_guard = self
            .inner
            .lifecycle_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = {
            let mut state = self.lock_state();
            let record = state.agents.get_mut(&updated.agent_id).filter(|record| {
                record
                    .governed_task
                    .as_ref()
                    .is_some_and(|current| current.id == updated.id)
            });
            match record {
                Some(record) => {
                    let current = record
                        .governed_task
                        .as_ref()
                        .expect("filtered governed task record");
                    if !same_governed_task_identity(&updated, current) {
                        return Err(DesktopBridgeError::GovernedTaskFailed {
                            message: "daemon mutation acknowledgment changed immutable governed task identity"
                                .to_string(),
                        });
                    }
                    record.governed_task = Some(updated.clone());
                    Some(record.snapshot(&updated.agent_id))
                }
                None => None,
            }
        };
        let should_drain = snapshot.is_some_and(|snapshot| {
            self.inner
                .lifecycle_dispatcher
                .enqueue(DesktopEvent::AgentRuntimeUpdate {
                    snapshot: Box::new(snapshot),
                })
        });
        drop(lifecycle_guard);
        if should_drain {
            self.inner.lifecycle_dispatcher.drain();
        }
        Ok(updated)
    }

    pub fn resize_agent(
        &self,
        request: TerminalResizeRequest,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        validate_dimensions(request.rows, request.cols)?;
        {
            let state = self.lock_state();
            let record = state.agents.get(&request.session_id).ok_or_else(|| {
                DesktopBridgeError::MissingTerminalSession {
                    session_id: request.session_id.clone(),
                }
            })?;
            record
                .backend
                .resize(request.cols, request.rows)
                .map_err(|error| DesktopBridgeError::TerminalWriteFailed {
                    message: error.to_string(),
                })?;
        }
        self.emit_runtime_snapshot(&request.session_id)
    }

    pub fn focus_agent(
        &self,
        request: TerminalFocusRequest,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        {
            let mut state = self.lock_state();
            if !state.agents.contains_key(&request.session_id) {
                return Err(DesktopBridgeError::MissingTerminalSession {
                    session_id: request.session_id,
                });
            }
            for record in state.agents.values_mut() {
                record.focused = false;
            }
            let Some(record) = state.agents.get_mut(&request.session_id) else {
                return Err(DesktopBridgeError::MissingTerminalSession {
                    session_id: request.session_id,
                });
            };
            record.focused = true;
        }
        self.emit_runtime_snapshot(&request.session_id)
    }

    pub fn close_agent(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError> {
        let lifecycle_guard = self
            .inner
            .lifecycle_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = self.lock_state().agents.remove(&request.session_id).ok_or(
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            },
        )?;
        let governed_task = record.governed_task.clone();
        if let Err(error) = record.backend.kill() {
            self.lock_state()
                .agents
                .insert(request.session_id.clone(), record);
            return Err(DesktopBridgeError::TerminalTerminationFailed {
                message: format!(
                    "session {} remains managed because process termination was not confirmed: {error}",
                    request.session_id
                ),
            });
        }
        let should_drain = self
            .inner
            .lifecycle_dispatcher
            .enqueue(DesktopEvent::TerminalExit {
                agent_id: request.session_id,
            });
        drop(record);
        drop(lifecycle_guard);
        let governed_error = if let (Some(task), Some(gateway)) = (
            governed_task.as_ref(),
            self.inner.governed_task_gateway.as_ref(),
        ) {
            gateway
                .mutate_current(
                    &task.project_id,
                    &task.id,
                    new_governed_request_id("runtime-close"),
                    GovernedTaskMutation::MarkRuntimeExited {
                        actor: governed_system_actor("desktop-runtime"),
                        reason: Some("operator closed runtime".to_string()),
                    },
                )
                .err()
        } else {
            None
        };
        if should_drain {
            self.inner.lifecycle_dispatcher.drain();
        }
        if let Some(error) = governed_error {
            return Err(DesktopBridgeError::GovernedTaskFailed {
                message: format!("runtime closed but durable exit recording failed: {error}"),
            });
        }
        Ok(())
    }

    pub fn snapshot_agents(&self) -> Vec<AgentRuntimeSnapshot> {
        let state = self.lock_state();
        let mut snapshots = state
            .agents
            .iter()
            .filter_map(|(agent_id, record)| {
                let snapshot = record.snapshot(agent_id);
                snapshot.alive.then_some(snapshot)
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| {
            right
                .focused
                .cmp(&left.focused)
                .then_with(|| left.label.cmp(&right.label))
        });
        snapshots
    }

    fn emit_runtime_snapshot(
        &self,
        agent_id: &str,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        let lifecycle_guard = self
            .inner
            .lifecycle_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = {
            let state = self.lock_state();
            let record = state.agents.get(agent_id).ok_or_else(|| {
                DesktopBridgeError::MissingTerminalSession {
                    session_id: agent_id.to_string(),
                }
            })?;
            record.snapshot(agent_id)
        };
        let should_drain =
            self.inner
                .lifecycle_dispatcher
                .enqueue(DesktopEvent::AgentRuntimeUpdate {
                    snapshot: Box::new(snapshot.clone()),
                });
        drop(lifecycle_guard);
        if should_drain {
            self.inner.lifecycle_dispatcher.drain();
        }
        Ok(snapshot)
    }

    pub fn dispatch_supervisor_local_action(
        &self,
        request: SupervisorLocalActionRequest,
    ) -> Result<(), DesktopBridgeError> {
        self.inner.sink.emit(DesktopEvent::SupervisorLocalAction {
            action: request.action.clone(),
        });
        match request.action {
            LocalSupervisorAction::FocusAgent { agent_id } => {
                self.focus_agent(TerminalFocusRequest {
                    session_id: agent_id,
                })?;
            }
            LocalSupervisorAction::SendInput {
                agent_id,
                content,
                confirmed,
            } => {
                if !confirmed {
                    return Err(DesktopBridgeError::InvalidTerminalRequest {
                        message: "supervisor send_input requires confirmation".to_string(),
                    });
                }
                self.write_agent(AgentWriteRequest {
                    agent_id,
                    data: content.into_bytes(),
                })?;
            }
        }
        Ok(())
    }

    fn lock_state(&self) -> MutexGuard<'_, RuntimeState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TerminalBridge for DesktopRuntime {
    fn open(
        &self,
        request: TerminalOpenRequest,
    ) -> Result<TerminalSessionResponse, DesktopBridgeError> {
        let registry = load_registry_for_spawn()?;
        let platform = registry
            .detect_from_command(&request.command)
            .or_else(|| registry.get("shell"))
            .map(|descriptor| descriptor.id.clone())
            .ok_or_else(|| DesktopBridgeError::InvalidTerminalRequest {
                message: "agent registry has no shell fallback".to_string(),
            })?;
        let agent_id = request.session_id.clone();
        let snapshot = self.spawn_agent(AgentSpawnRequest {
            agent_id,
            session_id: request.session_id,
            platform,
            command: Some(request.command),
            args: request.args,
            cwd: request.cwd,
            env: request.env,
            workspace: request.workspace,
            mcp_tools: request.mcp_tools,
            rows: request.rows,
            cols: request.cols,
            role: None,
            task: None,
            role_assignment: None,
            target: None,
        })?;
        Ok(TerminalSessionResponse {
            session_id: snapshot.agent_id,
            alive: snapshot.alive,
            rows: snapshot.rows,
            cols: snapshot.cols,
        })
    }

    fn write(&self, request: TerminalWriteRequest) -> Result<(), DesktopBridgeError> {
        self.write_agent(AgentWriteRequest {
            agent_id: request.session_id,
            data: request.data,
        })
    }

    fn resize(&self, request: TerminalResizeRequest) -> Result<(), DesktopBridgeError> {
        self.resize_agent(request).map(|_| ())
    }

    fn close(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError> {
        self.close_agent(request)
    }

    fn focus(&self, request: TerminalFocusRequest) -> Result<(), DesktopBridgeError> {
        self.focus_agent(request).map(|_| ())
    }
}

pub struct DesktopRuntimeBuilder {
    sink: Arc<dyn DesktopEventSink>,
    governed_task_gateway: Option<Arc<dyn GovernedTaskGateway>>,
}

impl DesktopRuntimeBuilder {
    pub fn with_event_sink(mut self, sink: Arc<dyn DesktopEventSink>) -> Self {
        self.sink = sink;
        self
    }

    pub fn with_governed_task_gateway(mut self, gateway: Arc<dyn GovernedTaskGateway>) -> Self {
        self.governed_task_gateway = Some(gateway);
        self
    }

    pub fn build(self) -> DesktopRuntime {
        DesktopRuntime {
            inner: Arc::new(DesktopRuntimeInner {
                state: Mutex::new(RuntimeState::default()),
                lifecycle_dispatcher: LifecycleEventDispatcher::new(Arc::clone(&self.sink)),
                sink: self.sink,
                governed_task_gateway: self.governed_task_gateway,
                lifecycle_events: Mutex::new(()),
            }),
        }
    }
}

fn governed_project_id(workspace_root: &str) -> Result<String, DesktopBridgeError> {
    let root = Path::new(workspace_root);
    let name = root
        .file_name()
        .and_then(|segment| segment.to_str())
        .unwrap_or("impulse-project");
    let project_id = impulse_ops::sanitize_id(name);
    if project_id == "unknown" {
        return Err(DesktopBridgeError::GovernedTaskFailed {
            message: format!(
                "canonical workspace `{workspace_root}` does not provide a stable project identity"
            ),
        });
    }
    Ok(project_id)
}

fn new_governed_request_id(prefix: &str) -> impulse_ops::governed_task::GovernedRequestId {
    impulse_ops::governed_task::GovernedRequestId::try_new(format!(
        "desktop-{prefix}-{}",
        Uuid::new_v4()
    ))
    .expect("generated desktop governed request id must be valid")
}

fn new_governed_task_id() -> impulse_ops::governed_task::GovernedTaskId {
    impulse_ops::governed_task::GovernedTaskId::try_new(format!("task-{}", Uuid::new_v4()))
        .expect("generated desktop governed task id must be valid")
}

fn governed_system_actor(id: &str) -> impulse_ops::governed_task::GovernedActor {
    impulse_ops::governed_task::GovernedActor {
        kind: impulse_ops::governed_task::GovernedActorKind::System,
        id: id.to_string(),
    }
}

fn validate_registered_task(
    task: &GovernedTaskRun,
    registration: &GovernedTaskRegistration,
) -> Result<(), DesktopBridgeError> {
    let mismatched = task.id != registration.task_id
        || task.project_id != registration.project_id
        || task.workspace_root != registration.workspace_root
        || task.task != registration.task
        || task.acceptance_criteria != registration.acceptance_criteria
        || task.approval_policy != registration.approval_policy
        || task.role_assignment != registration.role_assignment
        || task.role_compatibility != registration.role_compatibility
        || task.agent_id != registration.agent_id
        || task.runtime_id != registration.runtime_id
        || task.session_id != registration.session_id
        || task.initial_subject_revision != registration.initial_subject_revision;
    if mismatched {
        return Err(DesktopBridgeError::GovernedTaskFailed {
            message: "daemon registration response did not match the validated launch assignment"
                .to_string(),
        });
    }
    if task.id.as_str() == registration.agent_id
        || registration.session_id.as_deref() == Some(task.id.as_str())
    {
        return Err(DesktopBridgeError::GovernedTaskFailed {
            message: "daemon conflated governed task identity with agent/session routing identity"
                .to_string(),
        });
    }
    if task.execution_state != impulse_ops::governed_task::GovernedExecutionState::Registered
        || task.revision != 0
        || task.review_state != impulse_ops::governed_task::GovernedReviewState::AwaitingClaim
        || !task.claims.is_empty()
        || !task.verifications.is_empty()
        || !task.supervisor_verdicts.is_empty()
        || !task.operator_decisions.is_empty()
    {
        return Err(DesktopBridgeError::GovernedTaskFailed {
            message: "daemon registration did not return a fresh revision-zero registered task"
                .to_string(),
        });
    }
    Ok(())
}

fn validate_running_task(
    running: &GovernedTaskRun,
    registered: &GovernedTaskRun,
) -> Result<(), DesktopBridgeError> {
    let expected_revision = registered.revision.checked_add(1).ok_or_else(|| {
        DesktopBridgeError::GovernedTaskFailed {
            message: "registered governed task revision cannot advance".to_string(),
        }
    })?;
    if !same_governed_task_identity(running, registered)
        || running.revision != expected_revision
        || running.execution_state != impulse_ops::governed_task::GovernedExecutionState::Running
        || running.review_state != impulse_ops::governed_task::GovernedReviewState::AwaitingClaim
        || !running.claims.is_empty()
        || !running.verifications.is_empty()
        || !running.supervisor_verdicts.is_empty()
        || !running.operator_decisions.is_empty()
    {
        return Err(DesktopBridgeError::GovernedTaskFailed {
            message: "daemon running acknowledgment did not match the registered launch"
                .to_string(),
        });
    }
    Ok(())
}

fn same_governed_task_identity(left: &GovernedTaskRun, right: &GovernedTaskRun) -> bool {
    left.id == right.id
        && left.project_id == right.project_id
        && left.workspace_root == right.workspace_root
        && left.task == right.task
        && left.acceptance_criteria == right.acceptance_criteria
        && left.approval_policy == right.approval_policy
        && left.role_assignment == right.role_assignment
        && left.role_compatibility == right.role_compatibility
        && left.runtime_id == right.runtime_id
        && left.agent_id == right.agent_id
        && left.session_id == right.session_id
        && left.initial_subject_revision == right.initial_subject_revision
}

fn record_launch_abort(
    gateway: &dyn GovernedTaskGateway,
    task: &GovernedTaskRun,
    reason: &str,
) -> Result<GovernedTaskRun, String> {
    record_launch_abort_bound(gateway, &task.project_id, &task.id, reason)
}

fn record_launch_abort_bound(
    gateway: &dyn GovernedTaskGateway,
    project_id: &str,
    task_id: &impulse_ops::governed_task::GovernedTaskId,
    reason: &str,
) -> Result<GovernedTaskRun, String> {
    let launch_failed = gateway.mutate_current(
        project_id,
        task_id,
        new_governed_request_id("launch-failed"),
        GovernedTaskMutation::MarkLaunchFailed {
            actor: governed_system_actor("desktop-runtime"),
            reason: reason.to_string(),
        },
    );
    match launch_failed {
        Ok(task) => Ok(task),
        Err(launch_error) => gateway
            .mutate_current(
                project_id,
                task_id,
                new_governed_request_id("launch-aborted-exit"),
                GovernedTaskMutation::MarkRuntimeExited {
                    actor: governed_system_actor("desktop-runtime"),
                    reason: Some(reason.to_string()),
                },
            )
            .map_err(|exit_error| {
                format!(
                    "mark launch failed: {launch_error}; mark started runtime exited: {exit_error}"
                )
            }),
    }
}

/// Load the canonical AgentRegistry (from impulse-ops) and map failure to the
/// runtime bridge error used by spawn/terminal paths. Mirrors the pattern
/// introduced in mcp.rs (load_registry_for_tool) for consistency and to keep
/// the mapping in one place per domain.
fn load_registry_for_spawn(
) -> Result<impulse_ops::agent_registry::AgentRegistry, DesktopBridgeError> {
    impulse_ops::agent_registry::AgentRegistry::registry_for_runtime().map_err(|e| {
        DesktopBridgeError::InvalidTerminalRequest {
            message: format!("registry load: {e}"),
        }
    })
}

fn resolve_executable_command(
    platform: &AgentPlatformId,
    command: &str,
    explicit_override: bool,
) -> String {
    if explicit_override {
        return command.to_string();
    }
    let current_exe = std::env::current_exe().ok();
    let path = std::env::var_os("PATH");
    resolve_executable_command_with(platform, command, current_exe.as_deref(), path.as_deref())
}

fn resolve_executable_command_with(
    platform: &AgentPlatformId,
    command: &str,
    current_exe: Option<&Path>,
    path: Option<&std::ffi::OsStr>,
) -> String {
    let command_path = Path::new(command);
    if command.is_empty() || command.trim() != command || command_path.components().count() != 1 {
        return command.to_string();
    }

    let path_roots = path
        .map(std::env::split_paths)
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    if let Some(resolved) = find_executable_in_roots(command, &path_roots) {
        return resolved;
    }

    if platform.as_str() != "ion" {
        return command.to_string();
    }

    let mut sibling_roots = Vec::new();
    if let Some(parent) = current_exe.and_then(Path::parent) {
        sibling_roots.push(parent.to_path_buf());
        if parent.file_name().and_then(|value| value.to_str()) == Some("deps") {
            if let Some(target_profile) = parent.parent() {
                sibling_roots.push(target_profile.to_path_buf());
            }
        }
    }
    find_executable_in_roots(command, &sibling_roots).unwrap_or_else(|| command.to_string())
}

fn find_executable_in_roots(command: &str, roots: &[PathBuf]) -> Option<String> {
    let command_path = Path::new(command);
    let mut candidate_names = vec![command_path.to_path_buf()];
    if !std::env::consts::EXE_SUFFIX.is_empty() && command_path.extension().is_none() {
        candidate_names.push(PathBuf::from(format!(
            "{command}{}",
            std::env::consts::EXE_SUFFIX
        )));
    }

    let mut visited = HashSet::new();
    for root in roots {
        for candidate_name in &candidate_names {
            let candidate = root.join(candidate_name);
            if visited.insert(candidate.clone()) && is_executable_file(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn validate_dimensions(rows: u16, cols: u16) -> Result<(), DesktopBridgeError> {
    if rows == 0 || cols == 0 {
        return Err(DesktopBridgeError::InvalidTerminalRequest {
            message: "terminal rows and cols must be greater than zero".to_string(),
        });
    }
    Ok(())
}

fn bind_governed_workspace(request: &mut AgentSpawnRequest) -> Result<(), DesktopBridgeError> {
    if request.role_assignment.is_none() {
        return Ok(());
    }

    let cwd = request.cwd.as_deref().ok_or_else(|| {
        DesktopBridgeError::InvalidTerminalRequest {
            message: "governed role launch requires both `workspace.root` and `cwd`; select an existing workspace directory and retry"
                .to_string(),
        }
    })?;
    let workspace_root = request
        .workspace
        .as_ref()
        .map(|workspace| workspace.root.as_str())
        .ok_or_else(|| DesktopBridgeError::InvalidTerminalRequest {
            message: "governed role launch requires both `workspace.root` and `cwd`; select an existing workspace directory and retry"
                .to_string(),
        })?;

    let canonical_cwd = canonical_governed_directory("cwd", cwd)?;
    let canonical_workspace = canonical_governed_directory("workspace.root", workspace_root)?;
    if canonical_cwd != canonical_workspace {
        return Err(DesktopBridgeError::InvalidTerminalRequest {
            message: format!(
                "governed role launch requires `workspace.root` and `cwd` to resolve to the same canonical directory (workspace=`{}`, cwd=`{}`); select one workspace directory and retry",
                canonical_workspace.display(),
                canonical_cwd.display()
            ),
        });
    }

    let canonical_root = canonical_workspace
        .to_str()
        .ok_or_else(|| DesktopBridgeError::InvalidTerminalRequest {
            message: format!(
                "governed role launch workspace `{}` cannot be represented as UTF-8; select a different workspace directory",
                canonical_workspace.display()
            ),
        })?
        .to_string();
    request.cwd = Some(canonical_root.clone());
    if let Some(workspace) = request.workspace.as_mut() {
        workspace.root = canonical_root;
    }
    Ok(())
}

fn canonical_governed_directory(field: &str, value: &str) -> Result<PathBuf, DesktopBridgeError> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(DesktopBridgeError::InvalidTerminalRequest {
            message: format!(
                "governed role launch `{field}` path `{value}` must be absolute; select an absolute workspace directory and retry"
            ),
        });
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        DesktopBridgeError::InvalidTerminalRequest {
            message: format!(
                "governed role launch `{field}` path `{value}` must resolve to an existing directory: {error}; select a valid workspace directory and retry"
            ),
        }
    })?;
    if !canonical.is_dir() {
        return Err(DesktopBridgeError::InvalidTerminalRequest {
            message: format!(
                "governed role launch `{field}` path `{value}` is not a directory; select a valid workspace directory and retry"
            ),
        });
    }
    Ok(canonical)
}

fn enforcement_strength_label(strength: EnforcementStrength) -> &'static str {
    match strength {
        EnforcementStrength::Unsupported => "unsupported",
        EnforcementStrength::Advisory => "advisory",
        EnforcementStrength::Mediated => "mediated",
        EnforcementStrength::Structural => "structural",
    }
}

fn sanitize_runtime_id(value: &str) -> String {
    let sanitized = impulse_ops::sanitize_id(value);
    if sanitized == "unknown" {
        format!("agent-{}", Uuid::new_v4())
    } else {
        sanitized
    }
}

fn runtime_env(
    agent_id: &str,
    request: &AgentSpawnRequest,
    command: &str,
    workspace: Option<&WorkspaceTarget>,
    governed_task: Option<&GovernedTaskRun>,
) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("IMPULSE_AGENT_ID", agent_id.to_string()),
        ("IMPULSE_PLATFORM", request.platform.as_str().to_string()),
        ("IMPULSE_COMMAND", command.to_string()),
        ("IMPULSE_TERM_PROGRAM", "impulse-desktop".to_string()),
        ("IMPULSE_TERM_ROWS", request.rows.to_string()),
        ("IMPULSE_TERM_COLS", request.cols.to_string()),
    ];
    if let Some(session_id) = &request.session_id {
        env.push(("IMPULSE_SESSION_ID", session_id.clone()));
    }
    if let Some(task) = &request.task {
        env.push(("IMPULSE_TASK", task.clone()));
    }
    if let Some(task) = governed_task {
        env.push(("IMPULSE_GOVERNED_TASK_ID", task.id.to_string()));
    }
    if let Some(assignment) = &request.role_assignment {
        env.push(("IMPULSE_ROLE_ID", assignment.role.to_string()));
    }
    if let Some(workspace) = workspace {
        env.push(("IMPULSE_WORKSPACE_ROOT", workspace.root.clone()));
        env.push(("IMPULSE_PROJECT", workspace.root.clone()));
        if let Some(label) = &workspace.label {
            env.push(("IMPULSE_WORKSPACE_LABEL", label.clone()));
            env.push(("IMPULSE_PROJECT_LABEL", label.clone()));
        }
        if let Some(purpose) = &workspace.purpose {
            env.push(("IMPULSE_WORKSPACE_PURPOSE", purpose.clone()));
            env.push(("IMPULSE_PROJECT_PURPOSE", purpose.clone()));
        }
        if let Some(project_notes) = &workspace.project_notes {
            env.push((
                "IMPULSE_PROJECT_CONTEXT_SOURCE",
                "workspace_registry".to_string(),
            ));
            env.push((
                "IMPULSE_PROJECT_NOTES_HASH",
                project_notes_hash(project_notes),
            ));
        }
    }
    for (key, value) in &request.env {
        if let Some(key) = static_env_key(key) {
            env.push((key, value.clone()));
        }
    }
    env
}

fn static_env_key(key: &str) -> Option<&'static str> {
    match key {
        "IMPULSE_CAPABILITIES_PATH" => Some("IMPULSE_CAPABILITIES_PATH"),
        "IMPULSE_HOME" => Some("IMPULSE_HOME"),
        "TERM" => Some("TERM"),
        _ => None,
    }
}

pub(crate) fn project_notes_hash(notes: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in notes.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(feature = "legacy-tauri-runtime")]
pub struct LegacyTauriEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

#[cfg(feature = "legacy-tauri-runtime")]
impl<R: tauri::Runtime> LegacyTauriEventSink<R> {
    pub fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

#[cfg(feature = "legacy-tauri-runtime")]
impl<R: tauri::Runtime> DesktopEventSink for LegacyTauriEventSink<R> {
    fn emit(&self, event: DesktopEvent) {
        use tauri::Emitter;

        let _ = self.app.emit(event.name(), &event);
    }
}

const _: () = {
    fn _assert_send_sync<T: Send + Sync>() {}
    let _ = _assert_send_sync::<DesktopRuntime>;
};

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_ops::governed_task::{
        ApprovalPolicy, GovernedExecutionState, GovernedReviewState, GovernedTaskId,
    };
    use impulse_ops::role_assignment::{
        AgentRoleAssignment, AgentRoleId, EnforcementStrength, RoleCapabilityRequirement,
        RuntimeCapabilityId,
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn platform_id(value: &str) -> AgentPlatformId {
        AgentPlatformId::try_new(value).expect("valid test platform id")
    }

    fn role_assignment(
        capability: &str,
        minimum_enforcement: EnforcementStrength,
        mandatory: bool,
    ) -> AgentRoleAssignment {
        AgentRoleAssignment {
            role: AgentRoleId::try_new("builder").expect("valid test role id"),
            requirements: vec![RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new(capability)
                    .expect("valid test capability id"),
                minimum_enforcement,
                mandatory,
            }],
        }
    }

    #[derive(Default)]
    struct TestGovernedTaskGateway {
        tasks: Mutex<HashMap<GovernedTaskId, GovernedTaskRun>>,
        reject_registration: bool,
        corrupt_registration: bool,
        reject_running: bool,
        corrupt_running: bool,
    }

    impl TestGovernedTaskGateway {
        fn rejecting() -> Self {
            Self {
                tasks: Mutex::new(HashMap::new()),
                reject_registration: true,
                corrupt_registration: false,
                reject_running: false,
                corrupt_running: false,
            }
        }

        fn corrupting_registration() -> Self {
            Self {
                corrupt_registration: true,
                ..Self::default()
            }
        }

        fn rejecting_running() -> Self {
            Self {
                reject_running: true,
                ..Self::default()
            }
        }

        fn corrupting_running() -> Self {
            Self {
                corrupt_running: true,
                ..Self::default()
            }
        }

        fn task(&self, id: &GovernedTaskId) -> Option<GovernedTaskRun> {
            self.tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(id)
                .cloned()
        }
    }

    impl GovernedTaskGateway for TestGovernedTaskGateway {
        fn register(
            &self,
            registration: GovernedTaskRegistration,
        ) -> Result<GovernedTaskRun, String> {
            if self.reject_registration {
                return Err("test daemon rejected registration".to_string());
            }
            let task = GovernedTaskRun {
                id: registration.task_id,
                revision: 0,
                project_id: registration.project_id,
                workspace_root: registration.workspace_root,
                task: registration.task,
                acceptance_criteria: registration.acceptance_criteria,
                approval_policy: ApprovalPolicy::OperatorRequired,
                role_assignment: registration.role_assignment,
                role_compatibility: registration.role_compatibility,
                runtime_id: registration.runtime_id,
                agent_id: registration.agent_id,
                session_id: registration.session_id,
                initial_subject_revision: registration.initial_subject_revision,
                execution_state: GovernedExecutionState::Registered,
                review_state: GovernedReviewState::AwaitingClaim,
                claims: Vec::new(),
                verifications: Vec::new(),
                supervisor_verdicts: Vec::new(),
                operator_decisions: Vec::new(),
                events: Vec::new(),
                created_at: impulse_ops::now_rfc3339(),
                updated_at: impulse_ops::now_rfc3339(),
            };
            self.tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(task.id.clone(), task.clone());
            let mut response = task;
            if self.corrupt_registration {
                response.agent_id = "cross-wired-agent".to_string();
                response.id = GovernedTaskId::try_new("task-cross-wired-response").unwrap();
            }
            Ok(response)
        }

        fn mutate(&self, request: GovernedTaskMutationRequest) -> Result<GovernedTaskRun, String> {
            let mut tasks = self
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let task = tasks
                .get_mut(&request.task_id)
                .ok_or_else(|| "test task missing".to_string())?;
            if task.revision != request.expected_revision {
                return Err("revision conflict".to_string());
            }
            match request.mutation {
                GovernedTaskMutation::MarkRunning { .. } => {
                    if self.reject_running {
                        return Err("test daemon rejected running acknowledgment".to_string());
                    }
                    if task.execution_state != GovernedExecutionState::Registered {
                        return Err("invalid running transition".to_string());
                    }
                    task.execution_state = GovernedExecutionState::Running;
                }
                GovernedTaskMutation::MarkLaunchFailed { .. } => {
                    if task.execution_state != GovernedExecutionState::Registered {
                        return Err("invalid launch-failed transition".to_string());
                    }
                    task.execution_state = GovernedExecutionState::LaunchFailed;
                }
                GovernedTaskMutation::MarkRuntimeExited { .. } => {
                    if task.execution_state != GovernedExecutionState::Running {
                        return Err("invalid runtime-exit transition".to_string());
                    }
                    task.execution_state = GovernedExecutionState::RuntimeExited;
                }
                _ => return Err("unsupported test mutation".to_string()),
            }
            task.revision += 1;
            task.updated_at = impulse_ops::now_rfc3339();
            let mut response = task.clone();
            if self.corrupt_running && response.execution_state == GovernedExecutionState::Running {
                response.project_id = "cross-wired-project".to_string();
            }
            Ok(response)
        }

        fn mutate_current(
            &self,
            project_id: &str,
            task_id: &GovernedTaskId,
            request_id: impulse_ops::governed_task::GovernedRequestId,
            mutation: GovernedTaskMutation,
        ) -> Result<GovernedTaskRun, String> {
            let current = self
                .task(task_id)
                .ok_or_else(|| "test task missing".to_string())?;
            self.mutate(GovernedTaskMutationRequest {
                request_id,
                project_id: project_id.to_string(),
                task_id: task_id.clone(),
                expected_revision: current.revision,
                mutation,
            })
        }
    }

    fn runtime_with_task_gateway() -> (DesktopRuntime, Arc<TestGovernedTaskGateway>) {
        let gateway = Arc::new(TestGovernedTaskGateway::default());
        let gateway_trait: Arc<dyn GovernedTaskGateway> = gateway.clone();
        (
            DesktopRuntime::builder()
                .with_governed_task_gateway(gateway_trait)
                .build(),
            gateway,
        )
    }

    fn spawn_request(rows: u16, cols: u16, command: Option<&str>) -> AgentSpawnRequest {
        AgentSpawnRequest {
            agent_id: Some("agent-1".to_string()),
            session_id: Some("agent-1-session".to_string()),
            platform: platform_id("shell"),
            command: command.map(|value| value.to_string()),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows,
            cols,
            role: None,
            task: None,
            role_assignment: None,
            target: None,
        }
    }

    fn bind_to_temporary_workspace(request: &mut AgentSpawnRequest) -> tempfile::TempDir {
        let workspace = tempfile::tempdir().expect("temporary governed workspace");
        let root = workspace.path().display().to_string();
        request.cwd = Some(root.clone());
        request.workspace = Some(WorkspaceTarget::from_root(root));
        workspace
    }

    // --- spawn validation guards (return before any real PTY spawn) ---

    #[test]
    fn test_spawn_agent_zero_rows_returns_invalid_request() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .spawn_agent(spawn_request(0, 80, Some("sh")))
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::InvalidTerminalRequest { .. }
        ));
    }

    #[test]
    fn test_spawn_agent_zero_cols_returns_invalid_request() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .spawn_agent(spawn_request(24, 0, Some("sh")))
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::InvalidTerminalRequest { .. }
        ));
    }

    #[test]
    fn test_spawn_agent_blank_command_returns_invalid_request() {
        let runtime = DesktopRuntime::default();
        // Explicit whitespace command must be rejected before spawning.
        let err = runtime
            .spawn_agent(spawn_request(24, 80, Some("   ")))
            .unwrap_err();
        match err {
            DesktopBridgeError::InvalidTerminalRequest { message } => {
                assert!(message.contains("command"));
            }
            other => panic!("expected InvalidTerminalRequest, got {other:?}"),
        }
    }

    #[test]
    fn test_spawn_request_serde_keeps_task_and_assignment_optional() {
        let legacy: AgentSpawnRequest =
            serde_json::from_str(r#"{"platform":"shell","rows":24,"cols":80}"#).unwrap();
        assert!(legacy.task.is_none());
        assert!(legacy.role_assignment.is_none());

        let enriched: AgentSpawnRequest = serde_json::from_value(serde_json::json!({
            "platform": "shell",
            "rows": 24,
            "cols": 80,
            "task": "reconcile daemon truth",
            "role_assignment": {
                "role": "builder",
                "requirements": [{
                    "capability": "workspace.target",
                    "minimum_enforcement": "mediated",
                    "mandatory": true
                }]
            }
        }))
        .unwrap();
        assert_eq!(enriched.task.as_deref(), Some("reconcile daemon truth"));
        assert_eq!(
            enriched
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("builder")
        );
    }

    #[test]
    fn test_mandatory_incompatibility_blocks_before_id_reservation_or_pty_spawn() {
        let runtime = DesktopRuntime::default();
        let mut blocked = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
        let _workspace = bind_to_temporary_workspace(&mut blocked);
        blocked.agent_id = Some("reusable-after-block".to_string());
        let mut assignment =
            role_assignment("network.denied", EnforcementStrength::Structural, true);
        assignment.requirements.push(RoleCapabilityRequirement {
            capability: RuntimeCapabilityId::try_new("workspace.target").unwrap(),
            minimum_enforcement: EnforcementStrength::Structural,
            mandatory: true,
        });
        blocked.role_assignment = Some(assignment);
        blocked.task = Some("run governed build".to_string());

        let error = runtime.spawn_agent(blocked).unwrap_err();
        match error {
            DesktopBridgeError::InvalidTerminalRequest { message } => {
                assert!(message.contains("incompatible"));
                assert!(
                    message.contains("network.denied (required=structural, available=unsupported)")
                );
                assert!(
                    message.contains("workspace.target (required=structural, available=mediated)")
                );
                assert!(message.contains("choose a compatible platform"));
                assert!(message.contains("adjust the role requirement"));
            }
            other => panic!("expected compatibility rejection, got {other:?}"),
        }

        let mut allowed = spawn_request(24, 80, Some("sh"));
        allowed.agent_id = Some("reusable-after-block".to_string());
        allowed.args = vec!["-c".to_string(), "sleep 1".to_string()];
        let snapshot = runtime
            .spawn_agent(allowed)
            .expect("blocked launch must not reserve the requested agent id");
        runtime
            .close_agent(TerminalCloseRequest {
                session_id: snapshot.agent_id,
            })
            .unwrap();
    }

    #[test]
    fn test_governed_spawn_requires_nonblank_task_before_registry_id_or_pty_work() {
        let runtime = DesktopRuntime::default();
        for (case, task) in [
            ("missing", None),
            ("empty", Some(String::new())),
            ("whitespace", Some(" \t\n ".to_string())),
        ] {
            let agent_id = format!("task-required-{case}");
            let mut invalid = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
            invalid.agent_id = Some(agent_id.clone());
            invalid.platform = platform_id("missing-agent");
            invalid.task = task;
            invalid.role_assignment = Some(role_assignment(
                "workspace.target",
                EnforcementStrength::Mediated,
                true,
            ));

            let error = runtime.spawn_agent(invalid).unwrap_err();
            match error {
                DesktopBridgeError::InvalidTerminalRequest { message } => {
                    assert!(message.contains("role assignment requires a nonblank task"));
                }
                other => panic!("expected task validation rejection, got {other:?}"),
            }

            let mut allowed = spawn_request(24, 80, Some("sh"));
            allowed.agent_id = Some(agent_id);
            allowed.args = vec!["-c".to_string(), "sleep 1".to_string()];
            let snapshot = runtime
                .spawn_agent(allowed)
                .expect("task rejection must not reserve the requested agent id");
            runtime
                .close_agent(TerminalCloseRequest {
                    session_id: snapshot.agent_id,
                })
                .unwrap();
        }
    }

    #[test]
    fn test_governed_spawn_requires_explicit_workspace_and_cwd_before_id_reservation_or_pty_work() {
        let runtime = DesktopRuntime::default();
        let workspace_root = tempfile::tempdir().expect("temporary governed workspace");

        for (case, cwd, workspace) in [
            ("both-missing", None, None),
            (
                "cwd-missing",
                None,
                Some(WorkspaceTarget::from_root(
                    workspace_root.path().display().to_string(),
                )),
            ),
            (
                "workspace-missing",
                Some(workspace_root.path().display().to_string()),
                None,
            ),
        ] {
            let agent_id = format!("workspace-required-{case}");
            let mut invalid = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
            invalid.agent_id = Some(agent_id.clone());
            invalid.cwd = cwd;
            invalid.workspace = workspace;
            invalid.task = Some("run governed build".to_string());
            invalid.role_assignment = Some(role_assignment(
                "workspace.target",
                EnforcementStrength::Mediated,
                true,
            ));

            let error = runtime.spawn_agent(invalid).unwrap_err();
            match error {
                DesktopBridgeError::InvalidTerminalRequest { message } => {
                    assert!(message.contains("governed role launch"));
                    assert!(message.contains("workspace"));
                    assert!(message.contains("cwd"));
                }
                other => panic!("expected workspace binding rejection, got {other:?}"),
            }

            let mut legacy = spawn_request(24, 80, Some("sh"));
            legacy.agent_id = Some(agent_id);
            legacy.args = vec!["-c".to_string(), "sleep 1".to_string()];
            let snapshot = runtime
                .spawn_agent(legacy)
                .expect("governed rejection must not reserve the requested agent id");
            runtime
                .close_agent(TerminalCloseRequest {
                    session_id: snapshot.agent_id,
                })
                .unwrap();
        }
    }

    #[test]
    fn test_governed_spawn_rejects_nonexistent_or_mismatched_workspace_binding() {
        let runtime = DesktopRuntime::default();
        let first = tempfile::tempdir().expect("first governed workspace");
        let second = tempfile::tempdir().expect("second governed workspace");
        let missing = first.path().join("does-not-exist");

        for (case, cwd, workspace_root, expected) in [
            (
                "nonexistent",
                missing.display().to_string(),
                missing.display().to_string(),
                "existing directory",
            ),
            (
                "mismatched",
                first.path().display().to_string(),
                second.path().display().to_string(),
                "same canonical directory",
            ),
        ] {
            let mut invalid = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
            invalid.agent_id = Some(format!("invalid-binding-{case}"));
            invalid.cwd = Some(cwd);
            invalid.workspace = Some(WorkspaceTarget::from_root(workspace_root));
            invalid.task = Some("run governed build".to_string());
            invalid.role_assignment = Some(role_assignment(
                "workspace.target",
                EnforcementStrength::Mediated,
                true,
            ));

            let error = runtime.spawn_agent(invalid).unwrap_err();
            match error {
                DesktopBridgeError::InvalidTerminalRequest { message } => {
                    assert!(message.contains(expected), "unexpected error: {message}");
                }
                other => panic!("expected workspace binding rejection, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_governed_spawn_rejects_relative_or_nondirectory_workspace_binding() {
        let runtime = DesktopRuntime::default();
        let file = tempfile::NamedTempFile::new().expect("temporary non-directory target");

        for (case, path, expected) in [
            ("relative", ".".to_string(), "must be absolute"),
            (
                "not-directory",
                file.path().display().to_string(),
                "is not a directory",
            ),
        ] {
            let mut invalid = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
            invalid.agent_id = Some(format!("invalid-path-shape-{case}"));
            invalid.cwd = Some(path.clone());
            invalid.workspace = Some(WorkspaceTarget::from_root(path));
            invalid.task = Some("run governed build".to_string());
            invalid.role_assignment = Some(role_assignment(
                "workspace.target",
                EnforcementStrength::Mediated,
                true,
            ));

            let error = runtime.spawn_agent(invalid).unwrap_err();
            match error {
                DesktopBridgeError::InvalidTerminalRequest { message } => {
                    assert!(message.contains(expected), "unexpected error: {message}");
                }
                other => panic!("expected workspace binding rejection, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_governed_spawn_rejects_nul_or_oversized_task_before_id_reservation_or_pty_work() {
        let runtime = DesktopRuntime::default();
        let workspace = tempfile::tempdir().expect("temporary governed workspace");

        for (case, task, expected) in [
            (
                "nul",
                "inspect\0then mutate".to_string(),
                "must not contain NUL",
            ),
            ("oversized", "x".repeat(8_193), "at most 8192 UTF-8 bytes"),
        ] {
            let agent_id = format!("unsafe-task-{case}");
            let root = workspace.path().display().to_string();
            let mut invalid = spawn_request(24, 80, Some("definitely-not-an-impulse-executable"));
            invalid.agent_id = Some(agent_id.clone());
            invalid.cwd = Some(root.clone());
            invalid.workspace = Some(WorkspaceTarget::from_root(root));
            invalid.task = Some(task);
            invalid.role_assignment = Some(role_assignment(
                "workspace.target",
                EnforcementStrength::Mediated,
                true,
            ));

            let error = runtime.spawn_agent(invalid).unwrap_err();
            match error {
                DesktopBridgeError::InvalidTerminalRequest { message } => {
                    assert!(message.contains(expected), "unexpected error: {message}");
                }
                other => panic!("expected task safety rejection, got {other:?}"),
            }

            let mut legacy = spawn_request(24, 80, Some("sh"));
            legacy.agent_id = Some(agent_id);
            legacy.args = vec!["-c".to_string(), "sleep 1".to_string()];
            let snapshot = runtime
                .spawn_agent(legacy)
                .expect("task rejection must not reserve the requested agent id");
            runtime
                .close_agent(TerminalCloseRequest {
                    session_id: snapshot.agent_id,
                })
                .unwrap();
        }
    }

    #[test]
    fn test_governed_registration_failure_creates_no_pty_and_reserves_no_agent_id() {
        let gateway = Arc::new(TestGovernedTaskGateway::rejecting());
        let gateway_trait: Arc<dyn GovernedTaskGateway> = gateway;
        let runtime = DesktopRuntime::builder()
            .with_governed_task_gateway(gateway_trait)
            .build();
        let workspace = tempfile::tempdir().expect("temporary governed workspace");
        let marker = workspace.path().join("pty-created");
        let root = workspace.path().display().to_string();
        let mut request = spawn_request(24, 80, Some("sh"));
        request.agent_id = Some("registration-rejected".to_string());
        request.args = vec!["-c".to_string(), format!("touch {}", marker.display())];
        request.cwd = Some(root.clone());
        request.workspace = Some(WorkspaceTarget::from_root(root));
        request.task = Some("prove registration gate".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let error = runtime.spawn_agent(request).unwrap_err();
        assert!(matches!(
            error,
            DesktopBridgeError::GovernedTaskFailed { ref message }
                if message.contains("before PTY creation")
        ));
        assert!(
            !marker.exists(),
            "registration failure must prevent PTY work"
        );

        let mut legacy = spawn_request(24, 80, Some("sh"));
        legacy.agent_id = Some("registration-rejected".to_string());
        legacy.args = vec!["-c".to_string(), "sleep 1".to_string()];
        let snapshot = runtime
            .spawn_agent(legacy)
            .expect("failed registration must not reserve the runtime id");
        runtime
            .close_agent(TerminalCloseRequest {
                session_id: snapshot.agent_id,
            })
            .unwrap();
    }

    #[test]
    fn test_invalid_registration_ack_is_durably_rejected_before_pty_creation() {
        let gateway = Arc::new(TestGovernedTaskGateway::corrupting_registration());
        let gateway_trait: Arc<dyn GovernedTaskGateway> = gateway.clone();
        let runtime = DesktopRuntime::builder()
            .with_governed_task_gateway(gateway_trait)
            .build();
        let workspace = tempfile::tempdir().expect("temporary governed workspace");
        let marker = workspace.path().join("invalid-registration-created-pty");
        let root = workspace.path().display().to_string();
        let mut request = spawn_request(24, 80, Some("sh"));
        request.agent_id = Some("invalid-registration-ack".to_string());
        request.args = vec!["-c".to_string(), format!("touch {}", marker.display())];
        request.cwd = Some(root.clone());
        request.workspace = Some(WorkspaceTarget::from_root(root));
        request.task = Some("reject cross-wired registration".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let error = runtime.spawn_agent(request).unwrap_err();
        assert!(matches!(
            error,
            DesktopBridgeError::GovernedTaskFailed { ref message }
                if message.contains("did not match")
        ));
        assert!(!marker.exists());
        let tasks = gateway
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks.values().next().unwrap().execution_state,
            GovernedExecutionState::LaunchFailed
        );
    }

    #[test]
    fn test_running_ack_failure_consumes_agent_id_after_pty_callbacks_exist() {
        let gateway = Arc::new(TestGovernedTaskGateway::rejecting_running());
        let gateway_trait: Arc<dyn GovernedTaskGateway> = gateway.clone();
        let runtime = DesktopRuntime::builder()
            .with_governed_task_gateway(gateway_trait)
            .build();
        let mut request = spawn_request(24, 80, Some("sh"));
        let _workspace = bind_to_temporary_workspace(&mut request);
        request.agent_id = Some("running-ack-rejected".to_string());
        request.args = vec!["-c".to_string(), "sleep 0.1".to_string()];
        request.task = Some("reject running acknowledgment".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let error = runtime.spawn_agent(request).unwrap_err();
        assert!(matches!(
            error,
            DesktopBridgeError::GovernedTaskFailed { ref message }
                if message.contains("mark task running")
        ));
        assert_eq!(
            gateway
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .values()
                .next()
                .unwrap()
                .execution_state,
            GovernedExecutionState::LaunchFailed
        );

        let mut retry = spawn_request(24, 80, Some("sh"));
        retry.agent_id = Some("running-ack-rejected".to_string());
        retry.args = vec!["-c".to_string(), "sleep 0.1".to_string()];
        let error = runtime.spawn_agent(retry).unwrap_err();
        assert!(matches!(
            error,
            DesktopBridgeError::InvalidTerminalRequest { ref message }
                if message.contains("already been used")
        ));
    }

    #[test]
    fn test_cross_wired_running_ack_is_recorded_as_runtime_exit() {
        let gateway = Arc::new(TestGovernedTaskGateway::corrupting_running());
        let gateway_trait: Arc<dyn GovernedTaskGateway> = gateway.clone();
        let runtime = DesktopRuntime::builder()
            .with_governed_task_gateway(gateway_trait)
            .build();
        let mut request = spawn_request(24, 80, Some("sh"));
        let _workspace = bind_to_temporary_workspace(&mut request);
        request.agent_id = Some("cross-wired-running".to_string());
        request.args = vec!["-c".to_string(), "sleep 0.1".to_string()];
        request.task = Some("reject cross-wired running state".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let error = runtime.spawn_agent(request).unwrap_err();
        assert!(matches!(
            error,
            DesktopBridgeError::GovernedTaskFailed { ref message }
                if message.contains("running acknowledgment")
        ));
        assert_eq!(
            gateway
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .values()
                .next()
                .unwrap()
                .execution_state,
            GovernedExecutionState::RuntimeExited
        );
    }

    #[test]
    fn test_compatible_assignment_and_task_survive_canonicalized_spawn_snapshot() {
        let (runtime, gateway) = runtime_with_task_gateway();
        let mut request = spawn_request(24, 80, Some("sh"));
        let _workspace = bind_to_temporary_workspace(&mut request);
        request.platform = platform_id("generic_shell");
        request.args = vec!["-c".to_string(), "sleep 1".to_string()];
        request.task = Some("build governed launch".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let snapshot = runtime.spawn_agent(request).unwrap();
        assert_eq!(snapshot.platform.as_str(), "shell");
        assert_eq!(
            snapshot.current_task.as_deref(),
            Some("build governed launch")
        );
        assert_eq!(
            snapshot
                .role_assignment
                .as_ref()
                .map(|assignment| assignment.role.as_str()),
            Some("builder")
        );
        let compatibility = snapshot
            .role_compatibility
            .as_ref()
            .expect("typed compatibility is retained");
        assert_eq!(compatibility.platform.as_str(), "shell");
        assert!(compatibility.launch_allowed());
        assert!(!compatibility.is_degraded());
        let governed_task_id = snapshot
            .governed_task_id
            .clone()
            .expect("daemon task id is retained");
        assert_ne!(governed_task_id.as_str(), snapshot.agent_id);
        assert_eq!(snapshot.governed_task_revision, Some(1));
        assert_eq!(
            gateway.task(&governed_task_id).unwrap().execution_state,
            GovernedExecutionState::Running
        );
        runtime
            .close_agent(TerminalCloseRequest {
                session_id: snapshot.agent_id,
            })
            .unwrap();
    }

    #[test]
    fn test_optional_gap_allows_spawn_and_preserves_degraded_snapshot() {
        let (runtime, _) = runtime_with_task_gateway();
        let mut request = spawn_request(24, 80, Some("sh"));
        let _workspace = bind_to_temporary_workspace(&mut request);
        request.args = vec!["-c".to_string(), "sleep 1".to_string()];
        request.task = Some("run degraded governed launch".to_string());
        request.role_assignment = Some(role_assignment(
            "network.denied",
            EnforcementStrength::Structural,
            false,
        ));

        let snapshot = runtime.spawn_agent(request).unwrap();
        let compatibility = snapshot
            .role_compatibility
            .as_ref()
            .expect("optional compatibility result is retained");
        assert!(compatibility.launch_allowed());
        assert!(compatibility.is_degraded());
        runtime
            .close_agent(TerminalCloseRequest {
                session_id: snapshot.agent_id,
            })
            .unwrap();
    }

    // --- missing-session error paths ---

    #[test]
    fn test_write_agent_missing_session_returns_missing_session() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .write_agent(AgentWriteRequest {
                agent_id: "nope".to_string(),
                data: b"hello".to_vec(),
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { session_id } if session_id == "nope"
        ));
    }

    #[test]
    fn test_runtime_drop_releases_inner_while_long_running_reader_exists() {
        let runtime = DesktopRuntime::default();
        let runtime_weak = runtime.downgrade();
        let mut request = spawn_request(24, 80, Some("sh"));
        request.args = vec!["-lc".to_string(), "sleep 30".to_string()];
        runtime
            .spawn_agent(request)
            .expect("spawn long-running reader");

        drop(runtime);

        assert!(
            runtime_weak.upgrade().is_none(),
            "PTY callbacks must not retain DesktopRuntimeInner"
        );
    }

    #[test]
    fn test_resize_agent_missing_session_returns_missing_session() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .resize_agent(TerminalResizeRequest {
                session_id: "nope".to_string(),
                rows: 24,
                cols: 80,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { .. }
        ));
    }

    #[test]
    fn test_resize_agent_zero_dims_validates_before_lookup() {
        let runtime = DesktopRuntime::default();
        // Zero dims must fail with InvalidTerminalRequest even for an unknown
        // session — dimension validation runs before the session lookup.
        let err = runtime
            .resize_agent(TerminalResizeRequest {
                session_id: "nope".to_string(),
                rows: 0,
                cols: 0,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::InvalidTerminalRequest { .. }
        ));
    }

    #[test]
    fn test_focus_agent_missing_session_returns_missing_session() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .focus_agent(TerminalFocusRequest {
                session_id: "ghost".to_string(),
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { session_id } if session_id == "ghost"
        ));
    }

    #[test]
    fn test_close_agent_missing_session_returns_missing_session() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .close_agent(TerminalCloseRequest {
                session_id: "ghost".to_string(),
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { .. }
        ));
    }

    #[test]
    fn test_snapshot_agents_empty_runtime_is_empty() {
        let runtime = DesktopRuntime::default();
        assert!(runtime.snapshot_agents().is_empty());
    }

    // --- supervisor local action confirmation gate ---

    #[test]
    fn test_supervisor_send_input_unconfirmed_is_rejected() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .dispatch_supervisor_local_action(SupervisorLocalActionRequest {
                action: LocalSupervisorAction::SendInput {
                    agent_id: "agent-1".to_string(),
                    content: "rm -rf /".to_string(),
                    confirmed: false,
                },
            })
            .unwrap_err();
        match err {
            DesktopBridgeError::InvalidTerminalRequest { message } => {
                assert!(message.contains("confirmation"));
            }
            other => panic!("expected confirmation rejection, got {other:?}"),
        }
    }

    #[test]
    fn test_supervisor_send_input_confirmed_routes_to_missing_session() {
        let runtime = DesktopRuntime::default();
        // Confirmed input passes the gate, then fails because no agent exists.
        let err = runtime
            .dispatch_supervisor_local_action(SupervisorLocalActionRequest {
                action: LocalSupervisorAction::SendInput {
                    agent_id: "agent-1".to_string(),
                    content: "ls".to_string(),
                    confirmed: true,
                },
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { .. }
        ));
    }

    #[test]
    fn test_supervisor_focus_missing_agent_returns_missing_session() {
        let runtime = DesktopRuntime::default();
        let err = runtime
            .dispatch_supervisor_local_action(SupervisorLocalActionRequest {
                action: LocalSupervisorAction::FocusAgent {
                    agent_id: "agent-1".to_string(),
                },
            })
            .unwrap_err();
        assert!(matches!(
            err,
            DesktopBridgeError::MissingTerminalSession { .. }
        ));
    }

    // --- pure helpers ---

    #[test]
    fn test_validate_dimensions_accepts_nonzero() {
        assert!(validate_dimensions(24, 80).is_ok());
        assert!(validate_dimensions(0, 80).is_err());
        assert!(validate_dimensions(24, 0).is_err());
    }

    #[test]
    fn test_sanitize_runtime_id_replaces_unknown_with_uuid() {
        // A normal id is preserved; an empty/garbage id falls back to a uuid.
        assert_eq!(sanitize_runtime_id("agent-1"), "agent-1");
        let fallback = sanitize_runtime_id("");
        assert!(fallback.starts_with("agent-"));
        assert!(fallback.len() > "agent-".len());
    }

    #[test]
    fn test_project_notes_hash_is_deterministic_and_prefixed() {
        let a = project_notes_hash("hello world");
        let b = project_notes_hash("hello world");
        let c = project_notes_hash("different");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("fnv1a64:"));
    }

    #[test]
    fn test_runtime_env_includes_optional_task_and_product_role_id() {
        let mut request = spawn_request(24, 80, Some("sh"));
        request.task = Some("inspect compatibility".to_string());
        request.role_assignment = Some(role_assignment(
            "workspace.target",
            EnforcementStrength::Mediated,
            true,
        ));

        let env = runtime_env("agent-1", &request, "sh", None, None);
        assert!(env
            .iter()
            .any(|(key, value)| *key == "IMPULSE_TASK" && value == "inspect compatibility"));
        assert!(env
            .iter()
            .any(|(key, value)| *key == "IMPULSE_ROLE_ID" && value == "builder"));
    }

    #[test]
    fn test_registry_detection_used_by_terminal_bridge_is_token_safe() {
        let registry = load_registry_for_spawn().unwrap();
        assert_eq!(
            registry
                .detect_from_command("/usr/bin/ion")
                .map(|descriptor| descriptor.id.as_str()),
            Some("ion")
        );
        assert_ne!(
            registry
                .detect_from_command("notification")
                .map(|descriptor| descriptor.id.as_str()),
            Some("ion")
        );
    }

    #[test]
    fn test_resolve_executable_command_finds_workspace_sibling_from_deps_layout() {
        let root = tempfile::tempdir().expect("temporary target directory");
        let deps = root.path().join("deps");
        std::fs::create_dir(&deps).expect("create deps directory");
        let current_exe = deps.join("desktop-runtime-test");
        std::fs::write(&current_exe, b"test harness").expect("write current executable marker");
        let ion = root.path().join("ion");
        std::fs::write(&ion, b"#!/bin/sh\nexit 0\n").expect("write sibling Ion executable");
        #[cfg(unix)]
        std::fs::set_permissions(&ion, std::fs::Permissions::from_mode(0o755))
            .expect("mark sibling executable");

        assert_eq!(
            resolve_executable_command_with(&platform_id("ion"), "ion", Some(&current_exe), None,),
            ion.to_string_lossy()
        );
    }

    #[test]
    fn test_resolve_executable_command_prefers_path_over_untrusted_sibling() {
        let root = tempfile::tempdir().expect("temporary target directory");
        let deps = root.path().join("deps");
        let path_dir = root.path().join("path-bin");
        std::fs::create_dir(&deps).expect("create deps directory");
        std::fs::create_dir(&path_dir).expect("create PATH directory");
        let current_exe = deps.join("desktop-runtime-test");
        std::fs::write(&current_exe, b"test harness").expect("write current executable marker");
        let sibling = root.path().join("codex");
        let path_codex = path_dir.join("codex");
        for executable in [&sibling, &path_codex] {
            std::fs::write(executable, b"#!/bin/sh\nexit 0\n").expect("write executable");
            #[cfg(unix)]
            std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755))
                .expect("mark executable");
        }
        let search_path = std::env::join_paths([&path_dir]).expect("join test PATH");

        assert_eq!(
            resolve_executable_command_with(
                &platform_id("codex"),
                "codex",
                Some(&current_exe),
                Some(&search_path),
            ),
            path_codex.to_string_lossy(),
            "external agents must keep PATH semantics instead of trusting app siblings"
        );
    }

    #[test]
    fn test_workspace_target_from_root_extracts_label() {
        let ws = WorkspaceTarget::from_root("/home/user/projects/impulse");
        assert_eq!(ws.label.as_deref(), Some("impulse"));
        assert_eq!(ws.root, "/home/user/projects/impulse");
        assert!(ws.purpose.is_none());
    }

    #[test]
    fn test_default_builtin_mcp_tools_present() {
        let tools = default_builtin_mcp_tools();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|tool| tool.name == "impulse.agent_spawn"));
        // The spawn tool mutates terminals and must require confirmation.
        let spawn = tools
            .iter()
            .find(|tool| tool.name == "impulse.agent_spawn")
            .expect("agent_spawn tool present");
        assert!(spawn.requires_confirmation);
        // Read-only memory search must not require confirmation.
        let memory = tools
            .iter()
            .find(|tool| tool.name == "impulse.search_memory")
            .expect("search_memory tool present");
        assert!(!memory.requires_confirmation);
    }

    // --- event-name consistency ---

    #[test]
    fn test_desktop_event_name_matches_host_event_names() {
        let output = DesktopEvent::TerminalOutput {
            agent_id: "a".to_string(),
            data: vec![1, 2, 3],
        };
        assert_eq!(output.name(), "terminal_output");
        let exit = DesktopEvent::TerminalExit {
            agent_id: "a".to_string(),
        };
        assert_eq!(exit.name(), "terminal_exit");
        let supervisor = DesktopEvent::SupervisorLocalAction {
            action: LocalSupervisorAction::FocusAgent {
                agent_id: "a".to_string(),
            },
        };
        assert_eq!(supervisor.name(), "supervisor_local_action");
        let ops_connection = DesktopEvent::OpsConnectionUpdate {
            connected: false,
            error: Some("daemon unavailable".to_string()),
        };
        assert_eq!(ops_connection.name(), "ops_connection_update");
        // Every advertised host event name is non-empty and unique.
        let names = DesktopEvent::HOST_EVENT_NAMES;
        let mut deduped = names.to_vec();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), names.len());
    }

    // --- serde round-trips (CLAUDE.md requirement for Serialize+Deserialize) ---

    #[test]
    fn test_local_supervisor_action_round_trip() {
        let action = LocalSupervisorAction::SendInput {
            agent_id: "agent-1".to_string(),
            content: "echo hi".to_string(),
            confirmed: true,
        };
        let json = serde_json::to_string(&action).unwrap();
        let recovered: LocalSupervisorAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, recovered);
    }

    #[test]
    fn test_agent_platform_id_round_trip() {
        for id in [
            "claude-code",
            "codex",
            "opencode",
            "gemini",
            "cursor",
            "ion",
            "shell",
            "custom-agent",
        ] {
            let platform = platform_id(id);
            let json = serde_json::to_string(&platform).unwrap();
            let recovered: AgentPlatformId = serde_json::from_str(&json).unwrap();
            assert_eq!(platform, recovered);
        }
    }

    #[test]
    fn test_workspace_target_round_trip() {
        let ws = WorkspaceTarget {
            root: "/tmp/proj".to_string(),
            label: Some("proj".to_string()),
            purpose: Some("testing".to_string()),
            project_notes: Some("notes".to_string()),
        };
        let json = serde_json::to_string(&ws).unwrap();
        let recovered: WorkspaceTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(ws, recovered);
    }
}
