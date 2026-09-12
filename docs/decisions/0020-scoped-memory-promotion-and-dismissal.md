---
title: "ADR-0020: Scoped Memory Promotion and Dismissal"
description: An operator promotes or dismisses a review candidate; promotion appends to a hash-chained MEMORY.jsonl and regenerates a separate GENOME projection
status: review
created: 2026-09-12
updated: 2026-09-12
type: decision
category: architecture
phase: all
audience: builders
deciders: [Impulse Maintainers]
tags: [adr, memory, governance, provenance, retrieval]
---

# ADR-0020: Scoped Memory Promotion and Dismissal

## Status

Proposed and implemented on lane `claude/memory-promotion-adr0020-20260912`; accepted on merge.
Closes VISION step 10's state layer and the explicit "later ADR" ADR-0013 defers to: "explicit
promotion and dismissal require a later ADR covering operator authorization, semantic validation,
conflict/deduplication policy, audit history, and the exact `GENOME`/retrieval write boundary."
Also closes ADR-0018 follow-up 2 (`prune_superseded_derivations` must become a status-preserving
migration).

The daemon endpoint, the Dioxus Memory view's Promote/Dismiss controls, and the Ion `memory_search`
tool are deliberately **not** in this decision's implementation. They are handoffs, specified here
so the lanes that build them do not have to reopen these questions.

## Context

ADR-0013 stopped one step short on purpose. An accepted governed run projects exactly one
deterministic, review-only candidate into `.impulse/MEMORY_CANDIDATES.json`, and rule 5 froze the
lifecycle there: "every candidate has `pending_review` status. There is no promote, apply, dismiss,
or edit request in this slice." Rule 9 drew the matching line on the read side: a candidate never
mutates `GENOME.md` or `HISTORY.jsonl`, and "candidates do not enter retrieval or context injection
merely because they exist."

That leaves the review queue with no exit. Every accepted run accumulates a candidate no one can
act on, and the project's actual durable memory — `GENOME.md`, written by `impulse memory add` —
has no relationship to the governed evidence chain at all.

Three things make the missing step harder than "set a flag":

- **A decided status changes what a re-derivation means.** ADR-0018's follow-up 2 states the hazard
  precisely: `MemoryCandidateLedger::load` drops candidates at a superseded derivation version so
  reconciliation can re-derive them under the new one. That is lossless only while the status enum
  has one variant. The moment a candidate can be `Promoted`, a prune silently reverts an operator's
  decision — and orphans whatever durable record the promotion produced.
- **A promoted fact must be harder to forge than a pending one.** The candidate ledger is
  reconstructible from governed-task truth, so ADR-0013 could afford to treat tampering as
  "fail closed and re-derive". A promoted record is *not* reconstructible: it is a human judgment
  that this particular accepted run is worth remembering. Losing it, or silently altering it, is
  data loss.
- **Four different artifacts are now in play** — raw candidates, promoted records, the rendering
  people read, and the hand-curated GENOME — and the temptation to collapse them is exactly what
  the `do-not-unify` rule exists to stop.

Two research lines informed the shape. Zep/Graphiti (arXiv:2501.13956) argues for bitemporal
memory: a fact carries when it became valid separately from when it was recorded, and supersession
is a new assertion rather than an in-place edit. A-MEM (arXiv:2502.12110) argues the opposite of
what a naive implementation reaches for — memory notes should stay atomic and individually
addressable, with links between them, instead of being merged into one evolving blob. Both point
the same way: an append-only log of individually identified records, with a derived view for
reading, and no destructive rewrite.

The `wire-gate` rule supplies the authorization frame: a mutation of live state needs an approval
*before* the mutation and an invariant check *after*, and the post-state must be derivable from the
pre-state plus the approved mutation. ADR-0018 already built the "before" half for governed
acceptance. This decision reuses it verbatim rather than inventing a second authorization model.

## Decision

### Artifacts and boundaries

1. **Four artifacts, none merged into another.** `.impulse/MEMORY_CANDIDATES.json` is the private
   (mode 0600) raw review ledger. `.impulse/MEMORY.jsonl` is the append-only, hash-chained log of
   promoted records and is authoritative for what was promoted. `.impulse/GENOME_PROJECTION.md` is
   a derived, regenerated-wholesale rendering of the currently valid records. `.impulse/GENOME.md`
   is the pre-existing hand-curated `memory::Genome`. Each has one owner and one writer; a query
   that spans two of them is a federation, never a merge.

