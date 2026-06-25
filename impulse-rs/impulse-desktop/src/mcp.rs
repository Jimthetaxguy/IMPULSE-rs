//! First-class Rust MCP tools for the Impulse desktop harness.
//!
//! Coding agents that run inside an Impulse-managed PTY see a stable set of
//! built-in tools (descriptors via `crate::runtime::default_builtin_mcp_tools`)
//! and the same names are *executable* here. The tool bodies are first-class
//! Rust: there is no separate rmcp transport, no JSON-RPC daemon, and no
//! child-process indirection for the built-ins. The host command surface
//! (see `crate::host_commands::mcp_invoke`) calls directly into this module,
//! and the Dioxus UI renders the same descriptors and audit trail.
//!
//! Design contract (do not break without ADR):
//! - Tools are deny-by-default. A tool that needs to mutate terminal state
//!   must declare `requires_confirmation: true` in its descriptor AND require
//!   the caller to pass `confirmed: true` at invocation time.
//! - Tool results are `serde_json::Value` so Dioxus can render them directly
//!   and the desktop host can serialize them over the bridge boundary without a custom
//!   transport.
//! - The registry keeps an append-only audit log keyed by agent_id so the
//!   supervisor can review what each terminal-bound agent did.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::{
    default_builtin_mcp_tools, project_notes_hash, AgentRuntimeSnapshot, AgentSpawnRequest,
    AgentWriteRequest, BuiltInMcpTool, DesktopRuntime,
};
use crate::WorkspaceRegistry;

/// Typed error returned by every MCP tool body.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("mcp tool {tool}: missing required argument `{arg}`")]
    MissingArgument { tool: String, arg: String },
    #[error("mcp tool {tool}: argument `{arg}` has wrong type (expected {expected})")]
    ArgumentType {
        tool: String,
        arg: String,
        expected: &'static str,
    },
    #[error("mcp tool {tool}: {message}")]
    Tool { tool: String, message: String },
    #[error("mcp tool {tool}: {message} (denied — caller did not confirm)")]
    ConfirmationRequired { tool: String, message: String },
    #[error("mcp tool {tool}: agent `{agent_id}` not found")]
    UnknownAgent { tool: String, agent_id: String },
    #[error("mcp tool {tool}: workspace `{workspace}` not registered")]
    UnknownWorkspace { tool: String, workspace: String },
    #[error("mcp tool `{name}` is not registered")]
    UnknownTool { name: String },
}

impl McpError {
    pub fn tool(&self) -> &str {
        match self {
            Self::MissingArgument { tool, .. }
            | Self::ArgumentType { tool, .. }
            | Self::Tool { tool, .. }
            | Self::ConfirmationRequired { tool, .. }
            | Self::UnknownAgent { tool, .. }
            | Self::UnknownWorkspace { tool, .. } => tool,
            Self::UnknownTool { name } => name,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::MissingArgument { .. } | Self::ArgumentType { .. } => "argument",
            Self::Tool { .. } => "tool",
            Self::ConfirmationRequired { .. } => "confirmation",
            Self::UnknownAgent { .. }
            | Self::UnknownWorkspace { .. }
            | Self::UnknownTool { .. } => "routing",
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "tool": self.tool(),
            "category": self.category(),
            "message": self.to_string(),
        })
    }
}

