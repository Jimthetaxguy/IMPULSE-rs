//! Daemon wiring for the governed producers ADR-0012 and ADR-0019 defined but
//! left unreachable over the socket.
//!
//! Three things live here, kept out of `handlers.rs` so that a concurrent lane
//! rebasing that file does not have to merge them:
//!
//! 1. **Staged materialization at registration.** ADR-0019's staged worktree
//!    must exist before the Builder's PTY starts, so a registration that
//!    declares `world_scope = staged_authoritative` materializes it as part of
//!    registration. It is an operator-initiated launch, so a non-operator
//!    connection is refused; a materialization that fails leaves no task
//!    record behind.
//! 2. **The promote and discard endpoints.** Both are operator-class only:
//!    promotion is the step that makes a Builder's work canonical, and discard
//!    destroys work. A *blocked* promotion is an execution fact, not an error —
//!    it answers successfully with the typed outcome recorded against the
//!    already-accepted run.
//! 3. **Durable producer reservations** (ADR-0012's 2026-09-02 amendment) around
//!    verification, Supervisor review, and promotion. The side effect and the
//!    governed-task mutation that records it both run inside
//!    [`crate::state::with_reservation`]'s closure, which is the whole point:
//!    releasing after the side effect alone would reopen the crash window the
//!    journal exists to close.
//!
//! **Not panic-safe, by inheritance.** `with_reservation` has no
//! `catch_unwind`. A panic inside a producer closure leaves the reservation
//! open and propagates, exactly like a process crash: the reservation is
//! reconciled to `needs_rerun` at the next `State::new`, or closed by the
//! revision-scoped duplicate check once the task's revision next advances. It
//! is deliberately *not* treated as an ordinary `Err` return.

use std::path::PathBuf;

use anyhow::{Context, Result};
use impulse_ops::governed_task::{
    GovernedActor, GovernedActorKind, GovernedExecutionState, GovernedPromotion, GovernedRecordId,
    GovernedRequestId, GovernedReviewState, GovernedTaskId, GovernedTaskMutation,
    GovernedTaskMutationRequest, GovernedTaskRegistration, GovernedTaskRun, StagedWorktree,
    StagedWorktreeInput, StagedWorktreeStatus, WorldScope,
};
use impulse_ops::governed_wiring::{
    staged_worktree_is_discardable, unreferenced_accepted_commit_on_discard, GovernedProducerAck,
    GovernedPromotionRequest, GovernedStagedWorktreeDiscardAck,
    GovernedStagedWorktreeDiscardRequest,
};

use super::actor_provenance::{ActorProvenanceError, ConnectionClass};
use super::handlers::{
    persist_governed_mutation, require_current_governed_task, require_producer_request_state,
};
use super::protocol::{respond_err, respond_ok, DaemonRequest, DaemonResponse};
use crate::state::{ProducerKind, SharedState};

/// The actor every daemon-owned staged-worktree and promotion side effect is
/// recorded under. Mirrors `governed_producers::staged_system_actor`, which is
/// private to that module.
fn staged_system_actor() -> GovernedActor {
    GovernedActor {
        kind: GovernedActorKind::System,
        id: "impulse-daemon:staged_worktree".to_string(),
    }
}

/// Refuse a request that mints canonical state, or destroys work, on a
/// connection that never presented this daemon run's operator capability.
///
/// Checked *before* any state read or side effect, exactly like
/// `RecordOperatorDecision`: a non-operator connection must not be able to
/// learn anything, or change anything, by asking.
pub(crate) fn require_operator_class(
    class: ConnectionClass,
    request: &'static str,
) -> Result<(), ActorProvenanceError> {
    if class.is_operator() {
        return Ok(());
    }
    Err(ActorProvenanceError::OperatorClassRequired { request })
}

fn require_staged_scope(task: &GovernedTaskRun, what: &str) -> Result<()> {
    if task.world_scope != WorldScope::StagedAuthoritative {
        anyhow::bail!("governed {what} requires a staged_authoritative world scope");
    }
    Ok(())
}

// ── Staged materialization at registration ──────────────────────────────────

/// A stand-in task record carrying exactly the fields
/// `governed_producers::materialize_staged_worktree` reads: the declared world
/// scope, the attested initial OID, the canonical workspace root, and the task
/// id the staged path is derived from.
///
/// Materializing *before* `register_governed_task` is what makes "a failed
/// materialization leaves no task record" true. The workspace root is
/// canonicalized here the same way the state layer canonicalizes it, so the
/// path this derives and the path the ledger later validates cannot disagree;
/// `register_staged_governed_task` re-checks that equality rather than trusting
/// it.
fn provisional_staged_task(registration: &GovernedTaskRegistration) -> Result<GovernedTaskRun> {
    let workspace_root = PathBuf::from(&registration.workspace_root)
        .canonicalize()
        .with_context(|| {
            format!(
                "Failed to canonicalize governed workspace {}",
                registration.workspace_root
            )
        })?
        .display()
        .to_string();
    Ok(GovernedTaskRun {
        id: registration.task_id.clone(),
        revision: 0,
        project_id: registration.project_id.clone(),
        workspace_root,
        task: registration.task.clone(),
        acceptance_criteria: registration.acceptance_criteria.clone(),
        approval_policy: registration.approval_policy,
        world_scope: registration.world_scope,
        verification_profile: registration.verification_profile,
        role_assignment: registration.role_assignment.clone(),
        role_compatibility: registration.role_compatibility.clone(),
        runtime_id: registration.runtime_id.clone(),
        agent_id: registration.agent_id.clone(),
        session_id: registration.session_id.clone(),
        initial_subject_revision: registration.initial_subject_revision.clone(),
        staged_worktree: None,
        promotions: Vec::new(),
        execution_state: GovernedExecutionState::Registered,
        review_state: GovernedReviewState::AwaitingClaim,
        claims: Vec::new(),
        verifications: Vec::new(),
        supervisor_verdicts: Vec::new(),
        operator_decisions: Vec::new(),
        events: Vec::new(),
        created_at: impulse_ops::now_rfc3339(),
        updated_at: impulse_ops::now_rfc3339(),
    })
}

