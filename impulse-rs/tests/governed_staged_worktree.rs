//! ADR-0019: the staged Builder world scope, proven against real Git repositories.
//!
//! Nothing here is faked. Each test initializes a real repository in a temporary
//! directory, drives the daemon-owned producers, and inspects the resulting Git
//! state with `git` itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use impulse_ops::governed_task::{
    ApprovalPolicy, GovernedActor, GovernedActorKind, GovernedExecutionState,
    GovernedPromotionOutcome, GovernedRecordId, GovernedReviewState, GovernedTaskId,
    GovernedTaskRun, GovernedVerificationProfile, StagedWorktree, StagedWorktreeStatus,
    WorkerCompletionClaim, WorldScope,
};
use impulse_rs::governed_producers::{
    discard_staged_worktree, materialize_staged_worktree, promote_governed_outcome,
};
use tempfile::TempDir;

/// Local to this lane on purpose: a sibling lane is unifying the five existing
/// copies of this helper, and this file must not collide with that work.
fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_string()
}

fn init_repo() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path().canonicalize().expect("canonical tempdir");
    git(&repo, &["init", "--initial-branch=main"]);
    git(&repo, &["config", "user.email", "lane@example.invalid"]);
    git(&repo, &["config", "user.name", "Staged Lane"]);
    std::fs::write(repo.join("README.md"), "initial\n").expect("write README");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "initial"]);
    (dir, repo)
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "--verify", "HEAD^{commit}"])
}

fn commit_in(repo: &Path, name: &str, contents: &str) -> String {
    std::fs::write(repo.join(name), contents).expect("write file");
    git(repo, &["add", name]);
    git(repo, &["commit", "-m", &format!("add {name}")]);
    head(repo)
}

fn task(repo: &Path, initial: &str) -> GovernedTaskRun {
    GovernedTaskRun {
        id: GovernedTaskId::try_new("task-staged").unwrap(),
        revision: 1,
        project_id: "impulse-test".to_string(),
        workspace_root: repo.display().to_string(),
        task: "Ship the staged scope".to_string(),
        acceptance_criteria: vec!["the gate is green".to_string()],
        approval_policy: ApprovalPolicy::OperatorRequired,
        world_scope: WorldScope::StagedAuthoritative,
        verification_profile: Some(GovernedVerificationProfile::RustWorkspaceV1),
        role_assignment: None,
        role_compatibility: None,
        runtime_id: "ion".to_string(),
        agent_id: "worker-1".to_string(),
        session_id: None,
        initial_subject_revision: Some(initial.to_string()),
        staged_worktree: None,
        promotions: Vec::new(),
        execution_state: GovernedExecutionState::Registered,
        review_state: GovernedReviewState::AwaitingClaim,
        claims: Vec::new(),
        verifications: Vec::new(),
        supervisor_verdicts: Vec::new(),
        operator_decisions: Vec::new(),
        events: Vec::new(),
        created_at: "2026-09-02T00:00:00Z".to_string(),
        updated_at: "2026-09-02T00:00:00Z".to_string(),
    }
}

fn system_actor() -> GovernedActor {
    GovernedActor {
        kind: GovernedActorKind::System,
        id: "impulse-daemon:staged_worktree".to_string(),
    }
}

/// Attach a materialized staged worktree and an accepted claim, as the ledger
/// would hold them by the time promotion runs.
fn accepted(task: &GovernedTaskRun, root: &str, initial: &str, accepted: &str) -> GovernedTaskRun {
    let mut task = task.clone();
    task.staged_worktree = Some(StagedWorktree {
        id: GovernedRecordId::try_new("staged-1").unwrap(),
        actor: system_actor(),
        root: root.to_string(),
        initial_subject_revision: initial.to_string(),
        status: StagedWorktreeStatus::Active,
        materialized_at: "2026-09-02T00:00:00Z".to_string(),
        based_on_revision: 1,
    });
    task.claims.push(WorkerCompletionClaim {
        id: GovernedRecordId::try_new("claim-1").unwrap(),
        actor: GovernedActor {
            kind: GovernedActorKind::Worker,
            id: "worker-1".to_string(),
        },
        summary: "done".to_string(),
        subject_revision: accepted.to_string(),
        artifact_ids: Vec::new(),
        diff_ref: None,
        loop_report_digest: None,
        submitted_at: "2026-09-02T00:00:01Z".to_string(),
        based_on_revision: 2,
    });
    task.execution_state = GovernedExecutionState::RuntimeExited;
    task.review_state = GovernedReviewState::Accepted;
    task
}

