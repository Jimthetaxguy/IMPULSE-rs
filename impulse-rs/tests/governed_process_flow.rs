#![cfg(unix)]

//! Real-process proof for the daemon-owned Builder claim and verification path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use impulse_ops::agent_registry::{AgentPlatformId, AgentRegistry};
use impulse_ops::governed_task::{
    GovernedActor, GovernedActorKind, GovernedCommandEvidence, GovernedRequestId,
    GovernedReviewState, GovernedSupervisorReviewRequest, GovernedTaskMutation,
    GovernedTaskMutationRequest, GovernedTaskRegistration, GovernedTaskRun,
    GovernedVerificationInput, GovernedVerificationOutcome, GovernedVerificationProfile,
    OperatorDecisionInput, OperatorDecisionKind, SupervisorVerdictInput, SupervisorVerdictKind,
    WorkerCompletionClaimInput,
};
use impulse_ops::memory_candidate::{AcceptedRunSourceAssurance, MemoryCandidateStatus};
use impulse_ops::role_assignment::canonical_governed_builder_assignment;
use impulse_ops::ProjectOpsSnapshot;
use impulse_rs::client::DaemonClient;
use impulse_rs::daemon::{DaemonRequest, DaemonResponse};
use serde::de::DeserializeOwned;

const IMPULSE_BIN: &str = env!("CARGO_BIN_EXE_impulse-rs");

struct DaemonGuard {
    child: Option<Child>,
}

impl DaemonGuard {
    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("Git command must launch");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("Git output must be UTF-8")
        .trim()
        .to_string()
}

