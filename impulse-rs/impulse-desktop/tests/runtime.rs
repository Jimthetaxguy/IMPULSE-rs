use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use impulse_desktop::{
    AgentPlatformId, AgentSpawnRequest, AgentWriteRequest, DesktopEvent, DesktopEventSink,
    DesktopRuntime, GovernedRoutingMetadata, GovernedTaskGateway, LocalSupervisorAction,
    SupervisorLocalActionRequest, TerminalCloseRequest, TerminalFocusRequest,
    TerminalResizeRequest, WeakDesktopRuntime, WorkspaceTarget,
};
use impulse_ops::governed_task::{
    ApprovalPolicy, GovernedExecutionState, GovernedRequestId, GovernedReviewState, GovernedTaskId,
    GovernedTaskMutation, GovernedTaskMutationRequest, GovernedTaskRegistration, GovernedTaskRun,
};
use impulse_ops::role_assignment::{canonical_governed_builder_assignment, AgentRoleAssignment};
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

#[derive(Default)]
struct ReentrantLifecycleSink {
    events: Mutex<Vec<DesktopEvent>>,
    runtime: Mutex<Option<WeakDesktopRuntime>>,
    reentered: Mutex<bool>,
}

impl DesktopEventSink for ReentrantLifecycleSink {
    fn emit(&self, event: DesktopEvent) {
        self.events
            .lock()
            .expect("reentrant events mutex poisoned")
            .push(event.clone());
        let agent_id = match event {
            DesktopEvent::AgentRuntimeUpdate { snapshot } if snapshot.alive => snapshot.agent_id,
            _ => return,
        };
        let should_reenter = {
            let mut reentered = self.reentered.lock().expect("reentry mutex poisoned");
            if *reentered {
                false
            } else {
                *reentered = true;
                true
            }
        };
        if should_reenter {
            let runtime = self
                .runtime
                .lock()
                .expect("runtime mutex poisoned")
                .as_ref()
                .and_then(WeakDesktopRuntime::upgrade)
                .expect("runtime installed before spawn");
            runtime
                .focus_agent(TerminalFocusRequest {
                    session_id: agent_id,
                })
                .expect("reentrant focus succeeds");
        }
    }
}

struct BlockingOutputSink {
    events: Mutex<Vec<DesktopEvent>>,
    output_started: mpsc::Sender<()>,
    output_release: Mutex<mpsc::Receiver<()>>,
    blocked_once: Mutex<bool>,
}

