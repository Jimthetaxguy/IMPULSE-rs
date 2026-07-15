---
title: "ADR-0011: Daemon-Owned Governed Task Run Lifecycle"
status: accepted
created: 2026-07-13
deciders: [Impulse Maintainers]
---

# ADR-0011: Daemon-Owned Governed Task Run Lifecycle

## Status

Accepted.

## Context

ADR-0010 made role/task compatibility an authoritative pre-PTY gate, but the explicit task remains a
live runtime fact. Desktop telemetry expires, process exit is currently represented as completion in
some surfaces, verification is client-owned and discarded, and supervisor actions do not produce a
durable judgment tied to evidence. Those gaps make it possible to confuse a worker claim or exited
terminal with accepted work.

Impulse's operating contract distinguishes four forms of truth:

1. a worker claim;
2. recorded verifier evidence;
3. supervisor judgment;
4. operator approval.

They must share one daemon-owned identity without collapsing into one status or one review queue.

## Decision

Adopt a daemon-owned governed task-run record with the following rules:

1. **Registration precedes execution.** A governed desktop launch registers its immutable task,
   canonical project/workspace, role compatibility snapshot, runtime, and agent routing identity after
   preflight but before agent-id reservation or PTY creation. Registration failure blocks launch.
2. **Identity domains stay separate.** A governed task ID is not an agent ID or session ID. One task
   may outlive a terminal incarnation. The current slice keeps its assigned agent/runtime immutable;
   cross-runtime resume or reassignment requires a later revisioned contract.
3. **Lifecycle mutations are revisioned and idempotent.** Every mutation supplies `expected_revision`
   and a caller-generated request ID. The daemon serializes mutations, rejects stale revisions, and
   treats a replayed request ID as the same operation.
4. **Truth layers remain typed.** Worker completion claims, verification evidence, supervisor
   verdicts, and operator decisions are separate records with actor, timestamp, subject revision, and
   provenance.
5. **Process exit is not acceptance.** A launch failure or runtime exit records execution state while
   preserving the task. Neither state implies claim, verification, judgment, or approval.
6. **Acceptance requires evidence.** A supervisor may recommend acceptance only against the latest
   passing verification for the claimed subject revision. Failed, inconclusive, absent, or stale
   verification blocks that recommendation.
7. **The first approval policy is explicit operator approval.** `recommend_accept` moves a run to
   `awaiting_operator`; only a revision-matched operator `approve` decision moves it to `accepted`.
   The policy is represented in the task spec so future profiles can be added deliberately.
8. **Durable memory promotion remains downstream.** Only accepted runs are eligible for future memory
   promotion. This slice records provenance but does not automatically write durable project memory.
9. **The daemon snapshot is authoritative.** `ProjectOpsSnapshot` carries governed runs with a serde
   default for old payloads. Dioxus renders and acts on that state; transient runtime events may not
   overwrite it.
10. **Evidence is bounded.** The materialized record stores structured command outcomes, content
    digests, byte counts, truncation flags, and project-local references. It never embeds unbounded
    terminal output or inherits the parent process's secret-bearing environment as evidence.
11. **Legacy completion stays explicit.** Existing taskless sessions and `EndSession` remain supported
    as legacy/unverified history. They cannot transition a governed task to accepted.

## Lifecycle

```text
execution: registered -> running -> runtime_exited
           registered -> launch_failed

review: awaiting_claim -> awaiting_verification -> awaiting_supervisor
                                 |                    |
                                 +-> verification_failed
                                                      +-> changes_requested
                                                      +-> escalated
                                                      +-> awaiting_operator -> accepted | rejected

        verification_failed | changes_requested -> awaiting_verification (new claim)
```

The materialized state is a projection of typed records and append-only lifecycle events. The daemon
never infers `accepted` from a terminal signal or free-form text.

## Consequences

- The daemon becomes the single lifecycle authority while artifacts, Ion results, and generic
  verifier reports remain evidence producers or references rather than competing decision stores.
- The desktop needs an acknowledged command client in addition to its asynchronous telemetry sink.
- Protocol v4 adds the governed request family and snapshot fields.
- Runtime adapters can degrade differently, but every governed launch must satisfy registration and
  provenance requirements.
- Strong actor authentication and automated memory promotion remain later contracts; the local Unix
  socket boundary must not be described as cryptographic role identity.
- Evidence producers must redact secret-bearing argv before submission. The daemon validates
  bounded display arguments, SHA-256 digest shape, and project-local references, but cannot prove a
  producer performed complete semantic redaction.
- The desktop writes launch/exit lifecycle intent before daemon I/O to a bounded, owner-only,
  cross-process-locked project outbox; arbitrary review mutations are never replayed automatically.
  Abrupt desktop death before exit intent is generated is not detectable without a future runtime
  lease/orphan-reconciliation contract. A missing task target remains queued because registration
  may still commit; durable tombstones/expiry are required before safe automated deletion.
- Ledger reload replays the typed record/event chain and requires complete per-revision idempotency
  receipts before materialized state becomes authoritative. This detects corruption and incoherent
  edits; it is not an authenticated defense against a same-user attacker rewriting the whole file.
- Governed mutations route through one project-bound daemon writer. In-process CAS plus atomic
  rename is not a multi-daemon file-locking protocol; multi-daemon project access remains invalid.

## Validation

This decision is represented when tests prove:

1. registration happens before PTY creation and failure leaves no reserved runtime id;
2. lifecycle state and events survive daemon reconstruction;
3. stale and cross-project mutations fail without changing revision;
4. worker claim alone and runtime exit never accept a task;
5. supervisor recommendation requires current passing verification;
6. operator approval is explicit, revision-bound, and replay-safe;
7. concurrent decisions produce one winner;
8. daemon snapshots remain the UI source of truth;
9. old snapshots deserialize with an empty governed-run collection; and
10. forged accepted state, broken event chains, missing receipts, and malformed persisted evidence
    fail closed during daemon reconstruction.

## Related Documents

- [`0010-product-role-launch-contract.md`](0010-product-role-launch-contract.md)
- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
- [`../spec/TEST-TRACEABILITY.md`](../spec/TEST-TRACEABILITY.md)
- [`../superpowers/plans/2026-07-13-governed-task-run.md`](../superpowers/plans/2026-07-13-governed-task-run.md)
