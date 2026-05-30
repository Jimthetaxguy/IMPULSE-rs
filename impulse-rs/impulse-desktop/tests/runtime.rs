use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use impulse_desktop::{
    AgentPlatformKind, AgentSpawnRequest, AgentWriteRequest, DesktopEvent, DesktopEventSink,
    DesktopRuntime, LocalSupervisorAction, SupervisorLocalActionRequest, TerminalCloseRequest,
    TerminalFocusRequest, TerminalResizeRequest,
};

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

fn shell_spawn(agent_id: &str, script: &str) -> AgentSpawnRequest {
    AgentSpawnRequest {
        agent_id: Some(agent_id.to_string()),
        session_id: Some(format!("{agent_id}-session")),
        platform: AgentPlatformKind::Shell,
        command: Some("sh".to_string()),
        args: vec!["-lc".to_string(), script.to_string()],
        cwd: None,
        env: HashMap::new(),
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
