//! Durable review queue projected from accepted governed-task evidence.
//!
//! Governed tasks remain the source of truth. This ledger is independently
//! replaceable and is repaired deterministically after an interrupted
//! acceptance response or daemon restart.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use impulse_ops::governed_task::{GovernedRequestId, GovernedTaskId, OperatorAuthentication};
use impulse_ops::governed_task::{
    GovernedReviewState, GovernedTaskRun, GovernedVerificationOutcome, OperatorDecisionKind,
    SupervisorVerdictKind,
};
use impulse_ops::memory_candidate::{
    AcceptedRunCommandEvidence, AcceptedRunMemoryCandidate, AcceptedRunSourceAssurance,
    MemoryCandidateId, MemoryCandidateStatus, MemoryRecord,
    ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION, ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
};
use impulse_ops::memory_wiring::{
    MemoryCandidateDecision, MemoryCandidateDecisionInput, MemoryCandidateDecisionKind,
    MemoryCandidateDecisionOutcome,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::memory_record::{
    carry_status_forward, derive_promoted_record, read_index_marker, render_projection,
    write_projection, LedgerOrigin, MemoryLog, MemoryLogHead,
};
use super::State;
use crate::storage::Storage;

const MEMORY_CANDIDATES_FILE: &str = "MEMORY_CANDIDATES.json";
/// Bumped to 2 by ADR-0020: the ledger gained a compare-and-swap `revision`,
/// idempotency receipts, the committed memory-log head, and carried-forward
/// review statuses. Every new field is serde-defaulted, so a v1 ledger loads
/// unchanged and is rewritten at v2 on its first persist.
const MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION: u32 = 2;
const MEMORY_CANDIDATES_LEDGER_MIN_SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Typed decision failures. The daemon lane maps these onto its wire errors;
/// every one of them leaves the ledger, the log, and the projection untouched.
#[derive(Debug, thiserror::Error)]
pub enum MemoryCandidateDecisionError {
    #[error("memory candidate `{0}` was not found")]
    NotFound(MemoryCandidateId),
    #[error("memory candidate ledger revision conflict: expected {expected}, current {current}")]
    RevisionConflict { expected: u64, current: u64 },
    #[error(
        "memory candidate decision request id `{request_id}` was replayed with a different payload"
    )]
    IdempotencyPayloadConflict { request_id: GovernedRequestId },
    #[error(
        "memory candidate `{candidate_id}` was already {status}; a review decision is terminal"
    )]
    AlreadyDecided {
        candidate_id: MemoryCandidateId,
        status: &'static str,
    },
    #[error(
        "memory candidate `{candidate_id}` is superseded: it no longer matches the current deterministic derivation from accepted governed-task truth, so promoting it would record a fact the evidence no longer supports"
    )]
    SupersededCandidate { candidate_id: MemoryCandidateId },
    #[error(
        "a previous memory decision (request `{request_id}`) was interrupted after appending to the memory log and before committing the ledger; replay that exact request id to finish it before deciding anything else"
    )]
    InterruptedDecision { request_id: GovernedRequestId },
    #[error("memory candidate decisions belong to project `{expected}`, not `{actual}`")]
    ProjectMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MemoryCandidateLedger {
    schema_version: u32,
    /// Compare-and-swap revision for review decisions (ADR-0020). Serde-default
    /// 0 so a v1 ledger's first decision starts the sequence cleanly.
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    candidates: BTreeMap<MemoryCandidateId, AcceptedRunMemoryCandidate>,
    /// Review decisions belonging to candidates that a derivation-version
    /// migration has re-derived under a new id, keyed by governed task. Applied
    /// by reconciliation and then cleared. Persisted rather than held in memory
    /// so an interrupted migration does not lose an operator's decision.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pending_status_migrations: BTreeMap<GovernedTaskId, MemoryCandidateStatus>,
    /// The head of `MEMORY.jsonl` this ledger commits. The witness that makes
    /// trailing truncation of the append-only log detectable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory_log_head: Option<MemoryLogHead>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    processed_decisions: BTreeMap<GovernedRequestId, MemoryCandidateDecision>,
}

/// What a derivation-version migration moved, for the startup log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DerivationMigration {
    candidates: usize,
    carried_decisions: usize,
}

impl Default for MemoryCandidateLedger {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION,
            revision: 0,
            candidates: BTreeMap::new(),
            pending_status_migrations: BTreeMap::new(),
            memory_log_head: None,
            processed_decisions: BTreeMap::new(),
        }
    }
}

impl MemoryCandidateLedger {
    fn load(storage: &Storage) -> Result<Self> {
        let mut ledger: Self = storage
            .read_json(MEMORY_CANDIDATES_FILE)
            .context("Failed to read accepted-run memory candidate ledger")?;
        let migrated = ledger.migrate_superseded_derivations();
        if migrated.candidates > 0 {
            tracing::info!(
                superseded_candidates = migrated.candidates,
                carried_decisions = migrated.carried_decisions,
                derivation_version = ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
                "migrating memory candidates derived under a superseded derivation version; \
                 they are re-derived from accepted governed-task truth and any review decision \
                 is carried forward"
            );
        }
        ledger.validate_shape()?;
        Ok(ledger)
    }

    /// Migrate candidates whose `derivation_version` is not the current one,
    /// carrying any review decision forward.
    ///
    /// Governed tasks remain the source of truth and this ledger is
    /// independently replaceable, so a derivation-version bump must reconcile
    /// deterministically rather than fail an otherwise healthy daemon at
    /// startup: `validate_shape` rejects a stale `derivation_version`, and
    /// reconcile would then see a stale candidate as orphaned (its id is a
    /// digest over the derivation version).
    ///
    /// ADR-0018 follow-up 2: this replaces the ADR-0013-era
    /// `prune_superseded_derivations`, which simply dropped the record.
    /// Dropping was lossless only while `MemoryCandidateStatus` had one
    /// variant. Now that a candidate can be promoted or dismissed, a prune
    /// would silently revert an operator's review decision to
    /// `PendingReview` — and, for a promotion, orphan the record already in
    /// `MEMORY.jsonl`. The decision is parked under the candidate's governed
    /// task id, which survives re-derivation, and reconciliation reapplies it
    /// to the freshly derived candidate.
    fn migrate_superseded_derivations(&mut self) -> DerivationMigration {
        let mut migration = DerivationMigration::default();
        let stale = self
            .candidates
            .iter()
            .filter(|(_, candidate)| {
                candidate.derivation_version != ACCEPTED_RUN_MEMORY_DERIVATION_VERSION
            })
            .map(|(id, candidate)| {
                (
                    id.clone(),
                    candidate.governed_task_id.clone(),
                    candidate.status.clone(),
                )
            })
            .collect::<Vec<_>>();
        for (candidate_id, governed_task_id, status) in stale {
            self.candidates.remove(&candidate_id);
            migration.candidates += 1;
            if !status.is_pending() {
                migration.carried_decisions += 1;
                self.pending_status_migrations
                    .insert(governed_task_id, status);
            }
        }
        migration
    }

