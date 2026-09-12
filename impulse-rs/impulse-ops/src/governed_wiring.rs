//! Wire contracts for the daemon-owned governed producers added in protocol
//! v9: staged-worktree promotion, staged-worktree discard, and the
//! acknowledgement shape every producer request now answers with.
//!
//! These live beside [`crate::governed_task`] rather than inside it on
//! purpose. The record types, the mutation enum, and the state machine in that
//! module are the durable ledger contract; everything here is request/response
//! shape for one protocol version, and a concurrent lane owns `governed_task.rs`
//! for correctness fixes. Keeping the two apart means a protocol addition and a
//! ledger fix do not have to be the same edit.
//!
//! ADR-0012 (daemon-owned producers), ADR-0018 (operator-class provenance) and
//! ADR-0019 (staged world scope) are the governing decisions.

use serde::{Deserialize, Serialize};

use crate::governed_task::{
    GovernedExecutionState, GovernedRequestId, GovernedReviewState, GovernedTaskContractError,
    GovernedTaskId, GovernedTaskRun, WorldScope, MAX_GOVERNED_TEXT_BYTES,
};

/// Trigger for the daemon-owned promotion producer (ADR-0019).
///
/// Like every other producer request it carries coordinates only. The outcome
/// is computed by the daemon against real Git state and can never be
/// caller-authored, and a blocked promotion comes back as a *successful*
/// response carrying `GovernedPromotionOutcome::PromotionBlocked` rather than
/// an error: the run stays `accepted`, the staged worktree stays active, and an
/// operator decides what to do with a canonical branch that moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedPromotionRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
}

/// Trigger for the daemon-owned staged-worktree discard producer (ADR-0019).
///
/// `reason` is the only caller-authored field; the actor, the checkout path,
/// and the discardability decision are all daemon-derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedStagedWorktreeDiscardRequest {
    pub request_id: GovernedRequestId,
    pub project_id: String,
    pub task_id: GovernedTaskId,
    pub expected_revision: u64,
    pub reason: String,
}

impl GovernedStagedWorktreeDiscardRequest {
    /// `governed_task::validate_nonblank` is private to that module and this
    /// lane does not own the file, so the same rule is applied here rather than
    /// widening its visibility.
    pub fn validate(&self) -> Result<(), GovernedTaskContractError> {
        if self.reason.trim().is_empty()
            || self.reason.contains('\0')
            || self.reason.len() > MAX_GOVERNED_TEXT_BYTES
        {
            return Err(GovernedTaskContractError::InvalidField {
                field: "reason",
                message: format!(
                    "must be nonblank, NUL-free, and at most {MAX_GOVERNED_TEXT_BYTES} UTF-8 bytes"
                ),
            });
        }
        Ok(())
    }
}

/// Acknowledgement for a daemon-owned producer request.
///
/// The governed task is flattened into the response object, so this is a strict
/// superset of the bare `GovernedTaskRun` earlier protocol versions returned
/// and an older client deserializing a task still reads one.
///
/// `pending_rerun_reason` is what makes ADR-0012's durable reservation journal
/// observable on the wire: when a previous process was interrupted between a
/// producer side effect and its receipt, the reservation it left open is
/// reconciled to `needs_rerun` at reload, and this field carries that reason
/// back to the caller whose request id took it. The rerun itself still
/// proceeds — the field explains *why* work is being redone, it does not block
/// it. A request arriving while a same-revision reservation is genuinely still
/// open is refused instead, with the journal's own typed duplicate error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedProducerAck {
    #[serde(flatten)]
    pub task: GovernedTaskRun,
    /// True when the daemon recognized the request id and replayed the already
    /// recorded receipt instead of running the side effect a second time.
    #[serde(default)]
    pub replayed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_rerun_reason: Option<String>,
}

impl GovernedProducerAck {
    pub fn new(
        task: GovernedTaskRun,
        replayed: bool,
        pending_rerun_reason: Option<String>,
    ) -> Self {
        Self {
            task,
            replayed,
            pending_rerun_reason,
        }
    }
}

