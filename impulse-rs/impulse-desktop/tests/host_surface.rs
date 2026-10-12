use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use impulse_desktop::host_commands;
use impulse_desktop::{
    DesktopEvent, DesktopEventSink, DesktopRuntime, LocalSupervisorAction, McpToolRegistry,
    NativeIslandKind, NativeIslandRequest, RegisterWorkspaceRequest, ReviewDecision,
    ReviewDecisionRequest, ReviewQueueStatus, SupervisorLocalActionRequest, TerminalCloseRequest,
    TerminalOpenRequest, TerminalWriteRequest, WorkspaceRegistry,
};
use serde_json::json;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<DesktopEvent>>,
}

impl RecordingSink {
    fn output_for(&self, agent_id: &str) -> String {
        let mut output = Vec::new();
        for event in self.events.lock().expect("events mutex poisoned").iter() {
            if let DesktopEvent::TerminalOutput {
                agent_id: event_agent_id,
                data,
            } = event
            {
                if event_agent_id == agent_id {
                    output.extend(data);
                }
            }
        }
        String::from_utf8_lossy(&output).to_string()
    }
}

impl DesktopEventSink for RecordingSink {
    fn emit(&self, event: DesktopEvent) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(event);
    }
}

#[tokio::test]
async fn test_host_terminal_open_command_surface_returns_serializable_session() {
    let runtime = DesktopRuntime::default();
    let response = host_commands::terminal_open(
        &runtime,
        TerminalOpenRequest {
            session_id: Some("host-session".to_string()),
            command: "sh".to_string(),
            args: vec!["-lc".to_string(), "printf ready; sleep 1".to_string()],
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 24,
            cols: 80,
        },
    )
    .await
    .expect("terminal_open command should route");

    assert_eq!(response.session_id, "host-session");
    assert!(response.alive);

    host_commands::terminal_write(
        &runtime,
        TerminalWriteRequest {
            session_id: "host-session".to_string(),
            data: b"pwd".to_vec(),
        },
    )
    .await
    .expect("terminal_write should see shared command state");

    host_commands::terminal_close(
        &runtime,
        TerminalCloseRequest {
            session_id: "host-session".to_string(),
        },
    )
    .await
    .expect("terminal_close should see shared command state");
}

#[tokio::test]
async fn test_host_supervisor_local_action_routes_to_runtime() {
    let runtime = DesktopRuntime::default();
    host_commands::terminal_open(
        &runtime,
        TerminalOpenRequest {
            session_id: Some("supervisor-session".to_string()),
            command: "sh".to_string(),
            args: vec!["-lc".to_string(), "sleep 1".to_string()],
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 24,
            cols: 80,
        },
    )
    .await
    .expect("open terminal session");

    host_commands::supervisor_local_action(
        &runtime,
        SupervisorLocalActionRequest {
            action: LocalSupervisorAction::FocusAgent {
                agent_id: "supervisor-session".to_string(),
            },
        },
    )
    .await
    .expect("focus agent through supervisor action");

    let snapshot = host_commands::agent_snapshot(&runtime)
        .await
        .expect("snapshot agents");
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot[0].focused);

    host_commands::terminal_close(
        &runtime,
        TerminalCloseRequest {
            session_id: "supervisor-session".to_string(),
        },
    )
    .await
    .expect("close terminal session");
}

#[tokio::test]
async fn test_host_native_island_command_surface_returns_dto() {
    let result = host_commands::native_island_request(NativeIslandRequest {
        request_id: "host-native".to_string(),
        kind: NativeIslandKind::AppKitProbe,
        payload: json!({ "caller": "dioxus" }),
    })
    .await
    .expect("native island command should route");

    assert_eq!(result.request_id, "host-native");
    assert_eq!(result.payload["state_owner"], "dioxus");
}

#[tokio::test]
async fn test_host_native_island_file_open_panel_is_unhandled_without_desktop_app_feature() {
    let result = host_commands::native_island_request(NativeIslandRequest {
        request_id: "host-folder".to_string(),
        kind: NativeIslandKind::FileOpenPanel,
        payload: json!({}),
    })
    .await
    .expect("native island command should route");

    #[cfg(not(feature = "desktop-app"))]
    assert!(!result.handled);
}

