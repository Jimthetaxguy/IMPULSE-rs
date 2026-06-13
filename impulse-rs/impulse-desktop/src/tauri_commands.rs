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

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_spawn(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .spawn_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_spawn(
    runtime: &DesktopRuntime,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .spawn_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_write(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: AgentWriteRequest,
) -> Result<(), String> {
    runtime
        .write_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_write(
    runtime: &DesktopRuntime,
    request: AgentWriteRequest,
) -> Result<(), String> {
    runtime
        .write_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_resize(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalResizeRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .resize_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_resize(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .resize_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_focus(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalFocusRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .focus_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_focus(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime
        .focus_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_close(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime
        .close_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime
        .close_agent(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn agent_snapshot(
    runtime: tauri::State<'_, DesktopRuntime>,
) -> Result<Vec<AgentRuntimeSnapshot>, String> {
    Ok(runtime.snapshot_agents())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn agent_snapshot(runtime: &DesktopRuntime) -> Result<Vec<AgentRuntimeSnapshot>, String> {
    Ok(runtime.snapshot_agents())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn supervisor_local_action(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: SupervisorLocalActionRequest,
) -> Result<(), String> {
    runtime
        .dispatch_supervisor_local_action(request)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn supervisor_local_action(
    runtime: &DesktopRuntime,
    request: SupervisorLocalActionRequest,
) -> Result<(), String> {
    runtime
        .dispatch_supervisor_local_action(request)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn terminal_open(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    runtime.open(request).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn terminal_open(
    runtime: &DesktopRuntime,
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    runtime.open(request).map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn terminal_write(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalWriteRequest,
) -> Result<(), String> {
    runtime.write(request).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn terminal_write(
    runtime: &DesktopRuntime,
    request: TerminalWriteRequest,
) -> Result<(), String> {
    runtime.write(request).map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn terminal_resize(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalResizeRequest,
) -> Result<(), String> {
    runtime.resize(request).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn terminal_resize(
    runtime: &DesktopRuntime,
    request: TerminalResizeRequest,
) -> Result<(), String> {
    runtime.resize(request).map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn terminal_close(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime.close(request).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn terminal_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    runtime.close(request).map_err(|error| error.to_string())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn terminal_focus(
    runtime: tauri::State<'_, DesktopRuntime>,
    request: TerminalFocusRequest,
) -> Result<(), String> {
    runtime.focus(request).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn terminal_focus(
    runtime: &DesktopRuntime,
    request: TerminalFocusRequest,
) -> Result<(), String> {
    runtime.focus(request).map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn native_island_request(
    request: NativeIslandRequest,
) -> Result<NativeIslandResult, String> {
    DefaultNativeIslandHost
        .dispatch(request)
        .map_err(|error| error.to_string())
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

/// Shared state injected by Tauri. Both `DesktopRuntime` and the auxiliary
/// MCP/workspace registries live here so the Tauri command surface has a
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

/// Tauri command — invoke a built-in MCP tool by name. The command surface
/// is the same with or without the `tauri-runtime` feature: when Tauri is
/// enabled the function gets `tauri::State`; otherwise it takes a borrowed
/// reference so the SSR tests and pure-Rust callers can exercise it.
#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn mcp_invoke(
    state: tauri::State<'_, DesktopShellState>,
    request: McpInvokeRequest,
) -> Result<McpInvocation, String> {
    let state = state.inner().clone();
    let context = state.context();
    let _ = state; // keep borrow alive for the await
    invoke_blocking(&state, request, context)
}

#[cfg(not(feature = "tauri-runtime"))]
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
        .map_err(|error| error.to_string())
}

/// Tauri command — list the descriptors of every registered MCP tool.
#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn mcp_descriptors(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::runtime::BuiltInMcpTool>, String> {
    Ok(state.inner().mcp.descriptors())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn mcp_descriptors(
    state: &DesktopShellState,
) -> Result<Vec<crate::runtime::BuiltInMcpTool>, String> {
    Ok(state.mcp.descriptors())
}

/// Tauri command — list staged review payloads for the Dioxus review-first
/// console. The records live under `memory_root/review_queue`.
#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn review_queue(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::mcp::ReviewQueueItem>, String> {
    list_review_queue(&state.inner().memory_root).map_err(|error| error.to_string())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn review_queue(
    state: &DesktopShellState,
) -> Result<Vec<crate::mcp::ReviewQueueItem>, String> {
    list_review_queue(&state.memory_root).map_err(|error| error.to_string())
}

/// Tauri command — apply or skip one staged review payload through the MCP
/// registry so the decision produces the same audit receipt as normal tool
/// invocations.
#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn review_decision(
    state: tauri::State<'_, DesktopShellState>,
    request: ReviewDecisionRequest,
) -> Result<McpInvocation, String> {
    let state = state.inner().clone();
    review_decision_inner(&state, request)
}

#[cfg(not(feature = "tauri-runtime"))]
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
    let arguments = serde_json::to_value(&request).map_err(|error| error.to_string())?;
    state
        .mcp
        .invoke(
            "impulse.review_decision",
            None,
            arguments,
            request.confirmed,
            &context,
        )
        .map_err(|error| error.to_string())
}

/// Tauri command — list registered workspaces so the Dioxus switcher can
/// render and `impulse.list_workspaces` has a single source of truth.
#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn list_workspaces(
    state: tauri::State<'_, DesktopShellState>,
) -> Result<Vec<crate::workspace::WorkspaceEntry>, String> {
    Ok(state.inner().workspaces.list())
}

#[cfg(not(feature = "tauri-runtime"))]
pub async fn list_workspaces(
    state: &DesktopShellState,
) -> Result<Vec<crate::workspace::WorkspaceEntry>, String> {
    Ok(state.workspaces.list())
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn register_workspace(
    state: tauri::State<'_, DesktopShellState>,
    request: RegisterWorkspaceRequest,
) -> Result<crate::workspace::WorkspaceEntry, String> {
    register_workspace_inner(state.inner(), request)
}

#[cfg(not(feature = "tauri-runtime"))]
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
        .map_err(|error| error.to_string())?;
    state
        .workspaces
        .lookup(&root)
        .ok_or_else(|| format!("workspace `{root}` was registered but could not be read back"))
}