/// Acknowledgement for a staged-worktree discard.
///
/// Flattened the same way as [`GovernedProducerAck`], plus what the discard
/// cost. ADR-0019's Consequences require the surface offering a discard to say
/// when it drops the only ref to an accepted commit, and to show the OID so an
/// operator can recover it deliberately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedStagedWorktreeDiscardAck {
    #[serde(flatten)]
    pub task: GovernedTaskRun,
    /// Absolute path of the checkout that was removed.
    pub discarded_root: String,
    /// Set when this discard dropped the only ref to an accepted-but-blocked
    /// commit: the canonical branch never advanced, so once the checkout and
    /// its administrative entry are gone the commit is reachable only through
    /// the reflog until that expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreferenced_accepted_commit: Option<String>,
}

/// Every state a staged worktree can legitimately be reclaimed from (ADR-0019
/// rule 7), so a live Builder's work cannot be deleted out from under it.
///
/// The enforcing authority is the state layer's own copy of this rule in
/// `impulse-rs/src/state/governed_task.rs`, which refuses the mutation. This
/// one exists so the daemon can refuse *before* running the destructive side
/// effect rather than after: removing the checkout and only then having the
/// mutation refused would lose the work with nothing recorded. The two must
/// agree; unifying them means making the state layer call this function, a
/// one-line change in a file this lane does not own.
pub fn staged_worktree_is_discardable(task: &GovernedTaskRun) -> bool {
    // A runtime that failed to launch leaves a worktree nothing will ever use,
    // whatever the review state says.
    if task.execution_state == GovernedExecutionState::LaunchFailed {
        return true;
    }
    // A worktree with no shared-configuration pin can never be promoted, so
    // discarding it is the only way forward and must always be available.
    if task
        .staged_worktree
        .as_ref()
        .is_some_and(|staged| staged.shared_config_digest.is_unknown())
    {
        return true;
    }
    match task.review_state {
        // Terminal: the operator declined, or the loop contract tripped and the
        // task accepts no further claims.
        GovernedReviewState::Rejected | GovernedReviewState::Escalated => true,
        // Promoted (work is canonical, the checkout is spent) or blocked (the
        // operator may reclaim the space instead of retrying).
        GovernedReviewState::Accepted => task.latest_promotion().is_some(),
        _ => false,
    }
}

/// Every condition `PromoteGovernedOutcome` checks before it runs Git
/// (ADR-0019 rule 5), so an operator surface can disable the control rather
/// than offering a button whose only outcome is a typed daemon refusal.
///
/// The enforcing authority is the daemon endpoint
/// (`daemon::governed_wiring::promote_governed_outcome`) and, for the
/// at-most-once rule, the state layer's `RecordPromotion` transition. This is
/// the same predicate expressed once so a surface and the daemon cannot drift,
/// exactly as [`staged_worktree_is_discardable`] is for the discard path.
///
/// Deliberately *not* included: whether the canonical branch can actually be
/// advanced. That is observable only by running Git, and a canonical head that
/// moved is an execution fact the daemon reports as
/// `GovernedPromotionOutcome::PromotionBlocked` — never a reason to refuse the
/// attempt up front.
pub fn governed_outcome_is_promotable(task: &GovernedTaskRun) -> bool {
    task.world_scope == WorldScope::StagedAuthoritative
        && task.is_accepted()
        && task.active_staged_worktree().is_some()
        // A run is promoted at most once; a blocked run may still be retried.
        && !task
            .latest_promotion()
            .is_some_and(|promotion| promotion.outcome.is_promoted())
}