2. **`GENOME.md` is not regenerated, and this is a deliberate deviation from the staged plan's
   one-line sketch ("GENOME regenerated as a projection").** `GENOME.md` today holds decisions,
   preferences, and constraints an operator wrote by hand through `impulse memory add`.
   Regenerating it from promoted records would delete every one of them on the first promotion —
   a destructive rewrite of operator-authored content, which no amount of "it is a projection"
   framing makes acceptable. The projection therefore gets its own file. `GENOME.md` keeps its
   existing behavior and its existing writer, and ADR-0013 rule 9's promise that promotion "never
   mutates `GENOME.md`" survives intact rather than being quietly reversed. Merging the two
   artifacts is a separate decision with its own migration, and is not taken here.

3. **`MEMORY.jsonl` and `GENOME_PROJECTION.md` are tracked; the candidate ledger is not.** A
   promoted record is curated project memory in the same class as `GENOME.md` and `HISTORY.jsonl`,
   so it belongs in review. The raw candidate ledger and the retrieval-index marker
   (`.impulse/MEMORY_INDEX.json`) stay in the runtime gitignore list.

3a. **Neither file is a governed subject change.** Because they are tracked and *not* gitignored,
   a promotion would otherwise make the next governed registration refuse the workspace as dirty —
   first as an untracked `??`, and after the first commit as a tracked ` M` forever. Both paths are
   therefore exempt in `governed_producers::status_contains_subject_change`, in the untracked arm
   **and** in the tracked arm, which until now treated every tracked mutation as a subject change.
   The exemption is sound for exactly these two paths and no others: they are written only by the
   daemon's own decision path, a launched Builder is never given a way to write them (its
   `IMPULSE_*` environment is scrubbed and it has no promotion request), and their content is
   digest-chained, so a Builder that did somehow write them would fail the log's own load rather
   than smuggle a fact into an accepted run. The exemption is narrowed to modification and addition
   status codes — a **deletion** of the memory log is still a subject change, because destroying
   evidence is not a promotion side effect.

3b. **A fresh clone has the log but not the ledger.** `MEMORY_CANDIDATES.json` is gitignored and
   `MEMORY.jsonl` is not, so a clone legitimately arrives with a full memory log and no ledger at
   all. That is not the same state as a ledger that commits nothing, and reading it that way would
   make every clone of a project with more than one promoted record fail to start. When the
   candidate ledger file is **absent**, the verified log is adopted wholesale as committed, its head
   is recorded, and the orphan direction of the cross-check is skipped — a machine with no ledger
   has no basis on which to call a committed record orphaned. When the ledger file is **present**,
   its head (or its absence of one) is authoritative, exactly as before.

### The record

4. **`MemoryRecord { id, schema_version, kind, scope, project_id, source, request_id,
   based_on_ledger_revision, title, body, valid_from, superseded_by, digest }`.** `source` is
   `CandidateRef { candidate_id, candidate_source_digest, governed_task_id }` or
   `OperatorManual { note }`. `kind` is `accepted_run_outcome` or `operator_note`, and must agree
   with `source` — a candidate-backed record cannot claim to be an operator note.

5. **`scope` is declared as `project | workspace | global`, and only `project` is writable.** The
   other two are refused at the write boundary today. Declaring them now means a later stage widens
   a check rather than migrating every persisted record.

6. **The identity digest covers content, not the clock.** `digest` is
   `sha256-mem-v1:<hex>` over a fixed, ordered `MemoryRecordSourceV1` — no maps, no floats, no
   Unicode normalization, the same discipline as ADR-0013's `CandidateSourceV1` — and `id` is
   `memory-record-<hex>` built from the same hex, so the two can never drift. `valid_from` is
   deliberately **excluded**: a decision request replayed after a crash then re-derives the same
   record id even though the wall clock moved. `valid_from` is still tamper-evident, because the
   log entry digest covers the whole serialized line.

7. **`superseded_by` is reserved and always `None` in this stage.** Supersession is a later log
   entry kind, not an in-place edit of an append-only line. The projection already filters
   superseded records, so the reader side is ready when the writer side lands.

