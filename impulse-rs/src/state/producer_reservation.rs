//! Durable producer reservation journal.
//!
//! ADR-0012's consequences section names a gap: persisted receipts and the
//! per-task daemon lock deduplicate replay and concurrent requests, but they
//! do not provide crash-safe exactly-once producer execution. If the daemon
//! exits between a producer side effect (a cargo verification run, a
//! Supervisor model turn) and the durable receipt that records it, a
//! replayed request can repeat that side effect after restart.
//!
//! This journal closes the observability half of that gap. A caller reserves
//! before running the side effect and releases with a receipt reference once
//! the side effect *and* its governed-task mutation are durably recorded. A
//! reservation still open when the process reloads was interrupted before a
//! receipt existed; the daemon reconciles it at startup
//! ([`State::reconcile_producer_reservations`]) to
//! [`ReservationOutcome::NeedsRerun`] and records a note on the governed
//! task's own event chain so the operator surface can show it. Other
//! processes load the journal without repairing it, because an open
//! reservation there may belong to a daemon that is still running.
//! `RunGovernedVerification`, `RunGovernedSupervisorReview`, and staged
//! promotion call [`with_reservation`] around the side effect *and* the
//! mutation persist step.
//!
//! The journal is deliberately independent of `GOVERNED_TASKS.json`: a
//! corrupted or truncated journal fails closed at load without blocking
//! unrelated governed-task state, matching the private-ledger pattern in
//! `state/memory_candidate.rs`. The digest is an unkeyed SHA-256 — it
//! detects accidental corruption and torn/partial writes, not a deliberate
//! adversary with filesystem write access (who could simply recompute it);
//! it is a data-integrity check, not a tamper-resistance boundary.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use impulse_ops::governed_task::{
    GovernedActor, GovernedActorKind, GovernedRecordId, GovernedRequestId, GovernedTaskId,
    GovernedTaskMutation, GovernedTaskMutationRequest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::State;
use crate::storage::Storage;

const PRODUCER_RESERVATIONS_FILE: &str = "PRODUCER_RESERVATIONS.json";
/// The newest journal schema this build writes. Version 2 adds the explicit
/// [`ProducerEffectOutcome`] on each reservation (blueprint 2.1, "lost
/// response"). It is written only once a reservation actually carries one, so
/// an untouched version-1 journal stays readable by older builds; a build that
/// predates version 2 refuses it by version instead of by a confusing digest
/// mismatch over a field it does not know.
const PRODUCER_RESERVATIONS_SCHEMA_VERSION: u32 = 2;
/// The pre-effect-outcome schema. Loaded as-is; each reservation's outcome is
/// derived from its [`ReservationOutcome`] (see
/// [`ProducerReservation::effect_outcome`]).
const PRODUCER_RESERVATIONS_LEGACY_SCHEMA_VERSION: u32 = 1;

/// A reservation's identity. An alias over the existing governed-record id
/// type rather than a new newtype: reservations are governed-task-adjacent
/// records and the id already carries the validation (nonempty, no
/// whitespace/control characters, bounded length) and `Ord`/`Hash` this
/// journal needs for a `BTreeMap` key.
pub type ReservationId = GovernedRecordId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProducerReservationError {
    #[error(
        "producer reservation ledger digest mismatch: the journal may be corrupted or truncated"
    )]
    LedgerDigestMismatch,
    #[error("unsupported producer reservation ledger schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("producer reservation ledger key `{key}` does not match record id `{id}`")]
    KeyIdMismatch { key: String, id: String },
    #[error(
        "an open producer reservation already exists for task `{task_id}` producer {producer:?}"
    )]
    DuplicateOpenReservation {
        task_id: GovernedTaskId,
        producer: ProducerKind,
    },
    #[error("producer reservation `{0}` was not found")]
    NotFound(ReservationId),
    #[error("producer reservation `{0}` is already released")]
    AlreadyReleased(ReservationId),
}

/// The closed set of daemon-owned producers that mutate governed-task state
/// through an external side effect (a command run, a model turn) before
/// persisting a receipt. Mirrors the producer profiles named in ADR-0012;
/// `Promotion` is reserved for ADR-0019's staged-worktree promote producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerKind {
    Verification,
    SupervisorReview,
    Promotion,
}

/// How a reservation was closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ReservationOutcome {
    /// The side effect completed and its governed-task mutation was
    /// durably persisted. `receipt_ref` names the receipt (typically the
    /// governed request id that produced it) for traceability.
    Released { receipt_ref: String },
    /// The reservation was still open when the process reloaded: the side
    /// effect may have run partway, but no receipt exists to prove it
    /// finished and was recorded. A caller must treat the next request for
    /// this task/producer as fresh work, not a confirmed duplicate.
    NeedsRerun { reason: String },
}

/// What is known about a producer's external side effect (a verification run,
/// a Supervisor turn, a promotion), following the blueprint's effect lifecycle
/// *intended -> dispatched -> confirmed / failed-known / unknown*.
///
/// The distinction that matters is `FailedKnown` versus `Unknown`. A lost
/// response — the daemon killed after `cargo` finished but before the receipt
/// was durable, or a receipt write that failed after the side effect ran — is
/// **not** a failure: the effect may well have happened. Recording it as
/// failed would invite an operator (or a retry policy) to treat finished work
/// as never done. Only execution evidence closes an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerEffectOutcome {
    /// The side effect ran and its governed-task receipt is durable.
    Confirmed,
    /// The producer itself reported a definite failure before any receipt was
    /// due. Nothing durable claims the effect happened.
    FailedKnown,
    /// The side effect was dispatched and may have completed, but no durable
    /// receipt proves it either way. Always paired with
    /// [`ReservationOutcome::NeedsRerun`].
    Unknown,
}

/// How a producer's side effect failed, as far as the producer can tell.
///
/// Returned by the closure given to [`with_reservation_effect`]. The wrapped
/// error is handed back to the caller unchanged (same message, same
/// downcastable type); only the journal's record differs.
#[derive(Debug)]
pub enum ProducerSideEffectError {
    /// The producer reported a definite failure before any receipt was due:
    /// recorded as [`ProducerEffectOutcome::FailedKnown`].
    Failed(anyhow::Error),
    /// The side effect was dispatched, then its receipt could not be made
    /// durable (blueprint 2.1 "lost response"): recorded as
    /// [`ProducerEffectOutcome::Unknown`] and closed as `NeedsRerun`, the same
    /// record a crash at that point reconciles to at the next startup.
    ReceiptLost(anyhow::Error),
    /// The producer failed *after* dispatching its side effect, so the effect
    /// may have happened even though no mutation was built (for example a
    /// promotion whose compare-and-swap moved the canonical branch, followed
    /// by a failed working-tree sync or observation). Recorded as
    /// [`ProducerEffectOutcome::Unknown`] and closed as `NeedsRerun`, exactly
    /// like a lost receipt. Producers signal it with
    /// [`EffectDispatchedError::wrap`]; see [`classify_producer_error`].
    Dispatched(anyhow::Error),
}

impl ProducerSideEffectError {
    pub fn into_inner(self) -> anyhow::Error {
        match self {
            Self::Failed(error) | Self::ReceiptLost(error) | Self::Dispatched(error) => error,
        }
    }
}

