use crate::bridge::{
    TerminalBridge, TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest,
    TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};
use crate::mcp::{
    list_review_queue, McpContext, McpError, McpInvocation, McpToolRegistry, ReviewDecision,
};
use crate::native::{DefaultNativeIslandHost, NativeIslandRequest, NativeIslandResult};
use crate::project_boundary::{
    DesktopProjectBoundaryConnector, DesktopProjectBoundaryController, ProjectMemoryScope,
};
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
pub const GOVERNED_TASK_MUTATE_COMMAND: &str = "governed_task_mutate";
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
    GOVERNED_TASK_MUTATE_COMMAND,
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
    run_runtime_blocking("agent spawn", runtime.inner().clone(), move |runtime| {
        agent_spawn_inner(runtime, request)
    })
    .await
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_spawn(
    runtime: &DesktopRuntime,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    run_runtime_blocking("agent spawn", runtime.clone(), move |runtime| {
        agent_spawn_inner(runtime, request)
    })
    .await
}

pub async fn agent_spawn_with_state(
    state: &DesktopShellState,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        prepare_governed_project_boundary(&state, &request)?;
        state.runtime.spawn_agent(request).map_err(err_to_string)
    })
    .await
    .map_err(|error| format!("agent spawn worker failed: {error}"))?
}

fn prepare_governed_project_boundary(
    state: &DesktopShellState,
    request: &AgentSpawnRequest,
) -> Result<(), String> {
    let Some(project_root) = requested_governed_project_root(request)? else {
        return Ok(());
    };
    let project_root_text = project_root.to_str().ok_or_else(|| {
        format!(
            "governed project root `{}` cannot be represented as UTF-8",
            project_root.display()
        )
    })?;
    state
        .workspaces
        .touch(project_root_text)
        .map_err(|error| format!("select a registered workspace before launch: {error}"))?;
    state.connect_project_boundary(&project_root)
}

fn requested_governed_project_root(
    request: &AgentSpawnRequest,
) -> Result<Option<std::path::PathBuf>, String> {
    if request.role_assignment.is_none()
        && request.verification_profile.is_none()
        && request.acceptance_criteria.is_empty()
    {
        return Ok(None);
    }
    if request.role_assignment.is_none() {
        return Err(
            "verification and acceptance criteria require an explicit governed role assignment"
                .to_string(),
        );
    }
    let task = request
        .task
        .as_deref()
        .map(str::trim)
        .filter(|task| !task.is_empty())
        .ok_or_else(|| "governed launch requires a non-empty task".to_string())?;
    if task.contains('\0') {
        return Err("governed launch task must not contain NUL bytes".to_string());
    }
    if request.verification_profile.is_some() && request.acceptance_criteria.is_empty() {
        return Err(
            "closed-loop governed launch requires at least one acceptance criterion".to_string(),
        );
    }
    if request.verification_profile.is_none() && !request.acceptance_criteria.is_empty() {
        return Err("acceptance criteria require an explicit verification profile".to_string());
    }
    let cwd = request.cwd.as_deref().ok_or_else(|| {
        "governed launch requires both an absolute cwd and workspace root".to_string()
    })?;
    let workspace_root = request
        .workspace
        .as_ref()
        .map(|workspace| workspace.root.as_str())
        .ok_or_else(|| {
            "governed launch requires both an absolute cwd and workspace root".to_string()
        })?;
    let cwd = canonical_host_project_path("cwd", cwd)?;
    let workspace_root = canonical_host_project_path("workspace.root", workspace_root)?;
    if cwd != workspace_root {
        return Err(format!(
            "governed launch cwd `{}` and workspace root `{}` must resolve to the same project",
            cwd.display(),
            workspace_root.display()
        ));
    }
    Ok(Some(workspace_root))
}