8. **Record text carries no producer prose.** The body is built from the candidate, which ADR-0013
   rule 3 already restricted to registration-time task text plus daemon-observed evidence metadata.
   Worker claim summaries, Supervisor rationale, and operator rationale stay in the governed record.

### The log and its chain

9. **`MEMORY.jsonl` is a hash chain.** Each line is
   `MemoryLogEntry { seq, previous_digest, record, entry_digest }`, where `entry_digest` is
   `sha256-memlog-v1:<hex>` over a fixed, ordered `{seq, previous_digest, record}` source and
   `previous_digest` is the preceding entry's digest — or the constant
   `sha256-memlog-v1:genesis` for the first entry, so "no predecessor" is a value the verifier
   checks rather than a case it skips. Editing any byte of any record, reordering two lines, or
   cutting the file mid-line breaks the next link, and load refuses the file.

9a. **The chain is unkeyed SHA-256, and detects corruption, not forgery.** "Tamper-evident" here
    means self-consistency: no single edit to `MEMORY.jsonl` survives, because the next link, the
    entry digest, the record's own identity digest, and the ledger's committed head all have to
    agree. It does **not** mean authenticity against an actor who can write both artifacts — such an
    actor can rewrite the last entry, reseal its digests, update the ledger head and the candidate's
    `record_id`, and the result loads clean. That is the same boundary every other private ledger in
    this system already has (ADR-0018: the capability proves who opened a connection, nothing about
    the integrity of local state a same-uid process can rewrite), and closing it needs a key the
    daemon holds and a same-uid process cannot read — which this decision does not attempt.

10. **The committed head lives in the *other* artifact.** A hash chain is still a valid chain after
    its last lines are cut off, so the candidate ledger records
    `memory_log_head { entry_count, head_digest }`. Load compares the two. This is federation, not
    merge: each artifact is the witness that catches tampering with the other.

11. **Load fails closed, and never panics.** A malformed line, a broken link, a forged record
    digest, a log shorter than the committed head (trailing truncation), a head-digest mismatch, an
    uncommitted tail longer than one entry, a duplicate record id, a committed record no promoted
    candidate claims, or a promoted candidate naming a record that is not in the log — each is a
    distinct typed `MemoryLogError`, and each stops the daemon (or the direct-CLI command) from
    starting. **The operator-visible surface of that failure is the start-up error itself**, naming
    `.impulse/MEMORY.jsonl` and which corruption was hit. Serving a silently shortened memory is
    worse than not starting.

12. **A decision appends first and commits the ledger second, and an interrupted decision blocks
    the next one.** A process killed between the two steps leaves exactly one *uncommitted* trailing
    entry, which the projection and the retrieval index both ignore. The reverse order would instead
    leave a promoted candidate pointing at a record that does not exist, which the ledger alone
    cannot repair.

    Recovery runs through the **decision path**, not the receipt path: the receipt is written by the
    ledger commit, so after a crash no receipt exists and the replay branch cannot see the request at
    all. Instead, the uncommitted tail entry is matched by the `request_id` carried on its record.
    Replaying that exact request id re-derives the record and compares ids — and because the identity
    digest covers the payload but excludes `valid_from`, a genuine replay derives exactly the tail's
    id while any other payload under the same id does not, and is refused as an idempotency
    conflict. On a match the entry is adopted: the ledger commits with the tail's head and a receipt,
    and nothing is appended. The adopted record keeps the `valid_from` of its original append, and
    the decision records that same instant, because that is when the decision happened. A dismissal
    carrying an interrupted promotion's request id is likewise a payload conflict — only a promotion
    ever appends. Any *different* request is refused with a typed error naming the request id to
    replay.

12a. **Two uncommitted entries is corruption, and its recovery is manual and named.** One is the
    only count an interrupted decision can produce, so more means the file was written outside
    Impulse and the load fails closed. There is deliberately **no API to discard a log tail**:
    an endpoint that truncates an append-only evidence log is exactly the primitive this decision
    exists to avoid handing out. The manual step is stated by the error and is safe by construction,
    because an uncommitted entry is by definition referenced by nothing: keep the first
    `memory_log_head.entry_count` lines of `.impulse/MEMORY.jsonl` (the count is in
    `.impulse/MEMORY_CANDIDATES.json`) and discard the rest; with no head recorded and no ledger,
    remove the local candidate ledger instead and let rule 3b adopt the log.

### The projection

