---
title: Producer Reservation Journal (state layer)
description: Work card for claude-producer-reservation-journal-20260902 (Stage 3 state half, ADR-0012 amendment)
updated: 2026-09-02
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, governed-tasks, producers, crash-safety, state, adr-0012]
---

# Producer Reservation Journal (state layer)

## Lane Facts

- Owner: Claude (Fable 5.1), Stage 3 (state layer) of
  `docs/plans/2026-09-02-impulse-next-stages.md`.
- Role: implementation lane for one durable state-layer primitive. Handler wiring
  (`RunGovernedVerification`/`RunGovernedSupervisorReview` adopting `with_reservation`) is
  explicitly out of scope — Stage 3's handler half is the next lane, blocked on Stage 2's merge
  window per the plan.
- Branch: `claude/producer-reservation-journal-20260902`.
- Worktree: `.worktrees/producer-reservation-journal-20260902` (repository-relative).
- Base: `origin/main` at `36bda00`.
- Owned paths:
  - `impulse-rs/src/state/producer_reservation.rs` (new)
  - `impulse-rs/src/state/mod.rs` (module registration + re-export)
  - `impulse-rs/src/state/persistence.rs` (new `State` field, load + reconcile at `State::new`)
  - `impulse-rs/src/state/governed_task.rs` (minimal additive diff only — see below)
  - `impulse-rs/impulse-ops/src/governed_task.rs` (minimal additive diff only — see below)
  - `impulse-rs/.gitignore` (one new line: `.impulse/PRODUCER_RESERVATIONS.json`)
  - `docs/decisions/0012-daemon-owned-governed-runtime-producers.md` (amendment section +
    `updated:` front matter)
  - `docs/superpowers/specs/2026-09-02-producer-reservation-journal.md` (new)
  - `CONTEXT.md` (one glossary entry: "producer reservation")
  - this work card
- Blocked/shared paths (not touched): `impulse-rs/src/daemon/**`, `impulse-rs/src/governed_producers.rs`,
  `impulse-rs/impulse-desktop/**`, `.github/**`, `Cargo.toml`/`Cargo.lock`, `CLAUDE.md`, `AGENTS.md`.
- Plan/spec: `docs/superpowers/specs/2026-09-02-producer-reservation-journal.md` and the ADR-0012
  amendment above.