impl DesktopEventSink for BlockingOutputSink {
    fn emit(&self, event: DesktopEvent) {
        let should_block = if matches!(event, DesktopEvent::TerminalOutput { .. }) {
            let mut blocked = self.blocked_once.lock().expect("blocked mutex poisoned");
            if *blocked {
                false
            } else {
                *blocked = true;
                true
            }
        } else {
            false
        };
        if should_block {
            self.output_started.send(()).expect("signal output start");
            self.output_release
                .lock()
                .expect("release mutex poisoned")
                .recv_timeout(Duration::from_secs(2))
                .expect("release blocked output");
        }
        self.events
            .lock()
            .expect("blocking events mutex poisoned")
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

fn platform_id(value: &str) -> AgentPlatformId {
    AgentPlatformId::try_new(value).expect("valid test platform id")
}

fn shell_spawn(agent_id: &str, script: &str) -> AgentSpawnRequest {
    AgentSpawnRequest {
        agent_id: Some(agent_id.to_string()),
        session_id: Some(format!("{agent_id}-session")),
        platform: platform_id("shell"),
        command: Some("sh".to_string()),
        args: vec!["-lc".to_string(), script.to_string()],
        cwd: None,
        env: HashMap::new(),
        workspace: None,
        mcp_tools: Vec::new(),
        rows: 24,
        cols: 80,
        role: None,
        task: None,
        role_assignment: None,
        acceptance_criteria: Vec::new(),
        verification_profile: None,
        target: None,
    }
}

fn governed_builder_assignment() -> AgentRoleAssignment {
    canonical_governed_builder_assignment()
}

#[derive(Default)]
struct TestGovernedGateway {
    tasks: Mutex<HashMap<GovernedTaskId, GovernedTaskRun>>,
}

impl GovernedTaskGateway for TestGovernedGateway {
    fn register(&self, registration: GovernedTaskRegistration) -> Result<GovernedTaskRun, String> {
        let task = GovernedTaskRun {
            id: registration.task_id,
            revision: 0,
            project_id: registration.project_id,
            workspace_root: registration.workspace_root,
            task: registration.task,
            acceptance_criteria: registration.acceptance_criteria,
            approval_policy: ApprovalPolicy::OperatorRequired,
            verification_profile: registration.verification_profile,
            role_assignment: registration.role_assignment,
            role_compatibility: registration.role_compatibility,
            runtime_id: registration.runtime_id,
            agent_id: registration.agent_id,
            session_id: registration.session_id,
            initial_subject_revision: registration.initial_subject_revision,
            execution_state: GovernedExecutionState::Registered,
            review_state: GovernedReviewState::AwaitingClaim,
            claims: Vec::new(),
            verifications: Vec::new(),
            supervisor_verdicts: Vec::new(),
            operator_decisions: Vec::new(),
            events: Vec::new(),
            created_at: impulse_ops::now_rfc3339(),
            updated_at: impulse_ops::now_rfc3339(),
        };
        self.tasks
            .lock()
            .expect("task gateway mutex poisoned")
            .insert(task.id.clone(), task.clone());
        Ok(task)
    }

    fn mutate(&self, request: GovernedTaskMutationRequest) -> Result<GovernedTaskRun, String> {
        let mut tasks = self.tasks.lock().expect("task gateway mutex poisoned");
        let task = tasks
            .get_mut(&request.task_id)
            .ok_or_else(|| "task missing".to_string())?;
        if task.revision != request.expected_revision {
            return Err("revision conflict".to_string());
        }
        task.execution_state = match request.mutation {
            GovernedTaskMutation::MarkRunning { .. } => GovernedExecutionState::Running,
            GovernedTaskMutation::MarkLaunchFailed { .. } => GovernedExecutionState::LaunchFailed,
            GovernedTaskMutation::MarkRuntimeExited { .. } => GovernedExecutionState::RuntimeExited,
            _ => return Err("unsupported test mutation".to_string()),
        };
        task.revision += 1;
        Ok(task.clone())
    }

    fn mutate_current(
        &self,
        project_id: &str,
        task_id: &GovernedTaskId,
        request_id: GovernedRequestId,
        mutation: GovernedTaskMutation,
    ) -> Result<GovernedTaskRun, String> {
        let current = self
            .tasks
            .lock()
            .expect("task gateway mutex poisoned")
            .get(task_id)
            .cloned()
            .ok_or_else(|| "task missing".to_string())?;
        self.mutate(GovernedTaskMutationRequest {
            request_id,
            project_id: project_id.to_string(),
            task_id: task_id.clone(),
            expected_revision: current.revision,
            mutation,
        })
    }

    fn routing_metadata(&self) -> Option<GovernedRoutingMetadata> {
        Some(GovernedRoutingMetadata {
            socket_path: "/tmp/impulse-test.sock".to_string(),
            control_cli: "/tmp/impulse-test-cli".to_string(),
        })
    }
}

#[test]
fn test_desktop_event_names_are_advertised_by_host_manifest() {
    let runtime = DesktopRuntime::default();
    let snapshot = runtime
        .spawn_agent(shell_spawn("event-name-agent", "true"))
        .expect("spawn event-name agent");

    let events = [
        DesktopEvent::TerminalOutput {
            agent_id: "event-name-agent".to_string(),
            data: b"ok".to_vec(),
        },
        DesktopEvent::TerminalExit {
            agent_id: "event-name-agent".to_string(),
        },
        DesktopEvent::AgentRuntimeUpdate {
            snapshot: Box::new(snapshot),
        },
        DesktopEvent::OpsUpdate { payload: json!({}) },
        DesktopEvent::OpsConnectionUpdate {
            connected: true,
            error: None,
        },
        DesktopEvent::SupervisorLocalAction {
            action: LocalSupervisorAction::FocusAgent {
                agent_id: "event-name-agent".to_string(),
            },
        },
    ];

    for event in events {
        assert!(
            DesktopEvent::HOST_EVENT_NAMES.contains(&event.name()),
            "host manifest missing runtime event {}",
            event.name()
        );
    }
}

#[test]
fn test_immediate_exit_never_emits_a_live_snapshot_after_terminal_exit() {
    let sink = Arc::new(RecordingSink::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .build();
    for attempt in 0..16 {
        let agent_id = format!("immediate-exit-{attempt}");
        runtime
            .spawn_agent(shell_spawn(&agent_id, "exit 0"))
            .expect("spawn immediate-exit agent");

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && !sink.events().iter().any(|event| {
                matches!(
                    event,
                    DesktopEvent::TerminalExit { agent_id: exited } if exited == &agent_id
                )
            })
        {
            std::thread::sleep(Duration::from_millis(5));
        }

        let events = sink.events();
        let exit_index = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    DesktopEvent::TerminalExit { agent_id: exited } if exited == &agent_id
                )
            })
            .expect("immediate process emitted terminal_exit");
        assert!(
            !events[exit_index + 1..].iter().any(|event| {
                matches!(
                    event,
                    DesktopEvent::AgentRuntimeUpdate { snapshot }
                        if snapshot.agent_id == agent_id && snapshot.alive
                )
            }),
            "attempt {attempt} resurrected a dead agent: {events:?}"
        );
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !runtime.snapshot_agents().is_empty() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        runtime.snapshot_agents().is_empty(),
        "naturally exited runtimes must be reaped"
    );
}