    fn validate_shape(&self) -> Result<()> {
        if self.schema_version < MEMORY_CANDIDATES_LEDGER_MIN_SUPPORTED_SCHEMA_VERSION
            || self.schema_version > MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION
        {
            anyhow::bail!(
                "Unsupported memory candidate ledger schema version {}",
                self.schema_version
            );
        }
        let mut task_ids = BTreeSet::new();
        for (candidate_id, candidate) in &self.candidates {
            if candidate_id != &candidate.id {
                anyhow::bail!(
                    "memory candidate ledger key `{candidate_id}` does not match record id `{}`",
                    candidate.id
                );
            }
            candidate
                .validate_shape()
                .with_context(|| format!("Invalid memory candidate `{candidate_id}`"))?;
            if !task_ids.insert(candidate.governed_task_id.clone()) {
                anyhow::bail!(
                    "governed task `{}` has more than one accepted-run memory candidate",
                    candidate.governed_task_id
                );
            }
        }
        Ok(())
    }
}

impl State {
    pub(super) fn load_memory_candidate_ledger(
        storage: &Storage,
    ) -> Result<std::sync::Mutex<MemoryCandidateLedger>> {
        Ok(std::sync::Mutex::new(MemoryCandidateLedger::load(storage)?))
    }

    /// Reconcile the independently persisted review queue from authoritative
    /// accepted tasks. Missing candidates are recoverable; orphaned or
    /// digest-mismatched candidates fail closed.
    pub(super) fn reconcile_accepted_run_memory_candidates(&self) -> Result<()> {
        let tasks = self.all_governed_tasks()?;
        let expected = tasks
            .iter()
            .filter(|task| task.is_accepted())
            .map(derive_accepted_run_memory_candidate)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|candidate| (candidate.id.clone(), candidate))
            .collect::<BTreeMap<_, _>>();

        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        for (candidate_id, stored) in &ledger.candidates {
            let expected_candidate = expected.get(candidate_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "memory candidate `{candidate_id}` is orphaned from accepted governed-task truth"
                )
            })?;
            // The review status is operator state, not derived state, so the
            // comparison against governed-task truth deliberately ignores it.
            // Everything else must still match byte for byte.
            if !matches_ignoring_status(stored, expected_candidate) {
                anyhow::bail!(
                    "memory candidate `{candidate_id}` does not match its accepted governed-task source"
                );
            }
        }

        let mut repaired = ledger.clone();
        for (candidate_id, mut candidate) in expected {
            if repaired.candidates.contains_key(&candidate_id) {
                continue;
            }
            // A decision parked by a derivation-version migration rejoins its
            // re-derived candidate here, keyed by the governed task id, which
            // survives re-derivation.
            if let Some(status) = repaired
                .pending_status_migrations
                .remove(&candidate.governed_task_id)
            {
                carry_status_forward(&status, &mut candidate);
            }
            repaired.candidates.insert(candidate_id, candidate);
        }
        // A migration whose accepted task is gone cannot be reapplied. A parked
        // *pending* status carries nothing, so drop it. A parked decision is
        // operator state: keep it, say so loudly, and let it name itself in the
        // orphan error if its record is still in the log — silently clearing it
        // would turn a recoverable situation into an error pointing nowhere.
        repaired
            .pending_status_migrations
            .retain(|governed_task_id, status| {
                if status.is_pending() {
                    return false;
                }
                tracing::warn!(
                    governed_task_id = %governed_task_id,
                    status = status.label(),
                    "a parked memory review decision has no accepted governed task to reattach to; \
                     keeping it so its record is not reported as an unexplained orphan"
                );
                true
            });
        if repaired.candidates != ledger.candidates
            || repaired.pending_status_migrations != ledger.pending_status_migrations
        {
            self.storage()
                .write_private_json(MEMORY_CANDIDATES_FILE, &repaired)
                .context("Failed to persist reconciled accepted-run memory candidates")?;
            *ledger = repaired;
        }
        Ok(())
    }

    /// Verify the promoted-memory log against the reconciled candidate ledger
    /// and regenerate the projection. Called once at start-up, after
    /// [`Self::reconcile_accepted_run_memory_candidates`].
    ///
    /// Fails closed: a tampered, truncated, or head-mismatched `MEMORY.jsonl`
    /// stops the daemon from starting rather than serving a silently shortened
    /// memory. The operator surface for that failure is the daemon start-up
    /// error itself (and, in direct CLI mode, the command's error): typed as
    /// [`super::memory_record::MemoryLogError`], naming the file and which of
    /// the distinguishable corruptions was hit.
    pub(super) fn reconcile_promoted_memory_log(&self) -> Result<()> {
        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        // `MEMORY_CANDIDATES.json` is gitignored local state while
        // `MEMORY.jsonl` is tracked, so a fresh clone legitimately has a full
        // log and no ledger. That is not the same as a ledger that commits
        // nothing, and must not be read as an interrupted decision.
        let origin = if self.storage().path(MEMORY_CANDIDATES_FILE).exists() {
            LedgerOrigin::Local
        } else {
            LedgerOrigin::Absent
        };
        let log = MemoryLog::load(self.storage(), ledger.memory_log_head.as_ref(), origin)
            .context("Failed to load the promoted memory log")?;
        log.cross_check_statuses(
            ledger.candidates.values(),
            &ledger.pending_status_migrations,
            origin == LedgerOrigin::Local,
        )
        .context("Promoted memory log does not match the candidate ledger")?;
        if origin == LedgerOrigin::Absent {
            if let Some(adopted) = log.head() {
                tracing::info!(
                    entry_count = adopted.entry_count,
                    "adopting a checked-out promoted memory log: this machine had no candidate \
                     ledger, so the verified log is recorded as committed"
                );
                let mut repaired = ledger.clone();
                repaired.memory_log_head = Some(adopted);
                self.storage()
                    .write_private_json(MEMORY_CANDIDATES_FILE, &repaired)
                    .context("Failed to record the adopted promoted-memory log head")?;
                *ledger = repaired;
            }
        }
        if let Some(entry) = log.uncommitted_tail().first() {
            tracing::warn!(
                request_id = %entry.record.request_id,
                record_id = %entry.record.id,
                "promoted memory log holds one uncommitted entry from an interrupted decision; \
                 replay that request id to finish it"
            );
        }
        // Regenerate the projection so a deleted or hand-edited
        // GENOME_PROJECTION.md is restored from the log. Never marks the
        // retrieval index dirty: start-up changed no promoted record.
        //
        // Skipped entirely when nothing has ever been promoted and no
        // projection exists: `State::new` must not create `.impulse/` (or any
        // file in it) as a side effect of merely opening a project that has no
        // memory yet. A fresh clone takes the other branch — its tracked log is
        // non-empty — and rewrites the tracked projection only if the rendering
        // differs from what was committed.
        let projected = log.projected_records();
        if !projected.is_empty()
            || self
                .storage()
                .path(super::memory_record::GENOME_PROJECTION_FILE)
                .exists()
        {
            write_projection(
                self.storage(),
                &projected,
                &impulse_ops::now_rfc3339(),
                false,
            )
            .context("Failed to regenerate the GENOME projection")?;
        }
        drop(projected);
        let mut slot = self
            .memory_log
            .lock()
            .map_err(|error| anyhow::anyhow!("promoted memory log lock poisoned: {error}"))?;
        *slot = log;
        Ok(())
    }

    /// Ensure the projection exists after an acceptance mutation. The caller
    /// may invoke this again for an idempotent request replay.
    pub(super) fn ensure_accepted_run_memory_candidate(
        &self,
        task: &GovernedTaskRun,
    ) -> Result<()> {
        if !task.is_accepted() {
            return Ok(());
        }
        let candidate = derive_accepted_run_memory_candidate(task)?;
        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        if let Some(stored) = ledger.candidates.get(&candidate.id) {
            if stored != &candidate {
                anyhow::bail!(
                    "memory candidate `{}` conflicts with its deterministic accepted-run projection",
                    candidate.id
                );
            }
            return Ok(());
        }
        if ledger
            .candidates
            .values()
            .any(|stored| stored.governed_task_id == task.id)
        {
            anyhow::bail!(
                "governed task `{}` already has a different memory candidate",
                task.id
            );
        }

        let mut updated = ledger.clone();
        updated.candidates.insert(candidate.id.clone(), candidate);
        self.storage()
            .write_private_json(MEMORY_CANDIDATES_FILE, &updated)
            .context("Failed to persist accepted-run memory candidate")?;
        *ledger = updated;
        Ok(())
    }

    pub fn list_accepted_run_memory_candidates(
        &self,
        project_id: &str,
    ) -> Result<Vec<AcceptedRunMemoryCandidate>> {
        // Reuse the governed-task project boundary instead of independently
        // reconstructing project identity in the projection layer.
        let _ = self.list_governed_tasks(project_id)?;
        let ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        let mut candidates = ledger
            .candidates
            .values()
            .filter(|candidate| candidate.project_id == project_id)
            .cloned()
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.staged_at.cmp(&left.staged_at));
        Ok(candidates)
    }
}

