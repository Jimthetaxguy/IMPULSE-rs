---
title: Loop Contract Design
description: Design spec for the canonical loop contract primitive (ADR-0017) bounding Ion tool loops
updated: 2026-09-12
type: specification
category: architecture
phase: all
status: active
audience: builders
tags: [spec, loop-contract, ion, primitives, context-budget]
---

# Loop Contract Design

> Iteration 1 of goal `impulse-primitives-meta-harness-2026-09`. Written in autonomous mode; the
> assumptions below stand in for the questions a live brainstorming session would have asked.

## Goal

Give every Impulse-owned loop one typed declaration of what it may spend, one state machine that
stops it, and one typed record of how it ended. Start with the Ion tool loop, which is the only
Impulse-owned loop in production today.

## Assumptions

- Detecting a stalled loop early is worth more than letting the model use every round: three
  identical calls or three identical failures is strong evidence of no progress.
- Typed termination evidence must exist before any harness-diagnosis or evolution work
  (ADR-0016 draft) can consume it.
- The primitive must not touch files owned by the live Codex release lanes.
- Backward compatibility for existing error variants matters more than a single unified variant.

## Approaches considered

1. **More constants in `llm_backends`.** Cheapest, but keeps the same scattered shape and no
   evidence. Rejected.
2. **Typed contract inside `llm_backends`.** Clean for Ion, but ties the contract to provider types
   and blocks reuse by governed Builder iterations. Rejected.
3. **Standalone `loop_contract` module consumed by `llm_backends`.** Chosen. No provider, tool, or
   daemon dependency; the tool loop passes a breaker in and reads a report out.

## Components

| Unit | Purpose | Depends on |
|---|---|---|
| `LoopBudget` / `LoopContract` | Declared budget with validation | serde |
| `LoopBreaker` | Per-run trip evaluation and counters | `serde_json::Value` for call identity |
| `LoopTrip` / `LoopTermination` / `LoopReport` | Typed termination evidence | serde |
| `canonical_json` / `error_signature` | Stable call identity and error identity | none |
| `Agent::loop_contract`, `Agent::last_loop_report` | Contract applied to every `chat_with_tools` run and its evidence | `loop_contract` |
| `AgentError::ToolLoopStalled` | Surface for repeated-call and same-error trips | `LoopTrip` |

## Data flow

1. `chat_with_tools` clones the agent's contract, applies any explicit round or wall-clock
   override, and builds a `LoopBreaker`.
2. `run_tool_loop` asks the breaker to admit each round and reports each executed tool call.
3. A trip returns `LoopExit::Tripped`; a provider failure returns `LoopExit::Failed`.
4. The caller maps the exit to the existing `AgentError` variants (round cap, wall clock) or the
   new `ToolLoopStalled`, records the report, and leaves history untouched on every error path.
5. On success the report is `Completed` and history is committed.

## Error handling

- Invalid contracts fail at `with_loop_contract` with `LoopContractError`.
- Every trip is a typed `LoopTrip` with a `Display` impl; no free-text reasons.
- The wall-clock timeout remains the caller's `tokio::time::timeout`; the breaker records it.

## Testing

Unit tests in `loop_contract.rs` cover serde round trips, validation errors, every trip and reset
rule, and canonical JSON. Integration tests in `llm_backends` drive fake providers and executors
through `chat_with_tools` to prove each trip surfaces as the documented error with a report.

## Out of scope

Heartbeat liveness, loop checkpoints, automatic HALF_OPEN probes, decision traces, event-driven
triggers, persisting reports beside governed-task evidence, and moving the harness subprocess
timeout onto a contract.

---

## Addendum, 2026-09-12: context budget (Stage 1b-A)

The original contract bounded *how long* a loop may run (rounds, wall clock) and *whether it is
still progressing* (repeated calls, repeated batches, same errors). It said nothing about *how
large* the loop's own conversation may grow. In a tool loop that is the dimension that actually
runs away first: one `file_read` of a large file, or three rounds of verbose `bash_exec` output,
can outgrow the model's window long before ten rounds or 180 seconds are spent. The run then ends
in a provider-side rejection, which is neither a typed trip nor useful evidence.

### What the contract gains

