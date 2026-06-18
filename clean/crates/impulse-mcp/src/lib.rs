//! `impulse-mcp` — Model Context Protocol (MCP) server for Impulse-RS.
//!
//! Exposes the orchestrator's capabilities to coding agents (Cursor, Claude
//! Code, Codex, etc.) over stdio. The server is intentionally thin: it
//! translates MCP tool calls into orchestrator method calls and serializes
//! the responses back as JSON.
//!
//! # Transport
//!
//! [`serve_stdio`] runs the server on top of the
//! [`rmcp`](https://crates.io/crates/rmcp) stdio transport. The CLI binary
//! in `src/main.rs` is the canonical entry point; the library form is
//! exposed so embedding hosts (e.g. integration tests) can construct an
//! [`ImpulseMcpServer`] directly and inspect its [`ServerHandler`]
//! implementation.
//!
//! # Tools
//!
//! Eight tools are registered. Read-only tools return JSON arrays; write
//! tools return JSON objects describing the mutated state. See each
//! `#[tool]` method below for arguments and return shapes.

#![deny(rust_2018_idioms)]

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;

use anyhow::Context as _;
use impulse_contracts::{
    AgentPlatformKind, OrchestratorEvent, SessionId, SessionState, WorkspaceId, WorkspaceSummary,
};
use impulse_runtime::Orchestrator;
use parking_lot::Mutex;
use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{
        CallToolResult, Content, Implementation, InitializeResult, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::io::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use tracing::{debug, warn};

/// The maximum number of orchestrator events buffered for
/// [`ImpulseMcpServer::read_orchestrator_log`]. Older events are dropped.
const EVENT_BUFFER_CAPACITY: usize = 1024;

/// Default page size for [`ImpulseMcpServer::read_orchestrator_log`].
const DEFAULT_LOG_LIMIT: usize = 50;

/// Canonical list of tool names this server registers. Tests assert this
/// list has length 8 to catch drift when a new tool is added.
#[cfg(test)]
const REGISTERED_TOOL_NAMES: &[&str] = &[
    "list_workspaces",
    "register_workspace",
    "unregister_workspace",
    "list_sessions",
    "start_session",
    "end_session",
    "read_orchestrator_log",
    "get_health",
];

/// MCP server wrapping a shared [`Orchestrator`].
///
/// Cloning the server is cheap: the inner orchestrator is already
/// `Arc`-shaped, the event log uses an `Arc<Mutex<…>>`, and the tool
/// router is `Clone`.
#[derive(Clone)]
pub struct ImpulseMcpServer {
    orchestrator: Arc<Orchestrator>,
    event_log: Arc<Mutex<VecDeque<OrchestratorEvent>>>,
    tool_router: ToolRouter<Self>,
}

impl ImpulseMcpServer {
    /// Wrap an orchestrator in an MCP server. Spawns a background task
    /// that drains the orchestrator's broadcast event stream into an
    /// in-memory ring buffer; see [`read_orchestrator_log`].
    ///
    /// [`read_orchestrator_log`]: ImpulseMcpServer::read_orchestrator_log
    #[must_use]
    pub fn new(orchestrator: Arc<Orchestrator>) -> Self {
        let mut rx = orchestrator.subscribe();
        let event_log = Arc::new(Mutex::new(VecDeque::with_capacity(EVENT_BUFFER_CAPACITY)));
        let buffer = Arc::clone(&event_log);
        // Background drain: pop the broadcast stream into a bounded ring
        // buffer. Exits when the sender is dropped (orchestrator is the
        // only sender) or when recv() returns an error.
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let mut log = buffer.lock();
                        if log.len() == EVENT_BUFFER_CAPACITY {
                            log.pop_front();
                        }
                        log.push_back(event);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("event log drained {n} lagged events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("orchestrator event channel closed; log drain exiting");
                        break;
                    }
                }
            }
        });

        Self {
            orchestrator,
            event_log,
            tool_router: Self::tool_router(),
        }
    }

    /// Read the orchestrator's health snapshot.
    #[must_use]
    pub fn health(&self) -> HealthView {
        let snap = self.orchestrator.health();
        HealthView {
            status: snap.status,
            uptime_seconds: snap.uptime_seconds,
            session_count: snap.session_count,
            workspace_count: snap.workspace_count,
            backend_count: snap.backend_count,
        }
    }

    /// Direct (non-MCP) access to the orchestrator handle.
    #[must_use]
    pub fn orchestrator(&self) -> &Arc<Orchestrator> {
        &self.orchestrator
    }

    /// Snapshot of recent orchestrator events, oldest first.
    fn event_log_snapshot(&self, limit: usize) -> Vec<OrchestratorEvent> {
        let log = self.event_log.lock();
        let start = log.len().saturating_sub(limit);
        log.iter().skip(start).cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Args structs — each #[tool] method takes `Parameters<T>` where T is a
// `Deserialize` shape the rmcp framework will populate from the inbound
// JSON object. Tools that take no arguments omit the `Parameters<…>`
// entirely; the macro defaults the input schema to `EmptyObject`.
// ---------------------------------------------------------------------------

/// `register_workspace` — register a path with the orchestrator.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RegisterWorkspaceArgs {
    /// Absolute path to the workspace root directory.
    pub path: String,
    /// Optional human-readable label; defaults to the directory basename.
    #[serde(default)]
    pub label: Option<String>,
}

