---
title: "ADR-0012: Daemon-Owned Governed Runtime Producers"
status: accepted
created: 2026-07-13
updated: 2026-09-02
deciders: [Impulse Maintainers]
---

# ADR-0012: Daemon-Owned Governed Runtime Producers

## Status

Accepted.

## Context

ADR-0011 made governed task state durable and kept worker claim, verifier evidence, supervisor
judgment, and operator approval separate. It did not establish where those records come from. The
generic mutation protocol still accepts caller-composed actor, subject, evidence, and verdict
payloads; the desktop injects task identity into a child process but no process can submit a typed
claim; the existing verifier does not bind its commands to the task workspace or a stable subject;
and the current Supervisor controls record a UI-authored judgment rather than a launched agent turn.

That is sufficient for lifecycle contract tests but not for an honest governed-agent workflow.
Authentication and provenance are also different problems. A bearer token copied into a child
environment would prove possession inside the existing same-user boundary; it would not prove that
the holder is the assigned runtime process. This decision therefore improves producer integrity
without representing same-user local actors as cryptographically authenticated.

## Decision

Adopt a first closed-loop producer profile with these rules:

1. **Closed-loop mode is explicit.** A governed registration may select
   `rust_workspace_v1`. The profile requires non-empty acceptance criteria and an initial clean Git
   commit OID, plus the exact canonical Builder role assignment. The daemon recomputes role
   compatibility from its own runtime registry and requires the caller's supplied result to match
   exactly; a caller cannot strengthen or substitute for daemon-observed compatibility. Existing
   registrations without a producer profile retain their manual lifecycle and protocol
   compatibility.
2. **The Builder submits intent, not provenance.** The agent-facing claim request supplies task
   coordinates, expected task revision, summary, and artifact IDs. It does not supply actor identity
   or subject revision. The project-bound daemon derives the assigned Worker actor and observes the
   clean workspace `HEAD` commit.
3. **The first subject type is a clean committed Git OID.** The current workspace root must be a Git
   worktree, `HEAD` must resolve to a commit, tracked changes and Git-visible untracked files must be
   absent, and the claimed OID must descend from the registered initial OID. Exact local `.impulse`
   runtime-state/socket/cache/ledger/outbox paths are ignored or exempted; repository-authored ignore
   rules may also hide untracked files, but those bytes are outside the committed subject and never
   enter the detached verification checkout. Dirty-worktree attestation is a later contract.
4. **The daemon owns automatic verification.** A caller may request verification by task coordinates
   and expected revision only. For `rust_workspace_v1`, the daemon expands a fixed argv profile,
   materializes the claimed commit in a detached Git worktree, executes each fixed command with a
   scrubbed environment, bounded timeout, process-group cleanup, and streaming output digests, then
   derives the Verifier actor and `GovernedCommandEvidence` itself. The commands execute
   host-trusted project code; this is not an OS sandbox.
5. **Verification pins bytes before and after execution.** The daemon requires the clean source
   workspace OID to equal the latest claim before and after verification, and the detached checkout
   must remain clean after the fixed commands. It also compares a bounded byte manifest of every
   detached source-tree entry except Git administration and the external target directory, including
   ignored files. The v1 profile rejects source-tree symlinks. Subject drift or generated files
   cannot produce passing evidence. The Rust profile requires a committed, regular, non-symlink
   root `Cargo.lock`, and dependency-resolving commands run with `--locked`.
6. **Model-reported commands are not execution evidence.** Ion/Pi findings and harness text may
   inform review, but only daemon-observed child-process outcomes become governed command evidence.
7. **The daemon owns automatic Supervisor review.** A caller may request review by task coordinates
   and expected revision only. The daemon supplies bounded task, criterion, claim, and verification
   metadata to the configured API provider in one history-free, tool-free, temperature-zero turn,
   requires one strict JSON envelope bound to the exact task revision, record IDs, subject, and
   acceptance-criteria digest, derives a Supervisor actor bound to the API provider plus a digest
   of the exact resolved request model, and records the verdict. Free-form or
   mismatched output fails without a state mutation; generic external harness mode fails closed
   before spawning.
8. **Automatic producer payloads are not accepted through generic mutation.** For profiled tasks,
   public generic mutations cannot submit claims, verification records, or Supervisor verdicts.
   System lifecycle and explicit operator decisions retain their existing paths.
