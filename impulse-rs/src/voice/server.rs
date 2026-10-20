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

/// How long a webhook client has to finish its headers, within
/// `WEBHOOK_REQUEST_TIMEOUT`. Authentication is read from the headers, so
/// with only the request budget a client with no secret could hold a
/// connection slot for 30 s by never finishing them, and reconnect to hold it
/// again.
const WEBHOOK_HEADER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long an authenticated client has to finish its body, within
/// `WEBHOOK_REQUEST_TIMEOUT`. With only the request budget, a client that
/// stopped partway through its body held a connection slot for 30 s.
const WEBHOOK_BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
        self.serve_webhook_with_limit(
            listener,
            MAX_WEBHOOK_CONNECTIONS,
            WEBHOOK_HEADER_TIMEOUT,
            WEBHOOK_BODY_TIMEOUT,
        )
        .await
    }

    /// [`Self::serve_webhook_on`] with the connection cap passed in. A
    /// connection over the cap is closed at once instead of queued, so a
    /// client holding connections open cannot pin more than the cap.
    async fn serve_webhook_with_limit(
        &self,
        listener: TcpListener,
        max_connections: usize,
        header_timeout: std::time::Duration,
        body_timeout: std::time::Duration,
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
                    handle_http_connection(stream, bridge, auth, header_timeout, body_timeout),
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

/// What reading a request's header block came to.
enum HeaderBlock {
    /// The headers end at this offset in the buffer.
    End(usize),
    /// The peer closed the connection first.
    Closed,
    /// More than `MAX_WEBHOOK_HEADER_BYTES` arrived without an end.
    TooLarge,
    /// A line feed without a carriage return before it. Split there, a line
    /// hid a header from a proxy that requires CRLF; a client sending only
    /// LF used to wait out the header deadline with no reply.
    BareLineFeed,
}

/// Reads into `raw` until the end of the header block, which may arrive in
/// pieces. Each pass looks only at what arrived since the last one (from a
/// few bytes back, so a terminator split across reads is still found);
/// searching the whole buffer every time cost quadratic time in the size of
/// a header block sent in small pieces.
async fn read_header_block(stream: &mut TcpStream, raw: &mut Vec<u8>) -> Result<HeaderBlock> {
    let mut chunk = vec![0u8; 8 * 1024];
    let mut scanned: usize = 0;
    loop {
        let from = scanned.saturating_sub(3);
        let end = raw[from..]
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|position| from + position);
        let head = end.unwrap_or(raw.len());
        let bare_line_feed = (scanned.saturating_sub(1)..head)
            .any(|i| raw[i] == b'\n' && (i == 0 || raw[i - 1] != b'\r'));
        if bare_line_feed {
            return Ok(HeaderBlock::BareLineFeed);
        }
        if let Some(end) = end {
            return Ok(HeaderBlock::End(end));
        }
        if raw.len() > MAX_WEBHOOK_HEADER_BYTES {
            return Ok(HeaderBlock::TooLarge);
        }
        scanned = raw.len();
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(HeaderBlock::Closed);
        }
        raw.extend_from_slice(&chunk[..n]);
    }
}