12b. **Record text is Builder-influenced, so the projection escapes it.** A candidate's task and
    acceptance criteria come from registration and flow into the record's title and body, and
    `validate_text` permits newlines — so an unescaped body could emit a structurally valid second
    record section and a reader (or a runtime memory tool) would parse a fabricated fact as
    promoted. The title is collapsed to one line with a leading Markdown structural character
    escaped, and the body is rendered inside a fence whose backtick run is one longer than the
    longest run in the body itself. That is deterministic — the projection must stay byte-identical
    across runs, so a random nonce is not available — and CommonMark closes a fence only on a run at
    least as long as the one that opened it, so the body cannot close its own fence. The injected
    text is preserved verbatim inside the fence: neutralized, not censored.

### The decision

13. **`MemoryCandidateStatus::{PendingReview, Promoted { record_id, decided_at, decided_by },
    Dismissed { reason, decided_at, decided_by }}`, serde-defaulted to `PendingReview`.**
    `PendingReview` keeps its pre-ADR-0020 wire encoding (`"pending_review"`), so an existing ledger
    loads unchanged, and the decided variants are struct-tagged so the two encodings can never be
    confused. A dismissal reason is required to be nonblank: a dismissal leaves no durable record
    behind, so its reason is the only surviving account of why an accepted run was refused.

14. **`prune_superseded_derivations` becomes `migrate_superseded_derivations`.** A candidate at a
    superseded derivation version is still removed from the map so reconciliation can re-derive it,
    but a decided status is parked under the candidate's **governed task id**, which survives
    re-derivation, and reapplied to the freshly derived candidate. The promoted record keeps naming
    the candidate it was actually promoted from — the log is append-only and is not rewritten — so
    the cross-check requires only that every promoted candidate finds its record, never that a
    record's `source.candidate_id` still exists.

14a. **A parked decision whose task disappears is kept, not cleared.** If a migration parks a
    decision and the accepted governed task is then gone, the decision cannot be reattached to any
    candidate. Silently dropping it would leave its record in the log with nothing claiming it, and
    the cross-check would then refuse start-up with an orphan error pointing nowhere. The parked
    entry is kept and logged at warn, and the orphan error names both the record and the governed
    task it was promoted from. A parked *pending* status carries no information and is dropped.

15. **A review decision is terminal, and only covers a candidate that still matches its evidence.**
    A second, differing decision on a decided candidate is refused. A promotion is refused when the
    stored candidate is no longer byte-identical (ignoring status) to the current deterministic
    derivation from accepted governed-task truth — the wire-gate approval-invariant: the approval
    must still cover the thing being approved.

16. **`MemoryCandidateDecisionInput { request_id, project_id, candidate_id, decision, actor,
    expected_ledger_revision }` carries no authentication field, and refuses one.** It is
    `deny_unknown_fields`. The daemon stamps `OperatorAuthentication` onto the persisted
    `MemoryCandidateDecision` from the *connection class*, exactly as ADR-0018 does for
    `OperatorDecisionInput`/`OperatorDecision`. An in-process or direct-CLI caller gets the
    serde-default `Declared`.

17. **Idempotency and concurrency follow the governed-task discipline.** `expected_ledger_revision`
    is a compare-and-swap against a new ledger `revision`; `request_id` keys a receipt holding the
    full decision. A replayed request id with a different payload is refused; a replayed request id
    with the same payload returns the recorded outcome with `replayed: true` and appends nothing.

18. **Only a promotion can dirty the retrieval index.** Promoted records are indexed into their own
    `memory_records`/`memory_fts` tables — never into `genome_decisions`, which belongs to the
    hand-curated GENOME — and the indexer is fed the *projection*, so a pending or dismissed
    candidate cannot reach the index at all. ADR-0013 rule 9 is held by construction rather than by
    a check. The state layer never opens SQLite inside the ledger lock: it writes
    `.impulse/MEMORY_INDEX.json` (`projection_digest` vs `indexed_digest`), and the indexer stamps
    it clean. An empty projection is treated as trivially indexed.

### Authorization (handoff contract)

19. **`DecideMemoryCandidate` is operator-class only.** The request and ack types ship in
    `impulse-ops/src/memory_wiring.rs` with no handler. Whoever wires it must gate it exactly as
    `RecordOperatorDecision` is gated — refused on a connection that has not presented this daemon
    run's operator capability — stamp the authentication from the connection, and leave the ledger,
    the log, and the projection byte-identical on refusal.