#[test]
fn test_runtime_rejects_active_duplicate_and_closed_id_reuse() {
    let runtime = DesktopRuntime::default();
    runtime
        .spawn_agent(shell_spawn("reserved-id", "sleep 2"))
        .expect("spawn first incarnation");

    let duplicate = runtime
        .spawn_agent(shell_spawn("reserved-id", "sleep 2"))
        .expect_err("active duplicate id must be rejected");
    assert!(duplicate.to_string().contains("has already been used"));
    assert_eq!(runtime.snapshot_agents().len(), 1);

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "reserved-id".to_string(),
        })
        .expect("close first incarnation");
    let reused = runtime
        .spawn_agent(shell_spawn("reserved-id", "sleep 2"))
        .expect_err("closed id must stay reserved against stale callbacks");
    assert!(reused.to_string().contains("has already been used"));
    assert!(runtime.snapshot_agents().is_empty());
}

#[test]
fn test_lifecycle_dispatch_allows_sink_reentry_without_deadlock() {
    let sink = Arc::new(ReentrantLifecycleSink::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .build();
    *sink.runtime.lock().expect("runtime mutex poisoned") = Some(runtime.downgrade());

    runtime
        .spawn_agent(shell_spawn("reentrant-agent", "sleep 2"))
        .expect("spawn reentrant agent");

    let snapshots = runtime.snapshot_agents();
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots[0].focused);
    let events = sink.events.lock().expect("events mutex poisoned");
    let runtime_updates = events
        .iter()
        .filter_map(|event| match event {
            DesktopEvent::AgentRuntimeUpdate { snapshot } => Some(snapshot),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(runtime_updates.len(), 2);
    assert!(!runtime_updates[0].focused);
    assert!(runtime_updates[1].focused);
    drop(events);

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: "reentrant-agent".to_string(),
        })
        .expect("close reentrant agent");
}

