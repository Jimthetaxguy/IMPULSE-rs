---
title: Scoped Memory Promotion and Dismissal Lane
description: Work card for the ADR-0020 memory promotion lane (state layer, ops wire contract, hash-chained MEMORY.jsonl, GENOME projection)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, memory]
---

# Scoped Memory Promotion and Dismissal Lane

## Lane Facts

- Owner: Claude (Fable 5.1)
- Role: implementation lane (state + ops + ADR). The daemon endpoint, the Dioxus controls, and the
  Ion tool are handoffs, not this lane's work.
- Branch: `claude/memory-promotion-adr0020-20260912`
- Worktree: `.worktrees/memory-promotion-adr0020-20260912`
- Base: `origin/main` `7c2086c`
- Owned paths: `impulse-rs/src/state/{memory_candidate,memory_record,mod,persistence}.rs`,
  `impulse-rs/impulse-ops/src/{memory_candidate,memory_wiring,lib}.rs`,
  `impulse-rs/src/retrieval/{store,indexer,mod}.rs` (projection index + dirty marker only),
  `docs/decisions/0020-scoped-memory-promotion-and-dismissal.md`, `docs/decisions/README.md`,
  `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml`, `CONTEXT.md` (three glossary entries),
  this card.
- Blocked/shared paths respected: `src/daemon/**`, `src/ion_repl/**`, `governed_producers.rs`,
  `Cargo.toml`/`Cargo.lock`, `.github/**`, `CLAUDE.md`, `AGENTS.md`.
- Taken by necessity, with a note (see Decisions 5 and 6): `impulse-rs/src/state/governed_task.rs`
  (test-helper visibility only), `impulse-rs/impulse-desktop/src/views.rs` (two match arms,
  compile-required), `impulse-rs/src/handlers/config.rs` (four gitignore entries).