## Flow

```text
operator promotes candidate C at ledger revision R
  -> validate input (nonblank reason for a dismissal; project boundary)
  -> re-derive the expected candidate set from accepted governed-task truth
  -> receipt lookup on request_id  --replay--> return recorded outcome, append nothing
  -> CAS ledger.revision == R
  -> C must be pending, and must still match its derivation
  -> refuse if an uncommitted tail entry from another request exists
  -> derive record (identity digest excludes valid_from)
  -> append sealed entry to MEMORY.jsonl            <- side effect 1
  -> replace MEMORY_CANDIDATES.json: status, revision R+1, new head, receipt   <- side effect 2
  -> regenerate GENOME_PROJECTION.md, mark MEMORY_INDEX.json dirty

replay of an interrupted request id (no receipt exists: the ledger never committed)
  -> the uncommitted tail entry's record carries the request id; re-derive and compare ids
  -> ids match   -> adopt: commit the ledger with the tail's head + a receipt, append nothing
  -> ids differ  -> idempotency payload conflict

daemon restart
  -> reconcile candidates from governed-task truth (status carried across migrations)
  -> load + verify MEMORY.jsonl; ledger absent -> adopt the log wholesale (fresh clone)
                                 ledger present -> compare against its committed head
  -> fail closed on any mismatch
  -> cross-check promoted statuses against committed records (skipped when no ledger exists)
  -> regenerate the projection; never mark the index dirty
```

## Consequences

- An accepted run can finally leave the review queue, in both directions, without any producer or
  transition gaining an implicit semantic-memory write.
- A promoted record is tamper-evident and truncation-evident, and the failure mode is a refused
  start rather than a quietly shortened memory. That is a real availability cost: a corrupted
  `MEMORY.jsonl` stops the daemon until a human looks at it. It is the right trade for the one
  artifact in this system that cannot be re-derived.
- The crash window between the two side effects is narrowed, not closed. It leaves a recoverable
  uncommitted tail rather than an unrecoverable dangling reference, and it blocks further decisions
  until the interrupted request is replayed — which means a client that never retries wedges the
  promotion path until someone replays it or applies rule 12a's manual step. A durable two-phase
  reservation (the same gap CLAUDE.md already names for governed producers) would close it, and is
  not taken here.
- The chain detects corruption, not forgery (rule 9a). An actor with write access to both
  `.impulse/MEMORY.jsonl` and `.impulse/MEMORY_CANDIDATES.json` can fabricate a promoted record that
  loads clean and reaches the projection. Everything downstream of the projection — including any
  runtime memory tool — inherits that boundary and must not be described as reading authenticated
  memory.
- `status_contains_subject_change` now has a tracked-path exemption where it previously had none
  (rule 3a). That is a real widening of what counts as a clean governed subject, justified only by
  those two daemon-owned digest-chained paths; any future addition to that arm deserves the same
  scrutiny.
- Keeping the projection out of `GENOME.md` means a reader now has two places to look for project
  memory. That is the honest state of the system: one artifact is hand-written, one is promoted
  from evidence, and pretending otherwise would erode provenance. Unifying them is future work with
  its own migration.
- `MemoryCandidateStatus` is no longer `Copy`. Every match on it must handle three variants; the
  Dioxus Memory view renders the two new ones read-only.
- The ledger schema version moves 1 -> 2. Every new field is serde-defaulted, so a v1 ledger loads
  unchanged and is rewritten at v2 on its first persist. The retrieval schema version moves 2 -> 3,
  additively.
- `workspace` and `global` scopes exist in the type and are refused at the boundary. A later stage
  widens one check; it does not migrate stored records.
- Not adopted here: supersession and editing of promoted records, semantic deduplication or
  conflict detection between records, embeddings for promoted records, cross-project promotion,
  context injection of promoted records, and any promotion path that does not start from an
  accepted-run candidate.

## Verification

This decision is represented when tests prove:

1. every new type round-trips through serde; a candidate persisted without a `status` key loads as
   `PendingReview`; `"pending_review"` keeps its pre-ADR-0020 encoding; and a persisted
   `MemoryCandidateDecision` without an `authentication` key loads as `Declared`;