/// One tool invocation. `caller_agent_id` identifies the agent that triggered
/// the call (so the audit log can be grouped by terminal session).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpInvocation {
    pub call_id: String,
    pub tool: String,
    pub caller_agent_id: Option<String>,
    pub arguments: Value,
    pub confirmed: bool,
    pub result: Value,
    pub ok: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewQueueStatus {
    #[default]
    Pending,
    Applied,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Apply,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewQueueItem {
    pub id: String,
    pub staged_at_unix_ms: i64,
    #[serde(default)]
    pub status: ReviewQueueStatus,
    #[serde(default)]
    pub decided_at_unix_ms: Option<i64>,
    #[serde(default)]
    pub decision: Option<ReviewDecision>,
    #[serde(default)]
    pub target_agent_id: Option<String>,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ReviewQueueRecord {
    id: String,
    staged_at_unix_ms: i64,
    #[serde(default)]
    status: ReviewQueueStatus,
    #[serde(default)]
    decided_at_unix_ms: Option<i64>,
    #[serde(default)]
    decision: Option<ReviewDecision>,
    #[serde(default)]
    target_agent_id: Option<String>,
    #[serde(default)]
    arguments: Value,
}

impl ReviewQueueRecord {
    fn item(&self, path: &Path) -> ReviewQueueItem {
        ReviewQueueItem {
            id: self.id.clone(),
            staged_at_unix_ms: self.staged_at_unix_ms,
            status: self.status.clone(),
            decided_at_unix_ms: self.decided_at_unix_ms,
            decision: self.decision.clone(),
            target_agent_id: self.target_agent_id.clone(),
            arguments: self.arguments.clone(),
            path: path.display().to_string(),
            preview: review_preview(&self.arguments),
        }
    }
}

impl McpInvocation {
    pub fn new(
        tool: impl Into<String>,
        caller_agent_id: Option<String>,
        arguments: Value,
        confirmed: bool,
    ) -> Self {
        Self {
            call_id: Uuid::new_v4().to_string(),
            tool: tool.into(),
            caller_agent_id,
            arguments,
            confirmed,
            result: Value::Null,
            ok: false,
        }
    }

    pub fn with_result(mut self, ok: bool, result: Value) -> Self {
        self.ok = ok;
        self.result = result;
        self
    }
}

/// Trait every first-class MCP tool implements.
pub trait McpTool: Send + Sync {
    /// The descriptor surfaced to the Dioxus UI.
    fn descriptor(&self) -> &BuiltInMcpTool;

    /// Execute the tool.
    ///
    /// `arguments` is the deserialized JSON arguments the caller passed in.
    /// `confirmed` is the explicit confirmation flag for mutating tools.
    /// `ctx` exposes the desktop runtime and the workspace registry so the
    /// tool body can do real work (spawn, write, search, etc.).
    fn execute(
        &self,
        arguments: &Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError>;
}

/// Shared context passed to every tool invocation. Cloning is cheap because
/// the inner state is `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct McpContext {
    runtime: Arc<DesktopRuntime>,
    workspaces: Arc<WorkspaceRegistry>,
    memory_root: Arc<PathBuf>,
}

impl McpContext {
    pub fn new(
        runtime: Arc<DesktopRuntime>,
        workspaces: Arc<WorkspaceRegistry>,
        memory_root: PathBuf,
    ) -> Self {
        Self {
            runtime,
            workspaces,
            memory_root: Arc::new(memory_root),
        }
    }

    pub fn runtime(&self) -> &Arc<DesktopRuntime> {
        &self.runtime
    }

    pub fn workspaces(&self) -> &Arc<WorkspaceRegistry> {
        &self.workspaces
    }

    pub fn memory_root(&self) -> &Path {
        self.memory_root.as_ref()
    }
}

/// In-process registry. Holds the set of tools and an append-only audit log.
pub struct McpToolRegistry {
    tools: HashMap<String, Arc<dyn McpTool>>,
    audit: Mutex<Vec<McpInvocation>>,
}

impl Default for McpToolRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl McpToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            audit: Mutex::new(Vec::new()),
        }
    }

    /// Build a registry pre-loaded with eight built-ins: the six documented
    /// ones (`impulse.agent_spawn`, `impulse.agent_write`, `impulse.search_memory`,
    /// `impulse.review_injection`, `impulse.review_decision`,
    /// `impulse.project_context`) wired to their real bodies, plus two
    /// read-only helpers (`impulse.list_workspaces`, `impulse.list_agents`).
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for descriptor in default_builtin_mcp_tools() {
            registry.register(Arc::new(PassthroughMcpTool::new(descriptor)));
        }
        registry.register(Arc::new(AgentSpawnTool));
        registry.register(Arc::new(AgentWriteTool));
        registry.register(Arc::new(ListWorkspacesTool));
        registry.register(Arc::new(ListAgentsTool));
        registry.register(Arc::new(ListAgentPlatformsTool));
        registry.register(Arc::new(SearchMemoryTool));
        registry.register(Arc::new(ProjectContextTool));
        registry.register(Arc::new(ReviewInjectionTool));
        registry.register(Arc::new(ReviewDecisionTool));
        registry
    }

    pub fn register(&mut self, tool: Arc<dyn McpTool>) {
        let name = tool.descriptor().name.clone();
        self.tools.insert(name, tool);
    }

    pub fn descriptors(&self) -> Vec<BuiltInMcpTool> {
        let mut descriptors = self
            .tools
            .values()
            .map(|tool| tool.descriptor().clone())
            .collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.name.cmp(&right.name));
        descriptors
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn McpTool>> {
        self.tools.get(name).cloned()
    }

    pub fn invoke(
        &self,
        name: &str,
        caller_agent_id: Option<String>,
        arguments: Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<McpInvocation, McpError> {
        let tool = self
            .tools
            .get(name)
            .cloned()
            .ok_or_else(|| McpError::UnknownTool {
                name: name.to_string(),
            })?;
        let mut invocation = McpInvocation::new(name, caller_agent_id, arguments, confirmed);
        let result = tool.execute(&invocation.arguments, confirmed, ctx);
        let error = match result {
            Ok(value) => {
                invocation = invocation.with_result(true, value);
                None
            }
            Err(error) => {
                invocation = invocation.with_result(false, error.to_json());
                Some(error)
            }
        };
        self.lock_audit().push(invocation.clone());
        if !invocation.ok {
            return Err(error.unwrap_or_else(|| McpError::Tool {
                tool: name.to_string(),
                message: "tool reported failure".to_string(),
            }));
        }
        Ok(invocation)
    }

    pub fn audit_log(&self) -> Vec<McpInvocation> {
        self.lock_audit().clone()
    }

    pub fn audit_for(&self, agent_id: &str) -> Vec<McpInvocation> {
        self.lock_audit()
            .iter()
            .filter(|invocation| invocation.caller_agent_id.as_deref() == Some(agent_id))
            .cloned()
            .collect()
    }

    pub fn clear_audit(&self) {
        self.lock_audit().clear();
    }

    fn lock_audit(&self) -> MutexGuard<'_, Vec<McpInvocation>> {
        self.audit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A descriptor-only tool. The actual `agent_spawn` and `agent_write` tools
/// are replaced at registry construction time with real tool bodies
/// (`AgentSpawnTool`, `AgentWriteTool`) when an `McpContext` is available;
/// this passthrough exists so descriptor-only introspection works without a
/// context (e.g. for the SSR UI render).
pub struct PassthroughMcpTool {
    descriptor: BuiltInMcpTool,
}

impl PassthroughMcpTool {
    pub fn new(descriptor: BuiltInMcpTool) -> Self {
        Self { descriptor }
    }
}

impl McpTool for PassthroughMcpTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        &self.descriptor
    }

    fn execute(
        &self,
        _arguments: &Value,
        _confirmed: bool,
        _ctx: &McpContext,
    ) -> Result<Value, McpError> {
        Err(McpError::Tool {
            tool: self.descriptor.name.clone(),
            message: "descriptor-only passthrough; no real body bound in this registry".to_string(),
        })
    }
}