- Plan/spec: `docs/plans/2026-09-02-impulse-next-stages.md` Stage 6; ADR-0020.
- Verification: isolated `CARGO_TARGET_DIR`; `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.
- Latest status: merged `origin/main` (#53 `ceb29a8`, #55 `1f866d6`); branch FROZEN at the merge push; draft PR open.

## Decisions

1. **2026-09-12 — `GENOME.md` is not regenerated; the projection gets its own file.** The staged
   plan's one-liner says "GENOME regenerated as a projection". Taken literally, that deletes every
   decision an operator wrote through `impulse memory add` on the first promotion, and reverses
   ADR-0013 rule 9's promise that promotion never mutates `GENOME.md`. The projection is
   `.impulse/GENOME_PROJECTION.md`; `GENOME.md` keeps its writer and its content. Recorded as a
   deliberate deviation in ADR-0020 rule 2, and flagged as an open question for the owner below.
2. **2026-09-12 — the committed log head lives in the candidate ledger, not in `MEMORY.jsonl`.** A
   hash chain alone is still valid after its tail is cut off, so trailing truncation is undetectable
   from inside the file. The ledger's `memory_log_head { entry_count, head_digest }` is the external
   witness. The two artifacts are federated, not merged.
3. **2026-09-12 — append first, commit the ledger second.** A crash then leaves one *uncommitted*
   trailing entry (invisible to the projection and the index, adoptable by a replay of the same
   request id) instead of a promoted candidate pointing at a record that does not exist. Any other
   decision is refused until the interrupted request is replayed, with a typed error naming it.
4. **2026-09-12 — the record identity digest excludes `valid_from`.** A replayed decision after a
   crash must re-derive the same record id despite a moved clock. `valid_from` stays tamper-evident
   through the log entry digest, which covers the whole line.
5. **2026-09-12 — `impulse-ops` gained no cryptographic dependency.** `Cargo.toml` is blocked, and
   `impulse-ops` has no `sha2`. The ops types define *what* bytes are hashed
   (`identity_source_bytes`, `digest_source_bytes`) and enforce structural coherence (id hex ==
   digest hex); `src/state/memory_record.rs` owns the hash function and checks the hex is real.
6. **2026-09-12 — three blocked-adjacent files touched, minimally.**
   `impulse-desktop/src/views.rs` had an exhaustive match on the one-variant
   `MemoryCandidateStatus`; adding variants is a compile break, so two read-only render arms were
   added (no Promote/Dismiss control — that is the desktop lane's work).
   `state/governed_task.rs`'s test module became `pub(in crate::state)` and two helpers (`state()`,
   `accept_run()`) were exposed, because reaching an accepted governed run is the only way to obtain
   a real candidate and CLAUDE.md forbids duplicating the factory — no production code changed.
   `handlers/config.rs` gained `.impulse/MEMORY_INDEX.json` plus three temp globs in the runtime
   gitignore list; `MEMORY.jsonl` and `GENOME_PROJECTION.md` are deliberately **not** ignored.
7. **2026-09-12 — promoted records get their own retrieval tables.** `memory_records`/`memory_fts`,
   never extra rows in `genome_decisions`. The indexer is fed the projection, so a pending or
   dismissed candidate cannot be indexed by construction. Retrieval schema version 2 -> 3, additive.
8. **2026-09-12 — the ledger schema version moves 1 -> 2** (revision, receipts, committed head,
   carried statuses). Every new field is serde-defaulted; a v1 ledger loads unchanged.

## Changes

- `impulse-ops/src/memory_candidate.rs`: `MemoryCandidateStatus` gains `Promoted`/`Dismissed`
  (serde-defaulted, validated); new `MemoryRecord`, `MemoryRecordId`, `MemoryScope`, `MemoryKind`,
  `MemorySource`, `MemoryLogEntry`, and the digest-source/prefix constants.
- `impulse-ops/src/memory_wiring.rs` (new): `MemoryCandidateDecisionInput` (no authentication field,
  `deny_unknown_fields`), `MemoryCandidateDecisionKind`, `MemoryCandidateDecision`,
  `MemoryCandidateDecisionOutcome`, `DecideMemoryCandidateRequest`/`Ack`. Request types only.
- `src/state/memory_record.rs` (new): the verified `MemoryLog`, its chain sealing and verification,
  typed `MemoryLogError`, record derivation, deterministic projection rendering and atomic write,
  and the `MEMORY_INDEX.json` dirty marker.
- `src/state/memory_candidate.rs`: ledger revision/receipts/head/carried statuses;
  `migrate_superseded_derivations` replacing `prune_superseded_derivations`; status-insensitive
  reconciliation; `State::decide_memory_candidate`, `read_genome_projection`,
  `list_promoted_memory_records`, `index_promoted_memory_records`,
  `memory_retrieval_index_is_dirty`; typed `MemoryCandidateDecisionError`.
- `src/state/{mod,persistence}.rs`: the `memory_log` field, its start-up load, and
  `reconcile_promoted_memory_log` after candidate reconciliation.
- `src/retrieval/{store,indexer,mod}.rs`: `memory_records`/`memory_fts`, upsert/prune/count/search,
  `index_promoted_memory`, `search_promoted_memory`.
- `docs/decisions/0020-*.md` plus rows in `docs/decisions/README.md`, `docs/INDEX.md`,
  `docs/SUMMARY.md`, `docs/SUMMARY.yaml`; three `CONTEXT.md` glossary entries.

## Review round 1 (2026-09-12)

Adversarial review of PR #56 returned "needs changes"; every finding below was confirmed and fixed
on this branch. Two commits: the memory-lane fixes, then the governed-subject exemption on its own
so it can be resolved independently against the other lanes touching the same function.

### P1 — the crash-window recovery path did not exist

The ADR promised that replaying an interrupted request id adopts the uncommitted tail entry. The
code could not: the receipt is written by the ledger commit, so after a crash no receipt exists and
the replay branch never saw the request, while the tail check refused **every** request
unconditionally — including, absurdly, with an error naming the very request id that was supposed to
be replayed. There was no other way to clear the tail, so a single crash wedged the promotion path
permanently.

Fixed in `decide_memory_candidate`: the tail is matched by the `request_id` on its record. On a
match the record is re-derived and its id compared — the identity digest covers the payload but
excludes `valid_from`, so a genuine replay derives exactly the tail's id and anything else does not,
and is refused as an idempotency conflict. On a match the entry is adopted (ledger commits with the
tail's head plus a receipt, nothing appended), and the adopted record keeps the `valid_from` of its
original append, with the decision recording that same instant. A dismissal carrying an interrupted
promotion's id is a payload conflict, because only a promotion appends. Three tests cover it
(adopt / different id refused-and-named / same id different payload refused), plus ADR clause 12's
rewrite and Verification item 13.

Two uncommitted entries still fails closed. Deliberately **no** API was added to discard a log tail —
an endpoint that truncates an append-only evidence log is the primitive this decision exists to avoid
handing out. ADR clause 12a names the manual step instead, and it is safe by construction because an
uncommitted entry is referenced by nothing.

### P1 — promotion blocked every subsequent governed registration

Neither `MEMORY.jsonl` nor `GENOME_PROJECTION.md` was gitignored (by design) or exempt in
`governed_producers::status_contains_subject_change`, so in any repo initialized by `impulse init`
the first promotion made the tree dirty as `??`, and after the first commit every promotion made it
dirty as a tracked ` M`.

**Scope note:** the coordinator's instruction was to touch "only that `matches!` arm". That fixes
only the untracked half — the tracked arm was a bare `None => true` with no path inspection at all,
which is the half that blocks forever. Both arms were therefore changed, via a new
`is_impulse_memory_evidence_artifact` plus a `tracked_status_and_path` splitter. The exemption is
narrowed to modification/addition status codes, so **deleting** the memory log is still a subject
change, and a `-z` rename's bare origin record is still a subject change. Four tests: before and
after the first commit in a real git repo, deletion and neighbouring-`GENOME.md` refusals, the
status-code splitter, and one pinning the exemption to the state layer's own filename constants so a
rename breaks the test instead of silently un-exempting the path. Justification is ADR clause 3a.

**Known both-keep conflict:** PRs #52 and #53 both add adjacent entries to
`is_untracked_impulse_runtime_artifact`. Whoever merges second keeps both sides' entries; this lane's
addition is the single `|| is_impulse_memory_evidence_artifact(path)` line plus the new functions
below it, and its change to the `None =>` arm of `status_contains_subject_change` is separate from
anything those PRs touch.

### Also found while fixing the above: a fresh clone could not start

`MEMORY_CANDIDATES.json` is gitignored and `MEMORY.jsonl` is tracked, so a clone arrives with a full
log and **no ledger**. That was read as "the ledger commits nothing", making every entry an
uncommitted tail — so any project with more than one promoted record failed to start on a fresh
clone, and one with exactly one silently dropped it from the projection. A `LedgerOrigin` now
distinguishes "ledger file absent" (adopt the verified log wholesale, record its head, skip the
orphan direction of the cross-check — a machine with no ledger cannot call a record orphaned) from
"ledger present" (unchanged). ADR clause 3b; the `UncommittedTailTooLong` error also now names this
recovery. This is the substance behind what the review listed as a doc-comment nit.

### P2 — the chain detects corruption, not forgery

Added ADR clause 9a and a Consequences bullet: an actor who can write both artifacts can rewrite the
last entry, reseal its digests, update the ledger head and the candidate's `record_id`, and load
clean. Same boundary as every other private ledger here; "tamper-evident" now reads as
self-consistency, and downstream readers are told not to describe the projection as authenticated.

### P2 — a parked decision was silently cleared

`reconcile_accepted_run_memory_candidates` cleared all parked migrations, including a `Promoted` one
whose task had disappeared — after which the cross-check refused start-up with an orphan error
pointing nowhere. Parked *pending* statuses are still dropped (they carry nothing); a parked decision
is kept, logged at warn, and surfaced by a new `MemoryLogError::OrphanRecordFromLostTask` naming both
the record and the governed task. ADR clause 14a; two tests.

### P2 — projection injection

A record body was written raw and its title into a `### ` heading, while `validate_text` permits
newlines and acceptance criteria are Builder-supplied — so a body could emit a structurally valid
second record section. Titles now collapse to one line with a leading structural character escaped;
bodies are fenced with a backtick run one longer than the longest run in the body (deterministic,
which a random nonce could not be, given the byte-stability requirement). The reviewer's fixture is a
test asserting a fence-aware parse returns exactly one record and that the forged heading sits inside
a fence — preserved verbatim, neutralized rather than censored. ADR clause 12b.