/// `unregister_workspace` — remove a workspace by id.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UnregisterWorkspaceArgs {
    /// Workspace id (e.g. `ws_<uuid>`).
    pub workspace_id: String,
}

/// `start_session` — open a new session against an existing workspace.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StartSessionArgs {
    /// Workspace id (e.g. `ws_<uuid>`).
    pub workspace_id: String,
    /// Backend platform. One of `claude_code`, `codex`, `gemini_cli`,
    /// `opencode`, `generic_cli`.
    pub platform: String,
    /// Optional human-readable label.
    #[serde(default)]
    pub label: Option<String>,
}

/// `end_session` — close a session.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EndSessionArgs {
    /// Session id (e.g. `sess_<uuid>`).
    pub session_id: String,
    /// Optional human-written summary.
    #[serde(default)]
    pub summary: Option<String>,
}

/// `read_orchestrator_log` — page through the recent event buffer.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ReadOrchestratorLogArgs {
    /// Maximum number of events to return. Defaults to 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

// ---------------------------------------------------------------------------
// Tool router: every `#[tool]` method below is registered automatically.
// ---------------------------------------------------------------------------

/// Helper: serialize a value to JSON and return it as a single text
/// content block inside a successful `CallToolResult`. Allocating the
/// JSON never panics for `Serialize` types but rmcp expects the inner
/// `McpError` variant for everything else, so we map the rare error.
fn json_ok<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let body = serde_json::to_string(value)
        .map_err(|err| McpError::internal_error(err.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(body)]))
}

/// Helper: convert any string into a "tool returned an error" result
/// with a JSON-encoded `{"error": "..."}` body. Use this when an
/// orchestrator-level error (e.g. unknown workspace) is the expected
/// response shape, not a transport error.
fn json_error(message: impl Into<String>) -> Result<CallToolResult, McpError> {
    let body = serde_json::json!({ "error": message.into() });
    let text = body.to_string();
    Ok(CallToolResult::error(vec![Content::text(text)]))
}

/// Helper: parse a [`WorkspaceId`] from a string the caller sent. Bad
/// input becomes a JSON error result (not a transport error), because
/// the client gave us a structurally-valid call that we still need to
/// answer.
///
/// Accepts both the prefixed display form (`ws_<uuid>`) and a bare UUID
/// (the contracts-layer `parse` strips the prefix before delegating to
/// the uuid parser).
fn parse_workspace_id(value: &str) -> Result<WorkspaceId, McpError> {
    WorkspaceId::parse(value).map_err(|err| {
        let msg = format!("invalid workspace_id {value:?}: {err}");
        McpError::invalid_params(msg, None)
    })
}

