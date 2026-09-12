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
    GovernedTaskId, GovernedTaskRun, MAX_GOVERNED_TEXT_BYTES,
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

/// The accepted commit that discarding this staged worktree would leave
/// reachable only through the reflog.
///
/// A blocked promotion never advanced the canonical branch, so the accepted
/// commit exists nowhere but the staged checkout.
pub fn unreferenced_accepted_commit_on_discard(task: &GovernedTaskRun) -> Option<&str> {
    if !task.is_accepted() {
        return None;
    }
    let promotion = task.latest_promotion()?;
    if promotion.outcome.is_promoted() {
        return None;
    }
    Some(promotion.accepted_revision.as_str())
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
    fn test_only_an_accepted_but_blocked_promotion_names_an_unreferenced_commit() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
        accepted.review_state = GovernedReviewState::Accepted;

        assert_eq!(
            unreferenced_accepted_commit_on_discard(&accepted),
            None,
            "no promotion has been attempted yet"
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
}
