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
  `ProducerReservationError::DuplicateOpenReservation` if an open (unreleased) reservation *at or
  after* `revision` already exists for the same `(task_id, producer)` pair, so a request replayed
  while the original side effect is genuinely still in flight cannot start a second, competing run.
  An open reservation strictly *behind* `revision` cannot be that live attempt — the task has moved
  on since it was taken — so `reserve` closes it as `NeedsRerun` in the same persisted write rather
  than leaving it to block every future call for that task/producer until a process restart. See
  "Revision-scoped duplicate check" below for what this does and does not close.
- `release(id, receipt_ref) -> Result<()>` — marks the reservation `Released { receipt_ref }`.

  `reserve`, `release`, and the internal `close_reservation_needs_rerun` all follow a
  clone-mutate-persist-swap discipline (matching `state/governed_task.rs`'s `mutate_governed_task`
  and `state/memory_candidate.rs`'s candidate-write pattern): each clones the in-memory ledger,
  applies the change to the clone, persists the clone, and only then swaps it into the live
  `Mutex`-guarded state. An earlier version of this journal mutated the live ledger *before* the
  fallible persist call with no rollback on failure — a transient I/O error (a momentarily
  unwritable `.impulse/`, a full disk) would return `Err` while leaving a phantom reservation
  behind that the caller has no id for and can never release, permanently blocking every later
  `reserve()` for that task/producer until the daemon restarted. Fixed in adversarial review before
  merge; proven by `reserve_leaves_no_phantom_reservation_when_persist_fails` and
  `release_leaves_reservation_open_when_persist_fails`, both of which chmod `.impulse/` read-only
  to force the persist to fail and then assert the live ledger is unchanged and a corrected retry
  succeeds once the directory is writable again.
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

### Revision-scoped duplicate check

Keying `DuplicateOpenReservation` purely on `(task_id, producer)` has a second failure mode besides
the crash-and-restart one this journal was built for: an **in-process panic** (not a process exit)
during a side effect leaves a reservation open with no `State::new` reconcile ever running to close
it — the daemon process is still alive. Every subsequent `reserve()` for that task and producer
would then fail forever, until an operator restarted the daemon.

`reserve` now scopes the check by revision instead of by task/producer alone: an open reservation
with `stored.revision < incoming revision` cannot represent a still-relevant in-flight attempt,
because the caller's higher revision is proof the task has genuinely moved on since that
reservation was taken (nothing legitimate can still be racing to complete against a revision that
is no longer current). Such a reservation is closed as `NeedsRerun` in the same write that creates
the new one, rather than left to block.

**What this does not close**, stated explicitly per review: two attempts at the **exact same**
revision still conflict — a caller retrying immediately after an in-process panic, with the task's
revision unchanged (no other activity happened in between), hits `DuplicateOpenReservation` exactly
as before. Two options were on the table: this revision-scoped check, or an explicit
`clear_stale_reservation(id)` API for the operator surface. The revision-scoped check was chosen
because it needs no new operator plumbing and closes the more common case (the task advances
between the panicked attempt and the retry — including via an unrelated operator action or a
different producer's run), while the same-revision case is left as a known, documented gap rather
than quietly declared solved. Closing it fully needs either a real process restart (which still
works today, via `State::new`'s reconcile) or future operator-facing tooling; test:
`reserve_at_the_same_revision_still_conflicts_with_an_open_reservation` proves the gap is exactly
this narrow, not broader.

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
`DuplicateOpenReservation`. On success, a `release()` persist failure is logged and swallowed rather
than propagated as if the side effect itself had failed — `side_effect`'s own governed-task mutation
is already durable by the time `with_reservation` calls `release`, so a failure releasing *this*
journal's bookkeeping must not turn a durably-recorded success into a reported failure. (An earlier
version of this function used `?` on the success-path `release()` call, which had exactly that bug;
fixed before merge, proven by
`with_reservation_reports_success_even_when_release_persist_fails`, which forces the release to
fail from inside the closure itself, after `reserve()` has already succeeded.)

**Not panic-safe.** `with_reservation` has no `catch_unwind`. If `side_effect` panics, the
reservation is left open (matching the crash-before-receipt case) but `release()` never runs and
the panic propagates through `with_reservation` itself — there is no special handling distinguishing
a panic from any other way the calling task might stop running. The handler-wiring lane must treat
an in-process panic the same way it treats a process crash: the reservation stays open until either
a real process restart runs `State::new`'s reconcile, or (for the same-revision case reconcile
cannot reach without a restart) the revision-scoped check above closes it once the task's revision
next advances.

### Ledger validation

`ProducerReservationLedger` carries a `digest: String` field — a SHA-256 over the canonical JSON
serialization of its `reservations` map, recomputed and compared at every load (mirroring the
whole-file SHA-256 approach `src/basis.rs`'s `BasisDeclaration::Ledger` uses for its own freshness
check, adapted here to guard the reservation file itself rather than a basis source). **This is
corruption/truncation detection, not tamper resistance**: it is an unkeyed hash, so anyone with
filesystem write access to the file could edit its content and recompute a matching digest — the
same limitation the basis and memory-candidate ledgers accept. Its job is to catch an accidental
partial write or bit rot, not a deliberate adversary. A corrupted or truncated file fails closed
with `ProducerReservationError::LedgerDigestMismatch`, propagated through `State::new`'s `?` rather
than panicking — daemon startup fails rather than silently trusting a damaged journal.

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
6. a corrupted or truncated ledger digest fails `State::new` closed;
7. the ledger file is `0600` on Unix;
8. every `Serialize + Deserialize` type here has a round-trip test;
9. `with_reservation` releases with the receipt on success and with the failure recorded (not left
   open) when the closure errors, and reports success (not the release failure) when the side
   effect succeeds but the release persist itself fails;
10. `reserve`/`release`/`close_reservation_needs_rerun` leave the live ledger unchanged (no phantom
    open reservation, no silent release) when the underlying persist fails — forced by chmod'ing
    `.impulse/` read-only mid-test — and a corrected retry succeeds once it is writable again;
11. an open reservation strictly behind an incoming `reserve()` call's revision is closed as
    `NeedsRerun` rather than blocking, while two attempts at the exact same revision still conflict
    (the documented residual gap).

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
