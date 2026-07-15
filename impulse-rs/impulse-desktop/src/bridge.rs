use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::native::{NativeIslandHost, NativeIslandRequest, NativeIslandResult};
use crate::runtime::{BuiltInMcpTool, WorkspaceTarget};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DesktopBridgeError {
    #[error("terminal session {session_id} was not found")]
    MissingTerminalSession { session_id: String },
    #[error("invalid terminal request: {message}")]
    InvalidTerminalRequest { message: String },
    #[error("failed to spawn terminal agent: {message}")]
    TerminalSpawnFailed { message: String },
    #[error("governed task lifecycle failed: {message}")]
    GovernedTaskFailed { message: String },
    #[error("terminal write failed: {message}")]
    TerminalWriteFailed { message: String },
    #[error("terminal termination failed: {message}")]
    TerminalTerminationFailed { message: String },
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
        self.lock_sessions()
            .get(session_id)
            .map(|record| record.writes.len())
            .unwrap_or_default()
    }

    pub fn is_focused(&self, session_id: &str) -> bool {
        self.lock_sessions()
            .get(session_id)
            .map(|record| record.focused)
            .unwrap_or_default()
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, TerminalRecord>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        self.lock_sessions().insert(
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
        let mut sessions = self.lock_sessions();
        let record = sessions.get_mut(&request.session_id).ok_or_else(|| {
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id.clone(),
            }
        })?;
        record.writes.push(request.data);
        Ok(())
    }

    fn resize(&self, request: TerminalResizeRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.lock_sessions();
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
        let mut sessions = self.lock_sessions();
        sessions.remove(&request.session_id).map(|_| ()).ok_or(
            DesktopBridgeError::MissingTerminalSession {
                session_id: request.session_id,
            },
        )
    }

    fn focus(&self, request: TerminalFocusRequest) -> Result<(), DesktopBridgeError> {
        let mut sessions = self.lock_sessions();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn open_req(session: &str) -> TerminalOpenRequest {
        TerminalOpenRequest {
            session_id: Some(session.to_string()),
            command: "bash".to_string(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 24,
            cols: 80,
        }
    }

    #[test]
    fn test_open_uses_supplied_session_id_and_marks_alive() {
        let bridge = InMemoryTerminalBridge::default();
        let resp = bridge.open(open_req("s1")).unwrap();
        assert_eq!(resp.session_id, "s1");
        assert!(resp.alive);
        assert_eq!((resp.rows, resp.cols), (24, 80));
    }

    #[test]
    fn test_open_generates_session_id_when_absent() {
        let bridge = InMemoryTerminalBridge::default();
        let mut req = open_req("ignored");
        req.session_id = None;
        let resp = bridge.open(req).unwrap();
        assert!(!resp.session_id.is_empty());
        // The fallback is a UUID v4 (hyphenated).
        assert!(resp.session_id.contains('-'));
    }

    #[test]
    fn test_write_appends_and_counts_per_session() {
        let bridge = InMemoryTerminalBridge::default();
        bridge.open(open_req("s1")).unwrap();
        assert_eq!(bridge.write_count("s1"), 0);
        bridge
            .write(TerminalWriteRequest {
                session_id: "s1".into(),
                data: b"hi".to_vec(),
            })
            .unwrap();
        bridge
            .write(TerminalWriteRequest {
                session_id: "s1".into(),
                data: b"yo".to_vec(),
            })
            .unwrap();
        assert_eq!(bridge.write_count("s1"), 2);
        assert_eq!(bridge.write_count("missing"), 0);
    }

    #[test]
    fn test_mutations_on_missing_session_return_missing_error() {
        let bridge = InMemoryTerminalBridge::default();
        let err = bridge
            .write(TerminalWriteRequest {
                session_id: "x".into(),
                data: vec![],
            })
            .unwrap_err();
        assert_eq!(
            err,
            DesktopBridgeError::MissingTerminalSession {
                session_id: "x".into()
            }
        );
        assert!(bridge
            .resize(TerminalResizeRequest {
                session_id: "x".into(),
                rows: 1,
                cols: 1
            })
            .is_err());
        assert!(bridge
            .close(TerminalCloseRequest {
                session_id: "x".into()
            })
            .is_err());
        assert!(bridge
            .focus(TerminalFocusRequest {
                session_id: "x".into()
            })
            .is_err());
    }

    #[test]
    fn test_resize_persists_and_session_stays_present() {
        let bridge = InMemoryTerminalBridge::default();
        bridge.open(open_req("s1")).unwrap();
        bridge
            .resize(TerminalResizeRequest {
                session_id: "s1".into(),
                rows: 40,
                cols: 120,
            })
            .unwrap();
        // Session is still present after resize (a second resize succeeds).
        assert!(bridge
            .resize(TerminalResizeRequest {
                session_id: "s1".into(),
                rows: 10,
                cols: 10,
            })
            .is_ok());
    }

    #[test]
    fn test_focus_is_exclusive_across_sessions() {
        let bridge = InMemoryTerminalBridge::default();
        bridge.open(open_req("a")).unwrap();
        bridge.open(open_req("b")).unwrap();
        bridge
            .focus(TerminalFocusRequest {
                session_id: "a".into(),
            })
            .unwrap();
        assert!(bridge.is_focused("a"));
        assert!(!bridge.is_focused("b"));
        bridge
            .focus(TerminalFocusRequest {
                session_id: "b".into(),
            })
            .unwrap();
        assert!(!bridge.is_focused("a"));
        assert!(bridge.is_focused("b"));
    }

    #[test]
    fn test_close_removes_session() {
        let bridge = InMemoryTerminalBridge::default();
        bridge.open(open_req("s1")).unwrap();
        bridge
            .close(TerminalCloseRequest {
                session_id: "s1".into(),
            })
            .unwrap();
        assert!(bridge
            .write(TerminalWriteRequest {
                session_id: "s1".into(),
                data: vec![],
            })
            .is_err());
    }

    #[test]
    fn test_bridge_error_display_includes_context() {
        assert!(DesktopBridgeError::MissingTerminalSession {
            session_id: "s9".into()
        }
        .to_string()
        .contains("s9"));
        assert!(DesktopBridgeError::InvalidTerminalRequest {
            message: "bad dims".into()
        }
        .to_string()
        .contains("bad dims"));
        assert!(DesktopBridgeError::TerminalSpawnFailed {
            message: "no pty".into()
        }
        .to_string()
        .contains("no pty"));
        assert!(DesktopBridgeError::TerminalWriteFailed {
            message: "eof".into()
        }
        .to_string()
        .contains("eof"));
        assert!(DesktopBridgeError::UnsupportedNativeIsland {
            kind: "appkit".into()
        }
        .to_string()
        .contains("appkit"));
        assert!(DesktopBridgeError::NativeIslandFailed {
            message: "boom".into()
        }
        .to_string()
        .contains("boom"));
    }

    #[test]
    fn test_terminal_request_types_serde_round_trip() {
        let req = TerminalOpenRequest {
            session_id: Some("s1".into()),
            command: "bash".into(),
            args: vec!["-l".into()],
            cwd: Some("/tmp".into()),
            env: HashMap::from([("K".to_string(), "V".to_string())]),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 24,
            cols: 80,
        };
        let back: TerminalOpenRequest =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(req, back);

        let resp = TerminalSessionResponse {
            session_id: "s1".into(),
            alive: true,
            rows: 24,
            cols: 80,
        };
        let back: TerminalSessionResponse =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn test_empty_payload_is_empty_object() {
        assert_eq!(empty_payload(), serde_json::json!({}));
    }
}
