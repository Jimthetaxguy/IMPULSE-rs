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
    GovernedTaskRun, GovernedVerificationProfile, PromotionBlockedReason, SharedConfigComponent,
    StagedWorktree, StagedWorktreeInput, StagedWorktreeStatus, WorkerCompletionClaim, WorldScope,
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
        // The harness's own Git must not run the project's hooks either, or a
        // hook planted by a test would fire on the test's own commits and mask
        // what the producers actually did.
        .args(["-c", "core.hooksPath=/dev/null"])
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
/// Attach a materialized staged worktree and an accepted claim, as the ledger
/// would hold them by the time promotion runs. `shared_config_digest` is the
/// digest the producer actually recorded at materialization, so promotion sees
/// no configuration drift unless a test deliberately introduces some.
fn accepted(
    task: &GovernedTaskRun,
    staged: &StagedWorktreeInput,
    initial: &str,
    accepted: &str,
) -> GovernedTaskRun {
    let root = staged.root.as_str();
    let shared_config_digest = staged.shared_config_digest.clone();
    let mut task = task.clone();
    task.staged_worktree = Some(StagedWorktree {
        id: GovernedRecordId::try_new("staged-1").unwrap(),
        actor: system_actor(),
        root: root.to_string(),
        initial_subject_revision: initial.to_string(),
        shared_config_digest,
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
        loop_report_version: None,
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
    let task = accepted(&registered, &staged, &initial, &builder_commit);
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
            canonical_head: moved.clone(),
            reason: PromotionBlockedReason::CanonicalHeadMoved,
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

// ---------------------------------------------------------------------------
// Review round 1
// ---------------------------------------------------------------------------

/// P1-1: a detached canonical HEAD has no branch to advance. Promoting there
/// would move HEAD only, and the next `git switch` would orphan the work.
#[test]
fn test_promotion_blocks_on_a_detached_canonical_head_without_moving_anything() {
    let (_dir, repo) = init_repo();
    let (task, initial, _builder_commit, _root) = staged_with_builder_commit(&repo);
    let branch_before = git(&repo, &["rev-parse", "refs/heads/main"]);
    git(&repo, &["checkout", "--detach", "--quiet", "HEAD"]);
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).as_str(),
        "HEAD"
    );

    let promotion = promote_governed_outcome(&task).expect("a detached HEAD blocks, not errors");

    assert_eq!(
        promotion.outcome,
        GovernedPromotionOutcome::PromotionBlocked {
            canonical_head: initial.clone(),
            reason: PromotionBlockedReason::DetachedHead,
        }
    );
    // Neither HEAD nor the branch moved, and the Builder's file never landed.
    assert_eq!(head(&repo), initial);
    assert_eq!(git(&repo, &["rev-parse", "refs/heads/main"]), branch_before);
    assert!(!repo.join("feature.txt").exists());
}

/// P1-1: promotion moves a real branch ref, not just HEAD, so the accepted
/// commit survives a later `git switch`.
#[test]
fn test_promotion_advances_the_branch_ref_not_only_head() {
    let (_dir, repo) = init_repo();
    let (task, _initial, builder_commit, _root) = staged_with_builder_commit(&repo);

    promote_governed_outcome(&task).expect("promote accepted outcome");

    assert_eq!(
        git(&repo, &["rev-parse", "refs/heads/main"]),
        builder_commit
    );
    // Leaving and returning to the branch keeps the promoted commit.
    git(&repo, &["checkout", "--detach", "--quiet", "HEAD"]);
    git(&repo, &["checkout", "--quiet", "main"]);
    assert_eq!(head(&repo), builder_commit);
    assert!(repo.join("feature.txt").is_file());
}

/// P2-2: `.git/hooks` is shared across linked worktrees, so a Builder could
/// plant a hook that runs inside a daemon-owned producer.
#[test]
fn test_planted_git_hooks_never_execute_during_staging_or_promotion() {
    let (_dir, repo) = init_repo();
    let hooks = repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir");
    let marker = repo.join("hook-ran.txt");
    // `post-index-change` and `fsmonitor-watchman` fire on a bare `git status`,
    // which the promotion path runs twice; `pre-auto-gc` fires on ordinary
    // plumbing. Hooks are not only about the obviously mutating commands.
    for hook in [
        "post-checkout",
        "post-merge",
        "reference-transaction",
        "post-index-change",
        "pre-auto-gc",
        "fsmonitor-watchman",
    ] {
        let path = hooks.join(hook);
        std::fs::write(
            &path,
            format!("#!/bin/sh\necho {hook} >> {}\n", marker.display()),
        )
        .expect("write hook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod hook");
        }
    }

    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    assert!(
        !marker.exists(),
        "a planted hook executed during materialization: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );

    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");
    let task = accepted(&registered, &staged, &initial, &builder_commit);
    promote_governed_outcome(&task).expect("promote accepted outcome");

    assert_eq!(head(&repo), builder_commit);
    assert!(
        !marker.exists(),
        "a planted hook executed during promotion: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
}

/// P2-3: the fail-closed message must name the recovery.
#[test]
fn test_occupied_staged_path_error_names_the_recovery() {
    let (_dir, repo) = init_repo();
    let registered = task(&repo, &head(&repo));
    std::fs::create_dir_all(registered.expected_staged_worktree_root().unwrap())
        .expect("pre-create staged path");

    let error = materialize_staged_worktree(&registered).expect_err("occupied path fails closed");
    let message = error.to_string();
    assert!(message.contains("already exists"), "{message}");
    assert!(message.contains("worktree prune"), "{message}");
}

/// Review round 2, from the ADR-0018 lane's tip: `.git/hooks` is not the only
/// worktree-shared state. `.git/config` is shared too, and a `filter` driver
/// defined there executes during any checkout — including the working-tree sync
/// that promotion performs, in the canonical workspace, after review passed.
#[test]
fn test_a_filter_driver_planted_by_the_builder_never_executes_during_promotion() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let staged_root = PathBuf::from(&staged.root);
    let marker = repo.join("filter-ran.txt");

    // The Builder, working only inside its own staged worktree, writes to the
    // shared repository config and assigns the driver to every path.
    git(
        &staged_root,
        &[
            "config",
            "filter.evil.smudge",
            &format!("sh -c 'echo FILTER_FIRED >> {}; cat'", marker.display()),
        ],
    );
    git(&staged_root, &["config", "filter.evil.clean", "cat"]);
    std::fs::write(staged_root.join(".gitattributes"), "* filter=evil\n").expect("write attrs");
    git(&staged_root, &["add", ".gitattributes"]);
    let builder_commit = commit_in(&staged_root, "feature.txt", "builder work\n");

    // Negative control: prove the planted driver is actually armed, so the
    // assertion below is about the fix and not about a filter that never would
    // have run. Materializing a file in the staged worktree fires it.
    std::fs::remove_file(staged_root.join("feature.txt")).expect("remove for re-checkout");
    git(&staged_root, &["checkout", "--", "feature.txt"]);
    assert!(
        marker.exists(),
        "the planted filter driver is not armed; this test would pass vacuously"
    );
    std::fs::remove_file(&marker).expect("reset the marker");

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("promotion reports, never executes");

    assert!(
        !marker.exists(),
        "a Builder-planted filter driver executed during promotion: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        }),
        "a Builder that rewrote shared repository config must block promotion, naming the file"
    );
    assert_eq!(head(&repo), initial, "a blocked promotion moves nothing");
}

/// Benign churn blocks too, and must say which file changed — an operator who
/// ran `git remote add` mid-run should not have to guess.
#[test]
fn test_a_benign_shared_config_change_blocks_and_names_the_file() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");
    git(
        &repo,
        &["remote", "add", "origin", "https://example.invalid/x.git"],
    );

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("benign drift blocks, never errors");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        })
    );
    assert_eq!(head(&repo), initial);
}

/// `.git/info/attributes` is shared and never shows up in a diff of the work
/// tree, so it is the quietest door of the three.
#[test]
fn test_a_shared_info_attributes_change_blocks_and_names_that_file() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");
    let info = repo.join(".git").join("info");
    std::fs::create_dir_all(&info).expect("info dir");
    std::fs::write(info.join("attributes"), "* filter=evil\n").expect("write shared attributes");

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("attribute drift blocks");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::InfoAttributes
        })
    );
    assert_eq!(head(&repo), initial);
}