fn init_rust_repo() -> (tempfile::TempDir, String, String) {
    let repo = tempfile::Builder::new()
        .prefix("igp-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"governed_process_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("build.rs"),
        "fn main() {\n    assert!(!std::path::Path::new(\"ignored-poison\").exists());\n}\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn governed_process_fixture() -> bool {\n    true\n}\n",
    )
    .unwrap();
    std::fs::write(repo.path().join(".gitignore"), "target/\nignored-poison\n").unwrap();
    let lock_status = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(repo.path())
        .status()
        .expect("Cargo lockfile generation must launch");
    assert!(lock_status.success(), "Cargo lockfile generation failed");
    run_git(repo.path(), &["init", "--quiet"]);
    run_git(repo.path(), &["config", "user.email", "test@example.com"]);
    run_git(repo.path(), &["config", "user.name", "Impulse Test"]);
    let init = Command::new(IMPULSE_BIN)
        .arg("-c")
        .arg(repo.path().join(".impulse"))
        .arg("init")
        .current_dir(repo.path())
        .output()
        .expect("Impulse init must launch");
    assert!(
        init.status.success(),
        "Impulse init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let gitignore = std::fs::read_to_string(repo.path().join(".gitignore")).unwrap();
    assert!(gitignore
        .lines()
        .any(|line| line == ".impulse/LIVE_STATE.json"));
    assert!(!gitignore.lines().any(|line| line == ".impulse/"));
    run_git(
        repo.path(),
        &[
            "add",
            ".gitignore",
            ".impulse/GENOME.md",
            ".impulse/config.json",
            ".impulse/impulse-capabilities.json",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "src/lib.rs",
        ],
    );
    run_git(repo.path(), &["commit", "--quiet", "-m", "initial"]);
    assert_eq!(
        run_git(repo.path(), &["status", "--porcelain"]),
        "",
        "fresh init plus committed durable artifacts must leave a clean subject"
    );
    let oid = run_git(repo.path(), &["rev-parse", "HEAD"]);
    let project_id = impulse_ops::sanitize_id(
        &repo
            .path()
            .file_name()
            .expect("temporary repo has a name")
            .to_string_lossy(),
    );
    (repo, project_id, oid)
}

fn start_daemon(repo: &Path) -> (DaemonGuard, PathBuf) {
    let impulse_dir = repo.join(".impulse");
    let socket = impulse_dir.join("sockets").join("impulse.sock");
    let mut child = Command::new(IMPULSE_BIN)
        .args(["-c"])
        .arg(&impulse_dir)
        .arg("daemon")
        .current_dir(repo)
        .env("IMPULSE_TEST_MODE", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("daemon must launch");
    for _ in 0..200 {
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return (DaemonGuard { child: Some(child) }, socket);
        }
        if let Some(status) = child.try_wait().expect("poll daemon child") {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("daemon exited as {status}: {stderr}");
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("daemon socket did not become ready at {}", socket.display());
}

fn task_from_response(response: DaemonResponse) -> GovernedTaskRun {
    ok_from_response(response)
}

fn ok_from_response<T: DeserializeOwned>(response: DaemonResponse) -> T {
    match response {
        DaemonResponse::Ok { result } => serde_json::from_value(result).unwrap(),
        other => panic!("expected successful daemon response, received {other:?}"),
    }
}

fn request_id(value: &str) -> GovernedRequestId {
    GovernedRequestId::try_new(value).unwrap()
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn read_optional(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("failed to read {}: {error}", path.display()),
    }
}

async fn mutate_task(
    client: &DaemonClient,
    task: &GovernedTaskRun,
    mutation_request_id: &str,
    mutation: GovernedTaskMutation,
) -> GovernedTaskRun {
    task_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: GovernedTaskMutationRequest {
                    request_id: request_id(mutation_request_id),
                    project_id: task.project_id.clone(),
                    task_id: task.id.clone(),
                    expected_revision: task.revision,
                    mutation,
                },
            })
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn real_daemon_builder_cli_verification_and_restart_preserve_evidence() {
    let (repo, project_id, oid) = init_rust_repo();
    let genome_path = repo.path().join(".impulse/GENOME.md");
    let history_path = repo.path().join(".impulse/HISTORY.jsonl");
    let genome_before = std::fs::read(&genome_path).unwrap();
    let history_before = read_optional(&history_path);
    let (mut daemon, socket) = start_daemon(repo.path());
    assert_eq!(
        run_git(repo.path(), &["status", "--porcelain"]),
        "",
        "daemon runtime artifacts must remain covered by init's exact ignores"
    );
    let client = DaemonClient::new(socket.clone());
    let assignment = canonical_governed_builder_assignment();
    let compatibility = AgentRegistry::builtin()
        .evaluate_role_compatibility(&AgentPlatformId::try_new("ion").unwrap(), &assignment)
        .unwrap();
    let registration = GovernedTaskRegistration::builder(
        "process-register",
        "process-task",
        project_id.clone(),
        repo.path().display().to_string(),
        "prove the process-level governed producer bridge",
        "process-builder",
        "ion",
    )
    .acceptance_criteria(vec!["the committed Rust workspace passes".to_string()])
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(oid.clone())
    .role_assignment(assignment)
    .role_compatibility(compatibility)
    .build()
    .unwrap();
    let registered = task_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );
    let running = task_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: GovernedTaskMutationRequest {
                    request_id: request_id("process-running"),
                    project_id: project_id.clone(),
                    task_id: registered.id.clone(),
                    expected_revision: registered.revision,
                    mutation: GovernedTaskMutation::MarkRunning {
                        actor: GovernedActor {
                            kind: GovernedActorKind::System,
                            id: "process-test-desktop".to_string(),
                        },
                    },
                },
            })
            .await
            .unwrap(),
    );

    let claim = Command::new("sh")
        .args([
            "-c",
            "\"$IMPULSE_CONTROL_CLI\" --daemon governed-claim --summary 'real Builder child completed the committed fixture' --json",
        ])
        .current_dir(repo.path())
        .env("IMPULSE_CONTROL_CLI", IMPULSE_BIN)
        .env("IMPULSE_SOCKET_PATH", &socket)
        .env("IMPULSE_PROJECT_ID", &project_id)
        .env("IMPULSE_GOVERNED_TASK_ID", running.id.as_str())
        .output()
        .expect("Builder claim CLI must launch");
    assert!(
        claim.status.success(),
        "claim failed: {}",
        String::from_utf8_lossy(&claim.stderr)
    );
    let claimed: GovernedTaskRun = serde_json::from_slice(&claim.stdout).unwrap();
    assert_eq!(claimed.latest_claim().unwrap().actor.id, "process-builder");
    assert_eq!(claimed.latest_claim().unwrap().subject_revision, oid);

    // This ignored live-worktree input would make the committed build script
    // fail if verification reused Builder bytes. Detached subject materialization
    // deliberately excludes it.
    std::fs::write(repo.path().join("ignored-poison"), "must not be verified").unwrap();

    let verify = Command::new("sh")
        .args([
            "-c",
            "\"$IMPULSE_CONTROL_CLI\" --daemon governed-verify --json",
        ])
        .current_dir(repo.path())
        .env("IMPULSE_CONTROL_CLI", IMPULSE_BIN)
        .env("IMPULSE_SOCKET_PATH", &socket)
        .env("IMPULSE_PROJECT_ID", &project_id)
        .env("IMPULSE_GOVERNED_TASK_ID", running.id.as_str())
        .output()
        .expect("verification CLI must launch");
    assert!(
        verify.status.success(),
        "verification failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let verified: GovernedTaskRun = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(
        verified.review_state,
        GovernedReviewState::AwaitingSupervisor
    );
    assert_eq!(verified.latest_verification().unwrap().commands.len(), 4);

    let review_error = client
        .run_governed_supervisor_review(GovernedSupervisorReviewRequest {
            request_id: request_id("process-supervisor-review"),
            project_id: project_id.clone(),
            task_id: verified.id.clone(),
            expected_revision: verified.revision,
        })
        .await
        .expect_err("profiled review must fail closed without a configured API Supervisor");
    assert!(
        review_error
            .to_string()
            .contains("Impulse Agent must be configured"),
        "unexpected Supervisor preflight error: {review_error:#}"
    );
    assert_eq!(
        client
            .get_governed_task(project_id.clone(), verified.id.clone())
            .await
            .unwrap(),
        Some(verified.clone()),
        "a failed Supervisor producer turn must not advance governed truth"
    );
    let before_restart: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert!(
        before_restart.memory_candidates.is_empty(),
        "verification alone must never stage a durable memory candidate"
    );

    daemon.stop();
    let (restarted, restarted_socket) = start_daemon(repo.path());
    let restarted_client = DaemonClient::new(restarted_socket);
    let persisted = restarted_client
        .get_governed_task(project_id, verified.id.clone())
        .await
        .unwrap()
        .expect("verified task must survive daemon restart");
    assert_eq!(persisted, verified);
    let restarted_snapshot: ProjectOpsSnapshot = ok_from_response(
        restarted_client
            .send(DaemonRequest::GetOpsSnapshot)
            .await
            .unwrap(),
    );
    assert!(restarted_snapshot.memory_candidates.is_empty());
    assert_eq!(std::fs::read(&genome_path).unwrap(), genome_before);
    assert_eq!(read_optional(&history_path), history_before);
    drop(restarted);
}