impl State {
    /// Promote or dismiss one pending review candidate (ADR-0020).
    ///
    /// `authentication` is stamped by the caller from the *connection class*,
    /// never read from `input` — `MemoryCandidateDecisionInput` has no such
    /// field and refuses one (ADR-0018's rule, applied to this family). An
    /// in-process or direct-CLI caller passes
    /// [`OperatorAuthentication::Declared`].
    ///
    /// Ordering, and why: the log is appended first and the ledger second. A
    /// process killed between the two leaves one *uncommitted* trailing entry
    /// that the projection and the retrieval index both ignore; replaying the
    /// exact request id adopts that entry instead of appending a duplicate, and
    /// any other decision is refused until it does. The reverse order would
    /// instead leave a promoted candidate naming a record that does not exist,
    /// which is not recoverable from the ledger alone.
    pub fn decide_memory_candidate(
        &self,
        input: MemoryCandidateDecisionInput,
        authentication: OperatorAuthentication,
        now: &str,
    ) -> Result<MemoryCandidateDecisionOutcome> {
        input
            .validate()
            .context("Invalid memory candidate decision")?;
        let expected_project = self.governed_project_id();
        if input.project_id != expected_project {
            return Err(MemoryCandidateDecisionError::ProjectMismatch {
                expected: expected_project,
                actual: input.project_id,
            }
            .into());
        }

        // Derive governed truth before taking the candidate-ledger lock, so the
        // two locks are always acquired in the same order as reconciliation.
        let expected_candidates = self
            .all_governed_tasks()?
            .iter()
            .filter(|task| task.is_accepted())
            .map(derive_accepted_run_memory_candidate)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|candidate| (candidate.id.clone(), candidate))
            .collect::<BTreeMap<_, _>>();

        let mut ledger = self
            .memory_candidates
            .lock()
            .map_err(|error| anyhow::anyhow!("memory candidate ledger lock poisoned: {error}"))?;
        let mut log = self
            .memory_log
            .lock()
            .map_err(|error| anyhow::anyhow!("promoted memory log lock poisoned: {error}"))?;

        if let Some(recorded) = ledger.processed_decisions.get(&input.request_id) {
            require_idempotent_decision(recorded, &input)?;
            let candidate = ledger
                .candidates
                .get(&recorded.candidate_id)
                .cloned()
                .ok_or_else(|| {
                    MemoryCandidateDecisionError::NotFound(recorded.candidate_id.clone())
                })?;
            let record = recorded
                .record_id
                .as_ref()
                .and_then(|record_id| log.committed_record(record_id).cloned());
            let decision = recorded.clone();
            let projection_digest = super::memory_record::projection_digest(&render_projection(
                &log.projected_records(),
            ));
            let retrieval_index_dirty = read_index_marker(self.storage())?
                .map(|marker| marker.is_dirty())
                .unwrap_or(false);
            return Ok(MemoryCandidateDecisionOutcome {
                decision,
                candidate,
                record,
                replayed: true,
                projection_digest,
                retrieval_index_dirty,
            });
        }

        if ledger.revision != input.expected_ledger_revision {
            return Err(MemoryCandidateDecisionError::RevisionConflict {
                expected: input.expected_ledger_revision,
                current: ledger.revision,
            }
            .into());
        }

        let stored = ledger
            .candidates
            .get(&input.candidate_id)
            .cloned()
            .ok_or_else(|| MemoryCandidateDecisionError::NotFound(input.candidate_id.clone()))?;
        if !stored.status.is_pending() {
            return Err(MemoryCandidateDecisionError::AlreadyDecided {
                candidate_id: input.candidate_id,
                status: stored.status.label(),
            }
            .into());
        }
        // Wire-gate discipline: the approval must still cover the thing being
        // approved. A candidate that no longer matches the current
        // deterministic derivation from accepted governed-task truth is
        // superseded, and promoting it would durably record a fact its evidence
        // no longer supports.
        match expected_candidates.get(&input.candidate_id) {
            Some(expected) if matches_ignoring_status(&stored, expected) => {}
            _ => {
                return Err(MemoryCandidateDecisionError::SupersededCandidate {
                    candidate_id: input.candidate_id,
                }
                .into())
            }
        }

        // An interrupted decision left exactly one uncommitted entry: the
        // append landed, the ledger commit did not, so no receipt exists and
        // the replay path above could not see it. Replaying that same request
        // id adopts the entry; any other decision is refused until it does.
        let adopted = match log.uncommitted_tail().first() {
            Some(entry) if entry.record.request_id == input.request_id => {
                Some(entry.record.clone())
            }
            Some(entry) => {
                return Err(MemoryCandidateDecisionError::InterruptedDecision {
                    request_id: entry.record.request_id.clone(),
                }
                .into())
            }
            None => None,
        };

