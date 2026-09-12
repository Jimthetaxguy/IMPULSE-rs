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
    SharedRepositoryConfigPin, StagedWorktree, StagedWorktreeInput, StagedWorktreeStatus,
    WorkerCompletionClaim, WorldScope, LEGACY_SHARED_REPOSITORY_CONFIG_SCHEME_VERSION,
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

/// Review round 3. A worktree materialized before the pin existed cannot be
/// compared against anything, so promotion must refuse rather than guess.
#[test]
fn test_promotion_blocks_an_unpinned_staged_worktree_without_moving_anything() {
    let (_dir, repo) = init_repo();
    let (mut task, initial, _builder_commit, _root) = staged_with_builder_commit(&repo);
    if let Some(staged) = task.staged_worktree.as_mut() {
        staged.shared_config_digest = SharedRepositoryConfigPin::Unknown;
    }

    let promotion = promote_governed_outcome(&task).expect("an unpinned worktree blocks, not errs");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigUnpinned)
    );
    assert_eq!(head(&repo), initial, "a blocked promotion moves nothing");
    assert!(!repo.join("feature.txt").exists());
}

/// The symmetric case of the round-2 tests: a shared file that existed at
/// materialization and is gone at promotion is a change too. Absence is pinned.
#[test]
fn test_a_shared_file_deleted_after_materialization_blocks_and_names_it() {
    let (_dir, repo) = init_repo();
    let info = repo.join(".git").join("info");
    std::fs::create_dir_all(&info).expect("info dir");
    std::fs::write(info.join("attributes"), "* text=auto\n").expect("seed shared attributes");

    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");
    std::fs::remove_file(info.join("attributes")).expect("delete shared attributes");

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("deletion blocks, never errors");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::InfoAttributes
        })
    );
    assert_eq!(head(&repo), initial);
}

// ---------------------------------------------------------------------------
// Post-merge fixes for the #50 P1 review (2026-09-12)
// ---------------------------------------------------------------------------

/// Write an executable `core.fsmonitor` hook and point the *shared* repository
/// config at it. Git executes a pathname-valued `core.fsmonitor` to query
/// changed files, and `core.hooksPath` does not disable it.
fn plant_fsmonitor(repo: &Path, marker: &Path) -> PathBuf {
    // Inside `.git/`, which is not part of the work tree: a script in the work
    // tree would make the canonical checkout dirty and fail materialization for
    // a reason that has nothing to do with what this test is about.
    let script = repo.join(".git").join("fsmonitor-hook.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho FSMONITOR_FIRED >> {}\nprintf '/\\0'\n",
            marker.display()
        ),
    )
    .expect("write fsmonitor hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fsmonitor hook");
    }
    script
}

/// Second #50 P1 finding, the defense-in-depth half: every producer Git
/// invocation disables `core.fsmonitor` as well as `core.hooksPath`.
///
/// Materialization legitimately runs `git status` against configuration that
/// predates the run — the operator's own, which the trust boundary honors — so
/// there is no pin to compare against yet and ordering cannot help. Only the
/// `-c core.fsmonitor=false` in `hook_free_git` keeps this from executing.
#[test]
fn test_materialization_never_executes_a_pre_existing_fsmonitor_hook() {
    let (_dir, repo) = init_repo();
    let marker = repo.join(".git").join("fsmonitor-ran.txt");
    let script = plant_fsmonitor(&repo, &marker);
    git(
        &repo,
        &["config", "core.fsmonitor", &script.display().to_string()],
    );

    // Negative control: the hook really is armed for an ordinary `git status`.
    let armed = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["status", "--porcelain"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("run git status");
    assert!(armed.status.success());
    assert!(
        marker.exists(),
        "the planted fsmonitor hook is not armed; this test would pass vacuously"
    );
    std::fs::remove_file(&marker).expect("reset the marker");

    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");

    assert!(
        !marker.exists(),
        "a pathname-valued core.fsmonitor executed inside a producer: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    assert_eq!(head(Path::new(&staged.root)), initial);
}

/// Second #50 P1 finding, the load-bearing half: promotion compares the pinned
/// shared configuration **before** it spawns Git against the repository.
///
/// A Builder-planted `core.fsmonitor` runs on the `git status` that the old
/// ordering performed first, under operator authority, before the comparison
/// that was supposed to refuse the run could say anything.
#[test]
fn test_promotion_blocks_a_builder_planted_fsmonitor_without_executing_it() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let staged_root = PathBuf::from(&staged.root);
    let marker = repo.join(".git").join("fsmonitor-ran.txt");
    let script = plant_fsmonitor(&repo, &marker);

    // The Builder, working only inside its own staged worktree, writes the key
    // into `.git/config`, which every linked worktree shares.
    git(
        &staged_root,
        &["config", "core.fsmonitor", &script.display().to_string()],
    );
    let builder_commit = commit_in(&staged_root, "feature.txt", "builder work\n");
    assert!(
        marker.exists(),
        "the planted fsmonitor hook is not armed; this test would pass vacuously"
    );
    std::fs::remove_file(&marker).expect("reset the marker");

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("promotion reports, never executes");

    assert!(
        !marker.exists(),
        "a Builder-planted fsmonitor hook executed during promotion: {}",
        std::fs::read_to_string(&marker).unwrap_or_default()
    );
    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        })
    );
    assert_eq!(head(&repo), initial, "a blocked promotion moves nothing");
}