/// Materialize, then let the Builder commit inside the staged worktree.
fn staged_with_builder_commit(repo: &Path) -> (GovernedTaskRun, String, String, String) {
    let initial = head(repo);
    let registered = task(repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let root = staged.root.clone();
    let builder_commit = commit_in(Path::new(&root), "feature.txt", "builder work\n");
    let task = accepted(&registered, &root, &initial, &builder_commit);
    (task, initial, builder_commit, root)
}

#[test]
fn test_materialize_creates_a_detached_worktree_at_the_attested_oid() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);

    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");

    assert_eq!(
        PathBuf::from(&staged.root),
        registered.expected_staged_worktree_root().unwrap()
    );
    assert_eq!(staged.initial_subject_revision, initial);
    assert_eq!(staged.actor.kind, GovernedActorKind::System);
    let staged_root = PathBuf::from(&staged.root);
    assert!(staged_root.join("README.md").is_file());
    assert_eq!(head(&staged_root), initial);
    // Detached: the staged worktree carries no branch of its own.
    assert_eq!(
        git(&staged_root, &["rev-parse", "--abbrev-ref", "HEAD"]).as_str(),
        "HEAD"
    );
    // The canonical tree is untouched and still reads as clean to the producers.
    assert_eq!(head(&repo), initial);
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).as_str(),
        "main"
    );
}

#[test]
fn test_materialize_refuses_a_non_staged_world_scope() {
    let (_dir, repo) = init_repo();
    let mut registered = task(&repo, &head(&repo));
    registered.world_scope = WorldScope::Authoritative;

    let error = materialize_staged_worktree(&registered)
        .expect_err("only a staged scope materializes a worktree");
    assert!(error.to_string().contains("staged_authoritative"));
}

#[test]
fn test_materialize_refuses_a_head_that_moved_off_the_attested_oid() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    commit_in(&repo, "drift.txt", "moved\n");

    let error = materialize_staged_worktree(&registered)
        .expect_err("a moved canonical head must not be staged silently");
    assert!(error
        .to_string()
        .contains("moved off the registered initial OID"));
}

#[test]
fn test_materialize_refuses_to_reuse_an_existing_path() {
    let (_dir, repo) = init_repo();
    let registered = task(&repo, &head(&repo));
    let root = registered.expected_staged_worktree_root().unwrap();
    std::fs::create_dir_all(&root).expect("pre-create staged path");

    let error = materialize_staged_worktree(&registered)
        .expect_err("an occupied staged path must not be reused");
    assert!(error.to_string().contains("already exists"));
}

#[test]
fn test_promotion_fast_forwards_the_canonical_branch_when_head_has_not_moved() {
    let (_dir, repo) = init_repo();
    let (task, initial, builder_commit, _root) = staged_with_builder_commit(&repo);

    // The canonical tree is byte-identical up to the moment of promotion.
    assert_eq!(head(&repo), initial);
    assert!(!repo.join("feature.txt").exists());

    let promotion = promote_governed_outcome(&task).expect("promote accepted outcome");

    assert_eq!(promotion.accepted_revision, builder_commit);
    assert_eq!(promotion.initial_subject_revision, initial);
    assert_eq!(
        promotion.outcome,
        GovernedPromotionOutcome::Promoted {
            promoted_revision: builder_commit.clone()
        }
    );
    assert_eq!(head(&repo), builder_commit);
    assert_eq!(
        std::fs::read_to_string(repo.join("feature.txt")).unwrap(),
        "builder work\n"
    );
    // Fast-forward only: the canonical branch is still `main`.
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).as_str(),
        "main"
    );
}