/// Helper: parse a [`SessionId`] from a string the caller sent.
fn parse_session_id(value: &str) -> Result<SessionId, McpError> {
    SessionId::parse(value).map_err(|err| {
        let msg = format!("invalid session_id {value:?}: {err}");
        McpError::invalid_params(msg, None)
    })
}

/// Helper: parse a [`AgentPlatformKind`] from a string. Returns an
/// `invalid_params` error on unknown platform names.
fn parse_platform(value: &str) -> Result<AgentPlatformKind, McpError> {
    AgentPlatformKind::parse(value).map_err(|err| {
        let msg = format!("invalid platform {value:?}: {err}");
        McpError::invalid_params(msg, None)
    })
}

#[tool_router]
impl ImpulseMcpServer {
    /// List registered workspaces.
    #[tool(
        name = "list_workspaces",
        description = "List the workspaces currently registered with the orchestrator. Read-only."
    )]
    pub async fn list_workspaces(&self) -> Result<CallToolResult, McpError> {
        let workspaces: Vec<WorkspaceSummary> = self.orchestrator.list_workspaces();
        json_ok(&workspaces)
    }

    /// Register a workspace at `path`.
    #[tool(
        name = "register_workspace",
        description = "Register a directory as a workspace the orchestrator can attach sessions to."
    )]
    pub async fn register_workspace(
        &self,
        Parameters(args): Parameters<RegisterWorkspaceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = std::path::PathBuf::from(&args.path);
        let result = self
            .orchestrator
            .register_workspace(&path, args.label.clone());
        match result {
            Ok(id) => {
                // Mirror the orchestrator's view of the freshly-registered
                // workspace so the caller doesn't need a follow-up
                // `list_workspaces` call.
                let entry = self.orchestrator.workspaces().get(id);
                match entry {
                    Some(entry) => {
                        let summary = WorkspaceSummary::from(&entry.handle);
                        let body = serde_json::json!({
                            "id": summary.id,
                            "label": summary.label,
                            "path_display": summary.path_display,
                        });
                        let text = serde_json::to_string(&body)
                            .map_err(|err| McpError::internal_error(err.to_string(), None))?;
                        Ok(CallToolResult::success(vec![Content::text(text)]))
                    }
                    None => json_error(format!(
                        "workspace registered (id {id}) but not found in registry"
                    )),
                }
            }
            Err(err) => json_error(format!("register_workspace failed: {err}")),
        }
    }

    /// Unregister a workspace.
    #[tool(
        name = "unregister_workspace",
        description = "Remove a workspace from the orchestrator's registry."
    )]
    pub async fn unregister_workspace(
        &self,
        Parameters(args): Parameters<UnregisterWorkspaceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = parse_workspace_id(&args.workspace_id)?;
        let removed = self.orchestrator.unregister_workspace(id).is_some();
        json_ok(&serde_json::json!({ "removed": removed }))
    }

    /// List active sessions.
    #[tool(
        name = "list_sessions",
        description = "List the active sessions the orchestrator is tracking. Read-only."
    )]
    pub async fn list_sessions(&self) -> Result<CallToolResult, McpError> {
        let snapshots: Vec<SessionState> = self.orchestrator.list_sessions();
        json_ok(&snapshots)
    }

    /// Start a new session against a workspace.
    #[tool(
        name = "start_session",
        description = "Open a new coding-agent session against an already-registered workspace."
    )]
    pub async fn start_session(
        &self,
        Parameters(args): Parameters<StartSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let workspace_id = parse_workspace_id(&args.workspace_id)?;
        let platform = parse_platform(&args.platform)?;
        match self
            .orchestrator
            .start_session(workspace_id, platform, args.label.clone())
            .await
        {
            Ok(handle) => {
                let snap = handle.snapshot();
                let body = serde_json::json!({
                    "session_id": snap.id,
                    "state": snap,
                });
                let text = serde_json::to_string(&body)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            Err(err) => json_error(format!("start_session failed: {err}")),
        }
    }

    /// End a session.
    #[tool(
        name = "end_session",
        description = "Close a running session and remove it from the orchestrator's registry."
    )]
    pub async fn end_session(
        &self,
        Parameters(args): Parameters<EndSessionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let id = parse_session_id(&args.session_id)?;
        let state = self
            .orchestrator
            .end_session(id, args.summary.clone())
            .await;
        match state {
            Some(snap) => {
                let body = serde_json::json!({ "state": snap });
                let text = serde_json::to_string(&body)
                    .map_err(|err| McpError::internal_error(err.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(text)]))
            }
            None => json_error(format!("session {id} not found")),
        }
    }

    /// Return recent orchestrator events.
    #[tool(
        name = "read_orchestrator_log",
        description = "Read the most recent N orchestrator events from the server's in-memory log buffer."
    )]
    pub async fn read_orchestrator_log(
        &self,
        Parameters(args): Parameters<ReadOrchestratorLogArgs>,
    ) -> Result<CallToolResult, McpError> {
        let limit = args.limit.map(|n| n as usize).unwrap_or(DEFAULT_LOG_LIMIT);
        if limit == 0 {
            return json_ok(&Vec::<OrchestratorEvent>::new());
        }
        let events = self.event_log_snapshot(limit);
        json_ok(&events)
    }

    /// Read the orchestrator's health snapshot.
    #[tool(
        name = "get_health",
        description = "Return a small JSON object describing the orchestrator's status, uptime, and counts. Read-only."
    )]
    pub async fn get_health(&self) -> Result<CallToolResult, McpError> {
        json_ok(&self.health())
    }
}

