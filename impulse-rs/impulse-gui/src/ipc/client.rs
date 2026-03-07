//! Synchronous Unix socket client for the Impulse daemon.
//!
//! Uses `std::os::unix::net::UnixStream` — no tokio dependency.
//! Designed to run on a background `std::thread`, writing results to
//! `Arc<Mutex<SharedState>>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use super::types::*;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

pub struct DaemonClient {
    socket_path: PathBuf,
    stream: Option<UnixStream>,
}

impl DaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            stream: None,
        }
    }

    /// Update the socket path, dropping any existing connection.
    #[allow(dead_code)]
    pub fn set_socket_path(&mut self, path: std::path::PathBuf) {
        self.stream = None;
        self.socket_path = path;
    }

    /// Discover the socket path from the current directory.
    /// Looks for `.impulse/sockets/impulse.sock` relative to cwd,
    /// then falls back to `IMPULSE_SOCKET_PATH` env var.
    pub fn discover() -> Self {
        if let Ok(path) = std::env::var("IMPULSE_SOCKET_PATH") {
            return Self::new(PathBuf::from(path));
        }

        // Walk up from cwd looking for .impulse/sockets/impulse.sock
        if let Ok(cwd) = std::env::current_dir() {
            let mut dir = cwd.as_path();
            loop {
                let candidate = dir.join(".impulse/sockets/impulse.sock");
                if candidate.exists() {
                    return Self::new(candidate);
                }
                match dir.parent() {
                    Some(parent) => dir = parent,
                    None => break,
                }
            }
        }

        // Fallback to relative path (works if launched from project root)
        Self::new(PathBuf::from(".impulse/sockets/impulse.sock"))
    }

    /// Attempt to connect to the daemon socket.
    fn ensure_connected(&mut self) -> Result<&mut UnixStream, String> {
        if self.stream.is_none() {
            let stream = UnixStream::connect(&self.socket_path)
                .map_err(|e| format!("connect to {}: {}", self.socket_path.display(), e))?;

            stream
                .set_read_timeout(Some(READ_TIMEOUT))
                .map_err(|e| format!("set read timeout: {}", e))?;
            stream
                .set_write_timeout(Some(CONNECT_TIMEOUT))
                .map_err(|e| format!("set write timeout: {}", e))?;

            self.stream = Some(stream);
        }

        self.stream
            .as_mut()
            .ok_or_else(|| "stream not initialized after connect".to_string())
    }

    /// Send a request and read one response line.
    fn send(&mut self, req: &DaemonRequest) -> Result<DaemonResponse, String> {
        let json = serde_json::to_string(req).map_err(|e| format!("serialize: {}", e))?;
        let mut line = String::new();

        // Try to send twice (once with existing connection, once with fresh one if it fails).
        for attempt in 0..2 {
            let stream = match self.ensure_connected() {
                Ok(s) => s,
                Err(e) => {
                    if attempt == 0 {
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };

            // Write request.
            if stream.write_all(json.as_bytes()).is_err() {
                self.stream = None;
                continue;
            }
            if stream.write_all(b"\n").is_err() {
                self.stream = None;
                continue;
            }
            if stream.flush().is_err() {
                self.stream = None;
                continue;
            }

            // Read response.
            let mut reader = BufReader::new(&*stream);
            match reader.read_line(&mut line) {
                Ok(0) => {
                    self.stream = None;
                    continue;
                }
                Ok(_) => break,
                Err(_) => {
                    self.stream = None;
                    continue;
                }
            }
        }

        if line.is_empty() {
            return Err("daemon closed connection".to_string());
        }

        serde_json::from_str(&line).map_err(|e| format!("parse response: {}", e))
    }

    /// Extract the `result` value from an Ok response.
    fn ok_result(&self, resp: DaemonResponse) -> Result<serde_json::Value, String> {
        match resp {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => Err(message),
            DaemonResponse::AgentAssistResult { .. } => {
                Err("unexpected AgentAssistResult".to_string())
            }
            DaemonResponse::ConflictCheck { .. } => Err("unexpected ConflictCheck".to_string()),
        }
    }

    // -- High-level API --

    pub fn ping(&mut self) -> Result<bool, String> {
        let resp = self.send(&DaemonRequest::Ping)?;
        let result = self.ok_result(resp)?;
        Ok(result
            .get("pong")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub fn status(&mut self) -> Result<DaemonStatus, String> {
        let resp = self.send(&DaemonRequest::Status)?;
        let result = self.ok_result(resp)?;
        Ok(DaemonStatus {
            sessions: result.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            active: result.get("active").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        })
    }

    pub fn list_sessions(&mut self) -> Result<Vec<Session>, String> {
        let resp = self.send(&DaemonRequest::ListSessions)?;
        let result = self.ok_result(resp)?;

        let sessions = result
            .as_array()
            .map(|arr| arr.iter().filter_map(Session::from_value).collect())
            .unwrap_or_default();

        Ok(sessions)
    }

    #[allow(dead_code)]
    pub fn get_session(&mut self, id: &str) -> Result<Session, String> {
        let resp = self.send(&DaemonRequest::GetSession {
            session_id: id.to_string(),
        })?;
        let result = self.ok_result(resp)?;
        Session::from_value(&result).ok_or_else(|| "failed to parse session".to_string())
    }

    pub fn list_history(&mut self) -> Result<Vec<HistoryEntry>, String> {
        let resp = self.send(&DaemonRequest::InvokeTool {
            name: "session_query".to_string(),
            params: serde_json::json!({"limit": 50}),
        })?;
        let result = self.ok_result(resp)?;

        // InvokeTool returns {"tool": "name", "output": { ... }}
        let output = result.get("output").unwrap_or(&result);

        let entries = output
            .get("sessions")
            .or_else(|| output.get("entries"))
            .or_else(|| output.as_array().map(|_| output))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(HistoryEntry::from_value).collect())
            .unwrap_or_default();

        Ok(entries)
    }

    pub fn read_genome(&mut self) -> Result<Genome, String> {
        let resp = self.send(&DaemonRequest::InvokeTool {
            name: "genome_read".to_string(),
            params: serde_json::Value::Null,
        })?;
        let result = self.ok_result(resp)?;
        let output = result.get("output").unwrap_or(&result);
        Ok(Genome::from_value(output))
    }

    pub fn search(&mut self, query: &str) -> Result<Vec<SearchResult>, String> {
        let resp = self.send(&DaemonRequest::InvokeTool {
            name: "memory_search".to_string(),
            params: serde_json::json!({
                "query": query,
                "limit": 20,
            }),
        })?;
        let result = self.ok_result(resp)?;
        let output = result.get("output").unwrap_or(&result);

        let results = output
            .get("results")
            .or_else(|| output.as_array().map(|_| output))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(SearchResult::from_value).collect())
            .unwrap_or_default();

        Ok(results)
    }

    pub fn get_ops_snapshot(&mut self) -> Result<impulse_ops::ProjectOpsSnapshot, String> {
        let resp = self.send(&DaemonRequest::GetOpsSnapshot)?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse ops snapshot: {}", e))
    }

    pub fn subscribe_ops(
        &mut self,
        since_seq: Option<u64>,
    ) -> Result<impulse_ops::OpsSubscription, String> {
        let resp = self.send(&DaemonRequest::SubscribeOps { since_seq })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse ops subscription: {}", e))
    }

    pub fn publish_terminal_ops(
        &mut self,
        report: &impulse_ops::TerminalOpsReport,
    ) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::PublishTerminalOps {
            report: report.clone(),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    pub fn get_supervisor_permissions(
        &mut self,
    ) -> Result<impulse_ops::SupervisorPermissionState, String> {
        let resp = self.send(&DaemonRequest::GetSupervisorPermissions)?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse supervisor permissions: {}", e))
    }

    pub fn supervisor_chat(
        &mut self,
        prompt: &str,
        context: Option<&str>,
    ) -> Result<impulse_ops::SupervisorChatResult, String> {
        let resp = self.send(&DaemonRequest::SupervisorChat {
            prompt: prompt.to_string(),
            context: context.map(String::from),
        })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse supervisor chat: {}", e))
    }

    pub fn run_supervisor_action(
        &mut self,
        action: impulse_ops::SupervisorAction,
    ) -> Result<impulse_ops::SupervisorActionResult, String> {
        let resp = self.send(&DaemonRequest::RunSupervisorAction { action })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse supervisor action result: {}", e))
    }

    #[allow(dead_code)]
    pub fn list_artifacts(
        &mut self,
        limit: Option<usize>,
    ) -> Result<Vec<impulse_ops::ArtifactEnvelope>, String> {
        let resp = self.send(&DaemonRequest::ListArtifacts { limit })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse artifacts: {}", e))
    }

    #[allow(dead_code)]
    pub fn get_artifact(
        &mut self,
        artifact_id: &str,
    ) -> Result<impulse_ops::ArtifactEnvelope, String> {
        let resp = self.send(&DaemonRequest::GetArtifact {
            artifact_id: artifact_id.to_string(),
        })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse artifact: {}", e))
    }

    pub fn run_artifact_action(
        &mut self,
        artifact_id: &str,
        action_id: &str,
        params: serde_json::Value,
    ) -> Result<impulse_ops::ArtifactActionResult, String> {
        let resp = self.send(&DaemonRequest::RunArtifactAction {
            artifact_id: artifact_id.to_string(),
            action_id: action_id.to_string(),
            params,
        })?;
        let result = self.ok_result(resp)?;
        serde_json::from_value(result).map_err(|e| format!("parse artifact action: {}", e))
    }

    // -- Session management --

    pub fn create_session(
        &mut self,
        name: &str,
        platform: Option<&str>,
    ) -> Result<Session, String> {
        let resp = self.send(&DaemonRequest::CreateSession {
            name: name.to_string(),
            platform: platform.map(String::from),
        })?;
        let result = self.ok_result(resp)?;
        Session::from_value(&result).ok_or_else(|| "failed to parse created session".to_string())
    }

    pub fn end_session(&mut self, session_id: &str, summary: &str) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::EndSession {
            session_id: session_id.to_string(),
            summary: summary.to_string(),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    pub fn track_file(&mut self, session_id: &str, file_path: &str) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::TrackFile {
            session_id: session_id.to_string(),
            file_path: file_path.to_string(),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn track_tool(&mut self, session_id: &str, tool_name: &str) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::TrackTool {
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    // -- Phase 3 endpoints (consumed when Stewardship/Tools/Chat views land) --

    #[allow(dead_code)]
    pub fn chat(
        &mut self,
        session_id: &str,
        message: &str,
        inject_mode: Option<&str>,
    ) -> Result<ChatResponse, String> {
        let resp = self.send(&DaemonRequest::Chat {
            session_id: session_id.to_string(),
            message: message.to_string(),
            inject_mode: inject_mode.map(String::from),
            inject_explain: false,
        })?;
        let result = self.ok_result(resp)?;
        Ok(ChatResponse::from_value(&result))
    }

    #[allow(dead_code)]
    pub fn steward_status(&mut self) -> Result<StewardshipStatus, String> {
        let resp = self.send(&DaemonRequest::StewardStatus)?;
        let result = self.ok_result(resp)?;
        Ok(StewardshipStatus::from_value(&result))
    }

    #[allow(dead_code)]
    pub fn list_proposals(&mut self) -> Result<Vec<Proposal>, String> {
        let resp = self.send(&DaemonRequest::StewardProposals {
            action: "list".to_string(),
            id: None,
        })?;
        let result = self.ok_result(resp)?;
        let proposals = result
            .get("proposals")
            .or_else(|| result.as_array().map(|_| &result))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(Proposal::from_value).collect())
            .unwrap_or_default();
        Ok(proposals)
    }

    #[allow(dead_code)]
    pub fn approve_proposal(&mut self, id: &str) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::StewardProposals {
            action: "approve".to_string(),
            id: Some(id.to_string()),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn reject_proposal(&mut self, id: &str) -> Result<(), String> {
        let resp = self.send(&DaemonRequest::StewardProposals {
            action: "reject".to_string(),
            id: Some(id.to_string()),
        })?;
        self.ok_result(resp)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn steward_memory(&mut self) -> Result<CrossProjectMemory, String> {
        let resp = self.send(&DaemonRequest::StewardMemory)?;
        let result = self.ok_result(resp)?;
        Ok(CrossProjectMemory::from_value(&result))
    }

    #[allow(dead_code)]
    pub fn list_tools(&mut self, category: Option<&str>) -> Result<Vec<ToolInfo>, String> {
        let resp = self.send(&DaemonRequest::ListTools {
            category: category.map(String::from),
        })?;
        let result = self.ok_result(resp)?;
        let tools = result
            .get("tools")
            .or_else(|| result.as_array().map(|_| &result))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(ToolInfo::from_value).collect())
            .unwrap_or_default();
        Ok(tools)
    }

    #[allow(dead_code)]
    pub fn describe_tool(&mut self, name: &str) -> Result<ToolInfo, String> {
        let resp = self.send(&DaemonRequest::DescribeTool {
            name: name.to_string(),
        })?;
        let result = self.ok_result(resp)?;
        ToolInfo::from_value(&result).ok_or_else(|| "failed to parse tool info".to_string())
    }

    // -- Agent coordination --

    #[allow(dead_code)]
    pub fn agent_assist(&mut self, prompt: &str, context: Option<&str>) -> Result<String, String> {
        let resp = self.send(&DaemonRequest::AgentAssist {
            prompt: prompt.to_string(),
            context: context.map(String::from),
        })?;
        match resp {
            DaemonResponse::AgentAssistResult { response, .. } => Ok(response),
            other => {
                let result = self.ok_result(other)?;
                Ok(result
                    .get("response")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string())
            }
        }
    }
}