- Verification (isolated `CARGO_TARGET_DIR`, per the shared-cargo-target-dir memory note):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 ../docs/validate_docs.py --all` (from inside `impulse-rs/`).
- Latest status: implementation complete and gated on this lane. `cargo build --workspace` clean
  (includes `impulse-desktop`, confirming the new `GovernedTaskEventKind`/`GovernedTaskMutation`
  variants are additive and do not break any exhaustive match outside this lane's owned files —
  verified there are none). `cargo clippy --workspace --all-targets -- -D warnings` clean.
  `cargo fmt --all -- --check` clean. `cargo test --workspace`: **2325 passed / 0 failed / 9
  ignored**, including all 15 new `state::producer_reservation::tests`.
  `python3 ../docs/validate_docs.py --all` reports only the 4 pre-existing failures (ADR-0014
  `proposed` status, `LONG-RANGE-ENHANCEMENTS.md` and the two `RUST-MULTI-AGENT-*` guides stale) —
  no new failures from this lane's docs.

## Decisions

- 2026-09-02: `ReservationId` is a type alias over the existing
  `impulse_ops::governed_task::GovernedRecordId` rather than a new newtype — reservations are
  governed-task-adjacent records and the existing id type already carries the validation and
  `Ord`/`Hash` a `BTreeMap` key needs. Less new surface area for the same guarantee.
- 2026-09-02: Doc paths corrected mid-session. The repo carries two parallel `docs/` trees — a
  stale one nested at `impulse-rs/docs/` (last touched 2026-04-02, no `docs/plans/worktrees/`
  subdirectory) and the actively-used one at the worktree root (sibling of `impulse-rs/`, matching
  `CLAUDE.md`'s own `../docs/validate_docs.py` gate command run from inside `impulse-rs/`). The
  spec was drafted at the stale nested path by mistake and moved to the worktree-root
  `docs/superpowers/specs/` before this card was written; nothing was left behind at the stale
  path.
- 2026-09-02: The "distinguishable from a fresh one" acceptance requirement (a replayed request
  against a needs-rerun reservation must be observably different from a first-time request) is
  satisfied by a `pending_rerun_reason(task_id, producer, request_id)` query method rather than by
  changing `reserve`'s return type. This keeps `reserve() -> Result<ReservationId>` exactly as
  specified while still making the distinction observable and testable.
- 2026-09-02: `ReservationOutcome::Released` carries a `receipt_ref: String` field (the original
  two-variant sketch in the assignment showed `Released` as a unit variant, but `release(id,
  receipt_ref)`'s own signature implies somewhere to put the receipt reference — folding it into
  the outcome keeps the journal entry self-describing without a separate field on
  `ProducerReservation`).
- 2026-09-02: `GovernedTaskMutation::NoteProducerReservationInterrupted` reconstructs on reload
  from `event.actor`/`event.detail` alone (a single prepared string), mirroring the existing
  `MarkLaunchFailed`/`MarkRuntimeExited` pattern rather than the richer
  claim/verification/verdict pattern — the note carries no separate stored record type, so there
  is nothing to cross-reference by id on replay.
- 2026-09-02: `with_reservation`'s closure contract requires the side effect *and* its
  governed-task mutation persist together, returning `(value, receipt_ref)` only once both are
  durable. Releasing right after the raw side effect (before its mutation lands) would reopen the
  exact gap ADR-0012 names — this is spelled out in the function's own doc comment and in the spec
  so the handler lane adopts it correctly.

## Changes

- New module `impulse-rs/src/state/producer_reservation.rs`: `ProducerKind`, `ReservationOutcome`,
  `ProducerReservation`, `ReservationId` (alias), `ProducerReservationError`,
  `ProducerReservationLedger` (schema-versioned, whole-file-digest-verified, private/atomic via
  the existing `Storage::write_private_json`/`read_json` helpers). `State` methods: `reserve`,
  `release`, `open_reservations`, `pending_rerun_reason`, `reconcile_producer_reservations`
  (`pub(super)`, invoked once from `State::new`). Free async helper `with_reservation`.
- `impulse-ops/src/governed_task.rs`: additive
  `GovernedTaskEventKind::ProducerReservationInterrupted` and
  `GovernedTaskMutation::NoteProducerReservationInterrupted { actor, reason }` variants. No
  existing variant or field changed.
- `src/state/governed_task.rs`: three additive touches — one match arm in `apply_mutation` (pure
  event append, no execution/review state transition), one match arm in the ledger-reload replay
  validator (reconstructs the mutation from the event), and `governed_project_id` widened from
  private to `pub(crate)` (one keyword) so the sibling module can resolve the project id.
- `src/state/mod.rs` / `src/state/persistence.rs`: module registration, new `State` field, load +
  reconcile wiring in `State::new`.
- `.gitignore`: `.impulse/PRODUCER_RESERVATIONS.json` (the journal is a private, owner-only,
  process-local ledger — not intended to be committed, matching the ephemeral-state entries
  already listed there; `GOVERNED_TASKS.json`/`MEMORY_CANDIDATES.json` predate this convention and
  were left as-is since editing shared `.gitignore` conventions for other lanes' files is out of
  scope here).

## Tests

- `state::producer_reservation::tests` (15 tests): reserve/release round trip through a full
  `State` reload; duplicate-open-reservation rejection for the same task+producer, and that a
  different producer on the same task is allowed concurrently; release on an unknown or
  already-released reservation fails with a typed error; a simulated crash (reserve, drop `State`
  without releasing, reload) leaves the reservation closed as `NeedsRerun` with the reservation id,
  interrupting request id, and reason all present on the governed task's own event chain;
  reconcile is idempotent across repeated `State::new` calls (no duplicate event, no extra
  revision bump); a replayed request against a needs-rerun reservation is distinguishable via
  `pending_rerun_reason`, and the retry itself is not blocked; a forged or truncated ledger digest
  fails `State::new` closed (two separate tests); the ledger file is `0600` on Unix; serde
  round-trip tests for `ProducerReservation`, `ProducerKind` (all three variants), and
  `ReservationOutcome`; `with_reservation` releases with the receipt on success and with the
  failure recorded (not left open) when the closure errors, proven by then successfully
  re-reserving the same triple.

## Handoff Notes

- **Handler wiring (the actual crash-safety payoff) is not done here.** The spec's "Handoff:
  handler wiring" section gives the exact adoption shape for `RunGovernedVerification`
  (`src/daemon/handlers.rs`, ~1233-1270) and `RunGovernedSupervisorReview` (~1913-2000): wrap
  `governed_producers::run_verification`/the Supervisor turn *and* the `mutate_governed_task` call
  that records its receipt inside `with_reservation`'s closure, keeping the existing
  `acquire_governed_producer_lock` guard as the in-memory concurrent-request optimization it
  already is. Until that lands, `docs/decisions/0012-daemon-owned-governed-runtime-producers.md`'s
  amendment explicitly says the gap is now observable (an interrupted reservation is durably
  visible and annotated) but not yet load-bearing (a replayed request after a crash still reruns
  the side effect).
- **Doc-tree duplication discovered, not fixed.** `impulse-rs/docs/` and the worktree-root
  `docs/` are both real, git-tracked trees with overlapping subdirectory names
  (`superpowers/specs/`, `plans/`) but different, non-overlapping content — the nested one appears
  stale (no `docs/plans/worktrees/`, last entry 2026-04-02). This cost a mid-session
  misplace-and-move (see Decisions). Worth a cleanup pass to either retire or clearly annotate the
  nested tree, but that is out of scope for this lane and was not touched beyond moving the one
  file I had misplaced.
- A concurrent lane (`ion-tool-floor-20260902`, different worktree, its own isolated
  `CARGO_TARGET_DIR`) was running its own full-workspace `cargo test` at the same time; both
  processes briefly serialized on cargo's shared package-cache file lock (not a
  `CARGO_TARGET_DIR` collision — target dirs were correctly isolated) and both completed cleanly.
- **`impulse-rs/docs/decisions/0011-0013` initially appeared to exist under `impulse-rs/docs/`
  during early exploration this session**, then did not on a later, more careful check from the
  same working directory. Filesystem state during a multi-agent session with concurrent worktree
  activity should be treated as a snapshot, not a constant — always re-verify a path immediately
  before writing to it rather than trusting an earlier read in the same session.