fn canonical_host_project_path(field: &str, value: &str) -> Result<std::path::PathBuf, String> {
    let path = std::path::Path::new(value);
    if !path.is_absolute() {
        return Err(format!(
            "governed launch {field} `{value}` must be absolute"
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("governed launch {field} `{value}` is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!(
            "governed launch {field} `{}` is not a directory",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn agent_spawn_inner(
    runtime: &DesktopRuntime,
    request: AgentSpawnRequest,
) -> Result<AgentRuntimeSnapshot, String> {
    runtime.spawn_agent(request).map_err(err_to_string)
}

async fn run_runtime_blocking<T, F>(
    label: &'static str,
    runtime: DesktopRuntime,
    operation: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&DesktopRuntime) -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(move || operation(&runtime))
        .await
        .map_err(|error| format!("{label} worker failed: {error}"))?
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

/// Acknowledged governed-task mutation. The Unix request is blocking, so the
/// host runs it on Tokio's blocking pool rather than the Dioxus async worker.
#[cfg(feature = "legacy-tauri-runtime")]
#[tauri::command]
pub async fn governed_task_mutate(
    state: tauri::State<'_, DesktopShellState>,
    request: impulse_ops::governed_task::GovernedTaskMutationRequest,
) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
    governed_task_mutate_runtime(std::sync::Arc::clone(&state.inner().runtime), request).await
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn governed_task_mutate(
    state: &DesktopShellState,
    request: impulse_ops::governed_task::GovernedTaskMutationRequest,
) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
    governed_task_mutate_runtime(std::sync::Arc::clone(&state.runtime), request).await
}

async fn governed_task_mutate_runtime(
    runtime: std::sync::Arc<DesktopRuntime>,
    request: impulse_ops::governed_task::GovernedTaskMutationRequest,
) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
    tokio::task::spawn_blocking(move || {
        runtime.mutate_governed_task(request).map_err(err_to_string)
    })
    .await
    .map_err(|error| format!("governed task command worker failed: {error}"))?
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
    run_runtime_blocking("agent close", runtime.inner().clone(), move |runtime| {
        agent_close_inner(runtime, request)
    })
    .await
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn agent_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    run_runtime_blocking("agent close", runtime.clone(), move |runtime| {
        agent_close_inner(runtime, request)
    })
    .await
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
    run_runtime_blocking("terminal close", runtime.inner().clone(), move |runtime| {
        terminal_close_inner(runtime, request)
    })
    .await
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn terminal_close(
    runtime: &DesktopRuntime,
    request: TerminalCloseRequest,
) -> Result<(), String> {
    run_runtime_blocking("terminal close", runtime.clone(), move |runtime| {
        terminal_close_inner(runtime, request)
    })
    .await
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
    memory_scope: ProjectMemoryScope,
    project_boundary: Option<std::sync::Arc<dyn DesktopProjectBoundaryConnector>>,
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
            memory_scope: ProjectMemoryScope::connected(memory_root),
            project_boundary: None,
        }
    }

    pub fn new_live(
        runtime: std::sync::Arc<DesktopRuntime>,
        workspaces: std::sync::Arc<WorkspaceRegistry>,
        mcp: std::sync::Arc<McpToolRegistry>,
        memory_scope: ProjectMemoryScope,
        project_boundary: DesktopProjectBoundaryController,
    ) -> Self {
        Self {
            runtime,
            workspaces,
            mcp,
            memory_scope,
            project_boundary: Some(std::sync::Arc::new(project_boundary)),
        }
    }

    #[cfg(test)]
    fn new_live_with_connector(
        runtime: std::sync::Arc<DesktopRuntime>,
        workspaces: std::sync::Arc<WorkspaceRegistry>,
        mcp: std::sync::Arc<McpToolRegistry>,
        memory_scope: ProjectMemoryScope,
        project_boundary: std::sync::Arc<dyn DesktopProjectBoundaryConnector>,
    ) -> Self {
        Self {
            runtime,
            workspaces,
            mcp,
            memory_scope,
            project_boundary: Some(project_boundary),
        }
    }

    pub fn memory_root(&self) -> Result<std::path::PathBuf, String> {
        self.memory_scope.root()
    }

    pub fn context(&self) -> Result<McpContext, String> {
        Ok(McpContext::new(
            std::sync::Arc::clone(&self.runtime),
            std::sync::Arc::clone(&self.workspaces),
            self.memory_root()?,
        ))
    }

    pub fn connect_project_boundary(&self, project_root: &std::path::Path) -> Result<(), String> {
        let controller = self.project_boundary.as_ref().ok_or_else(|| {
            "dynamic project daemon connection is unavailable in this host".to_string()
        })?;
        let boundary = controller.connect_project(project_root)?;
        self.memory_scope.install(boundary.memory_root);
        Ok(())
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
    invoke_on_blocking_pool(state, request).await
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn mcp_invoke(
    state: &DesktopShellState,
    request: McpInvokeRequest,
) -> Result<McpInvocation, String> {
    invoke_on_blocking_pool(state.clone(), request).await
}

async fn invoke_on_blocking_pool(
    state: DesktopShellState,
    request: McpInvokeRequest,
) -> Result<McpInvocation, String> {
    tokio::task::spawn_blocking(move || {
        if request.tool == "impulse.agent_spawn" {
            if !request.confirmed {
                let error = McpError::ConfirmationRequired {
                    tool: request.tool.clone(),
                    message: "agent_spawn mutates terminal and project-boundary state".to_string(),
                };
                record_mcp_preflight_rejection(&state, &request, &error);
                return Err(error.to_string());
            }
            let spawn_request: AgentSpawnRequest =
                match serde_json::from_value(request.arguments.clone()) {
                    Ok(request) => request,
                    Err(parse_error) => {
                        let error = McpError::Tool {
                            tool: request.tool.clone(),
                            message: format!("invalid AgentSpawnRequest payload: {parse_error}"),
                        };
                        record_mcp_preflight_rejection(&state, &request, &error);
                        return Err(error.to_string());
                    }
                };
            if let Err(message) = prepare_governed_project_boundary(&state, &spawn_request) {
                let error = McpError::Tool {
                    tool: request.tool.clone(),
                    message,
                };
                record_mcp_preflight_rejection(&state, &request, &error);
                return Err(error.to_string());
            }
        }
        let context = match state.context() {
            Ok(context) => context,
            Err(message) if request.tool == "impulse.agent_spawn" => {
                let error = McpError::Tool {
                    tool: request.tool.clone(),
                    message,
                };
                record_mcp_preflight_rejection(&state, &request, &error);
                return Err(error.to_string());
            }
            Err(message) => return Err(message),
        };
        invoke_blocking(&state, request, context)
    })
    .await
    .map_err(|error| format!("MCP invocation worker failed: {error}"))?
}

fn resolved_mcp_caller_agent_id(
    state: &DesktopShellState,
    explicit: Option<String>,
) -> Option<String> {
    explicit.or_else(|| {
        state
            .runtime
            .snapshot_agents()
            .first()
            .map(|agent| agent.agent_id.clone())
    })
}

fn record_mcp_preflight_rejection(
    state: &DesktopShellState,
    request: &McpInvokeRequest,
    error: &McpError,
) {
    state.mcp.record_rejected_invocation(
        &request.tool,
        resolved_mcp_caller_agent_id(state, request.caller_agent_id.clone()),
        request.arguments.clone(),
        request.confirmed,
        error,
    );
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
    let caller_agent_id = resolved_mcp_caller_agent_id(state, caller_agent_id);
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
    list_review_queue(&state.inner().memory_root()?).map_err(err_to_string)
}

#[cfg(not(feature = "legacy-tauri-runtime"))]
pub async fn review_queue(
    state: &DesktopShellState,
) -> Result<Vec<crate::mcp::ReviewQueueItem>, String> {
    list_review_queue(&state.memory_root()?).map_err(err_to_string)
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
    let context = state.context()?;
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RecordingBoundaryConnector {
        calls: AtomicUsize,
    }

    impl DesktopProjectBoundaryConnector for RecordingBoundaryConnector {
        fn connect_project(
            &self,
            project_root: &std::path::Path,
        ) -> Result<crate::project_boundary::DesktopProjectBoundary, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let project_root = project_root
                .canonicalize()
                .map_err(|error| error.to_string())?;
            Ok(crate::project_boundary::DesktopProjectBoundary {
                project_root: project_root.clone(),
                memory_root: project_root.join(".impulse"),
                socket_path: project_root
                    .join(".impulse")
                    .join("sockets")
                    .join("impulse.sock"),
                daemon_mode: crate::daemon_sidecar::DesktopDaemonSidecarMode::Existing,
            })
        }
    }

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
            GOVERNED_TASK_MUTATE_COMMAND,
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

    #[tokio::test]
    async fn audited_mcp_spawn_connects_registered_project_before_context_lookup() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let canonical = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let canonical_text = canonical.display().to_string();
        let runtime = std::sync::Arc::new(DesktopRuntime::default());
        let workspaces = std::sync::Arc::new(
            WorkspaceRegistry::with_workspace_roots([canonical_text.as_str()])
                .expect("registered workspace"),
        );
        let mcp = std::sync::Arc::new(McpToolRegistry::with_builtins());
        let connector = std::sync::Arc::new(RecordingBoundaryConnector {
            calls: AtomicUsize::new(0),
        });
        let state = DesktopShellState::new_live_with_connector(
            runtime,
            workspaces,
            std::sync::Arc::clone(&mcp),
            ProjectMemoryScope::default(),
            connector.clone(),
        );
        let mut spawn = AgentSpawnRequest::terminal_harness(
            "ui-builder",
            crate::runtime::AgentPlatformId::try_new("missing-runtime").expect("valid platform id"),
            canonical_text.clone(),
            24,
            80,
        );
        spawn.task = Some("prove the visible launch path binds scope".to_string());
        spawn.role_assignment =
            Some(impulse_ops::role_assignment::canonical_governed_builder_assignment());
        let request = McpInvokeRequest {
            tool: "impulse.agent_spawn".to_string(),
            arguments: serde_json::to_value(spawn).expect("serialize spawn request"),
            confirmed: true,
            caller_agent_id: Some("impulse-ui".to_string()),
        };

        let error = mcp_invoke(&state, request)
            .await
            .expect_err("unknown runtime should fail after project activation");
        assert!(error.contains("unknown agent platform"), "{error}");
        assert!(!error.contains("project memory is unavailable"), "{error}");
        assert_eq!(connector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(state.memory_root().unwrap(), canonical.join(".impulse"));
        let audit = mcp.audit_log();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].tool, "impulse.agent_spawn");
        assert!(!audit[0].ok);
    }

    #[tokio::test]
    async fn unconfirmed_mcp_spawn_is_audited_before_project_activation() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let canonical = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let canonical_text = canonical.display().to_string();
        let runtime = std::sync::Arc::new(DesktopRuntime::default());
        let workspaces = std::sync::Arc::new(
            WorkspaceRegistry::with_workspace_roots([canonical_text.as_str()])
                .expect("registered workspace"),
        );
        let mcp = std::sync::Arc::new(McpToolRegistry::with_builtins());
        let connector = std::sync::Arc::new(RecordingBoundaryConnector {
            calls: AtomicUsize::new(0),
        });
        let state = DesktopShellState::new_live_with_connector(
            runtime,
            workspaces,
            std::sync::Arc::clone(&mcp),
            ProjectMemoryScope::default(),
            connector.clone(),
        );
        let mut spawn = AgentSpawnRequest::terminal_harness(
            "ui-builder",
            crate::runtime::AgentPlatformId::try_new("missing-runtime").expect("valid platform id"),
            canonical_text,
            24,
            80,
        );
        spawn.task = Some("this must not activate without confirmation".to_string());
        spawn.role_assignment =
            Some(impulse_ops::role_assignment::canonical_governed_builder_assignment());
        let request = McpInvokeRequest {
            tool: "impulse.agent_spawn".to_string(),
            arguments: serde_json::to_value(spawn).expect("serialize spawn request"),
            confirmed: false,
            caller_agent_id: Some("impulse-ui".to_string()),
        };

        let error = mcp_invoke(&state, request)
            .await
            .expect_err("unconfirmed spawn must fail before project activation");
        assert!(error.contains("caller did not confirm"), "{error}");
        assert_eq!(connector.calls.load(Ordering::SeqCst), 0);
        assert!(state.memory_root().is_err());
        let audit = mcp.audit_log();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].tool, "impulse.agent_spawn");
        assert_eq!(audit[0].caller_agent_id.as_deref(), Some("impulse-ui"));
        assert!(!audit[0].confirmed);
        assert!(!audit[0].ok);
        assert_eq!(audit[0].result["category"], "confirmation");
    }

    #[tokio::test]
    async fn malformed_mcp_spawn_is_audited_before_project_activation() {
        let runtime = std::sync::Arc::new(DesktopRuntime::default());
        let workspaces = std::sync::Arc::new(WorkspaceRegistry::default());
        let mcp = std::sync::Arc::new(McpToolRegistry::with_builtins());
        let connector = std::sync::Arc::new(RecordingBoundaryConnector {
            calls: AtomicUsize::new(0),
        });
        let state = DesktopShellState::new_live_with_connector(
            runtime,
            workspaces,
            std::sync::Arc::clone(&mcp),
            ProjectMemoryScope::default(),
            connector.clone(),
        );
        let request = McpInvokeRequest {
            tool: "impulse.agent_spawn".to_string(),
            arguments: serde_json::json!({"not": "an AgentSpawnRequest"}),
            confirmed: true,
            caller_agent_id: Some("impulse-ui".to_string()),
        };

        let error = mcp_invoke(&state, request)
            .await
            .expect_err("malformed spawn must fail before project activation");
        assert!(
            error.contains("invalid AgentSpawnRequest payload"),
            "{error}"
        );
        assert_eq!(connector.calls.load(Ordering::SeqCst), 0);
        assert!(state.memory_root().is_err());
        let audit = mcp.audit_log();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].tool, "impulse.agent_spawn");
        assert!(audit[0].confirmed);
        assert!(!audit[0].ok);
        assert_eq!(audit[0].result["category"], "tool");
    }
}