9. **Ion receives a typed claim tool; external runtimes receive a CLI bridge.** Both use the same
   daemon request. The packaged executable is `impulse-rs`; profiled governed panes preserve its
   exact path through `$IMPULSE_CONTROL_CLI`, while ordinary and unprofiled panes have inherited
   producer-routing variables removed. Every producer invocation includes the global `--daemon`
   flag before the subcommand. This is a transport difference, not a claim of capability parity or
   process authentication.
10. **Operator acceptance remains explicit.** A model Supervisor may recommend acceptance but cannot
    accept the task. The existing operator-required policy remains the only acceptance policy.
11. **Memory remains a separate promotion boundary.** An accepted run is immutable episodic
    provenance and eligibility, not semantic truth. The next memory slice will create a review-only
    promotion candidate; this producer profile never writes `GENOME.md` directly.

## Producer Flow

```text
Desktop preflight
  -> register profile + criteria + clean initial Git OID
  -> launch Builder with project/task/socket routing

Builder / Ion tool
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-claim --summary "..." [--artifact-id "..."]
     or governed_submit_claim({"summary":"...","artifact_ids":[]})
Daemon
  -> observe clean Git OID + assigned Worker actor
  -> record claim

Operator or routed terminal command
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-verify
Daemon
  -> attest subject -> materialize detached commit -> run fixed profile -> attest source subject
  -> derive bounded evidence + Verifier actor
  -> record verification

Operator or routed terminal command
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-review
Daemon
  -> call configured API provider with bounded evidence and no tools/history
  -> strict-parse and bind response IDs
  -> derive provider + exact-model-digest Supervisor actor and record verdict

Operator
  -> approve or reject explicitly
```

## Consequences

- The product gains one structurally governed, process-backed path without pretending every external
  CLI exposes the same controls.
- A clean local commit becomes the v1 checkpoint between implementation and review. Agents that work
  only in dirty trees must commit before claiming completion or remain on the unprofiled/manual path.
- The Rust verification profile is intentionally closed and versioned. Adding Node, Python, or
  project-defined profiles requires another explicit command and secret-handling contract.
- Generic same-user socket clients remain within the local threat model. Future hardening may bind
  Unix peer identity, parent/child PIDs, sandbox identity, or single-use launch capabilities.
  Current actor IDs must not be described as authenticated.
- Command stdout and stderr are streamed into digests and byte counts. Raw output is not retained in
  the governed task ledger. Durable output references require a separate redaction/storage policy.
- API Supervisor review is a bounded model judgment, not command evidence. Strict identity binding prevents
  accidental cross-task application but cannot make model judgment infallible.
- Persisted receipts and the per-task daemon lock deduplicate replay and concurrent requests. They
  do not provide crash-safe exactly-once producer execution: a daemon exit between a command/model
  side effect and durable receipt can repeat that side effect after restart. A durable producer
  reservation journal remains follow-up work.
- Dioxus renders profiled evidence and terminal command guidance; automatic producer buttons require
  a future acknowledged host-command contract.

## Validation

This decision is represented only when tests prove:

1. a profiled registration rejects empty criteria, missing/invalid initial OID, and unsupported
   profiles while old unprofiled payloads still deserialize;
2. claim requests derive the assigned Worker and clean current OID, reject dirty/mismatched/non-Git
   workspaces, and replay idempotently;
3. the verifier runs fixed locked argv in a symlink-free detached worktree with a committed regular
   root lockfile and a scrubbed environment, streaming digests/counts/truncation without production
   previews, timeout, process-group cleanup, and bounded post-kill reaping;
4. failed commands, pre/post subject drift, and ignored generated source-tree bytes cannot produce
   passing verification;
5. callers cannot inject automatic claim actor, subject, command evidence, or Supervisor verdict
   payloads for profiled tasks;
6. Supervisor output must match the exact task/revision/claim/verification/subject tuple and parse as
   one strict envelope;
7. Builder exit without a claim and Supervisor recommendation without operator approval never accept
   the task;
8. a real daemon plus CLI/verification child processes persist the exact `awaiting_supervisor`
   claim/evidence state across restart, while separate in-process daemon handler tests prove strict
   API review and operator-only acceptance; and
9. profiled registration rejects a forged runtime-compatibility result, and ordinary panes cannot
   inherit governed producer routing from the Desktop parent; and
10. the docs keep memory promotion, strong local actor authentication, generalized profiles, and
   external-runtime parity explicit as follow-ups.

## Amendment (2026-09-02): durable producer reservations

The Consequences section above names an open gap: persisted receipts and the per-task daemon lock
deduplicate replay and concurrent requests, but a daemon exit between a producer side effect and
its durable receipt could still repeat that side effect after restart.

