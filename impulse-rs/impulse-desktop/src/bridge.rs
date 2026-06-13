use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::runtime::{BuiltInMcpTool, WorkspaceTarget};
use crate::native::{NativeIslandHost, NativeIslandRequest, NativeIslandResult};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DesktopBridgeError {
    #[error("terminal session {session_id} was not found")]
    MissingTerminalSession { session_id: String },
    #[error("invalid terminal request: {message}")]
    InvalidTerminalRequest { message: String },
    #[error("failed to spawn terminal agent: {message}")]
    TerminalSpawnFailed { message: String },
    #[error("terminal write failed: {message}")]
    TerminalWriteFailed { message: String },
    #[error("native island {kind} is not supported on this platform")]
    UnsupportedNativeIsland { kind: String },
    #[error("native island failed: {message}")]
    NativeIslandFailed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalOpenRequest {
    pub session_id: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub workspace: Option<WorkspaceTarget>,
    #[serde(default)]
    pub mcp_tools: Vec<BuiltInMcpTool>,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalSessionResponse {
    pub session_id: String,
    pub alive: bool,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalWriteRequest {
    pub session_id: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalResizeRequest {
    pub session_id: String,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalCloseRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalFocusRequest {
    pub session_id: String,
}

pub trait TerminalBridge {
    fn open(
        &self,
        request: TerminalOpenRequest,
    ) -> Result<TerminalSessionResponse, DesktopBridgeError>;
    fn write(&self, request: TerminalWriteRequest) -> Result<(), DesktopBridgeError>;
    fn resize(&self, request: TerminalResizeRequest) -> Result<(), DesktopBridgeError>;
    fn close(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError>;
    fn focus(&self, request: TerminalFocusRequest) -> Result<(), DesktopBridgeError>;
}

pub struct DesktopCommandRouter<T, N> {
    terminal_bridge: T,
    native_islands: N,
}

impl<T, N> DesktopCommandRouter<T, N>
where
    T: TerminalBridge,
    N: NativeIslandHost,
{
    pub fn new(terminal_bridge: T, native_islands: N) -> Self {
        Self {
            terminal_bridge,
            native_islands,
        }
    }

    pub fn terminal_open(
        &self,
        request: TerminalOpenRequest,
    ) -> Result<TerminalSessionResponse, DesktopBridgeError> {
        self.terminal_bridge.open(request)
    }

    pub fn terminal_write(&self, request: TerminalWriteRequest) -> Result<(), DesktopBridgeError> {
        self.terminal_bridge.write(request)
    }

    pub fn terminal_resize(
        &self,
        request: TerminalResizeRequest,
    ) -> Result<(), DesktopBridgeError> {
        self.terminal_bridge.resize(request)
    }

    pub fn terminal_close(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError> {
        self.terminal_bridge.close(request)
    }

    pub fn terminal_focus(&self, request: TerminalFocusRequest) -> Result<(), DesktopBridgeError> {
        self.terminal_bridge.focus(request)
    }

    pub fn native_island_request(
        &self,
        request: NativeIslandRequest,
    ) -> Result<NativeIslandResult, DesktopBridgeError> {
        self.native_islands.dispatch(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalRecord {
    response: TerminalSessionResponse,
    writes: Vec<Vec<u8>>,
    focused: bool,
}

#[derive(Debug, Default)]
pub struct InMemoryTerminalBridge {
    sessions: Mutex<HashMap<String, TerminalRecord>>,
}

impl InMemoryTerminalBridge {
    pub fn write_count(&self, session_id: &str) -> usize {
        self.sessions
            .lock()
            .expect("terminal test mutex poisoned")
            .get(session_id)
            .map(|record| record.writes.len())
            .unwrap_or_default()
    }

    pub fn is_focused(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .expect("terminal test mutex poisoned")
            .get(session_id)
            .map(|record| record.focused)
            .unwrap_or_default()
    }
}

impl TerminalBridge for InMemoryTerminalBridge {
    fn open(
        &self,
        request: TerminalOpenRequest,
    ) -> Result<TerminalSessionResponse, DesktopBridgeError> {
        let session_id = request
            .session_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let response = TerminalSessionResponse {
            session_id: session_id.clone(),
            alive: true,
            rows: request.rows,
            cols: request.cols,
        };
        self.sessions
            .lock()
            .expect("terminal mutex poisoned")
            .insert(
                session_id,
                TerminalRecord {
                    response: response.clone(),
                    writes: Vec::new(),
                    focused: false,
                },
            );
        Ok(response)
    }

    fn write(&self, request: TerminalWriteRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.sessions.lock().expect("terminal mutex poisoned");
        let record = sessions.get_mut(&request.session_id).ok_or_else(|| {
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            }
        })?;
        record.writes.push(request.data);
        Ok(())
    }

    fn resize(&self, request: TerminalResizeRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.sessions.lock().expect("terminal mutex poisoned");
        let record = sessions.get_mut(&request.session_id).ok_or_else(|| {
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            }
        })?;
        record.response.rows = request.rows;
        record.response.cols = request.cols;
        Ok(())
    }

    fn close(&self, request: TerminalCloseRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.sessions.lock().expect("terminal mutex poisoned");
        sessions.remove(&request.session_id).map(|_| ()).ok_or(
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id,
            },
        )
    }

    fn focus(&self, request: TerminalFocusRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.sessions.lock().expect("terminal mutex poisoned");
        for record in sessions.values_mut() {
            record.focused = false;
        }
        let record = sessions.get_mut(&request.session_id).ok_or_else(|| {
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            }
        })?;
        record.focused = true;
        Ok(())
    }
}

pub fn empty_payload() -> Value {
    Value::Object(Default::default())
}