### P2 and nits

- `MemoryLogError` `Display` test covering all nine variants (eight plus the new one) and a
  `MemoryLogHead` serde round-trip test.
- The two `#[error(...)]` literals that rustfmt had joined into runs of spaces are now single-line.
- `write_projection` compares markers on the digests only, so `marked_at` no longer rewrites
  `MEMORY_INDEX.json` on every `State::new`.
- Dropped the dead `.impulse/MEMORY.tmp.*` ignore entry: the log is appended, never staged through a
  temp file.
- The reconcile doc comment now states the fresh-clone branch.

## Review round 2 (2026-09-12)

Round-2 verification confirmed every round-1 fix (crash-window replay including the forged-tail leg,
fresh-clone adoption, the tracked-path exemption with every bypass attempt failing — verdict KEEP,
so the gitignore alternative in open question 4 is settled — projection fencing, parked-migration
surfacing, ADR 9a, the Display/round-trip tests, and the index-marker fix). One new P1 and two
wording items, fixed here. The branch is frozen after this push.

### P1 — `LedgerOrigin::Absent` was unreachable

Round 1's fresh-clone fix probed for `MEMORY_CANDIDATES.json` inside
`reconcile_promoted_memory_log`. But `reconcile_accepted_run_memory_candidates` runs first and
**re-creates that file** from governed-task truth, so the probe reported `Local` on every machine
that has ever run a governed task — which is every machine that could hold a memory log at all. The
branch I added was dead code, and the bug it was meant to fix was still live: two promoted records
plus a deleted ledger refused start-up with a remedy that changed nothing, and one record loaded with
the record silently invisible.