#[tokio::test]
async fn real_daemon_accepted_run_candidate_is_stable_across_replay_and_restart() {
    let (repo, project_id, oid) = init_rust_repo();
    let genome_path = repo.path().join(".impulse/GENOME.md");
    let history_path = repo.path().join(".impulse/HISTORY.jsonl");
    let genome_before = std::fs::read(&genome_path).unwrap();
    let history_before = read_optional(&history_path);
    let (mut daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket);

    let registration = GovernedTaskRegistration::builder(
        "candidate-register",
        "candidate-task",
        project_id.clone(),
        repo.path().display().to_string(),
        "preserve accepted evidence as a review-only memory candidate",
        "candidate-builder",
        "codex",
    )
    .acceptance_criteria(vec![
        "exactly one stable candidate survives replay and restart".to_string(),
    ])
    .initial_subject_revision(oid.clone())
    .build()
    .unwrap();
    let registered = task_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );
    let running = mutate_task(
        &client,
        &registered,
        "candidate-running",
        GovernedTaskMutation::MarkRunning {
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "candidate-process-test".to_string(),
            },
        },
    )
    .await;

    let worker_prose = "worker-private-prose-must-not-become-memory";
    let claimed = mutate_task(
        &client,
        &running,
        "candidate-claim",
        GovernedTaskMutation::SubmitClaim {
            claim: WorkerCompletionClaimInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Worker,
                    id: "candidate-builder".to_string(),
                },
                summary: worker_prose.to_string(),
                subject_revision: oid.clone(),
                artifact_ids: vec!["candidate-claim-artifact".to_string()],
                diff_ref: Some(format!("git:{oid}")),
            },
        },
    )
    .await;
    let verified = mutate_task(
        &client,
        &claimed,
        "candidate-verify",
        GovernedTaskMutation::RecordVerification {
            verification: GovernedVerificationInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Verifier,
                    id: "candidate-process-verifier".to_string(),
                },
                claim_id: claimed.latest_claim().unwrap().id.clone(),
                subject_revision: oid,
                policy: "process-caller-composed-v1".to_string(),
                outcome: GovernedVerificationOutcome::Passed,
                commands: vec![GovernedCommandEvidence {
                    name: "cargo test --locked".to_string(),
                    executable: "cargo".to_string(),
                    redacted_args: vec!["test".to_string(), "--locked".to_string()],
                    command_digest: digest('a'),
                    exit_code: Some(0),
                    success: true,
                    output_digest: digest('b'),
                    output_ref: Some("evidence/candidate-process-test.log".to_string()),
                    output_bytes: 128,
                    output_truncated: false,
                }],
                artifact_ids: vec!["candidate-verification-artifact".to_string()],
                notes: None,
            },
        },
    )
    .await;

    let supervisor_prose = "supervisor-private-rationale-must-not-become-memory";
    let reviewed = mutate_task(
        &client,
        &verified,
        "candidate-review",
        GovernedTaskMutation::RecordSupervisorVerdict {
            verdict: SupervisorVerdictInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Supervisor,
                    id: "candidate-process-supervisor".to_string(),
                },
                verification_id: verified.latest_verification().unwrap().id.clone(),
                verdict: SupervisorVerdictKind::RecommendAccept,
                rationale: supervisor_prose.to_string(),
            },
        },
    )
    .await;
    let before_approval: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert!(before_approval.memory_candidates.is_empty());

    let operator_prose = "operator-private-rationale-must-not-become-memory";
    let approval_request = GovernedTaskMutationRequest {
        request_id: request_id("candidate-approve"),
        project_id: project_id.clone(),
        task_id: reviewed.id.clone(),
        expected_revision: reviewed.revision,
        mutation: GovernedTaskMutation::RecordOperatorDecision {
            decision: OperatorDecisionInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Operator,
                    id: "candidate-process-operator".to_string(),
                },
                supervisor_verdict_id: reviewed.latest_supervisor_verdict().unwrap().id.clone(),
                decision: OperatorDecisionKind::Approve,
                rationale: operator_prose.to_string(),
            },
        },
    };
    let accepted = task_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: approval_request.clone(),
            })
            .await
            .unwrap(),
    );
    assert_eq!(accepted.review_state, GovernedReviewState::Accepted);

    let accepted_snapshot: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert_eq!(accepted_snapshot.memory_candidates.len(), 1);
    let candidate = accepted_snapshot.memory_candidates[0].clone();
    candidate.validate_shape().unwrap();
    assert_eq!(candidate.status, MemoryCandidateStatus::PendingReview);
    assert_eq!(
        candidate.source_assurance,
        AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator
    );
    assert_eq!(candidate.governed_task_id, accepted.id);
    assert_eq!(candidate.accepted_task_revision, accepted.revision);
    assert_eq!(
        candidate.operator_decision_id,
        accepted.operator_decisions.last().unwrap().id
    );
    let serialized_candidate = serde_json::to_string(&candidate).unwrap();
    for excluded_prose in [worker_prose, supervisor_prose, operator_prose] {
        assert!(
            !serialized_candidate.contains(excluded_prose),
            "review candidate leaked excluded semantic prose: {excluded_prose}"
        );
    }

    let replayed = task_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: approval_request,
            })
            .await
            .unwrap(),
    );
    assert_eq!(replayed, accepted);
    let replay_snapshot: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert_eq!(replay_snapshot.memory_candidates, vec![candidate.clone()]);

    daemon.stop();
    let (restarted, restarted_socket) = start_daemon(repo.path());
    let restarted_client = DaemonClient::new(restarted_socket);
    assert_eq!(
        restarted_client
            .get_governed_task(project_id, accepted.id.clone())
            .await
            .unwrap(),
        Some(accepted)
    );
    let restarted_snapshot: ProjectOpsSnapshot = ok_from_response(
        restarted_client
            .send(DaemonRequest::GetOpsSnapshot)
            .await
            .unwrap(),
    );
    assert_eq!(restarted_snapshot.memory_candidates, vec![candidate]);
    assert_eq!(std::fs::read(&genome_path).unwrap(), genome_before);
    assert_eq!(read_optional(&history_path), history_before);
    drop(restarted);
}
