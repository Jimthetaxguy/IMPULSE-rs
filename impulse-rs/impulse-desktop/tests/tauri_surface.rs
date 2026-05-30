use std::collections::HashMap;

use impulse_desktop::tauri_commands;
use impulse_desktop::{
    DesktopRuntime, LocalSupervisorAction, NativeIslandKind, NativeIslandRequest,
    SupervisorLocalActionRequest, TerminalCloseRequest, TerminalOpenRequest, TerminalWriteRequest,
};
use serde_json::json;

#[tokio::test]
async fn test_tauri_terminal_open_command_surface_returns_serializable_session() {
    let runtime = DesktopRuntime::default();
    let response = tauri_commands::terminal_open(
        &runtime,
        TerminalOpenRequest {
            session_id: Some("tauri-session".to_string()),
            command: "sh".to_string(),
            args: vec!["-lc".to_string(), "printf ready; sleep 1".to_string()],
            cwd: None,
            env: HashMap::new(),
            rows: 24,
            cols: 80,
        },
    )
    .await
    .expect("terminal_open command should route");

    assert_eq!(response.session_id, "tauri-session");
    assert!(response.alive);

    tauri_commands::terminal_write(
        &runtime,
        TerminalWriteRequest {
            session_id: "tauri-session".to_string(),
            data: b"pwd".to_vec(),
        },
    )
    .await
    .expect("terminal_write should see shared command state");

    tauri_commands::terminal_close(
        &runtime,
        TerminalCloseRequest {
            session_id: "tauri-session".to_string(),
        },
    )
    .await
    .expect("terminal_close should see shared command state");
}

#[tokio::test]
async fn test_tauri_supervisor_local_action_routes_to_runtime() {
    let runtime = DesktopRuntime::default();
    tauri_commands::terminal_open(
        &runtime,
        TerminalOpenRequest {
            session_id: Some("supervisor-session".to_string()),
            command: "sh".to_string(),
            args: vec!["-lc".to_string(), "sleep 1".to_string()],
            cwd: None,
            env: HashMap::new(),
            rows: 24,
            cols: 80,
        },
    )
    .await
    .expect("open terminal session");

    tauri_commands::supervisor_local_action(
        &runtime,
        SupervisorLocalActionRequest {
            action: LocalSupervisorAction::FocusAgent {
                agent_id: "supervisor-session".to_string(),
            },
        },
    )
    .await
    .expect("focus agent through supervisor action");

    let snapshot = tauri_commands::agent_snapshot(&runtime)
        .await
        .expect("snapshot agents");
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot[0].focused);

    tauri_commands::terminal_close(
        &runtime,
        TerminalCloseRequest {
            session_id: "supervisor-session".to_string(),
        },
    )
    .await
    .expect("close terminal session");
}

#[tokio::test]
async fn test_tauri_native_island_command_surface_returns_dto() {
    let result = tauri_commands::native_island_request(NativeIslandRequest {
        request_id: "tauri-native".to_string(),
        kind: NativeIslandKind::AppKitProbe,
        payload: json!({ "caller": "dioxus" }),
    })
    .await
    .expect("native island command should route");

    assert_eq!(result.request_id, "tauri-native");
    assert_eq!(result.payload["state_owner"], "dioxus");
}