/// The ordering itself, proven independently of what any one Git key does: a
/// shared config that no Git process can parse.
///
/// Every `git` invocation in such a repository fails with `bad config line`, so
/// a promotion that still returns the typed blocked outcome cannot have run
/// one. Reverting the ordering turns this into an `Err`.
#[test]
fn test_promotion_compares_shared_config_before_running_any_git_command() {
    let (_dir, repo) = init_repo();
    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");

    // After materialization, so the pin was recorded against a parseable file.
    let config = repo.join(".git").join("config");
    let mut bytes = std::fs::read(&config).expect("read shared config");
    bytes.extend_from_slice(b"[unterminated\n");
    std::fs::write(&config, &bytes).expect("write shared config");
    // Negative control: Git really cannot run here any more.
    let broken = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("run git rev-parse");
    assert!(
        !broken.status.success(),
        "the config is still parseable; this test would pass vacuously"
    );

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task)
        .expect("the configuration comparison must not need a Git process");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        })
    );
    // The head was reported by reading the ref files directly.
    match promotion.outcome {
        GovernedPromotionOutcome::PromotionBlocked { canonical_head, .. } => {
            assert_eq!(canonical_head, initial)
        }
        other => panic!("expected a blocked promotion, got {other:?}"),
    }
}

/// Third #50 P1 finding: the digest sorted a rendered `git config --list`, so
/// swapping two repeated scalar keys produced the same digest even though Git
/// resolves the **last** one. Reproduced against Git 2.50.1: after the swap
/// `git config --get filter.evil.smudge` returns the other value.
#[test]
fn test_promotion_blocks_a_reordered_repeated_config_key() {
    let (_dir, repo) = init_repo();
    let marker = repo.join("filter-ran.txt");
    let inert = "[filter \"evil\"]\n\tsmudge = cat\n";
    let armed = format!(
        "[filter \"evil\"]\n\tsmudge = sh -c 'echo FILTER_FIRED >> {}; cat'\n",
        marker.display()
    );
    // Both lines exist before the run: the operator's own configuration, which
    // the trust boundary honors. Only their order changes during it.
    let config = repo.join(".git").join("config");
    let original = std::fs::read_to_string(&config).expect("read shared config");
    std::fs::write(&config, format!("{original}{armed}{inert}")).expect("seed repeated keys");
    assert_eq!(
        git(&repo, &["config", "--get", "filter.evil.smudge"]).as_str(),
        "cat",
        "the last repeated value wins; the inert one must be effective at first"
    );

    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");

    // The Builder swaps the two existing lines. No key is added, no value is
    // edited — and the armed driver is now the effective one.
    std::fs::write(&config, format!("{original}{inert}{armed}")).expect("reorder repeated keys");
    assert!(
        git(&repo, &["config", "--get", "filter.evil.smudge"]).contains("FILTER_FIRED"),
        "the reorder must actually change which driver Git resolves"
    );

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("a reorder blocks, never errors");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        }),
        "reordering repeated keys must not pass the pin"
    );
    assert!(!marker.exists(), "the reordered driver must not have run");
    assert_eq!(head(&repo), initial);
}

