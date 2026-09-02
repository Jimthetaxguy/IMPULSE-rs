#![cfg(unix)]

//! Real-daemon proof for ADR-0018 socket actor provenance.
//!
//! The threat these tests pin down: any same-user process that can reach the
//! daemon socket — including a launched Builder holding `IMPULSE_SOCKET_PATH` —
//! used to be able to send `RecordOperatorDecision` and mint `accepted` for its
//! own governed task. The "builder" here is a raw socket connection that never
//! presents the operator capability, which is exactly what a scrubbed governed
//! pane can do.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use impulse_ops::governed_task::{
    GovernedActor, GovernedActorKind, GovernedCommandEvidence, GovernedRequestId,
    GovernedReviewState, GovernedTaskMutation, GovernedTaskMutationRequest,
    GovernedTaskRegistration, GovernedTaskRun, GovernedVerificationInput,
    GovernedVerificationOutcome, GovernedVerificationProfile, OperatorAuthentication,
    OperatorDecisionInput, OperatorDecisionKind, SupervisorVerdictInput, SupervisorVerdictKind,
    WorkerCompletionClaimInput,
};
use impulse_ops::memory_candidate::AcceptedRunSourceAssurance;
use impulse_ops::operator_capability::OperatorCapabilityPresentation;
use impulse_ops::role_assignment::canonical_governed_builder_assignment;
use impulse_ops::ProjectOpsSnapshot;
use impulse_rs::client::DaemonClient;
use impulse_rs::daemon::{DaemonRequest, DaemonResponse};
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const IMPULSE_BIN: &str = env!("CARGO_BIN_EXE_impulse-rs");

