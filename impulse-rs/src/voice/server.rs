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

/// Environment variable holding the webhook's shared secret.
pub const WEBHOOK_SECRET_ENV: &str = "IMPULSE_VOICE_WEBHOOK_SECRET";

/// Largest HTTP header block the webhook reads before refusing the request.
const MAX_WEBHOOK_HEADER_BYTES: usize = 64 * 1024;

/// Webhook connections handled at once. Each may hold up to
/// `MAX_REQUEST_SIZE` bytes for up to `WEBHOOK_REQUEST_TIMEOUT`, so without a
/// cap one client opening connections could pin memory and tasks at will.
const MAX_WEBHOOK_CONNECTIONS: usize = 64;

/// How long one webhook connection may take to send its request.
const WEBHOOK_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// How the webhook authenticates callers. It is reachable through a public
/// tunnel in the documented setup, and even read-only tools (`file_read`,
/// `config_get`) disclose project files, so every route except the health
/// check requires the shared secret unless the operator explicitly opted out.
#[derive(Clone)]
pub enum WebhookAuth {
    /// `Authorization: Bearer <secret>` is required.
    Bearer(String),
    /// No authentication (`voice serve --allow-unauthenticated`).
    Unauthenticated,
}

impl std::fmt::Debug for WebhookAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Bearer(_) => "WebhookAuth::Bearer(<redacted>)",
            Self::Unauthenticated => "WebhookAuth::Unauthenticated",
        })
    }
}

impl WebhookAuth {
    fn admits(&self, headers: &str) -> bool {
        let Self::Bearer(secret) = self else {
            return true;
        };
        headers.lines().any(|line| {
            let Some((name, value)) = line.split_once(':') else {
                return false;
            };
            // RFC 7235: the scheme is case-insensitive and any whitespace
            // may separate it from the token.
            let mut credentials = value.trim().splitn(2, char::is_whitespace);
            let scheme = credentials.next().unwrap_or_default();
            name.trim().eq_ignore_ascii_case("authorization")
                && scheme.eq_ignore_ascii_case("bearer")
                && credentials
                    .next()
                    .is_some_and(|presented| constant_time_eq(secret, presented.trim()))
        })
    }
}

/// Compares in time independent of how many leading bytes match.
fn constant_time_eq(expected: &str, presented: &str) -> bool {
    let (expected, presented) = (expected.as_bytes(), presented.as_bytes());
    if expected.len() != presented.len() {
        return false;
    }
    expected
        .iter()
        .zip(presented)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

/// Whether a JSON-line transport's peer may confirm a mutating tool call.
/// Only stdio qualifies: its peer is the process that launched the server.
/// Any local process (or a browser page sending a cross-protocol request)
/// can reach the TCP port, so a `confirmed: true` arriving there is ignored,
/// the same rule the webhook applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationTrust {
    TrustedPeer,
    Untrusted,
}

/// Registry-backed voice server (MCP twin for ElevenLabs tool calling).
pub struct VoiceServer {
    bridge: Arc<VoiceToolBridge>,
    webhook_auth: WebhookAuth,
}

impl VoiceServer {
    pub fn new(registry: Arc<ToolRegistry>, ctx: ToolContext, policy: VoicePolicy) -> Self {
        Self {
            bridge: Arc::new(VoiceToolBridge::new(registry, ctx, policy)),
            webhook_auth: WebhookAuth::Unauthenticated,
        }
    }

    pub fn with_defaults() -> Self {
        Self {
            bridge: Arc::new(VoiceToolBridge::with_defaults()),
            webhook_auth: WebhookAuth::Unauthenticated,
        }
    }

