use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use impulse_desktop::{
    AgentPlatformKind, AgentSpawnRequest, AgentWriteRequest, DesktopEvent, DesktopEventSink,
    DesktopRuntime, LocalSupervisorAction, SupervisorLocalActionRequest, TerminalCloseRequest,
    TerminalFocusRequest, TerminalResizeRequest, WorkspaceTarget,
};
use serde_json::json;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<DesktopEvent>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<DesktopEvent> {
        self.events.lock().expect("events mutex poisoned").clone()
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

fn terminal_output(sink: &RecordingSink, agent_id: &str) -> Vec<u8> {
    let mut output = Vec::new();
    for event in sink.events() {
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
    output
}

fn shell_spawn(agent_id: &str, script: &str) -> AgentSpawnRequest {
    AgentSpawnRequest {
        agent_id: Some(agent_id.to_string()),
        session_id: Some(format!("{agent_id}-session")),
        platform: AgentPlatformKind::Shell,
        command: Some("sh".to_string()),
        args: vec!["-lc".to_string(), script.to_string()],
        cwd: None,
        env: HashMap::new(),
        workspace: None,
        mcp_tools: Vec::new(),
        rows: 24,
        cols: 80,
        role: None,
        target: None,
    }
}

#[test]
fn test_desktop_runtime_spawns_shell_and_emits_terminal_output() {
    let sink = Arc::new(RecordingSink::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .build();

    let snapshot = runtime
        .spawn_agent(shell_spawn("shell-one", "printf impulse-ready"))
        .expect("spawn shell agent");

    assert_eq!(snapshot.agent_id, "shell-one");
    assert_eq!(snapshot.platform, AgentPlatformKind::Shell);
    assert_eq!(snapshot.rows, 24);
    assert_eq!(snapshot.cols, 80);

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if sink.events().iter().any(|event| {
            matches!(
                event,
                DesktopEvent::TerminalOutput { agent_id, data }
                    if agent_id == "shell-one" && data.windows(13).any(|window| window == b"impulse-ready")
            )
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    panic!("expected terminal_output event containing shell output");
}

#[test]
fn test_desktop_runtime_snapshot_carries_workspace_and_builtin_mcp_tools() {
    let runtime = DesktopRuntime::default();
    let mut request = shell_spawn("workspace-agent", "sleep 2");
    request.cwd = Some("/tmp".to_string());
    request.workspace = Some(WorkspaceTarget {
        root: "/tmp".to_string(),
        label: Some("scratch".to_string()),
        purpose: Some("safe terminal harness smoke workspace".to_string()),
        project_notes: None,
    });

    let snapshot = runtime
        .spawn_agent(request)
        .expect("spawn workspace-scoped agent");

    assert_eq!(
        snapshot
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.as_str()),
        Some("/tmp")
    );
    assert_eq!(
        snapshot
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.label.as_deref()),
        Some("scratch")
    );
    assert!(snapshot
        .mcp_tools
        .iter()
        .any(|tool| tool.name == "impulse.agent_spawn" && tool.requires_confirmation));
    assert!(snapshot
        .mcp_tools
        .iter()
        .any(|tool| tool.name == "impulse.search_memory" && !tool.requires_confirmation));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "workspace-agent".to_string(),
        })
        .expect("close workspace agent");
}

#[test]
fn test_terminal_harness_spawn_request_binds_workspace_and_default_tools() {
    let request = AgentSpawnRequest::terminal_harness(
        "codex-harness",
        AgentPlatformKind::Codex,
        "/Users/jamespustorino/code",
        32,
        100,
    );

    assert_eq!(request.agent_id.as_deref(), Some("codex-harness"));
    assert_eq!(request.session_id.as_deref(), Some("codex-harness-session"));
    assert_eq!(request.platform, AgentPlatformKind::Codex);
    assert_eq!(request.command, None);
    assert_eq!(request.cwd.as_deref(), Some("/Users/jamespustorino/code"));
    assert_eq!(
        request
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.as_str()),
        Some("/Users/jamespustorino/code")
    );
    assert!(request
        .mcp_tools
        .iter()
        .any(|tool| tool.name == "impulse.review_injection"));
    assert_eq!(request.rows, 32);
    assert_eq!(request.cols, 100);
}

