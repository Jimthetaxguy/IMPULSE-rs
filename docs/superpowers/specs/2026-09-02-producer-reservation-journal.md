---
title: Producer Reservation Journal
description: State-layer design for a durable reservation journal closing ADR-0012's crash-between-side-effect-and-receipt gap
updated: 2026-09-02
type: specification
category: architecture
phase: all
status: active
audience: builders
tags: [spec, governed-tasks, producers, crash-safety, state]
---

# Producer Reservation Journal

> Stage 3 (state layer) of `docs/plans/2026-09-02-impulse-next-stages.md`. Handler wiring
> (`RunGovernedVerification`/`RunGovernedSupervisorReview`) is explicitly out of scope here and is
> handed off to the next lane; see "Handoff: handler wiring" below.

## Problem

ADR-0012's consequences section names an open gap:

> Persisted receipts and the per-task daemon lock deduplicate replay and concurrent requests. They
> do not provide crash-safe exactly-once producer execution: a daemon exit between a command/model
> side effect and durable receipt can repeat that side effect after restart.

`src/governed_producers.rs::run_verification` executes a real `cargo` workspace verification (or,
for `RunGovernedSupervisorReview`, a real model turn) before the daemon calls
`persist_governed_mutation` to record the receipt in `GOVERNED_TASKS.json`. Both the per-task
Tokio mutex (`State::acquire_governed_producer_lock`) and the request-id idempotency receipt are
in-memory-adjacent or receipt-dependent: neither one durably records "a side effect is in flight"
independent of whether its mutation ever lands. If the daemon process exits between the side effect
returning and the receipt being persisted, a replayed request finds no receipt and reruns the side
effect.

## Design

A new private, atomic, digest-verified ledger — `state/producer_reservation.rs`, persisted at
`.impulse/PRODUCER_RESERVATIONS.json` — records the *intent* to run a producer side effect before
it starts, independent of whether that side effect's own receipt is ever durably recorded.

### Types

- `ProducerKind` — `Verification | SupervisorReview | Promotion` (the third reserved for ADR-0019's
  staged-worktree promote producer).
- `ReservationOutcome` — `Released { receipt_ref: String } | NeedsRerun { reason: String }`.
- `ProducerReservation { id, task_id, revision, request_id, producer, reserved_at, released_at:
  Option<String>, outcome: Option<ReservationOutcome> }`. `id` reuses
  `impulse_ops::governed_task::GovernedRecordId` (aliased as `ReservationId`) rather than a new
  newtype — reservations are governed-task-adjacent records and the existing id type already
  carries the validation and `Ord`/`Hash` this journal's `BTreeMap` key needs.

### API on `State`

- `reserve(task_id, revision, request_id, producer) -> Result<ReservationId>` — fails with
  `ProducerReservationError::DuplicateOpenReservation` if an open (unreleased) reservation already
  exists for the same `(task_id, producer)` pair, so a request replayed while the original side
  effect is still in flight cannot start a second, competing run.
- `release(id, receipt_ref) -> Result<()>` — marks the reservation `Released { receipt_ref }`.
- `open_reservations() -> Result<Vec<ProducerReservation>>`.
- `pending_rerun_reason(task_id, producer, request_id) -> Result<Option<String>>` — the reason the
  most recent reservation for this exact triple needed a rerun, if any. Not named in the original
  three-method sketch, but required to make the acceptance criterion "a replayed request against a
  needs-rerun reservation is distinguishable from a fresh one" observable without complicating
  `reserve`'s return type.
- `reconcile_producer_reservations()` (`pub(super)`, invoked once from `State::new`) — closes every
  reservation still open from a previous process as `NeedsRerun { reason: "interrupted before
  receipt" }` and records a note on the owning governed task's own event chain (see below). Safe to
  call again: each note is written through the governed-task ledger's existing idempotent mutation
  path (`mutate_governed_task`), keyed by a request id derived deterministically from the
  reservation id, so a repeat reconcile cannot duplicate an event or re-bump the task revision.

### `with_reservation` — the integration contract

```rust
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
    Fut: Future<Output = Result<(T, String)>>,
```

`side_effect` must perform **both** the external side effect and persist the governed-task mutation
that records it, returning `(value, receipt_ref)` only once that mutation is durable. This ordering
is the entire point:

- Crash before the mutation is durable (during `run_verification`, or after it returns but before
  `persist_governed_mutation` completes): the reservation is still open at reload. Reconcile marks
  it needs-rerun. No receipt exists in `GOVERNED_TASKS.json`, so a legitimate replay correctly
  reruns the side effect — nothing was ever durably recorded, so redoing it is the right, idempotent
  behavior for a deterministic verification profile.
- Crash after the mutation is durable but before `release()` runs: the reservation is marked
  needs-rerun by reconcile, but the governed-task ledger already holds the receipt. The *existing*
  replay check (`require_producer_request_state` in `src/daemon/handlers.rs`) recognizes the
  request id and returns `replay = true`, so the handler does not call `run_verification` again —
  it replays the recorded verification input instead. This is the case the plan's acceptance
  criterion names: "a replayed request does not re-run cargo."

Releasing immediately after the side effect alone, before its mutation is persisted, would reopen
exactly the gap this journal exists to close — so `with_reservation` intentionally does not offer a
"release after just the side effect" shape.

On failure, the reservation is released with the failure recorded (`Released { receipt_ref: "failed:
<error>" }`) rather than left open, so a corrected retry is not blocked by
`DuplicateOpenReservation`.

### Ledger validation

