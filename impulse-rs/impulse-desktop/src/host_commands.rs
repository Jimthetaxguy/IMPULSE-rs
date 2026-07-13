use crate::bridge::{
    TerminalBridge, TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest,
    TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};
use crate::mcp::{list_review_queue, McpContext, McpInvocation, McpToolRegistry, ReviewDecision};
use crate::native::{DefaultNativeIslandHost, NativeIslandRequest, NativeIslandResult};
use crate::runtime::{
    AgentRuntimeSnapshot, AgentSpawnRequest, AgentWriteRequest, DesktopRuntime,
    SupervisorLocalActionRequest, WorkspaceTarget,
};
use crate::workspace::WorkspaceRegistry;
use crate::NativeIslandHost;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Status string the manifest-only Dioxus host bootstrap publishes before the
/// live eval bridge swaps in working `invoke`/`listen` implementations. The
/// host-adapter resolver in `ui.rs` treats a host carrying this status — or
/// whose `invoke`/`listen` are still the rejecting `__impulseHostPending`
/// stubs — as NOT ready, so the ops + terminal bridges fail closed (degrade or
/// fall back to legacy Tauri) instead of advertising a live bridge over stubs
/// that reject on the first call.
pub const PENDING_HOST_BOOTSTRAP_STATUS: &str = "manifest-only-pending-dioxus-eval-bridge";

pub const AGENT_CLOSE_COMMAND: &str = "agent_close";
pub const AGENT_FOCUS_COMMAND: &str = "agent_focus";
pub const AGENT_PLATFORMS_COMMAND: &str = "agent_platforms";
pub const AGENT_RESIZE_COMMAND: &str = "agent_resize";
pub const AGENT_SNAPSHOT_COMMAND: &str = "agent_snapshot";
pub const AGENT_SPAWN_COMMAND: &str = "agent_spawn";
pub const AGENT_WRITE_COMMAND: &str = "agent_write";
pub const LIST_WORKSPACES_COMMAND: &str = "list_workspaces";
pub const MCP_DESCRIPTORS_COMMAND: &str = "mcp_descriptors";
pub const MCP_INVOKE_COMMAND: &str = "mcp_invoke";
pub const NATIVE_ISLAND_REQUEST_COMMAND: &str = "native_island_request";
pub const REGISTER_WORKSPACE_COMMAND: &str = "register_workspace";
pub const REVIEW_DECISION_COMMAND: &str = "review_decision";
pub const REVIEW_QUEUE_COMMAND: &str = "review_queue";
pub const SUPERVISOR_LOCAL_ACTION_COMMAND: &str = "supervisor_local_action";
pub const TERMINAL_CLOSE_COMMAND: &str = "terminal_close";
pub const TERMINAL_FOCUS_COMMAND: &str = "terminal_focus";
pub const TERMINAL_OPEN_COMMAND: &str = "terminal_open";
pub const TERMINAL_RESIZE_COMMAND: &str = "terminal_resize";
pub const TERMINAL_WRITE_COMMAND: &str = "terminal_write";

pub const HOST_INVOKE_COMMANDS: &[&str] = &[
    AGENT_CLOSE_COMMAND,
    AGENT_FOCUS_COMMAND,
    AGENT_PLATFORMS_COMMAND,
    AGENT_RESIZE_COMMAND,
    AGENT_SNAPSHOT_COMMAND,
    AGENT_SPAWN_COMMAND,
    AGENT_WRITE_COMMAND,
    LIST_WORKSPACES_COMMAND,
    MCP_DESCRIPTORS_COMMAND,
    MCP_INVOKE_COMMAND,
    NATIVE_ISLAND_REQUEST_COMMAND,
    REGISTER_WORKSPACE_COMMAND,
    REVIEW_DECISION_COMMAND,
    REVIEW_QUEUE_COMMAND,
    SUPERVISOR_LOCAL_ACTION_COMMAND,
    TERMINAL_CLOSE_COMMAND,
    TERMINAL_FOCUS_COMMAND,
    TERMINAL_OPEN_COMMAND,
    TERMINAL_RESIZE_COMMAND,
    TERMINAL_WRITE_COMMAND,
];

