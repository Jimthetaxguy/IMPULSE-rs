---
title: Loop Contract Primitive
description: Work card for claude-loop-contract-20260901 (ADR-0017 typed loop budgets and termination evidence)
updated: 2026-09-01
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, loop-contract, adr-0017, ion, primitives]
---

# Loop Contract Primitive

## Lane Facts

- Owner: Claude (Fable 5.1), iteration 1 of goal `impulse-primitives-meta-harness-2026-09`.
- Role: implementation and integration lane for one primitive.
- Branch: `claude/loop-contract-20260901`.
- Worktree: `.worktrees/loop-contract-20260901` (repository-relative).
- Base: local `main` at `9c586ba` (one commit ahead of `origin/main`, unpushed at lane start);
  first lane commit applies the rustfmt fix that commit needed.
- Owned paths:
  - `impulse-rs/src/loop_contract.rs` (new)
  - `impulse-rs/src/llm_backends/mod.rs`, `impulse-rs/src/error.rs`, `impulse-rs/src/lib.rs`,
    `impulse-rs/src/ion_repl/chat.rs`
  - `docs/decisions/0017-canonical-loop-contract.md`, `docs/decisions/README.md` (one row),
    `docs/INDEX.md` (one row), `docs/SUMMARY.md` / `docs/SUMMARY.yaml` (one entry each)
  - `docs/superpowers/specs/2026-09-01-loop-contract-design.md`
  - `CONTEXT.md` (one glossary entry), this work card
- Blocked/shared paths: everything owned by the live Codex lanes
  (`impulse-rs/src/daemon/*`, `impulse-rs/impulse-desktop/*`, `.github/workflows/*`,
  `impulse-rs/scripts/*`); `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, canonical
  contract; the canonical checkout's dirty ADR-0015 and skill-reference edits.
- Plan/spec: `docs/superpowers/specs/2026-09-01-loop-contract-design.md` and ADR-0017.
- Verification (isolated `CARGO_TARGET_DIR`, see memory note on the shared target dir):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.
- Latest status: implementation complete and gated on this lane: `cargo build --workspace` clean,
  `cargo test --workspace` 2261 passed / 0 failed / 9 ignored (31 new tests over base),
  strict Clippy clean, rustfmt clean. `docs/validate_docs.py --all` reports only failures that
  pre-exist on `main` (ADR-0014's `proposed` status and three stale March guides); all lane
  docs validate. Pushed to `origin/claude/loop-contract-20260901`; draft PR
  [#39](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/39) open for owner review and Ubuntu CI.

## Decisions

- 2026-09-01: Choose the loop contract as iteration 1 over Builder staged-worktree scope
  (collides with the Codex packaged-acceptance lane's owned files) and the harness evolution
  plane (needs typed loop evidence first).
- 2026-09-01: Keep `ToolLoopLimitExceeded` and `ToolLoopTimedOut` as-is for callers that match on
  them; add one `ToolLoopStalled { trip }` variant for the new no-progress trips.
- 2026-09-01: The loop-contract module depends on no provider, tool, or daemon type so the same
  contract can bound governed Builder iterations later.
- 2026-09-01: File the decision as ADR-0017. ADR-0016 is reserved by the drafted harness
  evolution plane on `agent/claude-harness-evolution-20260826`.

## Changes

- New `loop_contract` module: `LoopBudget`, `LoopContract`, `LoopBreaker`, `LoopTrip`,
  `LoopTermination`, `LoopReport`, `canonical_json`, `error_signature`.
- `Agent` carries a `loop_contract` (default `ion_tool_loop`) and records `last_loop_report`
  after every `chat_with_tools` run; `run_tool_loop` admits rounds and reports tool calls
  through a `LoopBreaker`.
- `DEFAULT_MAX_TOOL_ROUNDS` / `DEFAULT_TOOL_LOOP_TIMEOUT` are now sourced from the contract.
- `ChatState::last_loop_report` exposes the evidence to the REPL.

## Tests

- `loop_contract` unit tests: serde round trips for every type, contract validation error paths,
  round cap, repeated-call and same-error streaks (including key-order independence and reset
  rules), disabled detectors, report counts, Display output.
- `llm_backends` tests: repeated-call stall, same-error stall, completed report, cap and timeout
  reports, custom contract applied and rejected, disabled detectors fall through to the cap.
- `error` test: `ToolLoopStalled` Display.

## Handoff Notes

- The wall-clock trip is still enforced by `tokio::time::timeout` in the caller; the breaker only
  records it. Governed Builder loops and the harness subprocess timeout are not yet on the
  contract (see ADR-0017 consequences).