struct DaemonGuard {
    child: Option<Child>,
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
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

fn init_project(with_git: bool) -> (tempfile::TempDir, String, Option<String>) {
    let repo = tempfile::Builder::new()
        .prefix("sap-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(repo.path().join("README.md"), "governed fixture\n").unwrap();
    if with_git {
        // `impulse init` only writes its ignore list into an existing Git
        // repository, and a profiled registration requires a clean worktree,
        // so the repository must exist first.
        run_git(repo.path(), &["init", "--quiet"]);
        run_git(repo.path(), &["config", "user.email", "test@example.com"]);
        run_git(repo.path(), &["config", "user.name", "Impulse Test"]);
    }
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

    let project_id = impulse_ops::sanitize_id(
        &repo
            .path()
            .file_name()
            .expect("temporary repo has a name")
            .to_string_lossy(),
    );

    let oid = if with_git {
        run_git(
            repo.path(),
            &[
                "add",
                ".gitignore",
                ".impulse/GENOME.md",
                ".impulse/config.json",
                ".impulse/impulse-capabilities.json",
                "README.md",
            ],
        );
        run_git(repo.path(), &["commit", "--quiet", "-m", "initial"]);
        assert_eq!(
            run_git(repo.path(), &["status", "--porcelain"]),
            "",
            "a profiled registration requires a clean canonical worktree"
        );
        Some(run_git(repo.path(), &["rev-parse", "HEAD"]))
    } else {
        None
    };

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

fn capability_path(socket: &Path) -> PathBuf {
    socket.with_extension("operator-cap")
}

fn published_capability(socket: &Path) -> String {
    std::fs::read_to_string(capability_path(socket))
        .expect("the daemon must publish its operator capability")
        .trim()
        .to_string()
}

/// Send requests over one raw connection that never presents the capability
/// unless a `PresentOperatorCapability` request is included explicitly. This is
/// what a launched governed pane can do with `IMPULSE_SOCKET_PATH` alone.
async fn raw_exchange(socket: &Path, requests: Vec<DaemonRequest>) -> Vec<DaemonResponse> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("raw client must connect");
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut responses = Vec::new();
    for request in requests {
        let line = serde_json::to_string(&request).unwrap();
        writer.write_all(line.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await.unwrap();
        responses.push(serde_json::from_str(&response_line).unwrap());
    }
    responses
}

fn ok_from_response<T: DeserializeOwned>(response: DaemonResponse) -> T {
    match response {
        DaemonResponse::Ok { result } => serde_json::from_value(result).unwrap(),
        other => panic!("expected successful daemon response, received {other:?}"),
    }
}

fn error_message(response: &DaemonResponse) -> &str {
    match response {
        DaemonResponse::Error { message } => message,
        other => panic!("expected an error response, received {other:?}"),
    }
}

fn request_id(value: &str) -> GovernedRequestId {
    GovernedRequestId::try_new(value).unwrap()
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn mutation_request(
    task: &GovernedTaskRun,
    id: &str,
    mutation: GovernedTaskMutation,
) -> GovernedTaskMutationRequest {
    GovernedTaskMutationRequest {
        request_id: request_id(id),
        project_id: task.project_id.clone(),
        task_id: task.id.clone(),
        expected_revision: task.revision,
        mutation,
    }
}

async fn mutate(
    client: &DaemonClient,
    task: &GovernedTaskRun,
    id: &str,
    mutation: GovernedTaskMutation,
) -> GovernedTaskRun {
    ok_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: mutation_request(task, id, mutation),
            })
            .await
            .unwrap(),
    )
}

/// Drive a caller-composed governed task to `awaiting_operator`.
async fn awaiting_operator(
    client: &DaemonClient,
    project_id: &str,
    workspace_root: &str,
    suffix: &str,
) -> GovernedTaskRun {
    let registration = GovernedTaskRegistration::builder(
        format!("register-{suffix}"),
        format!("task-{suffix}"),
        project_id.to_string(),
        workspace_root.to_string(),
        "prove socket actor provenance",
        "provenance-builder",
        "ion",
    )
    .acceptance_criteria(vec!["only an authenticated operator can accept".to_string()])
    .build()
    .unwrap();
    let registered: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );
    let running = mutate(
        client,
        &registered,
        &format!("running-{suffix}"),
        GovernedTaskMutation::MarkRunning {
            actor: GovernedActor {
                kind: GovernedActorKind::System,
                id: "provenance-test".to_string(),
            },
        },
    )
    .await;
    let claimed = mutate(
        client,
        &running,
        &format!("claim-{suffix}"),
        GovernedTaskMutation::SubmitClaim {
            claim: WorkerCompletionClaimInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Worker,
                    id: "provenance-builder".to_string(),
                },
                summary: "work complete".to_string(),
                subject_revision: "a".repeat(40),
                artifact_ids: vec!["claim-artifact".to_string()],
                diff_ref: None,
            },
        },
    )
    .await;
    let verified = mutate(
        client,
        &claimed,
        &format!("verify-{suffix}"),
        GovernedTaskMutation::RecordVerification {
            verification: GovernedVerificationInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Verifier,
                    id: "provenance-verifier".to_string(),
                },
                claim_id: claimed.latest_claim().unwrap().id.clone(),
                subject_revision: "a".repeat(40),
                policy: "caller-composed-v1".to_string(),
                outcome: GovernedVerificationOutcome::Passed,
                commands: vec![GovernedCommandEvidence {
                    name: "cargo test --locked".to_string(),
                    executable: "cargo".to_string(),
                    redacted_args: vec!["test".to_string()],
                    command_digest: digest('a'),
                    exit_code: Some(0),
                    success: true,
                    output_digest: digest('b'),
                    output_ref: None,
                    output_bytes: 64,
                    output_truncated: false,
                }],
                artifact_ids: vec![],
                notes: None,
            },
        },
    )
    .await;
    mutate(
        client,
        &verified,
        &format!("review-{suffix}"),
        GovernedTaskMutation::RecordSupervisorVerdict {
            verdict: SupervisorVerdictInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Supervisor,
                    id: "provenance-supervisor".to_string(),
                },
                verification_id: verified.latest_verification().unwrap().id.clone(),
                verdict: SupervisorVerdictKind::RecommendAccept,
                rationale: "evidence satisfies the criteria".to_string(),
            },
        },
    )
    .await
}

fn approval(task: &GovernedTaskRun, id: &str) -> GovernedTaskMutationRequest {
    mutation_request(
        task,
        id,
        GovernedTaskMutation::RecordOperatorDecision {
            decision: OperatorDecisionInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Operator,
                    id: "provenance-operator".to_string(),
                },
                supervisor_verdict_id: task.latest_supervisor_verdict().unwrap().id.clone(),
                decision: OperatorDecisionKind::Approve,
                rationale: "approved from the operator surface".to_string(),
            },
        },
    )
}