        let based_on_ledger_revision = ledger.revision;
        let next_revision = based_on_ledger_revision
            .checked_add(1)
            .context("memory candidate ledger revision exhausted u64")?;

        let mut updated = stored.clone();
        let mut record: Option<MemoryRecord> = None;
        let mut head = ledger.memory_log_head.clone();
        let promoting = input.decision.is_promote();
        // An adopted entry keeps its original `valid_from`, and the decision
        // records the same instant: the decision happened when the append did,
        // not when the replay arrived.
        let mut decided_at = now.to_string();

        match &input.decision {
            MemoryCandidateDecisionKind::Promote => {
                let derived = derive_promoted_record(
                    &stored,
                    &input.request_id,
                    based_on_ledger_revision,
                    now,
                )?;
                let promoted = match adopted {
                    Some(tail) => {
                        // The identity digest excludes `valid_from`, so a
                        // genuine replay derives exactly the tail's id. A
                        // different payload under the same request id does not.
                        if tail.id != derived.id {
                            return Err(MemoryCandidateDecisionError::IdempotencyPayloadConflict {
                                request_id: input.request_id,
                            }
                            .into());
                        }
                        decided_at = tail.valid_from.clone();
                        tail
                    }
                    None => {
                        log.append(self.storage(), derived.clone())?;
                        derived
                    }
                };
                head = log.full_head();
                updated.status = MemoryCandidateStatus::Promoted {
                    record_id: promoted.id.clone(),
                    decided_at: decided_at.clone(),
                    decided_by: input.actor.clone(),
                };
                record = Some(promoted);
            }
            MemoryCandidateDecisionKind::Dismiss { reason } => {
                // Only a promotion ever appends, so a dismissal that matches an
                // uncommitted tail's request id is the same id carrying a
                // different payload.
                if adopted.is_some() {
                    return Err(MemoryCandidateDecisionError::IdempotencyPayloadConflict {
                        request_id: input.request_id,
                    }
                    .into());
                }
                updated.status = MemoryCandidateStatus::Dismissed {
                    reason: reason.clone(),
                    decided_at: decided_at.clone(),
                    decided_by: input.actor.clone(),
                };
            }
        }
        updated
            .validate_shape()
            .context("Decided memory candidate failed its own contract")?;

        let decision = MemoryCandidateDecision {
            request_id: input.request_id.clone(),
            candidate_id: input.candidate_id.clone(),
            decision: input.decision.clone(),
            actor: input.actor.clone(),
            authentication,
            decided_at,
            based_on_ledger_revision,
            resulting_ledger_revision: next_revision,
            record_id: record.as_ref().map(|record| record.id.clone()),
        };

        let mut candidate_ledger = ledger.clone();
        candidate_ledger.schema_version = MEMORY_CANDIDATES_LEDGER_SCHEMA_VERSION;
        candidate_ledger.revision = next_revision;
        candidate_ledger
            .candidates
            .insert(input.candidate_id.clone(), updated.clone());
        candidate_ledger.memory_log_head = head;
        candidate_ledger
            .processed_decisions
            .insert(input.request_id.clone(), decision.clone());
        self.storage()
            .write_private_json(MEMORY_CANDIDATES_FILE, &candidate_ledger)
            .context("Failed to persist memory candidate decision")?;
        *ledger = candidate_ledger;
        log.commit_tail();

        // Only a promotion changes the projection, so only a promotion can
        // leave the retrieval index dirty. ADR-0013 rule 9 — a pending (or
        // dismissed) candidate is never indexed — is held by construction.
        let (projection_digest, retrieval_index_dirty) =
            write_projection(self.storage(), &log.projected_records(), now, promoting)?;

        Ok(MemoryCandidateDecisionOutcome {
            decision,
            candidate: updated,
            record,
            replayed: false,
            projection_digest,
            retrieval_index_dirty,
        })
    }

    /// Read the rendered GENOME projection, or `None` when nothing has been
    /// promoted yet. This is the surface a runtime memory tool reads; the raw
    /// candidate ledger is never exposed to one.
    pub fn read_genome_projection(&self) -> Result<Option<String>> {
        let path = self
            .storage()
            .path(super::memory_record::GENOME_PROJECTION_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(
            std::fs::read_to_string(&path).context("Failed to read the GENOME projection")?,
        ))
    }

    /// Reindex the promoted memory records into the real retrieval index and
    /// stamp the dirty marker clean.
    ///
    /// Only projected records are passed to the indexer, so a pending or
    /// dismissed candidate can never enter the index (ADR-0013 rule 9). Returns
    /// how many records were indexed.
    pub fn index_promoted_memory_records(&self, now: &str) -> Result<usize> {
        let records = self.list_promoted_memory_records()?;
        let indexed = crate::retrieval::index_promoted_memory(self.storage().base_path(), &records)
            .context("Failed to index promoted memory records")?;
        let borrowed = records.iter().collect::<Vec<_>>();
        let digest = super::memory_record::projection_digest(&render_projection(&borrowed));
        super::memory_record::stamp_indexed(self.storage(), &digest, now)?;
        Ok(indexed)
    }

    /// Whether the retrieval index is behind the current projection.
    pub fn memory_retrieval_index_is_dirty(&self) -> Result<bool> {
        Ok(read_index_marker(self.storage())?
            .map(|marker| marker.is_dirty())
            .unwrap_or(false))
    }

    /// Every committed promoted record, in log order.
    pub fn list_promoted_memory_records(&self) -> Result<Vec<MemoryRecord>> {
        let log = self
            .memory_log
            .lock()
            .map_err(|error| anyhow::anyhow!("promoted memory log lock poisoned: {error}"))?;
        Ok(log
            .projected_records()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>())
    }
}

/// Compare two candidates on everything except the operator-owned review
/// status. Derivation produces `PendingReview`; a decided candidate still has
/// to match governed-task truth in every derived field.
fn matches_ignoring_status(
    stored: &AcceptedRunMemoryCandidate,
    expected: &AcceptedRunMemoryCandidate,
) -> bool {
    let mut normalized = stored.clone();
    normalized.status = expected.status.clone();
    &normalized == expected
}

/// A replayed request id must carry the same payload it did the first time.
fn require_idempotent_decision(
    recorded: &MemoryCandidateDecision,
    input: &MemoryCandidateDecisionInput,
) -> Result<()> {
    if recorded.candidate_id != input.candidate_id
        || recorded.decision != input.decision
        || recorded.actor != input.actor
        || recorded.based_on_ledger_revision != input.expected_ledger_revision
    {
        return Err(MemoryCandidateDecisionError::IdempotencyPayloadConflict {
            request_id: input.request_id.clone(),
        }
        .into());
    }
    Ok(())
}

