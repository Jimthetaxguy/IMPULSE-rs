---
title: Harness Step Model Hook
description: Work card for step-model-hook-adr-0015
updated: 2026-08-17
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, step-model, adr-0015]
---

# Harness Step Model Hook

## Lane Facts
- Owner: Cursor cloud agent
- Role: implementer
- Branch: `cursor/step-model-hook-adr-0015-66c5`
- Worktree: `/workspace`
- Owned paths: `impulse-rs/src/agent/step_model.rs`; `impulse-rs/src/agent/mod.rs`; `impulse-rs/src/llm_backends/mod.rs`; `docs/decisions/0015-harness-owned-step-model.md`; `docs/decisions/README.md`; `docs/INDEX.md`; `docs/SUMMARY.md`; `docs/SUMMARY.yaml`; this work card
- Blocked/shared paths: `Cargo.toml`; `Cargo.lock`; `AGENTS.md`; `CLAUDE.md`; `docs/validate_docs.py`; protocol/spec docs; `impulse-rs/src/monty/routing.rs`; ADR-0014 / SettlementRecord (absent on main; do not invent); PRs 24–26
- Plan/spec: harness-owned step model choice (ADR-0015 proposed). Gateway must not pick the model.
- Verification: `cargo test -p impulse-rs --lib agent::step_model llm_backends::tests agent::tests`; `cargo test -p impulse-rs --lib`; `cargo fmt --all -- --check`
- Latest status: implemented on `cursor/step-model-hook-adr-0015-66c5`; PR #27 opened as draft; not stacked on PRs 24–26

## Decisions
- 2026-08-17: Branch from `main` (`d13050c`). Do not stack on PR 24 (base-URL override), PR 25 (stale-basis), or PR 26 (SettlementRecord / ADR-0014).
- 2026-08-17: Put types and `decide_step_model` in `impulse-rs/src/agent/step_model.rs`. No new crate. No LiteLLM/OpenRouter router.
- 2026-08-17: Optional escalate model lives on `HarnessStepContext`, not a new State config key in this slice. Callers copy `impulse_agent_escalate_model` onto the context when they have it.
- 2026-08-17: Arena log is a structured `StepModelRecord` via `tracing` beside ADR-0011 four-party attestation. No SettlementRecord field. No durable ledger write.

## Changes
- Added `impulse-rs/src/agent/step_model.rs` with `HarnessStepContext`,
  `StepModelDecision`, `StepModelReason::{Configured, AfterVerifierFailure}`,
  `StepModelRecord`, `decide_step_model`, and arena `record_step_model`.
- Called the hook at `Agent::chat`, `run_tool_loop`, and
  `ImpulseAgent::query_stateless`.
- Added proposed ADR-0015 and listed it in the decision/index catalogs.
- Did not touch `monty/routing.rs`, `handle_chat`, Worker CLI, SettlementRecord,
  or PRs 24–26.

## Tests
- Unit tests in `step_model.rs` for identity, after-verifier-failure escalate,
  no token-count escalate, Operator/Verifier admissibility, and serde
  round-trips.
- Fill-site tests in `llm_backends` and `agent` prove the hook is actually
  called.

## Handoff Notes
- Local commit `c7f9e7ee` on `feat/step-model-hook-adr-0015` was not cloned or used.
- Worker CLI / harness subprocess model stays opaque. `handle_chat` / daemon chat picker unchanged.
