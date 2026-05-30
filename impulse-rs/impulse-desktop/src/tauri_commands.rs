use crate::bridge::{
    TerminalBridge, TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest,
    TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};
use crate::native::{DefaultNativeIslandHost, NativeIslandRequest, NativeIslandResult};
use crate::runtime::{
    AgentRuntimeSnapshot, AgentSpawnRequest, AgentWriteRequest, DesktopRuntime,
    SupervisorLocalActionRequest,
};
use crate::NativeIslandHost;

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
