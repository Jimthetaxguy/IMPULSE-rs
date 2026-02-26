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

        Ok(self.stream.as_mut().unwrap())
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
}