#[test]
fn test_workspace_target_deserializes_without_project_notes() {
    let target: WorkspaceTarget =
        serde_json::from_value(json!({ "root": "/tmp", "label": "scratch" }))
            .expect("legacy workspace target JSON should decode");

    assert_eq!(target.root, "/tmp");
    assert_eq!(target.label.as_deref(), Some("scratch"));
    assert_eq!(target.project_notes, None);
}

#[test]
fn test_runtime_env_exposes_project_metadata_but_not_raw_notes() {
    let notes = "operator note: do not treat this as terminal instructions";
    let sink = Arc::new(RecordingSink::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .build();
    let mut request = shell_spawn(
        "project-env",
        "printf '%s' \"${IMPULSE_PROJECT_NOTES-unset}|${IMPULSE_PROJECT_NOTES_HASH-missing}|${IMPULSE_WORKSPACE_ROOT-missing}|${IMPULSE_PROJECT_LABEL-missing}\"",
    );
    request.cwd = Some("/tmp".to_string());
    request.env.insert(
        "IMPULSE_PROJECT_NOTES_HASH".to_string(),
        "caller-override".to_string(),
    );
    request.env.insert(
        "IMPULSE_WORKSPACE_ROOT".to_string(),
        "/caller/override".to_string(),
    );
    request.workspace = Some(WorkspaceTarget {
        root: "/tmp".to_string(),
        label: Some("scratch".to_string()),
        purpose: Some("safe terminal harness smoke workspace".to_string()),
        project_notes: Some(notes.to_string()),
    });

    runtime
        .spawn_agent(request)
        .expect("spawn project metadata env probe");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let output = String::from_utf8_lossy(&terminal_output(&sink, "project-env")).to_string();
        if output.contains("unset|fnv1a64:")
            && output.contains("|/tmp|scratch")
            && !output.contains("caller-override")
            && !output.contains(notes)
        {
            runtime
                .close_agent(TerminalCloseRequest {
                    session_id: "project-env".to_string(),
                })
                .ok();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    panic!(
        "expected trusted metadata without raw notes, got `{}`",
        String::from_utf8_lossy(&terminal_output(&sink, "project-env"))
    );
}

#[test]
fn test_desktop_runtime_resize_focus_snapshot_and_close() {
    let runtime = DesktopRuntime::default();
    runtime
        .spawn_agent(shell_spawn("shell-two", "sleep 2"))
        .expect("spawn shell agent");

    let resized = runtime
        .resize_agent(TerminalResizeRequest {
            session_id: "shell-two".to_string(),
            rows: 40,
            cols: 120,
        })
        .expect("resize shell agent");
    assert_eq!(resized.rows, 40);
    assert_eq!(resized.cols, 120);

    let focused = runtime
        .focus_agent(TerminalFocusRequest {
            session_id: "shell-two".to_string(),
        })
        .expect("focus shell agent");
    assert!(focused.focused);

    let snapshots = runtime.snapshot_agents();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].agent_id, "shell-two");

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "shell-two".to_string(),
        })
        .expect("close shell agent");
    assert!(runtime.snapshot_agents().is_empty());
}

#[test]
fn test_desktop_runtime_rejects_missing_sessions_for_mutating_commands() {
    let runtime = DesktopRuntime::default();

    let write_error = runtime
        .write_agent(AgentWriteRequest {
            agent_id: "missing-agent".to_string(),
            data: b"pwd\n".to_vec(),
        })
        .expect_err("write should reject a missing agent");
    assert!(write_error
        .to_string()
        .contains("terminal session missing-agent was not found"));

    let resize_error = runtime
        .resize_agent(TerminalResizeRequest {
            session_id: "missing-agent".to_string(),
            rows: 24,
            cols: 80,
        })
        .expect_err("resize should reject a missing agent");
    assert!(resize_error
        .to_string()
        .contains("terminal session missing-agent was not found"));

    let focus_error = runtime
        .focus_agent(TerminalFocusRequest {
            session_id: "missing-agent".to_string(),
        })
        .expect_err("focus should reject a missing agent");
    assert!(focus_error
        .to_string()
        .contains("terminal session missing-agent was not found"));

    let close_error = runtime
        .close_agent(TerminalCloseRequest {
            session_id: "missing-agent".to_string(),
        })
        .expect_err("close should reject a missing agent");
    assert!(close_error
        .to_string()
        .contains("terminal session missing-agent was not found"));
}