#[test]
fn test_promotion_blocks_without_touching_the_canonical_branch_when_head_moved() {
    let (_dir, repo) = init_repo();
    let (task, _initial, _builder_commit, _root) = staged_with_builder_commit(&repo);

    let moved = commit_in(&repo, "canonical.txt", "someone else\n");

    let promotion = promote_governed_outcome(&task).expect("a blocked promotion is not an error");

    assert_eq!(
        promotion.outcome,
        GovernedPromotionOutcome::PromotionBlocked {
            canonical_head: moved.clone()
        }
    );
    assert_eq!(head(&repo), moved);
    assert!(!repo.join("feature.txt").exists());
}

#[test]
fn test_promotion_refuses_a_task_that_is_not_accepted() {
    let (_dir, repo) = init_repo();
    let (mut task, _initial, _builder_commit, _root) = staged_with_builder_commit(&repo);
    task.review_state = GovernedReviewState::AwaitingOperator;

    let error =
        promote_governed_outcome(&task).expect_err("promotion requires operator acceptance");
    assert!(error.to_string().contains("accepted governed task"));
}

#[test]
fn test_promotion_refuses_a_claim_the_staged_worktree_does_not_hold() {
    let (_dir, repo) = init_repo();
    let (mut task, initial, _builder_commit, _root) = staged_with_builder_commit(&repo);
    // Point the accepted claim at the initial commit, not at what the Builder built.
    task.claims[0].subject_revision = initial;

    let error = promote_governed_outcome(&task)
        .expect_err("promotion must land exactly what the staged worktree holds");
    assert!(error
        .to_string()
        .contains("does not match the accepted subject revision"));
}

#[test]
fn test_promotion_refuses_a_dirty_staged_worktree() {
    let (_dir, repo) = init_repo();
    let (task, _initial, _builder_commit, root) = staged_with_builder_commit(&repo);
    std::fs::write(Path::new(&root).join("README.md"), "uncommitted\n")
        .expect("dirty the staged tree");

    let error = promote_governed_outcome(&task)
        .expect_err("an uncommitted staged worktree must not be promoted");
    assert!(error.to_string().contains("clean descendant"));
}

#[test]
fn test_discard_removes_the_staged_worktree_and_its_administrative_entry() {
    let (_dir, repo) = init_repo();
    let (task, initial, _builder_commit, root) = staged_with_builder_commit(&repo);
    assert!(Path::new(&root).is_dir());
    assert!(git(&repo, &["worktree", "list"]).contains(&root));

    discard_staged_worktree(&task).expect("discard staged worktree");

    assert!(!Path::new(&root).exists());
    assert!(!git(&repo, &["worktree", "list"]).contains(&root));
    // Nothing the Builder did reached the canonical branch.
    assert_eq!(head(&repo), initial);
    assert!(!repo.join("feature.txt").exists());
}

#[test]
fn test_discard_refuses_a_task_with_no_active_staged_worktree() {
    let (_dir, repo) = init_repo();
    let registered = task(&repo, &head(&repo));

    let error = discard_staged_worktree(&registered)
        .expect_err("a task with no staged worktree has nothing to discard");
    assert!(error.to_string().contains("no active staged worktree"));
}

#[test]
fn test_rejected_run_leaves_the_canonical_tree_byte_identical() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let before = std::fs::read_to_string(repo.join("README.md")).unwrap();
    let (mut task, _initial, _builder_commit, root) = staged_with_builder_commit(&repo);
    task.review_state = GovernedReviewState::Rejected;

    discard_staged_worktree(&task).expect("a rejected run discards its staged worktree");

    assert_eq!(head(&repo), initial);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        before
    );
    assert!(!Path::new(&root).exists());
    assert_eq!(git(&repo, &["status", "--porcelain"]).as_str(), "");
}