// ──────────────────────────── Built-in tool bodies ───────────────────────────

/// `impulse.agent_spawn` — start a terminal coding agent in an explicit
/// workspace through the Rust PTY runtime.
pub struct AgentSpawnTool;

impl McpTool for AgentSpawnTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.agent_spawn",
                "Start a terminal coding agent in an explicit workspace through the Rust PTY runtime.",
                vec!["terminal".to_string(), "workspace".to_string()],
                true,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        if !confirmed {
            return Err(McpError::ConfirmationRequired {
                tool: "impulse.agent_spawn".to_string(),
                message: "agent_spawn mutates terminal state".to_string(),
            });
        }
        let mut request: AgentSpawnRequest =
            serde_json::from_value(arguments.clone()).map_err(|error| McpError::Tool {
                tool: "impulse.agent_spawn".to_string(),
                message: format!("invalid AgentSpawnRequest payload: {error}"),
            })?;
        if let Some(workspace) = &request.workspace {
            ctx.workspaces()
                .touch(&workspace.root)
                .map_err(|message| McpError::Tool {
                    tool: "impulse.agent_spawn".to_string(),
                    message: message.to_string(),
                })?;
        }
        // Actually drive from canonical AgentRegistry (not discarded): resolve command via the ops helper
        // so launch is uniformly based on registry descriptors.
        let reg =
            impulse_ops::agent_registry::AgentRegistry::registry_for_runtime().map_err(|e| {
                McpError::Tool {
                    tool: "impulse.agent_spawn".to_string(),
                    message: e.to_string(),
                }
            })?;
        let resolved_cmd = impulse_ops::agent_registry::resolve_launch_command(
            &reg,
            request.platform.as_str(),
            request.command.as_deref(),
        );
        if request.command.as_ref().is_none_or(|c| c.trim().is_empty()) {
            request.command = Some(resolved_cmd);
        }
        let snapshot: AgentRuntimeSnapshot =
            ctx.runtime()
                .spawn_agent(request)
                .map_err(|error| McpError::Tool {
                    tool: "impulse.agent_spawn".to_string(),
                    message: error.to_string(),
                })?;
        Ok(serde_json::to_value(&snapshot).map_err(|e| McpError::Tool {
            tool: "impulse.agent_spawn".to_string(),
            message: e.to_string(),
        })?)
    }
}

/// `impulse.agent_write` — send confirmed input bytes to a running terminal
/// coding agent.
pub struct AgentWriteTool;

