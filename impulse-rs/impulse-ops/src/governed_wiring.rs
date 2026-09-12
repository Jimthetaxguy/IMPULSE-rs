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
    GovernedTaskId, GovernedTaskRun, SharedConfigComponent, MAX_GOVERNED_TEXT_BYTES,
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

/// Why a daemon-owned producer refused to run Git inside a staged worktree at
/// all (ADR-0019 rule 13).
///
/// Carried on a *successful-shape* response, exactly like a blocked promotion:
/// the run is not broken and the evidence is not wrong — the staged world is in
/// a state the daemon may not touch, and the remedy is a specific operator
/// action rather than a retry. Making it typed rather than an error string is
/// what lets a surface render the remedy; the desktop currently recognizes this
/// condition by matching on the producer's prose, which breaks the first time
/// that prose is improved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StagedConfigRefusalReason {
    /// The staged worktree predates the configuration pin, so there is nothing
    /// to compare against.
    ///
    /// The wire name is spelled out rather than taken from the variant, so it
    /// matches [`StagedConfigRefusalReason::as_str`] exactly and shares
    /// `PromotionBlockedReason`'s vocabulary for the same condition. Two names
    /// for one reason — a terse `kind` on the wire and a descriptive one in
    /// logs — is a trap for whoever has to correlate them.
    #[serde(rename = "repository_config_unpinned")]
    Unpinned,
    /// Worktree-shared repository configuration changed since materialization.
    /// The component names which file, because benign drift blocks too and the
    /// operator must not have to guess.
    #[serde(rename = "repository_config_changed")]
    Changed { component: SharedConfigComponent },
    /// The repository carries submodule configuration, which the staged scope
    /// cannot pin and therefore refuses to run in.
    #[serde(rename = "unsupported_submodules")]
    UnsupportedSubmodules { path: String },
}

impl StagedConfigRefusalReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unpinned => "repository_config_unpinned",
            Self::Changed { .. } => "repository_config_changed",
            Self::UnsupportedSubmodules { .. } => "unsupported_submodules",
        }
    }

    /// What the operator has to do about it. One remedy per reason, so a
    /// surface never has to infer one.
    pub fn remedy(&self) -> &'static str {
        match self {
            Self::Unpinned | Self::Changed { .. } => {
                "discard the staged worktree and re-materialize it, then re-run the producer"
            }
            Self::UnsupportedSubmodules { .. } => {
                "register this task with the authoritative world scope; the staged scope does not support submodule repositories"
            }
        }
    }
}

impl std::fmt::Display for StagedConfigRefusalReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Changed { component } => {
                write!(formatter, "{} ({component})", self.as_str())
            }
            Self::UnsupportedSubmodules { path } => {
                write!(formatter, "{} ({path})", self.as_str())
            }
            other => formatter.write_str(other.as_str()),
        }
    }
}

/// A producer request answered with a refusal rather than a record.
///
/// Flattened like the other acknowledgements, so the governed task is still
/// readable straight off the response, and `refused` is the discriminator a
/// client checks. The task is unchanged — a refusal records nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernedStagedConfigRefusalAck {
    #[serde(flatten)]
    pub task: GovernedTaskRun,
    /// Always `true`. Present so a client can discriminate on one field rather
    /// than on the presence of `reason`.
    pub refused: bool,
    pub reason: StagedConfigRefusalReason,
    /// `reason.remedy()`, carried on the wire so a surface does not have to
    /// keep its own copy of the mapping.
    pub remedy: String,
}

