use std::path::PathBuf;
use std::sync::Arc;

use impulse_desktop::desktop_host::desktop_config;
use impulse_desktop::host_bridge::{
    channel_event_sink, install_live_host_context, LiveDesktopApp, LiveHostContext,
};
use impulse_desktop::host_commands::DesktopShellState;
use impulse_desktop::runtime::DesktopRuntime;
use impulse_desktop::workspace::WorkspaceRegistry;
use impulse_desktop::McpToolRegistry;

fn main() {
    // Wire the runtime's event sink to a channel the in-webview bridge drains,
    // then assemble the same command surface the legacy Tauri host exposed.
    let (event_sink, event_rx) = channel_event_sink();
    let runtime = Arc::new(
        DesktopRuntime::builder()
            .with_event_sink(event_sink)
            .build(),
    );
    let workspaces = Arc::new(WorkspaceRegistry::with_default_workspaces());
    let mcp = Arc::new(McpToolRegistry::with_builtins());
    let state = DesktopShellState::new(runtime, workspaces, mcp, resolve_memory_root());

    // Hand the state + event stream to the bridge hook before launch.
    install_live_host_context(LiveHostContext::new(state, event_rx));

    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config())
        .launch(LiveDesktopApp);
}

/// Resolve the `.impulse/` memory root: explicit `IMPULSE_HOME`, else
/// `~/.impulse`, else `.impulse` under the current directory.
fn resolve_memory_root() -> PathBuf {
    if let Ok(home) = std::env::var("IMPULSE_HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home);
        }
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|value| !value.trim().is_empty());
    match home {
        Some(home) => PathBuf::from(home).join(".impulse"),
        None => PathBuf::from(".impulse"),
    }
}
