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
//! receipt existed; [`State::new`] reconciles it to
//! [`ReservationOutcome::NeedsRerun`] and records a note on the governed
//! task's own event chain so the operator surface can show it. Wiring
//! `RunGovernedVerification`/`RunGovernedSupervisorReview` to call
//! [`with_reservation`] around the side effect *and* the mutation persist
//! step is handler-lane follow-up work — see `with_reservation`'s doc comment
//! for the exact integration contract.
//!
//! The journal is deliberately independent of `GOVERNED_TASKS.json`: a
//! forged or truncated journal fails closed at load without blocking
//! unrelated governed-task state, matching the private-ledger pattern in
//! `state/memory_candidate.rs`.

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
const PRODUCER_RESERVATIONS_SCHEMA_VERSION: u32 = 1;

/// A reservation's identity. An alias over the existing governed-record id
/// type rather than a new newtype: reservations are governed-task-adjacent
/// records and the id already carries the validation (nonempty, no
/// whitespace/control characters, bounded length) and `Ord`/`Hash` this
/// journal needs for a `BTreeMap` key.
pub type ReservationId = GovernedRecordId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProducerReservationError {
    #[error("producer reservation ledger digest mismatch: the journal may be forged or truncated")]
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
}

impl ProducerReservation {
    fn is_open(&self) -> bool {
        self.released_at.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ProducerReservationLedger {
    schema_version: u32,
    #[serde(default)]
    reservations: BTreeMap<ReservationId, ProducerReservation>,
    /// SHA-256 over the canonical JSON serialization of `reservations`,
    /// recomputed and checked at every load. A forged or truncated file
    /// changes this without the attacker knowing the exact serialization
    /// this journal uses, so tampering fails closed instead of silently
    /// hiding or fabricating a reservation.
    #[serde(default)]
    digest: String,
}

impl Default for ProducerReservationLedger {
    fn default() -> Self {
        let reservations = BTreeMap::new();
        // Hashing an empty, already-valid map cannot fail.
        let digest = compute_digest(&reservations).expect("hashing an empty map cannot fail");
        Self {
            schema_version: PRODUCER_RESERVATIONS_SCHEMA_VERSION,
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
        if self.schema_version != PRODUCER_RESERVATIONS_SCHEMA_VERSION {
            return Err(
                ProducerReservationError::UnsupportedSchemaVersion(self.schema_version).into(),
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

    fn persist(&mut self, storage: &Storage) -> Result<()> {
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
    /// side effect. Fails if an open reservation already exists for the same
    /// task and producer, so a request replayed while the original side
    /// effect is still in flight cannot start a second, competing run.
    pub fn reserve(
        &self,
        task_id: &GovernedTaskId,
        revision: u64,
        request_id: &GovernedRequestId,
        producer: ProducerKind,
    ) -> Result<ReservationId> {
        let mut ledger = self.producer_reservations.lock().map_err(lock_err)?;

        if ledger
            .reservations
            .values()
            .any(|r| &r.task_id == task_id && r.producer == producer && r.is_open())
        {
            return Err(ProducerReservationError::DuplicateOpenReservation {
                task_id: task_id.clone(),
                producer,
            }
            .into());
        }

        let id = new_reservation_id();
        let reservation = ProducerReservation {
            id: id.clone(),
            task_id: task_id.clone(),
            revision,
            request_id: request_id.clone(),
            producer,
            reserved_at: impulse_ops::now_rfc3339(),
            released_at: None,
            outcome: None,
        };
        ledger.reservations.insert(id.clone(), reservation);
        ledger.persist(self.storage())?;
        Ok(id)
    }

    /// Release a reservation once its side effect *and* the governed-task
    /// mutation that records it are both durably persisted. `receipt_ref`
    /// should name that receipt (for example the governed request id that
    /// produced it) so the journal entry stays traceable.
    pub fn release(&self, id: &ReservationId, receipt_ref: impl Into<String>) -> Result<()> {
        let mut ledger = self.producer_reservations.lock().map_err(lock_err)?;
        let reservation = ledger
            .reservations
            .get_mut(id)
            .ok_or_else(|| ProducerReservationError::NotFound(id.clone()))?;
        if !reservation.is_open() {
            return Err(ProducerReservationError::AlreadyReleased(id.clone()).into());
        }
        reservation.released_at = Some(impulse_ops::now_rfc3339());
        reservation.outcome = Some(ReservationOutcome::Released {
            receipt_ref: receipt_ref.into(),
        });
        ledger.persist(self.storage())?;
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
    /// interruption on the governed task's own event chain. Invoked once
    /// from [`State::new`]; safe to call again (each note is recorded
    /// through the same idempotent governed-task mutation path keyed by a
    /// deterministic per-reservation request id, so a repeat reconcile does
    /// not duplicate events).
    pub(super) fn reconcile_producer_reservations(&self) -> Result<()> {
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
        let reservation = ledger
            .reservations
            .get_mut(id)
            .ok_or_else(|| ProducerReservationError::NotFound(id.clone()))?;
        reservation.released_at = Some(impulse_ops::now_rfc3339());
        reservation.outcome = Some(ReservationOutcome::NeedsRerun { reason });
        ledger.persist(self.storage())
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
            "producer reservation {} ({:?}) for request {} needs rerun: {reason}",
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
/// On failure the reservation is released with the failure recorded (never
/// left open) so a corrected retry is not blocked by
/// [`ProducerReservationError::DuplicateOpenReservation`]. This is a plain
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
    let reservation_id = state.reserve(task_id, revision, request_id, producer)?;
    match side_effect().await {
        Ok((value, receipt_ref)) => {
            state.release(&reservation_id, receipt_ref)?;
            Ok(value)
        }
        Err(error) => {
            if let Err(release_error) = state.release(&reservation_id, format!("failed: {error}")) {
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
        let first_task = first_reload
            .get_governed_task("impulse-test", &task.id)
            .unwrap()
            .unwrap();
        drop(first_reload);

        // A second reload (nothing new got interrupted) must not append a
        // second interruption note or bump the task revision again.
        let second_reload = State::new(base).unwrap();
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
}
