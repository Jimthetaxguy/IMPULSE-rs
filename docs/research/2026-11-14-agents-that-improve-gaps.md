# Agents that improve: gap list for IMPULSE-rs

Source: `docs/research/2026-11-14-agents-that-improve.md` (given by Jim via Grok-Prime, Nov 14, 2026).
Read against `origin/main` at `c46e9ee` on Nov 14, 2026. ADRs live in `docs/decisions/`; paths below are under `impulse-rs/` unless they start with `docs/`.

## Constraints this list respects

- `decide_step_model` (`impulse-step-model/src/lib.rs`) stays a thin picker that names a model. It does not become a gateway or a learned router.
- No new crate and no new tree. Every change below extends a file or ADR that already exists (ADR-0013, 0015, 0016, 0017, 0020, 0023).
- Nothing here merges without Jim. This branch is research only and changes no code.

## The four roles

**Propose.** Impulse already has this. `state/memory_candidate.rs` derives one review-only `AcceptedRunMemoryCandidate` per accepted governed run (ADR-0013), and ADR-0016 describes harness-patch candidates in the same shape. Stewardship cleanup proposals sit in `stewardship/approval.rs`. What is missing is any proposal that comes from a correction. Candidates come only from runs a person accepted, so a Supervisor `ChangesRequested` or an operator `Rejected` (`GovernedReviewState` in `impulse-ops/src/governed_task.rs`) teaches nothing. Smallest next change: none yet. This waits on the evidence work below.

