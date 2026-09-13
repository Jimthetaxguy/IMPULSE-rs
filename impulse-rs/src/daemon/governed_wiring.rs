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
//!    record behind. Because the checkout is created *before* the ledger write,
//!    a replay must be recognized before the producer runs: the request id is
//!    checked against the governed ledger's own receipts first, so retrying a
//!    registration that already succeeded (a client that timed out on a slow
//!    `git worktree add`, say) returns the recorded task instead of failing on
//!    a staged path that is "already occupied" by its own earlier attempt.
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
    GovernedPromotionRequest, GovernedStagedConfigRefusalAck, GovernedStagedWorktreeDiscardAck,
    GovernedStagedWorktreeDiscardRequest, StagedConfigRefusalReason,
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

/// Authorize a registration by world scope, before any of its caller-supplied
/// fields are used.
///
/// Split out of [`register_governed_task`] so the dispatcher can run it at the
/// very top of the `RegisterGovernedTask` arm: the profiled preflight there
/// spawns `git` inside a caller-chosen path, which a non-operator connection
/// must not be able to reach for a scope it is not allowed to request.
/// `register_governed_task` still performs the same check, so the function is
/// safe on its own; this is ordering, not a relocation.
pub(crate) fn require_staged_registration_class(
    registration: &GovernedTaskRegistration,
    class: ConnectionClass,
) -> Result<(), ActorProvenanceError> {
    if !registration.world_scope.requires_staged_worktree() {
        return Ok(());
    }
    require_operator_class(class, "RegisterGovernedTask with a staged world scope")
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
    require_staged_registration_class(&registration, connection_class)?;

    // Replay before side effect. `materialize_staged_worktree` runs before the
    // ledger write (that is what makes "a failed materialization leaves no task
    // record" true), which means a retry of a registration that already
    // succeeded would otherwise reach the producer and fail on a staged path
    // occupied by its own earlier attempt -- with a recovery message telling the
    // operator to delete a directory that is in fact a live Builder's checkout.
    // A client retry is not hypothetical: `DaemonClient` re-sends acknowledged
    // requests, and `git worktree add` on a large repository can outlast the
    // response timeout. Consulting the governed ledger's own receipts first
    // makes the replay idempotent, and `register_governed_task` then
    // fingerprint-checks the replayed request against the recorded one.
    if state.governed_producer_request_is_replay(&registration.request_id, &registration.task_id)? {
        return state
            .register_governed_task(registration)
            .context("failed to replay a recorded staged governed registration");
    }

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

/// Admission check for a promotion attempt, before a reservation is taken or a
/// Git process is spawned.
///
/// It calls the ledger's own `RecordPromotion` predicate rather than restating
/// it. An earlier version checked only "accepted" and "has an active staged
/// worktree", which is strictly weaker: an accepted task with a worktree but no
/// claim, or one already promoted, was admitted here and then refused by the
/// ledger — after the reservation had been taken and the producer called. The
/// cross-check matrix test is what surfaced that.
fn promote_preflight(task: &GovernedTaskRun) -> Result<()> {
    crate::state::record_promotion_preconditions_hold(task)
        .map_err(|failure| anyhow::anyhow!("governed promotion refused: {failure}"))
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

    promote_preflight(&task)?;

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
        // Recomputed, not `None`. `DaemonClient` retries an acknowledged
        // request, so the *first* response carrying this warning can be lost
        // and the retry is then the only copy the operator ever sees. The
        // recorded promotion outcome is still on the task, so the answer is
        // derivable rather than needing to have been remembered.
        let unreferenced = unreferenced_accepted_commit_on_discard(&task).map(str::to_string);
        return Ok(GovernedStagedWorktreeDiscardAck {
            task,
            discarded_root: staged_root,
            unreferenced_accepted_commit: unreferenced,
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
            handle_governed_promotion(state, request, connection_class).await
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

// ── Typed staged-configuration refusal (ADR-0019 rule 13) ───────────────────

/// Recognize a producer's refusal to run Git inside a staged worktree.
///
/// `StagedConfigRefusal` is raised before any Git process is spawned, so it is
/// not a run failure and must not be reported as one: nothing is broken, the
/// staged world is in a state the daemon may not touch. Downcasting it here is
/// what lets the endpoint answer with a typed reason and its remedy instead of
/// an error string a surface would have to parse.
pub(crate) fn staged_config_refusal(error: &anyhow::Error) -> Option<StagedConfigRefusalReason> {
    use crate::governed_producers::StagedConfigRefusal;
    error
        .downcast_ref::<StagedConfigRefusal>()
        .map(|refusal| match refusal {
            StagedConfigRefusal::Unpinned { .. } => StagedConfigRefusalReason::Unpinned,
            StagedConfigRefusal::Changed { component, .. } => StagedConfigRefusalReason::Changed {
                component: *component,
            },
            StagedConfigRefusal::UnsupportedSubmodules { path, .. } => {
                StagedConfigRefusalReason::UnsupportedSubmodules { path: path.clone() }
            }
        })
}

/// Answer a producer error: a staged-configuration refusal becomes a typed
/// successful-shape response; everything else stays an error.
///
/// `task` is the record as it stood before the producer ran. A refusal mutates
/// nothing, so echoing it back is accurate rather than a convenience.
pub(crate) fn respond_producer_error(
    task: &GovernedTaskRun,
    error: anyhow::Error,
) -> DaemonResponse {
    match staged_config_refusal(&error) {
        Some(reason) => respond_ok(&GovernedStagedConfigRefusalAck::new(task.clone(), reason)),
        // `{:#}` so a genuine failure keeps its context chain; `Display` alone
        // would drop the layer carrying the operator's recovery text.
        None => respond_err(format!("{error:#}")),
    }
}

/// `PromoteGovernedOutcome`, with a staged-configuration refusal answered in
/// the same typed shape `SubmitGovernedClaim` and `RunGovernedVerification`
/// use, instead of the error string it used to fall through to.
///
/// The operator-class check runs first and on its own: the refusal path
/// re-reads the task before the producer runs, and a non-operator connection
/// must not cause even that read — v9 documents the operator-class requests as
/// checked before any state read. `promote_governed_outcome` repeats the check;
/// the repetition is cheap and keeps that function safe to call on its own.
///
/// Of promotion's two staged-configuration gates only the submodule check
/// reaches this path. A drifted pin at promotion is ADR-0019 rule 6's recorded
/// `promotion_blocked { repository_config_changed }` on the accepted run —
/// there is an accepted run to record it against, which the claim and
/// verification producers never have — so it arrives here as an `Ok` and is
/// answered as an ordinary producer acknowledgement.
pub(crate) async fn handle_governed_promotion(
    state: &SharedState,
    request: GovernedPromotionRequest,
    connection_class: ConnectionClass,
) -> DaemonResponse {
    if let Err(error) = require_operator_class(connection_class, "PromoteGovernedOutcome") {
        return respond_err(error);
    }
    // The task is re-read for the refusal path only: a refusal records nothing,
    // so the response echoes the record exactly as it stood. A lookup failure
    // here cannot mask the real error -- it falls through to reporting it.
    let unchanged =
        require_current_governed_task(state, &request.project_id, &request.task_id).ok();
    match promote_governed_outcome(state, request, connection_class).await {
        Ok(ack) => respond_ok(&ack),
        Err(error) => match unchanged {
            Some(task) => respond_producer_error(&task, error),
            None => respond_err(format!("{error:#}")),
        },
    }
}

// ── Verification and Supervisor review, under a durable reservation ─────────

/// `RunGovernedVerification`, with the fixed-profile command run and its
/// receipt inside one reservation.
pub(crate) async fn handle_governed_verification(
    state: &SharedState,
    request: impulse_ops::governed_task::GovernedVerificationRequest,
) -> DaemonResponse {
    // The task is re-read for the refusal path only: a refusal records nothing,
    // so the response echoes the record exactly as it stood. A lookup failure
    // here cannot mask the real error -- it falls through to reporting it.
    let unchanged =
        require_current_governed_task(state, &request.project_id, &request.task_id).ok();
    match run_governed_verification(state, request).await {
        Ok(ack) => respond_ok(&ack),
        Err(error) => match unchanged {
            Some(task) => respond_producer_error(&task, error),
            None => respond_err(format!("{error:#}")),
        },
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
        repo_state_with_runtime_ignores(true)
    }

    /// The same fixture without the ignore list `impulse init` writes.
    ///
    /// This is the configuration every `.impulse` cleanliness exemption exists
    /// for, and the one the ignore-list fixture cannot reach: with the list
    /// present, a missing exemption is invisible because Git never reports the
    /// file at all.
    fn repo_state_without_runtime_ignores() -> (tempfile::TempDir, SharedState, String, String) {
        repo_state_with_runtime_ignores(false)
    }

    fn repo_state_with_runtime_ignores(
        runtime_ignores: bool,
    ) -> (tempfile::TempDir, SharedState, String, String) {
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
        // is written, and every later governed observation fails -- which is
        // exactly what `repo_state_without_runtime_ignores` exists to exercise.
        let mut ignores = String::from("target/\n");
        if runtime_ignores {
            for entry in crate::handlers::config::repo_runtime_gitignore_entries() {
                ignores.push_str(entry);
                ignores.push('\n');
            }
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
        assert_eq!(
            registered
                .launch_working_directory()
                .expect("a materialized staged task has a launch working directory"),
            staged.root
        );
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

    /// An unpinned staged worktree cannot be launched, and can always be
    /// reclaimed — the endpoint path, driven forward with no ledger surgery.
    ///
    /// `require_shared_config_digest` accepts `Unknown` as a legitimate stored
    /// value, so submitting the materialization mutation with an unpinned input
    /// produces a genuine pre-pin record with a naturally computed receipt.
    /// From there #53's `MarkRunning` precondition refuses the launch, which is
    /// precisely why discard must be available: an unpinned worktree can never
    /// be promoted *and* can never be worked in, so reclaiming it is the only
    /// way forward.
    #[tokio::test]
    async fn an_unpinned_staged_worktree_cannot_be_launched_but_can_be_reclaimed() {
        let (repo, state, project_id, oid) = repo_state();
        let registration = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );

        // Registered without this lane's auto-materializing wrapper, then the
        // worktree recorded the way a build predating the pin would record it.
        let registered = state.register_governed_task(registration).unwrap();
        let staged_input =
            crate::governed_producers::materialize_staged_worktree(&registered).unwrap();
        let staged_root = staged_input.root.clone();
        let unpinned = mutate(
            &state,
            &registered,
            "unpinned-materialize",
            GovernedTaskMutation::MaterializeStagedWorktree {
                staged: StagedWorktreeInput {
                    shared_config_digest:
                        impulse_ops::governed_task::SharedRepositoryConfigPin::Unknown,
                    ..staged_input
                },
            },
        );
        assert!(
            unpinned
                .staged_worktree
                .as_ref()
                .is_some_and(|staged| staged.shared_config_digest.is_unknown()),
            "the fixture must actually be unpinned or it proves nothing"
        );

        // #53: no launch without a comparable pin.
        let launch = state.mutate_governed_task(GovernedTaskMutationRequest {
            request_id: request_id("unpinned-running"),
            project_id: project_id.clone(),
            task_id: unpinned.id.clone(),
            expected_revision: unpinned.revision,
            mutation: GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        });
        let launch_error = launch.expect_err("an unpinned worktree must not be launched into");
        assert!(
            format!("{launch_error:#}").contains("pin cannot be compared"),
            "got: {launch_error:#}"
        );

        // So the checkout is reclaimable, with nothing else to do with it.
        assert!(staged_worktree_is_discardable(&unpinned));
        let ack = discard_governed_staged_worktree(
            &state,
            discard_request(&unpinned, "unpinned-discard"),
            ConnectionClass::Operator,
        )
        .await
        .expect("an unpinned worktree is always reclaimable");
        assert_eq!(ack.discarded_root, staged_root);
        assert_eq!(
            ack.unreferenced_accepted_commit, None,
            "nothing was accepted, so nothing is orphaned"
        );
        assert!(!Path::new(&staged_root).exists());
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), oid);
    }

    /// The accepted/unpinned/zero-promotions case, and why its coverage sits at
    /// the function rather than at the endpoint.
    ///
    /// An earlier version of this comment claimed the state "cannot be
    /// synthesized". That was wrong — `state/governed_task.rs` synthesizes one
    /// by rewriting the record *and* its materialization receipt's fingerprint,
    /// and a ledger so built loads fine. The accurate reason is narrower and
    /// worth stating, because it changed under this lane:
    ///
    /// - An unpinned worktree can no longer be driven *forward* into an accepted
    ///   state by this build at all. #53 made `MarkRunning` refuse an unpinned
    ///   staged task, so the run can never start, never claim, and never be
    ///   accepted — proven directly by the test above.
    /// - So the only accepted-and-unpinned records that exist are ones a
    ///   *pre-pin build* accepted, then this build loaded. Reproducing that
    ///   means rewriting the receipt fingerprint for every mutation in the
    ///   history, through `fingerprint_mutation`, which is private to
    ///   `state/governed_task.rs` — a file this lane does not own and which two
    ///   other lanes merged this week.
    ///
    /// The state is real (those ledgers exist in the wild, which is exactly why
    /// `staged_worktree_is_discardable` short-circuits on an unpinned worktree),
    /// so the behavior is covered on the pure function in `impulse-ops` and the
    /// wiring that carries it to the operator is covered here. Making
    /// `fingerprint_mutation` `pub(crate)` would allow the full endpoint test
    /// and is on the handoff list rather than taken unilaterally.
    #[test]
    fn the_discard_ack_is_filled_from_the_shared_orphaned_commit_rule() {
        let mut unpinned = accepted_unpinned_task();
        assert!(
            staged_worktree_is_discardable(&unpinned),
            "an unpinned accepted worktree is reclaimable with no promotion attempt"
        );
        let expected = unreferenced_accepted_commit_on_discard(&unpinned)
            .map(str::to_string)
            .expect("an accepted run with no promotion still orphans its claim commit");

        // What both endpoint branches construct.
        let ack = GovernedStagedWorktreeDiscardAck {
            discarded_root: unpinned.staged_worktree.as_ref().unwrap().root.clone(),
            unreferenced_accepted_commit: unreferenced_accepted_commit_on_discard(&unpinned)
                .map(str::to_string),
            task: unpinned.clone(),
        };
        assert_eq!(
            ack.unreferenced_accepted_commit.as_deref(),
            Some(expected.as_str())
        );

        // A promoted run orphans nothing, and the same wiring must say so.
        unpinned
            .promotions
            .push(impulse_ops::governed_task::GovernedPromotion {
                id: impulse_ops::governed_task::GovernedRecordId::try_new("promo-u").unwrap(),
                actor: staged_system_actor(),
                accepted_revision: expected.clone(),
                initial_subject_revision: "a".repeat(40),
                outcome: GovernedPromotionOutcome::Promoted {
                    promoted_revision: expected.clone(),
                },
                recorded_at: "2026-09-12T00:00:00Z".to_string(),
                based_on_revision: 4,
            });
        assert_eq!(unreferenced_accepted_commit_on_discard(&unpinned), None);
    }

    /// An accepted, unpinned, never-promoted staged task, in memory.
    fn accepted_unpinned_task() -> GovernedTaskRun {
        let mut task = matrix_task(
            GovernedReviewState::Accepted,
            GovernedExecutionState::RuntimeExited,
            impulse_ops::governed_task::SharedRepositoryConfigPin::Unknown,
            None,
        );
        task.claims
            .push(impulse_ops::governed_task::WorkerCompletionClaim {
                id: impulse_ops::governed_task::GovernedRecordId::try_new("claim-u").unwrap(),
                actor: GovernedActor {
                    kind: GovernedActorKind::Worker,
                    id: "worker".to_string(),
                },
                summary: "done".to_string(),
                subject_revision: "b".repeat(40),
                artifact_ids: Vec::new(),
                diff_ref: None,
                loop_report_digest: None,
                loop_report_version: None,
                submitted_at: "2026-09-12T00:00:00Z".to_string(),
                based_on_revision: 2,
            });
        task
    }

    // ── Typed staged-configuration refusal ──────────────────────────────────

    /// Plant a `filter.*.smudge` driver in the shared repository configuration,
    /// which is what ADR-0019 rule 13 pins. `git config` in the staged worktree
    /// writes `.git/config`, shared with the canonical checkout.
    fn plant_shared_config_driver(repo: &Path, staged: &Path) -> PathBuf {
        let marker = repo.join("smudge-ran.txt");
        let script = repo.join("smudge.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf 'ran' > {}\ncat\n", marker.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        git(
            staged,
            &[
                "config",
                "filter.planted.smudge",
                &format!("sh {}", script.display()),
            ],
        );
        marker
    }

    /// A Builder that rewrites shared Git configuration is refused with a typed
    /// reason, not an error string, and nothing runs against that tree.
    #[tokio::test]
    async fn a_drifted_config_pin_refuses_the_claim_with_a_typed_reason() {
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
        let staged = Path::new(&staged_root);
        let running = mutate(
            &state,
            &registered,
            "refusal-running",
            GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        );

        std::fs::write(
            staged.join("src/lib.rs"),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
        )
        .unwrap();
        git(staged, &["add", "src/lib.rs"]);
        git(staged, &["commit", "--quiet", "-m", "builder work"]);
        let builder_oid = git(staged, &["rev-parse", "HEAD"]);
        let marker = plant_shared_config_driver(repo.path(), staged);

        let response = super::super::handlers::handle_governed_producer_request(
            DaemonRequest::SubmitGovernedClaim {
                request: impulse_ops::governed_task::GovernedClaimRequest {
                    request_id: request_id("refusal-claim"),
                    project_id: project_id.clone(),
                    task_id: running.id.clone(),
                    expected_revision: running.revision,
                    summary: "work complete".to_string(),
                    artifact_ids: Vec::new(),
                },
            },
            &state,
        )
        .await;

        let ack: GovernedStagedConfigRefusalAck = match response {
            DaemonResponse::Ok { result } => serde_json::from_value(result)
                .expect("a refusal is a successful-shape response carrying a typed reason"),
            other => panic!("expected a typed refusal, received {other:?}"),
        };
        assert!(ack.refused);
        assert_eq!(
            ack.reason,
            StagedConfigRefusalReason::Changed {
                component: impulse_ops::governed_task::SharedConfigComponent::RepositoryConfig,
            },
            "the refusal must name which shared file changed"
        );
        assert!(
            ack.remedy.contains("discard") && ack.remedy.contains("re-materialize"),
            "the refusal must carry its remedy: {}",
            ack.remedy
        );

        assert!(
            !marker.exists(),
            "the producer must refuse before Git materializes anything"
        );
        let after = state
            .get_governed_task(&project_id, &running.id)
            .unwrap()
            .unwrap();
        assert_eq!(after, running, "a refused claim records nothing");
        assert!(after.latest_claim().is_none());
        assert_eq!(
            git(staged, &["rev-parse", "HEAD"]),
            builder_oid,
            "the Builder's own commit is untouched; the remedy is discard-and-re-materialize"
        );
    }

    /// The verification refusal must release its reservation, or the retry the
    /// remedy ends in would meet `DuplicateOpenReservation` and be blocked.
    #[tokio::test]
    async fn a_refused_verification_releases_its_reservation() {
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
        let staged = Path::new(&staged_root);
        let running = mutate(
            &state,
            &registered,
            "refusal-verify-running",
            GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        );
        std::fs::write(
            staged.join("src/lib.rs"),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
        )
        .unwrap();
        git(staged, &["add", "src/lib.rs"]);
        git(staged, &["commit", "--quiet", "-m", "builder work"]);

        let claimed = task_from_producer_response(
            super::super::handlers::handle_governed_producer_request(
                DaemonRequest::SubmitGovernedClaim {
                    request: impulse_ops::governed_task::GovernedClaimRequest {
                        request_id: request_id("refusal-verify-claim"),
                        project_id: project_id.clone(),
                        task_id: running.id.clone(),
                        expected_revision: running.revision,
                        summary: "work complete".to_string(),
                        artifact_ids: Vec::new(),
                    },
                },
                &state,
            )
            .await,
        );
        assert_eq!(
            claimed.review_state,
            GovernedReviewState::AwaitingVerification
        );

        // The drift arrives between the claim and the verification.
        let marker = plant_shared_config_driver(repo.path(), staged);

        let response = handle_governed_verification(
            &state,
            impulse_ops::governed_task::GovernedVerificationRequest {
                request_id: request_id("refusal-verify"),
                project_id: project_id.clone(),
                task_id: claimed.id.clone(),
                expected_revision: claimed.revision,
            },
        )
        .await;
        let ack: GovernedStagedConfigRefusalAck = match response {
            DaemonResponse::Ok { result } => {
                serde_json::from_value(result).expect("a refusal is a successful-shape response")
            }
            other => panic!("expected a typed refusal, received {other:?}"),
        };
        assert!(ack.refused);
        assert!(matches!(
            ack.reason,
            StagedConfigRefusalReason::Changed { .. }
        ));
        assert!(!marker.exists(), "no Git ran against the staged tree");

        assert!(
            state.open_reservations().unwrap().is_empty(),
            "the refusal must release its reservation, or the retry the remedy \
             ends in would meet DuplicateOpenReservation"
        );
        let recorded = state
            .get_governed_task(&project_id, &claimed.id)
            .unwrap()
            .unwrap();
        assert_eq!(recorded, claimed, "a refused verification records nothing");
    }

    /// Promotion answers a staged-configuration refusal in the same typed shape
    /// the claim and verification endpoints use. Of promotion's two gates only
    /// the submodule check refuses — a drifted pin is a recorded blocked
    /// outcome, pinned by the next test — so the trigger is a `.gitmodules` the
    /// Builder introduced after acceptance.
    ///
    /// The planted smudge driver is the tripwire, not the trigger. Its absence
    /// proves nothing was materialized, and its presence proves the submodule
    /// gate runs *before* the pin comparison: a comparison reached first would
    /// have answered `promotion_blocked`, not this refusal.
    #[tokio::test]
    async fn a_submodule_introduced_after_acceptance_refuses_the_promotion_with_a_typed_reason() {
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
        let staged_root = accepted.active_staged_worktree().unwrap().root.clone();
        let staged = Path::new(&staged_root);

        let marker = plant_shared_config_driver(repo.path(), staged);
        let gitmodules = staged.join(".gitmodules");
        std::fs::write(
            &gitmodules,
            "[submodule \"sub\"]\n\tpath = sub\n\turl = ../sub\n",
        )
        .unwrap();

        let response = handle_governed_staged_request(
            DaemonRequest::PromoteGovernedOutcome {
                request: promotion_request(&accepted, "promote-submodule"),
            },
            &state,
            ConnectionClass::Operator,
        )
        .await;

        let ack: GovernedStagedConfigRefusalAck = match response {
            DaemonResponse::Ok { result } => serde_json::from_value(result)
                .expect("a refusal is a successful-shape response carrying a typed reason"),
            other => panic!("expected a typed refusal, received {other:?}"),
        };
        assert!(ack.refused);
        assert_eq!(
            ack.reason,
            StagedConfigRefusalReason::UnsupportedSubmodules {
                path: gitmodules.display().to_string(),
            },
            "the refusal must name the submodule configuration it found"
        );
        assert_eq!(
            ack.remedy,
            ack.reason.remedy(),
            "the remedy travels on the wire so no surface keeps its own mapping"
        );
        assert_eq!(
            ack.task, accepted,
            "the refusal echoes the record as it stood"
        );

        assert!(
            !marker.exists(),
            "the refusal precedes every Git invocation; nothing was materialized"
        );
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "the refusal must release its reservation, or the retry the remedy \
             ends in would meet DuplicateOpenReservation"
        );
        let after = state
            .get_governed_task(&project_id, &accepted.id)
            .unwrap()
            .unwrap();
        assert_eq!(after, accepted, "a refused promotion records nothing");
        assert!(after.latest_promotion().is_none());
        assert_eq!(after.review_state, GovernedReviewState::Accepted);
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]),
            oid,
            "the canonical branch never moved"
        );
        assert_eq!(
            git(staged, &["rev-parse", "HEAD"]),
            accepted_oid,
            "the Builder's accepted commit is untouched"
        );
    }

    /// The asymmetry the previous test relies on, pinned at the endpoint: a
    /// drifted pin at promotion is ADR-0019 rule 6's recorded
    /// `promotion_blocked { repository_config_changed }` on the accepted run,
    /// answered as a producer acknowledgement — not the typed refusal the claim
    /// and verification endpoints give the very same drift, because promotion
    /// has an accepted run to record the outcome against and they do not.
    #[tokio::test]
    async fn a_drifted_config_pin_at_promotion_is_a_recorded_blocked_outcome_not_a_refusal() {
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
        let (accepted, _accepted_oid) = accepted_staged_task(&state, registered);
        let staged_root = accepted.active_staged_worktree().unwrap().root.clone();
        let marker = plant_shared_config_driver(repo.path(), Path::new(&staged_root));

        let response = handle_governed_staged_request(
            DaemonRequest::PromoteGovernedOutcome {
                request: promotion_request(&accepted, "promote-drifted"),
            },
            &state,
            ConnectionClass::Operator,
        )
        .await;

        let value = match response {
            DaemonResponse::Ok { result } => result,
            other => panic!("a blocked promotion is a successful response, received {other:?}"),
        };
        assert!(
            value.get("refused").is_none(),
            "a blocked promotion is not a refusal: {value}"
        );
        let ack: GovernedProducerAck =
            serde_json::from_value(value).expect("a producer acknowledgement");
        assert!(!ack.replayed);
        let promotion = ack
            .task
            .latest_promotion()
            .expect("the blocked outcome is recorded on the accepted run");
        assert_eq!(
            promotion.outcome,
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid.clone(),
                reason: PromotionBlockedReason::RepositoryConfigChanged {
                    component: impulse_ops::governed_task::SharedConfigComponent::RepositoryConfig,
                },
            }
        );
        assert_eq!(ack.task.review_state, GovernedReviewState::Accepted);
        assert!(!marker.exists(), "no Git ran against either tree");
        assert!(state.open_reservations().unwrap().is_empty());
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), oid);
    }

    /// Only a staged-configuration refusal is re-shaped. Every other promotion
    /// failure still comes back as the error it always was — here a
    /// precondition the ledger refuses, with its context chain intact.
    #[tokio::test]
    async fn a_genuine_promotion_failure_still_answers_as_an_error() {
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

        let response = handle_governed_staged_request(
            DaemonRequest::PromoteGovernedOutcome {
                request: promotion_request(&registered, "promote-unaccepted"),
            },
            &state,
            ConnectionClass::Operator,
        )
        .await;
        match response {
            DaemonResponse::Error { message } => {
                assert!(
                    message.contains("governed promotion refused"),
                    "got: {message}"
                );
            }
            other => panic!("an unaccepted run cannot be promoted, received {other:?}"),
        }
        assert!(state.open_reservations().unwrap().is_empty());
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), oid);
    }

    fn task_from_producer_response(response: DaemonResponse) -> GovernedTaskRun {
        match response {
            DaemonResponse::Ok { result } => {
                serde_json::from_value(result).expect("response must contain a governed task")
            }
            other => panic!("expected a governed task response, received {other:?}"),
        }
    }

    // ── Promote preconditions, observed at the endpoint ─────────────────────

    /// An already-promoted run is refused by the endpoint, and the producer is
    /// never called.
    ///
    /// "Never called" is proven rather than asserted: the staged checkout is
    /// removed from disk first, so a producer that *did* run would fail loudly
    /// on a missing worktree instead of returning the precondition message.
    #[tokio::test]
    async fn an_already_promoted_run_is_refused_before_the_producer_runs() {
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
        let promoted = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "already-promote-1"),
            ConnectionClass::Operator,
        )
        .await
        .expect("the first promotion succeeds");
        assert!(promoted
            .task
            .latest_promotion()
            .unwrap()
            .outcome
            .is_promoted());
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), accepted_oid);

        // Sabotage: if the producer runs, it cannot possibly succeed quietly.
        let staged_root = promoted.task.staged_worktree.as_ref().unwrap().root.clone();
        std::fs::remove_dir_all(&staged_root).unwrap();

        let error = promote_governed_outcome(
            &state,
            promotion_request(&promoted.task, "already-promote-2"),
            ConnectionClass::Operator,
        )
        .await
        .expect_err("a run is promoted at most once");
        let rendered = format!("{error:#}");
        assert!(
            rendered.contains("this governed outcome was already promoted"),
            "expected the ledger's own precondition message, got: {rendered}"
        );
        assert!(
            !rendered.contains("worktree") || !rendered.contains("No such file"),
            "the producer must not have run: {rendered}"
        );
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "a precondition refusal takes no reservation"
        );
        let after = state
            .get_governed_task(&project_id, &promoted.task.id)
            .unwrap()
            .unwrap();
        assert_eq!(after, promoted.task, "a refused promotion records nothing");
        assert_eq!(after.promotions.len(), 1);
    }

    /// A discarded staged worktree is refused the same way, and the producer is
    /// again never reached: the checkout it would observe is gone.
    #[tokio::test]
    async fn a_promotion_without_an_active_worktree_is_refused_before_the_producer_runs() {
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
        std::fs::write(repo.path().join("NOTES.md"), "canonical work\n").unwrap();
        git(repo.path(), &["add", "NOTES.md"]);
        git(repo.path(), &["commit", "--quiet", "-m", "canonical move"]);
        let blocked = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "no-worktree-promote-1"),
            ConnectionClass::Operator,
        )
        .await
        .unwrap()
        .task;
        let reclaimed = discard_governed_staged_worktree(
            &state,
            discard_request(&blocked, "no-worktree-discard"),
            ConnectionClass::Operator,
        )
        .await
        .unwrap()
        .task;

        let error = promote_governed_outcome(
            &state,
            promotion_request(&reclaimed, "no-worktree-promote-2"),
            ConnectionClass::Operator,
        )
        .await
        .expect_err("there is no staged worktree left to promote from");
        assert!(
            format!("{error:#}").contains("promotion requires an active staged worktree"),
            "got: {error:#}"
        );
        assert!(state.open_reservations().unwrap().is_empty());
    }

    /// An accepted staged task with no worker claim, checked at the endpoint's
    /// own admission function.
    ///
    /// Deliberately not driven through the ledger: `Accepted` is only reachable
    /// via an operator decision on a Supervisor verdict on a verification on a
    /// claim, so a claimless accepted task cannot be constructed by any
    /// sequence of real mutations. The check is defense in depth against a
    /// future transition that relaxes that chain, which is exactly the kind of
    /// thing an inline `is_accepted() && has_worktree()` preflight would have
    /// missed — it did, until the matrix test found it.
    #[test]
    fn an_accepted_task_without_a_claim_is_refused_by_the_endpoint_admission() {
        let mut claimless = matrix_task(
            GovernedReviewState::Accepted,
            GovernedExecutionState::RuntimeExited,
            impulse_ops::governed_task::SharedRepositoryConfigPin::Recorded(
                impulse_ops::governed_task::SharedRepositoryConfigDigest::current(
                    format!("sha256:{}", "c".repeat(64)),
                    None,
                    None,
                ),
            ),
            None,
        );
        assert!(claimless.latest_claim().is_none());
        let error = promote_preflight(&claimless)
            .expect_err("a promotion has nothing to reference without a claim");
        assert!(
            format!("{error:#}").contains("promotion requires an accepted worker claim"),
            "got: {error:#}"
        );

        // With a claim the same task is admitted, so the refusal above is about
        // the claim and not about some other precondition.
        claimless
            .claims
            .push(impulse_ops::governed_task::WorkerCompletionClaim {
                id: impulse_ops::governed_task::GovernedRecordId::try_new("claim-a").unwrap(),
                actor: GovernedActor {
                    kind: GovernedActorKind::Worker,
                    id: "worker".to_string(),
                },
                summary: "done".to_string(),
                subject_revision: "b".repeat(40),
                artifact_ids: Vec::new(),
                diff_ref: None,
                loop_report_digest: None,
                loop_report_version: None,
                submitted_at: "2026-09-12T00:00:00Z".to_string(),
                based_on_revision: 2,
            });
        assert!(promote_preflight(&claimless).is_ok());
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

    // ── One staged run, end to end ──────────────────────────────────────────

    /// Echoes the daemon's own bounded review payload back as a strict
    /// envelope. The Supervisor turn is the single step in the chain that needs
    /// a provider; every other step below is the real producer against a real
    /// repository.
    struct BoundSupervisorProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[crate::llm_backends::async_trait]
    impl crate::llm_backends::LlmProvider for BoundSupervisorProvider {
        fn name(&self) -> &str {
            "bound-supervisor-e2e"
        }

        fn default_model(&self) -> &str {
            "bound-supervisor-e2e-model"
        }

        fn supported_models(&self) -> Vec<&str> {
            vec!["bound-supervisor-e2e-model"]
        }

        async fn chat(
            &self,
            request: crate::llm_backends::ChatRequest,
        ) -> crate::llm_backends::AgentResult<crate::llm_backends::ChatResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let user = request
                .messages
                .iter()
                .find(|message| message.role == crate::llm_backends::Role::User)
                .expect("Supervisor request must have a user payload");
            let payload: serde_json::Value =
                serde_json::from_str(&user.content).expect("Supervisor payload must be JSON");
            let response = serde_json::json!({
                "contract_version": payload["contract_version"],
                "task_id": payload["task_id"],
                "task_revision": payload["task_revision"],
                "claim_id": payload["claim_id"],
                "verification_id": payload["verification_id"],
                "subject_revision": payload["subject_revision"],
                "acceptance_criteria_count": payload["acceptance_criteria_count"],
                "acceptance_criteria_digest": payload["acceptance_criteria_digest"],
                "verdict": "recommend_accept",
                "rationale": "daemon-observed evidence supports every exact criterion"
            });
            Ok(crate::llm_backends::ChatResponse {
                content: response.to_string(),
                model: self.default_model().to_string(),
                usage: crate::llm_backends::Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: crate::llm_backends::StopReason::EndTurn,
                tool_calls: Vec::new(),
            })
        }
    }

    /// One staged governed run through every endpoint this lane added:
    /// register (materializes) -> launch -> Builder commit -> claim -> verify ->
    /// Supervisor review -> operator approval -> promote -> discard.
    ///
    /// Every step but the Supervisor turn is the real producer against a real
    /// Git repository and a real Cargo workspace. This is the test the lane
    /// could not write before #53, whose staged-verification fix made the chain
    /// completable; it is what proves the scope is reachable rather than merely
    /// wired.
    #[tokio::test]
    async fn one_staged_run_completes_through_every_endpoint() {
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
        .expect("an operator registers the staged run");
        let staged_root = registered
            .active_staged_worktree()
            .expect("registration materializes the checkout")
            .root
            .clone();
        assert_eq!(
            registered
                .launch_working_directory()
                .expect("a materialized staged task has a launch working directory"),
            staged_root
        );

        let running = mutate(
            &state,
            &registered,
            "e2e-running",
            GovernedTaskMutation::MarkRunning {
                actor: staged_system_actor(),
            },
        );
        assert_eq!(running.execution_state, GovernedExecutionState::Running);

        // The Builder works inside its own checkout. The canonical tree must
        // not move until promotion.
        let staged = Path::new(&staged_root);
        std::fs::write(
            staged.join("src/lib.rs"),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
        )
        .unwrap();
        git(staged, &["add", "src/lib.rs"]);
        git(staged, &["commit", "--quiet", "-m", "builder work"]);
        let builder_oid = git(staged, &["rev-parse", "HEAD"]);
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]),
            oid,
            "the canonical branch stays on the registered initial OID"
        );

        let claimed = task_from_producer_response(
            super::super::handlers::handle_governed_producer_request(
                DaemonRequest::SubmitGovernedClaim {
                    request: impulse_ops::governed_task::GovernedClaimRequest {
                        request_id: request_id("e2e-claim"),
                        project_id: project_id.clone(),
                        task_id: running.id.clone(),
                        expected_revision: running.revision,
                        summary: "staged work complete".to_string(),
                        artifact_ids: Vec::new(),
                    },
                },
                &state,
            )
            .await,
        );
        assert_eq!(
            claimed.latest_claim().unwrap().subject_revision,
            builder_oid,
            "the claim binds the Builder's own commit, not the canonical head"
        );

        let verified = run_governed_verification(
            &state,
            impulse_ops::governed_task::GovernedVerificationRequest {
                request_id: request_id("e2e-verify"),
                project_id: project_id.clone(),
                task_id: claimed.id.clone(),
                expected_revision: claimed.revision,
            },
        )
        .await
        .expect("a staged claim verifies");
        assert!(!verified.replayed);
        assert_eq!(verified.pending_rerun_reason, None);
        assert_eq!(
            verified.task.latest_verification().unwrap().outcome,
            impulse_ops::governed_task::GovernedVerificationOutcome::Passed
        );
        assert!(state.open_reservations().unwrap().is_empty());

        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let agent =
            crate::agent::ImpulseAgent::with_test_provider(Box::new(BoundSupervisorProvider {
                calls: Arc::clone(&calls),
            }));
        let cached_agent = Arc::new(tokio::sync::Mutex::new(Some(agent)));
        let judged: GovernedProducerAck = match handle_governed_supervisor_review(
            &state,
            impulse_ops::governed_task::GovernedSupervisorReviewRequest {
                request_id: request_id("e2e-review"),
                project_id: project_id.clone(),
                task_id: verified.task.id.clone(),
                expected_revision: verified.task.revision,
            },
            &cached_agent,
        )
        .await
        {
            DaemonResponse::Ok { result } => serde_json::from_value(result).unwrap(),
            other => panic!("expected a Supervisor verdict, received {other:?}"),
        };
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            judged.task.review_state,
            GovernedReviewState::AwaitingOperator
        );

        let accepted = task_from_producer_response(
            super::super::handlers::handle_governed_task_request(
                DaemonRequest::MutateGovernedTask {
                    request: GovernedTaskMutationRequest {
                        request_id: request_id("e2e-approve"),
                        project_id: project_id.clone(),
                        task_id: judged.task.id.clone(),
                        expected_revision: judged.task.revision,
                        mutation: GovernedTaskMutation::RecordOperatorDecision {
                            decision: OperatorDecisionInput {
                                actor: GovernedActor {
                                    kind: GovernedActorKind::Operator,
                                    id: "e2e-operator".to_string(),
                                },
                                supervisor_verdict_id: judged
                                    .task
                                    .latest_supervisor_verdict()
                                    .unwrap()
                                    .id
                                    .clone(),
                                decision: OperatorDecisionKind::Approve,
                                rationale: "evidence supports the criteria".to_string(),
                            },
                        },
                    },
                },
                &state,
                ConnectionClass::Operator,
            )
            .await,
        );
        assert_eq!(accepted.review_state, GovernedReviewState::Accepted);
        assert_eq!(
            git(repo.path(), &["rev-parse", "HEAD"]),
            oid,
            "acceptance alone does not make the work canonical"
        );

        let promoted = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "e2e-promote"),
            ConnectionClass::Operator,
        )
        .await
        .expect("an operator promotes the accepted staged outcome");
        assert_eq!(
            promoted.task.latest_promotion().unwrap().outcome,
            GovernedPromotionOutcome::Promoted {
                promoted_revision: builder_oid.clone(),
            }
        );
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), builder_oid);
        assert_eq!(
            std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap(),
            "pub fn wiring_fixture() -> u8 {\n    1\n}\n",
            "promotion syncs the canonical working tree, not just the ref"
        );

        let discarded = discard_governed_staged_worktree(
            &state,
            discard_request(&promoted.task, "e2e-discard"),
            ConnectionClass::Operator,
        )
        .await
        .expect("a promoted run's checkout is spent");
        assert_eq!(discarded.discarded_root, staged_root);
        assert_eq!(
            discarded.unreferenced_accepted_commit, None,
            "a promoted commit is on the canonical branch, so nothing is orphaned"
        );
        assert!(!Path::new(&staged_root).exists());
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "every producer released its reservation"
        );
    }

    // ── Review round 1 regressions ──────────────────────────────────────────

    /// P1-1. Recording an operator approval writes
    /// `.impulse/MEMORY_CANDIDATES.json` (ADR-0013), and promotion — which is
    /// only reachable *after* an approval — observes the canonical tree. In a
    /// project whose `.impulse` is not gitignored, a missing cleanliness
    /// exemption therefore makes the daemon fail on a tree it dirtied itself.
    ///
    /// Every other fixture here writes the `impulse init` ignore list, which
    /// hides the bug completely. This one does not, and it carries a negative
    /// control so it cannot pass vacuously.
    #[tokio::test]
    async fn an_approval_does_not_dirty_the_canonical_tree_for_promotion() {
        let (repo, state, project_id, oid) = repo_state_without_runtime_ignores();
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

        // Negative control: Git must actually see the candidate ledger here,
        // or the assertion below proves nothing.
        let candidates = repo.path().join(".impulse").join("MEMORY_CANDIDATES.json");
        assert!(
            candidates.exists(),
            "the approval must have written the accepted-run candidate ledger"
        );
        let untracked = git(
            repo.path(),
            &["status", "--porcelain", "--untracked-files=all"],
        );
        assert!(
            untracked.contains(".impulse/MEMORY_CANDIDATES.json"),
            "this fixture must leave the candidate ledger untracked, or the \
             exemption under test is never exercised; saw: {untracked}"
        );

        let ack = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "promote-after-approval"),
            ConnectionClass::Operator,
        )
        .await
        .expect("promotion must not fail on a tree the daemon dirtied itself");
        assert_eq!(
            ack.task.latest_promotion().unwrap().outcome,
            GovernedPromotionOutcome::Promoted {
                promoted_revision: accepted_oid.clone(),
            }
        );
        assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), accepted_oid);
    }

    /// P1-2. `materialize_staged_worktree` runs before the ledger write, so a
    /// retry of a registration that already succeeded would reach the producer
    /// and fail on a staged path occupied by its own earlier attempt. The
    /// request id is checked against the ledger's receipts first.
    #[test]
    fn a_replayed_staged_registration_returns_the_recorded_task_and_touches_nothing() {
        let (repo, state, project_id, oid) = repo_state();
        let first = register_governed_task(
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
        let staged_root = first.active_staged_worktree().unwrap().root.clone();
        let marker = Path::new(&staged_root).join("builder-scratch.txt");
        std::fs::write(&marker, "a live Builder is working here\n").unwrap();

        // The same request id again, as a timed-out client would resend it.
        let replayed = register_governed_task(
            &state,
            registration(
                &project_id,
                repo.path(),
                &oid,
                WorldScope::StagedAuthoritative,
            ),
            ConnectionClass::Operator,
        )
        .expect("a replayed registration must not fail on its own staged path");

        assert_eq!(replayed, first, "the replay returns the recorded task");
        assert!(
            marker.exists(),
            "the replay must not disturb a live Builder's checkout"
        );
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "a live Builder is working here\n"
        );
        assert_eq!(
            git(Path::new(&staged_root), &["rev-parse", "HEAD"]),
            oid,
            "the staged worktree is untouched"
        );
    }

    /// P1-2, second half. A genuinely occupied staged path (a directory that
    /// outlived an interrupted run, not this task's own replay) still fails
    /// closed, and the recovery instructions survive to the caller.
    #[test]
    fn an_occupied_staged_path_still_fails_closed_with_its_recovery_text() {
        let (repo, state, project_id, oid) = repo_state();
        let registration = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );
        let squatter = repo
            .path()
            .join(".impulse")
            .join("worktrees")
            .join(registration.task_id.as_str());
        std::fs::create_dir_all(&squatter).unwrap();

        let error = register_governed_task(&state, registration, ConnectionClass::Operator)
            .expect_err("a leftover directory is not adoptable");
        let rendered = format!("{error:#}");
        assert!(rendered.contains("already exists"), "{rendered}");
        assert!(
            rendered.contains("worktree prune"),
            "the operator needs the recovery command, not just the symptom: {rendered}"
        );
    }

    /// P2-1. The profiled preflight in `handle_governed_task_request` spawns
    /// `git` inside a caller-supplied path. A non-operator connection must be
    /// refused before that happens, so this endpoint cannot be used to probe
    /// the filesystem or start a subprocess pre-capability.
    #[tokio::test]
    async fn a_non_operator_staged_registration_is_refused_before_git_runs() {
        let (repo, state, project_id, oid) = repo_state();
        let elsewhere = tempfile::Builder::new()
            .prefix("impulse-not-a-repo-")
            .tempdir()
            .unwrap();
        assert!(
            !elsewhere.path().join(".git").exists(),
            "the probe target must not be a Git repository"
        );

        let mut probing = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );
        probing.workspace_root = elsewhere.path().display().to_string();

        let response = super::super::handlers::handle_governed_task_request(
            DaemonRequest::RegisterGovernedTask {
                registration: probing,
            },
            &state,
            ConnectionClass::NonOperator,
        )
        .await;
        let message = match response {
            DaemonResponse::Error { message } => message,
            other => panic!("expected a refusal, received {other:?}"),
        };
        assert!(
            message.contains("operator-class connection"),
            "expected the class refusal, got: {message}"
        );
        assert!(
            !message.contains("git root discovery") && !message.contains("canonicalize"),
            "the refusal must land before any filesystem or Git observation: {message}"
        );
    }

    /// P2-3. `DaemonClient` retries acknowledged requests, so the first
    /// response carrying the orphaned-commit warning can be lost. The replay
    /// must recompute it rather than answering `None`.
    #[tokio::test]
    async fn a_replayed_discard_still_names_the_unreferenced_commit() {
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

        std::fs::write(repo.path().join("NOTES.md"), "canonical work\n").unwrap();
        git(repo.path(), &["add", "NOTES.md"]);
        git(repo.path(), &["commit", "--quiet", "-m", "canonical move"]);
        let blocked = promote_governed_outcome(
            &state,
            promotion_request(&accepted, "replay-discard-promote"),
            ConnectionClass::Operator,
        )
        .await
        .unwrap()
        .task;

        let request = discard_request(&blocked, "replay-discard");
        let first =
            discard_governed_staged_worktree(&state, request.clone(), ConnectionClass::Operator)
                .await
                .unwrap();
        assert_eq!(
            first.unreferenced_accepted_commit.as_deref(),
            Some(accepted_oid.as_str())
        );

        let replayed = discard_governed_staged_worktree(&state, request, ConnectionClass::Operator)
            .await
            .expect("the retry replays the recorded discard");
        assert_eq!(
            replayed.unreferenced_accepted_commit.as_deref(),
            Some(accepted_oid.as_str()),
            "a lost first response must not cost the operator the warning"
        );
        assert_eq!(replayed.discarded_root, first.discarded_root);
    }

    /// One staged task in an arbitrary review/execution/pin/promotion state.
    ///
    /// Shared by the discardability matrix and the orphaned-commit wiring test
    /// so the two cannot disagree about what a state even looks like.
    fn matrix_task(
        review: GovernedReviewState,
        execution: GovernedExecutionState,
        pin: impulse_ops::governed_task::SharedRepositoryConfigPin,
        promotion: Option<GovernedPromotionOutcome>,
    ) -> GovernedTaskRun {
        let mut task = GovernedTaskRun {
            id: impulse_ops::governed_task::GovernedTaskId::try_new("task-matrix").unwrap(),
            revision: 5,
            project_id: "demo".to_string(),
            workspace_root: "/tmp/demo".to_string(),
            task: "matrix".to_string(),
            acceptance_criteria: vec!["c".to_string()],
            approval_policy: impulse_ops::governed_task::ApprovalPolicy::OperatorRequired,
            world_scope: WorldScope::StagedAuthoritative,
            verification_profile: Some(GovernedVerificationProfile::RustWorkspaceV1),
            role_assignment: None,
            role_compatibility: None,
            runtime_id: "ion".to_string(),
            agent_id: "worker".to_string(),
            session_id: None,
            initial_subject_revision: Some("a".repeat(40)),
            staged_worktree: Some(impulse_ops::governed_task::StagedWorktree {
                id: impulse_ops::governed_task::GovernedRecordId::try_new("staged-m").unwrap(),
                actor: staged_system_actor(),
                root: "/tmp/demo/.impulse/worktrees/task-matrix".to_string(),
                initial_subject_revision: "a".repeat(40),
                shared_config_digest: pin,
                status: StagedWorktreeStatus::Active,
                materialized_at: "2026-09-12T00:00:00Z".to_string(),
                based_on_revision: 1,
            }),
            promotions: Vec::new(),
            execution_state: execution,
            review_state: review,
            claims: Vec::new(),
            verifications: Vec::new(),
            supervisor_verdicts: Vec::new(),
            operator_decisions: Vec::new(),
            events: Vec::new(),
            created_at: "2026-09-12T00:00:00Z".to_string(),
            updated_at: "2026-09-12T00:00:00Z".to_string(),
        };
        if let Some(outcome) = promotion {
            task.promotions
                .push(impulse_ops::governed_task::GovernedPromotion {
                    id: impulse_ops::governed_task::GovernedRecordId::try_new("promo-m").unwrap(),
                    actor: staged_system_actor(),
                    accepted_revision: "b".repeat(40),
                    initial_subject_revision: "a".repeat(40),
                    outcome,
                    recorded_at: "2026-09-12T00:00:00Z".to_string(),
                    based_on_revision: 4,
                });
        }
        task
    }

    /// P2-4. The daemon preflights discardability against its own copy of the
    /// rule because the destructive side effect runs before the mutation the
    /// state layer enforces. Two copies can drift; this makes drift fail
    /// loudly rather than silently letting the daemon delete a checkout the
    /// ledger would then refuse to record.
    #[test]
    fn both_discardability_rules_agree_over_the_whole_state_matrix() {
        use crate::state::staged_worktree_is_discardable as state_layer_rule;

        let base = matrix_task;
        let pins = [
            impulse_ops::governed_task::SharedRepositoryConfigPin::Unknown,
            impulse_ops::governed_task::SharedRepositoryConfigPin::Recorded(
                impulse_ops::governed_task::SharedRepositoryConfigDigest::current(
                    format!("sha256:{}", "c".repeat(64)),
                    None,
                    None,
                ),
            ),
        ];
        let promotions = [
            None,
            Some(GovernedPromotionOutcome::Promoted {
                promoted_revision: "b".repeat(40),
            }),
            Some(GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: "c".repeat(40),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            }),
        ];
        let reviews = [
            GovernedReviewState::AwaitingClaim,
            GovernedReviewState::AwaitingVerification,
            GovernedReviewState::AwaitingSupervisor,
            GovernedReviewState::AwaitingOperator,
            GovernedReviewState::ChangesRequested,
            GovernedReviewState::VerificationFailed,
            GovernedReviewState::Accepted,
            GovernedReviewState::Rejected,
            GovernedReviewState::Escalated,
        ];
        let executions = [
            GovernedExecutionState::Registered,
            GovernedExecutionState::Running,
            GovernedExecutionState::LaunchFailed,
            GovernedExecutionState::RuntimeExited,
        ];

        let mut compared = 0usize;
        for review in reviews {
            for execution in executions {
                for pin in &pins {
                    for promotion in &promotions {
                        let task = base(review, execution, pin.clone(), promotion.clone());
                        assert_eq!(
                            staged_worktree_is_discardable(&task),
                            state_layer_rule(&task),
                            "the daemon preflight and the state-layer rule disagree for \
                             review={review:?} execution={execution:?} pin={pin:?} \
                             promotion={promotion:?}"
                        );
                        compared += 1;
                    }
                }
            }
        }
        assert_eq!(
            compared,
            reviews.len() * executions.len() * pins.len() * promotions.len()
        );
        assert!(
            compared >= 200,
            "the matrix must be exhaustive, not a sample"
        );
    }

    /// The promote counterpart to the discardability cross-check.
    ///
    /// `impulse_ops::governed_wiring::governed_outcome_is_promotable` exists so
    /// a surface can decide whether to offer the control at all. From
    /// `impulse-ops` it can import neither the daemon endpoint's inline checks
    /// nor the ledger's `RecordPromotion` preconditions, so on its own it can
    /// only *restate* them — which is the drift this test exists to prevent.
    /// Here both are importable, so the sandwich is checkable:
    ///
    /// - **superset of the endpoint**: every task the promote endpoint would run
    ///   for, the shared predicate must also admit (otherwise a surface hides a
    ///   control that works);
    /// - **subset of the ledger**: every task the predicate admits, the ledger's
    ///   preconditions must accept (otherwise a surface offers a control that is
    ///   guaranteed to be refused).
    #[test]
    fn promotability_sits_between_the_endpoint_and_the_ledger_over_the_whole_matrix() {
        use crate::state::record_promotion_preconditions_hold as ledger_rule;
        use impulse_ops::governed_wiring::governed_outcome_is_promotable as shared_rule;

        // The endpoint's own admission check, called rather than restated, so
        // changing it changes this test.
        let endpoint_admits = |task: &GovernedTaskRun| promote_preflight(task).is_ok();

        let pins = [
            impulse_ops::governed_task::SharedRepositoryConfigPin::Unknown,
            impulse_ops::governed_task::SharedRepositoryConfigPin::Recorded(
                impulse_ops::governed_task::SharedRepositoryConfigDigest::current(
                    format!("sha256:{}", "c".repeat(64)),
                    None,
                    None,
                ),
            ),
        ];
        let promotions = [
            None,
            Some(GovernedPromotionOutcome::Promoted {
                promoted_revision: "b".repeat(40),
            }),
            Some(GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: "c".repeat(40),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            }),
        ];
        let reviews = [
            GovernedReviewState::AwaitingClaim,
            GovernedReviewState::AwaitingVerification,
            GovernedReviewState::AwaitingSupervisor,
            GovernedReviewState::AwaitingOperator,
            GovernedReviewState::ChangesRequested,
            GovernedReviewState::VerificationFailed,
            GovernedReviewState::Accepted,
            GovernedReviewState::Rejected,
            GovernedReviewState::Escalated,
        ];
        let executions = [
            GovernedExecutionState::Registered,
            GovernedExecutionState::Running,
            GovernedExecutionState::LaunchFailed,
            GovernedExecutionState::RuntimeExited,
        ];

        let mut compared = 0usize;
        let mut admitted = 0usize;
        for review in reviews {
            for execution in executions {
                for pin in &pins {
                    for promotion in &promotions {
                        // With a claim and without: `latest_claim` is one of the
                        // conditions, so both halves must be covered.
                        for with_claim in [false, true] {
                            let mut task =
                                matrix_task(review, execution, pin.clone(), promotion.clone());
                            if with_claim {
                                task.claims.push(
                                    impulse_ops::governed_task::WorkerCompletionClaim {
                                        id: impulse_ops::governed_task::GovernedRecordId::try_new(
                                            "claim-p",
                                        )
                                        .unwrap(),
                                        actor: GovernedActor {
                                            kind: GovernedActorKind::Worker,
                                            id: "worker".to_string(),
                                        },
                                        summary: "done".to_string(),
                                        subject_revision: "b".repeat(40),
                                        artifact_ids: Vec::new(),
                                        diff_ref: None,
                                        loop_report_digest: None,
                                        loop_report_version: None,
                                        submitted_at: "2026-09-12T00:00:00Z".to_string(),
                                        based_on_revision: 2,
                                    },
                                );
                            }
                            let shared = shared_rule(&task);
                            if endpoint_admits(&task) {
                                assert!(
                                    shared,
                                    "the shared predicate must not hide a control the endpoint \
                                     would run: review={review:?} execution={execution:?} \
                                     pin={pin:?} promotion={promotion:?} with_claim={with_claim}"
                                );
                            }
                            if shared {
                                admitted += 1;
                                assert!(
                                    ledger_rule(&task).is_ok(),
                                    "the shared predicate must not promise a promotion the \
                                     ledger refuses: review={review:?} execution={execution:?} \
                                     pin={pin:?} promotion={promotion:?} with_claim={with_claim} \
                                     ledger={:?}",
                                    ledger_rule(&task)
                                );
                            }
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            compared,
            reviews.len() * executions.len() * pins.len() * promotions.len() * 2
        );
        assert!(
            admitted > 0,
            "a sandwich test that admits nothing proves nothing"
        );
    }

    /// Nit from review round 1: `roll_back_staged_checkout` has five call sites
    /// and had no direct coverage. Leaving the administrative entry behind
    /// would make the operator's retry fail on a path Git still considers
    /// registered, which is the whole reason it goes through the real discard
    /// producer rather than `remove_dir_all`.
    #[test]
    fn rolling_back_a_checkout_removes_its_administrative_entry_too() {
        let (repo, _state, project_id, oid) = repo_state();
        let registration = registration(
            &project_id,
            repo.path(),
            &oid,
            WorldScope::StagedAuthoritative,
        );
        let provisional = provisional_staged_task(&registration).unwrap();
        let staged = crate::governed_producers::materialize_staged_worktree(&provisional).unwrap();
        let root = PathBuf::from(&staged.root);
        assert!(root.exists());
        assert!(
            git(repo.path(), &["worktree", "list"]).contains(&staged.root),
            "the checkout must be registered before the rollback is meaningful"
        );

        roll_back_staged_checkout(&provisional, &staged);

        assert!(!root.exists(), "the checkout directory is removed");
        assert!(
            !git(repo.path(), &["worktree", "list"]).contains(&staged.root),
            "the administrative entry must go with it, or a retry fails on a \
             path Git still considers registered"
        );
        // Proof that the retry actually works: materializing again succeeds.
        let again = crate::governed_producers::materialize_staged_worktree(&provisional)
            .expect("a rolled-back path is reusable");
        assert_eq!(again.root, staged.root);
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