/// Best-effort removal of a checkout this registration created but could not
/// record, through the same producer a normal discard uses so the
/// administrative entry goes with it. Leaving the entry behind would make the
/// operator's retry fail on a path Git still considers registered.
fn roll_back_staged_checkout(provisional: &GovernedTaskRun, staged: &StagedWorktreeInput) {
    let mut task = provisional.clone();
    let id = match GovernedRecordId::try_new("staged-worktree-rollback") {
        Ok(id) => id,
        Err(error) => {
            tracing::error!(%error, "staged rollback record id is invalid");
            return;
        }
    };
    task.staged_worktree = Some(StagedWorktree {
        id,
        actor: staged.actor.clone(),
        root: staged.root.clone(),
        initial_subject_revision: staged.initial_subject_revision.clone(),
        shared_config_digest: staged.shared_config_digest.clone(),
        status: StagedWorktreeStatus::Active,
        materialized_at: impulse_ops::now_rfc3339(),
        based_on_revision: task.revision,
    });
    if let Err(error) = crate::governed_producers::discard_staged_worktree(&task) {
        tracing::warn!(
            checkout = %staged.root,
            %error,
            "failed to roll back a staged worktree whose registration did not complete; \
             delete the directory and run `git worktree prune` before retrying"
        );
    }
}

/// Deterministic request id for the materialization mutation, so a retry of the
/// same registration replays through the governed ledger's own idempotency
/// receipt instead of recording a second worktree.
fn materialization_request_id(task: &GovernedTaskRun) -> Result<GovernedRequestId> {
    GovernedRequestId::try_new(format!("staged-worktree-{}", task.id))
        .context("staged governed task id does not form a valid mutation request id")
}

/// Register a governed task, materializing its staged worktree first when the
/// registration declares one.
pub(crate) fn register_governed_task(
    state: &SharedState,
    registration: GovernedTaskRegistration,
    connection_class: ConnectionClass,
) -> Result<GovernedTaskRun> {
    if !registration.world_scope.requires_staged_worktree() {
        return state.register_governed_task(registration);
    }
    require_operator_class(
        connection_class,
        "RegisterGovernedTask with a staged world scope",
    )?;

    let provisional = provisional_staged_task(&registration)?;
    let staged = crate::governed_producers::materialize_staged_worktree(&provisional)
        .context("failed to materialize the staged worktree for this registration")?;

    let registered = match state.register_governed_task(registration) {
        Ok(registered) => registered,
        Err(error) => {
            roll_back_staged_checkout(&provisional, &staged);
            return Err(error);
        }
    };

    // The ledger validates the staged root against its own derivation. If the
    // two ever disagreed the mutation below would be refused *after* the
    // checkout existed, so the disagreement is caught here instead.
    let expected = match registered.expected_staged_worktree_root() {
        Ok(expected) => expected,
        Err(error) => {
            roll_back_staged_checkout(&provisional, &staged);
            return Err(error.into());
        }
    };
    if expected.as_path() != std::path::Path::new(&staged.root) {
        roll_back_staged_checkout(&provisional, &staged);
        anyhow::bail!(
            "staged worktree was materialized at {} but the registered task derives {}",
            staged.root,
            expected.display()
        );
    }

    let request_id = match materialization_request_id(&registered) {
        Ok(request_id) => request_id,
        Err(error) => {
            roll_back_staged_checkout(&provisional, &staged);
            return Err(error);
        }
    };
    let mutation_request = GovernedTaskMutationRequest {
        request_id,
        project_id: registered.project_id.clone(),
        task_id: registered.id.clone(),
        expected_revision: registered.revision,
        mutation: GovernedTaskMutation::MaterializeStagedWorktree {
            staged: staged.clone(),
        },
    };
    match state.mutate_governed_task(mutation_request) {
        Ok(task) => Ok(task),
        Err(error) => {
            roll_back_staged_checkout(&provisional, &staged);
            Err(error.context(
                "staged worktree was materialized but could not be recorded; the checkout was \
                 rolled back and the registered task has no staged worktree",
            ))
        }
    }
}

// ── Durable reservation wrapper ─────────────────────────────────────────────

/// Run one producer side effect and persist its governed-task receipt under a
/// durable reservation.
///
/// `build_mutation` must perform the external side effect and return the
/// mutation that records it; this helper persists that mutation *inside* the
/// reservation's closure, because releasing between the two would reopen
/// ADR-0012's crash-between-side-effect-and-receipt window.
pub(crate) async fn reserved_producer<F, Fut>(
    state: &SharedState,
    task_id: &GovernedTaskId,
    expected_revision: u64,
    request_id: &GovernedRequestId,
    producer: ProducerKind,
    build_mutation: F,
) -> Result<GovernedTaskRun>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<GovernedTaskMutationRequest>>,
{
    crate::state::with_reservation(
        state,
        task_id,
        expected_revision,
        request_id,
        producer,
        || async move {
            let mutation_request = build_mutation().await?;
            let receipt = mutation_request.request_id.as_str().to_string();
            let updated = persist_governed_mutation(state, mutation_request).await?;
            Ok((updated, receipt))
        },
    )
    .await
}

/// The reason a previous, interrupted attempt by this exact request id needs a
/// rerun, if any. Surfaced on the acknowledgement so an operator can tell a
/// rerun-after-a-crash apart from ordinary first-time work.
pub(crate) fn pending_rerun_reason(
    state: &SharedState,
    task_id: &GovernedTaskId,
    producer: ProducerKind,
    request_id: &GovernedRequestId,
) -> Option<String> {
    match state.pending_rerun_reason(task_id, producer, request_id) {
        Ok(reason) => reason,
        Err(error) => {
            tracing::warn!(%error, "failed to read the producer reservation journal");
            None
        }
    }
}

// ── Promotion ───────────────────────────────────────────────────────────────