impl McpTool for AgentWriteTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.agent_write",
                "Send confirmed input bytes to a running terminal coding agent.",
                vec!["terminal".to_string()],
                true,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        if !confirmed {
            return Err(McpError::ConfirmationRequired {
                tool: "impulse.agent_write".to_string(),
                message: "agent_write mutates terminal state".to_string(),
            });
        }
        let request: AgentWriteRequest =
            serde_json::from_value(arguments.clone()).map_err(|error| McpError::Tool {
                tool: "impulse.agent_write".to_string(),
                message: format!("invalid AgentWriteRequest payload: {error}"),
            })?;
        ctx.runtime()
            .write_agent(request)
            .map_err(|error| McpError::Tool {
                tool: "impulse.agent_write".to_string(),
                message: error.to_string(),
            })?;
        Ok(json!({ "ok": true }))
    }
}

/// `impulse.list_workspaces` — enumerate the registered workspace targets so a
/// coding agent can pick a folder before spawning.
pub struct ListWorkspacesTool;

impl McpTool for ListWorkspacesTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.list_workspaces",
                "List all workspace folders the Impulse runtime currently knows about.",
                vec!["workspace".to_string(), "read_only".to_string()],
                false,
            )
        })
    }

    fn execute(
        &self,
        _arguments: &Value,
        _confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        Ok(serde_json::to_value(ctx.workspaces().list())
            .unwrap_or_else(|_| Value::Array(Vec::new())))
    }
}

/// `impulse.list_agents` — enumerate the live agent runtime snapshots.
pub struct ListAgentsTool;

impl McpTool for ListAgentsTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.list_agents",
                "List the live terminal coding agents the Impulse runtime is supervising.",
                vec!["agents".to_string(), "read_only".to_string()],
                false,
            )
        })
    }

    fn execute(
        &self,
        _arguments: &Value,
        _confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        // Monitoring path also driven by registry: include available platforms report alongside live snapshots.
        use impulse_ops::agent_registry::AgentPlatformsReport;
        let live = ctx.runtime().snapshot_agents();
        let reg =
            impulse_ops::agent_registry::AgentRegistry::registry_for_runtime().map_err(|e| {
                McpError::Tool {
                    tool: "impulse.list_agents".to_string(),
                    message: e.to_string(),
                }
            })?;
        let report = AgentPlatformsReport::from_registry(&reg);
        Ok(serde_json::to_value(serde_json::json!({
            "live_agents": live,
            "available_platforms": report.platforms,
        }))
        .map_err(|e| McpError::Tool {
            tool: "impulse.list_agents".to_string(),
            message: e.to_string(),
        })?)
    }
}

/// `impulse.list_agent_platforms` — list the registered agent CLI types (from canonical
/// impulse-ops AgentRegistry) that can be launched/monitored as terminal agents.
/// This makes multi-agent registration observable via the tool surface.
pub struct ListAgentPlatformsTool;

impl McpTool for ListAgentPlatformsTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.list_agent_platforms",
                "List registered terminal coding agent platforms (claude-code, codex, etc) from the canonical registry.",
                vec!["agents".to_string(), "read_only".to_string()],
                false,
            )
        })
    }

    fn execute(
        &self,
        _arguments: &Value,
        _confirmed: bool,
        _ctx: &McpContext,
    ) -> Result<Value, McpError> {
        use impulse_ops::agent_registry::AgentPlatformsReport;
        let registry =
            impulse_ops::agent_registry::AgentRegistry::registry_for_runtime().map_err(|e| {
                McpError::Tool {
                    tool: "impulse.list_agent_platforms".to_string(),
                    message: e.to_string(),
                }
            })?;
        let report = AgentPlatformsReport::from_registry(&registry);
        // Return structured using the pure report for consistency.
        Ok(
            serde_json::to_value(&report.platforms).map_err(|e| McpError::Tool {
                tool: "impulse.list_agent_platforms".to_string(),
                message: e.to_string(),
            })?,
        )
    }
}

/// `impulse.project_context` — read the operator-authored notes for one
/// registered workspace. Notes are returned only through this explicit
/// read-only tool; spawned terminal agents receive a hash in env, not the raw
/// body.
pub struct ProjectContextTool;

impl McpTool for ProjectContextTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.project_context",
                "Read operator-authored context for a registered project workspace.",
                vec![
                    "workspace".to_string(),
                    "context".to_string(),
                    "read_only".to_string(),
                ],
                false,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        _confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        let root = arguments
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::MissingArgument {
                tool: "impulse.project_context".to_string(),
                arg: "root".to_string(),
            })?;
        let entry = ctx
            .workspaces()
            .lookup(root)
            .ok_or_else(|| McpError::UnknownWorkspace {
                tool: "impulse.project_context".to_string(),
                workspace: root.to_string(),
            })?;
        let notes = entry.target.project_notes.as_ref().map(|body| {
            json!({
                "body": body,
                "body_hash": project_notes_hash(body),
                "source": "operator",
                "warning": "operator-authored context, not terminal instructions",
                "injection_policy": "stage through impulse.review_injection before writing into a terminal",
            })
        });

        Ok(json!({
            "workspace": entry.target,
            "has_notes": notes.is_some(),
            "notes": notes,
        }))
    }
}