2. `MemoryCandidateDecisionInput` refuses a client-supplied `authentication` field, and refuses a
   blank, whitespace-only, or oversized dismissal reason;
3. a promotion appends exactly one log entry, marks the candidate `Promoted`, renders the
   projection, and leaves the retrieval index dirty; a dismissal appends nothing, writes no
   `MEMORY.jsonl`, and leaves the index clean;
4. replaying the same request id returns the recorded outcome, appends no second entry, and leaves
   the log byte-identical; replaying it with a different payload is refused;
5. a second decision on a decided candidate, a revision conflict, an unknown candidate, and a wrong
   project are each refused with their own typed error, and every error variant's `Display` names
   what went wrong;
6. a candidate that no longer matches the current deterministic derivation is refused;
7. a tampered `MEMORY.jsonl`, a whole-line truncation, a partial final line, a head-digest
   mismatch, and two uncommitted trailing entries each fail closed at load without panicking;
8. an orphan committed record and a promoted candidate naming a missing record each fail closed;
9. a v1 ledger whose candidate sits at the previous derivation version and carries a `Promoted`
   status migrates forward: the decision survives, the record is not orphaned, and the candidate is
   re-derived at the current version;
10. the projection is byte-identical across two renders and two writes, and its digest is stable;
11. against the real SQLite retrieval index in a temp `IMPULSE_HOME`, a pending candidate is not
    searchable and indexes zero records, a promoted record is searchable by id, and a dismissed
    candidate never reaches the index;
12. a promoted record's identity digest is independent of `valid_from` and sensitive to content,
    request id, and ledger revision; and the record contract carries no worker, Supervisor, or
    operator prose;
13. an interrupted decision (append landed, ledger rolled back) reloads with the entry uncommitted
    and the candidate still pending; replaying that exact request id adopts the entry, appends no
    second line, keeps the original `valid_from`, and reloads clean; a *different* request id is
    refused with an error naming the one to replay; and the same request id carrying a dismissal is
    refused as a payload conflict;
14. a record body carrying a forged `## memory-record-…` section and a fence-breaking backtick run
    parses back to exactly one record, with the injected heading inside a fence and never at the top
    level, and a multi-line title collapses to one escaped line;
15. a tracked memory log with no local candidate ledger is adopted wholesale, while the same log
    read as a local ledger's uncommitted tail fails closed with an error naming the recovery;
16. a parked decision whose accepted task is gone survives reconciliation, and the resulting orphan
    error names both the record and the governed task;
17. every `MemoryLogError` variant's `Display` names what went wrong, and `MemoryLogHead` round-trips
    through serde; and
18. a promotion followed by a governed registration succeeds in a repository that does not gitignore
    `.impulse`, both before and after the memory log is committed.

Source of truth: `impulse-rs/impulse-ops/src/{memory_candidate,memory_wiring}.rs`,
`impulse-rs/src/state/{memory_candidate,memory_record,persistence}.rs`,
`impulse-rs/src/retrieval/{store,indexer,mod}.rs`, `impulse-rs/src/handlers/config.rs`,
`impulse-rs/src/governed_producers.rs` (the subject-change exemption only).

## Related Documents

- [`0013-deterministic-accepted-run-memory-candidates.md`](0013-deterministic-accepted-run-memory-candidates.md)
  — the review-only slice this decision completes; its rule 9 is preserved, not reversed.
- [`0018-socket-actor-provenance.md`](0018-socket-actor-provenance.md) — supplies the authorization
  model reused verbatim, and names the status-preserving migration as follow-up 2.
- [`0019-builder-staged-worktree-world-scope.md`](0019-builder-staged-worktree-world-scope.md) —
  establishes the integrity of the repository state an authenticated request acts on.
- [`0011-governed-task-run-lifecycle.md`](0011-governed-task-run-lifecycle.md)
- [`../plans/2026-09-02-impulse-next-stages.md`](../plans/2026-09-02-impulse-next-stages.md) — Stage 6.
- [`../plans/worktrees/2026-09-12-claude-memory-promotion-adr0020.md`](../plans/worktrees/2026-09-12-claude-memory-promotion-adr0020.md)
- [`../../VISION.md`](../../VISION.md) — step 10.
- Zep/Graphiti, arXiv:2501.13956 (bitemporal memory, supersession as new assertion); A-MEM,
  arXiv:2502.12110 (atomic, individually addressable memory notes).