**Evidence.** This is the strongest role. A candidate pins `claim_id`, `verification_id`, `supervisor_verdict_id`, `operator_decision_id`, the commands run and a `source_digest`. A promoted `MemoryRecord` (`impulse-ops/src/memory_candidate.rs`) carries `MemorySource::CandidateRef { candidate_id, candidate_source_digest, governed_task_id }`, and `.impulse/MEMORY.jsonl` is a hash-chained append-only log (`state/memory_record.rs`, ADR-0020). The handoff gate (`handoff/gate.rs`, `.impulse/HANDOFF_GATE.json`, PR #96) refuses a claim whose file, commit or URL source does not record the claimed subject. `basis.rs` fails closed when a source a run read has moved. What is missing is that an agent can't reach this evidence at the point of use (see theme A).

**Evaluate.** `examples/selector_baselines.rs` compares the live selector with the best fixed model and choose-at-start on a held-out test split, reporting pass rate, cost per success and p50/p95 latency. `settlement.rs` requires a per-candidate check matrix and records dissent. `verify/mod.rs` runs detected install, typecheck, test, lint and build steps. `loop_contract.rs` (ADR-0017) leaves a typed `LoopReport` per loop. What is missing is the doc's section E: nothing counts tasks that newly pass separately from tasks that newly fail, and nothing compares a change against the previous system. Smallest next change: rank 2 below.

**Authority.** Promotion and dismissal are operator decisions with `decided_by` recorded (`MemoryCandidateStatus`, ADR-0020). Governed tasks need an operator to reach `Accepted` (ADR-0011). ADR-0016 rule 3 says no optimizer applies its own patch. Known holes, confirmed in code: MCP `tools/call` (`mcp/server.rs`) and daemon `InvokeTool` (`daemon/handlers.rs`) call `registry.execute` with no approval step. `ion_repl/chat.rs` mints an `ApprovalGrant` and then runs `tool.run` with no write-ahead record of the intended call, so a crash leaves no trace of what was approved. Smallest next change: a write-ahead intent row, listed under "Next after those" below.

## The three themes

**A. Evidence-linked memory.** Impulse has the write side: candidates, the promoted log, `GENOME_PROJECTION.md`, and a `memory_records` FTS table filled by `retrieval/indexer.rs::index_promoted_memory_records`. `.impulse/GENOME.md` (hand-curated `memory::Genome`, seeded by `handlers/config.rs::handle_init`) and `.impulse/HISTORY.jsonl` are separate and carry no source references. `handlers/config.rs` keeps `MEMORY.jsonl` and `GENOME_PROJECTION.md` tracked for review and gitignores the private candidate ledger and `blackboard.db`. The read side is where the gap is. `retrieval::search_promoted_memory` is only called from tests in `state/memory_candidate.rs`. The agent-facing `memory_search` tool (`tooling/builtin/memory_search.rs`) searches only `history` and `genome`. `RetrievalStore::search_memory_keyword` returns title and body but not the stored `source_json`. So an agent cannot retrieve a promoted lesson, and could not see where it came from if it did. The doc's Phase 1 test ("retrieve a lesson and inspect its origin") fails today. The blackboard (ADR-0023, `blackboard/mod.rs`, `blackboard.db`) already pages large results by reference, so source inspection would not flood context. Nothing sorts retrieved memory into apply, inspect, conflict or exclude (doc section D); that is later work.

**B. Staged method revisions with rollback.** ADR-0016 (status: draft) defines `HarnessVersion`, the typed `HarnessPatch`, `harness-candidate-<sha256>` and an append-only lineage ledger with operator revert. None of those types exist in code; a search of `origin/main` finds no `HarnessVersion` or `HarnessPatch`. For memory, ADR-0020 rule 7 reserves `MemoryRecord::superseded_by`, and the projection already filters superseded records, but the log has no supersede entry kind. A promoted record can't be retired, so promoted memory has no rollback. The `rust_workspace_v1` detached worktree in `governed_producers.rs` (ADR-0012) and ADR-0019's staged worktree are the existing place to evaluate a candidate. Guess: memory supersession is the smaller first rollback step, because the reader side is already done.

**C. Measuring whether changes improve results.** `agent/step_model.rs::record_step_model` builds a `StepModelRecord` but only writes it to `tracing`. The comment says durable ledger attachment is later work. So `selector_baselines.rs` has no producer for its `runs.jsonl` input, and its reports give a net pass rate with no per-task diff. Known gap: cache-read tokens are not recorded (no `cache_read` field anywhere under `llm_backends/` or `token_tracker/`), so cost per success undercounts savings from the Anthropic prompt cache. `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md` and `tools/benchmark.rs` measure speed, not task outcomes.

## Smallest changes, ranked

1. **Let agents read promoted memory with its source.** Add a `memory` scope to `memory_search` that calls the existing `retrieval::search_promoted_memory`. Return `source_json` (candidate id, governed task id) next to each hit. This is one tool and one SELECT column, it reads only records an operator already promoted, and it passes the doc's Phase 1 test.
2. **Report gains and losses separately in `selector_baselines.rs`.** For each policy, list test-split tasks that pass under it and fail under a named reference policy, and the reverse, and print the lost count beside the pass rate. This is eval only and never calls or changes `decide_step_model`. It is also the "fixed and regressed task lists" that ADR-0016 rule 4 requires in every evidence bundle.
3. **Add a supersede entry to `MEMORY.jsonl`.** An operator-only decision that sets `superseded_by` through a new log entry kind, as ADR-0020 rule 7 already plans. This gives promoted memory a rollback with the same chain, ledger and authority rules, and needs no new store.

Next after those (not ranked): a write-ahead intent row before `tool.run` in `chat.rs`, and the same approval check on MCP `tools/call` and daemon `InvokeTool`; a durable `StepModelRecord` so `runs.jsonl` has a producer; and recording cache-read tokens.

## Not verified

- I did not build or run tests. Claims about behavior come from reading source and ADR text.
- `ToolRegistry::execute` (`tooling/registry.rs`) and `ToolExecutor` (`tooling/executor.rs`) check allowed paths and output size but have no approval step. I did not check every tool for its own guard.
- I did not check whether `LoopReport` is written anywhere durable.
- ADR-0017 and ADR-0023 say "accepted on merge". I did not check whether every slice they describe is on `main`.
