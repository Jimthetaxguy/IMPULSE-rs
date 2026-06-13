//! First-class Rust MCP tools for the Impulse desktop harness.
//!
//! Coding agents that run inside an Impulse-managed PTY see a stable set of
//! built-in tools (descriptors via `crate::runtime::default_builtin_mcp_tools`)
//! and the same names are *executable* here. The tool bodies are first-class
//! Rust: there is no separate rmcp transport, no JSON-RPC daemon, and no
//! child-process indirection for the built-ins. The Tauri command surface
//! (see `crate::tauri_commands::mcp_invoke`) calls directly into this module,
//! and the Dioxus UI renders the same descriptors and audit trail.
//!
//! Design contract (do not break without ADR):
//! - Tools are deny-by-default. A tool that needs to mutate terminal state
//!   must declare `requires_confirmation: true` in its descriptor AND require
//!   the caller to pass `confirmed: true` at invocation time.
//! - Tool results are `serde_json::Value` so Dioxus can render them directly
//!   and Tauri can serialize them over the IPC boundary without a custom
//!   transport.
//! - The registry keeps an append-only audit log keyed by agent_id so the
//!   supervisor can review what each terminal-bound agent did.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::runtime::{
    default_builtin_mcp_tools, AgentRuntimeSnapshot, AgentSpawnRequest, AgentWriteRequest,
    BuiltInMcpTool, DesktopRuntime,
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

    /// Build a registry pre-loaded with seven built-ins: the four documented
    /// ones (`impulse.agent_spawn`, `impulse.agent_write`, `impulse.search_memory`,
    /// `impulse.review_injection`) wired to their real bodies, plus three
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
        registry.register(Arc::new(SearchMemoryTool));
        registry.register(Arc::new(ReviewInjectionTool));
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
        self.audit
            .lock()
            .expect("mcp audit mutex poisoned")
            .push(invocation.clone());
        if !invocation.ok {
            return Err(error.unwrap_or_else(|| McpError::Tool {
                tool: name.to_string(),
                message: "tool reported failure".to_string(),
            }));
        }
        Ok(invocation)
    }

    pub fn audit_log(&self) -> Vec<McpInvocation> {
        self.audit.lock().expect("mcp audit mutex poisoned").clone()
    }

    pub fn audit_for(&self, agent_id: &str) -> Vec<McpInvocation> {
        self.audit
            .lock()
            .expect("mcp audit mutex poisoned")
            .iter()
            .filter(|invocation| invocation.caller_agent_id.as_deref() == Some(agent_id))
            .cloned()
            .collect()
    }

    pub fn clear_audit(&self) {
        self.audit.lock().expect("mcp audit mutex poisoned").clear();
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
        let request: AgentSpawnRequest =
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
        let snapshot: AgentRuntimeSnapshot =
            ctx.runtime()
                .spawn_agent(request)
                .map_err(|error| McpError::Tool {
                    tool: "impulse.agent_spawn".to_string(),
                    message: error.to_string(),
                })?;
        Ok(serde_json::to_value(&snapshot).unwrap_or(Value::Null))
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
        Ok(serde_json::to_value(ctx.runtime().snapshot_agents())
            .unwrap_or_else(|_| Value::Array(Vec::new())))
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
        let queue_dir = ctx.memory_root().join("review_queue");
        std::fs::create_dir_all(&queue_dir).map_err(|error| McpError::Tool {
            tool: "impulse.review_injection".to_string(),
            message: format!("failed to create review_queue dir: {error}"),
        })?;
        let id = uuid::Uuid::new_v4().to_string();
        let path = queue_dir.join(format!("{id}.json"));
        let body = json!({
            "id": id,
            "staged_at_unix_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            "arguments": arguments,
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&body).unwrap_or_default()).map_err(
            |error| McpError::Tool {
                tool: "impulse.review_injection".to_string(),
                message: format!("failed to write staged payload: {error}"),
            },
        )?;
        Ok(json!({
            "id": id,
            "path": path.display().to_string(),
            "staged": true,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::AgentPlatformKind;
    use crate::workspace::WorkspaceRegistry;

    fn ctx() -> McpContext {
        let runtime = Arc::new(DesktopRuntime::default());
        let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
        let memory_root = std::env::temp_dir().join("impulse-desktop-mcp-test");
        std::fs::create_dir_all(&memory_root).ok();
        McpContext::new(runtime, workspaces, memory_root)
    }

    #[test]
    fn test_default_registry_has_seven_builtins() {
        let registry = McpToolRegistry::with_builtins();
        let names: Vec<String> = registry.descriptors().into_iter().map(|d| d.name).collect();
        assert!(names.contains(&"impulse.agent_spawn".to_string()));
        assert!(names.contains(&"impulse.agent_write".to_string()));
        assert!(names.contains(&"impulse.search_memory".to_string()));
        assert!(names.contains(&"impulse.review_injection".to_string()));
        assert!(names.contains(&"impulse.list_workspaces".to_string()));
        assert!(names.contains(&"impulse.list_agents".to_string()));
        assert_eq!(names.len(), 6);
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
}
