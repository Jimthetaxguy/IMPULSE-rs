use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use impulse_ops::{AgentRole, AgentStatus, ContextHealthSummary, MachineTarget};
use impulse_term::TerminalBackend;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bridge::{
    DesktopBridgeError, TerminalBridge, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentPlatformKind {
    ClaudeCode,
    Codex,
    OpenCode,
    Gemini,
    Cursor,
    Shell,
}

impl AgentPlatformKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
            Self::Cursor => "cursor",
            Self::Shell => "shell",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Gemini => "Gemini",
            Self::Cursor => "Cursor",
            Self::Shell => "Shell",
        }
    }

    pub fn default_command(self) -> String {
        // Enum is wire slug only. Real command resolution is in
        // impulse_ops::agent_registry::resolve_launch_command (single source of truth).
        if self == Self::Shell {
            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
        } else {
            "sh".to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSpawnRequest {
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub platform: AgentPlatformKind,
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
    #[serde(default)]
    pub target: Option<MachineTarget>,
}

impl AgentSpawnRequest {
    pub fn terminal_harness(
        agent_id: impl Into<String>,
        platform: AgentPlatformKind,
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
    pub platform: AgentPlatformKind,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub workspace: Option<WorkspaceTarget>,
    pub session_id: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub alive: bool,
    pub focused: bool,
    pub status: AgentStatus,
    pub current_task: Option<String>,
    pub role: Option<AgentRole>,
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
    platform: AgentPlatformKind,
    label: String,
    command: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    workspace: Option<WorkspaceTarget>,
    session_id: Option<String>,
    role: Option<AgentRole>,
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
            platform: self.platform,
            command: self.command.clone(),
            args: self.args.clone(),
            cwd: self.cwd.as_ref().map(|path| path.display().to_string()),
            workspace: self.workspace.clone(),
            session_id: self.session_id.clone(),
            rows,
            cols,
            alive,
            focused: self.focused,
            status,
            current_task: None,
            role: self.role.clone(),
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

struct DesktopRuntimeInner {
    state: Mutex<RuntimeState>,
    sink: Arc<dyn DesktopEventSink>,
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
            record.backend.kill();
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
        validate_dimensions(request.rows, request.cols)?;
        // Centralized registry load (symmetric to load_registry_for_tool in mcp.rs).
        // Keeps the error mapping for spawn in one place and makes future changes cheaper.
        let registry = load_registry_for_spawn()?;
        let provided_blank = request
            .command
            .as_ref()
            .is_some_and(|c| c.trim().is_empty());
        let command = impulse_ops::agent_registry::resolve_launch_command(
            &registry,
            request.platform.as_str(),
            request.command.as_deref(),
        );
        if provided_blank || command.trim().is_empty() {
            return Err(DesktopBridgeError::InvalidTerminalRequest {
                message: "command cannot be empty".to_string(),
            });
        }

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
        let lifecycle_guard = self
            .inner
            .lifecycle_events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        {
            let mut state = self.lock_state();
            if !state.used_agent_ids.insert(agent_id.clone()) {
                return Err(DesktopBridgeError::InvalidTerminalRequest {
                    message: format!(
                        "agent id `{agent_id}` has already been used by this runtime; choose a new id"
                    ),
                });
            }
        }
        let env_pairs = runtime_env(&agent_id, &request, &command, workspace.as_ref());
        let output_runtime = Arc::downgrade(&self.inner);
        let output_agent_id = agent_id.clone();
        let output_callback = Arc::new(move |data: &[u8]| {
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
        let exit_callback = Arc::new(move || {
            let Some(runtime) = exit_runtime.upgrade() else {
                return;
            };
            let should_drain = {
                let _lifecycle_guard = runtime
                    .lifecycle_events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let removed = runtime
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .agents
                    .remove(&exit_agent_id);
                let should_emit = removed.is_some();
                drop(removed);
                should_emit
                    && runtime
                        .lifecycle_dispatcher
                        .enqueue(DesktopEvent::TerminalExit {
                            agent_id: exit_agent_id.clone(),
                        })
            };
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
                return Err(DesktopBridgeError::TerminalSpawnFailed {
                    message: error.to_string(),
                });
            }
        };

        let record = RuntimeRecord {
            platform: request.platform,
            label: registry
                .get(request.platform.as_str())
                .map(|d| d.label.clone())
                .unwrap_or_else(|| request.platform.label().to_string()),
            command,
            args: request.args,
            cwd,
            workspace,
            session_id: request.session_id,
            role: request.role,
            target: request.target,
            mcp_tools,
            focused: false,
            status: AgentStatus::Starting,
            backend,
        };
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
        record.backend.kill();
        let should_drain = self
            .inner
            .lifecycle_dispatcher
            .enqueue(DesktopEvent::TerminalExit {
                agent_id: request.session_id,
            });
        drop(record);
        drop(lifecycle_guard);
        if should_drain {
            self.inner.lifecycle_dispatcher.drain();
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
        let platform = AgentPlatformKind::from_command(&request.command);
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

impl AgentPlatformKind {
    fn from_command(command: &str) -> Self {
        let command = command.to_ascii_lowercase();
        if command.contains("claude") {
            Self::ClaudeCode
        } else if command.contains("codex") {
            Self::Codex
        } else if command.contains("opencode") {
            Self::OpenCode
        } else if command.contains("gemini") || command.contains("antigravity") {
            Self::Gemini
        } else if command.contains("cursor") {
            Self::Cursor
        } else {
            Self::Shell
        }
    }
}

pub struct DesktopRuntimeBuilder {
    sink: Arc<dyn DesktopEventSink>,
}

impl DesktopRuntimeBuilder {
    pub fn with_event_sink(mut self, sink: Arc<dyn DesktopEventSink>) -> Self {
        self.sink = sink;
        self
    }

    pub fn build(self) -> DesktopRuntime {
        DesktopRuntime {
            inner: Arc::new(DesktopRuntimeInner {
                state: Mutex::new(RuntimeState::default()),
                lifecycle_dispatcher: LifecycleEventDispatcher::new(Arc::clone(&self.sink)),
                sink: self.sink,
                lifecycle_events: Mutex::new(()),
            }),
        }
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

fn validate_dimensions(rows: u16, cols: u16) -> Result<(), DesktopBridgeError> {
    if rows == 0 || cols == 0 {
        return Err(DesktopBridgeError::InvalidTerminalRequest {
            message: "terminal rows and cols must be greater than zero".to_string(),
        });
    }
    Ok(())
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

    fn spawn_request(rows: u16, cols: u16, command: Option<&str>) -> AgentSpawnRequest {
        AgentSpawnRequest {
            agent_id: Some("agent-1".to_string()),
            session_id: Some("agent-1-session".to_string()),
            platform: AgentPlatformKind::Shell,
            command: command.map(|value| value.to_string()),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows,
            cols,
            role: None,
            target: None,
        }
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
    fn test_agent_platform_kind_from_command() {
        assert_eq!(
            AgentPlatformKind::from_command("/usr/bin/claude"),
            AgentPlatformKind::ClaudeCode
        );
        assert_eq!(
            AgentPlatformKind::from_command("codex"),
            AgentPlatformKind::Codex
        );
        assert_eq!(
            AgentPlatformKind::from_command("opencode"),
            AgentPlatformKind::OpenCode
        );
        assert_eq!(
            AgentPlatformKind::from_command("gemini"),
            AgentPlatformKind::Gemini
        );
        assert_eq!(
            AgentPlatformKind::from_command("/opt/antigravity"),
            AgentPlatformKind::Gemini
        );
        assert_eq!(
            AgentPlatformKind::from_command("cursor-agent"),
            AgentPlatformKind::Cursor
        );
        assert_eq!(
            AgentPlatformKind::from_command("bash"),
            AgentPlatformKind::Shell
        );
    }

    #[test]
    fn test_agent_platform_kind_default_command() {
        // Enum default is now fallback only. Real values come from resolve_launch_command (ops registry).
        // These assert the minimal fallback behavior for the enum slug.
        assert_eq!(
            AgentPlatformKind::Shell.default_command(),
            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
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
    fn test_agent_platform_kind_round_trip() {
        for kind in [
            AgentPlatformKind::ClaudeCode,
            AgentPlatformKind::Codex,
            AgentPlatformKind::OpenCode,
            AgentPlatformKind::Gemini,
            AgentPlatformKind::Cursor,
            AgentPlatformKind::Shell,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let recovered: AgentPlatformKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, recovered);
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