#[tokio::test]
async fn test_host_workspace_registration_preserves_project_notes() {
    let memory_root = tempfile::tempdir().expect("tempdir");
    let state = host_commands::DesktopShellState::new(
        Arc::new(DesktopRuntime::default()),
        Arc::new(WorkspaceRegistry::empty()),
        Arc::new(McpToolRegistry::with_builtins()),
        memory_root.path().to_path_buf(),
    );

    let entry = host_commands::register_workspace(
        &state,
        RegisterWorkspaceRequest {
            root: "/tmp".to_string(),
            label: Some("scratch".to_string()),
            purpose: Some("safe harness workspace".to_string()),
            project_notes: Some("operator-authored context".to_string()),
        },
    )
    .await
    .expect("register workspace");

    assert_eq!(entry.target.root, "/tmp");
    assert_eq!(entry.target.label.as_deref(), Some("scratch"));
    assert_eq!(
        entry.target.project_notes.as_deref(),
        Some("operator-authored context")
    );

    let listed = host_commands::list_workspaces(&state)
        .await
        .expect("list workspaces");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].target.project_notes.as_deref(),
        Some("operator-authored context")
    );
}

#[tokio::test]
async fn test_host_mcp_agent_spawn_rejects_unregistered_workspace() {
    let memory_root = tempfile::tempdir().expect("tempdir");
    let runtime = Arc::new(DesktopRuntime::default());
    let state = host_commands::DesktopShellState::new(
        Arc::clone(&runtime),
        Arc::new(WorkspaceRegistry::empty()),
        Arc::new(McpToolRegistry::with_builtins()),
        memory_root.path().to_path_buf(),
    );

    let error = host_commands::mcp_invoke(
        &state,
        host_commands::McpInvokeRequest {
            tool: "impulse.agent_spawn".to_string(),
            arguments: json!({
                "agent_id": "unregistered-agent",
                "session_id": "unregistered-agent-session",
                "platform": "shell",
                "command": "sh",
                "args": ["-lc", "sleep 1"],
                "cwd": "/tmp",
                "workspace": { "root": "/tmp", "label": "scratch" },
                "rows": 24,
                "cols": 80
            }),
            confirmed: true,
            caller_agent_id: Some("impulse-ui".to_string()),
        },
    )
    .await
    .expect_err("unregistered workspace should fail before spawn");

    assert!(error.contains("not registered"));
    assert!(
        host_commands::agent_snapshot(runtime.as_ref())
            .await
            .expect("snapshot agents")
            .is_empty(),
        "failed workspace validation must not spawn a terminal"
    );
}

#[tokio::test]
async fn test_host_mcp_agent_spawn_audits_and_touches_registered_workspace() {
    let memory_root = tempfile::tempdir().expect("tempdir");
    let runtime = Arc::new(DesktopRuntime::default());
    let state = host_commands::DesktopShellState::new(
        Arc::clone(&runtime),
        Arc::new(WorkspaceRegistry::empty()),
        Arc::new(McpToolRegistry::with_builtins()),
        memory_root.path().to_path_buf(),
    );

    host_commands::register_workspace(
        &state,
        RegisterWorkspaceRequest {
            root: "/tmp".to_string(),
            label: Some("scratch".to_string()),
            purpose: Some("safe launch workspace".to_string()),
            project_notes: Some("launch through audited MCP path".to_string()),
        },
    )
    .await
    .expect("register launch workspace");

    let invocation = host_commands::mcp_invoke(
        &state,
        host_commands::McpInvokeRequest {
            tool: "impulse.agent_spawn".to_string(),
            arguments: json!({
                "agent_id": "mcp-launch-agent",
                "session_id": "mcp-launch-agent-session",
                "platform": "shell",
                "command": "sh",
                "args": ["-lc", "printf spawned; sleep 1"],
                "cwd": "/tmp",
                "workspace": {
                    "root": "/tmp",
                    "label": "scratch",
                    "purpose": "safe launch workspace",
                    "project_notes": "launch through audited MCP path"
                },
                "rows": 24,
                "cols": 80
            }),
            confirmed: true,
            caller_agent_id: Some("impulse-ui".to_string()),
        },
    )
    .await
    .expect("spawn through MCP command");

    assert!(invocation.ok);
    assert_eq!(invocation.tool, "impulse.agent_spawn");
    assert_eq!(invocation.caller_agent_id.as_deref(), Some("impulse-ui"));
    assert_eq!(invocation.result["agent_id"], "mcp-launch-agent");
    assert_eq!(invocation.result["workspace"]["root"], "/tmp");

    let agents = host_commands::agent_snapshot(runtime.as_ref())
        .await
        .expect("snapshot spawned agent");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].agent_id, "mcp-launch-agent");
    assert_eq!(
        agents[0]
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.as_str()),
        Some("/tmp")
    );

    let listed = host_commands::list_workspaces(&state)
        .await
        .expect("list touched workspaces");
    assert_eq!(listed.len(), 1);
    assert!(listed[0].last_used_unix_ms.is_some());
    assert!(state
        .mcp
        .audit_log()
        .iter()
        .any(|item| item.tool == "impulse.agent_spawn" && item.ok));

    host_commands::terminal_close(
        runtime.as_ref(),
        TerminalCloseRequest {
            session_id: "mcp-launch-agent".to_string(),
        },
    )
    .await
    .ok();
}