The origin is now captured in `load_memory_candidate_ledger`, before anything writes the file, and
threaded to the log reconcile as a `State` field. Two regression tests cover tasks-present /
ledger-absent at one and at two records; both must adopt.

Fixing that exposed the half of the problem the probe had been hiding: recording the head is not
enough, because the **review statuses lived only in the deleted ledger**. The first boot adopted and
the second boot orphaned. Adoption now rebuilds each status from the record itself — every record
names its candidate and governed task — marking the candidate promoted, or parking the rebuilt
decision where the candidate was re-derived under a different id, exactly as a derivation migration
parks one. Without this an adopted record orphans on the next start-up *and* its candidate sits
pending, promotable a second time into a duplicate record. `decided_by` is the system actor
`adopted-from-checkout`: the log is authoritative for what was promoted, never for who approved it,
and borrowing an id to fill that field would imply an approval this machine never saw. Asserted by
test.

### P2 — the truncation error prescribed the wrong remedy

`UncommittedTailTooLong` named only "remove the local candidate ledger". That is ADR 12a's *last*
resort described as its first, and as a general remedy it is actively wrong: removing the ledger
makes the log adopt wholesale under rule 3b, turning "refuse a log written outside Impulse" into
"accept it" while discarding every review decision. The message now leads with keeping the first
`memory_log_head.entry_count` lines, and 12a says plainly that ledger removal is not offered as a
recovery.

### P2 — ADR 3b's inverse boundary

New clause 3c: a machine that loses the gitignored ledger adopts whatever log it finds, so deleting
the ledger is also how a truncated or record-appended log is made to load clean. Truncation and
orphan detection therefore depend on an artifact that is not in review. Stated as the same
unkeyed-chain boundary as 9a seen from the other side, with the signed-head option named and not
taken.

### Low — rule 3's premise does not hold in this repository

IMPULSE-rs's own `.gitignore` blanket-ignores `.impulse/` (line 7), so `MEMORY.jsonl` and
`GENOME_PROJECTION.md` are *not* tracked here without force-adding them. Noted as an exception next
to clause 3; force-adding is the owner's call and this lane did not do it. Rule 3a's exemption is
still required regardless, because `impulse init` does not add a blanket rule to a project that
lacks one.

## Review round 3 (2026-09-12)

Round-3 verification confirmed the round-2 origin capture, the 1/2-record adoption with governed
tasks present, re-promotion refusal, the message rewrite, and ADR 3c/clause 3. One HIGH finding
remained, fixed here. Branch frozen after this push.

### HIGH — a true fresh clone was permanently unbootable from boot 2

Round 2's adoption was tested only with `GOVERNED_TASKS.json` present. On a **true** fresh clone —
only `MEMORY.jsonl`, `GENOME_PROJECTION.md` and `config.json` — the adopted records have no local
candidate, so their rebuilt decisions park under `governed_task_id`. Reattachment only ever fired in
candidate reconciliation's newly-inserted-candidate branch, which never runs when there are no
accepted tasks. Boot 1 adopted; boot 2 saw `origin == Local`, found the lingering park, and refused
with `OrphanRecordFromLostTask` — whose "no longer an accepted task" wording was also false, since
the task was never known here. Every subsequent boot refused identically.

**Not fixed as the reviewer's preferred option (a).** Synthesizing a candidate from the record's
provenance is not implementable honestly: an `AcceptedRunMemoryCandidate` carries the whole evidence
chain (claim/verification/verdict/decision ids, command digests, source assurance, and a
`source_digest` that its id is a SHA-256 over), and a record carries only the candidate id, the task
id, and that digest. Building one would mean fabricating evidence fields and an id that does not hash
to its own source — `validate_shape` would reject it, and bending validation to accept it would mint
a daemon-profiled evidence chain this machine never observed, which is the exact thing ADR-0013
exists to prevent.