#[tokio::test]
async fn a_builder_socket_cannot_mint_accepted_but_the_operator_surface_can() {
    let (repo, project_id, _) = init_project(false);
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());
    let workspace_root = repo.path().display().to_string();

    let judged = awaiting_operator(&client, &project_id, &workspace_root, "gate").await;
    assert_eq!(judged.review_state, GovernedReviewState::AwaitingOperator);

    // A launched Builder holding IMPULSE_SOCKET_PATH: same user, same socket,
    // no capability.
    let refused = raw_exchange(
        &socket,
        vec![DaemonRequest::MutateGovernedTask {
            request: approval(&judged, "builder-approve"),
        }],
    )
    .await;
    let message = error_message(&refused[0]);
    assert!(
        message.contains("operator-class connection"),
        "expected a typed authorization refusal, got: {message}"
    );
    assert!(message.contains("RecordOperatorDecision"));

    // The task is untouched: same revision, still awaiting the operator, and
    // no memory candidate was staged.
    let after_refusal = client
        .get_governed_task(project_id.clone(), judged.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_refusal, judged, "a refused approval mutates nothing");
    let snapshot: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert!(snapshot.memory_candidates.is_empty());

    // The same request from the operator surface, which reads the published
    // capability, is accepted.
    let accepted: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::MutateGovernedTask {
                request: approval(&judged, "operator-approve"),
            })
            .await
            .unwrap(),
    );
    assert_eq!(accepted.review_state, GovernedReviewState::Accepted);
    let decision = accepted.operator_decisions.last().unwrap();
    assert_eq!(
        decision.authentication,
        OperatorAuthentication::CapabilityAuthenticated,
        "the daemon stamps the connection's provenance onto the record"
    );

    let accepted_snapshot: ProjectOpsSnapshot =
        ok_from_response(client.send(DaemonRequest::GetOpsSnapshot).await.unwrap());
    assert_eq!(accepted_snapshot.memory_candidates.len(), 1);
    let candidate = &accepted_snapshot.memory_candidates[0];
    candidate.validate_shape().unwrap();
    assert_eq!(candidate.governed_task_id, accepted.id);
    // This fixture composes its own evidence, so the assurance stays at the
    // weaker half of the chain even though the operator authenticated.
    assert_eq!(
        candidate.source_assurance,
        AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator
    );
}

#[tokio::test]
async fn a_wrong_or_absent_capability_never_raises_a_connection() {
    let (repo, project_id, _) = init_project(false);
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());
    let workspace_root = repo.path().display().to_string();

    let judged = awaiting_operator(&client, &project_id, &workspace_root, "wrong-token").await;

    let responses = raw_exchange(
        &socket,
        vec![
            DaemonRequest::PresentOperatorCapability(OperatorCapabilityPresentation {
                token: "f".repeat(64),
            }),
            DaemonRequest::PresentOperatorCapability(OperatorCapabilityPresentation {
                token: "not-even-hex".to_string(),
            }),
            DaemonRequest::MutateGovernedTask {
                request: approval(&judged, "wrong-token-approve"),
            },
        ],
    )
    .await;

    assert!(
        error_message(&responses[0]).contains("does not match this daemon run"),
        "a well-formed but wrong token is rejected"
    );
    assert!(error_message(&responses[1]).contains("does not match this daemon run"));
    assert!(error_message(&responses[2]).contains("operator-class connection"));

    let unchanged = client
        .get_governed_task(project_id, judged.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged, judged);
}

#[tokio::test]
async fn the_published_capability_is_owner_only_and_raises_the_presenting_connection() {
    let (repo, _project_id, _) = init_project(false);
    let (_daemon, socket) = start_daemon(repo.path());

    let path = capability_path(&socket);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the capability must be owner-read-write only");
    let token = published_capability(&socket);
    assert_eq!(token.len(), 64);

    let responses = raw_exchange(
        &socket,
        vec![DaemonRequest::PresentOperatorCapability(
            OperatorCapabilityPresentation {
                token: token.clone(),
            },
        )],
    )
    .await;
    let result: serde_json::Value = ok_from_response(responses.into_iter().next().unwrap());
    assert_eq!(result["connection_class"], "operator");

    // Classification is per connection: a second, fresh connection starts at
    // non-operator again.
    let fresh = raw_exchange(&socket, vec![DaemonRequest::Status]).await;
    assert!(matches!(fresh[0], DaemonResponse::Ok { .. }));

    // A second daemon run mints a different capability.
    let other = init_project(false).0;
    let (_other_daemon, other_socket) = start_daemon(other.path());
    assert_ne!(token, published_capability(&other_socket));
}