    /// Sets how the webhook transport authenticates callers.
    pub fn with_webhook_auth(mut self, auth: WebhookAuth) -> Self {
        self.webhook_auth = auth;
        self
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
            let response = self
                .process_request_with_trust(&line, ConfirmationTrust::TrustedPeer)
                .await;
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
        eprintln!(
            "Voice JSON-line server listening on {addr} (tools/list, tools/call); \
             mutating tools are never confirmed over TCP"
        );

        loop {
            let socket = match listener.accept().await {
                Ok((socket, _)) => socket,
                Err(error) => {
                    // A transient accept failure (EMFILE, ECONNABORTED) must
                    // not end the server; pausing keeps EMFILE from spinning.
                    tracing::warn!(%error, "voice tcp accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            let bridge = Arc::clone(&self.bridge);
            tokio::spawn(async move {
                let server = VoiceServer {
                    bridge,
                    webhook_auth: WebhookAuth::Unauthenticated,
                };
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
                    let response = server
                        .process_request_with_trust(&line, ConfirmationTrust::Untrusted)
                        .await;
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
        if matches!(self.webhook_auth, WebhookAuth::Unauthenticated) {
            eprintln!(
                "WARNING: the voice webhook is unauthenticated; anyone who can reach this port \
                 (including through a tunnel) can call its tools. Set {WEBHOOK_SECRET_ENV} instead."
            );
        }
        self.serve_webhook_on(listener).await
    }

    /// The webhook accept loop on an already-bound listener (tests bind port 0).
    async fn serve_webhook_on(&self, listener: TcpListener) -> Result<()> {
        self.serve_webhook_with_limit(listener, MAX_WEBHOOK_CONNECTIONS)
            .await
    }

    /// [`Self::serve_webhook_on`] with the connection cap passed in. A
    /// connection over the cap is closed at once instead of queued, so a
    /// client holding connections open cannot pin more than the cap.
    async fn serve_webhook_with_limit(
        &self,
        listener: TcpListener,
        max_connections: usize,
    ) -> Result<()> {
        let slots = Arc::new(tokio::sync::Semaphore::new(max_connections));
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(error) => {
                    tracing::warn!(%error, "voice webhook accept failed");
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            // Over the cap the connection is closed at once; a written reply
            // would need a task per refusal to drain the request first.
            let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                drop(stream);
                continue;
            };
            let bridge = Arc::clone(&self.bridge);
            let auth = self.webhook_auth.clone();
            tokio::spawn(async move {
                let _slot = slot;
                let handled = tokio::time::timeout(
                    WEBHOOK_REQUEST_TIMEOUT,
                    handle_http_connection(stream, bridge, auth),
                )
                .await;
                match handled {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        tracing::debug!(error = %err, "voice webhook connection closed")
                    }
                    Err(_) => tracing::debug!("voice webhook connection timed out"),
                }
            });
        }
    }

    /// Process one JSON-line request from a trusted peer (stdio). See
    /// [`Self::process_request_with_trust`].
    pub async fn process_request(&self, request_str: &str) -> serde_json::Value {
        self.process_request_with_trust(request_str, ConfirmationTrust::TrustedPeer)
            .await
    }

    /// Process one JSON-line request (MCP-compatible method names). A
    /// `confirmed` flag is honored only from a [`ConfirmationTrust::TrustedPeer`].
    pub async fn process_request_with_trust(
        &self,
        request_str: &str,
        trust: ConfirmationTrust,
    ) -> serde_json::Value {
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
                let confirmed = trust == ConfirmationTrust::TrustedPeer
                    && params
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

async fn handle_http_connection(
    mut stream: TcpStream,
    bridge: Arc<VoiceToolBridge>,
    auth: WebhookAuth,
) -> Result<()> {
    // Read until the end of the header block, which may arrive in pieces.
    let mut raw = Vec::with_capacity(8 * 1024);
    let mut chunk = vec![0u8; 8 * 1024];
    let header_end = loop {
        if let Some(position) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break position;
        }
        if raw.len() > MAX_WEBHOOK_HEADER_BYTES {
            return write_http_response(
                &mut stream,
                431,
                "application/json",
                br#"{"error":"request headers too large"}"#,
            )
            .await;
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&chunk[..n]);
    };
    let header_text = std::str::from_utf8(&raw[..header_end]).unwrap_or("");
    let body = &raw[header_end + 4..];

    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let is_health = method == "GET" && matches!(path, "/healthz" | "/health");
    if !is_health && !auth.admits(header_text) {
        return write_http_response(
            &mut stream,
            401,
            "application/json",
            br#"{"error":"missing or wrong Authorization bearer secret"}"#,
        )
        .await;
    }

    let content_length = header_text
        .lines()
        .find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(body.len());
    // Refuse before allocating: a caller-chosen length is never trusted.
    if content_length > MAX_REQUEST_SIZE {
        return write_http_response(
            &mut stream,
            413,
            "application/json",
            br#"{"error":"request body too large"}"#,
        )
        .await;
    }

    // A health check needs no body, and it is unauthenticated, so reading
    // a declared one let any client make the server buffer up to the limit.
    let content_length = if is_health { 0 } else { content_length };
    let mut body_owned = body.to_vec();
    while body_owned.len() < content_length {
        let m = stream.read(&mut chunk).await?;
        if m == 0 {
            break;
        }
        body_owned.extend_from_slice(&chunk[..m]);
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

    write_http_response(&mut stream, status, content_type, &payload).await
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    payload: &[u8],
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        payload.len()
    );
    if status == 401 {
        response.push_str("WWW-Authenticate: Bearer\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).await?;
    stream.write_all(payload).await?;
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

    /// Review P1: any local process can reach the TCP transport, so a
    /// client-supplied `confirmed: true` there must not unlock a mutating tool.
    #[tokio::test]
    async fn tcp_requests_cannot_confirm_a_mutating_tool() {
        let server = VoiceServer::with_defaults();
        let resp = server
            .process_request_with_trust(
                r#"{"method":"tools/call","params":{"name":"bash_exec","arguments":{"command":"echo no"},"confirmed":true}}"#,
                ConfirmationTrust::Untrusted,
            )
            .await;
        assert_eq!(resp["status"], "denied", "{resp}");
    }

    async fn spawn_webhook(auth: WebhookAuth) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = VoiceServer::with_defaults().with_webhook_auth(auth);
        tokio::spawn(async move {
            let _ = server.serve_webhook_on(listener).await;
        });
        addr
    }

    async fn send_raw(addr: std::net::SocketAddr, parts: &[&[u8]]) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        for part in parts {
            stream.write_all(part).await.unwrap();
            stream.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            stream.read_to_string(&mut response),
        )
        .await
        .expect("response within 10s")
        .unwrap();
        response
    }

    /// Review P1: the old handler allocated `vec![0; Content-Length]` from
    /// the caller's header, so one request could abort the process.
    #[tokio::test]
    async fn webhook_refuses_an_oversized_content_length_before_allocating() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        let response = send_raw(
            addr,
            &[b"POST /voice/tools HTTP/1.1\r\nContent-Length: 9223372036854775807\r\n\r\n"],
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        let health = send_raw(addr, &[b"GET /healthz HTTP/1.1\r\n\r\n"]).await;
        assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    }

    #[tokio::test]
    async fn webhook_requires_the_bearer_secret_except_for_health() {
        let addr = spawn_webhook(WebhookAuth::Bearer("s3cret".into())).await;
        let body = br#"{"tool_name":"system_info","parameters":{"include_env":false}}"#;
        let request = |auth: &str| {
            format!(
                "POST /voice/tools HTTP/1.1\r\n{auth}Content-Length: {}\r\n\r\n{}",
                body.len(),
                std::str::from_utf8(body).unwrap()
            )
        };
        let missing = send_raw(addr, &[request("").as_bytes()]).await;
        assert!(missing.starts_with("HTTP/1.1 401"), "{missing}");
        let wrong = send_raw(
            addr,
            &[request("Authorization: Bearer nope\r\n").as_bytes()],
        )
        .await;
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        let schema = send_raw(addr, &[b"GET /voice/schema HTTP/1.1\r\n\r\n"]).await;
        assert!(schema.starts_with("HTTP/1.1 401"), "{schema}");
        let ok = send_raw(
            addr,
            &[request("authorization: Bearer s3cret\r\n").as_bytes()],
        )
        .await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        let health = send_raw(addr, &[b"GET /healthz HTTP/1.1\r\n\r\n"]).await;
        assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    }

    /// Headers may arrive in several reads; the old handler did one read.
    #[tokio::test]
    async fn webhook_reads_headers_split_across_writes() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        let response = send_raw(addr, &[b"GET /hea", b"lthz HTTP/1.1\r\n", b"\r\n"]).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    /// Refutation review of fbba874: RFC 7235 makes the scheme
    /// case-insensitive; `bearer` and a tab separator got 401.
    #[tokio::test]
    async fn webhook_accepts_the_bearer_scheme_in_any_case() {
        let addr = spawn_webhook(WebhookAuth::Bearer("s3cret".into())).await;
        for auth in [
            "Authorization: bearer s3cret",
            "authorization: BEARER\ts3cret",
        ] {
            let request = format!("GET /voice/schema HTTP/1.1\r\n{auth}\r\n\r\n");
            let response = send_raw(addr, &[request.as_bytes()]).await;
            assert!(response.starts_with("HTTP/1.1 200"), "{auth}: {response}");
        }
    }

    /// Refutation review of fbba874: an unauthenticated health check that
    /// declared a body made the server wait for (and buffer) it.
    #[tokio::test]
    async fn webhook_health_check_does_not_read_a_declared_body() {
        let addr = spawn_webhook(WebhookAuth::Bearer("s3cret".into())).await;
        let response = send_raw(
            addr,
            &[b"GET /healthz HTTP/1.1\r\nContent-Length: 1000\r\n\r\n"],
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    /// Recorded review P3: there was no cap on concurrent connections.
    #[tokio::test]
    async fn webhook_refuses_connections_past_its_cap() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = VoiceServer::with_defaults().with_webhook_auth(WebhookAuth::Unauthenticated);
        tokio::spawn(async move {
            let _ = server.serve_webhook_with_limit(listener, 1).await;
        });

        // Holds the only slot: headers never finish.
        let mut held = TcpStream::connect(addr).await.unwrap();
        held.write_all(b"GET /healthz HTTP/1.1\r\n").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let mut refused = TcpStream::connect(addr).await.unwrap();
        let _ = refused.write_all(b"GET /healthz HTTP/1.1\r\n\r\n").await;
        let mut reply = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            refused.read_to_string(&mut reply),
        )
        .await
        .expect("an over-cap connection is closed at once, not served later");
        assert!(
            read.is_err() || reply.is_empty(),
            "served past the cap: {reply}"
        );

        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let served = send_raw(addr, &[b"GET /healthz HTTP/1.1\r\n\r\n"]).await;
        assert!(served.starts_with("HTTP/1.1 200"), "{served}");
    }

    #[test]
    fn webhook_auth_debug_never_prints_the_secret() {
        let shown = format!("{:?}", WebhookAuth::Bearer("s3cret".into()));
        assert!(!shown.contains("s3cret"), "{shown}");
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