async fn handle_http_connection(
    mut stream: TcpStream,
    bridge: Arc<VoiceToolBridge>,
    auth: WebhookAuth,
    header_timeout: std::time::Duration,
    body_timeout: std::time::Duration,
) -> Result<()> {
    let mut raw = Vec::with_capacity(8 * 1024);
    let header_end = match tokio::time::timeout(
        header_timeout,
        read_header_block(&mut stream, &mut raw),
    )
    .await
    {
        Ok(read) => match read? {
            HeaderBlock::End(position) => position,
            HeaderBlock::Closed => return Ok(()),
            HeaderBlock::TooLarge => {
                return reply_and_close(
                    &mut stream,
                    431,
                    "application/json",
                    br#"{"error":"request headers too large"}"#,
                )
                .await
            }
            HeaderBlock::BareLineFeed => {
                return reply_and_close(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"a bare line feed in the request head; lines end with CRLF"}"#,
                )
                .await
            }
        },
        // Nothing is answered: the client has not authenticated yet.
        Err(_) => return Ok(()),
    };
    // Read as text. A header block that is not valid UTF-8 used to become
    // empty here, which skipped every header check and told a correctly
    // authenticated client its secret was wrong.
    let Ok(header_text) = std::str::from_utf8(&raw[..header_end]) else {
        return reply_and_close(
            &mut stream,
            400,
            "application/json",
            br#"{"error":"request headers are not valid UTF-8"}"#,
        )
        .await;
    };
    if let Some((status, problem)) = malformed_header(header_text) {
        return reply_and_close(&mut stream, status, "application/json", problem).await;
    }
    let body = &raw[header_end + 4..];

    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    // Routes match the path alone: a provider or probe adding a query
    // string (`/healthz?probe=1`) was refused or sent to the wrong route.
    let target = parts.next().unwrap_or("/");
    let path = target.split_once('?').map_or(target, |(path, _)| path);

    let is_health = method == "GET" && matches!(path, "/healthz" | "/health");
    if !is_health && !auth.admits(header_text) {
        return reply_and_close(
            &mut stream,
            401,
            "application/json",
            br#"{"error":"missing or wrong Authorization bearer secret"}"#,
        )
        .await;
    }

    let body_owned = match tokio::time::timeout(
        body_timeout,
        read_body(&mut stream, header_text, body, is_health),
    )
    .await
    {
        Ok(read) => match read? {
            BodyRead::Complete(bytes) => bytes,
            BodyRead::Refused(status, payload) => {
                return reply_and_close(&mut stream, status, "application/json", payload).await
            }
        },
        Err(_) => {
            return reply_and_close(
                &mut stream,
                408,
                "application/json",
                br#"{"error":"request body not received in time"}"#,
            )
            .await
        }
    };

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

    reply_and_close(&mut stream, status, content_type, &payload).await
}

/// The longest chunk-size or trailer line read, extensions included.
const MAX_CHUNK_LINE: usize = 4 * 1024;

/// Encoded bytes a chunked body may take, framing included: twice the body
/// limit, so many tiny chunks cannot make the server read without bound.
const MAX_CHUNKED_READ: usize = 2 * MAX_REQUEST_SIZE;

/// What reading a chunked request body came to.
enum ChunkedBody {
    Complete(Vec<u8>),
    /// The decoded body passed `MAX_REQUEST_SIZE`, or its encoding passed
    /// `MAX_CHUNKED_READ`.
    TooLarge,
    /// Not valid chunked encoding, or the peer closed before the end.
    Malformed,
}

