//! Voice server — same shape as [`crate::mcp::server::McpServer`].
//!
//! Wraps `Arc<ToolRegistry>` + `ToolContext` + voice policy and exposes:
//! - JSON-line methods `tools/list` and `tools/call` (stdio / 127.0.0.1 TCP)
//! - HTTP webhook `POST /voice/tools` for ElevenLabs **server tools**
//! - Schema export for ElevenLabs client-tool registration
//!
//! Tool execution always goes through [`super::adapter::VoiceToolBridge`] →
//! real `ToolRegistry::execute` (not a parallel toy registry).

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::daemon::{read_bounded_line, BoundedLine, MAX_REQUEST_SIZE};
use crate::tooling::{ToolContext, ToolRegistry};

use super::adapter::VoiceToolBridge;
use super::envelope::{ElevenLabsClientToolRequest, ElevenLabsToolResult, VoiceToolCallSource};
use super::policy::VoicePolicy;
use super::schema::{elevenlabs_client_tool_schemas, ElevenLabsClientToolSchema};
use super::webhook::parse_webhook_tool_request;

/// Transport for the registry-backed voice server (mirrors MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTransport {
    /// JSON-line protocol on stdio (`tools/list`, `tools/call`).
    Stdio,
    /// JSON-line protocol on `127.0.0.1:port`.
    Tcp(u16),
    /// HTTP webhook on `127.0.0.1:port` for ElevenLabs server tools.
    Webhook(u16),
}

/// Registry-backed voice server (MCP twin for ElevenLabs tool calling).
pub struct VoiceServer {
    bridge: Arc<VoiceToolBridge>,
}

impl VoiceServer {
    pub fn new(registry: Arc<ToolRegistry>, ctx: ToolContext, policy: VoicePolicy) -> Self {
        Self {
            bridge: Arc::new(VoiceToolBridge::new(registry, ctx, policy)),
        }
    }

    pub fn with_defaults() -> Self {
        Self {
            bridge: Arc::new(VoiceToolBridge::with_defaults()),
        }
    }

    pub fn bridge(&self) -> &VoiceToolBridge {
        &self.bridge
    }

    /// Export ElevenLabs client-tool schemas from the live registry + policy.
    pub fn client_tool_schemas(&self) -> Vec<ElevenLabsClientToolSchema> {
        elevenlabs_client_tool_schemas(self.bridge.registry(), self.bridge.policy())
    }

    pub async fn serve(&self, transport: VoiceTransport) -> Result<()> {
        match transport {
            VoiceTransport::Stdio => self.serve_stdio().await,
            VoiceTransport::Tcp(port) => self.serve_jsonline_tcp(port).await,
            VoiceTransport::Webhook(port) => self.serve_webhook_http(port).await,
        }
    }

    /// Bounded JSON-line loop (same discipline as MCP stdio).
    async fn serve_stdio(&self) -> Result<()> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut writer = tokio::io::BufWriter::new(stdout);

        loop {
            let line = match read_bounded_line(&mut reader, MAX_REQUEST_SIZE).await? {
                BoundedLine::Eof => break,
                BoundedLine::TooLarge => {
                    let response = serde_json::json!({
                        "error": {
                            "code": -32600,
                            "message": format!("Request too large (max {} bytes)", MAX_REQUEST_SIZE)
                        }
                    });
                    writer
                        .write_all(serde_json::to_string(&response)?.as_bytes())
                        .await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    break;
                }
                BoundedLine::Line(line) => line,
            };
            let response = self.process_request(&line).await;
            writer
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
        Ok(())
    }

    async fn serve_jsonline_tcp(&self, port: u16) -> Result<()> {
        let addr = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("bind voice tcp {addr}"))?;
        eprintln!("Voice JSON-line server listening on {addr} (tools/list, tools/call)");

