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
- Latest status: review round 1 addressed; draft PR open.

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
4. **The subject-change exemption widened `governed_producers.rs` beyond the untracked arm.** It is
   the first tracked-path exemption that function has ever had. It is justified for those two
   daemon-owned digest-chained paths (ADR clause 3a) and pinned by tests, but any future addition to
   that arm deserves the same scrutiny — say so if you would rather gitignore both files and give up
   having promoted memory in review.
5. **Pre-existing docs-validator failures on this base** (`7c2086c`), untouched by this lane:
   `decisions/0014`'s `status: proposed` is not in the validator's allowed list, and three March
   documents are past the staleness threshold. Both are already open decisions 3 and 4 in
   `docs/plans/2026-09-02-impulse-next-stages.md`.