impl GovernedStagedConfigRefusalAck {
    pub fn new(task: GovernedTaskRun, reason: StagedConfigRefusalReason) -> Self {
        let remedy = reason.remedy().to_string();
        Self {
            task,
            refused: true,
            reason,
            remedy,
        }
    }
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

/// Whether a governed task's *state* admits a promotion attempt.
///
/// Shared so a surface can decide whether to offer the control at all without
/// restating the daemon endpoint's checks or the ledger's preconditions. The
/// relationship the cross-check test in `src/daemon/governed_wiring.rs` pins:
/// this is a **superset** of what the promote endpoint lets through (never
/// refuse something the endpoint would run) and a **subset** of the ledger's
/// `RecordPromotion` preconditions (never promise something the ledger would
/// refuse). It says nothing about whether the promotion would *succeed* — a
/// canonical head that moved is a blocked outcome, not an inadmissible request.
pub fn governed_outcome_is_promotable(task: &GovernedTaskRun) -> bool {
    task.world_scope == crate::governed_task::WorldScope::StagedAuthoritative
        && task.is_accepted()
        && task.active_staged_worktree().is_some()
        && task.latest_claim().is_some()
        && !task
            .latest_promotion()
            .is_some_and(|previous| previous.outcome.is_promoted())
        && task.promotions.len() < crate::governed_task::MAX_GOVERNED_RECORDS_PER_KIND
}

/// The accepted commit that discarding this staged worktree would leave
/// reachable only through the reflog.
///
/// The canonical branch stays on the registered initial OID until a promotion
/// succeeds, so for any accepted run that has not been promoted, the accepted
/// commit exists nowhere but the staged checkout. Two such states reach a
/// discard:
///
/// - **A blocked promotion.** The attempt was made and refused; the recorded
///   outcome carries the revision.
/// - **No promotion at all.** An accepted task whose staged worktree has an
///   `Unknown` configuration pin is discardable *without* a promotion attempt —
///   [`staged_worktree_is_discardable`] short-circuits on an unpinned worktree,
///   because such a worktree can never be promoted and discarding is the only
///   way forward. The first version of this function returned `None` there,
///   which silently dropped the warning in exactly the state that has no other
///   escape. The accepted claim's `subject_revision` is the answer, and it is
///   the same value a recorded promotion would have carried.
pub fn unreferenced_accepted_commit_on_discard(task: &GovernedTaskRun) -> Option<&str> {
    if !task.is_accepted() {
        return None;
    }
    match task.latest_promotion() {
        Some(promotion) if promotion.outcome.is_promoted() => None,
        Some(promotion) => Some(promotion.accepted_revision.as_str()),
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
        SharedRepositoryConfigPin::Recorded(
            crate::governed_task::SharedRepositoryConfigDigest::current(
                format!("sha256:{}", "c".repeat(64)),
                None,
                None,
            ),
        )
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

    /// Every operator-facing string this module and its neighbours produce, checked
    /// for the one defect that has now hit this lane twice: a wrapped string
    /// literal whose `\\` continuation is missing, which the formatter then joins
    /// into one string carrying a run of interior spaces mid-sentence.
    ///
    /// Review round 1 caught it in two CLI messages; it came straight back in
    /// `remedy()` for `UnsupportedSubmodules`, because the literal was authored
    /// through a shell heredoc that ate the backslash. A lint would be better, but
    /// a test that enumerates the strings is what is available, so these are
    /// enumerated at their source rather than at each render site.
    #[test]
    fn test_operator_facing_strings_have_no_run_of_interior_spaces() {
        let mut messages: Vec<String> = Vec::new();
        for reason in every_refusal_reason() {
            messages.push(reason.remedy().to_string());
            messages.push(reason.to_string());
            messages.push(reason.as_str().to_string());
        }
        for reason in every_blocked_reason() {
            messages.push(reason.to_string());
            messages.push(reason.as_str().to_string());
        }
        assert!(
            messages.len() >= 15,
            "the enumerations must not have gone empty"
        );
        for message in messages {
            assert!(!message.trim().is_empty(), "empty operator string");
            assert_eq!(
                message.trim(),
                message,
                "operator strings carry no leading or trailing whitespace: {message:?}"
            );
            assert!(
                !message.contains("  "),
                "operator string has a run of interior spaces, which is what a wrapped \
                 literal with no continuation looks like once it is joined: {message:?}"
            );
            assert!(
                !message.contains('\n') && !message.contains('\t'),
                "operator string must be one line: {message:?}"
            );
        }
    }

    /// One of each. The match below is what fails to compile if a variant is
    /// added without extending this list.
    fn every_refusal_reason() -> Vec<StagedConfigRefusalReason> {
        let all = vec![
            StagedConfigRefusalReason::Unpinned,
            StagedConfigRefusalReason::Changed {
                component: SharedConfigComponent::RepositoryConfig,
            },
            StagedConfigRefusalReason::Changed {
                component: SharedConfigComponent::WorktreeConfig,
            },
            StagedConfigRefusalReason::Changed {
                component: SharedConfigComponent::InfoAttributes,
            },
            StagedConfigRefusalReason::UnsupportedSubmodules {
                path: ".git/modules/vendor".to_string(),
            },
        ];
        for reason in &all {
            match reason {
                StagedConfigRefusalReason::Unpinned => {}
                StagedConfigRefusalReason::Changed { .. } => {}
                StagedConfigRefusalReason::UnsupportedSubmodules { .. } => {}
            }
        }
        all
    }

    /// Same shape for ADR-0019's blocked-promotion reasons, which the promote
    /// endpoint and the CLI both render.
    fn every_blocked_reason() -> Vec<PromotionBlockedReason> {
        let all = vec![
            PromotionBlockedReason::CanonicalHeadMoved,
            PromotionBlockedReason::DetachedHead,
            PromotionBlockedReason::ConcurrentBranchUpdate,
            PromotionBlockedReason::RepositoryConfigUnpinned,
            PromotionBlockedReason::RepositoryConfigChanged {
                component: SharedConfigComponent::RepositoryConfig,
            },
            PromotionBlockedReason::RepositoryConfigChanged {
                component: SharedConfigComponent::WorktreeConfig,
            },
            PromotionBlockedReason::RepositoryConfigChanged {
                component: SharedConfigComponent::InfoAttributes,
            },
        ];
        for reason in &all {
            match reason {
                PromotionBlockedReason::CanonicalHeadMoved => {}
                PromotionBlockedReason::DetachedHead => {}
                PromotionBlockedReason::ConcurrentBranchUpdate => {}
                PromotionBlockedReason::RepositoryConfigChanged { .. } => {}
                PromotionBlockedReason::RepositoryConfigUnpinned => {}
            }
        }
        all
    }

    /// The specific string that regressed, pinned by content as well as by shape.
    #[test]
    fn test_the_submodule_remedy_reads_as_one_sentence() {
        let remedy = StagedConfigRefusalReason::UnsupportedSubmodules {
            path: ".git/modules/vendor".to_string(),
        }
        .remedy();
        assert_eq!(
            remedy,
            "register this task with the authoritative world scope; the staged scope does not support submodule repositories"
        );
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

    fn refusal_ack() -> GovernedStagedConfigRefusalAck {
        GovernedStagedConfigRefusalAck::new(
            with_staged(task(), pinned()),
            StagedConfigRefusalReason::Changed {
                component: SharedConfigComponent::InfoAttributes,
            },
        )
    }

    #[test]
    fn test_refusal_ack_round_trips_through_serde() {
        let original = refusal_ack();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GovernedStagedConfigRefusalAck = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
        assert!(recovered.refused);
        assert_eq!(recovered.remedy, recovered.reason.remedy());
    }

    #[test]
    fn test_every_refusal_reason_round_trips_through_serde() {
        for reason in every_refusal_reason() {
            let json = serde_json::to_string(&reason).unwrap();
            let recovered: StagedConfigRefusalReason = serde_json::from_str(&json).unwrap();
            assert_eq!(recovered, reason, "round trip changed {json}");
            // The wire discriminator is the snake_case `kind`, which is what a
            // non-Rust client matches on.
            // One name per reason: the wire discriminator and the string a log
            // or a CLI renders must be the same token, or correlating them
            // becomes a lookup table nobody maintains.
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(value["kind"], reason.as_str());
        }
    }

    /// Same guarantee the two sibling acks carry: the refusal flattens the
    /// governed task, so a client written against the pre-v9 bare
    /// `GovernedTaskRun` response still parses one out of it. Without this a
    /// refusal would look like a malformed task to an older reader rather than
    /// like a task plus fields it does not know.
    #[test]
    fn test_refusal_ack_is_wire_compatible_with_a_bare_governed_task() {
        let expected = with_staged(task(), pinned());
        let ack = GovernedStagedConfigRefusalAck::new(
            expected.clone(),
            StagedConfigRefusalReason::Unpinned,
        );
        let json = serde_json::to_value(&ack).unwrap();
        let recovered: GovernedTaskRun = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(recovered, expected);
        // And the discriminator an updated client keys on is present beside it.
        assert_eq!(json["refused"], serde_json::Value::Bool(true));
        assert_eq!(json["reason"]["kind"], "repository_config_unpinned");
        assert!(json["remedy"].as_str().unwrap().contains("re-materialize"));
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
    fn test_an_accepted_run_that_was_never_promoted_names_its_unreferenced_commit() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
        accepted.review_state = GovernedReviewState::Accepted;

        // Renamed from "only an accepted but blocked promotion ...", which
        // stopped being true: the canonical branch stays on the initial OID
        // until a promotion *succeeds*, so an accepted run with no promotion
        // attempt orphans its commit just as a blocked one does. That state is
        // reachable -- an accepted task with an unpinned worktree is
        // discardable without any promotion attempt -- and returning `None`
        // there dropped the warning in the one case that has no other escape.
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&accepted),
            Some(oid('b').as_str()),
            "an accepted run with no promotion attempt still orphans its commit"
        );

        // The state that actually reaches a discard with no promotion.
        let mut unpinned = with_claim(with_staged(task(), SharedRepositoryConfigPin::Unknown));
        unpinned.review_state = GovernedReviewState::Accepted;
        assert!(
            staged_worktree_is_discardable(&unpinned),
            "an unpinned worktree is discardable with no promotion attempt"
        );
        assert_eq!(
            unreferenced_accepted_commit_on_discard(&unpinned),
            Some(oid('b').as_str()),
            "the accepted/unpinned/zero-promotions case must not lose its warning"
        );

        // With no claim there is nothing to name, and nothing to lose.
        let mut claimless = with_staged(task(), SharedRepositoryConfigPin::Unknown);
        claimless.review_state = GovernedReviewState::Accepted;
        assert_eq!(unreferenced_accepted_commit_on_discard(&claimless), None);

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
    ///
    /// The fixture carries a claim because `governed_outcome_is_promotable`
    /// requires one: `governed_producers::promote_governed_outcome` takes the
    /// accepted revision from `latest_claim()`, so a claimless accepted run is
    /// not a promotable state — and is not one the ledger produces either.
    #[test]
    fn test_promotable_requires_a_staged_scope_acceptance_and_an_active_worktree() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
        accepted.review_state = GovernedReviewState::Accepted;
        assert!(governed_outcome_is_promotable(&accepted));

        let mut claimless = accepted.clone();
        claimless.claims.clear();
        assert!(
            !governed_outcome_is_promotable(&claimless),
            "promotion reads its accepted revision off the claim, so there must be one"
        );

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

    /// ADR-0019 rule 6: a blocked promotion is an execution fact and the
    /// operator may retry it; a successful one is final.
    #[test]
    fn test_a_blocked_promotion_stays_promotable_and_a_promoted_one_does_not() {
        let mut accepted = with_claim(with_staged(task(), pinned()));
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
