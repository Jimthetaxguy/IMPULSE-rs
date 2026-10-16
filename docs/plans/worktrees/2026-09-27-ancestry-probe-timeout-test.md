---
title: Deterministic timed-out ancestry probe test
description: Work card for ancestry-probe-timeout-test-20260927
updated: 2026-09-27
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff]
---

# Deterministic timed-out ancestry probe test

## Lane Facts
- Owner: Claude Code (session a6d4465a), at James's request
- Role: implementer
- Branch: `claude/ancestry-probe-timeout-test-20260927`, from `origin/main` at ed6fe5e
- Worktree: `.worktrees/ancestry-probe-timeout-test-20260927`
- Owned paths: `impulse-rs/src/governed_producers.rs` (the bounded runner's deadline handling and its tests), this card
- Blocked/shared paths: `Cargo.toml`, `Cargo.lock`, every other source file, `main`
- Plan/spec: the failing CI run on PR 69, https://github.com/Jimthetaxguy/IMPULSE-rs/actions/runs/36339312234/job/108676142868
- Verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`, `cargo test --no-default-features --lib`, `cargo test --workspace`, each with `--locked`, an isolated `CARGO_TARGET_DIR`, and stdin from `/dev/null`
- Latest status: fixed, gate green, branch pushed; no PR opened

## Problem
`governed_producers::tests::test_a_timed_out_ancestry_probe_is_not_reported_as_a_governance_finding`
failed once on `Test (ubuntu-latest)` for PR 69, a lockfile-only change, and
passed on re-run and on PR 70 minutes earlier. The test passes a zero deadline
to the ancestry probe and expects `GitProbeFailure::TimedOut`. Its comment says
no spawned process can have exited before the runner's first poll. That holds
only when the parent thread keeps running: `run_bounded_process` polls
`try_wait` before it compares the elapsed time with the deadline, and between
`spawn` and that first poll it creates two pipes' reader threads. If the parent
is descheduled there, a fast `git merge-base --is-ancestor` has already exited
and the runner reports its exit status instead of a timeout.

## Reproduction
Each loop ran one test repeatedly in eight parallel workers while `yes`
saturated all ten cores of an Apple Silicon Mac.

| Code | Test | Runs | Failures |
|---|---|---|---|
| main, unmodified | the ancestry test | 400 | 0 |
| main plus the new tests, runner unmodified | `test_run_bounded_process_zero_deadline_always_times_out` | 200 | 59 |
| fixed runner | `test_run_bounded_process_zero_deadline_always_times_out` | 200 | 0 |
| fixed runner | the ancestry test | 400 | 0 |

The ancestry test did not fail here before the change. `git` needs several
milliseconds to start on macOS, which is longer than the parent needs to reach
its first poll, so the race that CI lost on Linux is rarely lost on this
machine. The new runner test uses `true`, which exits in about a millisecond,
and reaches the same defect: in 59 of 200 runs the unmodified runner returned
`Some(exit status 0)` with `timed_out == false` for a zero deadline.

## Decisions
- 2026-09-27: reproduce before changing anything, with the baseline test binary
  run in parallel loops under CPU load.
- 2026-09-27: a zero deadline is an already-expired deadline. The runner kills
  and reaps the child without polling it, so the result is always `timed_out`.
  Every non-zero deadline keeps the poll-first order.
- 2026-09-27, alternatives rejected:
  - Making the probed command unable to finish. `hook_free_git` deliberately
    disables every hook, fsmonitor and global configuration path that could run
    code, so nothing inside the repository can stall `merge-base`, and a command
    seam would add a second injection point to a function whose job is to
    decide what Git may execute.
  - Checking the deadline before the poll for every timeout. That reports a
    child as timed out when it finished inside its deadline but was observed
    after it, which turns load into spurious timeouts in production.

## Changes
- `impulse-rs/src/governed_producers.rs`: `run_bounded_process` skips the poll
  when `timeout.is_zero()` and documents its deadline contract; the ancestry
  test's comment no longer claims that no child can exit before the first poll.
- Production callers all pass module constants of 10 s or more
  (`GIT_PROBE_TIMEOUT`, `GIT_MATERIALIZE_TIMEOUT`, `GIT_CLEANUP_TIMEOUT`), so
  none of them changes behavior.

## Tests
- `test_run_bounded_process_zero_deadline_always_times_out`, 100 spawns of
  `true` per run: passed
- `test_run_bounded_process_nonzero_deadline_reports_the_exit_status`, for
  `true` and `false`: passed
- `test_run_bounded_process_kills_a_child_that_outlives_its_deadline`,
  `sleep 30` with a 50 ms deadline: passed
- `test_run_bounded_process_spawn_failure_names_the_command`, the error path:
  passed
- `test_a_timed_out_ancestry_probe_is_not_reported_as_a_governance_finding`,
  unchanged apart from its comment: passed
- Gate, each cargo command with `--locked`, an isolated target directory and
  stdin from `/dev/null`: `cargo fmt --all -- --check` clean;
  `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test`
  2458 passed, 0 failed, 7 ignored; `cargo test --no-default-features --lib`
  2204 passed, 0 failed, 5 ignored; `cargo test --workspace` 3088 passed,
  0 failed, 9 ignored.

## Handoff Notes
- The first pass of `cargo test` and of the no-default-features lib tests each
  failed on two daemon tests with "daemon socket did not become ready"
  (`tests/integration_enhancements.rs:91` and `src/integration_tests.rs:155`).
  Those tests start the daemon through `cargo run` and give it 15 seconds. Two
  other full gates were running on the same machine at the time. Both steps
  passed when rerun on their own, and the workspace step passed them on its
  first pass. They do not touch the code this lane changed, and they are a
  second load-sensitive spot worth its own lane.
- The second `TimedOut` assertion in this file,
  `test_git_completed_successfully_separates_unknown_from_failed`, builds its
  inputs by hand and has no race. The zero-deadline call in
  `llm_backends/mod.rs` is rejected by validation before anything runs.