/// ADR-0018: the published file is a *hand-off* channel, not the source of
/// truth. Anything that can read it can also overwrite it — same uid — so what
/// matters is that overwriting it cannot forge operator class. A presentation
/// is compared against the running daemon's in-memory capability, so rewriting
/// the file only denies the real operator, and only until the daemon restarts.
#[tokio::test]
async fn overwriting_the_published_capability_cannot_forge_operator_class() {
    let (repo, project_id, _) = init_project(false);
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());
    let workspace_root = repo.path().display().to_string();

    let judged = awaiting_operator(&client, &project_id, &workspace_root, "forged-file").await;
    let genuine = published_capability(&socket);

    // A same-uid process replaces the file with a well-formed token of its own.
    let forged = "9".repeat(64);
    assert_ne!(forged, genuine);
    std::fs::write(capability_path(&socket), format!("{forged}\n")).unwrap();

    // Presenting the forged token is refused: the daemon compares against its
    // own in-memory capability, which the file rewrite never touched.
    let responses = raw_exchange(
        &socket,
        vec![
            DaemonRequest::PresentOperatorCapability(OperatorCapabilityPresentation {
                token: forged,
            }),
            DaemonRequest::MutateGovernedTask {
                request: approval(&judged, "forged-file-approve"),
            },
        ],
    )
    .await;
    assert!(error_message(&responses[0]).contains("does not match this daemon run"));
    assert!(error_message(&responses[1]).contains("operator-class connection"));
    assert_eq!(
        client
            .get_governed_task(project_id.clone(), judged.id.clone())
            .await
            .unwrap()
            .unwrap(),
        judged,
        "a forged capability file must leave the task untouched"
    );

    // The exposure is availability, not authentication: `DaemonClient` now reads
    // the forged file and is refused, while the genuine token still works.
    let refused = client
        .send(DaemonRequest::MutateGovernedTask {
            request: approval(&judged, "forged-file-client-approve"),
        })
        .await
        .unwrap();
    assert!(error_message(&refused).contains("operator-class connection"));

    let accepted = raw_exchange(
        &socket,
        vec![
            DaemonRequest::PresentOperatorCapability(OperatorCapabilityPresentation {
                token: genuine,
            }),
            DaemonRequest::MutateGovernedTask {
                request: approval(&judged, "genuine-approve"),
            },
        ],
    )
    .await;
    let task: GovernedTaskRun = ok_from_response(accepted.into_iter().nth(1).unwrap());
    assert_eq!(task.review_state, GovernedReviewState::Accepted);
    assert_eq!(
        task.operator_decisions.last().unwrap().authentication,
        OperatorAuthentication::CapabilityAuthenticated
    );
}

#[tokio::test]
async fn a_profiled_tasks_lifecycle_marks_require_operator_class() {
    let (repo, project_id, oid) = init_project(true);
    let oid = oid.expect("git fixture produces a commit");
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());

    let assignment = canonical_governed_builder_assignment();
    let compatibility = impulse_ops::agent_registry::AgentRegistry::builtin()
        .evaluate_role_compatibility(
            &impulse_ops::agent_registry::AgentPlatformId::try_new("ion").unwrap(),
            &assignment,
        )
        .unwrap();
    let registration = GovernedTaskRegistration::builder(
        "register-profiled",
        "task-profiled",
        project_id.clone(),
        repo.path().display().to_string(),
        "prove profiled lifecycle marks are operator-only",
        "profiled-builder",
        "ion",
    )
    .acceptance_criteria(vec!["the committed workspace passes".to_string()])
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(oid)
    .role_assignment(assignment)
    .role_compatibility(compatibility)
    .build()
    .unwrap();
    let registered: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );

    let mark_running = GovernedTaskMutation::MarkRunning {
        actor: GovernedActor {
            kind: GovernedActorKind::System,
            id: "profiled-launcher".to_string(),
        },
    };
    let refused = raw_exchange(
        &socket,
        vec![DaemonRequest::MutateGovernedTask {
            request: mutation_request(
                &registered,
                "profiled-running-builder",
                mark_running.clone(),
            ),
        }],
    )
    .await;
    assert!(
        error_message(&refused[0]).contains("MarkRunning"),
        "a Builder does not narrate its own profiled lifecycle"
    );

    let running = mutate(
        &client,
        &registered,
        "profiled-running-operator",
        mark_running,
    )
    .await;
    assert_eq!(running.revision, registered.revision + 1);
}
