---
title: "ADR-0013: Deterministic Accepted-Run Memory Candidates"
status: accepted
created: 2026-07-15
deciders: [Impulse Maintainers]
---

# ADR-0013: Deterministic Accepted-Run Memory Candidates

## Status

Accepted.

## Context

ADR-0011 made an operator-approved governed run the durable acceptance authority. ADR-0012 made the
first profiled claim, verification, and Supervisor producers daemon-owned, while explicitly keeping
semantic-memory promotion downstream. An accepted run is valuable episodic provenance, but it is
not automatically a durable project fact or decision. Copying worker prose or a model rationale
directly into `GENOME.md` would collapse claim, evidence, judgment, and curated memory into one
unreviewed write.

The first memory step therefore needs to be inspectable and recoverable without making semantic
promotion implicit. It also must not couple the governed-task ledger's lifecycle to a second
artifact strongly enough that a missing review projection invalidates an otherwise valid accepted
run.

## Decision

Adopt a deterministic, review-only materialized view of accepted governed runs with these rules:

1. **The governed task remains authoritative.** `.impulse/GOVERNED_TASKS.json` owns acceptance and
   its episodic source chain. `.impulse/MEMORY_CANDIDATES.json` is a separate, owner-only review
   ledger that can be reconstructed from accepted task truth.
2. **One accepted task produces one deterministic candidate.** The candidate ID uses
   `memory-candidate-<sha256>`. The digest hashes the exact JSON bytes emitted from the fixed,
   ordered `CandidateSourceV1` struct; it contains no maps or floating-point values and performs no
   semantic Unicode normalization. Its versioned source binds project/workspace/task identity,
   accepted revision, task-registration text and criteria, runtime/session routing, subject,
   verification policy, record IDs, artifact references, successful command evidence, source
   assurance, and staging timestamp. Schema and derivation versions are explicit.
3. **The proposed text excludes semantic producer prose.** Candidate text is derived from the
   same-user client's task-registration text plus daemon-observed evidence metadata. Worker claim
   summaries, Supervisor rationale, and operator rationale remain only in the referenced governed
   record.
4. **Source assurance is honest.** Candidates distinguish daemon-profiled evidence from
   caller-composed evidence. Both variants describe the operator as declared inside the current
   same-user socket trust boundary; neither claims cryptographic human authentication.
5. **The v1 lifecycle is pending review only.** Every candidate has `pending_review` status. There
   is no promote, apply, dismiss, or edit request in this slice, and Dioxus renders the candidate as
   read-only. The candidate ledger is durable review state, not curated semantic memory.
6. **Operator decisions are terminal.** `accepted` and `rejected` cannot receive another operator
   decision. An accepted task therefore cannot later become rejected and orphan its candidate; in
   an approve/reject race, only the accepted winner is eligible for a candidate.
7. **Acceptance commits before projection.** The daemon persists the governed-task acceptance via
   private-file atomic replacement first, then ensures its candidate through a separate replacement.
   A candidate-write error can therefore be returned after acceptance is already recorded. Replaying
   the same idempotent acceptance request repairs the missing candidate without another transition.
8. **Startup reconciliation repairs absence and rejects contradiction.** Daemon startup derives the
   expected set from every accepted task, inserts missing candidates, and replaces the owner-only
   ledger through the same private-file helper. Orphaned candidates, duplicate task projections,
   malformed records, and digest/content mismatches fail closed.
9. **Project memory is untouched.** Candidate creation, replay, reconciliation, and rendering never
   mutate `.impulse/GENOME.md` or `.impulse/HISTORY.jsonl`, and candidates do not enter retrieval or
   context injection merely because they exist.
10. **Protocol v6 is an additive read-model change.** `ProjectOpsSnapshot.memory_candidates` is
   serde-defaulted for older payloads. V6 adds no candidate mutation request. The Dioxus Memory view
   exposes provenance and the explicit `Pending review — not stored in GENOME` boundary.

## Reconciliation Flow

```text
operator approval
  -> replace accepted governed task + request receipt through private-file helper
  -> derive versioned candidate from accepted task truth
  -> separately replace owner-only MEMORY_CANDIDATES.json

same request replay or daemon restart
  -> revalidate authoritative governed-task chain
  -> derive expected accepted-run candidate set
  -> repair missing candidate
  -> fail closed on orphan or mismatched stored candidate
```

## Consequences

- An accepted run becomes visible to memory reviewers without granting a worker, Supervisor model,
  or acceptance transition an implicit semantic-memory write.
- Separate files avoid making a disposable materialized view part of governed-task event replay,
  but the two-file operation is not transactionally atomic. Replay/startup repair is the explicit
  recovery contract.
- Each private-file helper syncs the temporary file, applies mode `0600`, and renames it over the
  destination. It does not fsync the parent directory, so this decision does not claim full
  power-loss durability or a cross-file transaction.
- Because candidates are deterministic and insert-only, tampering and derivation drift are
  detectable. A future derivation change must bump its version rather than silently rewrite source
  meaning.
- Candidate durability is local daemon state at mode `0600`; it is gitignored alongside governed
  runtime state. `GENOME.md` and `HISTORY.jsonl` retain their existing deliberate project-memory
  behavior.
- Explicit promotion and dismissal require a later ADR covering operator authorization, semantic
  validation, conflict/deduplication policy, audit history, and the exact `GENOME`/retrieval write
  boundary.
- The next integration forcing function remains one process-level launched Builder plus Supervisor
  workflow that reaches operator acceptance and observes exactly one staged candidate.

## Validation

This decision is represented only when tests prove:

1. accepted current passing evidence creates exactly one deterministic pending candidate;
2. worker/Supervisor/operator rationale text cannot appear in the candidate contract;
3. rejected or merely recommended tasks never create a candidate;
4. identical acceptance replay does not duplicate the candidate;
5. startup repairs a missing candidate without changing `GENOME.md` or `HISTORY.jsonl`;
6. malformed, orphaned, duplicate-task, or source-mismatched candidate state fails closed;
7. accepted/rejected decisions are terminal and an approve-then-reject attempt cannot orphan a
   candidate;
8. owner-only file replacement and exact init ignore paths include the candidate ledger and temps;
9. old snapshots deserialize with an empty candidate collection and protocol v6 snapshots expose
   the typed candidates; and
10. Dioxus renders candidate provenance and the pending/not-in-GENOME boundary without a promotion
   action.

## Related Documents

- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`0012-daemon-owned-governed-runtime-producers.md`](0012-daemon-owned-governed-runtime-producers.md)
- [`../../VISION.md`](../../VISION.md)
- [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md)
- [`../spec/TEST-TRACEABILITY.md`](../spec/TEST-TRACEABILITY.md)
- [`../plans/worktrees/2026-07-15-accepted-run-memory-candidates.md`](../plans/worktrees/2026-07-15-accepted-run-memory-candidates.md)