`ProducerReservationLedger` carries a `digest: String` field — a SHA-256 over the canonical JSON
serialization of its `reservations` map, recomputed and compared at every load (mirroring the
whole-file SHA-256 approach `src/basis.rs`'s `BasisDeclaration::Ledger` uses for its own freshness
check, adapted here to guard the reservation file itself rather than a basis source). A forged or
truncated file fails closed with `ProducerReservationError::LedgerDigestMismatch`, propagated
through `State::new`'s `?` rather than panicking — daemon startup fails rather than silently
trusting a tampered journal.

### Governed-task event chain integration

`reconcile_producer_reservations` needed to make an interrupted reservation visible on the
operator surface without inventing a second source of truth. It reuses the existing, already-tested
idempotent governed-task mutation pipeline rather than writing directly into `GOVERNED_TASKS.json`:

1. A new `GovernedTaskEventKind::ProducerReservationInterrupted` variant
   (`impulse-ops/src/governed_task.rs`) and a matching
   `GovernedTaskMutation::NoteProducerReservationInterrupted { actor, reason }` variant.
2. `apply_mutation` (`src/state/governed_task.rs`) gained one new match arm: a pure observability
   append — it does not transition `execution_state` or `review_state`, since the crash it records
   can interrupt a producer regardless of the task's current review state.
3. The ledger-reload replay validator gained a matching arm that reconstructs the mutation directly
   from the recorded event's `actor`/`detail`, exactly mirroring the existing
   `MarkLaunchFailed`/`MarkRuntimeExited` pattern (a single string field, no separate stored record
   type) rather than the richer claim/verification/verdict pattern that needs cross-referenced
   record ids.
4. `governed_project_id()` on `State` was widened from private to `pub(crate)` (one keyword) so the
   sibling `producer_reservation` module can resolve the project id for the mutation request.

This is genuinely additive: no existing match arm changed, and old `GOVERNED_TASKS.json` files with
no such event still deserialize and replay unchanged.

## Handoff: handler wiring

Not done in this lane (state layer only; `src/daemon/**` and `src/governed_producers.rs` are owned
by other lanes this session). The exact adoption shape for `RunGovernedVerification`
(`src/daemon/handlers.rs`, ~1233-1270) and `RunGovernedSupervisorReview` (~1913-2000):

```rust
let verification = if replay {
    replay_verification_input(&task, request.expected_revision)?
} else {
    preflight_verification(&task)?;
    crate::state::with_reservation(
        state,
        &request.task_id,
        request.expected_revision,
        &request.request_id,
        crate::state::ProducerKind::Verification,
        || async {
            let verification = crate::governed_producers::run_verification(&task).await?;
            let mutation_request = /* build RecordVerification GovernedTaskMutationRequest */;
            let updated = state.mutate_governed_task(mutation_request)?;
            Ok((verification, request.request_id.to_string()))
        },
    )
    .await?
};
```

The `_producer_guard` (`state.acquire_governed_producer_lock`) stays; it is an in-memory
optimization that avoids two concurrent in-process requests both reaching `reserve()`, not a
replacement for the durable reservation. The same shape applies to
`RunGovernedSupervisorReview` with `ProducerKind::SupervisorReview` and the Supervisor turn plus
`RecordSupervisorVerdict` mutation as the closure body. Note that `with_reservation`'s closure must
build and persist the mutation itself (via `state.mutate_governed_task`), not just run the raw side
effect — the caller currently does that persist step *outside* the request-handling match in
`handle_governed_producer_request`, so adopting this helper also means moving that persist call
inside the closure for these two branches.

## Acceptance

Proven at the state layer by `src/state/producer_reservation.rs`'s test module:

1. reserve/release round trips through a full `State` reload;
2. `reserve` rejects a duplicate open reservation for the same task+producer, and allows a
   different producer on the same task concurrently;
3. a simulated crash (reserve, drop `State` without releasing, reload) leaves the reservation
   closed as `NeedsRerun` after reconcile, with the reservation id, the interrupting request id,
   and the reason all present in the governed task's own event chain;
4. reconcile is idempotent across repeated `State::new` calls (no duplicate event, no extra
   revision bump);
5. a replayed request against a needs-rerun reservation is distinguishable from a fresh one via
   `pending_rerun_reason`, and the retry itself is not blocked (the prior reservation is closed, not
   open);
6. a forged or truncated ledger digest fails `State::new` closed;
7. the ledger file is `0600` on Unix;
8. every `Serialize + Deserialize` type here has a round-trip test;
9. `with_reservation` releases with the receipt on success and with the failure recorded (not left
   open) when the closure errors.

Full handler-level acceptance (a replayed request genuinely skipping a second `cargo` run, proven
against the execution counter at `src/governed_producers.rs:1080-1081`) is out of scope for this
lane and belongs to the handler-wiring follow-up above.

## Research

- ADR-0012's consequences section (the gap statement quoted above).
- `impulse-desktop/src/daemon_ops.rs:1061-1068` — the existing write-ahead lifecycle outbox
  pattern for ambiguous launch/exit mutations awaiting daemon reconciliation; this journal is the
  same shape (durable intent recorded before an ambiguous operation, reconciled on reload) applied
  to producer side effects instead of process lifecycle.
- Helland, "Life beyond Distributed Transactions" (2012) — idempotence and at-least-once delivery
  as the operating assumption; this journal turns "at least once" into "at least once, and the
  second time is visibly flagged" rather than attempting exactly-once across a process crash.
- `src/basis.rs`'s `BasisDeclaration::Ledger` — the whole-file SHA-256 pattern this journal reuses
  for its own digest.