/// Small pure helper ratchet (autoresearch-style dedup): converts any
/// Display error into the `String` surface required by the host command
/// facade. Eliminates 18+ identical closures while preserving identical
/// semantics for Tauri + dioxus + tests.
fn err_to_string<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_spawn(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_spawn_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_spawn(
    runtime: &DesktopRuntime,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_spawn_inner(runtime, request)
}

fn agent_spawn_inner(
    runtime: &DesktopRuntime,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime.spawn_agent(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_write(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: AgentWriteRequest,
) -> Result<(), String> {
    agent_write_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_write(
    runtime: &DesktopRuntime,
    request: AgentWriteRequest,
) -> Result<(), String> {
    agent_write_inner(runtime, request)
}

fn agent_write_inner(runtime: &DesktopRuntime, request: AgentWriteRequest) -> Result<(), String> {
    runtime.write_agent(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_resize(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalResizeRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_resize_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_resize(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_resize_inner(runtime, request)
}

fn agent_resize_inner(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime.resize_agent(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_focus(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalFocusRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_focus_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_focus(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    agent_focus_inner(runtime, request)
}

fn agent_focus_inner(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime.focus_agent(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_close(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    agent_close_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    agent_close_inner(runtime, request)
}

fn agent_close_inner(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime.close_agent(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_snapshot(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<AgentRuntimeSnapshot>, String> {
    agent_snapshot_inner(&runtime)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_snapshot(runtime: &DesktopRuntime) -> Result<Vec<AgentRuntimeSnapshot>, String> {
    agent_snapshot_inner(runtime)
}

fn agent_snapshot_inner(runtime: &DesktopRuntime) -> Result<Vec<AgentRuntimeSnapshot>, String> {
    Ok(runtime.snapshot_agents())
}

/// Return the runtime registry's open platform catalog for launcher rendering.
/// Registry parse failures are surfaced to the host instead of silently
/// falling back to a hard-coded list.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn agent_platforms() -> Result<Vec<impulse_ops::agent_registry::AgentPlatformInfo>, String>
{
    agent_platforms_inner()
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_platforms() -> Result<Vec<impulse_ops::agent_registry::AgentPlatformInfo>, String>
{
    agent_platforms_inner()
}

fn agent_platforms_inner() -> Result<Vec<impulse_ops::agent_registry::AgentPlatformInfo>, String> {
    let registry = impulse_ops::agent_registry::AgentRegistry::registry_for_runtime()
        .map_err(err_to_string)?;
    Ok(impulse_ops::agent_registry::AgentPlatformsReport::from_registry(&registry).platforms)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn supervisor_local_action(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: SupervisorLocalActionRequest,
) -> Result<(), String> {
    supervisor_local_action_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn supervisor_local_action(
    runtime: &DesktopRuntime,
    request: SupervisorLocalActionRequest,
) -> Result<(), String> {
    supervisor_local_action_inner(runtime, request)
}

fn supervisor_local_action_inner(
    runtime: &DesktopRuntime,
    request: SupervisorLocalActionRequest,
) -> Result<(), String> {
    runtime
        .dispatch_supervisor_local_action(request)
        .map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn terminal_open(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    terminal_open_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_open(
    runtime: &DesktopRuntime,
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    terminal_open_inner(runtime, request)
}

fn terminal_open_inner(
    runtime: &DesktopRuntime,
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    runtime.open(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn terminal_write(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalWriteRequest,
) -> Result<(), String> {
    terminal_write_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_write(
    runtime: &DesktopRuntime,
    request: TerminalWriteRequest,
) -> Result<(), String> {
    terminal_write_inner(runtime, request)
}

fn terminal_write_inner(
    runtime: &DesktopRuntime,
    request: TerminalWriteRequest,
) -> Result<(), String> {
    runtime.write(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn terminal_resize(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalResizeRequest,
) -> Result<(), String> {
    terminal_resize_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_resize(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<(), String> {
    terminal_resize_inner(runtime, request)
}

fn terminal_resize_inner(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<(), String> {
    runtime.resize(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn terminal_close(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    terminal_close_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    terminal_close_inner(runtime, request)
}

fn terminal_close_inner(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime.close(request).map_err(err_to_string)
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn terminal_focus(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalFocusRequest,
) -> Result<(), String> {
    terminal_focus_inner(&runtime, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_focus(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<(), String> {
    terminal_focus_inner(runtime, request)
}

fn terminal_focus_inner(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<(), String> {
    runtime.focus(request).map_err(err_to_string)
}

#[cfg_attr(feature = "legacy-tauri-runtime", tauri::command)]
pub async fn native_island_request(
    request: NativeIslandRequest,
) -> Result<NativeIslandResult, String> {
    DefaultNativeIslandHost
        .dispatch(request)
        .map_err(err_to_string)
}

// ──────────────────────────── MCP + workspace surface ───────────────────────────

/// Request body for `mcp_invoke`. The supervisor (Dioxus UI or external
/// coding agent) names a tool, supplies arguments, and either confirms or
/// declines the call. The runtime walks the registry, runs the tool body,
/// appends to the audit log, and returns the invocation record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpInvokeRequest {
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub caller_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterWorkspaceRequest {
    pub root: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub project_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewDecisionRequest {
    pub id: String,
    pub decision: ReviewDecision,
    #[serde(default)]
    pub target_agent_id: Option<String>,
    #[serde(default)]
    pub confirmed: bool,
}

impl RegisterWorkspaceRequest {
    pub fn into_target(self) -> WorkspaceTarget {
        WorkspaceTarget {
            root: self.root,
            label: self.label,
            purpose: self.purpose,
            project_notes: self.project_notes,
        }
    }
}

/// Shared state injected by the desktop host. Both `DesktopRuntime` and the auxiliary
/// MCP/workspace registries live here so the host command surface has a
/// single `State<DesktopShellState>` to read from.
#[derive(Clone)]
pub struct DesktopShellState {
    pub runtime: std::sync::Arc<DesktopRuntime>,
    pub workspaces: std::sync::Arc<WorkspaceRegistry>,
    pub mcp: std::sync::Arc<McpToolRegistry>,
    pub memory_root: std::path::PathBuf,
}

impl DesktopShellState {
    pub fn new(
        runtime: std::sync::Arc<DesktopRuntime>,
        workspaces: std::sync::Arc<WorkspaceRegistry>,
        mcp: std::sync::Arc<McpToolRegistry>,
        memory_root: std::path::PathBuf,
    ) -> Self {
        Self {
            runtime,
            workspaces,
            mcp,
            memory_root,
        }
    }

    pub fn context(&self) -> McpContext {
        McpContext::new(
            std::sync::Arc::clone(&self.runtime),
            std::sync::Arc::clone(&self.workspaces),
            self.memory_root.clone(),
        )
    }
}

/// Host command — invoke a built-in MCP tool by name. The command surface
/// is the same with or without the `legacy-tauri-runtime` feature: when the
/// legacy adapter is enabled the function gets `tauri::State`; otherwise it
/// takes a borrowed reference so SSR tests and pure-Rust callers can exercise
/// the same command body.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn mcp_invoke(
    state: tauri::State<'_, DesktopShellState>,
    request: McpInvokeRequest,
) -> Result<McpInvocation, String> {
    let state = state.inner().clone();
    let context = state.context();
    invoke_blocking(&state, request, context)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn mcp_invoke(
    state: &DesktopShellState,
    request: McpInvokeRequest,
) -> Result<McpInvocation, String> {
    let context = state.context();
    invoke_blocking(state, request, context)
}

fn invoke_blocking(
    state: &DesktopShellState,
    request: McpInvokeRequest,
    context: McpContext,
) -> Result<McpInvocation, String> {
    let McpInvokeRequest {
        tool,
        arguments,
        confirmed,
        caller_agent_id,
    } = request;
    let caller_agent_id = caller_agent_id.or_else(|| {
        state
            .runtime
            .snapshot_agents()
            .first()
            .map(|agent| agent.agent_id.clone())
    });
    state
        .mcp
        .invoke(&tool, caller_agent_id, arguments, confirmed, &context)
        .map_err(err_to_string)
}

/// host command — list the descriptors of every registered MCP tool.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn mcp_descriptors(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::runtime::BuiltInMcpTool>, String> {
    Ok(state.inner().mcp.descriptors())
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn mcp_descriptors(
    state: &DesktopShellState,
) -> Result<Vec<crate::runtime::BuiltInMcpTool>, String> {
    Ok(state.mcp.descriptors())
}

/// host command — list staged review payloads for the Dioxus review-first
/// console. The records live under `memory_root/review_queue`.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn review_queue(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::mcp::ReviewQueueItem>, String> {
    list_review_queue(&state.inner().memory_root).map_err(err_to_string)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn review_queue(
    state: &DesktopShellState,
) -> Result<Vec<crate::mcp::ReviewQueueItem>, String> {
    list_review_queue(&state.memory_root).map_err(err_to_string)
}

/// host command — apply or skip one staged review payload through the MCP
/// registry so the decision produces the same audit receipt as normal tool
/// invocations.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn review_decision(
    state: tauri::State<'_, DesktopShellState>,
    request: ReviewDecisionRequest,
) -> Result<McpInvocation, String> {
    let state = state.inner().clone();
    review_decision_inner(&state, request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn review_decision(
    state: &DesktopShellState,
    request: ReviewDecisionRequest,
) -> Result<McpInvocation, String> {
    review_decision_inner(state, request)
}

fn review_decision_inner(
    state: &DesktopShellState,
    request: ReviewDecisionRequest,
) -> Result<McpInvocation, String> {
    let context = state.context();
    let arguments = serde_json::to_value(&request).map_err(err_to_string)?;
    state
        .mcp
        .invoke(
            "impulse.review_decision",
            None,
            arguments,
            request.confirmed,
            &context,
        )
        .map_err(err_to_string)
}

/// host command — list registered workspaces so the Dioxus switcher can
/// render and `impulse.list_workspaces` has a single source of truth.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn list_workspaces(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::workspace::WorkspaceEntry>, String> {
    Ok(state.inner().workspaces.list())
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn list_workspaces(
    state: &DesktopShellState,
) -> Result<Vec<crate::workspace::WorkspaceEntry>, String> {
    Ok(state.workspaces.list())
}

#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn register_workspace(
    state: tauri::State<'_, DesktopShellState>,
    request: RegisterWorkspaceRequest,
) -> Result<crate::workspace::WorkspaceEntry, String> {
    register_workspace_inner(state.inner(), request)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn register_workspace(
    state: &DesktopShellState,
    request: RegisterWorkspaceRequest,
) -> Result<crate::workspace::WorkspaceEntry, String> {
    register_workspace_inner(state, request)
}

fn register_workspace_inner(
    state: &DesktopShellState,
    request: RegisterWorkspaceRequest,
) -> Result<crate::workspace::WorkspaceEntry, String> {
    let root = request.root.clone();
    state
        .workspaces
        .register_workspace(request.into_target())
        .map_err(err_to_string)?;
    state
        .workspaces
        .lookup(&root)
        .ok_or_else(|| format!("workspace `{root}` was registered but could not be read back"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn host_invoke_manifest_has_unique_command_names() {
        let mut seen = HashSet::new();
        for command in HOST_INVOKE_COMMANDS {
            assert!(
                seen.insert(command),
                "duplicate host invoke command: {command}"
            );
        }
    }

    #[test]
    fn host_invoke_manifest_covers_ui_required_commands() {
        for command in [
            AGENT_FOCUS_COMMAND,
            AGENT_RESIZE_COMMAND,
            AGENT_SNAPSHOT_COMMAND,
            AGENT_WRITE_COMMAND,
            LIST_WORKSPACES_COMMAND,
            MCP_DESCRIPTORS_COMMAND,
            MCP_INVOKE_COMMAND,
            REGISTER_WORKSPACE_COMMAND,
            REVIEW_DECISION_COMMAND,
            REVIEW_QUEUE_COMMAND,
        ] {
            assert!(
                HOST_INVOKE_COMMANDS.contains(&command),
                "host invoke manifest missing UI-required command: {command}"
            );
        }
    }
}