#[test]
fn test_manual_close_never_delivers_buffered_output_after_exit() {
    let (output_started_tx, output_started_rx) = mpsc::channel();
    let (output_release_tx, output_release_rx) = mpsc::channel();
    let sink = Arc::new(BlockingOutputSink {
        events: Mutex::new(Vec::new()),
        output_started: output_started_tx,
        output_release: Mutex::new(output_release_rx),
        blocked_once: Mutex::new(false),
    });
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .build();
    let spawn_runtime = runtime.clone();
    let spawn_thread = std::thread::spawn(move || {
        spawn_runtime.spawn_agent(shell_spawn(
            "buffered-output-agent",
            "printf buffered-output; sleep 30",
        ))
    });
    output_started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("reader reached output sink");

    let closer = runtime.clone();
    let close_thread = std::thread::spawn(move || {
        closer.close_agent(TerminalCloseRequest {
            session_id: "buffered-output-agent".to_string(),
        })
    });
    close_thread
        .join()
        .expect("close thread")
        .expect("close buffered-output agent");
    output_release_tx.send(()).expect("release output sink");
    spawn_thread
        .join()
        .expect("spawn thread")
        .expect("spawn buffered-output agent");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !sink
            .events
            .lock()
            .expect("events mutex poisoned")
            .iter()
            .any(|event| {
                matches!(
                    event,
                    DesktopEvent::TerminalExit { agent_id }
                        if agent_id == "buffered-output-agent"
                )
            })
    {
        std::thread::sleep(Duration::from_millis(5));
    }

    let events = sink.events.lock().expect("events mutex poisoned");
    let exit_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                DesktopEvent::TerminalExit { agent_id }
                    if agent_id == "buffered-output-agent"
            )
        })
        .expect("terminal_exit delivered");
    assert!(events[..exit_index].iter().any(|event| matches!(
        event,
        DesktopEvent::TerminalOutput { agent_id, .. }
            if agent_id == "buffered-output-agent"
    )));
    assert!(!events[exit_index + 1..].iter().any(|event| matches!(
        event,
        DesktopEvent::TerminalOutput { agent_id, .. }
            if agent_id == "buffered-output-agent"
    )));
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
    assert_eq!(snapshot.platform, platform_id("shell"));
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
fn test_ungoverned_cross_workspace_spawn_does_not_receive_daemon_control_routing() {
    const HELPER_ENV: &str = "IMPULSE_ROUTING_ISOLATION_TEST_HELPER";
    if std::env::var_os(HELPER_ENV).is_none() {
        let output = std::process::Command::new(
            std::env::current_exe().expect("locate desktop runtime test binary"),
        )
        .args([
            "--exact",
            "test_ungoverned_cross_workspace_spawn_does_not_receive_daemon_control_routing",
            "--nocapture",
        ])
        .env(HELPER_ENV, "1")
        .env("IMPULSE_SOCKET_PATH", "/tmp/parent-leak.sock")
        .env("IMPULSE_CONTROL_CLI", "/bin/echo")
        .output()
        .expect("run routing-isolation subprocess");
        assert!(
            output.status.success(),
            "poisoned-parent routing isolation failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let workspace = tempfile::tempdir().expect("temporary ungoverned workspace");
    let sink = Arc::new(RecordingSink::default());
    let gateway: Arc<dyn GovernedTaskGateway> = Arc::new(TestGovernedGateway::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .with_governed_task_gateway(gateway)
        .build();
    let mut request = shell_spawn(
        "ungoverned-cross-workspace",
        "printf '%s|%s' \"${IMPULSE_SOCKET_PATH-unset}\" \"${IMPULSE_CONTROL_CLI-unset}\"; sleep 0.1",
    );
    request.cwd = Some(workspace.path().display().to_string());
    request.workspace = Some(WorkspaceTarget::from_root(
        workspace.path().display().to_string(),
    ));

    runtime
        .spawn_agent(request)
        .expect("ordinary cross-workspace agent should launch");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let output = String::from_utf8_lossy(&terminal_output(&sink, "ungoverned-cross-workspace"))
            .to_string();
        if output.contains("unset|unset") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ordinary agent output did not prove routing isolation: {output:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn test_governed_spawn_binds_symlink_equivalent_workspace_to_canonical_pty_directory() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temporary workspace parent");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).expect("create governed workspace");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(args)
            .status()
            .expect("run Git fixture command");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Impulse Test"]);
    git(&["commit", "--allow-empty", "--quiet", "-m", "initial"]);
    let alias = temp.path().join("workspace-alias");
    symlink(&workspace, &alias).expect("create workspace symlink");
    let canonical = workspace.canonicalize().expect("canonical workspace");
    let canonical_text = canonical.display().to_string();

    let sink = Arc::new(RecordingSink::default());
    let gateway: Arc<dyn GovernedTaskGateway> = Arc::new(TestGovernedGateway::default());
    let runtime = DesktopRuntime::builder()
        .with_event_sink(sink.clone())
        .with_governed_task_gateway(gateway)
        .build();
    let mut request = shell_spawn(
        "canonical-workspace-agent",
        "printf '%s|%s|%s|%s|%s|%s|%s' \"$PWD\" \"$IMPULSE_WORKSPACE_ROOT\" \"$IMPULSE_GOVERNED_TASK_ID\" \"$IMPULSE_PROJECT_ID\" \"$IMPULSE_SOCKET_PATH\" \"$IMPULSE_CONTROL_CLI\" \"$IMPULSE_GOVERNED_VERIFICATION_PROFILE\"",
    );
    request.cwd = Some(alias.display().to_string());
    request.workspace = Some(WorkspaceTarget {
        root: workspace.display().to_string(),
        label: Some("governed workspace".to_string()),
        purpose: None,
        project_notes: None,
    });
    request.task = Some("prove canonical workspace binding".to_string());
    request.role_assignment = Some(governed_builder_assignment());
    request.acceptance_criteria = vec!["routing metadata is exact".to_string()];
    request.verification_profile =
        Some(impulse_ops::governed_task::GovernedVerificationProfile::RustWorkspaceV1);

    let snapshot = runtime
        .spawn_agent(request)
        .expect("symlink-equivalent paths must bind to one canonical workspace");
    assert_eq!(snapshot.cwd.as_deref(), Some(canonical_text.as_str()));
    assert_eq!(
        snapshot
            .workspace
            .as_ref()
            .map(|target| target.root.as_str()),
        Some(canonical_text.as_str())
    );
    let telemetry = impulse_desktop::daemon_ops::agent_runtime_from_snapshot(&snapshot);
    assert_eq!(telemetry.working_directory, canonical_text);
    assert_eq!(telemetry.governed_task_id, snapshot.governed_task_id);

    let expected_output = format!(
        "{canonical_text}|{canonical_text}|{}|workspace|/tmp/impulse-test.sock|/tmp/impulse-test-cli|rust_workspace_v1",
        snapshot
            .governed_task_id
            .as_ref()
            .expect("governed task id")
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let output = String::from_utf8_lossy(&terminal_output(&sink, "canonical-workspace-agent"))
            .to_string();
        if output.contains(&expected_output) {
            runtime
                .close_agent(TerminalCloseRequest {
                    session_id: snapshot.agent_id,
                })
                .ok();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    panic!(
        "expected PTY cwd and trusted workspace env to use `{canonical_text}`, got `{}`",
        String::from_utf8_lossy(&terminal_output(&sink, "canonical-workspace-agent"))
    );
}

#[test]
fn test_terminal_harness_spawn_request_binds_workspace_and_default_tools() {
    let request = AgentSpawnRequest::terminal_harness(
        "codex-harness",
        platform_id("codex"),
        "/workspace",
        32,
        100,
    );

    assert_eq!(request.agent_id.as_deref(), Some("codex-harness"));
    assert_eq!(request.session_id.as_deref(), Some("codex-harness-session"));
    assert_eq!(request.platform, platform_id("codex"));
    assert_eq!(request.command, None);
    assert_eq!(request.cwd.as_deref(), Some("/workspace"));
    assert_eq!(
        request
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.as_str()),
        Some("/workspace")
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
fn test_agent_spawn_request_accepts_open_registry_ids_with_plain_string_wire_shape() {
    for platform in ["codex", "ion", "custom-agent"] {
        let request: AgentSpawnRequest = serde_json::from_value(json!({
            "platform": platform,
            "rows": 24,
            "cols": 80
        }))
        .expect("registered and custom platform ids must use the same open string wire shape");

        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["platform"], platform);
    }
}

#[test]
fn test_desktop_runtime_rejects_unknown_explicit_platform_before_spawning() {
    let request: AgentSpawnRequest = serde_json::from_value(json!({
        "agent_id": "unknown-platform",
        "platform": "missing-agent",
        "command": "sh",
        "args": ["-lc", "true"],
        "rows": 24,
        "cols": 80
    }))
    .expect("open platform identity must decode before registry validation");

    let error = DesktopRuntime::default()
        .spawn_agent(request)
        .expect_err("unknown explicit platform must fail closed");
    assert!(error.to_string().contains("unknown agent platform"));
}

#[test]
fn test_desktop_runtime_canonicalizes_registered_platform_identity() {
    let runtime = DesktopRuntime::default();
    let mut request = shell_spawn("canonical-platform", "sleep 2");
    request.platform = platform_id("ShElL");

    let snapshot = runtime
        .spawn_agent(request)
        .expect("case-insensitive registered identity should launch");
    assert_eq!(snapshot.platform, platform_id("shell"));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: snapshot.agent_id,
        })
        .expect("close canonicalization test agent");
}

#[test]
fn test_desktop_runtime_canonicalizes_registered_alias_to_platform_id() {
    let runtime = DesktopRuntime::default();
    let mut request = shell_spawn("canonical-alias", "sleep 2");
    request.platform = platform_id("generic_shell");

    let snapshot = runtime
        .spawn_agent(request)
        .expect("registered alias should launch through its owning platform");
    assert_eq!(snapshot.platform, platform_id("shell"));

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: snapshot.agent_id,
        })
        .expect("close alias canonicalization test agent");
}

#[test]
#[ignore = "requires `cargo build -p impulse-rs --bin ion` in the shared target directory"]
fn test_desktop_runtime_spawns_real_default_ion_sibling_without_override() {
    let current_exe = std::env::current_exe().expect("locate desktop runtime test binary");
    let deps_dir = current_exe.parent().expect("test binary parent");
    let target_profile = if deps_dir.file_name().and_then(|name| name.to_str()) == Some("deps") {
        deps_dir.parent().expect("target profile directory")
    } else {
        deps_dir
    };
    let ion = target_profile.join(format!("ion{}", std::env::consts::EXE_SUFFIX));
    assert!(
        ion.is_file(),
        "build the real Ion binary before running this integration test: {}",
        ion.display()
    );

    let runtime = DesktopRuntime::default();
    let request = AgentSpawnRequest {
        agent_id: Some("real-ion-sibling".to_string()),
        session_id: Some("real-ion-sibling-session".to_string()),
        platform: platform_id("ion"),
        command: None,
        args: Vec::new(),
        cwd: None,
        env: HashMap::new(),
        workspace: None,
        mcp_tools: Vec::new(),
        rows: 24,
        cols: 80,
        role: None,
        task: None,
        role_assignment: None,
        acceptance_criteria: Vec::new(),
        verification_profile: None,
        target: None,
    };

    let snapshot = runtime
        .spawn_agent(request)
        .expect("launch real Ion through the desktop PTY runtime");
    assert_eq!(snapshot.platform, platform_id("ion"));
    assert_eq!(snapshot.command, ion.to_string_lossy());
    assert!(snapshot.alive);

    runtime
        .close_agent(TerminalCloseRequest {
            session_id: snapshot.agent_id,
        })
        .expect("close real Ion integration process");
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