fn replay_promotion(task: &GovernedTaskRun, expected_revision: u64) -> Result<&GovernedPromotion> {
    task.promotions
        .iter()
        .find(|promotion| promotion.based_on_revision == expected_revision)
        .context("stale governed promotion request has no record at its expected revision")
}

async fn promote_governed_outcome(
    state: &SharedState,
    request: GovernedPromotionRequest,
    connection_class: ConnectionClass,
) -> Result<GovernedProducerAck> {
    require_operator_class(connection_class, "PromoteGovernedOutcome")?;
    let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
    let task = require_current_governed_task(state, &request.project_id, &request.task_id)?;
    require_staged_scope(&task, "promotion")?;

    let replay = require_producer_request_state(
        state,
        &task,
        &request.request_id,
        request.expected_revision,
    )?;
    if replay {
        // The receipt already exists: answer from it rather than touching Git
        // a second time.
        replay_promotion(&task, request.expected_revision)?;
        return Ok(GovernedProducerAck::new(task, true, None));
    }

    if !task.is_accepted() {
        anyhow::bail!("governed promotion requires an accepted governed task");
    }
    if task.active_staged_worktree().is_none() {
        anyhow::bail!("governed promotion requires an active staged worktree");
    }

    let pending = pending_rerun_reason(
        state,
        &request.task_id,
        ProducerKind::Promotion,
        &request.request_id,
    );
    let promotion_task = task.clone();
    let promotion_request = request.clone();
    let updated = reserved_producer(
        state,
        &request.task_id,
        request.expected_revision,
        &request.request_id,
        ProducerKind::Promotion,
        || async move {
            let promotion =
                crate::governed_producers::promote_governed_outcome_async(promotion_task).await?;
            Ok(GovernedTaskMutationRequest {
                request_id: promotion_request.request_id,
                project_id: promotion_request.project_id,
                task_id: promotion_request.task_id,
                expected_revision: promotion_request.expected_revision,
                mutation: GovernedTaskMutation::RecordPromotion { promotion },
            })
        },
    )
    .await?;
    Ok(GovernedProducerAck::new(updated, false, pending))
}

// ── Staged-worktree discard ─────────────────────────────────────────────────

async fn discard_governed_staged_worktree(
    state: &SharedState,
    request: GovernedStagedWorktreeDiscardRequest,
    connection_class: ConnectionClass,
) -> Result<GovernedStagedWorktreeDiscardAck> {
    require_operator_class(connection_class, "DiscardGovernedStagedWorktree")?;
    request.validate()?;
    let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
    let task = require_current_governed_task(state, &request.project_id, &request.task_id)?;
    require_staged_scope(&task, "staged worktree discard")?;

    let replay = require_producer_request_state(
        state,
        &task,
        &request.request_id,
        request.expected_revision,
    )?;
    let staged_root = task
        .staged_worktree
        .as_ref()
        .map(|staged| staged.root.clone())
        .context("governed staged worktree discard requires a materialized worktree")?;
    if replay {
        return Ok(GovernedStagedWorktreeDiscardAck {
            task,
            discarded_root: staged_root,
            unreferenced_accepted_commit: None,
        });
    }

    if task.active_staged_worktree().is_none() {
        anyhow::bail!("governed staged worktree was already discarded");
    }
    // Refuse *before* the destructive side effect. The state layer enforces the
    // same rule on the mutation, but by then the checkout would already be
    // gone with nothing recorded.
    if !staged_worktree_is_discardable(&task) {
        anyhow::bail!(
            "a staged worktree is discarded only after a rejection, an escalation, a launch \
             failure, or a recorded promotion outcome; this task is execution={:?} review={:?}",
            task.execution_state,
            task.review_state
        );
    }
    let unreferenced = unreferenced_accepted_commit_on_discard(&task).map(str::to_string);

    crate::governed_producers::discard_staged_worktree_async(task.clone()).await?;
    let mutation_request = GovernedTaskMutationRequest {
        request_id: request.request_id,
        project_id: request.project_id,
        task_id: request.task_id,
        expected_revision: request.expected_revision,
        mutation: GovernedTaskMutation::DiscardStagedWorktree {
            actor: staged_system_actor(),
            reason: request.reason,
        },
    };
    let updated = persist_governed_mutation(state, mutation_request).await?;
    Ok(GovernedStagedWorktreeDiscardAck {
        task: updated,
        discarded_root: staged_root,
        unreferenced_accepted_commit: unreferenced,
    })
}

// ── Dispatch ────────────────────────────────────────────────────────────────

/// Entry point for the two operator-class staged endpoints.
pub(crate) async fn handle_governed_staged_request(
    request: DaemonRequest,
    state: &SharedState,
    connection_class: ConnectionClass,
) -> DaemonResponse {
    match request {
        DaemonRequest::PromoteGovernedOutcome { request } => {
            match promote_governed_outcome(state, request, connection_class).await {
                Ok(ack) => respond_ok(&ack),
                Err(error) => respond_err(error),
            }
        }
        DaemonRequest::DiscardGovernedStagedWorktree { request } => {
            match discard_governed_staged_worktree(state, request, connection_class).await {
                Ok(ack) => respond_ok(&ack),
                Err(error) => respond_err(error),
            }
        }
        _ => respond_err("Internal routing error: not a governed staged request"),
    }
}

// ── Verification and Supervisor review, under a durable reservation ─────────

/// `RunGovernedVerification`, with the fixed-profile command run and its
/// receipt inside one reservation.
pub(crate) async fn handle_governed_verification(
    state: &SharedState,
    request: impulse_ops::governed_task::GovernedVerificationRequest,
) -> DaemonResponse {
    match run_governed_verification(state, request).await {
        Ok(ack) => respond_ok(&ack),
        Err(error) => respond_err(error),
    }
}