- `LoopBudget::max_context_chars: Option<usize>` — characters of working history the loop may
  carry into a model round. `None` disables it; `Some(0)` is rejected by `validate` with
  `LoopContractError::ZeroContextBudget`. Serde default is `None` and the field is skipped when
  unset, so every budget persisted before this change still loads unchanged.
- `LoopTrip::ContextBudget { chars, limit }` — the history is still over budget after compaction.
- `LoopReport::compactions` — how many tool results this run compacted. Skipped on the wire when
  zero, which is what keeps the field out of every already-persisted governed
  `loop_report_digest`: the governed Builder contract sets no context budget, so its reports
  serialize byte-identically and `GOVERNED_BUILDER_LOOP_VERSION` does not move.
- `LoopContract::ion_tool_loop()` defaults to `ION_DEFAULT_MAX_CONTEXT_CHARS = 200_000`.
  `LoopContract::governed_builder()` leaves it `None` — the daemon holds claim records, not a
  conversation, so it has nothing to measure or compact.

**Why characters, and why 200,000.** The same contract bounds Anthropic, OpenAI, and MiniMax runs,
and each tokenizes differently; a character count is the cheapest measure that is exact,
provider-neutral, and reproducible from a stored history. 200,000 characters is roughly 50-57k
tokens at ~3.5-4 characters per token — about a quarter of the smallest window in the supported
model set. The headroom is deliberate: the measured history excludes the system prompt and tool
schemas that ride along on every request, the reply has to fit, and a compaction pass that only
starts when the window is nearly full has nothing cheap left to drop. This is a working-set cap
that keeps turns small and affordable, not a last-resort guard against provider rejection.

### Where the work lives

Measurement and compaction stay with the caller (`llm_backends`), because they need `Message`,
`ToolCall`, and `ToolResult`. `loop_contract` only declares the limit, counts the compactions, and
names the trip — rule 6 of the original decision (no provider, tool, or daemon types in the module)
is preserved exactly.

### The compaction algorithm

`llm_backends::enforce_context_budget` runs once per round, immediately after `begin_round` and
before the provider call:

1. With no `max_context_chars` set, do nothing.
2. Measure the history (`history_chars`: prose, plus each call's id, name, and *canonicalized*
   input, plus each result's id and content — canonical JSON so the measurement never drifts with
   map ordering). At or under budget, do nothing; the common case is one pass and no allocation.
3. Otherwise replace tool-result content with a bounded stub, **oldest first** (message order, then
   result order within a message), stopping the moment the running total is back under budget.
   Oldest-first because the newest results are what the model is reasoning about this round.
4. Skip a result that is already a stub, or whose stub would not be shorter than the content it
   replaces — compaction may never grow the history.
5. Still over budget with every eligible result compacted: return `LoopTrip::ContextBudget`.

The stub is `[compacted <N> chars from tool '<name>']`, with the tool name resolved through the
matching `tool_use` id and dropped when the id has no matching call. **`tool_use_id` is never
touched**, so the `tool_use`/`tool_result` pairing both provider APIs validate stays intact, and
the model can see that something was elided and ask for it again rather than reasoning over a
silent gap.

Only tool results are compacted. The user's turn and the model's own words are the turn itself; a
loop that cannot fit them is over budget in a way this pass must not paper over — it trips instead.

### Invariants preserved

Only the caller's `working` copy is mutated. A trip discards `working` entirely and `self.history`
is only ever assigned on the success path, so the history-untouched-on-error invariant from rule 3
holds unchanged. On success the stubs *do* persist into history — that is the point: a compacted
result stays compacted for the rest of the session rather than being re-measured every round.

### Surfacing

A `ContextBudget` trip surfaces through the existing `AgentError::ToolLoopStalled { trip }`
catch-all rather than a new variant, so `src/error.rs` is untouched and callers that already render
`LoopTrip::Display` generically need no change. The rendered message
("Tool-use loop stalled: conversation history is N characters after compaction, over the
M-character context budget") is self-explanatory. `ToolLoopStalled`'s own doc comment still
describes only the no-progress detectors and should be widened the next time that file is opened by
its owning lane.

### Not adopted here

Summarizing dropped content with a model call, tiered/external memory (MemGPT-style paging of
evicted results into a retrievable store), token-accurate measurement through a real tokenizer,
dropping whole message pairs, and per-provider budgets. Each needs either a provider round trip or
a store this loop does not have; the stub keeps the reference (id and tool name) that a future
retrieval step would need.
