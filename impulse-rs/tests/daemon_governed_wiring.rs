#![cfg(unix)]

//! Socket-level proof for the ADR-0019 staged endpoints (protocol v9).
//!
//! The in-process handler tests in `src/daemon/governed_wiring.rs` cover the
//! producer outcomes. What can only be proven against a real daemon is the
//! ADR-0018 boundary these endpoints sit behind: a launched Builder holding
//! `IMPULSE_SOCKET_PATH` — modelled here by a raw socket connection that never
//! presents the operator capability — must not be able to stage its own world
//! scope, fast-forward the canonical branch, or delete the evidence of its own
//! run.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use impulse_ops::governed_task::{
    GovernedRequestId, GovernedTaskId, GovernedTaskRegistration, GovernedTaskRun,
    GovernedVerificationProfile, StagedWorktreeStatus, WorldScope,
};
use impulse_ops::governed_wiring::{
    GovernedPromotionRequest, GovernedStagedWorktreeDiscardRequest,
};
use impulse_ops::role_assignment::canonical_governed_builder_assignment;
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
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .output()
        .expect("Git command must launch");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git output must be UTF-8")
        .trim()
        .to_string()
}

/// A real project: a Git repository with an `impulse init` runtime namespace and
/// one clean commit, which is what a profiled registration attests against.
fn init_project() -> (tempfile::TempDir, String, String) {
    let repo = tempfile::Builder::new()
        .prefix("dgw-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(repo.path().join("README.md"), "governed fixture\n").unwrap();
    run_git(repo.path(), &["init", "--quiet", "--initial-branch=main"]);
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

/// One raw connection that never presents the operator capability: exactly what
/// a launched governed pane holding `IMPULSE_SOCKET_PATH` can do.
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
        other => panic!("expected a successful daemon response, received {other:?}"),
    }
}

fn error_message(response: &DaemonResponse) -> &str {
    match response {
        DaemonResponse::Error { message } => message,
        other => panic!("expected an error response, received {other:?}"),
    }
}

fn staged_registration(
    project_id: &str,
    workspace_root: &Path,
    oid: &str,
    suffix: &str,
) -> GovernedTaskRegistration {
    let assignment = canonical_governed_builder_assignment();
    let platform = impulse_ops::agent_registry::AgentPlatformId::try_new("ion").unwrap();
    let compatibility = impulse_ops::agent_registry::AgentRegistry::registry_for_runtime()
        .unwrap()
        .evaluate_role_compatibility(&platform, &assignment)
        .unwrap();
    GovernedTaskRegistration::builder(
        format!("register-{suffix}"),
        format!("task-{suffix}"),
        project_id.to_string(),
        workspace_root.display().to_string(),
        "prove the staged endpoints over the socket",
        "wire-builder",
        "ion",
    )
    .acceptance_criteria(vec!["only an operator may stage and promote".to_string()])
    .world_scope(WorldScope::StagedAuthoritative)
    .verification_profile(GovernedVerificationProfile::RustWorkspaceV1)
    .initial_subject_revision(oid.to_string())
    .role_assignment(assignment)
    .role_compatibility(compatibility)
    .build()
    .unwrap()
}

fn request_id(value: &str) -> GovernedRequestId {
    GovernedRequestId::try_new(value).unwrap()
}

#[tokio::test]
async fn a_builder_socket_cannot_stage_its_own_world_scope_but_the_operator_surface_can() {
    let (repo, project_id, oid) = init_project();
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());

    let registration = staged_registration(&project_id, repo.path(), &oid, "staged-wire");
    let task_id = registration.task_id.clone();

    // A launched Builder: same user, same socket, no capability.
    let refused = raw_exchange(
        &socket,
        vec![DaemonRequest::RegisterGovernedTask {
            registration: registration.clone(),
        }],
    )
    .await;
    let message = error_message(&refused[0]);
    assert!(
        message.contains("operator-class connection"),
        "expected a typed authorization refusal, got: {message}"
    );
    assert!(
        message.contains("staged world scope"),
        "the refusal must name what was refused, got: {message}"
    );
    assert!(
        client
            .get_governed_task(project_id.clone(), task_id.clone())
            .await
            .unwrap()
            .is_none(),
        "a refused staged registration records nothing"
    );
    assert!(
        !repo.path().join(".impulse/worktrees").exists(),
        "a refused staged registration materializes nothing"
    );

    // The operator surface presents the capability automatically.
    let registered: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );
    let staged = registered
        .active_staged_worktree()
        .expect("registration materializes the staged worktree before any launch");
    assert_eq!(staged.status, StagedWorktreeStatus::Active);
    assert_eq!(
        Path::new(&staged.root),
        registered.expected_staged_worktree_root().unwrap()
    );
    assert!(Path::new(&staged.root).join(".git").exists());
    assert_eq!(
        run_git(Path::new(&staged.root), &["rev-parse", "HEAD"]),
        oid,
        "the staged worktree starts at the daemon-attested initial OID"
    );
    assert_eq!(
        registered
            .launch_working_directory()
            .expect("a materialized staged task has a launch working directory"),
        staged.root
    );

    // The canonical tree is untouched.
    assert_eq!(run_git(repo.path(), &["rev-parse", "HEAD"]), oid);
}

