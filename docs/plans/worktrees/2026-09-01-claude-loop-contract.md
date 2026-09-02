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

## Review follow-up (Codex review on `9e0f68e`, 2026-09-01)

- P1, batched calls kept executing after a trip: `run_tool_loop` now returns the trip at the
  tripping call, so later calls in the same tool-use response never run and the report counts only
  what executed. Regression: `test_batched_tool_calls_stop_executing_once_breaker_trips`.
- P2, stale report after a provider failure: every run clears `last_loop_report` when it starts
  and a failed model round records `LoopTermination::Failed { error }` (bounded first line).
  Regression: `test_provider_failure_replaces_stale_loop_report`.
- P2, unvalidated effective contract: `loop_contract` is private behind a getter and the
  effective contract (after round and wall-clock overrides) is validated at the execution
  boundary, surfacing `AgentError::InvalidRequest`. Regression:
  `test_invalid_effective_contract_is_rejected_before_the_loop_runs` (seeds a prior run so the
  cleared report and the absence of a new model call are proven, per the adversarial check).

Adversarial verification of those fixes (18-agent workflow, two refuter lenses per fix plus three
fresh sweeps of the diff) confirmed the fixes and surfaced these further defects, all addressed:

- P1, `error_signature` was `{` for every bridged-tool failure: a dynamic tool's failure payload
  is pretty-printed JSON, so three different `bash_exec` failures shared one signature and tripped
  `SameError` with meaningless evidence. The signature is now the first line carrying a letter or
  digit. Regressions: `test_error_signature_skips_structural_lines_of_json_payloads`,
  `test_same_error_streak_ignores_json_structural_first_lines`,
  `test_distinct_json_failures_do_not_trip_same_error`.
- P3, a batch re-issued every round never tripped: the per-call comparison only sees the previous
  call. `LoopBreaker::end_round` now trips `LoopTrip::RepeatedRound` on identical consecutive
  batches (same limit as repeated calls). Regressions: `test_end_round_trips_when_the_same_batch_repeats`,
  `test_end_round_resets_on_a_different_batch_or_an_empty_round`,
  `test_repeated_batch_trips_after_three_identical_rounds`.
- P2/P3, a call cut off by the wall clock vanished from the report: `dispatch_call` counts before
  execution and the report carries `tool_calls_interrupted`. Regressions:
  `test_dispatched_but_unobserved_calls_are_reported_as_interrupted`,
  `test_wall_clock_cutoff_mid_tool_call_reports_the_call_as_interrupted`.
- P3, `WallClock { seconds }` reported `0` for sub-second budgets: the trip now carries `millis`.
- P3, stale rustdoc on `ChatState::turn`, `ToolLoopTimedOut`, `DEFAULT_TOOL_LOOP_TIMEOUT`, and
  `chat_with_tools_capped` (zero cap now rejected): updated. `impulse-ion/TUI_SPEC.md` gained a
  T9 loop-contract entry. `CLAUDE.md` line 100 still describes the loop as bounded only by the
  cap and wall clock; it is a shared file outside this lane, left for the owner.
- Accepted as intended, documented in ADR-0017 consequences: three user declines of a gated tool
  in one turn trip `SameError` and end the turn.
- Refuted by the check and not acted on: wall-clock enforcement only at `.await` points is
  pre-existing, documented design (ADR-0017 rule 5). Logged for later: an empty `MaxTokens` reply
  is committed to history as a completed turn (pre-existing branch, now labelled `Completed`).

## Handoff Notes

- The wall-clock trip is still enforced by `tokio::time::timeout` in the caller; the breaker only
  records it. Governed Builder loops and the harness subprocess timeout are not yet on the
  contract (see ADR-0017 consequences).
- Pre-existing flake, not introduced here: under a broad `cargo test --lib -- ion_repl` filter the
  five tests that `git init` a temp repo (`tool_verify`, `mod::test_respond_verify_*`,
  `chat::test_turn_with_tools_executes_*`) occasionally fail in fixture setup with
  `insufficient permission for adding an object to repository database .git/objects`; every rerun
  and every full workspace gate passed. Each fixture uses its own `tempfile::TempDir`, so the
  cause is not a shared path. Seen twice on 2026-09-01 across two lanes.
- Post-merge status update: gate on the final tree was `cargo test --workspace` 2273 passed /
  0 failed / 9 ignored, build, strict Clippy, and rustfmt clean.