#[derive(Serialize)]
struct CandidateSourceV1<'a> {
    derivation_version: u32,
    project_id: &'a str,
    workspace_root: &'a str,
    governed_task_id: &'a impulse_ops::governed_task::GovernedTaskId,
    accepted_task_revision: u64,
    task: &'a str,
    acceptance_criteria: &'a [String],
    runtime_id: &'a str,
    agent_id: &'a str,
    session_id: &'a Option<String>,
    verification_profile: Option<impulse_ops::governed_task::GovernedVerificationProfile>,
    verification_policy: &'a str,
    subject_revision: &'a str,
    claim_id: &'a impulse_ops::governed_task::GovernedRecordId,
    verification_id: &'a impulse_ops::governed_task::GovernedRecordId,
    supervisor_verdict_id: &'a impulse_ops::governed_task::GovernedRecordId,
    operator_decision_id: &'a impulse_ops::governed_task::GovernedRecordId,
    claimed_artifact_ids: &'a [String],
    verification_artifact_ids: &'a [String],
    commands: &'a [AcceptedRunCommandEvidence],
    source_assurance: AcceptedRunSourceAssurance,
    staged_at: &'a str,
}

pub(super) fn derive_accepted_run_memory_candidate(
    task: &GovernedTaskRun,
) -> Result<AcceptedRunMemoryCandidate> {
    if task.review_state != GovernedReviewState::Accepted {
        anyhow::bail!(
            "governed task `{}` is not eligible for a memory candidate",
            task.id
        );
    }
    let claim = task
        .latest_claim()
        .context("accepted governed task has no current worker claim")?;
    let verification = task
        .latest_verification()
        .context("accepted governed task has no current verification")?;
    let supervisor = task
        .latest_supervisor_verdict()
        .context("accepted governed task has no current Supervisor verdict")?;
    let operator = task
        .operator_decisions
        .last()
        .context("accepted governed task has no current operator decision")?;
    // The projection is pinned to the revision the acceptance landed on, not to
    // the task's current revision. ADR-0019 records promotion facts after an
    // acceptance, and a post-accept revision must not re-derive the candidate
    // under a different identity. For every record written before ADR-0019 the
    // two are the same number, so no stored digest changes.
    let accepted_task_revision = operator
        .based_on_revision
        .checked_add(1)
        .context("accepted governed task operator decision revision exhausted u64")?;
    if verification.outcome != GovernedVerificationOutcome::Passed
        || verification.claim_id != claim.id
        || verification.subject_revision != claim.subject_revision
        || supervisor.verdict != SupervisorVerdictKind::RecommendAccept
        || supervisor.verification_id != verification.id
        || operator.decision != OperatorDecisionKind::Approve
        || operator.supervisor_verdict_id != supervisor.id
        || accepted_task_revision > task.revision
    {
        anyhow::bail!(
            "accepted governed task `{}` has an incoherent candidate evidence chain",
            task.id
        );
    }

    let commands = verification
        .commands
        .iter()
        .map(|command| AcceptedRunCommandEvidence {
            name: command.name.clone(),
            command_digest: command.command_digest.clone(),
            output_digest: command.output_digest.clone(),
            exit_code: command.exit_code,
            success: command.success,
            output_bytes: command.output_bytes,
            output_truncated: command.output_truncated,
        })
        .collect::<Vec<_>>();
    // Assurance is the weaker half of the chain: caller-composed evidence
    // never claims an authenticated operator, however the approval arrived.
    let source_assurance = match (
        task.verification_profile.is_some(),
        operator.authentication.is_capability_authenticated(),
    ) {
        (true, true) => AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator,
        (true, false) => AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator,
        (false, _) => AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator,
    };
    let source = CandidateSourceV1 {
        derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
        project_id: &task.project_id,
        workspace_root: &task.workspace_root,
        governed_task_id: &task.id,
        accepted_task_revision,
        task: &task.task,
        acceptance_criteria: &task.acceptance_criteria,
        runtime_id: &task.runtime_id,
        agent_id: &task.agent_id,
        session_id: &task.session_id,
        verification_profile: task.verification_profile,
        verification_policy: &verification.policy,
        subject_revision: &verification.subject_revision,
        claim_id: &claim.id,
        verification_id: &verification.id,
        supervisor_verdict_id: &supervisor.id,
        operator_decision_id: &operator.id,
        claimed_artifact_ids: &claim.artifact_ids,
        verification_artifact_ids: &verification.artifact_ids,
        commands: &commands,
        source_assurance,
        staged_at: &operator.decided_at,
    };
    let source_bytes = serde_json::to_vec(&source)
        .context("Failed to serialize accepted-run memory candidate source")?;
    let source_hex = format!("{:x}", Sha256::digest(source_bytes));
    let source_digest = format!("sha256-v1:{source_hex}");
    let candidate_id = MemoryCandidateId::try_new(format!("memory-candidate-{source_hex}"))?;
    let proposed_summary = match source_assurance {
        AcceptedRunSourceAssurance::DaemonProfiledEvidenceAuthenticatedOperator => format!(
            "Accepted governed outcome for task: {}. Daemon-profiled evidence passed and an authenticated operator approved it; pending semantic-memory review.",
            task.task
        ),
        AcceptedRunSourceAssurance::DaemonProfiledEvidenceDeclaredOperator => format!(
            "Accepted governed outcome for task: {}. Daemon-profiled evidence passed; pending semantic-memory review.",
            task.task
        ),
        AcceptedRunSourceAssurance::CallerComposedEvidenceDeclaredOperator => format!(
            "Accepted governed outcome for task: {}. Caller-composed evidence passed; pending semantic-memory review.",
            task.task
        ),
    };
    let candidate = AcceptedRunMemoryCandidate {
        id: candidate_id,
        schema_version: ACCEPTED_RUN_MEMORY_CANDIDATE_SCHEMA_VERSION,
        derivation_version: ACCEPTED_RUN_MEMORY_DERIVATION_VERSION,
        status: MemoryCandidateStatus::PendingReview,
        project_id: task.project_id.clone(),
        workspace_root: task.workspace_root.clone(),
        governed_task_id: task.id.clone(),
        accepted_task_revision,
        task: task.task.clone(),
        acceptance_criteria: task.acceptance_criteria.clone(),
        proposed_summary,
        runtime_id: task.runtime_id.clone(),
        agent_id: task.agent_id.clone(),
        session_id: task.session_id.clone(),
        verification_profile: task.verification_profile,
        verification_policy: verification.policy.clone(),
        subject_revision: verification.subject_revision.clone(),
        claim_id: claim.id.clone(),
        verification_id: verification.id.clone(),
        supervisor_verdict_id: supervisor.id.clone(),
        operator_decision_id: operator.id.clone(),
        claimed_artifact_ids: claim.artifact_ids.clone(),
        verification_artifact_ids: verification.artifact_ids.clone(),
        commands,
        source_assurance,
        source_digest,
        staged_at: operator.decided_at.clone(),
    };
    candidate.validate_shape()?;
    Ok(candidate)
}