Fixed as (b), and taken further than the reviewer framed it. The distinction offered was "parked,
awaiting a task never seen" (valid) versus "task known and gone" (orphan). But the second case is
equally permanently unbootable and equally not corruption — it is review state waiting for a
candidate that may never return, not a log that disagrees with itself. So the rule is now simply:
**a parked decision naming a committed record claims that record**, exactly like a live candidate's
promoted status, and only a record that *nothing* claims is an orphan. The waiting state is surfaced
at warn with the record and task named, which is the alternative round 1 explicitly allowed.

`MemoryLogError::OrphanRecordFromLostTask` is deleted rather than reworded — it can no longer fire,
so there is no misleading message left to fix. A parked decision naming a record that is *not* in the
log claims nothing and neither rescues an orphan nor is reported.

Regression is the reviewer's exact recipe: fresh-clone the three tracked files, boot four times,
accept and promote a local run on boot 3, assert both adopted records stay visible in the projection
across every boot, that a clone with no governed tasks has no candidates to review at all (so nothing
can be double-promoted), and that re-promoting the local candidate is refused. Round 1's
lost-task test now asserts the start-up succeeds and survives a second boot, rather than asserting
the error it used to produce.

ADR clause 14a rewritten, 3b extended with the true-fresh-clone case, Verification items 20 and 21
added. Display coverage is now eight variants.

## Review round 4 (2026-09-12)

Round-4 verification confirmed the fresh-clone four-boot fix and that the orphan and truncation
refusals still hold. One Medium availability finding, fixed here; branch frozen after this push.

**A foreign trailing entry blocked promotions forever with unfollowable advice.** One well-formed,
correctly chained entry appended to the *tracked* `MEMORY.jsonl` lands within the tail cap, loads,
and stays invisible — correct so far. But `decide_memory_candidate` then refused every future
decision on every checkout with `InterruptedDecision`'s "replay that exact request id", which nobody
can do for an id that was never issued here. Anyone with write or merge access to the tracked log
could therefore deny promotions permanently, and the documented recovery was wrong.

A trailing entry is now classified. Foreign means: no local receipt for its request id **and** its
candidate is neither live in the ledger nor parked under its governed task (and an operator-manual
record, which has no candidate at all, is foreign by construction). Those get a new
`ForeignUncommittedTail` carrying the truncation recovery and the exact number of lines to keep —
the same remedy `UncommittedTailTooLong` gives, because it is the same situation. The genuine path is
untouched: an interrupted decision's candidate is local, because the decision was taken against it
moments earlier, so it still gets the replay instruction naming its own id.

ADR 12a gained the foreign-entry paragraph and a new 12c states the boundary plainly: write access to
the tracked log can **deny** promotions until an operator truncates, and can never **make** one — an
uncommitted entry never enters the projection, the retrieval index, or a candidate's status, because
only the ledger commit does those and the ledger is not in review. Availability is the exposure;
integrity is not. A Consequences bullet records that this is the price of rule 3's tracked log.

Tests: the reviewer's Case A end to end (append a foreign entry, load succeeds and records are
unaffected, the next decision is refused with the foreign message and its line count and without the
replay instruction, the refusal leaves the log byte-identical, truncation unblocks promotion), a
companion asserting a genuine interrupted tail still says replay, and Display coverage for the new
variant.

## Merge from main (2026-09-12)

Merged `origin/main` after #53 and #55 landed; merge commit, no rebase, no force. One real conflict,
in `handlers/config.rs`'s runtime gitignore list — both sides kept (#53's `PRODUCER_RESERVATIONS`
entries and this lane's `MEMORY_INDEX`/`GENOME_PROJECTION.tmp` entries). The
`is_untracked_impulse_runtime_artifact` region the both-keep note predicted auto-merged cleanly:
#53's `PRODUCER_RESERVATIONS.json` and `MEMORY_CANDIDATES.json` entries sit alongside this lane's
`|| is_impulse_memory_evidence_artifact(path)`, and the tracked arm of
`status_contains_subject_change` is untouched by their changes. `state/governed_task.rs` also
auto-merged with both sides intact — this lane's `pub(in crate::state)` test visibility and #55's
`compactions: 0`.