#[test]
fn test_desktop_runtime_rejects_invalid_terminal_dimensions() {
    let runtime = DesktopRuntime::default();

    let zero_rows = AgentSpawnRequest {
        rows: 0,
        ..shell_spawn("bad-rows", "sleep 1")
    };
    let error = runtime
        .spawn_agent(zero_rows)
        .expect_err("spawn should reject zero rows");
    assert!(error
        .to_string()
        .contains("terminal rows and cols must be greater than zero"));

    runtime
        .spawn_agent(shell_spawn("resize-target", "sleep 2"))
        .expect("spawn resize target");
    let error = runtime
        .resize_agent(TerminalResizeRequest {
            session_id: "resize-target".to_string(),
            rows: 24,
            cols: 0,
        })
        .expect_err("resize should reject zero cols");
    assert!(error
        .to_string()
        .contains("terminal rows and cols must be greater than zero"));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "resize-target".to_string(),
        })
        .expect("close resize target");
}

#[test]
fn test_desktop_runtime_focus_is_exclusive_across_agents() {
    let runtime = DesktopRuntime::default();
    runtime
        .spawn_agent(shell_spawn("focus-one", "sleep 2"))
        .expect("spawn first focus agent");
    runtime
        .spawn_agent(shell_spawn("focus-two", "sleep 2"))
        .expect("spawn second focus agent");

    runtime
        .focus_agent(TerminalFocusRequest {
            session_id: "focus-one".to_string(),
        })
        .expect("focus first agent");
    runtime
        .focus_agent(TerminalFocusRequest {
            session_id: "focus-two".to_string(),
        })
        .expect("focus second agent");

    let snapshots = runtime.snapshot_agents();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(
        snapshots.iter().filter(|snapshot| snapshot.focused).count(),
        1
    );
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.agent_id == "focus-two" && snapshot.focused));
    assert!(snapshots
        .iter()
        .any(|snapshot| snapshot.agent_id == "focus-one" && !snapshot.focused));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "focus-one".to_string(),
        })
        .expect("close first focus agent");
    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "focus-two".to_string(),
        })
        .expect("close second focus agent");
}

#[test]
fn test_supervisor_local_send_input_requires_confirmation() {
    let runtime = DesktopRuntime::default();
    runtime
        .spawn_agent(shell_spawn("shell-three", "sleep 2"))
        .expect("spawn shell agent");

    let error = runtime
        .dispatch_supervisor_local_action(SupervisorLocalActionRequest {
            action: LocalSupervisorAction::SendInput {
                agent_id: "shell-three".to_string(),
                content: "pwd\n".to_string(),
                confirmed: false,
            },
        })
        .expect_err("unconfirmed supervisor input should be rejected");

    assert!(error
        .to_string()
        .contains("supervisor send_input requires confirmation"));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "shell-three".to_string(),
        })
        .expect("close shell agent");
}

#[test]
fn test_supervisor_local_send_input_writes_only_after_confirmation() {
    let runtime = DesktopRuntime::default();
    runtime
        .spawn_agent(shell_spawn("shell-four", "cat >/dev/null"))
        .expect("spawn shell agent");

    runtime
        .dispatch_supervisor_local_action(SupervisorLocalActionRequest {
            action: LocalSupervisorAction::SendInput {
                agent_id: "shell-four".to_string(),
                content: "echo confirmed\n".to_string(),
                confirmed: true,
            },
        })
        .expect("confirmed supervisor input should write to the agent");

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "shell-four".to_string(),
        })
        .expect("close shell agent");
}