#[cfg(test)]
pub(super) fn memory_candidates_file() -> &'static str {
    MEMORY_CANDIDATES_FILE
}

#[cfg(test)]
pub(in crate::state) mod tests {
    use super::*;
    use impulse_ops::governed_task::{GovernedActor, GovernedActorKind};
    use impulse_ops::memory_candidate::MemoryRecordId;

    use crate::state::governed_task::tests::{accept_run, state};

    pub(in crate::state) fn sample_candidate() -> AcceptedRunMemoryCandidate {
        let (_root, state) = state();
        let task = accept_run(&state, "sample");
        derive_accepted_run_memory_candidate(&task).expect("accepted run derives a candidate")
    }

    fn operator() -> GovernedActor {
        GovernedActor {
            kind: GovernedActorKind::Operator,
            id: "james".to_string(),
        }
    }

    fn promote(
        candidate_id: &MemoryCandidateId,
        request_id: &str,
        revision: u64,
    ) -> MemoryCandidateDecisionInput {
        MemoryCandidateDecisionInput {
            request_id: GovernedRequestId::try_new(request_id).unwrap(),
            project_id: String::new(),
            candidate_id: candidate_id.clone(),
            decision: MemoryCandidateDecisionKind::Promote,
            actor: operator(),
            expected_ledger_revision: revision,
        }
    }

    fn dismiss(
        candidate_id: &MemoryCandidateId,
        request_id: &str,
        revision: u64,
        reason: &str,
    ) -> MemoryCandidateDecisionInput {
        MemoryCandidateDecisionInput {
            request_id: GovernedRequestId::try_new(request_id).unwrap(),
            project_id: String::new(),
            candidate_id: candidate_id.clone(),
            decision: MemoryCandidateDecisionKind::Dismiss {
                reason: reason.to_string(),
            },
            actor: operator(),
            expected_ledger_revision: revision,
        }
    }

    /// Build a state holding exactly one accepted run and its pending candidate.
    fn state_with_candidate() -> (tempfile::TempDir, std::sync::Arc<State>, MemoryCandidateId) {
        let (root, state) = state();
        accept_run(&state, "one");
        let candidates = state
            .list_accepted_run_memory_candidates(&state.governed_project_id())
            .unwrap();
        assert_eq!(candidates.len(), 1);
        let id = candidates[0].id.clone();
        (root, state, id)
    }

    fn with_project(
        mut input: MemoryCandidateDecisionInput,
        state: &State,
    ) -> MemoryCandidateDecisionInput {
        input.project_id = state.governed_project_id();
        input
    }

    #[test]
    fn test_decide_memory_candidate_promote_writes_record_projection_and_dirty_index() {
        let (_root, state, candidate_id) = state_with_candidate();
        let outcome = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::CapabilityAuthenticated,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();

        assert!(!outcome.replayed);
        assert!(outcome.retrieval_index_dirty);
        let record = outcome.record.expect("a promotion produces a record");
        assert_eq!(record.project_id, state.governed_project_id());
        assert_eq!(
            outcome.decision.authentication,
            OperatorAuthentication::CapabilityAuthenticated
        );
        assert_eq!(outcome.decision.resulting_ledger_revision, 1);
        assert!(outcome.candidate.status.is_promoted());

        let projection = state
            .read_genome_projection()
            .unwrap()
            .expect("a promotion renders the projection");
        assert!(projection.contains(record.id.as_str()));
        assert!(projection.contains("do not edit"));
        assert_eq!(state.list_promoted_memory_records().unwrap(), vec![record]);

        // GENOME.md is a separate, hand-curated artifact and stays untouched.
        assert!(!state.storage().path("GENOME.md").exists());
    }

    #[test]
    fn test_decide_memory_candidate_replay_is_idempotent() {
        let (_root, state, candidate_id) = state_with_candidate();
        let first = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let log_after_first = std::fs::read(state.storage().path("MEMORY.jsonl")).unwrap();

        let replay = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T11:00:00Z",
            )
            .unwrap();