/// `impulse.search_memory` — read-only: scan the local `.impulse/HISTORY.jsonl`
/// for prior session lines matching the query. Real, on-disk, in-process — not
/// a mock. Returns the matching lines with a small metadata envelope.
pub struct SearchMemoryTool;

impl McpTool for SearchMemoryTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.search_memory",
                "Search Impulse memory and session history for context before agent action.",
                vec!["memory".to_string(), "read_only".to_string()],
                false,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        _confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::MissingArgument {
                tool: "impulse.search_memory".to_string(),
                arg: "query".to_string(),
            })?
            .to_ascii_lowercase();
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .min(200) as usize;
        let history_path = ctx.memory_root().join("HISTORY.jsonl");
        let mut matches = Vec::new();
        if history_path.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&history_path) {
                for line in contents.lines() {
                    if line.to_ascii_lowercase().contains(&query) {
                        matches.push(line.to_string());
                        if matches.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }
        Ok(json!({
            "query": query,
            "history_path": history_path.display().to_string(),
            "scanned_present": history_path.is_file(),
            "matches": matches,
        }))
    }
}

/// `impulse.review_injection` — stage a payload for review rather than
/// injecting it directly. Real bodies: writes the staged payload to a
/// review-queue file under `memory_root/review_queue/<id>.json` and returns
/// the path + id. The Dioxus UI / supervisor then either approves
/// (returning a McpInvocation with `ok: true`) or discards. The "review
/// before apply" rule is enforced by the tool always being `requires_confirmation: true`.
pub struct ReviewInjectionTool;

impl McpTool for ReviewInjectionTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.review_injection",
                "Stage retrieved context for review before injecting it into an agent terminal.",
                vec!["context".to_string(), "review".to_string()],
                true,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        if !confirmed {
            return Err(McpError::ConfirmationRequired {
                tool: "impulse.review_injection".to_string(),
                message: "review_injection stages payload to disk; user must confirm".to_string(),
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        let record = ReviewQueueRecord {
            id: id.clone(),
            staged_at_unix_ms: current_unix_ms(),
            status: ReviewQueueStatus::Pending,
            decided_at_unix_ms: None,
            decision: None,
            target_agent_id: arguments
                .get("target_agent_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            arguments: arguments.clone(),
        };
        let path = write_review_record(ctx.memory_root(), &record)?;
        Ok(json!({
            "id": id,
            "path": path.display().to_string(),
            "preview": review_preview(arguments),
            "staged": true,
        }))
    }
}

/// `impulse.review_decision` — apply or skip one staged review payload. This
/// is intentionally a mutating MCP tool so every decision produces an audit
/// row through the same registry path as the staging action.
pub struct ReviewDecisionTool;

impl McpTool for ReviewDecisionTool {
    fn descriptor(&self) -> &BuiltInMcpTool {
        static DESCRIPTOR: std::sync::OnceLock<BuiltInMcpTool> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| {
            BuiltInMcpTool::new(
                "impulse.review_decision",
                "Apply or skip a staged review payload with an audit receipt.",
                vec![
                    "context".to_string(),
                    "review".to_string(),
                    "terminal".to_string(),
                ],
                true,
            )
        })
    }

    fn execute(
        &self,
        arguments: &Value,
        confirmed: bool,
        ctx: &McpContext,
    ) -> Result<Value, McpError> {
        if !confirmed {
            return Err(McpError::ConfirmationRequired {
                tool: "impulse.review_decision".to_string(),
                message: "review_decision can write to a terminal or close a staged review"
                    .to_string(),
            });
        }
        let id = arguments.get("id").and_then(Value::as_str).ok_or_else(|| {
            McpError::MissingArgument {
                tool: "impulse.review_decision".to_string(),
                arg: "id".to_string(),
            }
        })?;
        let decision_value = arguments
            .get("decision")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::MissingArgument {
                tool: "impulse.review_decision".to_string(),
                arg: "decision".to_string(),
            })?;
        let decision = match decision_value {
            "apply" => ReviewDecision::Apply,
            "skip" => ReviewDecision::Skip,
            _ => {
                return Err(McpError::ArgumentType {
                    tool: "impulse.review_decision".to_string(),
                    arg: "decision".to_string(),
                    expected: "apply|skip",
                });
            }
        };

        let path = review_queue_path(ctx.memory_root(), id)?;
        let mut record = read_review_record(&path)?;
        if record.status != ReviewQueueStatus::Pending {
            return Err(McpError::Tool {
                tool: "impulse.review_decision".to_string(),
                message: format!("review item `{id}` is already {:?}", record.status),
            });
        }

        if decision == ReviewDecision::Apply {
            let target_agent_id = arguments
                .get("target_agent_id")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| record.target_agent_id.clone())
                .ok_or_else(|| McpError::MissingArgument {
                    tool: "impulse.review_decision".to_string(),
                    arg: "target_agent_id".to_string(),
                })?;
            let data = review_payload_bytes(&record.arguments)?;
            ctx.runtime()
                .write_agent(AgentWriteRequest {
                    agent_id: target_agent_id.clone(),
                    data,
                })
                .map_err(|error| McpError::Tool {
                    tool: "impulse.review_decision".to_string(),
                    message: error.to_string(),
                })?;
            record.target_agent_id = Some(target_agent_id);
            record.status = ReviewQueueStatus::Applied;
        } else {
            record.status = ReviewQueueStatus::Skipped;
        }
        record.decision = Some(decision);
        record.decided_at_unix_ms = Some(current_unix_ms());
        write_review_record(ctx.memory_root(), &record)?;

        Ok(serde_json::to_value(record.item(&path)).unwrap_or(Value::Null))
    }
}

