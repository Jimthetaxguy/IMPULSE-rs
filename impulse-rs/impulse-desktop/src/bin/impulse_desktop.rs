use std::sync::Arc;

use impulse_desktop::daemon_ops::DesktopDaemonOpsConfig;
use impulse_desktop::daemon_sidecar::{DesktopDaemonSidecarHandle, DesktopDaemonSidecarMode};
use impulse_desktop::desktop_host::desktop_config_with_shutdown;
use impulse_desktop::desktop_shutdown::DesktopShutdownCoordinator;
use impulse_desktop::host_bridge::{
    channel_event_sink, install_live_host_context, LiveDesktopApp, LiveHostContext,
};
use impulse_desktop::host_commands::DesktopShellState;
use impulse_desktop::project_boundary::{
    DesktopProjectBoundaryController, ProjectMemoryScope, SwitchableDesktopEventSink,
    SwitchableGovernedTaskGateway,
};
use impulse_desktop::runtime::{DesktopEventSink, DesktopRuntime};
use impulse_desktop::workspace::WorkspaceRegistry;
use impulse_desktop::McpToolRegistry;

fn main() {
    let discovered_boundary = DesktopDaemonOpsConfig::discover_explicit_project_bound();
    if package_scope_probe_enabled() {
        emit_package_scope_receipt(discovered_boundary.as_ref());
        return;
    }

    // Wire the runtime's event sink to a channel the in-webview bridge drains,
    // then assemble the same command surface the legacy Tauri host exposed.
    let (event_sink, event_rx) = channel_event_sink();
    let downstream: Arc<dyn DesktopEventSink> = event_sink.clone();
    let runtime_sink = SwitchableDesktopEventSink::new(Arc::clone(&downstream));
    let governed_task_gateway = SwitchableGovernedTaskGateway::default();
    let runtime = Arc::new(
        DesktopRuntime::builder()
            .with_event_sink(Arc::new(runtime_sink.clone()))
            .with_governed_task_gateway(Arc::new(governed_task_gateway.clone()))
            .build(),
    );
    let daemon_sidecar = DesktopDaemonSidecarHandle::default();
    let shutdown_coordinator =
        DesktopShutdownCoordinator::new(Arc::clone(&runtime), None, daemon_sidecar, None);
    let memory_scope = ProjectMemoryScope::default();
    let project_boundary = DesktopProjectBoundaryController::new(
        downstream,
        runtime_sink,
        governed_task_gateway,
        memory_scope.clone(),
        shutdown_coordinator.clone(),
    );
    let initial_project_root = if let Some(config) = discovered_boundary {
        let boundary = project_boundary
            .connect_config(config)
            .unwrap_or_else(|error| {
                eprintln!("desktop project daemon boundary unavailable: {error}");
                std::process::exit(2);
            });
        match boundary.daemon_mode {
            DesktopDaemonSidecarMode::Existing => {
                eprintln!("desktop attached to existing Impulse daemon");
            }
            DesktopDaemonSidecarMode::Spawned => {
                eprintln!("desktop started packaged Impulse daemon companion");
            }
        }
        Some(boundary.project_root)
    } else {
        eprintln!(
            "desktop oversight disconnected: the first governed launch will bind its selected project"
        );
        None
    };
    let workspaces = Arc::new(match initial_project_root {
        Some(project_root) => {
            WorkspaceRegistry::with_workspace_roots([project_root.to_string_lossy().into_owned()])
                .unwrap_or_else(|error| {
                    eprintln!("desktop could not register its connected project: {error}");
                    std::process::exit(2);
                })
        }
        None => WorkspaceRegistry::empty(),
    });
    let mcp = Arc::new(McpToolRegistry::with_builtins());
    let state =
        DesktopShellState::new_live(runtime, workspaces, mcp, memory_scope, project_boundary);

    // Hand the state + event stream to the bridge hook before launch.
    install_live_host_context(
        LiveHostContext::new(state, event_rx)
            .with_shutdown_coordinator(shutdown_coordinator.clone()),
    );

    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config_with_shutdown(shutdown_coordinator))
        .launch(LiveDesktopApp);
}

fn package_scope_probe_enabled() -> bool {
    std::env::var("IMPULSE_DESKTOP_SCOPE_PROBE")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn emit_package_scope_receipt(daemon_config: Option<&DesktopDaemonOpsConfig>) {
    let project_root = daemon_config
        .and_then(|config| config.project_root.as_ref())
        .map(|path| path.display().to_string());
    let memory_root = daemon_config
        .and_then(|config| config.project_root.as_ref())
        .map(|path| path.join(".impulse").display().to_string());
    let receipt = serde_json::json!({
        "status": "desktop-scope-resolved",
        "daemon_configured": daemon_config.is_some(),
        "project_root": project_root,
        "memory_root": memory_root,
    });
    println!("IMPULSE_DESKTOP_SCOPE_RECEIPT {receipt}");
}