        loop {
            let (socket, _) = listener.accept().await?;
            let bridge = Arc::clone(&self.bridge);
            tokio::spawn(async move {
                let server = VoiceServer { bridge };
                let (reader, mut writer) = socket.into_split();
                let mut reader = BufReader::new(reader);
                loop {
                    let line = match read_bounded_line(&mut reader, MAX_REQUEST_SIZE).await {
                        Ok(BoundedLine::Eof) => break,
                        Ok(BoundedLine::TooLarge) => {
                            let _ = writer
                                .write_all(
                                    serde_json::json!({
                                        "error": {"code": -32600, "message": "request too large"}
                                    })
                                    .to_string()
                                    .as_bytes(),
                                )
                                .await;
                            let _ = writer.write_all(b"\n").await;
                            break;
                        }
                        Ok(BoundedLine::Line(line)) => line,
                        Err(_) => break,
                    };
                    let response = server.process_request(&line).await;
                    if writer
                        .write_all(
                            serde_json::to_string(&response)
                                .unwrap_or_default()
                                .as_bytes(),
                        )
                        .await
                        .is_err()
                    {
                        break;
                    }
                    if writer.write_all(b"\n").await.is_err() {
                        break;
                    }
                }
            });
        }
    }

    /// Minimal HTTP/1.1 server for ElevenLabs server-tool webhooks.
    ///
    /// - `GET  /healthz` → `ok`
    /// - `GET  /voice/schema` → ElevenLabs client-tool schemas JSON
    /// - `POST /voice/tools` → tool invoke (body = webhook/client-tool JSON)
    async fn serve_webhook_http(&self, port: u16) -> Result<()> {
        let addr = format!("127.0.0.1:{port}");
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("bind voice webhook {addr}"))?;
        eprintln!(
            "Voice webhook server listening on http://{addr}/voice/tools (ElevenLabs server tools)"
        );

        loop {
            let (stream, _) = listener.accept().await?;
            let bridge = Arc::clone(&self.bridge);
            tokio::spawn(async move {
                if let Err(err) = handle_http_connection(stream, bridge).await {
                    tracing::debug!(error = %err, "voice webhook connection closed");
                }
            });
        }
    }

    /// Process one JSON-line request (MCP-compatible method names).
    pub async fn process_request(&self, request_str: &str) -> serde_json::Value {
        let request: serde_json::Value = match serde_json::from_str(request_str) {
            Ok(value) => value,
            Err(err) => {
                return serde_json::json!({
                    "error": {"code": -32700, "message": format!("Parse error: {err}")}
                });
            }
        };

        let method = request
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match method {
            "tools/list" => {
                let tools = self.client_tool_schemas();
                serde_json::json!({ "tools": tools, "provider": "elevenlabs_agent" })
            }
            "tools/call" => {
                let params = request.get("params").unwrap_or(&request);
                let name = params
                    .get("name")
                    .or_else(|| params.get("tool"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = params
                    .get("arguments")
                    .or_else(|| params.get("params"))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let confirmed = params
                    .get("confirmed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let tool_call_id = params
                    .get("tool_call_id")
                    .or_else(|| params.get("id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let req = ElevenLabsClientToolRequest {
                    tool_call_id,
                    tool: name,
                    params: arguments,
                    confirmed,
                    wait_for_response: true,
                    source: VoiceToolCallSource::ClientTool,
                };
                let result = self.bridge.handle_client_tool(req).await;
                serde_json::to_value(result).unwrap_or_else(
                    |e| serde_json::json!({"error": {"code": -32603, "message": e.to_string()}}),
                )
            }
            "voice/schema" => {
                serde_json::json!({ "client_tools": self.client_tool_schemas() })
            }
            _ => serde_json::json!({
                "error": {"code": -32601, "message": "Method not found (use tools/list, tools/call, voice/schema)"}
            }),
        }
    }
}

async fn handle_http_connection(mut stream: TcpStream, bridge: Arc<VoiceToolBridge>) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let raw = &buf[..n];
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("incomplete HTTP headers")?;
    let header_text = std::str::from_utf8(&raw[..header_end]).unwrap_or("");
    let body = &raw[header_end + 4..];

    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let content_length = header_text
        .lines()
        .find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(body.len());

    let mut body_owned = body.to_vec();
    while body_owned.len() < content_length {
        let mut more = vec![0u8; content_length - body_owned.len()];
        let m = stream.read(&mut more).await?;
        if m == 0 {
            break;
        }
        body_owned.extend_from_slice(&more[..m]);
    }
    body_owned.truncate(content_length);

    let (status, content_type, payload) = match (method, path) {
        ("GET", "/healthz") | ("GET", "/health") => (200, "text/plain", b"ok".to_vec()),
        ("GET", "/voice/schema") => {
            let schemas = elevenlabs_client_tool_schemas(bridge.registry(), bridge.policy());
            let json = serde_json::json!({ "client_tools": schemas });
            (
                200,
                "application/json",
                serde_json::to_vec_pretty(&json).unwrap_or_default(),
            )
        }
        ("POST", "/voice/tools") | ("POST", "/tools/call") => {
            let result = match parse_webhook_tool_request(&body_owned) {
                Ok(req) => bridge.handle_client_tool(req).await,
                Err(err) => ElevenLabsToolResult::error("", None, err),
            };
            let status_code = match result.status {
                super::envelope::ElevenLabsToolResultStatus::Ok => 200,
                super::envelope::ElevenLabsToolResultStatus::Denied => 403,
                super::envelope::ElevenLabsToolResultStatus::Error => 400,
            };
            (
                status_code,
                "application/json",
                serde_json::to_vec_pretty(&result).unwrap_or_default(),
            )
        }
        _ => (
            404,
            "application/json",
            br#"{"error":"not found; use GET /healthz, GET /voice/schema, POST /voice/tools"}"#
                .to_vec(),
        ),
    };

    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tools_list_exports_registry_backed_schemas() {
        let server = VoiceServer::with_defaults();
        let resp = server.process_request(r#"{"method":"tools/list"}"#).await;
        let tools = resp["tools"].as_array().expect("tools array");
        assert!(
            tools.iter().any(|t| t["name"] == "system_info"),
            "expected system_info in {tools:?}"
        );
        assert_eq!(resp["provider"], "elevenlabs_agent");
    }

    #[tokio::test]
    async fn tools_call_runs_real_system_info() {
        let server = VoiceServer::with_defaults();
        let resp = server
            .process_request(
                r#"{"method":"tools/call","params":{"name":"system_info","arguments":{"include_env":false}}}"#,
            )
            .await;
        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["tool"], "system_info");
        assert!(resp["result"]["output"]["os"].is_string());
    }

    #[tokio::test]
    async fn tools_call_denies_bash_without_confirm() {
        let server = VoiceServer::with_defaults();
        let resp = server
            .process_request(
                r#"{"method":"tools/call","params":{"name":"bash_exec","arguments":{"command":"echo no"}}}"#,
            )
            .await;
        assert_eq!(resp["status"], "denied");
    }
}