/// The accepted commit that discarding this staged worktree would leave
/// reachable only through the reflog.
///
/// An accepted run's work lives only in the staged checkout until a promotion
/// puts it on the canonical branch. So the question is not "was a promotion
/// blocked" but "is this run accepted and *not* promoted" — and that covers
/// three populations, not one:
///
/// - a blocked promotion (the canonical branch never moved);
/// - an accepted run with **no promotion attempt at all**, which is reachable
///   whenever the run is discardable for some other reason — most concretely an
///   unpinned staged worktree, which [`staged_worktree_is_discardable`]
///   short-circuits to `true` precisely because it can never be promoted. That
///   is the exact population `PromotionBlockedReason::RepositoryConfigUnpinned`
///   exists for, and it is the one this function used to answer `None` for,
///   telling an operator a discard cost nothing moments before it cost them a
///   commit (review round 1, P1);
/// - an accepted run whose promotion succeeded — the only case that genuinely
///   costs nothing, because the commit is on the canonical branch.
///
/// The OID comes from the same place the daemon's own promotion producer takes
/// it: `governed_producers::promote_governed_outcome` sets
/// `accepted_revision = task.latest_claim().subject_revision`. A blocked
/// promotion's recorded `accepted_revision` is preferred when one exists,
/// because that is the value the daemon already committed to the ledger.
pub fn unreferenced_accepted_commit_on_discard(task: &GovernedTaskRun) -> Option<&str> {
    if !task.is_accepted() {
        return None;
    }
    match task.latest_promotion() {
        // Promoted: the commit is on the canonical branch and survives.
        Some(promotion) if promotion.outcome.is_promoted() => None,
        // Blocked: answer with the OID the daemon already recorded.
        Some(promotion) => Some(promotion.accepted_revision.as_str()),
        // Never attempted. An accepted run cannot exist without the claim the
        // acceptance was granted against, so this is the same OID a promotion
        // would have used.
        None => task
            .latest_claim()
            .map(|claim| claim.subject_revision.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governed_task::{
        ApprovalPolicy, GovernedActor, GovernedActorKind, GovernedPromotion,
        GovernedPromotionOutcome, GovernedRecordId, GovernedVerificationProfile,
        PromotionBlockedReason, SharedRepositoryConfigPin, StagedWorktree, StagedWorktreeStatus,
        WorkerCompletionClaim, WorldScope,
    };

    fn actor() -> GovernedActor {
        GovernedActor {
            kind: GovernedActorKind::System,
            id: "impulse-daemon:staged_worktree".to_string(),
        }
    }

    fn oid(character: char) -> String {
        character.to_string().repeat(40)
    }

    fn task() -> GovernedTaskRun {
        GovernedTaskRun {
            id: GovernedTaskId::try_new("task-wiring").unwrap(),
            revision: 4,
            project_id: "demo".to_string(),
            workspace_root: "/tmp/demo".to_string(),
            task: "Ship the wiring".to_string(),
            acceptance_criteria: vec!["the gate is green".to_string()],
            approval_policy: ApprovalPolicy::OperatorRequired,
            world_scope: WorldScope::StagedAuthoritative,
            verification_profile: Some(GovernedVerificationProfile::RustWorkspaceV1),
            role_assignment: None,
            role_compatibility: None,
            runtime_id: "ion".to_string(),
            agent_id: "worker-1".to_string(),
            session_id: None,
            initial_subject_revision: Some(oid('a')),
            staged_worktree: None,
            promotions: Vec::new(),
            execution_state: GovernedExecutionState::Running,
            review_state: GovernedReviewState::AwaitingClaim,
            claims: Vec::new(),
            verifications: Vec::new(),
            supervisor_verdicts: Vec::new(),
            operator_decisions: Vec::new(),
            events: Vec::new(),
            created_at: "2026-09-12T00:00:00Z".to_string(),
            updated_at: "2026-09-12T00:00:00Z".to_string(),
        }
    }

    fn with_staged(mut task: GovernedTaskRun, pin: SharedRepositoryConfigPin) -> GovernedTaskRun {
        task.staged_worktree = Some(StagedWorktree {
            id: GovernedRecordId::try_new("staged-1").unwrap(),
            actor: actor(),
            root: "/tmp/demo/.impulse/worktrees/task-wiring".to_string(),
            initial_subject_revision: oid('a'),
            shared_config_digest: pin,
            status: StagedWorktreeStatus::Active,
            materialized_at: "2026-09-12T00:00:00Z".to_string(),
            based_on_revision: 1,
        });
        task
    }

    fn pinned() -> SharedRepositoryConfigPin {
        SharedRepositoryConfigPin::Recorded(crate::governed_task::SharedRepositoryConfigDigest {
            repository_config: format!("sha256:{}", "c".repeat(64)),
            worktree_config: None,
            info_attributes: None,
        })
    }

    fn with_claim(mut task: GovernedTaskRun) -> GovernedTaskRun {
        task.claims.push(WorkerCompletionClaim {
            id: GovernedRecordId::try_new("claim-1").unwrap(),
            actor: GovernedActor {
                kind: GovernedActorKind::Worker,
                id: "worker-1".to_string(),
            },
            summary: "done".to_string(),
            subject_revision: oid('b'),
            artifact_ids: Vec::new(),
            diff_ref: None,
            loop_report_digest: None,
            loop_report_version: None,
            submitted_at: "2026-09-12T00:00:00Z".to_string(),
            based_on_revision: 2,
        });
        task
    }

    fn with_promotion(
        mut task: GovernedTaskRun,
        outcome: GovernedPromotionOutcome,
    ) -> GovernedTaskRun {
        task.promotions.push(GovernedPromotion {
            id: GovernedRecordId::try_new("promotion-1").unwrap(),
            actor: actor(),
            accepted_revision: oid('b'),
            initial_subject_revision: oid('a'),
            outcome,
            recorded_at: "2026-09-12T00:00:00Z".to_string(),
            based_on_revision: 3,
        });
        task
    }

    fn promotion_request() -> GovernedPromotionRequest {
        GovernedPromotionRequest {
            request_id: GovernedRequestId::try_new("promote-1").unwrap(),
            project_id: "demo".to_string(),
            task_id: GovernedTaskId::try_new("task-wiring").unwrap(),
            expected_revision: 4,
        }
    }

    fn discard_request() -> GovernedStagedWorktreeDiscardRequest {
        GovernedStagedWorktreeDiscardRequest {
            request_id: GovernedRequestId::try_new("discard-1").unwrap(),
            project_id: "demo".to_string(),
            task_id: GovernedTaskId::try_new("task-wiring").unwrap(),
            expected_revision: 4,
            reason: "the run was rejected".to_string(),
        }
    }

    #[test]
    fn test_promotion_request_round_trips_through_serde() {
        let original = promotion_request();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GovernedPromotionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn test_discard_request_round_trips_through_serde() {
        let original = discard_request();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GovernedStagedWorktreeDiscardRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn test_producer_ack_round_trips_through_serde() {
        let original = GovernedProducerAck::new(
            with_staged(task(), pinned()),
            true,
            Some("interrupted before receipt".to_string()),
        );
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GovernedProducerAck = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn test_discard_ack_round_trips_through_serde() {
        let original = GovernedStagedWorktreeDiscardAck {
            task: with_staged(task(), pinned()),
            discarded_root: "/tmp/demo/.impulse/worktrees/task-wiring".to_string(),
            unreferenced_accepted_commit: Some(oid('b')),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GovernedStagedWorktreeDiscardAck = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
    }

    /// The acknowledgements flatten the task, so a client written against the
    /// pre-v9 bare-`GovernedTaskRun` response still reads one.
    #[test]
    fn test_producer_ack_is_wire_compatible_with_a_bare_governed_task() {
        let expected = with_staged(task(), pinned());
        let ack = GovernedProducerAck::new(expected.clone(), false, None);
        let json = serde_json::to_value(&ack).unwrap();
        let recovered: GovernedTaskRun = serde_json::from_value(json).unwrap();
        assert_eq!(recovered, expected);
    }

    #[test]
    fn test_discard_ack_is_wire_compatible_with_a_bare_governed_task() {
        let expected = with_staged(task(), pinned());
        let ack = GovernedStagedWorktreeDiscardAck {
            task: expected.clone(),
            discarded_root: "/tmp/demo/.impulse/worktrees/task-wiring".to_string(),
            unreferenced_accepted_commit: None,
        };
        let json = serde_json::to_value(&ack).unwrap();
        let recovered: GovernedTaskRun = serde_json::from_value(json).unwrap();
        assert_eq!(recovered, expected);
    }

    #[test]
    fn test_promotion_request_rejects_unknown_fields() {
        let error = serde_json::from_str::<GovernedPromotionRequest>(
            r#"{"request_id":"promote-1","project_id":"demo","task_id":"task-wiring","expected_revision":4,"outcome":"promoted"}"#,
        )
        .expect_err("a caller must not be able to author the promotion outcome");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn test_discard_request_rejects_unknown_fields() {
        let error = serde_json::from_str::<GovernedStagedWorktreeDiscardRequest>(
            r#"{"request_id":"discard-1","project_id":"demo","task_id":"task-wiring","expected_revision":4,"reason":"x","actor":"operator"}"#,
        )
        .expect_err("a caller must not be able to author the discard actor");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn test_discard_request_validate_rejects_a_blank_reason() {
        let mut request = discard_request();
        request.reason = "   ".to_string();
        let error = request
            .validate()
            .expect_err("a blank discard reason must be refused");
        assert!(matches!(
            error,
            GovernedTaskContractError::InvalidField {
                field: "reason",
                ..
            }
        ));
        assert!(error.to_string().contains("reason"));
    }

    #[test]
    fn test_discard_request_validate_rejects_a_nul_byte_and_oversize_text() {
        let mut request = discard_request();
        request.reason = "bad\0reason".to_string();
        assert!(request.validate().is_err());

        let mut oversize = discard_request();
        oversize.reason = "x".repeat(MAX_GOVERNED_TEXT_BYTES + 1);
        assert!(oversize.validate().is_err());
    }

    #[test]
    fn test_discard_request_validate_accepts_a_real_reason() {
        assert!(discard_request().validate().is_ok());
    }

    #[test]
    fn test_a_live_staged_worktree_is_not_discardable() {
        let live = with_staged(task(), pinned());
        assert!(!staged_worktree_is_discardable(&live));
    }

    #[test]
    fn test_terminal_review_states_are_discardable() {
        for review_state in [
            GovernedReviewState::Rejected,
            GovernedReviewState::Escalated,
        ] {
            let mut terminal = with_staged(task(), pinned());
            terminal.review_state = review_state;
            assert!(
                staged_worktree_is_discardable(&terminal),
                "{review_state:?} must be reclaimable"
            );
        }
    }

    #[test]
    fn test_an_accepted_task_is_discardable_only_after_a_promotion_outcome() {
        let mut accepted = with_staged(task(), pinned());
        accepted.review_state = GovernedReviewState::Accepted;
        assert!(
            !staged_worktree_is_discardable(&accepted),
            "an accepted run with no promotion outcome still owns its checkout"
        );

        let promoted = with_promotion(
            accepted.clone(),
            GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
        );
        assert!(staged_worktree_is_discardable(&promoted));

        let blocked = with_promotion(
            accepted,
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            },
        );
        assert!(staged_worktree_is_discardable(&blocked));
    }

    #[test]
    fn test_a_launch_failure_and_an_unpinned_worktree_are_always_discardable() {
        let mut launch_failed = with_staged(task(), pinned());
        launch_failed.execution_state = GovernedExecutionState::LaunchFailed;
        assert!(staged_worktree_is_discardable(&launch_failed));

        let unpinned = with_staged(task(), SharedRepositoryConfigPin::Unknown);
        assert!(staged_worktree_is_discardable(&unpinned));
    }

    #[test]
    fn test_an_unpromoted_accepted_run_names_an_unreferenced_commit() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
        accepted.review_state = GovernedReviewState::Accepted;

        // Review round 1, P1 corrected this assertion. It used to demand `None`
        // here on the reasoning that "no promotion has been attempted yet" --
        // but the commit's reachability does not depend on whether anyone tried
        // to promote it. An accepted, unpromoted run's work lives only in the
        // staged checkout either way.
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&accepted),
            Some(oid('b').as_str()),
            "an accepted run that was never promoted still loses its commit"
        );

        let promoted = with_promotion(
            accepted.clone(),
            GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
        );
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&promoted),
            None,
            "a promoted commit is on the canonical branch"
        );

        let blocked = with_promotion(
            accepted,
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::DetachedHead,
            },
        );
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&blocked),
            Some(oid('b').as_str())
        );
    }

    #[test]
    fn test_a_rejected_run_names_no_unreferenced_accepted_commit() {
        let mut rejected = with_claim(with_staged(task(), pinned()));
        rejected.review_state = GovernedReviewState::Rejected;
        assert_eq!(unreferenced_accepted_commit_on_discard(&rejected), None);
    }

    /// Every clause the daemon endpoint checks before it runs Git, one at a
    /// time. A surface that disables Promote on a different rule than the
    /// daemon enforces is the drift this predicate exists to prevent.
    #[test]
    fn test_promotable_requires_a_staged_scope_acceptance_and_an_active_worktree() {
        let mut accepted = with_staged(task(), pinned());
        accepted.review_state = GovernedReviewState::Accepted;
        assert!(governed_outcome_is_promotable(&accepted));

        let mut authoritative = accepted.clone();
        authoritative.world_scope = WorldScope::Authoritative;
        assert!(
            !governed_outcome_is_promotable(&authoritative),
            "promotion is a staged-scope operation"
        );

        let mut awaiting_operator = accepted.clone();
        awaiting_operator.review_state = GovernedReviewState::AwaitingOperator;
        assert!(
            !governed_outcome_is_promotable(&awaiting_operator),
            "nothing is promoted before an operator accepts it"
        );

        let mut discarded = accepted.clone();
        if let Some(staged) = discarded.staged_worktree.as_mut() {
            staged.status = StagedWorktreeStatus::Discarded;
        }
        assert!(
            !governed_outcome_is_promotable(&discarded),
            "a reclaimed checkout has nothing left to promote"
        );

        let mut unstaged = accepted;
        unstaged.staged_worktree = None;
        assert!(!governed_outcome_is_promotable(&unstaged));
    }

    /// Review round 1, P1. An accepted run whose staged worktree carries no
    /// configuration pin is discardable through
    /// [`staged_worktree_is_discardable`]'s unknown-pin short-circuit **with
    /// zero promotion attempts** — the population
    /// `PromotionBlockedReason::RepositoryConfigUnpinned` exists for. This
    /// function used to answer `None` there, so the surface offering the
    /// discard told the operator it cost nothing moments before it cost them a
    /// commit.
    #[test]
    fn test_an_accepted_run_names_its_commit_even_with_no_promotion_attempt() {
        let mut unpinned = with_claim(with_staged(task(), SharedRepositoryConfigPin::Unknown));
        unpinned.review_state = GovernedReviewState::Accepted;

        assert!(
            staged_worktree_is_discardable(&unpinned),
            "an unpinned worktree can never be promoted, so it is always reclaimable"
        );
        assert!(
            unpinned.latest_promotion().is_none(),
            "the discard is reachable without any promotion attempt"
        );
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&unpinned),
            Some(oid('b').as_str()),
            "the OID must come from the accepted claim, the same place the daemon's promotion \
             producer takes it"
        );

        // Same shape, a recorded pin: still accepted, still unpromoted, still
        // costs a commit. The pin is not what makes the cost real.
        let mut pinned_accepted = with_claim(with_staged(task(), pinned()));
        pinned_accepted.review_state = GovernedReviewState::Accepted;
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&pinned_accepted),
            Some(oid('b').as_str())
        );
    }

    /// The three populations, kept apart. Only a promoted run costs nothing.
    #[test]
    fn test_only_a_promoted_run_costs_nothing_to_discard() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
        accepted.review_state = GovernedReviewState::Accepted;

        let promoted = with_promotion(
            accepted.clone(),
            GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
        );
        assert_eq!(unreferenced_accepted_commit_on_discard(&promoted), None);

        let blocked = with_promotion(
            accepted.clone(),
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::RepositoryConfigUnpinned,
            },
        );
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&blocked),
            Some(oid('b').as_str()),
            "a blocked promotion answers with the OID the daemon already recorded"
        );

        // Accepted but claimless: the state layer should not produce this, but
        // the function must not invent an OID for it.
        let mut claimless = accepted.clone();
        claimless.claims.clear();
        assert_eq!(unreferenced_accepted_commit_on_discard(&claimless), None);

        // Not accepted: nothing was ever accepted to lose.
        let mut rejected = accepted;
        rejected.review_state = GovernedReviewState::Rejected;
        assert_eq!(unreferenced_accepted_commit_on_discard(&rejected), None);
    }

    /// Review round 1, P2: the promote-side counterpart to the daemon's
    /// `both_discardability_rules_agree_over_the_whole_state_matrix`.
    ///
    /// `governed_outcome_is_promotable` must be a **superset** of the daemon
    /// endpoint's inline checks (never offer what the endpoint refuses) and a
    /// **subset** of the state layer's `RecordPromotion` preconditions (never
    /// offer what the ledger would reject).
    ///
    /// **Boundary, stated rather than hidden:** the authorities live in
    /// `impulse-rs` (`daemon::governed_wiring::promote_governed_outcome` and
    /// `state::governed_task`'s `RecordPromotion` arm), which `impulse-ops`
    /// cannot depend on and which this lane does not own. The two rules are
    /// therefore restated here from those exact sites and the relationship is
    /// checked over the full matrix. The version that imports the real
    /// functions belongs beside the discardability matrix in
    /// `src/daemon/governed_wiring.rs`; it is a handoff, recorded on the lane
    /// card.
    #[test]
    fn test_promotability_is_between_the_daemon_and_state_layer_rules_over_the_matrix() {
        // Mirrors `daemon::governed_wiring::promote_governed_outcome`'s
        // preflight: require_staged_scope, is_accepted, active_staged_worktree.
        fn daemon_inline_checks(task: &GovernedTaskRun) -> bool {
            task.world_scope == WorldScope::StagedAuthoritative
                && task.is_accepted()
                && task.active_staged_worktree().is_some()
        }
        // Mirrors `state::governed_task`'s `RecordPromotion` arm: it refuses
        // "this governed outcome was already promoted" and otherwise validates
        // the outcome. Everything else it accepts.
        fn state_layer_preconditions(task: &GovernedTaskRun) -> bool {
            !task
                .latest_promotion()
                .is_some_and(|promotion| promotion.outcome.is_promoted())
        }

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
        let scopes = [
            WorldScope::StagedAuthoritative,
            WorldScope::Authoritative,
            WorldScope::ReadOnlySnapshot,
            WorldScope::DisposableScratch,
        ];
        let staged_statuses = [
            Some(StagedWorktreeStatus::Active),
            Some(StagedWorktreeStatus::Discarded),
            None,
        ];
        let promotions = [
            None,
            Some(GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            }),
            Some(GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::CanonicalHeadMoved,
            }),
        ];

        let mut compared = 0usize;
        let mut promotable_seen = 0usize;
        for review in reviews {
            for execution in executions {
                for scope in scopes {
                    for staged_status in staged_statuses {
                        for promotion in &promotions {
                            let mut candidate = with_claim(task());
                            candidate.review_state = review;
                            candidate.execution_state = execution;
                            candidate.world_scope = scope;
                            candidate.staged_worktree = staged_status.map(|status| {
                                let mut staged = with_staged(task(), pinned())
                                    .staged_worktree
                                    .expect("fixture staged worktree");
                                staged.status = status;
                                staged
                            });
                            if let Some(outcome) = promotion.clone() {
                                candidate = with_promotion(candidate, outcome);
                            }

                            let promotable = governed_outcome_is_promotable(&candidate);
                            if promotable {
                                promotable_seen += 1;
                            }
                            assert!(
                                !promotable || daemon_inline_checks(&candidate),
                                "offered a promotion the daemon endpoint would refuse: \
                                 review={review:?} execution={execution:?} scope={scope:?} \
                                 staged={staged_status:?} promotion={promotion:?}"
                            );
                            assert!(
                                !promotable || state_layer_preconditions(&candidate),
                                "offered a promotion the state layer would reject: \
                                 review={review:?} execution={execution:?} scope={scope:?} \
                                 staged={staged_status:?} promotion={promotion:?}"
                            );
                            compared += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            compared,
            reviews.len()
                * executions.len()
                * scopes.len()
                * staged_statuses.len()
                * promotions.len()
        );
        assert!(
            compared >= 400,
            "the matrix must be exhaustive, not a sample"
        );
        assert!(
            promotable_seen > 0,
            "a matrix where nothing is promotable proves nothing about the upper bound"
        );
    }

    /// ADR-0019 rule 6: a blocked promotion is an execution fact and the
    /// operator may retry it; a successful one is final.
    #[test]
    fn test_a_blocked_promotion_stays_promotable_and_a_promoted_one_does_not() {
        let mut accepted = with_staged(task(), pinned());
        accepted.review_state = GovernedReviewState::Accepted;

        let blocked = with_promotion(
            accepted.clone(),
            GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: oid('c'),
                reason: PromotionBlockedReason::ConcurrentBranchUpdate,
            },
        );
        assert!(
            governed_outcome_is_promotable(&blocked),
            "reconciling the canonical branch and retrying is the documented remedy"
        );

        let promoted = with_promotion(
            accepted,
            GovernedPromotionOutcome::Promoted {
                promoted_revision: oid('b'),
            },
        );
        assert!(
            !governed_outcome_is_promotable(&promoted),
            "a run is promoted at most once"
        );
    }
}