async fn run_governed_verification(
    state: &SharedState,
    request: impulse_ops::governed_task::GovernedVerificationRequest,
) -> Result<GovernedProducerAck> {
    // The in-memory per-task lock stays: it keeps two concurrent in-process
    // requests from both reaching `reserve()`, which the durable journal would
    // then have to refuse. It is an optimization, not the crash-safety
    // boundary.
    let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
    let task = require_current_governed_task(state, &request.project_id, &request.task_id)?;
    if task.verification_profile.is_none() {
        anyhow::bail!("governed verification producer requires a closed-loop task profile");
    }
    let replay = require_producer_request_state(
        state,
        &task,
        &request.request_id,
        request.expected_revision,
    )?;
    if replay {
        // A durable receipt already exists, so no reservation is taken and no
        // command is run: the recorded evidence is replayed verbatim.
        let verification =
            super::handlers::replay_verification_input(&task, request.expected_revision)?;
        let updated = persist_governed_mutation(
            state,
            GovernedTaskMutationRequest {
                request_id: request.request_id,
                project_id: request.project_id,
                task_id: request.task_id,
                expected_revision: request.expected_revision,
                mutation: GovernedTaskMutation::RecordVerification { verification },
            },
        )
        .await?;
        return Ok(GovernedProducerAck::new(updated, true, None));
    }

    super::handlers::preflight_verification(&task)?;
    let pending = pending_rerun_reason(
        state,
        &request.task_id,
        ProducerKind::Verification,
        &request.request_id,
    );
    let verification_task = task.clone();
    let producer_request = request.clone();
    let updated = reserved_producer(
        state,
        &request.task_id,
        request.expected_revision,
        &request.request_id,
        ProducerKind::Verification,
        || async move {
            let verification =
                crate::governed_producers::run_verification(&verification_task).await?;
            Ok(GovernedTaskMutationRequest {
                request_id: producer_request.request_id,
                project_id: producer_request.project_id,
                task_id: producer_request.task_id,
                expected_revision: producer_request.expected_revision,
                mutation: GovernedTaskMutation::RecordVerification { verification },
            })
        },
    )
    .await?;
    Ok(GovernedProducerAck::new(updated, false, pending))
}