/// The digest covers files the shared config reaches through `include.path`,
/// because their contents are configuration too and can be edited without
/// touching `.git/config` itself.
#[test]
fn test_promotion_blocks_a_change_to_an_included_config_file() {
    let (_dir, repo) = init_repo();
    let included = repo.join(".git").join("extra.config");
    std::fs::write(&included, "[user]\n\temail = before@example.invalid\n")
        .expect("seed included config");
    let config = repo.join(".git").join("config");
    let original = std::fs::read_to_string(&config).expect("read shared config");
    std::fs::write(
        &config,
        format!("{original}[include]\n\tpath = extra.config\n"),
    )
    .expect("seed include directive");
    assert_eq!(
        git(&repo, &["config", "--get", "user.email"]).as_str(),
        "before@example.invalid",
        "the include must actually be in effect"
    );

    let initial = head(&repo);
    let registered = task(&repo, &initial);
    let staged = materialize_staged_worktree(&registered).expect("materialize staged worktree");
    let builder_commit = commit_in(Path::new(&staged.root), "feature.txt", "builder work\n");

    // `.git/config` itself is byte-identical; only the included file changed.
    std::fs::write(&included, "[user]\n\temail = after@example.invalid\n")
        .expect("rewrite included config");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        format!("{original}[include]\n\tpath = extra.config\n")
    );

    let task = accepted(&registered, &staged, &initial, &builder_commit);
    let promotion = promote_governed_outcome(&task).expect("an include change blocks");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigChanged {
            component: SharedConfigComponent::RepositoryConfig
        })
    );
    assert_eq!(head(&repo), initial);
}

/// An unchanged repository still promotes: the raw-bytes digest is stable
/// across the run when nothing writes to the pinned files.
#[test]
fn test_an_unchanged_shared_config_with_includes_still_promotes() {
    let (_dir, repo) = init_repo();
    let included = repo.join(".git").join("extra.config");
    std::fs::write(&included, "[user]\n\temail = stable@example.invalid\n")
        .expect("seed included config");
    let config = repo.join(".git").join("config");
    let original = std::fs::read_to_string(&config).expect("read shared config");
    std::fs::write(
        &config,
        format!("{original}[include]\n\tpath = extra.config\n"),
    )
    .expect("seed include directive");

    let (task, _initial, builder_commit, _root) = staged_with_builder_commit(&repo);
    let promotion = promote_governed_outcome(&task).expect("promote accepted outcome");

    assert_eq!(
        promotion.outcome,
        GovernedPromotionOutcome::Promoted {
            promoted_revision: builder_commit.clone()
        }
    );
    assert_eq!(head(&repo), builder_commit);
}

/// Third #50 P1 finding, the migration half: a pin recorded under the
/// superseded sorted-listing scheme is not comparable with a raw-bytes one, so
/// it must be refused like an absent pin rather than compared across schemes.
#[test]
fn test_promotion_blocks_a_pin_recorded_under_a_superseded_scheme() {
    let (_dir, repo) = init_repo();
    let (mut task, initial, _builder_commit, _root) = staged_with_builder_commit(&repo);
    if let Some(staged) = task.staged_worktree.as_mut() {
        let mut digest = staged
            .shared_config_digest
            .recorded()
            .expect("a freshly materialized worktree is pinned")
            .clone();
        digest.scheme_version = LEGACY_SHARED_REPOSITORY_CONFIG_SCHEME_VERSION;
        staged.shared_config_digest = SharedRepositoryConfigPin::Recorded(digest);
    }

    let promotion =
        promote_governed_outcome(&task).expect("a superseded pin blocks, it does not error");

    assert_eq!(
        promotion.outcome.blocked_reason(),
        Some(PromotionBlockedReason::RepositoryConfigUnpinned)
    );
    assert_eq!(head(&repo), initial);
    assert!(!repo.join("feature.txt").exists());
}