#[tokio::test]
async fn a_builder_socket_can_neither_promote_nor_discard() {
    let (repo, project_id, oid) = init_project();
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());

    let registered: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask {
                registration: staged_registration(&project_id, repo.path(), &oid, "staged-gate"),
            })
            .await
            .unwrap(),
    );
    let staged_root = registered.active_staged_worktree().unwrap().root.clone();

    let refused = raw_exchange(
        &socket,
        vec![
            DaemonRequest::PromoteGovernedOutcome {
                request: GovernedPromotionRequest {
                    request_id: request_id("builder-promote"),
                    project_id: project_id.clone(),
                    task_id: registered.id.clone(),
                    expected_revision: registered.revision,
                },
            },
            DaemonRequest::DiscardGovernedStagedWorktree {
                request: GovernedStagedWorktreeDiscardRequest {
                    request_id: request_id("builder-discard"),
                    project_id: project_id.clone(),
                    task_id: registered.id.clone(),
                    expected_revision: registered.revision,
                    reason: "builder tries to erase its own run".to_string(),
                },
            },
        ],
    )
    .await;

    let promote_message = error_message(&refused[0]);
    assert!(
        promote_message.contains("operator-class connection"),
        "got: {promote_message}"
    );
    assert!(promote_message.contains("PromoteGovernedOutcome"));

    let discard_message = error_message(&refused[1]);
    assert!(
        discard_message.contains("operator-class connection"),
        "got: {discard_message}"
    );
    assert!(discard_message.contains("DiscardGovernedStagedWorktree"));

    // Nothing moved and nothing was deleted.
    let after = client
        .get_governed_task(project_id.clone(), registered.id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after, registered, "refused requests mutate nothing");
    assert!(Path::new(&staged_root).exists(), "the checkout survives");
    assert_eq!(run_git(repo.path(), &["rev-parse", "HEAD"]), oid);
}

#[tokio::test]
async fn an_unknown_task_id_is_refused_before_any_side_effect() {
    let (repo, project_id, _oid) = init_project();
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());

    let error = client
        .promote_governed_outcome(GovernedPromotionRequest {
            request_id: request_id("promote-missing"),
            project_id: project_id.clone(),
            task_id: GovernedTaskId::try_new("task-does-not-exist").unwrap(),
            expected_revision: 1,
        })
        .await
        .expect_err("promoting an unknown task must fail");
    assert!(error.to_string().contains("was not found"), "got: {error}");
    assert!(!repo.path().join(".impulse/worktrees").exists());
}

/// Review round 1, P1-2: staged materialization runs before the ledger write,
/// so a retry of a registration that already succeeded must be recognized from
/// the request id rather than reaching the producer and failing on a staged
/// path occupied by its own earlier attempt.
///
/// Driven over the real socket because that is where the retry comes from:
/// `DaemonClient` re-sends acknowledged requests, and `git worktree add` on a
/// large repository can outlast the response timeout.
#[tokio::test]
async fn a_replayed_staged_registration_over_the_socket_returns_the_recorded_task() {
    let (repo, project_id, oid) = init_project();
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());
    let registration = staged_registration(&project_id, repo.path(), &oid, "staged-replay");

    let first: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask {
                registration: registration.clone(),
            })
            .await
            .unwrap(),
    );
    let staged_root = first.active_staged_worktree().unwrap().root.clone();

    // Stand in for a Builder that has already started working in the checkout.
    let marker = Path::new(&staged_root).join("builder-scratch.txt");
    std::fs::write(&marker, "a live Builder is working here\n").unwrap();

    let replayed: GovernedTaskRun = ok_from_response(
        client
            .send(DaemonRequest::RegisterGovernedTask { registration })
            .await
            .unwrap(),
    );

    assert_eq!(replayed, first, "the replay returns the recorded task");
    assert!(
        marker.exists(),
        "a replayed registration must not disturb a live Builder's checkout"
    );
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap(),
        "a live Builder is working here\n"
    );
    assert_eq!(
        run_git(Path::new(&staged_root), &["rev-parse", "HEAD"]),
        oid,
        "the staged worktree is untouched by the replay"
    );
    assert_eq!(
        client
            .get_governed_task(project_id.clone(), first.id.clone())
            .await
            .unwrap()
            .unwrap(),
        first,
        "the replay records no second revision"
    );
}

/// Review round 1, P1-2 (second half): a genuinely occupied staged path — a
/// directory that outlived an interrupted run, not this task's own replay —
/// still fails closed, and the producer's recovery instructions survive to the
/// wire.
///
/// The daemon previously rendered these errors with `Display`, which collapses
/// an `anyhow` context chain to its outermost message, so the operator saw
/// "failed to materialize the staged worktree for this registration" and none
/// of the text telling them what to do about it.
#[tokio::test]
async fn a_materialization_failure_reaches_the_operator_with_its_recovery_text() {
    let (repo, project_id, oid) = init_project();
    let (_daemon, socket) = start_daemon(repo.path());
    let client = DaemonClient::new(socket.clone());
    let registration = staged_registration(&project_id, repo.path(), &oid, "staged-occupied");
    let task_id = registration.task_id.clone();

    std::fs::create_dir_all(
        repo.path()
            .join(".impulse")
            .join("worktrees")
            .join(task_id.as_str()),
    )
    .unwrap();

    let response = client
        .send(DaemonRequest::RegisterGovernedTask { registration })
        .await
        .unwrap();
    let message = error_message(&response);
    assert!(
        message.contains("already exists"),
        "the operator must be told the symptom: {message}"
    );
    assert!(
        message.contains("worktree prune"),
        "the operator must be told the recovery, not just the symptom: {message}"
    );
    assert!(
        client
            .get_governed_task(project_id, task_id)
            .await
            .unwrap()
            .is_none(),
        "a failed materialization still records no task"
    );
}