/// Request and header lines RFC 9112 says a server must refuse with 400,
/// each of which a proxy in front could frame differently from this server:
/// a request line that is not exactly `METHOD SP target SP HTTP/...`, a
/// control character other than tab (a bare CR inside a line hid a second
/// header from this server but not from a proxy that breaks lines there), a
/// folded line, a line without a colon, a field name that is not a token,
/// and a `Content-Length` that is not plain digits or that repeats with a
/// different value. They used to be skipped or read leniently. No request
/// could be smuggled through this server, since every connection closes
/// after one reply, so this is defense in depth.
fn malformed_header(header_text: &str) -> Option<(u16, &'static [u8])> {
    let mut lines = header_text.lines();
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split(' ').collect();
    if parts.len() != 3
        || parts.iter().any(|part| part.is_empty())
        || !parts[2].starts_with("HTTP/")
    {
        return Some((400, br#"{"error":"malformed request line"}"#));
    }
    // Only HTTP/1.0 and HTTP/1.1 are served; `HTTP/9.9` used to be read as
    // either.
    if !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") {
        return Some((
            505,
            br#"{"error":"only HTTP/1.0 and HTTP/1.1 are supported"}"#,
        ));
    }
    if header_text
        .lines()
        .any(|line| line.chars().any(|c| c.is_ascii_control() && c != '\t'))
    {
        return Some((
            400,
            br#"{"error":"control characters in the request head"}"#,
        ));
    }
    let mut length: Option<&str> = None;
    for line in lines {
        if line.starts_with([' ', '\t']) {
            return Some((400, br#"{"error":"folded header lines are not accepted"}"#));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Some((400, br#"{"error":"header line without a colon"}"#));
        };
        if name.is_empty() || !name.chars().all(is_token_char) {
            return Some((400, br#"{"error":"malformed header field name"}"#));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Some((400, br#"{"error":"Content-Length must be digits"}"#));
            }
            if length.is_some_and(|earlier| earlier != value) {
                return Some((400, br#"{"error":"conflicting Content-Length values"}"#));
            }
            length = Some(value);
        }
    }
    None
}

/// A character RFC 9110 allows in a field name (`tchar`).
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// How a request frames its body.
enum BodyFraming {
    /// No `Transfer-Encoding`: `Content-Length` bytes, or what came with the
    /// headers.
    Length,
    Chunked,
    /// Refused with this status and JSON body.
    Refused(u16, &'static [u8]),
}

/// RFC 9112 section 6: a request with both `Transfer-Encoding` and
/// `Content-Length` is refused rather than guessed at, and so is any coding
/// but `chunked`, which this server cannot undo (`gzip, chunked` was
/// de-chunked and handed on still compressed).
fn body_framing(header_text: &str) -> BodyFraming {
    let mut has_encoding = false;
    let mut has_length = false;
    let mut codings = Vec::new();
    for line in header_text.lines().skip(1) {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("transfer-encoding:") {
            has_encoding = true;
            codings.extend(
                value
                    .split(',')
                    .map(|coding| coding.trim().to_string())
                    .filter(|coding| !coding.is_empty()),
            );
        } else if lower.starts_with("content-length:") {
            has_length = true;
        }
    }
    if !has_encoding {
        BodyFraming::Length
    } else if has_length {
        BodyFraming::Refused(
            400,
            br#"{"error":"Transfer-Encoding and Content-Length together"}"#,
        )
    } else if codings == ["chunked"] {
        BodyFraming::Chunked
    } else {
        BodyFraming::Refused(
            501,
            br#"{"error":"only the chunked transfer coding is supported"}"#,
        )
    }
}

/// What reading a request body came to.
enum BodyRead {
    Complete(Vec<u8>),
    /// Refused with this status and JSON body; nothing more is read.
    Refused(u16, &'static [u8]),
}

/// Reads the body after the headers; `buffered` is what arrived with them.
async fn read_body(
    stream: &mut TcpStream,
    header_text: &str,
    buffered: &[u8],
    is_health: bool,
) -> Result<BodyRead> {
    // A health check needs no body, and it is unauthenticated, so reading a
    // declared one let any client make the server buffer up to the limit.
    if is_health {
        return Ok(BodyRead::Complete(Vec::new()));
    }
    match body_framing(header_text) {
        BodyFraming::Refused(status, payload) => Ok(BodyRead::Refused(status, payload)),
        BodyFraming::Chunked => Ok(match read_chunked_body(stream, buffered).await? {
            ChunkedBody::Complete(decoded) => BodyRead::Complete(decoded),
            ChunkedBody::TooLarge => {
                BodyRead::Refused(413, br#"{"error":"request body too large"}"#)
            }
            ChunkedBody::Malformed => {
                BodyRead::Refused(400, br#"{"error":"malformed chunked request body"}"#)
            }
        }),
        BodyFraming::Length => {
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(buffered.len());
            // Refuse before allocating: a caller-chosen length is never trusted.
            if content_length > MAX_REQUEST_SIZE {
                return Ok(BodyRead::Refused(
                    413,
                    br#"{"error":"request body too large"}"#,
                ));
            }
            let mut body = buffered.to_vec();
            let mut chunk = vec![0u8; 8 * 1024];
            while body.len() < content_length {
                let n = stream.read(&mut chunk).await?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            body.truncate(content_length);
            Ok(BodyRead::Complete(body))
        }
    }
}

/// Reads a chunked request body (RFC 9112, section 7.1). `buffered` is what
/// arrived with the headers; chunk extensions and trailer fields are read
/// and dropped. Without this, a chunked request, which carries no
/// `Content-Length`, was parsed from its raw framing and answered 400.
async fn read_chunked_body(stream: &mut TcpStream, buffered: &[u8]) -> Result<ChunkedBody> {
    let mut raw = buffered.to_vec();
    let mut read = raw.len();
    let mut body = Vec::new();
    let mut scratch = vec![0u8; 8 * 1024];
    let mut in_trailers = false;
    loop {
        // The next line: a chunk size, or a trailer field after the last chunk.
        let line_end = loop {
            if let Some(at) = raw.windows(2).position(|pair| pair == b"\r\n") {
                break at;
            }
            if raw.len() > MAX_CHUNK_LINE {
                return Ok(ChunkedBody::Malformed);
            }
            let n = stream.read(&mut scratch).await?;
            if n == 0 {
                return Ok(ChunkedBody::Malformed);
            }
            read += n;
            if read > MAX_CHUNKED_READ {
                return Ok(ChunkedBody::TooLarge);
            }
            raw.extend_from_slice(&scratch[..n]);
        };
        if line_end > MAX_CHUNK_LINE {
            return Ok(ChunkedBody::Malformed);
        }
        let line = String::from_utf8_lossy(&raw[..line_end]).into_owned();
        raw.drain(..line_end + 2);
        if in_trailers {
            if line.is_empty() {
                return Ok(ChunkedBody::Complete(body));
            }
            continue;
        }
        let size_text = line.split(';').next().unwrap_or("").trim();
        if size_text.is_empty() || !size_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(ChunkedBody::Malformed);
        }
        let Ok(size) = usize::from_str_radix(size_text, 16) else {
            return Ok(ChunkedBody::TooLarge);
        };
        if size == 0 {
            in_trailers = true;
            continue;
        }
        if size > MAX_REQUEST_SIZE - body.len() {
            return Ok(ChunkedBody::TooLarge);
        }
        while raw.len() < size + 2 {
            let n = stream.read(&mut scratch).await?;
            if n == 0 {
                return Ok(ChunkedBody::Malformed);
            }
            read += n;
            if read > MAX_CHUNKED_READ {
                return Ok(ChunkedBody::TooLarge);
            }
            raw.extend_from_slice(&scratch[..n]);
        }
        if &raw[size..size + 2] != b"\r\n" {
            return Ok(ChunkedBody::Malformed);
        }
        body.extend_from_slice(&raw[..size]);
        raw.drain(..size + 2);
    }
}

/// How long a closing connection may keep sending, and how much of it is
/// read and dropped meanwhile; see [`reply_and_close`].
const WEBHOOK_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const WEBHOOK_DRAIN_BYTES: usize = 1024 * 1024;

/// Writes the reply and closes the connection. A request whose body was not
/// read (a 401, a 413, a health check that declared one) leaves bytes
/// unread, and closing with unread bytes makes the kernel send a reset,
/// which can discard the reply before the client reads it. So the write
/// side is shut first, and what the client still sends is read and dropped,
/// within `WEBHOOK_DRAIN_TIMEOUT` and `WEBHOOK_DRAIN_BYTES`.
async fn reply_and_close(
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
        408 => "Request Timeout",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        505 => "HTTP Version Not Supported",
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
    let _ = stream.shutdown().await;
    let mut sink = [0u8; 8 * 1024];
    let mut drained = 0;
    let _ = tokio::time::timeout(WEBHOOK_DRAIN_TIMEOUT, async {
        while drained < WEBHOOK_DRAIN_BYTES {
            match stream.read(&mut sink).await {
                Ok(0) | Err(_) => break,
                Ok(n) => drained += n,
            }
        }
    })
    .await;
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

    /// Recorded review item: a refused request's body was left unread, and
    /// closing with unread bytes made the kernel send a reset, which could
    /// discard the 401 before the client read it.
    #[tokio::test]
    async fn webhook_refusal_closes_cleanly_while_the_body_is_unread() {
        let addr = spawn_webhook(WebhookAuth::Bearer("s3cret".into())).await;
        let body = vec![b'x'; 64 * 1024];
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let head = format!(
            "POST /voice/tool HTTP/1.1\r\nAuthorization: Bearer wrong\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        stream.shutdown().await.unwrap();
        let mut reply = Vec::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            stream.read_to_end(&mut reply),
        )
        .await
        .expect("a reply within 10 s");
        let reply = String::from_utf8_lossy(&reply);
        assert!(
            read.is_ok(),
            "the connection was reset: {read:?}, after {reply:?}"
        );
        assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
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

    /// Round 3 on ba9c873: the header read had the whole 30 s request
    /// budget, and auth comes from the headers, so a client with no secret
    /// that never finished them, and reconnected, held every slot.
    #[tokio::test]
    async fn webhook_closes_a_client_that_does_not_finish_its_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = VoiceServer::with_defaults();
        tokio::spawn(async move {
            let _ = server
                .serve_webhook_with_limit(
                    listener,
                    1,
                    std::time::Duration::from_millis(200),
                    WEBHOOK_BODY_TIMEOUT,
                )
                .await;
        });

        let started = std::time::Instant::now();
        let mut slow = TcpStream::connect(addr).await.unwrap();
        slow.write_all(b"POST /tool HTTP/1.1\r\n").await.unwrap();
        let mut buf = Vec::new();
        let closed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            slow.read_to_end(&mut buf),
        )
        .await
        .expect("the server closes a client whose headers never finish");
        assert!(closed.is_err() || buf.is_empty(), "answered: {buf:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));

        // The slot is free again for the next client.
        let served = send_raw(addr, &[b"GET /healthz HTTP/1.1\r\n\r\n"]).await;
        assert!(served.starts_with("HTTP/1.1 200"), "{served}");
    }

    /// Round 4 (reviewer B): only the header read had its own deadline, so a
    /// client that stopped partway through its body held a slot for 30 s.
    #[tokio::test]
    async fn webhook_answers_408_when_the_body_stops_arriving() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = VoiceServer::with_defaults().with_webhook_auth(WebhookAuth::Unauthenticated);
        tokio::spawn(async move {
            let _ = server
                .serve_webhook_with_limit(
                    listener,
                    1,
                    WEBHOOK_HEADER_TIMEOUT,
                    std::time::Duration::from_millis(200),
                )
                .await;
        });
        let mut stalled = TcpStream::connect(addr).await.unwrap();
        stalled
            .write_all(b"POST /voice/tools HTTP/1.1\r\nContent-Length: 100\r\n\r\n{\"tool")
            .await
            .unwrap();
        let mut reply = Vec::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stalled.read_to_end(&mut reply),
        )
        .await
        .expect("answered before the 30 s request timeout");
        let reply = String::from_utf8_lossy(&reply);
        assert!(reply.starts_with("HTTP/1.1 408"), "{read:?} {reply}");
    }

    /// Round 4 (reviewer B): Transfer-Encoding with Content-Length was
    /// accepted, and `gzip, chunked` was de-chunked and handed on compressed.
    #[tokio::test]
    async fn webhook_refuses_ambiguous_or_unsupported_body_framing() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        let both = send_raw(
            addr,
            &[b"POST /voice/tools HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 5\r\n\r\n0\r\n\r\n"],
        )
        .await;
        assert!(both.starts_with("HTTP/1.1 400"), "{both}");
        let gzip = send_raw(
            addr,
            &[b"POST /voice/tools HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n"],
        )
        .await;
        assert!(gzip.starts_with("HTTP/1.1 501"), "{gzip}");
    }

    /// Round 5 (reviewer B): header shapes RFC 9112 says to refuse were
    /// skipped or read leniently, each framed differently by a proxy.
    #[tokio::test]
    async fn webhook_refuses_malformed_header_lines() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        // Each carries a valid tool call that lenient parsing served with 200,
        // so only the header check can turn it into a 400.
        let body = r#"{"tool_name":"system_info","parameters":{"include_env":false}}"#;
        let n = body.len();
        for headers in [
            format!("Transfer-Encoding : chunked\r\nContent-Length: {n}"),
            format!("Content-Length: {n}\r\n\tX-Folded: yes"),
            format!("Content-Length: {n}\r\nContent-Length: {}", n + 5),
            format!("Content-Length: +{n}"),
            format!("Content-Length: {n}\r\nno colon here"),
            // Round 6 (reviewer B): a bare CR inside a line, a NUL, and a
            // field name that is not a token were still accepted.
            format!("Content-Length: {n}\r\nX-A: b\rTransfer-Encoding: chunked"),
            format!("Content-Length: {n}\r\nX-A: b\0c"),
            format!("Content-Length: {n}\r\nX(A): b"),
        ] {
            let request = format!("POST /voice/tools HTTP/1.1\r\n{headers}\r\n\r\n{body}");
            let response = send_raw(addr, &[request.as_bytes()]).await;
            assert!(
                response.starts_with("HTTP/1.1 400"),
                "{headers:?}: {response}"
            );
        }
        // Round 7 (reviewer B): a bare line feed split a line where a proxy
        // requiring CRLF would not.
        let request = format!(
            "POST /voice/tools HTTP/1.1\r\nX-A: b\nX-B: c\r\nContent-Length: {n}\r\n\r\n{body}"
        );
        let response = send_raw(addr, &[request.as_bytes()]).await;
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");

        // Round 6 (reviewer B): a loose request line, and a header block
        // that is not UTF-8, which skipped every check.
        for request_line in [
            "POST  /voice/tools  HTTP/1.1",
            " POST /voice/tools HTTP/1.1",
        ] {
            let request = format!("{request_line}\r\nContent-Length: {n}\r\n\r\n{body}");
            let response = send_raw(addr, &[request.as_bytes()]).await;
            assert!(
                response.starts_with("HTTP/1.1 400"),
                "{request_line:?}: {response}"
            );
        }
        let mut latin1 = b"POST /voice/tools HTTP/1.1\r\nUser-Agent: caf\xe9\r\n".to_vec();
        latin1.extend_from_slice(format!("Content-Length: {n}\r\n\r\n{body}").as_bytes());
        let response = send_raw(addr, &[&latin1]).await;
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(response.contains("not valid UTF-8"), "{response}");

        // A repeated Content-Length with the same value is still accepted.
        let repeated = format!(
            "POST /voice/tools HTTP/1.1\r\nContent-Length: {0}\r\nContent-Length: {0}\r\n\r\n{body}",
            body.len()
        );
        let response = send_raw(addr, &[repeated.as_bytes()]).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    /// Round 7 (reviewer B): a client sending only line feeds waited out
    /// the header deadline with no reply; `HTTP/9.9` was served; a query
    /// string kept a request from its route.
    #[tokio::test]
    async fn webhook_answers_lf_only_requests_versions_and_query_strings() {
        let addr = spawn_webhook(WebhookAuth::Bearer("s3cret".into())).await;
        let started = std::time::Instant::now();
        let lf_only = send_raw(addr, &[b"GET /healthz HTTP/1.1\n\n"]).await;
        assert!(lf_only.starts_with("HTTP/1.1 400"), "{lf_only}");
        assert!(started.elapsed() < std::time::Duration::from_secs(4));

        let version = send_raw(addr, &[b"GET /healthz HTTP/9.9\r\n\r\n"]).await;
        assert!(version.starts_with("HTTP/1.1 505"), "{version}");

        let probe = send_raw(addr, &[b"GET /healthz?probe=1 HTTP/1.1\r\n\r\n"]).await;
        assert!(probe.starts_with("HTTP/1.1 200"), "{probe}");
    }

    /// Recorded review item: a chunked body, which carries no
    /// Content-Length, was parsed from its raw framing and answered 400.
    #[tokio::test]
    async fn webhook_reads_a_chunked_body() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        let body = r#"{"tool_name":"system_info","parameters":{"include_env":false}}"#;
        let (first, second) = body.split_at(20);
        let request = format!(
            "POST /voice/tools HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{:x};note=1\r\n{first}\r\n{:x}\r\n{second}\r\n0\r\nX-Trailer: yes\r\n\r\n",
            first.len(),
            second.len()
        );
        let (head, rest) = request.split_at(request.len() / 2);
        let response = send_raw(addr, &[head.as_bytes(), rest.as_bytes()]).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    #[tokio::test]
    async fn webhook_refuses_a_malformed_or_oversized_chunked_body() {
        let addr = spawn_webhook(WebhookAuth::Unauthenticated).await;
        let malformed = send_raw(
            addr,
            &[b"POST /voice/tools HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n{}\r\n0\r\n\r\n"],
        )
        .await;
        assert!(malformed.starts_with("HTTP/1.1 400"), "{malformed}");
        assert!(malformed.contains("malformed chunked"), "{malformed}");
        let oversized = send_raw(
            addr,
            &[b"POST /voice/tools HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nffffffffff\r\n"],
        )
        .await;
        assert!(oversized.starts_with("HTTP/1.1 413"), "{oversized}");
    }

    /// Recorded review P3: there was no cap on concurrent connections.
    #[tokio::test]
    async fn webhook_refuses_connections_past_its_cap() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = VoiceServer::with_defaults().with_webhook_auth(WebhookAuth::Unauthenticated);
        tokio::spawn(async move {
            let _ = server
                .serve_webhook_with_limit(listener, 1, WEBHOOK_HEADER_TIMEOUT, WEBHOOK_BODY_TIMEOUT)
                .await;
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