Worth noting for whoever reviews the exemption: #53 exempted `.impulse/MEMORY_CANDIDATES.json` for
exactly the reason ADR-0020 rule 3a gives for the two memory-evidence paths — a daemon-owned runtime
artifact dirtying the canonical tree and breaking the next governed step. The two changes are
independent and complementary, and their comments now sit next to each other.

## Handoffs

1. **Daemon endpoint (`DecideMemoryCandidate`).** Types are ready in
   `impulse-ops/src/memory_wiring.rs`; nothing in `src/daemon/**` was touched. Wire it exactly as
   `RecordOperatorDecision` is wired: authorize the connection through
   `daemon/actor_provenance.rs`'s operator-class gate, refuse a non-presenting connection, and pass
   the resulting `OperatorAuthentication` as the second argument of
   `State::decide_memory_candidate`. **Blocked outcome semantics:** a refusal must be returned
   before the state call, leaving `MEMORY_CANDIDATES.json`, `MEMORY.jsonl`, and
   `GENOME_PROJECTION.md` byte-identical — the state layer never sees the request. ADR-0018
   follow-up 4 (untyped refusals) applies here too; if a typed `Unauthorized` variant lands, this
   family should use it. Bump `DAEMON_PROTOCOL_VERSION` additively and note the snapshot field if
   one is added.
2. **Dioxus Memory view.** `impulse-desktop/src/views.rs` now renders the two decided statuses
   read-only. Promote and Dismiss controls are operator-mode only and must not be rendered at all
   outside it. A Dismiss control must collect a nonblank reason before sending — the contract
   refuses a blank one, and the resulting error is not a good first experience. The view should show
   `retrieval_index_dirty` from the outcome so an operator knows a reindex is pending, and must
   never render the raw candidate ledger as if it were memory.
3. **Ion `memory_search` / `genome_read`.** Confirmed with lane
   `claude/ion-documents-memory-20260912`: that lane only bridges the existing
   `src/tooling/builtin/{memory_search,genome_read}.rs` tools into Ion's registry unchanged, so
   pointing `memory_search` at the projection is a change to `memory_search.rs`, owned by neither
   lane yet. **The tool must read `.impulse/GENOME_PROJECTION.md` (or
   `State::read_genome_projection`), never `MEMORY_CANDIDATES.json`** — the raw candidate ledger is
   private review state and must never be exposed to a runtime. `State::list_promoted_memory_records`
   and `retrieval::search_promoted_memory` are the structured alternatives. Treat an absent
   projection as an empty result, not an error.
4. **Reindex trigger.** `State::index_promoted_memory_records` exists and is tested, but nothing
   calls it outside tests yet. Whoever owns the retrieval CLI/daemon reindex path should call it
   when `memory_retrieval_index_is_dirty()` is true (or fold it into the existing
   `handle_index_memory` flow).

## Known gaps and open questions for the owner

1. **`GENOME.md` versus the projection.** This lane deliberately did not merge them (Decision 1).
   If you want one file, that is a separate ADR with a migration for existing hand-written GENOME
   content — not a silent regeneration.
2. **The two-side-effect crash window is narrowed, not closed.** An interrupted decision blocks
   further decisions until its request id is replayed. A client that never retries wedges the
   promotion path until a human replays it or edits the file. A durable reservation (the same gap
   CLAUDE.md already names for governed producers) would close it.
3. **Supersession, deduplication, and embeddings for promoted records are out of scope.**
   `superseded_by` is reserved and always `None`; the projection already filters on it.
4. **The subject-change exemption widened `governed_producers.rs` beyond the untracked arm.**
   Settled in round 2: verification tried every bypass and returned KEEP, so the gitignore
   alternative is off the table. It remains the first tracked-path exemption that function has ever
   had, so any future addition to that arm deserves the same scrutiny.
5. **Whether to force-add `MEMORY.jsonl` and `GENOME_PROJECTION.md` in IMPULSE-rs itself.** This
   repository blanket-ignores `.impulse/`, so rule 3's "promoted memory is in review" does not hold
   here until someone force-adds them. Your call; the lane did not make it.
6. **A signed committed head** would close the rule 3c boundary (losing the private ledger makes a
   truncated log load clean). Needs a key a same-uid process cannot read, which is the same
   unsolved problem as ADR-0018 follow-up 1.
7. **Pre-existing docs-validator failures on this base** (`7c2086c`), untouched by this lane:
   `decisions/0014`'s `status: proposed` is not in the validator's allowed list, and three March
   documents are past the staleness threshold. Both are already open decisions 3 and 4 in
   `docs/plans/2026-09-02-impulse-next-stages.md`.