        assert!(replay.replayed);
        assert_eq!(replay.decision, first.decision);
        assert_eq!(replay.record, first.record);
        assert_eq!(replay.projection_digest, first.projection_digest);
        assert_eq!(
            std::fs::read(state.storage().path("MEMORY.jsonl")).unwrap(),
            log_after_first,
            "a replay must not append a second entry"
        );
    }

    #[test]
    fn test_decide_memory_candidate_replay_with_a_different_payload_is_refused() {
        let (_root, state, candidate_id) = state_with_candidate();
        state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let error = state
            .decide_memory_candidate(
                with_project(
                    dismiss(&candidate_id, "decide-1", 0, "changed my mind"),
                    &state,
                ),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("replayed with a different payload"));
    }

    #[test]
    fn test_decide_memory_candidate_dismiss_requires_a_reason_and_never_dirties_the_index() {
        let (_root, state, candidate_id) = state_with_candidate();
        let error = state
            .decide_memory_candidate(
                with_project(dismiss(&candidate_id, "decide-1", 0, "   "), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("decision.reason"));

        let outcome = state
            .decide_memory_candidate(
                with_project(
                    dismiss(
                        &candidate_id,
                        "decide-2",
                        0,
                        "duplicates an existing record",
                    ),
                    &state,
                ),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        assert!(outcome.record.is_none());
        assert!(
            !outcome.retrieval_index_dirty,
            "a dismissal changes no promoted record, so it must not dirty the index"
        );
        assert!(!state.storage().path("MEMORY.jsonl").exists());
        assert!(state.read_genome_projection().unwrap().is_some());
        assert!(state.list_promoted_memory_records().unwrap().is_empty());
    }

    #[test]
    fn test_decide_memory_candidate_second_decision_on_a_decided_candidate_is_refused() {
        let (_root, state, candidate_id) = state_with_candidate();
        state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let error = state
            .decide_memory_candidate(
                with_project(dismiss(&candidate_id, "decide-2", 1, "too late"), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("already promoted"));
    }

    #[test]
    fn test_decide_memory_candidate_revision_conflict_is_refused() {
        let (_root, state, candidate_id) = state_with_candidate();
        let error = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 7), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("revision conflict"));
    }

    #[test]
    fn test_decide_memory_candidate_unknown_candidate_and_wrong_project_are_refused() {
        let (_root, state, _candidate_id) = state_with_candidate();
        let missing =
            MemoryCandidateId::try_new(format!("memory-candidate-{}", "0".repeat(64))).unwrap();
        let error = state
            .decide_memory_candidate(
                with_project(promote(&missing, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("was not found"));

        let mut wrong_project = promote(&missing, "decide-2", 0);
        wrong_project.project_id = "some-other-project".to_string();
        let error = state
            .decide_memory_candidate(
                wrong_project,
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("belong to project"));
    }

    #[test]
    fn test_decide_memory_candidate_superseded_candidate_is_refused() {
        let (_root, state, candidate_id) = state_with_candidate();
        // Rewrite the persisted candidate so it is still self-consistent and
        // still keyed by its original id, but no longer equals the current
        // deterministic derivation from accepted governed-task truth.
        let path = state.storage().path(MEMORY_CANDIDATES_FILE);
        let mut ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        ledger["candidates"][candidate_id.as_str()]["task"] =
            serde_json::json!("a task nobody accepted");
        std::fs::write(&path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();

        // Reloading fails closed on the mismatch, which is ADR-0013 rule 8; the
        // decision-time guard is the second, independent check on the same
        // invariant, so drive it directly against the in-memory ledger.
        assert!(State::new(state.storage().base_path().to_path_buf()).is_err());

        let stored = state
            .list_accepted_run_memory_candidates(&state.governed_project_id())
            .unwrap()
            .remove(0);
        let mut superseded = stored.clone();
        superseded.task = "a task nobody accepted".to_string();
        assert!(!matches_ignoring_status(&superseded, &stored));
        assert!(matches_ignoring_status(&stored, &stored));
    }

    #[test]
    fn test_matches_ignoring_status_tolerates_only_the_status_field() {
        let candidate = sample_candidate();
        let mut promoted = candidate.clone();
        promoted.status = MemoryCandidateStatus::Promoted {
            record_id: MemoryRecordId::try_new(format!("memory-record-{}", "a".repeat(64)))
                .unwrap(),
            decided_at: "2026-09-12T10:00:00Z".to_string(),
            decided_by: operator(),
        };
        assert!(matches_ignoring_status(&promoted, &candidate));

        let mut altered = promoted;
        altered.proposed_summary.push_str(" tampered");
        assert!(!matches_ignoring_status(&altered, &candidate));
    }

    #[test]
    fn test_status_preserving_migration_carries_a_decision_across_a_derivation_bump() {
        let (_root, state, candidate_id) = state_with_candidate();
        let outcome = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let record_id = outcome.record.as_ref().unwrap().id.clone();
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        // Simulate the v1 -> v2 derivation bump the ADR-0018 follow-up warns
        // about: the persisted candidate sits at the previous derivation
        // version, carrying a Promoted status, and must not be reverted to
        // pending or lose its record.
        let path = base.join(MEMORY_CANDIDATES_FILE);
        let mut ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        ledger["candidates"][candidate_id.as_str()]["derivation_version"] =
            serde_json::json!(ACCEPTED_RUN_MEMORY_DERIVATION_VERSION - 1);
        std::fs::write(&path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();

        let reloaded = State::new(base).expect("a superseded derivation migrates, never fails");
        let candidates = reloaded
            .list_accepted_run_memory_candidates(&reloaded.governed_project_id())
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].status.promoted_record_id(),
            Some(&record_id),
            "the review decision must survive re-derivation"
        );
        assert_eq!(
            candidates[0].derivation_version,
            ACCEPTED_RUN_MEMORY_DERIVATION_VERSION
        );
        assert_eq!(
            reloaded.list_promoted_memory_records().unwrap().len(),
            1,
            "the promoted record must not be orphaned by the migration"
        );
    }

    #[test]
    fn test_reload_after_a_promotion_verifies_the_log_and_keeps_the_projection() {
        let (_root, state, candidate_id) = state_with_candidate();
        let outcome = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let projection = state.read_genome_projection().unwrap().unwrap();
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        let reloaded = State::new(base).unwrap();
        assert_eq!(
            reloaded.list_promoted_memory_records().unwrap(),
            vec![outcome.record.unwrap()]
        );
        assert_eq!(
            reloaded.read_genome_projection().unwrap().unwrap(),
            projection
        );
    }

    #[test]
    fn test_reload_with_a_tampered_memory_log_fails_closed() {
        let (_root, state, candidate_id) = state_with_candidate();
        state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        let log_path = base.join("MEMORY.jsonl");
        let raw = std::fs::read_to_string(&log_path).unwrap();
        std::fs::write(
            &log_path,
            raw.replace("Accepted governed outcome", "Fabricated outcome"),
        )
        .unwrap();

        let error = State::new(base).unwrap_err();
        assert!(
            format!("{error:#}").contains("modified outside Impulse"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_reload_with_a_truncated_memory_log_fails_closed() {
        let (_root, state, candidate_id) = state_with_candidate();
        state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        std::fs::write(base.join("MEMORY.jsonl"), "").unwrap();
        let error = State::new(base).unwrap_err();
        assert!(
            format!("{error:#}").contains("truncated"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn test_decision_error_display_covers_every_variant() {
        let candidate_id =
            MemoryCandidateId::try_new(format!("memory-candidate-{}", "a".repeat(64))).unwrap();
        let request_id = GovernedRequestId::try_new("request-a").unwrap();
        let rendered = [
            MemoryCandidateDecisionError::NotFound(candidate_id.clone()).to_string(),
            MemoryCandidateDecisionError::RevisionConflict {
                expected: 1,
                current: 2,
            }
            .to_string(),
            MemoryCandidateDecisionError::IdempotencyPayloadConflict {
                request_id: request_id.clone(),
            }
            .to_string(),
            MemoryCandidateDecisionError::AlreadyDecided {
                candidate_id: candidate_id.clone(),
                status: "promoted",
            }
            .to_string(),
            MemoryCandidateDecisionError::SupersededCandidate {
                candidate_id: candidate_id.clone(),
            }
            .to_string(),
            MemoryCandidateDecisionError::InterruptedDecision { request_id }.to_string(),
            MemoryCandidateDecisionError::ProjectMismatch {
                expected: "a".to_string(),
                actual: "b".to_string(),
            }
            .to_string(),
        ];
        for (message, expected) in rendered.iter().zip([
            "was not found",
            "revision conflict",
            "replayed with a different payload",
            "already promoted",
            "superseded",
            "interrupted",
            "belong to project",
        ]) {
            assert!(
                message.contains(expected),
                "`{message}` should mention `{expected}`"
            );
        }
    }

    #[test]
    fn test_pending_candidate_is_never_indexed_but_a_promoted_record_is() {
        // Runs against the real SQLite retrieval index inside the state's own
        // temp IMPULSE_HOME — not a stub.
        let (_root, state, candidate_id) = state_with_candidate();
        let base = state.storage().base_path().to_path_buf();
        let query = "governed";

        // Pending: nothing indexed, nothing searchable. ADR-0013 rule 9.
        assert_eq!(
            state
                .index_promoted_memory_records("2026-09-12T10:00:00Z")
                .unwrap(),
            0
        );
        assert!(crate::retrieval::search_promoted_memory(&base, query, 10)
            .unwrap()
            .is_empty());
        assert!(!state.memory_retrieval_index_is_dirty().unwrap());

        let outcome = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let record_id = outcome.record.unwrap().id;
        assert!(
            state.memory_retrieval_index_is_dirty().unwrap(),
            "a promotion must leave the index needing a reindex"
        );

        assert_eq!(
            state
                .index_promoted_memory_records("2026-09-12T10:05:00Z")
                .unwrap(),
            1
        );
        assert!(!state.memory_retrieval_index_is_dirty().unwrap());
        let hits = crate::retrieval::search_promoted_memory(&base, query, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, record_id.as_str());
        assert_eq!(hits[0].source, "memory");
    }

    #[test]
    fn test_dismissed_candidate_never_reaches_the_retrieval_index() {
        let (_root, state, candidate_id) = state_with_candidate();
        let base = state.storage().base_path().to_path_buf();
        state
            .decide_memory_candidate(
                with_project(
                    dismiss(
                        &candidate_id,
                        "decide-1",
                        0,
                        "duplicates an existing record",
                    ),
                    &state,
                ),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        assert!(!state.memory_retrieval_index_is_dirty().unwrap());
        assert_eq!(
            state
                .index_promoted_memory_records("2026-09-12T10:05:00Z")
                .unwrap(),
            0
        );
        assert!(
            crate::retrieval::search_promoted_memory(&base, "governed", 10)
                .unwrap()
                .is_empty()
        );
    }

    /// Reproduce the crash window: the append lands, the ledger commit does
    /// not. The ledger file is rolled back to its pre-decision bytes, which is
    /// exactly what a process killed between the two side effects leaves.
    fn crash_between_append_and_commit(
        state: &State,
        candidate_id: &MemoryCandidateId,
        request_id: &str,
    ) -> std::path::PathBuf {
        let base = state.storage().base_path().to_path_buf();
        let ledger_path = state.storage().path(MEMORY_CANDIDATES_FILE);
        let before = std::fs::read(&ledger_path).unwrap();
        state
            .decide_memory_candidate(
                with_project(promote(candidate_id, request_id, 0), state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        std::fs::write(&ledger_path, before).unwrap();
        base
    }

    #[test]
    fn test_interrupted_decision_is_finished_by_replaying_the_same_request_id() {
        let (_root, state, candidate_id) = state_with_candidate();
        let base = crash_between_append_and_commit(&state, &candidate_id, "decide-1");
        let log_after_crash = std::fs::read(base.join("MEMORY.jsonl")).unwrap();
        drop(state);

        // The log's one entry is uncommitted: the daemon starts, warns, and the
        // projection does not yet carry the record.
        let reloaded = State::new(base.clone()).unwrap();
        assert!(reloaded.list_promoted_memory_records().unwrap().is_empty());
        let recovered_candidate = reloaded
            .list_accepted_run_memory_candidates(&reloaded.governed_project_id())
            .unwrap()
            .remove(0);
        assert!(recovered_candidate.status.is_pending());

        // Replaying the same request id adopts the entry instead of appending
        // a second one, and commits the ledger.
        let outcome = reloaded
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &reloaded),
                OperatorAuthentication::Declared,
                "2026-09-12T12:00:00Z",
            )
            .unwrap();
        assert!(!outcome.replayed, "this is the original decision finishing");
        let record = outcome.record.expect("adoption yields the logged record");
        assert_eq!(
            std::fs::read(base.join("MEMORY.jsonl")).unwrap(),
            log_after_crash,
            "adoption must not append a second entry"
        );
        assert_eq!(
            record.valid_from, "2026-09-12T10:00:00Z",
            "the adopted record keeps the instant its append happened"
        );
        assert_eq!(outcome.decision.decided_at, record.valid_from);
        assert_eq!(
            outcome.candidate.status.promoted_record_id(),
            Some(&record.id)
        );
        assert_eq!(
            reloaded.list_promoted_memory_records().unwrap(),
            vec![record]
        );

        // And the recovered state reloads clean.
        drop(reloaded);
        assert_eq!(
            State::new(base)
                .unwrap()
                .list_promoted_memory_records()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn test_interrupted_decision_blocks_a_different_request_until_it_is_replayed() {
        let (_root, state, candidate_id) = state_with_candidate();
        let base = crash_between_append_and_commit(&state, &candidate_id, "decide-1");
        drop(state);
        let reloaded = State::new(base).unwrap();

        let error = reloaded
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-2", 0), &reloaded),
                OperatorAuthentication::Declared,
                "2026-09-12T12:00:00Z",
            )
            .unwrap_err();
        let rendered = format!("{error:#}");
        assert!(rendered.contains("interrupted"));
        assert!(
            rendered.contains("decide-1"),
            "the error must name the request to replay: {rendered}"
        );
    }

    #[test]
    fn test_interrupted_decision_refuses_the_same_request_id_with_a_different_payload() {
        let (_root, state, candidate_id) = state_with_candidate();
        let base = crash_between_append_and_commit(&state, &candidate_id, "decide-1");
        drop(state);
        let reloaded = State::new(base).unwrap();

        // Same request id, now a dismissal: only a promotion ever appends, so
        // this is a payload conflict, not an adoption.
        let error = reloaded
            .decide_memory_candidate(
                with_project(
                    dismiss(&candidate_id, "decide-1", 0, "changed my mind"),
                    &reloaded,
                ),
                OperatorAuthentication::Declared,
                "2026-09-12T12:00:00Z",
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("replayed with a different payload"));
    }

    #[test]
    fn test_startup_keeps_a_parked_decision_whose_accepted_task_is_gone() {
        let (_root, state, candidate_id) = state_with_candidate();
        let outcome = state
            .decide_memory_candidate(
                with_project(promote(&candidate_id, "decide-1", 0), &state),
                OperatorAuthentication::Declared,
                "2026-09-12T10:00:00Z",
            )
            .unwrap();
        let record_id = outcome.record.unwrap().id;
        let base = state.storage().base_path().to_path_buf();
        drop(state);

        // Age the candidate's derivation and remove the governed task ledger,
        // so the parked decision has nothing to reattach to.
        let path = base.join(MEMORY_CANDIDATES_FILE);
        let mut ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        ledger["candidates"][candidate_id.as_str()]["derivation_version"] =
            serde_json::json!(ACCEPTED_RUN_MEMORY_DERIVATION_VERSION - 1);
        std::fs::write(&path, serde_json::to_vec_pretty(&ledger).unwrap()).unwrap();
        std::fs::remove_file(base.join("GOVERNED_TASKS.json")).unwrap();

        let error = State::new(base).unwrap_err();
        let rendered = format!("{error:#}");
        assert!(
            rendered.contains(record_id.as_str()),
            "the orphan error must name the record: {rendered}"
        );
        assert!(
            rendered.contains("no longer an accepted task"),
            "the orphan error must point at the lost task, not nowhere: {rendered}"
        );
    }
}