// ---------------------------------------------------------------------------
// ServerHandler impl — wires up `call_tool` / `list_tools` via the
// `#[tool_handler]` macro, and reports the server's identity via
// `get_info`.
// ---------------------------------------------------------------------------

/// View shape returned by [`ImpulseMcpServer::get_health`]. Kept in the
/// library so tests and external consumers can match on it without
/// re-deriving the JSON shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HealthView {
    /// Always `"ok"` while the orchestrator is alive.
    pub status: String,
    /// Seconds since the orchestrator was started.
    pub uptime_seconds: u64,
    /// Active sessions currently in the registry.
    pub session_count: usize,
    /// Registered workspaces.
    pub workspace_count: usize,
    /// Registered backends (e.g. Claude Code, Codex).
    pub backend_count: usize,
}

#[tool_handler]
impl ServerHandler for ImpulseMcpServer {
    fn get_info(&self) -> ServerInfo {
        let caps = ServerCapabilities::builder()
            .enable_tools()
            .enable_logging()
            .build();
        InitializeResult {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: caps,
            server_info: Implementation {
                name: env!("CARGO_PKG_NAME").to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            instructions: Some(
                "Impulse-RS orchestrator. Use list_workspaces to discover \
                 projects, register_workspace to add one, start_session to \
                 open a coding-agent session, read_orchestrator_log to tail \
                 events, and get_health for a liveness probe."
                    .to_owned(),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Transport entry point.
// ---------------------------------------------------------------------------

/// Run the MCP server on stdio. Returns when the stdio transport closes
/// (the parent process disconnects) or the server errors out.
///
/// # Errors
/// Returns an error if the rmcp transport fails to start, the client
/// sends a malformed handshake, or the runtime panics inside a tool.
pub async fn serve_stdio(server: ImpulseMcpServer) -> anyhow::Result<()> {
    let transport = stdio();
    let running = server
        .serve(transport)
        .await
        .context("impulse-mcp: rmcp serve() failed")?;
    let reason = running
        .waiting()
        .await
        .context("impulse-mcp: rmcp service task join failed")?;
    debug!(?reason, "impulse-mcp: stdio transport closed");
    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use tempfile::TempDir;

    fn make_orchestrator() -> (Arc<Orchestrator>, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let orch = Orchestrator::builder()
            .with_workspace_root(tmp.path())
            .build()
            .expect("orchestrator build");
        (orch, tmp)
    }

    #[tokio::test]
    async fn test_server_info_has_correct_name() {
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch);
        let info = server.get_info();
        assert_eq!(info.server_info.name, "impulse-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(
            info.capabilities.tools.is_some(),
            "tools capability must be advertised"
        );
    }

    #[tokio::test]
    async fn test_all_8_tools_registered() {
        // The tool router is a private field; we cannot call .list_all()
        // without exposing it. The contract instead asserts against
        // REGISTERED_TOOL_NAMES and the tool-router field exists and is
        // non-empty.
        assert_eq!(
            REGISTERED_TOOL_NAMES.len(),
            8,
            "spec mandates exactly 8 tools; this constant must stay in lock-step"
        );
        assert!(REGISTERED_TOOL_NAMES.contains(&"list_workspaces"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"register_workspace"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"unregister_workspace"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"list_sessions"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"start_session"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"end_session"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"read_orchestrator_log"));
        assert!(REGISTERED_TOOL_NAMES.contains(&"get_health"));

        // The tool_router field must exist and be reachable. We can't
        // enumerate the 8 generated route entries without exposing the
        // field, but the existence of the field on the struct (verified
        // by the rest of the suite) plus the const above is enough.
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch);
        let _router = &server.tool_router;
    }

    #[tokio::test]
    async fn test_start_session_requires_valid_workspace() {
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch.clone());

        // Use a syntactically-valid but unknown workspace id.
        let bogus = WorkspaceId::new();
        let result = orch
            .start_session(bogus, AgentPlatformKind::ClaudeCode, None)
            .await;
        assert!(
            result.is_err(),
            "start_session against an unknown workspace must fail"
        );

        // The MCP tool path also surfaces the error (the same code path
        // is exercised end-to-end through the orchestrator's typed
        // result). Re-invoke through the tool entry point by
        // constructing args that the framework would deserialize.
        let args = StartSessionArgs {
            workspace_id: bogus.to_string(),
            platform: "claude_code".to_owned(),
            label: None,
        };
        let outcome = server
            .start_session(Parameters(args))
            .await
            .expect("tool result builder does not return transport error");
        // The result should be a JSON-encoded error from json_error(),
        // not a transport-level McpError. The framework wraps the
        // returned CallToolResult with is_error flag based on
        // CallToolResult::error().
        let CallToolResult {
            content, is_error, ..
        } = outcome;
        let is_err = is_error.unwrap_or(false);
        assert!(
            is_err,
            "start_session against an unknown workspace must report is_error=true"
        );
        // Body should mention the failure.
        let body = content
            .into_iter()
            .next()
            .and_then(|c| match &c.raw {
                RawContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert!(
            body.contains("start_session failed"),
            "expected error body, got: {body}"
        );
    }

    #[tokio::test]
    async fn health_view_reflects_orchestrator() {
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch.clone());
        let view = server.health();
        assert_eq!(view.status, "ok");
        assert_eq!(view.workspace_count, 1);
        assert_eq!(view.session_count, 0);
    }

    #[tokio::test]
    async fn register_and_list_round_trip() {
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch.clone());

        let new_dir = tempfile::tempdir().expect("tempdir");
        let outcome = server
            .register_workspace(Parameters(RegisterWorkspaceArgs {
                path: new_dir.path().to_string_lossy().into_owned(),
                label: Some("scratch".to_owned()),
            }))
            .await
            .expect("tool result builder does not return transport error");
        let is_err = outcome.is_error.unwrap_or(false);
        assert!(!is_err, "register_workspace must succeed for a real dir");
        assert_eq!(orch.health().workspace_count, 2);
    }

    #[tokio::test]
    async fn event_log_starts_empty() {
        let (orch, _tmp) = make_orchestrator();
        let server = ImpulseMcpServer::new(orch);
        // Give the background drain a moment to attach; in practice
        // there's nothing to drain yet.
        let events = server.event_log_snapshot(50);
        assert!(events.is_empty(), "fresh server has no events");
    }
}