/// Marks a producer error raised after the producer's external side effect
/// was dispatched, so the reservation journal records the effect as
/// [`ProducerEffectOutcome::Unknown`] instead of a known failure.
///
/// Transparent: its `Display`, `{:#}` chain and `source()` are the wrapped
/// error's, so an operator sees the same message. [`classify_producer_error`]
/// unwraps it again before the error is returned to the caller.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EffectDispatchedError(anyhow::Error);

impl EffectDispatchedError {
    /// Wrap an error raised after the side effect may already have happened.
    /// Wrapping an already-wrapped error is a no-op.
    pub fn wrap(error: anyhow::Error) -> anyhow::Error {
        if error.is::<Self>() {
            return error;
        }
        anyhow::Error::new(Self(error))
    }
}

/// Classify a producer error for the reservation journal: an error marked
/// with [`EffectDispatchedError`] is [`ProducerSideEffectError::Dispatched`]
/// (effect outcome unknown, needs rerun); anything else is a definite
/// [`ProducerSideEffectError::Failed`] raised before the effect was
/// dispatched.
pub fn classify_producer_error(error: anyhow::Error) -> ProducerSideEffectError {
    match error.downcast::<EffectDispatchedError>() {
        Ok(EffectDispatchedError(inner)) => ProducerSideEffectError::Dispatched(inner),
        Err(error) => ProducerSideEffectError::Failed(error),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerReservation {
    pub id: ReservationId,
    pub task_id: GovernedTaskId,
    pub revision: u64,
    pub request_id: GovernedRequestId,
    pub producer: ProducerKind,
    pub reserved_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ReservationOutcome>,
    /// Recorded when the reservation closes. Absent on version-1 records and
    /// on open reservations; read it through [`Self::effect_outcome`], which
    /// derives the value for legacy records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<ProducerEffectOutcome>,
}

impl ProducerReservation {
    fn is_open(&self) -> bool {
        self.released_at.is_none()
    }

    /// The effect outcome, `None` while the reservation is still open.
    ///
    /// Version-1 records carry no explicit value, so it is derived from how
    /// they were closed: `NeedsRerun` is unknown (no receipt proves anything),
    /// a release whose receipt reference is a `failed: …` note is a known
    /// failure, and any other release is confirmed.
    pub fn effect_outcome(&self) -> Option<ProducerEffectOutcome> {
        if let Some(effect) = self.effect {
            return Some(effect);
        }
        match self.outcome.as_ref()? {
            ReservationOutcome::NeedsRerun { .. } => Some(ProducerEffectOutcome::Unknown),
            ReservationOutcome::Released { receipt_ref } if receipt_ref.starts_with("failed: ") => {
                Some(ProducerEffectOutcome::FailedKnown)
            }
            ReservationOutcome::Released { .. } => Some(ProducerEffectOutcome::Confirmed),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ProducerReservationLedger {
    schema_version: u32,
    #[serde(default)]
    reservations: BTreeMap<ReservationId, ProducerReservation>,
    /// SHA-256 over the canonical JSON serialization of `reservations`,
    /// recomputed and checked at every load. This is corruption/truncation
    /// detection, not tamper resistance: it is an unkeyed hash, so anyone
    /// with filesystem write access to this file could edit the content and
    /// recompute a matching digest. Its job is to catch an accidental
    /// partial write or bit rot, not a deliberate adversary.
    #[serde(default)]
    digest: String,
}

impl Default for ProducerReservationLedger {
    fn default() -> Self {
        let reservations = BTreeMap::new();
        // Hashing an empty, already-valid map cannot fail.
        let digest = compute_digest(&reservations).expect("hashing an empty map cannot fail");
        Self {
            schema_version: PRODUCER_RESERVATIONS_LEGACY_SCHEMA_VERSION,
            reservations,
            digest,
        }
    }
}

fn compute_digest(reservations: &BTreeMap<ReservationId, ProducerReservation>) -> Result<String> {
    let canonical =
        serde_json::to_vec(reservations).context("Failed to canonicalize reservations")?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

impl ProducerReservationLedger {
    fn load(storage: &Storage) -> Result<Self> {
        let ledger: Self = storage
            .read_json(PRODUCER_RESERVATIONS_FILE)
            .context("Failed to read producer reservation journal")?;
        ledger.validate_shape()?;
        Ok(ledger)
    }

    fn validate_shape(&self) -> Result<()> {
        if !matches!(
            self.schema_version,
            PRODUCER_RESERVATIONS_LEGACY_SCHEMA_VERSION | PRODUCER_RESERVATIONS_SCHEMA_VERSION
        ) {
            return Err(
                ProducerReservationError::UnsupportedSchemaVersion(self.schema_version).into(),
            );
        }
        if self.schema_version < self.required_schema_version() {
            anyhow::bail!(
                "producer reservation journal declares schema version {} but records effect outcomes, which require version {}",
                self.schema_version,
                PRODUCER_RESERVATIONS_SCHEMA_VERSION
            );
        }
        for (key, reservation) in &self.reservations {
            if key != &reservation.id {
                return Err(ProducerReservationError::KeyIdMismatch {
                    key: key.to_string(),
                    id: reservation.id.to_string(),
                }
                .into());
            }
        }
        let expected_digest = compute_digest(&self.reservations)
            .context("Failed to recompute producer reservation journal digest")?;
        if expected_digest != self.digest {
            return Err(ProducerReservationError::LedgerDigestMismatch.into());
        }
        Ok(())
    }

    /// Version 2 once any reservation records an explicit effect outcome.
    fn required_schema_version(&self) -> u32 {
        if self.reservations.values().any(|r| r.effect.is_some()) {
            PRODUCER_RESERVATIONS_SCHEMA_VERSION
        } else {
            PRODUCER_RESERVATIONS_LEGACY_SCHEMA_VERSION
        }
    }

    fn persist(&mut self, storage: &Storage) -> Result<()> {
        // Never lowers a version already on disk.
        self.schema_version = self.schema_version.max(self.required_schema_version());
        self.digest = compute_digest(&self.reservations)
            .context("Failed to compute producer reservation journal digest")?;
        storage
            .write_private_json(PRODUCER_RESERVATIONS_FILE, &self)
            .context("Failed to persist producer reservation journal")
    }
}

fn lock_err<T: std::fmt::Display>(error: T) -> anyhow::Error {
    anyhow::anyhow!("producer reservation ledger lock poisoned: {error}")
}

fn new_reservation_id() -> ReservationId {
    ReservationId::try_new(format!("producer-reservation-{}", Uuid::new_v4()))
        .expect("generated producer reservation UUID must be valid")
}

impl State {
    pub(super) fn load_producer_reservation_ledger(
        storage: &Storage,
    ) -> Result<std::sync::Mutex<ProducerReservationLedger>> {
        Ok(std::sync::Mutex::new(ProducerReservationLedger::load(
            storage,
        )?))
    }

    /// Reserve a producer slot for `task_id`/`producer` before running its
    /// side effect. Fails if an open reservation *at or after* `revision`
    /// already exists for the same task and producer, so a request replayed
    /// while the original side effect is genuinely still in flight cannot
    /// start a second, competing run. An open reservation strictly *behind*
    /// `revision` cannot be that live attempt — the task has since moved
    /// on — so it is closed as `NeedsRerun` in the same persisted write
    /// rather than left to block forever. See the design spec for the
    /// residual gap this does not close: two attempts at the exact same
    /// revision still conflict by design.
    ///
    /// Mutates the live ledger only after a successful persist (clone,
    /// mutate the clone, persist the clone, then swap it in) so a transient
    /// I/O failure here leaves the durable and in-memory state identical to
    /// before the call — never a phantom reservation the caller has no id
    /// for and can never release.
    pub fn reserve(
        &self,
        task_id: &GovernedTaskId,
        revision: u64,
        request_id: &GovernedRequestId,
        producer: ProducerKind,
    ) -> Result<ReservationId> {
        let mut ledger = self.producer_reservations.lock().map_err(lock_err)?;

        let mut stale_ids = Vec::new();
        for reservation in ledger.reservations.values() {
            if &reservation.task_id != task_id
                || reservation.producer != producer
                || !reservation.is_open()
            {
                continue;
            }
            if reservation.revision < revision {
                stale_ids.push(reservation.id.clone());
            } else {
                return Err(ProducerReservationError::DuplicateOpenReservation {
                    task_id: task_id.clone(),
                    producer,
                }
                .into());
            }
        }

        let mut candidate = ledger.clone();
        let now = impulse_ops::now_rfc3339();
        for stale_id in &stale_ids {
            if let Some(stale) = candidate.reservations.get_mut(stale_id) {
                stale.released_at = Some(now.clone());
                stale.outcome = Some(ReservationOutcome::NeedsRerun {
                    reason: format!(
                        "superseded by a new reservation at revision {revision} \
                         (this one was reserved at revision {})",
                        stale.revision
                    ),
                });
                stale.effect = Some(ProducerEffectOutcome::Unknown);
            }
        }

        let id = new_reservation_id();
        let reservation = ProducerReservation {
            id: id.clone(),
            task_id: task_id.clone(),
            revision,
            request_id: request_id.clone(),
            producer,
            reserved_at: now,
            released_at: None,
            outcome: None,
            effect: None,
        };
        candidate.reservations.insert(id.clone(), reservation);
        candidate.persist(self.storage())?;
        *ledger = candidate;
        Ok(id)
    }

    /// Release a reservation once its side effect *and* the governed-task
    /// mutation that records it are both durably persisted. `receipt_ref`
    /// should name that receipt (for example the governed request id that
    /// produced it) so the journal entry stays traceable.
    ///
    /// Same clone-mutate-persist-swap discipline as `reserve`: a persist
    /// failure here leaves the reservation exactly as open as it was before
    /// the call, never silently (or partially) released.
    pub fn release(&self, id: &ReservationId, receipt_ref: impl Into<String>) -> Result<()> {
        self.close_released(id, receipt_ref.into(), ProducerEffectOutcome::Confirmed)
    }

    /// Release a reservation whose producer reported a definite failure. The
    /// effect is recorded as [`ProducerEffectOutcome::FailedKnown`].
    pub fn release_failed(&self, id: &ReservationId, error: &anyhow::Error) -> Result<()> {
        self.close_released(
            id,
            format!("failed: {error}"),
            ProducerEffectOutcome::FailedKnown,
        )
    }

    fn close_released(
        &self,
        id: &ReservationId,
        receipt_ref: String,
        effect: ProducerEffectOutcome,
    ) -> Result<()> {
        let mut ledger = self.producer_reservations.lock().map_err(lock_err)?;
        let existing = ledger
            .reservations
            .get(id)
            .ok_or_else(|| ProducerReservationError::NotFound(id.clone()))?;
        if !existing.is_open() {
            return Err(ProducerReservationError::AlreadyReleased(id.clone()).into());
        }

        let mut candidate = ledger.clone();
        let reservation = candidate
            .reservations
            .get_mut(id)
            .expect("presence just confirmed against the same ledger");
        reservation.released_at = Some(impulse_ops::now_rfc3339());
        reservation.outcome = Some(ReservationOutcome::Released { receipt_ref });
        reservation.effect = Some(effect);
        candidate.persist(self.storage())?;
        *ledger = candidate;
        Ok(())
    }

    /// All reservations still open (no `released_at`). Non-empty only when
    /// a prior process was interrupted before reconciliation ran, or
    /// (transiently, in-process) while a side effect is running.
    pub fn open_reservations(&self) -> Result<Vec<ProducerReservation>> {
        let ledger = self.producer_reservations.lock().map_err(lock_err)?;
        Ok(ledger
            .reservations
            .values()
            .filter(|r| r.is_open())
            .cloned()
            .collect())
    }

    /// The most recent reservation for this exact task/producer/request triple,
    /// open or closed. Lets the operator surface (and tests) read the recorded
    /// [`ProducerEffectOutcome`] of an attempt.
    pub fn latest_reservation(
        &self,
        task_id: &GovernedTaskId,
        producer: ProducerKind,
        request_id: &GovernedRequestId,
    ) -> Result<Option<ProducerReservation>> {
        let ledger = self.producer_reservations.lock().map_err(lock_err)?;
        Ok(ledger
            .reservations
            .values()
            .filter(|r| {
                &r.task_id == task_id && r.producer == producer && &r.request_id == request_id
            })
            .max_by_key(|r| {
                (
                    chrono::DateTime::parse_from_rfc3339(&r.reserved_at).ok(),
                    r.reserved_at.clone(),
                )
            })
            .cloned())
    }

    /// The reason the most recent reservation for this exact
    /// task/producer/request triple needed a rerun, if any. Lets a caller
    /// distinguish a request that is replaying against a previously
    /// interrupted attempt from one that has never been reserved before.
    pub fn pending_rerun_reason(
        &self,
        task_id: &GovernedTaskId,
        producer: ProducerKind,
        request_id: &GovernedRequestId,
    ) -> Result<Option<String>> {
        let ledger = self.producer_reservations.lock().map_err(lock_err)?;
        let mut matches: Vec<&ProducerReservation> = ledger
            .reservations
            .values()
            .filter(|r| {
                &r.task_id == task_id && r.producer == producer && &r.request_id == request_id
            })
            .collect();
        matches.sort_by(|a, b| a.reserved_at.cmp(&b.reserved_at));
        Ok(matches.into_iter().rev().find_map(|r| match &r.outcome {
            Some(ReservationOutcome::NeedsRerun { reason }) => Some(reason.clone()),
            _ => None,
        }))
    }

    /// Close any reservation left open by a previous process and note the
    /// interruption on the governed task's own event chain. Invoked by the
    /// daemon at startup, once it has found no other daemon listening (every
    /// other process leaves live reservations alone); safe to call again (each note is recorded
    /// through the same idempotent governed-task mutation path keyed by a
    /// deterministic per-reservation request id, so a repeat reconcile does
    /// not duplicate events).
    /// Closes every reservation still open as `NeedsRerun` and notes the
    /// interruption on the governed task's event chain.
    ///
    /// Only the process that owns reservations may call this: the daemon, at
    /// startup, after it has confirmed no other daemon serves the project.
    /// Any other process (a hook's CLI invocation, the TUI) would close a
    /// running daemon's live reservation and write a stale copy of the
    /// governed ledger over the daemon's.
    pub fn reconcile_producer_reservations(&self) -> Result<()> {
        for reservation in self.open_reservations()? {
            let reason = "interrupted before receipt".to_string();
            self.close_reservation_needs_rerun(&reservation.id, reason.clone())?;
            if let Err(error) = self.note_producer_reservation_interrupted(&reservation, &reason) {
                // The reservation is already durably marked needs-rerun in
                // this journal regardless of whether the governed task can
                // be annotated (it may have been deleted, or belong to a
                // different project). Losing the operator-surface note is
                // not a reason to fail daemon startup.
                tracing::warn!(
                    "producer reservation {} marked needs-rerun but could not annotate governed task {}: {error}",
                    reservation.id,
                    reservation.task_id
                );
            }
        }
        Ok(())
    }

    fn close_reservation_needs_rerun(&self, id: &ReservationId, reason: String) -> Result<()> {
        let mut ledger = self.producer_reservations.lock().map_err(lock_err)?;
        let existing = ledger
            .reservations
            .get(id)
            .ok_or_else(|| ProducerReservationError::NotFound(id.clone()))?;
        // Now called on the live path too (a lost receipt), not only by the
        // startup reconcile: never overwrite an outcome already recorded.
        if !existing.is_open() {
            return Err(ProducerReservationError::AlreadyReleased(id.clone()).into());
        }

        let mut candidate = ledger.clone();
        let reservation = candidate
            .reservations
            .get_mut(id)
            .expect("presence just confirmed against the same ledger");
        reservation.released_at = Some(impulse_ops::now_rfc3339());
        reservation.outcome = Some(ReservationOutcome::NeedsRerun { reason });
        // No receipt exists, so nothing proves the side effect did or did not
        // happen: a lost response is unknown, never failed.
        reservation.effect = Some(ProducerEffectOutcome::Unknown);
        candidate.persist(self.storage())?;
        *ledger = candidate;
        Ok(())
    }

    fn note_producer_reservation_interrupted(
        &self,
        reservation: &ProducerReservation,
        reason: &str,
    ) -> Result<()> {
        let project_id = self.governed_project_id();
        let task = self
            .get_governed_task(&project_id, &reservation.task_id)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "governed task `{}` not found for interrupted producer reservation `{}`",
                    reservation.task_id,
                    reservation.id
                )
            })?;
        let actor = GovernedActor {
            kind: GovernedActorKind::System,
            id: "producer-reservation-journal".to_string(),
        };
        let detail = format!(
            "producer reservation {} ({:?}) for request {} needs rerun (effect outcome unknown): {reason}",
            reservation.id, reservation.producer, reservation.request_id
        );
        // Deterministic per-reservation request id: a repeat reconcile (or
        // a reconcile that outlives a crash mid-mutation) replays through
        // the governed task ledger's own idempotency receipt rather than
        // appending a second event.
        let request_id = GovernedRequestId::try_new(format!(
            "producer-reservation-reconcile-{}",
            reservation.id
        ))
        .context("Failed to build deterministic reconcile request id")?;
        let mutation_request = GovernedTaskMutationRequest {
            request_id,
            project_id,
            task_id: reservation.task_id.clone(),
            expected_revision: task.revision,
            mutation: GovernedTaskMutation::NoteProducerReservationInterrupted {
                actor,
                reason: detail,
            },
        };
        self.mutate_governed_task(mutation_request)?;
        Ok(())
    }
}

/// Reserve, run `side_effect`, and release with its receipt reference.
///
/// `side_effect` must perform *both* the external side effect (a command
/// run, a model turn) and persist the governed-task mutation that records
/// it, returning `(value, receipt_ref)` only once that mutation is durable.
/// This ordering is what closes ADR-0012's gap: if the process crashes
/// before the mutation is durable, the reservation is still open at reload
/// and reconciliation marks it needs-rerun, so a legitimate replay reruns
/// the (never-recorded) side effect; if it crashes after the mutation is
/// durable but before `release` runs, the reservation is marked needs-rerun
/// but the governed-task ledger already holds the receipt, so the existing
/// replay check (`require_producer_request_state` and friends) recognizes
/// the request and skips rerunning the side effect. Releasing immediately
/// after the side effect alone — before its mutation is persisted — would
/// reopen the gap this journal exists to close.
///
/// On failure the reservation is closed (never left open) so a corrected retry
/// is not blocked by [`ProducerReservationError::DuplicateOpenReservation`]. A
/// failure is recorded as [`ProducerEffectOutcome::FailedKnown`] unless the
/// producer marked it with [`EffectDispatchedError`] (then
/// [`ProducerEffectOutcome::Unknown`]); producers that can tell a lost receipt
/// apart use [`with_reservation_effect`]. This is a plain
/// async helper, not a method on `State`, so the handler lane can adopt it
/// around `crate::governed_producers::run_verification` and the Supervisor
/// review turn without a circular dependency on `src/daemon`.
pub async fn with_reservation<F, Fut, T>(
    state: &State,
    task_id: &GovernedTaskId,
    revision: u64,
    request_id: &GovernedRequestId,
    producer: ProducerKind,
    side_effect: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(T, String)>>,
{
    with_reservation_effect(
        state,
        task_id,
        revision,
        request_id,
        producer,
        || async move { side_effect().await.map_err(classify_producer_error) },
    )
    .await
}

/// [`with_reservation`] for a producer that can tell a definite failure apart
/// from a lost receipt. See [`ProducerSideEffectError`].
pub async fn with_reservation_effect<F, Fut, T>(
    state: &State,
    task_id: &GovernedTaskId,
    revision: u64,
    request_id: &GovernedRequestId,
    producer: ProducerKind,
    side_effect: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<(T, String), ProducerSideEffectError>>,
{
    let reservation_id = state.reserve(task_id, revision, request_id, producer)?;
    match side_effect().await {
        Ok((value, receipt_ref)) => {
            // The side effect's own governed-task mutation is already
            // durable at this point (that is `side_effect`'s contract).
            // A failure releasing *this* journal entry must not turn a
            // durably-recorded success into a reported failure — log and
            // continue, exactly like the error branches below.
            if let Err(release_error) = state.release(&reservation_id, receipt_ref) {
                tracing::error!(
                    "failed to release producer reservation {reservation_id} after a successful side effect: {release_error}"
                );
            }
            Ok(value)
        }
        Err(ProducerSideEffectError::Dispatched(error)) => {
            // Never "failed": the producer gave up after dispatching its side
            // effect, which may well have happened (Codex review on #141).
            let reason =
                format!("side effect dispatched but its outcome was not observed: {error:#}");
            if let Err(close_error) = state.close_reservation_needs_rerun(&reservation_id, reason) {
                tracing::error!(
                    "failed to close producer reservation {reservation_id} as needs-rerun after a post-dispatch failure: {close_error}"
                );
            }
            Err(error)
        }
        Err(ProducerSideEffectError::ReceiptLost(error)) => {
            // Never "failed": the effect may have completed. A replay of the
            // same request reruns it and reports this reason.
            let reason = format!("receipt not recorded after the side effect ran: {error:#}");
            if let Err(close_error) = state.close_reservation_needs_rerun(&reservation_id, reason) {
                tracing::error!(
                    "failed to close producer reservation {reservation_id} as needs-rerun after a lost receipt: {close_error}"
                );
            }
            Err(error)
        }
        Err(ProducerSideEffectError::Failed(error)) => {
            if let Err(release_error) = state.release_failed(&reservation_id, &error) {
                tracing::error!(
                    "failed to release producer reservation {reservation_id} after side-effect error: {release_error}"
                );
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use impulse_ops::governed_task::{ApprovalPolicy, GovernedTaskRegistration};
    use tempfile::TempDir;

    use super::*;

    fn state() -> (TempDir, Arc<State>) {
        let root = TempDir::new().unwrap();
        let base = root.path().join("impulse-test");
        std::fs::create_dir_all(base.join(".impulse")).unwrap();
        let state = Arc::new(State::new(base.join(".impulse")).unwrap());
        (root, state)
    }

    fn registration(state: &State, request_id: &str) -> GovernedTaskRegistration {
        let root = state.storage().base_path().parent().unwrap();
        GovernedTaskRegistration::builder(
            request_id,
            format!("task-{request_id}"),
            "impulse-test",
            root.display().to_string(),
            "Ship the producer reservation journal",
            "worker-1",
            "codex",
        )
        .approval_policy(ApprovalPolicy::OperatorRequired)
        .build()
        .unwrap()
    }

    fn registered_task(state: &State, suffix: &str) -> impulse_ops::governed_task::GovernedTaskRun {
        state
            .register_governed_task(registration(state, &format!("register-{suffix}")))
            .unwrap()
    }

    fn request_id(value: &str) -> GovernedRequestId {
        GovernedRequestId::try_new(value).unwrap()
    }

    #[test]
    fn reserve_then_release_round_trips_through_reload() {
        let (root, state) = state();
        let task = registered_task(&state, "roundtrip");

        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        assert!(state
            .open_reservations()
            .unwrap()
            .iter()
            .any(|r| r.id == id));

        state.release(&id, "receipt-1").unwrap();
        assert!(state.open_reservations().unwrap().is_empty());

        // Reload from disk: the released reservation and its receipt
        // reference survive, and the ledger still validates.
        drop(state);
        let base = root.path().join("impulse-test").join(".impulse");
        let reloaded = State::new(base).unwrap();
        assert!(reloaded.open_reservations().unwrap().is_empty());
    }

    #[test]
    fn reserve_rejects_duplicate_open_reservation_for_same_task_and_producer() {
        let (_root, state) = state();
        let task = registered_task(&state, "dup");

        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();

        let error = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-2"),
                ProducerKind::Verification,
            )
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn reserve_at_a_newer_revision_closes_a_stale_older_open_reservation_instead_of_blocking() {
        let (_root, state) = state();
        let task = registered_task(&state, "stale-revision");

        // Simulate a reservation opened at an old revision that never got
        // released (e.g. an in-process panic, not a process restart, so
        // State::new's reconcile pass never ran for it).
        let stale_id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-stale"),
                ProducerKind::Verification,
            )
            .unwrap();

        // A caller with proof the task has moved on (a strictly higher
        // revision) must not be blocked by that stale reservation forever.
        let fresh_id = state
            .reserve(
                &task.id,
                task.revision + 1,
                &request_id("req-fresh"),
                ProducerKind::Verification,
            )
            .unwrap();
        assert_ne!(stale_id, fresh_id);

        let open = state.open_reservations().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, fresh_id);

        // The stale one is durably recorded as needs-rerun, not silently
        // dropped.
        let reason = state
            .pending_rerun_reason(
                &task.id,
                ProducerKind::Verification,
                &request_id("req-stale"),
            )
            .unwrap()
            .expect("the stale reservation must be recorded needs-rerun");
        assert!(reason.contains("superseded"));
    }

    #[test]
    fn reserve_at_the_same_revision_still_conflicts_with_an_open_reservation() {
        // Documents the residual gap the revision-scoped check accepts: two
        // attempts at the exact same revision (e.g. a same-revision retry
        // after an in-process panic with no other intervening activity)
        // still conflict. Only a real process restart (State::new's
        // reconcile) or future operator tooling clears that case.
        let (_root, state) = state();
        let task = registered_task(&state, "same-revision");

        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        let error = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-2"),
                ProducerKind::Verification,
            )
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn reserve_allows_a_different_producer_on_the_same_task_concurrently() {
        let (_root, state) = state();
        let task = registered_task(&state, "diff-producer");

        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        // A different producer kind on the same task is not a duplicate.
        let second = state.reserve(
            &task.id,
            task.revision,
            &request_id("req-2"),
            ProducerKind::SupervisorReview,
        );
        assert!(second.is_ok());
    }

    #[test]
    fn release_unknown_reservation_fails() {
        let (_root, state) = state();
        let bogus = ReservationId::try_new("producer-reservation-missing").unwrap();
        let error = state.release(&bogus, "receipt").unwrap_err();
        assert!(error.to_string().contains("was not found"));
    }

    #[test]
    fn release_twice_fails_on_the_second_call() {
        let (_root, state) = state();
        let task = registered_task(&state, "double-release");
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        state.release(&id, "receipt-1").unwrap();
        let error = state.release(&id, "receipt-2").unwrap_err();
        assert!(error.to_string().contains("already released"));
    }

    /// A lost receipt now closes reservations on the live path, so it must
    /// not overwrite an outcome a release already recorded.
    #[test]
    fn needs_rerun_close_refuses_an_already_released_reservation() {
        let (_root, state) = state();
        let task = registered_task(&state, "rerun-after-release");
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        state.release(&id, "receipt-1").unwrap();
        let error = state
            .close_reservation_needs_rerun(&id, "late lost receipt".to_string())
            .unwrap_err();
        assert!(error.to_string().contains("already released"));
        let recorded = state
            .latest_reservation(&task.id, ProducerKind::Verification, &request_id("req-1"))
            .unwrap()
            .unwrap();
        assert_eq!(
            recorded.outcome,
            Some(ReservationOutcome::Released {
                receipt_ref: "receipt-1".to_string()
            })
        );
        assert_eq!(recorded.effect, Some(ProducerEffectOutcome::Confirmed));
    }

    /// Simulates a daemon crash between a producer side effect and its
    /// receipt: a reservation is opened and never released, then the
    /// process reloads. Reload leaves exactly one open reservation visible
    /// before reconciliation runs (proven by inspecting the raw ledger),
    /// and `State::new`'s reconcile pass then closes it as needs-rerun with
    /// the request id recorded in the governed task's own event chain.
    #[test]
    fn interrupted_reservation_reconciles_to_needs_rerun_and_notes_the_governed_task() {
        let (root, state) = state();
        let task = registered_task(&state, "crash");
        let crash_request = request_id("verify-crash");

        let id = state
            .reserve(
                &task.id,
                task.revision,
                &crash_request,
                ProducerKind::Verification,
            )
            .unwrap();
        // No release() call: this models the crash.
        drop(state);

        let base = root.path().join("impulse-test").join(".impulse");
        let reloaded = State::new(base).unwrap();
        // The restarted daemon reconciles once it owns the project.
        reloaded.reconcile_producer_reservations().unwrap();

        // The interrupted reservation is no longer open...
        assert!(reloaded.open_reservations().unwrap().is_empty());

        // ...but reconciliation marked it needs-rerun and recorded why.
        let reloaded_task = reloaded
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        let note = reloaded_task
            .events
            .iter()
            .find(|event| {
                event.kind
                    == impulse_ops::governed_task::GovernedTaskEventKind::ProducerReservationInterrupted
            })
            .expect("interrupted reservation must be noted on the governed task");
        assert!(note.detail.contains(id.as_str()));
        assert!(note.detail.contains(crash_request.as_str()));
        assert!(note.detail.contains("interrupted before receipt"));
    }

    /// Regression: `State::new` used to reconcile, so any hook's CLI
    /// invocation closed the running daemon's open reservation as
    /// interrupted and wrote its own stale copy of the governed ledger.
    #[test]
    fn opening_state_elsewhere_leaves_a_live_reservation_open() {
        let (root, daemon_state) = state();
        let task = registered_task(&daemon_state, "live");
        let request = GovernedRequestId::try_new("verify-live").unwrap();
        let id = daemon_state
            .reserve(
                &task.id,
                task.revision,
                &request,
                ProducerKind::Verification,
            )
            .unwrap();
        let revision_before = daemon_state
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap()
            .revision;

        // A hook's CLI process opens the same project while the daemon's
        // side effect is still running.
        let base = root.path().join("impulse-test").join(".impulse");
        let hook_state = State::new(base.clone()).unwrap();
        assert!(hook_state
            .open_reservations()
            .unwrap()
            .iter()
            .any(|reservation| reservation.id == id));
        drop(hook_state);

        let on_disk = State::new(base).unwrap();
        assert!(on_disk
            .open_reservations()
            .unwrap()
            .iter()
            .any(|reservation| reservation.id == id));
        let task_after = on_disk
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(task_after.revision, revision_before);
        assert!(!task_after.events.iter().any(|event| {
            event.kind
                == impulse_ops::governed_task::GovernedTaskEventKind::ProducerReservationInterrupted
        }));

        // The daemon still releases its own reservation normally.
        daemon_state.release(&id, "receipt-live").unwrap();
    }

    #[test]
    fn reconcile_is_idempotent_across_repeated_calls() {
        let (root, state) = state();
        let task = registered_task(&state, "idempotent");
        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("verify-crash"),
                ProducerKind::Verification,
            )
            .unwrap();
        drop(state);

        let base = root.path().join("impulse-test").join(".impulse");
        let first_reload = State::new(base.clone()).unwrap();
        first_reload.reconcile_producer_reservations().unwrap();
        let first_task = first_reload
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        drop(first_reload);

        // A second reload (nothing new got interrupted) must not append a
        // second interruption note or bump the task revision again.
        let second_reload = State::new(base).unwrap();
        second_reload.reconcile_producer_reservations().unwrap();
        let second_task = second_reload
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(first_task.revision, second_task.revision);
        assert_eq!(first_task.events.len(), second_task.events.len());
    }

    #[test]
    fn replay_against_a_needs_rerun_reservation_is_distinguishable_from_fresh() {
        let (root, state) = state();
        let task = registered_task(&state, "distinguish");
        let replayed_request = request_id("verify-replay");

        // Nothing has ever reserved this task/producer/request triple.
        assert!(state
            .pending_rerun_reason(&task.id, ProducerKind::Verification, &replayed_request)
            .unwrap()
            .is_none());

        state
            .reserve(
                &task.id,
                task.revision,
                &replayed_request,
                ProducerKind::Verification,
            )
            .unwrap();
        drop(state); // crash before release()

        let base = root.path().join("impulse-test").join(".impulse");
        let reloaded = State::new(base).unwrap();
        reloaded.reconcile_producer_reservations().unwrap();

        // The same request id now carries a recorded needs-rerun reason:
        // a caller replaying it can tell this apart from a first attempt.
        let reason = reloaded
            .pending_rerun_reason(&task.id, ProducerKind::Verification, &replayed_request)
            .unwrap()
            .expect("replayed request must be distinguishable as a needs-rerun retry");
        assert!(reason.contains("interrupted before receipt"));

        // The retry itself is free to proceed: the prior reservation was
        // closed by reconciliation, so it is not a duplicate-open error.
        assert!(reloaded
            .reserve(
                &task.id,
                task.revision,
                &replayed_request,
                ProducerKind::Verification,
            )
            .is_ok());
    }

    #[test]
    fn forged_digest_fails_ledger_load_closed() {
        let (root, state) = state();
        let task = registered_task(&state, "forged");
        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        drop(state);

        let base = root.path().join("impulse-test").join(".impulse");
        let path = base.join(PRODUCER_RESERVATIONS_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["digest"] = serde_json::Value::String("sha256:forged".to_string());
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let error = State::new(base).unwrap_err();
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[test]
    fn truncated_ledger_fails_load_closed() {
        let (root, state) = state();
        let task = registered_task(&state, "truncated");
        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        drop(state);

        let base = root.path().join("impulse-test").join(".impulse");
        let path = base.join(PRODUCER_RESERVATIONS_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // Truncate: drop a reservation without updating the digest.
        value["reservations"].as_object_mut().unwrap().clear();
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let error = State::new(base).unwrap_err();
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[cfg(unix)]
    #[test]
    fn ledger_file_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let (root, state) = state();
        let task = registered_task(&state, "perms");
        state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        let base = root.path().join("impulse-test").join(".impulse");
        let path = base.join(PRODUCER_RESERVATIONS_FILE);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        drop(state);
    }

    /// Mirrors `ion_repl::history`'s unwritable-parent test: force the
    /// journal's own directory read-only so the atomic write inside
    /// `persist()` fails, and prove `reserve()` leaves no phantom open
    /// reservation behind — the exact regression this round of review
    /// caught (the ledger mutated in-memory before the fallible persist,
    /// with no rollback on failure).
    #[cfg(unix)]
    #[test]
    fn reserve_leaves_no_phantom_reservation_when_persist_fails() {
        use std::os::unix::fs::PermissionsExt;

        let (root, state) = state();
        let task = registered_task(&state, "unwritable-reserve");
        let base = root.path().join("impulse-test").join(".impulse");

        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = state.reserve(
            &task.id,
            task.revision,
            &request_id("req-1"),
            ProducerKind::Verification,
        );
        // Restore before any assertion that could panic and skip cleanup.
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(result.is_err(), "reserve must surface the write failure");
        assert!(
            state.open_reservations().unwrap().is_empty(),
            "a failed persist must not leave a phantom open reservation in the live ledger"
        );

        // The directory is writable again: the same call now succeeds,
        // proving the failed attempt did not leave a duplicate-open
        // reservation behind either.
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        assert!(state
            .open_reservations()
            .unwrap()
            .iter()
            .any(|r| r.id == id));
    }

    #[cfg(unix)]
    #[test]
    fn release_leaves_reservation_open_when_persist_fails() {
        use std::os::unix::fs::PermissionsExt;

        let (root, state) = state();
        let task = registered_task(&state, "unwritable-release");
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("req-1"),
                ProducerKind::Verification,
            )
            .unwrap();
        let base = root.path().join("impulse-test").join(".impulse");

        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = state.release(&id, "receipt-1");
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(result.is_err(), "release must surface the write failure");
        assert!(
            state
                .open_reservations()
                .unwrap()
                .iter()
                .any(|r| r.id == id),
            "a failed persist must leave the reservation open, not silently released"
        );

        // Writable again: release now succeeds for real.
        state.release(&id, "receipt-1").unwrap();
        assert!(state.open_reservations().unwrap().is_empty());
    }

    #[test]
    fn serde_round_trip_producer_reservation() {
        let original = ProducerReservation {
            id: new_reservation_id(),
            task_id: GovernedTaskId::try_new("task-1").unwrap(),
            revision: 3,
            request_id: request_id("req-1"),
            producer: ProducerKind::SupervisorReview,
            reserved_at: impulse_ops::now_rfc3339(),
            released_at: Some(impulse_ops::now_rfc3339()),
            outcome: Some(ReservationOutcome::NeedsRerun {
                reason: "interrupted before receipt".to_string(),
            }),
            effect: Some(ProducerEffectOutcome::Unknown),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ProducerReservation = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn serde_round_trip_producer_kind_and_outcome() {
        for kind in [
            ProducerKind::Verification,
            ProducerKind::SupervisorReview,
            ProducerKind::Promotion,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let recovered: ProducerKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, recovered);
        }

        let released = ReservationOutcome::Released {
            receipt_ref: "req-1".to_string(),
        };
        let json = serde_json::to_string(&released).unwrap();
        let recovered: ReservationOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(released, recovered);
    }

    #[tokio::test]
    async fn with_reservation_releases_with_receipt_on_success() {
        let (_root, state) = state();
        let task = registered_task(&state, "with-reservation-ok");

        let result = with_reservation(
            &state,
            &task.id,
            task.revision,
            &request_id("req-1"),
            ProducerKind::Verification,
            || async { Ok((42, "receipt-42".to_string())) },
        )
        .await
        .unwrap();
        assert_eq!(result, 42);
        assert!(state.open_reservations().unwrap().is_empty());
    }

    #[tokio::test]
    async fn with_reservation_releases_with_failure_recorded_when_side_effect_fails() {
        let (_root, state) = state();
        let task = registered_task(&state, "with-reservation-err");
        let call_request = request_id("req-1");

        let error = with_reservation(
            &state,
            &task.id,
            task.revision,
            &call_request,
            ProducerKind::Verification,
            || async { anyhow::bail!("cargo test failed") as Result<((), String)> },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("cargo test failed"));

        // Not left open: a corrected retry is free to reserve again.
        assert!(state.open_reservations().unwrap().is_empty());
        assert!(state
            .reserve(
                &task.id,
                task.revision,
                &call_request,
                ProducerKind::Verification,
            )
            .is_ok());
    }

    /// The success path must not propagate a `release()` persist failure as
    /// if the durably-recorded side effect itself had failed. The closure
    /// chmods the ledger directory read-only from *inside* itself, after
    /// `reserve()` (which needed to succeed to get here) but before
    /// `with_reservation`'s own `release()` call runs — simulating a
    /// transient I/O failure landing exactly between a successful,
    /// durable side effect and this journal's own bookkeeping.
    #[cfg(unix)]
    #[tokio::test]
    async fn with_reservation_reports_success_even_when_release_persist_fails() {
        use std::os::unix::fs::PermissionsExt;

        let (root, state) = state();
        let task = registered_task(&state, "with-reservation-release-fails");
        let base = root.path().join("impulse-test").join(".impulse");
        let base_for_closure = base.clone();

        let result = with_reservation(
            &state,
            &task.id,
            task.revision,
            &request_id("req-1"),
            ProducerKind::Verification,
            move || async move {
                std::fs::set_permissions(&base_for_closure, std::fs::Permissions::from_mode(0o500))
                    .unwrap();
                Ok((42, "receipt-42".to_string()))
            },
        )
        .await;

        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(
            result.unwrap(),
            42,
            "a durably-recorded success must be reported as success even though \
             this journal's own release() persist failed"
        );
        // The reservation is left open (release never durably completed);
        // this is the accepted, documented residual — the governed-task
        // mutation itself is still durable, which is what replay safety
        // actually depends on.
        assert!(!state.open_reservations().unwrap().is_empty());
    }

    // ── Effect outcome: unknown vs failed-known (blueprint 2.1 "lost response")

    fn persisted_journal_schema_version(state: &State) -> u64 {
        let path = state.storage().path(PRODUCER_RESERVATIONS_FILE);
        let journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        journal["schema_version"].as_u64().unwrap()
    }

    #[test]
    fn a_crash_before_the_receipt_reconciles_to_an_unknown_effect_not_a_failure() {
        let (root, state) = state();
        let task = registered_task(&state, "lost-response");
        let request = request_id("verify-lost-response");
        state
            .reserve(
                &task.id,
                task.revision,
                &request,
                ProducerKind::Verification,
            )
            .unwrap();
        // The side effect ran; the daemon died before its receipt.
        drop(state);

        let base = root.path().join("impulse-test").join(".impulse");
        let reloaded = State::new(base).unwrap();
        reloaded.reconcile_producer_reservations().unwrap();

        let reservation = reloaded
            .latest_reservation(&task.id, ProducerKind::Verification, &request)
            .unwrap()
            .expect("the journal keeps the interrupted attempt");
        assert_eq!(reservation.effect, Some(ProducerEffectOutcome::Unknown));
        assert_eq!(
            reservation.effect_outcome(),
            Some(ProducerEffectOutcome::Unknown)
        );
        assert!(matches!(
            reservation.outcome,
            Some(ReservationOutcome::NeedsRerun { .. })
        ));
        let task = reloaded
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        let note = task
            .events
            .iter()
            .find(|event| {
                event.kind
                    == impulse_ops::governed_task::GovernedTaskEventKind::ProducerReservationInterrupted
            })
            .expect("the interruption is noted on the task");
        assert!(note.detail.contains("effect outcome unknown"));
    }

    #[tokio::test]
    async fn a_receipt_lost_after_the_side_effect_is_unknown_and_needs_rerun() {
        let (_root, state) = state();
        let task = registered_task(&state, "receipt-lost");
        let request = request_id("verify-receipt-lost");

        let error = with_reservation_effect(
            &state,
            &task.id,
            task.revision,
            &request,
            ProducerKind::Verification,
            || async {
                Err::<((), String), _>(ProducerSideEffectError::ReceiptLost(anyhow::anyhow!(
                    "governed ledger write failed after cargo test finished"
                )))
            },
        )
        .await
        .unwrap_err();
        // The caller still sees the original error, unchanged.
        assert_eq!(
            error.to_string(),
            "governed ledger write failed after cargo test finished"
        );

        let reservation = state
            .latest_reservation(&task.id, ProducerKind::Verification, &request)
            .unwrap()
            .unwrap();
        assert_eq!(
            reservation.effect_outcome(),
            Some(ProducerEffectOutcome::Unknown),
            "a lost receipt must never be recorded as a failure"
        );
        assert!(matches!(
            reservation.outcome,
            Some(ReservationOutcome::NeedsRerun { .. })
        ));
        // A replay of the same request is told it is rerunning, and is free to.
        assert!(state
            .pending_rerun_reason(&task.id, ProducerKind::Verification, &request)
            .unwrap()
            .unwrap()
            .contains("receipt not recorded"));
        assert!(state.open_reservations().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failure_the_producer_reports_itself_is_failed_known() {
        let (_root, state) = state();
        let task = registered_task(&state, "failed-known");
        let request = request_id("verify-failed-known");
        with_reservation(
            &state,
            &task.id,
            task.revision,
            &request,
            ProducerKind::Verification,
            || async { anyhow::bail!("cargo could not be spawned") as Result<((), String)> },
        )
        .await
        .unwrap_err();
        let reservation = state
            .latest_reservation(&task.id, ProducerKind::Verification, &request)
            .unwrap()
            .unwrap();
        assert_eq!(
            reservation.effect_outcome(),
            Some(ProducerEffectOutcome::FailedKnown)
        );
        assert_eq!(
            state
                .pending_rerun_reason(&task.id, ProducerKind::Verification, &request)
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_confirmed_side_effect_is_recorded_as_confirmed() {
        let (_root, state) = state();
        let task = registered_task(&state, "confirmed");
        let request = request_id("verify-confirmed");
        with_reservation(
            &state,
            &task.id,
            task.revision,
            &request,
            ProducerKind::Verification,
            || async { Ok(((), "verify-confirmed".to_string())) },
        )
        .await
        .unwrap();
        let reservation = state
            .latest_reservation(&task.id, ProducerKind::Verification, &request)
            .unwrap()
            .unwrap();
        assert_eq!(
            reservation.effect_outcome(),
            Some(ProducerEffectOutcome::Confirmed)
        );
    }

    /// Compatibility: a version-1 journal (no `effect` field) still loads, its
    /// outcomes are derived, and it is only rewritten at version 2 once a
    /// reservation actually records an effect.
    #[test]
    fn a_v1_journal_loads_and_derives_effect_outcomes() {
        let (root, state) = state();
        let task = registered_task(&state, "legacy-journal");
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("legacy"),
                ProducerKind::Verification,
            )
            .unwrap();
        // Only an open reservation so far: nothing needs version 2.
        assert_eq!(persisted_journal_schema_version(&state), 1);
        drop(state);

        // Hand-write the legacy shape a pre-upgrade daemon left behind: closed
        // with a `failed: …` receipt and no `effect` field.
        let base = root.path().join("impulse-test").join(".impulse");
        let path = base.join(PRODUCER_RESERVATIONS_FILE);
        let mut journal: ProducerReservationLedger =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let reservation = journal.reservations.get_mut(&id).unwrap();
        reservation.released_at = Some(impulse_ops::now_rfc3339());
        reservation.outcome = Some(ReservationOutcome::Released {
            receipt_ref: "failed: cargo test failed".to_string(),
        });
        journal.digest = compute_digest(&journal.reservations).unwrap();
        let bytes = serde_json::to_vec(&journal).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("\"effect\""));
        std::fs::write(&path, bytes).unwrap();

        let reloaded = State::new(base).unwrap();
        let legacy = reloaded
            .latest_reservation(&task.id, ProducerKind::Verification, &request_id("legacy"))
            .unwrap()
            .unwrap();
        assert_eq!(legacy.effect, None);
        assert_eq!(
            legacy.effect_outcome(),
            Some(ProducerEffectOutcome::FailedKnown)
        );
        assert_eq!(persisted_journal_schema_version(&reloaded), 1);

        // The first new close writes version 2.
        let fresh = reloaded
            .reserve(
                &task.id,
                task.revision,
                &request_id("fresh"),
                ProducerKind::Verification,
            )
            .unwrap();
        reloaded.release(&fresh, "fresh").unwrap();
        assert_eq!(persisted_journal_schema_version(&reloaded), 2);
    }

    #[test]
    fn a_v1_journal_that_records_effect_outcomes_is_refused() {
        let (root, state) = state();
        let task = registered_task(&state, "forged-v1-journal");
        let id = state
            .reserve(
                &task.id,
                task.revision,
                &request_id("forged"),
                ProducerKind::Verification,
            )
            .unwrap();
        state.release(&id, "forged").unwrap();
        drop(state);
        let base = root.path().join("impulse-test").join(".impulse");
        let path = base.join(PRODUCER_RESERVATIONS_FILE);
        let mut journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        journal["schema_version"] = serde_json::json!(1);
        std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        let error = match State::new(base) {
            Ok(_) => panic!("a v1 journal cannot carry effect outcomes"),
            Err(error) => error,
        };
        assert!(
            format!("{error:#}").contains("require version 2"),
            "unexpected error: {error:#}"
        );
    }

    // ── Post-dispatch producer failures (Codex review on #141) ───────────────

    #[test]
    fn classify_producer_error_separates_dispatched_from_failed() {
        let plain = classify_producer_error(anyhow::anyhow!("git could not be spawned"));
        assert!(matches!(plain, ProducerSideEffectError::Failed(_)));

        let marked = EffectDispatchedError::wrap(
            anyhow::anyhow!("worktree sync failed").context("promotion left the branch moved"),
        );
        // Transparent: the operator-facing message and chain are unchanged.
        assert_eq!(marked.to_string(), "promotion left the branch moved");
        assert_eq!(
            format!("{marked:#}"),
            "promotion left the branch moved: worktree sync failed"
        );
        // Wrapping twice stays one layer.
        let marked = EffectDispatchedError::wrap(marked);
        match classify_producer_error(marked) {
            ProducerSideEffectError::Dispatched(inner) => {
                assert!(!inner.is::<EffectDispatchedError>());
                assert_eq!(
                    format!("{inner:#}"),
                    "promotion left the branch moved: worktree sync failed"
                );
            }
            other => panic!("a marked error is dispatched, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failure_after_dispatch_is_unknown_and_needs_rerun_not_failed_known() {
        let (_root, state) = state();
        let task = registered_task(&state, "dispatched-then-failed");
        let request = request_id("promote-dispatched");
        let error = with_reservation(
            &state,
            &task.id,
            task.revision,
            &request,
            ProducerKind::Promotion,
            || async {
                // The compare-and-swap moved the branch; the sync after it failed.
                Err::<((), String), _>(EffectDispatchedError::wrap(anyhow::anyhow!(
                    "canonical worktree sync failed after the branch moved"
                )))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "canonical worktree sync failed after the branch moved"
        );
        assert!(
            !error.is::<EffectDispatchedError>(),
            "the caller gets the producer's own error back"
        );
        let reservation = state
            .latest_reservation(&task.id, ProducerKind::Promotion, &request)
            .unwrap()
            .unwrap();
        assert_eq!(
            reservation.effect_outcome(),
            Some(ProducerEffectOutcome::Unknown)
        );
        assert!(matches!(
            reservation.outcome,
            Some(ReservationOutcome::NeedsRerun { .. })
        ));
        assert!(state
            .pending_rerun_reason(&task.id, ProducerKind::Promotion, &request)
            .unwrap()
            .unwrap()
            .contains("outcome was not observed"));
    }
}