pub fn list_review_queue(memory_root: &Path) -> Result<Vec<ReviewQueueItem>, McpError> {
    let queue_dir = review_queue_dir(memory_root);
    if !queue_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let entries = std::fs::read_dir(&queue_dir).map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to read review queue: {error}"),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| McpError::Tool {
            tool: "impulse.review_queue".to_string(),
            message: format!("failed to read review queue entry: {error}"),
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let record = read_review_record(&path)?;
        items.push(record.item(&path));
    }
    items.sort_by(|left, right| {
        right
            .staged_at_unix_ms
            .cmp(&left.staged_at_unix_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(items)
}

fn review_queue_dir(memory_root: &Path) -> PathBuf {
    memory_root.join("review_queue")
}

fn review_queue_path(memory_root: &Path, id: &str) -> Result<PathBuf, McpError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(McpError::ArgumentType {
            tool: "impulse.review_decision".to_string(),
            arg: "id".to_string(),
            expected: "safe review item id",
        });
    }
    Ok(review_queue_dir(memory_root).join(format!("{id}.json")))
}

fn read_review_record(path: &Path) -> Result<ReviewQueueRecord, McpError> {
    let bytes = std::fs::read(path).map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to read `{}`: {error}", path.display()),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to decode `{}`: {error}", path.display()),
    })
}

fn write_review_record(
    memory_root: &Path,
    record: &ReviewQueueRecord,
) -> Result<PathBuf, McpError> {
    let queue_dir = review_queue_dir(memory_root);
    std::fs::create_dir_all(&queue_dir).map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to create review_queue dir: {error}"),
    })?;
    let path = review_queue_path(memory_root, &record.id)?;
    let temp_path = path.with_extension("json.tmp");
    std::fs::write(
        &temp_path,
        serde_json::to_vec_pretty(record).unwrap_or_default(),
    )
    .map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to write staged payload: {error}"),
    })?;
    std::fs::rename(&temp_path, &path).map_err(|error| McpError::Tool {
        tool: "impulse.review_queue".to_string(),
        message: format!("failed to publish staged payload: {error}"),
    })?;
    Ok(path)
}

fn review_payload_bytes(arguments: &Value) -> Result<Vec<u8>, McpError> {
    if let Some(content) = arguments.get("content").and_then(Value::as_str) {
        return Ok(content.as_bytes().to_vec());
    }
    if let Some(data) = arguments.get("data").and_then(Value::as_str) {
        return Ok(data.as_bytes().to_vec());
    }
    if let Some(data) = arguments.get("data").and_then(Value::as_array) {
        let mut bytes = Vec::with_capacity(data.len());
        for value in data {
            let Some(byte) = value.as_u64().and_then(|value| u8::try_from(value).ok()) else {
                return Err(McpError::ArgumentType {
                    tool: "impulse.review_decision".to_string(),
                    arg: "data".to_string(),
                    expected: "byte array",
                });
            };
            bytes.push(byte);
        }
        return Ok(bytes);
    }
    Err(McpError::MissingArgument {
        tool: "impulse.review_decision".to_string(),
        arg: "content".to_string(),
    })
}

fn review_preview(arguments: &Value) -> String {
    let raw = arguments
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("data").and_then(Value::as_str))
        .map(ToString::to_string)
        .unwrap_or_else(|| serde_json::to_string(arguments).unwrap_or_default());
    let mut preview = raw.replace('\n', "\\n");
    if preview.len() > 160 {
        preview.truncate(157);
        preview.push_str("...");
    }
    preview
}