#[tokio::test]
async fn test_host_review_queue_apply_writes_only_after_review_decision_and_audits() {
    let memory_root = tempfile::tempdir().expect("memory tempdir");
    let sink = Arc::new(RecordingSink::default());
    let runtime = Arc::new(
        DesktopRuntime::builder()
            .with_event_sink(sink.clone())
            .build(),
    );
    let state = host_commands::DesktopShellState::new(
        Arc::clone(&runtime),
        Arc::new(WorkspaceRegistry::empty()),
        Arc::new(McpToolRegistry::with_builtins()),
        memory_root.path().to_path_buf(),
    );

    host_commands::terminal_open(
        runtime.as_ref(),
        TerminalOpenRequest {
            session_id: Some("review-agent".to_string()),
            command: "cat".to_string(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 24,
            cols: 80,
        },
    )
    .await
    .expect("open review target terminal");

    let staged = host_commands::mcp_invoke(
        &state,
        host_commands::McpInvokeRequest {
            tool: "impulse.review_injection".to_string(),
            arguments: json!({
                "content": "approved context\n",
                "target_agent_id": "review-agent"
            }),
            confirmed: true,
            caller_agent_id: Some("supervisor".to_string()),
        },
    )
    .await
    .expect("stage review payload");
    let id = staged.result["id"].as_str().expect("review id").to_string();

    let queued = host_commands::review_queue(&state)
        .await
        .expect("list review queue");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].status, ReviewQueueStatus::Pending);
    assert!(
        !sink.output_for("review-agent").contains("approved context"),
        "staging must not write to terminal"
    );

    let decision = host_commands::review_decision(
        &state,
        ReviewDecisionRequest {
            id: id.clone(),
            decision: ReviewDecision::Apply,
            target_agent_id: Some("review-agent".to_string()),
            confirmed: true,
        },
    )
    .await
    .expect("apply review payload");

    assert!(decision.ok);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if sink.output_for("review-agent").contains("approved context") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(sink.output_for("review-agent").contains("approved context"));

    let queued = host_commands::review_queue(&state)
        .await
        .expect("list review queue after apply");
    assert_eq!(queued[0].status, ReviewQueueStatus::Applied);
    assert_eq!(queued[0].decision, Some(ReviewDecision::Apply));
    let audit = state.mcp.audit_log();
    assert!(audit
        .iter()
        .any(|item| item.tool == "impulse.review_injection"));
    assert!(audit
        .iter()
        .any(|item| item.tool == "impulse.review_decision"));

    host_commands::terminal_close(
        runtime.as_ref(),
        TerminalCloseRequest {
            session_id: "review-agent".to_string(),
        },
    )
    .await
    .ok();
}

#[tokio::test]
async fn test_host_review_queue_skip_updates_queue_without_terminal_write_and_audits() {
    let memory_root = tempfile::tempdir().expect("memory tempdir");
    let sink = Arc::new(RecordingSink::default());
    let runtime = Arc::new(
        DesktopRuntime::builder()
            .with_event_sink(sink.clone())
            .build(),
    );
    let state = host_commands::DesktopShellState::new(
        Arc::clone(&runtime),
        Arc::new(WorkspaceRegistry::empty()),
        Arc::new(McpToolRegistry::with_builtins()),
        memory_root.path().to_path_buf(),
    );

    let staged = host_commands::mcp_invoke(
        &state,
        host_commands::McpInvokeRequest {
            tool: "impulse.review_injection".to_string(),
            arguments: json!({
                "content": "skipped context\n",
                "target_agent_id": "review-agent"
            }),
            confirmed: true,
            caller_agent_id: Some("supervisor".to_string()),
        },
    )
    .await
    .expect("stage review payload");
    let id = staged.result["id"].as_str().expect("review id").to_string();

    let decision = host_commands::review_decision(
        &state,
        ReviewDecisionRequest {
            id,
            decision: ReviewDecision::Skip,
            target_agent_id: None,
            confirmed: true,
        },
    )
    .await
    .expect("skip review payload");

    assert!(decision.ok);
    assert!(
        !sink.output_for("review-agent").contains("skipped context"),
        "skipping a review item must not write to the terminal"
    );

    let queued = host_commands::review_queue(&state)
        .await
        .expect("list review queue after skip");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].status, ReviewQueueStatus::Skipped);
    assert_eq!(queued[0].decision, Some(ReviewDecision::Skip));

    let audit = state.mcp.audit_log();
    assert!(audit
        .iter()
        .any(|item| item.tool == "impulse.review_injection"));
    assert!(audit
        .iter()
        .any(|item| item.tool == "impulse.review_decision"));
}