The state layer half of that gap is now closed. `impulse-rs/src/state/producer_reservation.rs`
adds a durable, atomic journal (`.impulse/PRODUCER_RESERVATIONS.json`, distinct from
`GOVERNED_TASKS.json`) that records the *intent* to run a producer side effect before it starts.
It carries a whole-file SHA-256 digest checked at every load, which is corruption/truncation
detection, not tamper resistance (an unkeyed hash cannot stop an adversary who can already write
the file — it exists to catch an accidental partial write, not a deliberate one), matching the
guarantee the basis and memory-candidate ledgers already accept:

- `State::reserve(task_id, revision, request_id, producer)` fails closed with a typed
  `DuplicateOpenReservation` error if an open reservation at or after `revision` already exists for
  the same task and producer, so a request replayed while the original side effect is genuinely
  still in flight cannot start a second, competing run. An open reservation strictly behind the
  incoming revision is closed as `NeedsRerun` instead of blocking — it cannot be a live attempt, since
  the caller's higher revision proves the task has moved on. This does not fully close a same-day
  adversarial-review finding: an in-process panic (not a process exit) leaves a reservation open
  with no `State::new` reconcile to clear it, and two attempts at the *exact same* revision still
  conflict until either a real process restart or future operator tooling; the revision-scoped
  check narrows this to that one residual case rather than eliminating it, chosen over an explicit
  operator-facing `clear_stale_reservation` API because it needs no new plumbing. See the design
  spec's "Revision-scoped duplicate check" section.
- `State::release(id, receipt_ref)` closes a reservation once its side effect and governed-task
  mutation are both durably recorded. `reserve`, `release`, and reconcile's internal close-as-
  needs-rerun step all clone the in-memory ledger, mutate the clone, persist the clone, and only
  then swap it into the live state — the same discipline `mutate_governed_task` and the
  memory-candidate ledger already use — so a transient persist failure (a momentarily unwritable
  `.impulse/`, a full disk) leaves the live ledger exactly as it was, never a phantom reservation
  the caller can neither find nor release. An earlier version of this journal mutated the live
  ledger before the fallible persist with no rollback; fixed in adversarial review before this PR
  merged.
- A reservation still open when the process reloads was interrupted before a receipt existed.
  `State::new` reconciles it to `NeedsRerun` and records a note on the owning governed task's own
  event chain (a new, purely additive `GovernedTaskEventKind::ProducerReservationInterrupted` /
  `GovernedTaskMutation::NoteProducerReservationInterrupted` pair in
  `impulse-ops/src/governed_task.rs`), so the operator surface can show it without inventing a
  second source of truth.
- `producer_reservation::with_reservation` is a ready-to-adopt async wrapper: reserve, run a closure
  that performs the side effect *and* persists its governed-task mutation, then release with the
  resulting receipt reference. Releasing before the mutation is persisted would reopen the gap this
  journal exists to close, so the helper's closure contract requires both steps together. It has no
  `catch_unwind` and is not panic-safe: a panic inside the closure leaves the reservation open
  (same as a crash) but propagates through `with_reservation` rather than being caught and
  released, so the handler-wiring lane must treat an in-process panic exactly like a process crash,
  not as a normal `Err` return.

**Not yet done — handler wiring remains follow-up work**, tracked as the next lane in
`docs/plans/2026-09-02-impulse-next-stages.md`'s Stage 3: `RunGovernedVerification` and
`RunGovernedSupervisorReview` in `src/daemon/handlers.rs` do not yet call `with_reservation` around
`governed_producers::run_verification`/the Supervisor review turn. Until that wiring lands, the gap
this amendment describes is observable (an interrupted reservation is now durably visible and
annotated) but not yet load-bearing (a replayed request after a crash still reruns the side effect,
exactly as before). See the design spec for the exact adoption shape.

Design spec: [`../superpowers/specs/2026-09-02-producer-reservation-journal.md`](../superpowers/specs/2026-09-02-producer-reservation-journal.md).

## Related Documents

- [`0010-product-role-launch-contract.md`](0010-product-role-launch-contract.md)
- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
- [`../spec/TEST-TRACEABILITY.md`](../spec/TEST-TRACEABILITY.md)
- [`../superpowers/plans/2026-07-13-governed-runtime-producers.md`](../superpowers/plans/2026-07-13-governed-runtime-producers.md)
- [`../superpowers/specs/2026-09-02-producer-reservation-journal.md`](../superpowers/specs/2026-09-02-producer-reservation-journal.md)
