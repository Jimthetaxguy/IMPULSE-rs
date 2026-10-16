#![cfg(unix)]
//! CLI process status and output contract for typed producer refusals.
use impulse_ops::governed_wiring::{
    GovernedProducerAck, GovernedStagedConfigRefusalAck, StagedConfigRefusalReason,
};
use impulse_rs::daemon::{DaemonRequest, DaemonResponse};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

fn staged_accepted_task() -> impulse_ops::governed_task::GovernedTaskRun {
    use impulse_ops::governed_task as gt;
    gt::GovernedTaskRun {
        id: gt::GovernedTaskId::try_new("staged-task").expect("task id"),
        revision: 7,
        project_id: "project".to_string(),
        workspace_root: "/tmp/project".to_string(),
        task: "Prove the cockpit drives the staged controls".to_string(),
        acceptance_criteria: vec!["the gate is green".to_string()],
        approval_policy: gt::ApprovalPolicy::OperatorRequired,
        verification_profile: Some(gt::GovernedVerificationProfile::RustWorkspaceV1),
        role_assignment: None,
        role_compatibility: None,
        runtime_id: "ion".to_string(),
        agent_id: "worker-1".to_string(),
        session_id: None,
        initial_subject_revision: Some("a".repeat(40)),
        world_scope: gt::WorldScope::StagedAuthoritative,
        staged_worktree: Some(gt::StagedWorktree {
            id: gt::GovernedRecordId::try_new("staged-1").expect("staged id"),
            actor: gt::GovernedActor {
                kind: gt::GovernedActorKind::System,
                id: "impulse-daemon:staged_worktree".to_string(),
            },
            root: "/tmp/project/.impulse/worktrees/staged-task".to_string(),
            initial_subject_revision: "a".repeat(40),
            shared_config_digest: gt::SharedRepositoryConfigPin::Unknown,
            status: gt::StagedWorktreeStatus::Active,
            materialized_at: "2026-09-12T00:00:00Z".to_string(),
            based_on_revision: 1,
        }),
        promotions: vec![],
        execution_state: gt::GovernedExecutionState::RuntimeExited,
        review_state: gt::GovernedReviewState::Accepted,
        claims: vec![],
        verifications: vec![],
        supervisor_verdicts: vec![],
        operator_decisions: vec![],
        events: vec![],
        created_at: "2026-09-12T00:00:00Z".to_string(),
        updated_at: "2026-09-12T00:00:00Z".to_string(),
    }
}

fn run_producer(
    command: &str,
    json: bool,
    refused: bool,
) -> (std::process::Output, GovernedStagedConfigRefusalAck) {
    let dir = tempfile::Builder::new()
        .prefix("ipr-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let task = staged_accepted_task();
    let refusal = GovernedStagedConfigRefusalAck::new(
        task.clone(),
        StagedConfigRefusalReason::UnsupportedSubmodules {
            path: "staged/.gitmodules".to_string(),
        },
    );
    let expected_refusal = refusal.clone();
    let command_name = command.to_string();
    let server = thread::spawn(move || {
        for index in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("CLI did not connect to its socket fixture: {error}"),
                }
            };
            // BSD/macOS can inherit the listener's O_NONBLOCK flag on accept.
            // Normalize before cloning: these line reads require blocking I/O,
            // bounded by the existing read timeout rather than request timing.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                let request: DaemonRequest = serde_json::from_str(&line).unwrap();
                let presentation = matches!(request, DaemonRequest::PresentOperatorCapability(_));
                let result = match request {
                    DaemonRequest::PresentOperatorCapability(_) => {
                        serde_json::json!({"connection_class":"operator"})
                    }
                    DaemonRequest::GetGovernedTask { .. } if index == 0 => {
                        serde_json::to_value(&task).unwrap()
                    }
                    request if index == 1 => {
                        assert!(matches!(
                            (command_name.as_str(), request),
                            (
                                "governed-promote",
                                DaemonRequest::PromoteGovernedOutcome { .. }
                            ) | ("governed-claim", DaemonRequest::SubmitGovernedClaim { .. })
                                | (
                                    "governed-verify",
                                    DaemonRequest::RunGovernedVerification { .. }
                                )
                        ));
                        if refused {
                            serde_json::to_value(&refusal).unwrap()
                        } else {
                            let mut updated = task.clone();
                            updated.revision += 1;
                            serde_json::to_value(GovernedProducerAck::new(updated, false, None))
                                .unwrap()
                        }
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                let response = serde_json::to_string(&DaemonResponse::Ok { result }).unwrap();
                writeln!(stream, "{response}").unwrap();
                if !presentation {
                    break;
                }
            }
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_impulse-rs"));
    child
        .args(["--daemon", "--socket"])
        .arg(&socket)
        .arg(command)
        .args(["--project-id", "project", "--task-id", "staged-task"])
        .current_dir(dir.path());
    if command == "governed-claim" {
        child.args(["--summary", "completed fixture"]);
    }
    if json {
        child.arg("--json");
    }
    let output = child.output().unwrap();
    server.join().unwrap();
    (output, expected_refusal)
}

#[test]
fn refusals_keep_typed_output_and_exit_nonzero_for_all_cli_producers() {
    for command in ["governed-promote", "governed-claim", "governed-verify"] {
        for json in [false, true] {
            let (output, refusal) = run_producer(command, json, true);
            assert!(
                !output.status.success(),
                "{command}, json={json} must signal refused work"
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            if json {
                let actual: GovernedStagedConfigRefusalAck =
                    serde_json::from_str(&stdout).expect("stdout remains one typed JSON object");
                assert_eq!(actual, refusal);
            } else {
                assert!(stdout.contains(&refusal.reason.to_string()));
                assert!(stdout.contains(&refusal.remedy));
                assert!(stdout.contains("the task is unchanged"));
            }
            assert!(String::from_utf8_lossy(&output.stderr).contains("the producer did not run"));
        }
    }
}

#[test]
fn recorded_promotion_retains_success_exit_in_text_and_json() {
    for json in [false, true] {
        let (output, _) = run_producer("governed-promote", json, false);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if json {
            let ack: GovernedProducerAck = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(ack.task.revision, 8);
        }
    }
}
