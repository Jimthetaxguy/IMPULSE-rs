use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::daemon::{DaemonRequest, DaemonResponse};

pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    #[allow(dead_code)]
    pub fn default_path() -> Self {
        Self::new(PathBuf::from(".impulse/sockets/impulse.sock"))
    }

    pub async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .context(format!(
                "Failed to connect to socket: {}",
                self.socket_path.display()
            ))
    }

    pub async fn send(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        let mut stream = self.connect().await?;

        let request_json = serde_json::to_string(&request)?;
        stream.write_all(request_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let (reader, _) = stream.split();
        let mut reader = BufReader::new(reader);

        let mut response_line = String::new();
        reader.read_line(&mut response_line).await?;

        let response: DaemonResponse =
            serde_json::from_str(&response_line).context("Failed to parse daemon response")?;

        Ok(response)
    }

    pub async fn ping(&self) -> Result<bool> {
        let response = self.send(DaemonRequest::Ping).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result
                .get("pong")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)),
            DaemonResponse::Error { message } => {
                anyhow::bail!("Ping failed: {}", message)
            }
            _ => anyhow::bail!("Ping: unexpected response type"),
        }
    }

    pub async fn status(&self) -> Result<serde_json::Value> {
        let response = self.send(DaemonRequest::Status).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Status failed: {}", message),
            _ => anyhow::bail!("Status: unexpected response type"),
        }
    }

    pub async fn create_session(
        &self,
        name: String,
        platform: Option<String>,
    ) -> Result<(String, String)> {
        let response = self
            .send(DaemonRequest::CreateSession { name, platform })
            .await?;

        match response {
            DaemonResponse::Ok { result } => {
                let session_id = result["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' in response"))?
                    .to_string();
                let session_name = result["name"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'name' in response"))?
                    .to_string();
                Ok((session_id, session_name))
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Create session failed: {}", message)
            }
            _ => anyhow::bail!("Create session: unexpected response type"),
        }
    }

    pub async fn end_session(&self, session_id: String, summary: String) -> Result<String> {
        let response = self
            .send(DaemonRequest::EndSession {
                session_id,
                summary,
            })
            .await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result["session_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'session_id' in response"))?
                .to_string()),
            DaemonResponse::Error { message } => anyhow::bail!("End session failed: {}", message),
            _ => anyhow::bail!("End session: unexpected response type"),
        }
    }

    pub async fn track_file(&self, session_id: String, file_path: String) -> Result<()> {
        let response = self
            .send(DaemonRequest::TrackFile {
                session_id,
                file_path,
            })
            .await?;

        match response {
            DaemonResponse::Ok { .. } => Ok(()),
            DaemonResponse::Error { message } => anyhow::bail!("Track file failed: {}", message),
            _ => anyhow::bail!("Track file: unexpected response type"),
        }
    }

    pub async fn track_tool(&self, session_id: String, tool_name: String) -> Result<()> {
        let response = self
            .send(DaemonRequest::TrackTool {
                session_id,
                tool_name,
            })
            .await?;

        match response {
            DaemonResponse::Ok { .. } => Ok(()),
            DaemonResponse::Error { message } => anyhow::bail!("Track tool failed: {}", message),
            _ => anyhow::bail!("Track tool: unexpected response type"),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<serde_json::Value>> {
        let response = self.send(DaemonRequest::ListSessions).await?;

        match response {
            DaemonResponse::Ok { result } => {
                let sessions = result.as_array().cloned().unwrap_or_default();
                Ok(sessions)
            }
            DaemonResponse::Error { message } => anyhow::bail!("List sessions failed: {}", message),
            _ => anyhow::bail!("List sessions: unexpected response type"),
        }
    }

    pub async fn get_session(&self, session_id: String) -> Result<serde_json::Value> {
        let response = self.send(DaemonRequest::GetSession { session_id }).await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Get session failed: {}", message),
            _ => anyhow::bail!("Get session: unexpected response type"),
        }
    }

    pub async fn chat(
        &self,
        session_id: String,
        message: String,
        inject_mode: Option<String>,
        inject_explain: bool,
    ) -> Result<serde_json::Value> {
        let response = self
            .send(DaemonRequest::Chat {
                session_id,
                message,
                inject_mode,
                inject_explain,
            })
            .await?;

        match response {
            DaemonResponse::Ok { result } => Ok(result),
            DaemonResponse::Error { message } => anyhow::bail!("Chat failed: {}", message),
            _ => anyhow::bail!("Chat: unexpected response type"),
        }
    }

    pub async fn agent_assist(&self, prompt: &str, context: Option<&str>) -> Result<String> {
        let response = self
            .send(DaemonRequest::AgentAssist {
                prompt: prompt.to_string(),
                context: context.map(|c| c.to_string()),
            })
            .await?;

        match response {
            DaemonResponse::AgentAssistResult { success, response } => {
                if success {
                    Ok(response)
                } else {
                    anyhow::bail!("Agent assist failed: {}", response)
                }
            }
            DaemonResponse::Error { message } => {
                anyhow::bail!("Agent assist failed: {}", message)
            }
            _ => anyhow::bail!("Agent assist: unexpected response type"),
        }
    }
}
