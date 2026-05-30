use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use impulse_ops::{AgentRole, AgentStatus, ContextHealthSummary, MachineTarget};
use impulse_term::TerminalBackend;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bridge::{
    DesktopBridgeError, TerminalBridge, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentPlatformKind {
    ClaudeCode,
    Codex,
    OpenCode,
    Shell,
}

impl AgentPlatformKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Shell => "shell",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Shell => "Shell",
        }
    }

    pub fn default_command(self) -> String {
        match self {
            Self::ClaudeCode => "claude".to_string(),
            Self::Codex => "codex".to_string(),
            Self::OpenCode => "opencode".to_string(),
            Self::Shell => std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()),
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
    pub rows: u16,
    pub cols: u16,
    #[serde(default)]
    pub role: Option<AgentRole>,
    #[serde(default)]
    pub target: Option<MachineTarget>,
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
    pub session_id: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub alive: bool,
    pub focused: bool,
    pub status: AgentStatus,
    pub current_task: Option<String>,
    pub role: Option<AgentRole>,
    pub target: Option<MachineTarget>,
    pub output_bytes: u64,
    pub output_lines: u64,
    pub context: ContextHealthSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum DesktopEvent {
    TerminalOutput { agent_id: String, data: Vec<u8> },
    TerminalExit { agent_id: String },
    AgentRuntimeUpdate { snapshot: Box<AgentRuntimeSnapshot> },
    OpsUpdate { payload: serde_json::Value },
    SupervisorLocalAction { action: LocalSupervisorAction },
}

impl DesktopEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::TerminalOutput { .. } => "terminal_output",
            Self::TerminalExit { .. } => "terminal_exit",
            Self::AgentRuntimeUpdate { .. } => "agent_runtime_update",
            Self::OpsUpdate { .. } => "ops_update",
            Self::SupervisorLocalAction { .. } => "supervisor_local_action",
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

struct RuntimeRecord {
    platform: AgentPlatformKind,
    label: String,
    command: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    session_id: Option<String>,
    role: Option<AgentRole>,
    target: Option<MachineTarget>,
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
            session_id: self.session_id.clone(),
            rows,
            cols,
            alive,
            focused: self.focused,
            status,
            current_task: None,
            role: self.role.clone(),
            target: self.target.clone(),
            output_bytes: self.backend.output_bytes(),
            output_lines: self.backend.output_lines(),
            context: ContextHealthSummary::default(),
        }
    }
}

#[derive(Default)]
struct RuntimeState {
    agents: HashMap<String, RuntimeRecord>,
}

#[derive(Clone)]
pub struct DesktopRuntime {
    state: Arc<Mutex<RuntimeState>>,
    sink: Arc<dyn DesktopEventSink>,
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

    pub fn spawn_agent(
        &self,
        request: AgentSpawnRequest,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        validate_dimensions(request.rows, request.cols)?;
        let command = request
            .command
            .clone()
            .unwrap_or_else(|| request.platform.default_command());
        if command.trim().is_empty() {
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
        let env_pairs = runtime_env(&agent_id, &request, &command);
        let sink = Arc::clone(&self.sink);
        let output_agent_id = agent_id.clone();
        let output_callback = Arc::new(move |data: &[u8]| {
            sink.emit(DesktopEvent::TerminalOutput {
                agent_id: output_agent_id.clone(),
                data: data.to_vec(),
            });
        });
        let sink = Arc::clone(&self.sink);
        let exit_agent_id = agent_id.clone();
        let exit_callback = Arc::new(move || {
            sink.emit(DesktopEvent::TerminalExit {
                agent_id: exit_agent_id.clone(),
            });
        });

        let backend = TerminalBackend::spawn_with_callbacks(
            &command,
            &request.args,
            cwd.as_deref(),
            &env_pairs,
            request.rows,
            request.cols,
            None,
            Some(output_callback),
            Some(exit_callback),
        )
        .map_err(|error| DesktopBridgeError::TerminalSpawnFailed {
            message: error.to_string(),
        })?;

        let record = RuntimeRecord {
            platform: request.platform,
            label: request.platform.label().to_string(),
            command,
            args: request.args,
            cwd,
            session_id: request.session_id,
            role: request.role,
            target: request.target,
            focused: false,
            status: AgentStatus::Starting,
            backend,
        };
        let snapshot = record.snapshot(&agent_id);
        self.state
            .lock()
            .expect("desktop runtime mutex poisoned")
            .agents
            .insert(agent_id, record);
        self.sink.emit(DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(snapshot.clone()),
        });
        Ok(snapshot)
    }

    pub fn write_agent(&self, request: AgentWriteRequest) -> Result<(), DesktopBridgeError> {
        let state = self.state.lock().expect("desktop runtime mutex poisoned");
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
        let state = self.state.lock().expect("desktop runtime mutex poisoned");
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
        let snapshot = record.snapshot(&request.session_id);
        self.sink.emit(DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(snapshot.clone()),
        });
        Ok(snapshot)
    }

    pub fn focus_agent(
        &self,
        request: TerminalFocusRequest,
    ) -> Result<AgentRuntimeSnapshot, DesktopBridgeError> {
        let mut state = self.state.lock().expect("desktop runtime mutex poisoned");
        if !state.agents.contains_key(&request.session_id) {
            return Err(DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id,
            });
        }
        for record in state.agents.values_mut() {
            record.focused = false;
        }
        let record = state
            .agents
            .get_mut(&request.session_id)
            .expect("agent existence checked above");
        record.focused = true;
        let snapshot = record.snapshot(&request.session_id);
        self.sink.emit(DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(snapshot.clone()),
        });
        Ok(snapshot)
    }

    pub fn close_agent(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError> {
        let mut state = self.state.lock().expect("desktop runtime mutex poisoned");
        let record = state.agents.remove(&request.session_id).ok_or(
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            },
        )?;
        record.backend.kill();
        self.sink.emit(DesktopEvent::TerminalExit {
            agent_id: request.session_id,
        });
        Ok(())
    }

    pub fn snapshot_agents(&self) -> Vec<AgentRuntimeSnapshot> {
        let state = self.state.lock().expect("desktop runtime mutex poisoned");
        let mut snapshots = state
            .agents
            .iter()
            .map(|(agent_id, record)| record.snapshot(agent_id))
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| {
            right
                .focused
                .cmp(&left.focused)
                .then_with(|| left.label.cmp(&right.label))
        });
        snapshots
    }

    pub fn dispatch_supervisor_local_action(
        &self,
        request: SupervisorLocalActionRequest,
    ) -> Result<(), DesktopBridgeError> {
        self.sink.emit(DesktopEvent::SupervisorLocalAction {
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
            state: Arc::new(Mutex::new(RuntimeState::default())),
            sink: self.sink,
        }
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
        "IMPULSE_PROJECT" => Some("IMPULSE_PROJECT"),
        "TERM" => Some("TERM"),
        _ => None,
    }
}

#[cfg(feature = "tauri-runtime")]
pub struct TauriEventSink<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

#[cfg(feature = "tauri-runtime")]
impl<R: tauri::Runtime> TauriEventSink<R> {
    pub fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

#[cfg(feature = "tauri-runtime")]
impl<R: tauri::Runtime> DesktopEventSink for TauriEventSink<R> {
    fn emit(&self, event: DesktopEvent) {
        use tauri::Emitter;

        let _ = self.app.emit(event.name(), &event);
    }
}

#[allow(dead_code)]
fn _assert_path_send_sync(_: &Path) {}