/// `RunGovernedSupervisorReview`, with the strict API turn and its verdict
/// receipt inside one reservation.
///
/// The managed-agent turn guard is acquired before the reservation and held
/// across it, so this endpoint keeps the same fail-fast `Busy` behavior every
/// other agent handler has.
pub(crate) async fn handle_governed_supervisor_review(
    state: &SharedState,
    request: impulse_ops::governed_task::GovernedSupervisorReviewRequest,
    cached_agent: &std::sync::Arc<tokio::sync::Mutex<Option<crate::agent::ImpulseAgent>>>,
) -> DaemonResponse {
    let _producer_guard = state.acquire_governed_producer_lock(&request.task_id).await;
    let task = match require_current_governed_task(state, &request.project_id, &request.task_id) {
        Ok(task) => task,
        Err(error) => return respond_err(error),
    };
    if task.verification_profile.is_none() {
        return respond_err("governed Supervisor producer requires a closed-loop task profile");
    }
    let replay = match require_producer_request_state(
        state,
        &task,
        &request.request_id,
        request.expected_revision,
    ) {
        Ok(replay) => replay,
        Err(error) => return respond_err(error),
    };

    if replay {
        let verdict =
            match super::handlers::replay_supervisor_input(&task, request.expected_revision) {
                Ok(verdict) => verdict,
                Err(error) => return respond_err(error),
            };
        let mutation_request = GovernedTaskMutationRequest {
            request_id: request.request_id,
            project_id: request.project_id,
            task_id: request.task_id,
            expected_revision: request.expected_revision,
            mutation: GovernedTaskMutation::RecordSupervisorVerdict { verdict },
        };
        return match persist_governed_mutation(state, mutation_request).await {
            Ok(updated) => respond_ok(&GovernedProducerAck::new(updated, true, None)),
            Err(error) => respond_err(error),
        };
    }

    if let Err(error) = super::handlers::preflight_supervisor_review(&task) {
        return respond_err(error);
    }
    let (system_prompt, user_prompt) =
        match crate::governed_producers::supervisor_review_prompt(&task) {
            Ok(prompt) => prompt,
            Err(error) => return respond_err(error),
        };
    let mut agent_guard = match super::handlers::try_lock_agent_for_turn(cached_agent, state) {
        Ok(guard) => guard,
        Err(_) => return super::handlers::agent_turn_busy_response(),
    };
    let agent = match agent_guard.as_mut() {
        Some(agent) if agent.is_ready() => agent,
        Some(_) => {
            return respond_err(
                "Impulse Agent is configured but not ready for governed Supervisor review",
            )
        }
        None => {
            return respond_err(
                "Impulse Agent must be configured before governed Supervisor review",
            )
        }
    };
    let supervisor_actor = agent.governed_review_actor();

    let pending = pending_rerun_reason(
        state,
        &request.task_id,
        ProducerKind::SupervisorReview,
        &request.request_id,
    );
    let review_task = task.clone();
    let producer_request = request.clone();
    let result = reserved_producer(
        state,
        &request.task_id,
        request.expected_revision,
        &request.request_id,
        ProducerKind::SupervisorReview,
        || async move {
            let response = agent
                .query_stateless(&system_prompt, &user_prompt)
                .await
                .map_err(|error| {
                    anyhow::anyhow!("governed Supervisor review turn failed: {error}")
                })?;
            let verdict = crate::governed_producers::bind_supervisor_review(
                &review_task,
                &response,
                supervisor_actor,
            )?;
            Ok(GovernedTaskMutationRequest {
                request_id: producer_request.request_id,
                project_id: producer_request.project_id,
                task_id: producer_request.task_id,
                expected_revision: producer_request.expected_revision,
                mutation: GovernedTaskMutation::RecordSupervisorVerdict { verdict },
            })
        },
    )
    .await;
    match result {
        Ok(updated) => respond_ok(&GovernedProducerAck::new(updated, false, pending)),
        Err(error) => respond_err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    use impulse_ops::agent_registry::{AgentPlatformId, AgentRegistry};
    use impulse_ops::governed_task::{
        GovernedCommandEvidence, GovernedPromotionOutcome, GovernedRequestId,
        GovernedTaskEventKind, GovernedTaskRegistration, GovernedVerificationInput,
        GovernedVerificationOutcome, GovernedVerificationProfile, GovernedVerificationRequest,
        OperatorDecisionInput, OperatorDecisionKind, PromotionBlockedReason, StagedWorktreeStatus,
        SupervisorVerdictInput, SupervisorVerdictKind, WorkerCompletionClaimInput,
    };
    use impulse_ops::role_assignment::{
        canonical_governed_builder_assignment, AgentRoleAssignment, RoleCompatibility,
    };

    use super::*;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            // The harness's own Git must not run project hooks either.
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("git {args:?} must launch: {error}"));
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

    /// A real Rust workspace in a real Git repository plus its project state,
    /// matching the fixture the governed producer handler tests already use.
    fn repo_state() -> (tempfile::TempDir, SharedState, String, String) {
        let repo = tempfile::Builder::new()
            .prefix("impulse-governed-wiring-")
            .tempdir()
            .unwrap();
        std::fs::create_dir(repo.path().join("src")).unwrap();
        std::fs::write(
            repo.path().join("Cargo.toml"),
            "[package]\nname = \"wiring_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join("src/lib.rs"),
            "pub fn wiring_fixture() -> bool {\n    true\n}\n",
        )
        .unwrap();
        // The ignore list `impulse init` writes. Without it an ungitignored
        // `.impulse` makes the canonical tree dirty the moment a runtime ledger
        // is written, and every later governed observation fails.
        let mut ignores = String::from("target/\n");
        for entry in crate::handlers::config::repo_runtime_gitignore_entries() {
            ignores.push_str(entry);
            ignores.push('\n');
        }
        std::fs::write(repo.path().join(".gitignore"), ignores).unwrap();
        let lock_status = Command::new("cargo")
            .arg("generate-lockfile")
            .current_dir(repo.path())
            .status()
            .expect("Cargo lockfile generation must launch");
        assert!(lock_status.success(), "Cargo lockfile generation failed");
        git(repo.path(), &["init", "--quiet", "--initial-branch=main"]);
        git(
            repo.path(),
            &["config", "user.email", "lane@example.invalid"],
        );
        git(repo.path(), &["config", "user.name", "Wiring Lane"]);
        git(
            repo.path(),
            &[
                "add",
                ".gitignore",
                "Cargo.toml",
                "Cargo.lock",
                "src/lib.rs",
            ],
        );
        git(repo.path(), &["commit", "--quiet", "-m", "initial"]);
        let oid = git(repo.path(), &["rev-parse", "HEAD"]);
        let project_id = impulse_ops::sanitize_id(
            &repo
                .path()
                .file_name()
                .expect("temp repo has a name")
                .to_string_lossy(),
        );
        let state: SharedState = Arc::new(
            crate::state::State::new(repo.path().join(".impulse"))
                .expect("project state must initialize"),
        );
        (repo, state, project_id, oid)
    }

    fn profiled_role() -> (AgentRoleAssignment, RoleCompatibility) {
        let assignment = canonical_governed_builder_assignment();
        let platform = AgentPlatformId::try_new("ion").unwrap();
        let compatibility = AgentRegistry::builtin()
            .evaluate_role_compatibility(&platform, &assignment)
            .unwrap();
        (assignment, compatibility)
    }

    fn registration(
        project_id: &str,
        workspace_root: &Path,
        oid: &str,
        scope: WorldScope,
    ) -> GovernedTaskRegistration {
        let (assignment, compatibility) = profiled_role();
        GovernedTaskRegistration::builder(
            "register-wiring",
            "task-wiring",
            project_id.to_string(),
            workspace_root.display().to_string(),
            "prove the daemon governed wiring",
            "worker-wiring",
            "ion",
        )
        .acceptance_criteria(vec!["the endpoint is reachable".to_string()])
        .world_scope(scope)
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

    fn digest(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn mutate(
        state: &SharedState,
        task: &GovernedTaskRun,
        id: &str,
        mutation: GovernedTaskMutation,
    ) -> GovernedTaskRun {
        state
            .mutate_governed_task(GovernedTaskMutationRequest {
                request_id: request_id(id),
                project_id: task.project_id.clone(),
                task_id: task.id.clone(),
                expected_revision: task.revision,
                mutation,
            })
            .unwrap_or_else(|error| panic!("mutation `{id}` must apply: {error}"))
    }

    /// Drive a registered staged task to `accepted` through the state layer
    /// directly.
    ///
    /// The daemon-owned producer chain is deliberately not used: this lane
    /// tests the *endpoints*, and an end-to-end staged verification does not
    /// pass on this base (the sibling ADR-0019 P1 lane owns that fix). The
    /// claim's subject is a real commit made inside the staged worktree, which
    /// is what promotion actually inspects.
    fn accepted_staged_task(
        state: &SharedState,
        task: GovernedTaskRun,
    ) -> (GovernedTaskRun, String) {
        let staged_root = task
            .active_staged_worktree()
            .expect("registration materialized a staged worktree")
            .root
            .clone();
        let staged = Path::new(&staged_root);
        std::fs::write(
            staged.join("src/lib.rs"),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
        )
        .unwrap();
        git(staged, &["add", "src/lib.rs"]);
        git(staged, &["commit", "--quiet", "-m", "builder work"]);
        let accepted_oid = git(staged, &["rev-parse", "HEAD"]);

        let running = mutate(
            state,
            &task,
            "running-wiring",
            GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        );
        let claimed = mutate(
            state,
            &running,
            "claim-wiring",
            GovernedTaskMutation::SubmitClaim {
                claim: WorkerCompletionClaimInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::Worker,
                        id: "worker-wiring".to_string(),
                    },
                    summary: "work complete".to_string(),
                    subject_revision: accepted_oid.clone(),
                    artifact_ids: Vec::new(),
                    diff_ref: None,
                },
            },
        );
        let verified = mutate(
            state,
            &claimed,
            "verify-wiring",
            GovernedTaskMutation::RecordVerification {
                verification: GovernedVerificationInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::Verifier,
                        id: "wiring-verifier".to_string(),
                    },
                    claim_id: claimed.latest_claim().unwrap().id.clone(),
                    subject_revision: accepted_oid.clone(),
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
                    artifact_ids: Vec::new(),
                    notes: None,
                },
            },
        );
        let judged = mutate(
            state,
            &verified,
            "review-wiring",
            GovernedTaskMutation::RecordSupervisorVerdict {
                verdict: SupervisorVerdictInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::Supervisor,
                        id: "wiring-supervisor".to_string(),
                    },
                    verification_id: verified.latest_verification().unwrap().id.clone(),
                    verdict: SupervisorVerdictKind::RecommendAccept,
                    rationale: "every criterion is supported".to_string(),
                },
            },
        );
        let accepted = mutate(
            state,
            &judged,
            "approve-wiring",
            GovernedTaskMutation::RecordOperatorDecision {
                decision: OperatorDecisionInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::Operator,
                        id: "wiring-operator".to_string(),
                    },
                    supervisor_verdict_id: judged.latest_supervisor_verdict().unwrap().id.clone(),
                    decision: OperatorDecisionKind::Approve,
                    rationale: "approved".to_string(),
                },
            },
        );
        assert_eq!(accepted.review_state, GovernedReviewState::Accepted);
        (accepted, accepted_oid)
    }

    fn promotion_request(task: &GovernedTaskRun, id: &str) -> GovernedPromotionRequest {
        GovernedPromotionRequest {
            request_id: request_id(id),
            project_id: task.project_id.clone(),
            task_id: task.id.clone(),
            expected_revision: task.revision,
        }
    }

    fn discard_request(task: &GovernedTaskRun, id: &str) -> GovernedStagedWorktreeDiscardRequest {
        GovernedStagedWorktreeDiscardRequest {
            request_id: request_id(id),
            project_id: task.project_id.clone(),
            task_id: task.id.clone(),
            expected_revision: task.revision,
            reason: "operator reclaimed the checkout".to_string(),
        }
    }

    // ── Registration ────────────────────────────────────────────────────────

    #[test]
    fn staged_registration_materializes_the_worktree_before_any_launch() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .expect("an operator may register a staged governed task");

        let staged = registered
            .active_staged_worktree()
            .expect("registration must materialize the staged worktree");
        assert_eq!(staged.status, StagedWorktreeStatus::Active);
        assert_eq!(
            Path::new(&staged.root),
            registered.expected_staged_worktree_root().unwrap()
        );
        assert!(
            Path::new(&staged.root).join(".git").exists(),
            "the staged root must be a real linked worktree"
        );
        assert_eq!(
            git(Path::new(&staged.root), &["rev-parse", "HEAD"]),
            oid,
            "the staged worktree starts at the attested initial OID"
        );
        assert_eq!(registered.launch_working_directory(), staged.root);
        assert_eq!(
            registered.execution_state,
            GovernedExecutionState::Registered,
            "materialization must not advance the launch lifecycle"
        );
        assert!(registered
            .events
            .iter()
            .any(|event| event.kind == GovernedTaskEventKind::StagedWorktreeMaterialized));
    }

    #[test]
    fn a_non_operator_connection_cannot_register_a_staged_task() {
        let (repo, state, project_id, oid) = repo_state();
        let registration = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );
        let task_id = registration.task_id.clone();
        let error = register_governed_task(&state, registration, ConnectionClass::NonOperator)
            .expect_err("a launched runtime must not be able to stage its own world scope");
        assert!(
            error.to_string().contains("operator-class connection"),
            "expected a typed authorization refusal, got: {error}"
        );
        assert!(
            state
                .get_governed_task(&project_id, &task_id)
                .unwrap()
                .is_none(),
            "a refused staged registration records nothing"
        );
        assert!(
            !repo.path().join(".impulse/worktrees").exists(),
            "a refused staged registration materializes nothing"
        );
    }

    #[test]
    fn an_unstaged_registration_needs_no_operator_class() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(&project_id, repo.path(), &oid, WorldScope::Authoritative),
            ConnectionClass::NonOperator,
        )
        .expect("an authoritative registration keeps its pre-ADR-0019 behavior");
        assert!(registered.staged_worktree.is_none());
        assert_eq!(registered.world_scope, WorldScope::Authoritative);
    }

    #[test]
    fn a_staged_registration_that_cannot_materialize_leaves_no_task_record() {
        let (repo, state, project_id, oid) = repo_state();
        // A directory surviving an interrupted run makes the producer fail
        // closed rather than adopt someone else's half-finished tree.
        let registration = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );
        let task_id = registration.task_id.clone();
        let squatter = repo
            .path()
            .join(".impulse")
            .join("worktrees")
            .join(task_id.as_str());
        std::fs::create_dir_all(&squatter).unwrap();

        let error = register_governed_task(&state, registration, ConnectionClass::Operator)
            .expect_err("materialization must fail on an occupied staged path");
        assert!(
            format!("{error:#}").contains("already exists"),
            "the error must name the recovery, got: {error:#}"
        );
        assert!(
            state
                .get_governed_task(&project_id, &task_id)
                .unwrap()
                .is_none(),
            "a failed materialization leaves no task record"
        );
    }

    // ── Promotion ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_non_operator_connection_cannot_promote_and_changes_nothing() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .unwrap();
        let (accepted, _) = accepted_staged_task(&state, registered);

        let error = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "promote-refused"),
            ConnectionClass::NonOperator,
        )
        .await
        .expect_err("promotion is the step that makes work canonical");
        assert!(error.to_string().contains("operator-class connection"));
        assert!(error.to_string().contains("PromoteGovernedOutcome"));

        let after = state
            .get_governed_task(&project_id, &accepted.id)
            .unwrap()
            .unwrap();
        assert_eq!(after, accepted, "a refused promotion mutates nothing");
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]),
            oid,
            "the canonical branch never moved"
        );
    }

    #[tokio::test]
    async fn an_operator_promotion_fast_forwards_the_canonical_branch() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .unwrap();
        let (accepted, accepted_oid) = accepted_staged_task(&state, registered);

        let ack = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "promote-ok"),
            ConnectionClass::Operator,
        )
        .await
        .expect("an operator may promote an accepted staged outcome");
        assert!(!ack.replayed);

        let promotion = ack
            .task
            .latest_promotion()
            .expect("a promotion is recorded");
        assert_eq!(
            promotion.outcome,
            GovernedPromotionOutcome::Promoted {
                promoted_revision: accepted_oid.clone(),
            }
        );
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), accepted_oid);
        assert_eq!(
            git(repo.path(), &["symbolic-ref", "--quiet", "HEAD"]),
            "refs/heads/main"
        );
        assert_eq!(
            git(
                repo.path(),
                &["status", "--porcelain", "--untracked-files=no"]
            ),
            "",
            "the working tree was synced to the promoted commit"
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap(),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
            "promotion syncs the canonical working tree, not just the ref"
        );
        assert_eq!(ack.task.review_state, GovernedReviewState::Accepted);
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&ack.task),
            None,
            "a promoted commit is on the canonical branch"
        );
    }

    #[tokio::test]
    async fn a_canonical_head_that_moved_blocks_promotion_as_a_successful_response() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .unwrap();
        let (accepted, accepted_oid) = accepted_staged_task(&state, registered);

        // Someone else advanced the canonical branch while the Builder worked.
        std::fs::write(repo.path().join("NOTES.md"), "canonical work\n").unwrap();
        git(repo.path(), &["add", "NOTES.md"]);
        git(repo.path(), &["commit", "--quiet", "-m", "canonical move"]);
        let moved = git(repo.path(), &["rev-parse", "HEAD"]);
        assert_ne!(moved, oid);

        let ack = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "promote-blocked"),
            ConnectionClass::Operator,
        )
        .await
        .expect("a blocked promotion is an execution fact, not an error");

        let promotion = ack.task.latest_promotion().expect("the block is recorded");
        assert_eq!(
            promotion.outcome,
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: moved.clone(),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            }
        );
        assert_eq!(
            ack.task.review_state,
            GovernedReviewState::Accepted,
            "a blocked promotion leaves review state alone"
        );
        assert!(
            ack.task.active_staged_worktree().is_some(),
            "the operator may reconcile and retry"
        );
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), moved);
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&ack.task),
            Some(accepted_oid.as_str())
        );
    }

    // ── Discard ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_non_operator_connection_cannot_discard_a_staged_worktree() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .unwrap();
        let staged_root = registered.active_staged_worktree().unwrap().root.clone();

        let error = discard_governed_staged_worktree(
            &state,
            discard_request(&registered, "discard-refused"),
            ConnectionClass::NonOperator,
        )
        .await
        .expect_err("a Builder must not be able to delete the evidence of its own run");
        assert!(error.to_string().contains("operator-class connection"));
        assert!(error.to_string().contains("DiscardGovernedStagedWorktree"));
        assert!(Path::new(&staged_root).exists(), "the checkout survives");
        assert_eq!(
            state
                .get_governed_task(&project_id, &registered.id)
                .unwrap()
                .unwrap(),
            registered
        );
    }

    #[tokio::test]
    async fn a_live_staged_worktree_is_refused_and_a_blocked_run_is_reclaimed() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .unwrap();
        let staged_root = registered.active_staged_worktree().unwrap().root.clone();

        // Refused while the run is live, *before* anything is deleted.
        let error = discard_governed_staged_worktree(
            &state,
            discard_request(&registered, "discard-live"),
            ConnectionClass::Operator,
        )
        .await
        .expect_err("a live Builder's checkout is not reclaimable");
        assert!(
            error.to_string().contains("only after a rejection"),
            "expected a typed reason, got: {error}"
        );
        assert!(Path::new(&staged_root).exists());

        let (accepted, accepted_oid) = accepted_staged_task(&state, registered);
        std::fs::write(repo.path().join("NOTES.md"), "canonical work\n").unwrap();
        git(repo.path(), &["add", "NOTES.md"]);
        git(repo.path(), &["commit", "--quiet", "-m", "canonical move"]);
        let blocked = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "discard-blocked-promote"),
            ConnectionClass::Operator,
        )
        .await
        .unwrap()
        .task;

        let ack = discard_governed_staged_worktree(
            &state,
            discard_request(&blocked, "discard-after-block"),
            ConnectionClass::Operator,
        )
        .await
        .expect("a blocked run's checkout may be reclaimed");
        assert_eq!(ack.discarded_root, staged_root);
        assert_eq!(
            ack.unreferenced_accepted_commit.as_deref(),
            Some(accepted_oid.as_str()),
            "the operator must be told which commit lost its only ref"
        );
        assert!(!Path::new(&staged_root).exists(), "the checkout is gone");
        assert_eq!(
            ack.task.staged_worktree.as_ref().unwrap().status,
            StagedWorktreeStatus::Discarded
        );
        assert!(ack.task.active_staged_worktree().is_none());
        assert!(ack
            .task
            .events
            .iter()
            .any(|event| event.kind == GovernedTaskEventKind::StagedWorktreeDiscarded));
    }

    // ── Durable producer reservations (ADR-0012 amendment) ──────────────────

    /// Register an *authoritative* profiled task and drive it to
    /// `awaiting_verification`, which is what the reservation tests need.
    ///
    /// Deliberately not staged: an end-to-end staged verification does not pass
    /// on this base, and the reservation contract is scope-independent.
    fn awaiting_verification(
        state: &SharedState,
        repo: &Path,
        project_id: &str,
        oid: &str,
    ) -> GovernedTaskRun {
        let registered = register_governed_task(
            state,
            registration(project_id, repo, oid, WorldScope::Authoritative),
            ConnectionClass::Operator,
        )
        .unwrap();
        let running = mutate(
            state,
            &registered,
            "running-reservation",
            GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        );
        mutate(
            state,
            &running,
            "claim-reservation",
            GovernedTaskMutation::SubmitClaim {
                claim: WorkerCompletionClaimInput {
                    actor: GovernedActor {
                        kind: GovernedActorKind::Worker,
                        id: "worker-wiring".to_string(),
                    },
                    summary: "work complete".to_string(),
                    subject_revision: oid.to_string(),
                    artifact_ids: Vec::new(),
                    diff_ref: None,
                },
            },
        )
    }

    #[tokio::test]
    async fn a_live_same_revision_reservation_refuses_a_second_verification() {
        let (repo, state, project_id, oid) = repo_state();
        let claimed = awaiting_verification(&state, repo.path(), &project_id, &oid);

        // Stand in for a verification genuinely still in flight.
        let held = state
            .reserve(
                &claimed.id,
                claimed.revision,
                &request_id("verify-in-flight"),
                ProducerKind::Verification,
            )
            .expect("the first reservation is taken");

        let error = run_governed_verification(
            &state,
            GovernedVerificationRequest {
                request_id: request_id("verify-duplicate"),
                project_id: project_id.clone(),
                task_id: claimed.id.clone(),
                expected_revision: claimed.revision,
            },
        )
        .await
        .expect_err("a competing run at the same revision must be refused");
        assert!(
            error.to_string().contains("open producer reservation"),
            "expected the journal's typed duplicate error, got: {error}"
        );

        // Releasing the in-flight reservation unblocks the next attempt.
        state.release(&held, "verify-in-flight").unwrap();
        assert!(state.open_reservations().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_interrupted_reservation_is_visible_and_the_rerun_proceeds() {
        let (repo, state, project_id, oid) = repo_state();
        let claimed = awaiting_verification(&state, repo.path(), &project_id, &oid);
        let replayed_request = request_id("verify-interrupted");

        // A process that died between the side effect and its receipt.
        state
            .reserve(
                &claimed.id,
                claimed.revision,
                &replayed_request,
                ProducerKind::Verification,
            )
            .unwrap();
        drop(state);

        let state: SharedState = Arc::new(
            crate::state::State::new(repo.path().join(".impulse"))
                .expect("the daemon reloads and reconciles"),
        );
        let reloaded = state
            .get_governed_task(&project_id, &claimed.id)
            .unwrap()
            .unwrap();
        assert!(
            reloaded.events.iter().any(|event| {
                event.kind == GovernedTaskEventKind::ProducerReservationInterrupted
                    && event.detail.contains("verify-interrupted")
            }),
            "the interruption must be visible on the task's own event chain: {:?}",
            reloaded.events
        );
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "reconcile closes the interrupted reservation rather than blocking forever"
        );
        assert_eq!(
            state
                .pending_rerun_reason(&reloaded.id, ProducerKind::Verification, &replayed_request)
                .unwrap()
                .as_deref(),
            Some("interrupted before receipt")
        );

        // The rerun is not blocked, and it says why it is rerunning.
        let ack = run_governed_verification(
            &state,
            GovernedVerificationRequest {
                request_id: replayed_request,
                project_id: project_id.clone(),
                task_id: reloaded.id.clone(),
                expected_revision: reloaded.revision,
            },
        )
        .await
        .expect("the rerun proceeds");
        assert!(!ack.replayed, "no receipt was ever recorded");
        assert_eq!(
            ack.pending_rerun_reason.as_deref(),
            Some("interrupted before receipt")
        );
        assert!(ack.task.latest_verification().is_some());
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "the rerun released its own reservation"
        );
    }

    #[tokio::test]
    async fn a_fresh_verification_reports_no_pending_rerun() {
        let (repo, state, project_id, oid) = repo_state();
        let claimed = awaiting_verification(&state, repo.path(), &project_id, &oid);
        let ack = run_governed_verification(
            &state,
            GovernedVerificationRequest {
                request_id: request_id("verify-fresh"),
                project_id: project_id.clone(),
                task_id: claimed.id.clone(),
                expected_revision: claimed.revision,
            },
        )
        .await
        .expect("a first verification runs");
        assert!(!ack.replayed);
        assert_eq!(ack.pending_rerun_reason, None);
        assert!(state.open_reservations().unwrap().is_empty());
    }

    // ── Routing and refusals ────────────────────────────────────────────────

    #[tokio::test]
    async fn the_staged_dispatcher_refuses_an_unrelated_request() {
        let (_repo, state, _project_id, _oid) = repo_state();
        let response =
            handle_governed_staged_request(DaemonRequest::Ping, &state, ConnectionClass::Operator)
                .await;
        match response {
            DaemonResponse::Error { message } => {
                assert!(message.contains("Internal routing error"));
            }
            other => panic!("expected a routing refusal, received {other:?}"),
        }
    }

    #[tokio::test]
    async fn promotion_requires_a_staged_world_scope() {
        let (repo, state, project_id, oid) = repo_state();
        let registered = register_governed_task(
            &state,
            registration(&project_id, repo.path(), &oid, WorldScope::Authoritative),
            ConnectionClass::Operator,
        )
        .unwrap();
        let error = promote_governed_outcome(
            &state,
            promotion_request(&registered, "promote-authoritative"),
            ConnectionClass::Operator,
        )
        .await
        .expect_err("an authoritative task has nothing to promote");
        assert!(error
            .to_string()
            .contains("staged_authoritative world scope"));
    }

    #[test]
    fn require_operator_class_names_the_refused_request() {
        let error = require_operator_class(ConnectionClass::NonOperator, "PromoteGovernedOutcome")
            .expect_err("a non-operator connection is refused");
        assert!(error.to_string().contains("PromoteGovernedOutcome"));
        assert!(error.to_string().contains("operator-class connection"));
        assert!(
            require_operator_class(ConnectionClass::Operator, "PromoteGovernedOutcome").is_ok()
        );
    }
}