fn current_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{AgentPlatformKind, WorkspaceTarget};
    use crate::workspace::WorkspaceRegistry;

    fn ctx() -> McpContext {
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
        let memory_root = std::env::temp_dir().join("impulse-desktop-mcp-test");
        std::fs::create_dir_all(&memory_root).ok();
        McpContext::new(runtime, workspaces, memory_root)
    }

    #[test]
    fn test_default_registry_has_nine_unique_builtins() {
        let registry = McpToolRegistry::with_builtins();
        let names: Vec<String> = registry.descriptors().into_iter().map(|d| d.name).collect();
        assert!(names.contains(&"impulse.agent_spawn".to_string()));
        assert!(names.contains(&"impulse.agent_write".to_string()));
        assert!(names.contains(&"impulse.search_memory".to_string()));
        assert!(names.contains(&"impulse.review_injection".to_string()));
        assert!(names.contains(&"impulse.review_decision".to_string()));
        assert!(names.contains(&"impulse.project_context".to_string()));
        assert!(names.contains(&"impulse.list_workspaces".to_string()));
        assert!(names.contains(&"impulse.list_agents".to_string()));
        // The new list_agent_platforms wires the canonical ops AgentRegistry so
        // agent CLI types (claude, codex, ...) are observable/launchable.
        assert!(names.contains(&"impulse.list_agent_platforms".to_string()));
        // Nine unique names now.
        assert_eq!(names.len(), 9, "registry names were: {names:?}");
    }

    #[test]
    fn test_review_injection_real_body_overrides_passthrough() {
        let registry = McpToolRegistry::with_builtins();
        let tool = registry
            .get("impulse.review_injection")
            .expect("review_injection is registered");
        // ReviewInjectionTool type-asserts by name uniqueness: the registry
        // returns the real body (not the passthrough) when both share a name.
        assert!(tool.descriptor().requires_confirmation);
    }

    #[test]
    fn test_default_registry_unknown_tool_returns_error() {
        let registry = McpToolRegistry::with_builtins();
        let context = ctx();
        let result = registry.invoke("impulse.does_not_exist", None, json!({}), false, &context);
        assert!(matches!(result, Err(McpError::UnknownTool { .. })));
    }

    #[test]
    fn test_mutating_tool_requires_confirmation() {
        let registry = McpToolRegistry::with_builtins();
        let context = ctx();
        let result = registry.invoke("impulse.list_workspaces", None, json!({}), false, &context);
        // read-only tool, no confirmation required, should succeed
        assert!(result.is_ok());

        let result = registry.invoke("impulse.agent_spawn", None, json!({}), false, &context);
        // mutating tool, unconfirmed: should error
        assert!(matches!(result, Err(McpError::ConfirmationRequired { .. })));
    }

    #[test]
    fn test_review_injection_writes_staged_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
        let context = McpContext::new(runtime, workspaces, dir.path().to_path_buf());
        let invocation = McpToolRegistry::with_builtins()
            .invoke(
                "impulse.review_injection",
                Some("alpha".to_string()),
                json!({ "content": "stage me for review" }),
                true,
                &context,
            )
            .expect("review_invoke ok");
        let id = invocation
            .result
            .get("id")
            .and_then(Value::as_str)
            .expect("id field")
            .to_string();
        let path = invocation
            .result
            .get("path")
            .and_then(Value::as_str)
            .expect("path field");
        assert!(std::path::Path::new(path).is_file());
        assert!(!id.is_empty());
        let queue = list_review_queue(dir.path()).expect("list queue");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, id);
        assert_eq!(queue[0].status, ReviewQueueStatus::Pending);
        assert_eq!(queue[0].preview, "stage me for review");
    }

    #[test]
    fn test_review_decision_skip_updates_queue_and_rejects_double_decision() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
        let context = McpContext::new(runtime, workspaces, dir.path().to_path_buf());
        let registry = McpToolRegistry::with_builtins();
        let staged = registry
            .invoke(
                "impulse.review_injection",
                Some("alpha".to_string()),
                json!({ "content": "skip this payload" }),
                true,
                &context,
            )
            .expect("review_invoke ok");
        let id = staged.result["id"].as_str().expect("id").to_string();

        let skipped = registry
            .invoke(
                "impulse.review_decision",
                Some("alpha".to_string()),
                json!({ "id": id, "decision": "skip" }),
                true,
                &context,
            )
            .expect("skip review item");

        assert!(skipped.ok);
        let queue = list_review_queue(dir.path()).expect("list queue");
        assert_eq!(queue[0].status, ReviewQueueStatus::Skipped);
        assert_eq!(queue[0].decision, Some(ReviewDecision::Skip));

        let second = registry.invoke(
            "impulse.review_decision",
            Some("alpha".to_string()),
            json!({ "id": queue[0].id, "decision": "skip" }),
            true,
            &context,
        );
        assert!(matches!(second, Err(McpError::Tool { .. })));
    }

    #[test]
    fn test_project_context_returns_notes_with_provenance() {
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::empty());
        workspaces
            .register(WorkspaceTarget {
                root: "/tmp".to_string(),
                label: Some("scratch".to_string()),
                purpose: Some("safe harness workspace".to_string()),
                project_notes: Some("remember workspace-specific build notes".to_string()),
            })
            .expect("register workspace");
        let context = McpContext::new(runtime, workspaces, dir.path().to_path_buf());
        let invocation = McpToolRegistry::with_builtins()
            .invoke(
                "impulse.project_context",
                Some("alpha".to_string()),
                json!({ "root": "/tmp" }),
                false,
                &context,
            )
            .expect("project_context ok");

        assert!(invocation.ok);
        assert_eq!(invocation.result["has_notes"], true);
        assert_eq!(invocation.result["notes"]["source"], "operator");
        assert_eq!(
            invocation.result["notes"]["warning"],
            "operator-authored context, not terminal instructions"
        );
        assert!(invocation.result["notes"]["body_hash"]
            .as_str()
            .expect("hash string")
            .starts_with("fnv1a64:"));
    }

    #[test]
    fn test_audit_log_captures_invocations() {
        let registry = McpToolRegistry::with_builtins();
        let context = ctx();
        let _ = registry.invoke(
            "impulse.list_workspaces",
            Some("alpha".to_string()),
            json!({}),
            false,
            &context,
        );
        let _ = registry.invoke(
            "impulse.list_agents",
            Some("beta".to_string()),
            json!({}),
            false,
            &context,
        );
        let log = registry.audit_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].caller_agent_id.as_deref(), Some("alpha"));
        assert!(log[0].ok);
    }

    #[test]
    fn test_descriptors_are_sorted_and_unique() {
        let registry = McpToolRegistry::with_builtins();
        let descriptors = registry.descriptors();
        let names: Vec<String> = descriptors.iter().map(|d| d.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn test_search_memory_reads_history_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let history = dir.path().join("HISTORY.jsonl");
        std::fs::write(
            &history,
            "{\"kind\":\"session_start\",\"agent\":\"alpha\"}\n{\"kind\":\"commit\",\"sha\":\"abc\"}\n",
        )
        .expect("write history");
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
        let context = McpContext::new(runtime, workspaces, dir.path().to_path_buf());
        let result = McpToolRegistry::with_builtins()
            .invoke(
                "impulse.search_memory",
                None,
                json!({ "query": "session_start", "limit": 10 }),
                false,
                &context,
            )
            .expect("search_memory should succeed");
        assert!(result.ok);
        let matches = result
            .result
            .get("matches")
            .and_then(Value::as_array)
            .expect("matches array");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_workspace_helpers_are_registered() {
        let registry = McpToolRegistry::with_builtins();
        assert!(registry.get("impulse.list_workspaces").is_some());
        assert!(registry.get("impulse.list_agents").is_some());
    }

    #[test]
    fn test_default_platform_kind_serializes_back() {
        // Sanity: ensure the runtime's AgentPlatformKind can survive a JSON
        // round-trip through the mcp boundary.
        let kind = AgentPlatformKind::Shell;
        let value = serde_json::to_value(kind).expect("serialize");
        let back: AgentPlatformKind = serde_json::from_value(value).expect("deserialize");
        assert_eq!(kind, back);
    }

    #[tokio::test]
    async fn test_mcp_list_agent_platforms_execute_includes_claude_codex() {
        // Exercises the execute body of ListAgentPlatformsTool (and by registry central) with real types.
        use crate::host_bridge::channel_event_sink;
        let (sink, _rx) = channel_event_sink();
        let runtime = Arc::new(DesktopRuntime::builder().with_event_sink(sink).build());
        let workspaces = Arc::new(WorkspaceRegistry::empty());
        let mcp_reg = Arc::new(McpToolRegistry::with_builtins());
        let mem = std::env::temp_dir().join("impulse-mcp-test");
        let ctx = McpContext::new(runtime, workspaces, mem);
        let tool = ListAgentPlatformsTool;
        let val = tool
            .execute(&serde_json::Value::Null, false, &ctx)
            .expect("platforms execute");
        let arr = val.as_array().expect("platforms array");
        let ids: Vec<&str> = arr
            .iter()
            .filter_map(|v| v.get("id").and_then(|i| i.as_str()))
            .collect();
        assert!(
            ids.iter().any(|&i| i == "claude-code"),
            "missing claude-code: {ids:?}"
        );
        assert!(ids.iter().any(|&i| i == "codex"), "missing codex: {ids:?}");
    }
}
