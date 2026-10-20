---
title: Typed error and finish states
description: Work card for typed-error-finish-states
updated: 2026-10-20
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Typed error and finish states

## Lane Facts
- Owner: cloud agent
- Role: implement Brief 01 (distinct error and finish states)
- Branch: cursor/typed-error-finish-states-dab9
- Worktree: /workspace
- Owned paths: impulse-rs governed ledger exit kind, impulse-ion gate outcome, tooling executor output limits, ion-verify exit status, the stub gates those tests drive, this work card
- Blocked/shared paths: decide_step_model (impulse-step-model/src/lib.rs, src/agent/step_model.rs) is not touched. No ADR amendment. Cargo.toml, AGENTS.md, CLAUDE.md, and the canonical contract are not edited.
- Plan/spec: uploads/01-impulse-error-states.md plus Jim's decisions on 2026-10-20
- Verification: from impulse-rs, with CARGO_TARGET_DIR isolated, cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace
- Latest status: locally verified; draft PR pending. `decide_step_model` sources are unchanged. `impulse-step-model/examples/selector_baselines.rs` was reflowed by rustfmt 1.99 and a pre-existing `single_match` was rewritten to `if let` so `cargo clippy --all-targets -D warnings` stays clean.

## Decisions
- 2026-10-20: Cancel stays a `GovernedExitKind` on the run record and on `MarkRuntimeExited`. No new `GovernedExecutionState::Cancelled`.
- 2026-10-20: An answered `OperatorCancelled` exit refuses any later non-replay worker claim, staged or authoritative. A second cancel of an already-terminal run succeeds and changes nothing.
- 2026-10-20: `ion_verify` reports `GateOutcome` (`pass`, `refused`, `undecided`, `contract_violation`, `gate_error`). `verify()` rejects a response whose `request_id` is not the request's.
- 2026-10-20: `ion-verify` exits 0 / 1 / 2. Exit 2 is a gate error, matching `handlers/guard.rs`.
- 2026-10-20: Dropped artifacts and truncated output name what was left out. Oversized `output_logs` keep their key and state `omitted_bytes`.
- 2026-10-20: `decide_step_model` is unchanged. `ModelUnavailable` is a variant only; the launcher does not emit it yet.

## Changes
- `GovernedExitKind` on the run record and on `MarkRuntimeExited` (serde default `Natural`, omitted from the mutation fingerprint when natural).
- Operator-cancelled runs refuse a later non-replay worker claim before the staged-worktree check. A second cancel of an already-terminal run returns the current record and does not bump the revision.
- `GateOutcome` / `GateErrorKind`. `verify()` rejects a foreign `request_id`. `ion-verify` exits 2 on a gate error. `ToolOutcome.ok` is true only for `Pass`.
- Artifact and output limits name every drop. Oversized `output_logs` keep their key and record `omitted_bytes`.

## Tests
- Ledger: claim refused after operator cancel (authoritative and staged, before discard), pre-exit-kind replay reads as natural, second cancel is a no-op.
- Ion: needs discussion is undecided, timeout is a gate error with exit 2, foreign request id is a gate error, approve plus a contract violation is not pass.
- Executor: dropped artifacts are named, truncated output states remainder bytes, oversized output logs are named with size.

## Handoff Notes
- GitHub Actions is blocked by billing. Local gate on 2026-10-20, `CARGO_TARGET_DIR=/workspace/impulse-rs/target-agent`, rustc 1.99.1: `cargo build --workspace` exit 0; `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `cargo test --workspace` 3144 passed, 0 failed, 8 ignored.
- `ModelUnavailable` is not produced until the gate launcher emits a structured signal.
- Brief D4 (model-error split inside `decide_step_model`) is out of scope.
