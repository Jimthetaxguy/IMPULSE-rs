---
title: Loop Contract Design
description: Design spec for the canonical loop contract primitive (ADR-0017) bounding Ion tool loops
updated: 2026-09-01
type: specification
category: architecture
phase: all
status: active
audience: builders
tags: [spec, loop-contract, ion, primitives]
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
