---
title: Flaky Harness Kill Test
description: Work card for claude/flaky-harness-kill-test-20260912 (make agent::tests::test_harness_query_kills_hung_child_instead_of_orphaning deterministic under machine load)
updated: 2026-09-12
type: doc
category: planning
phase: all
status: ready
audience: builders
tags: [worktree, lane, testing, agent, harness, tokio]
---

# Flaky Harness Kill Test

## Lane Facts

- Owner: Claude (Fable 5.1).
- Role: test-only fix lane. Diagnose why
  `agent::tests::test_harness_query_kills_hung_child_instead_of_orphaning`
  (`impulse-rs/src/agent/mod.rs`) fails under machine load and passes in
  isolation, reproduce the flake, make the test deterministic, prove it with
  a loop under artificial load.
- Branch: `claude/flaky-harness-kill-test-20260912` from `main` at `e470767`.
- Worktree: `.worktrees/flaky-harness-kill-test-20260912`.
- Owned paths:
  - The `mod tests` block of `impulse-rs/src/agent/mod.rs` (the flaky test
    and its sibling abort test only).
  - `impulse-rs/Cargo.toml` `[dev-dependencies]` (a `tokio` `test-util`
    feature line) and `impulse-rs/Cargo.lock` if it changes.
  - This work card.
- Blocked/shared paths (not touched): every production code path, including
  `ImpulseAgent::harness_query_structured_with_timeout` and
  `impulse-rs/src/process_group.rs`; `impulse-rs/src/test_support.rs`;
  `.github/**`; `CLAUDE.md`; `AGENTS.md`; `CONTEXT.md`.
- Acceptance criteria:
  - The recorded failure mode is reproduced (or its mechanism is
    demonstrated) before any change.
  - After the change, the test contains no wall-clock bet: the harness
    timeout can only fire after the fake harness has published its wrapper
    and grandchild pids.
  - 30/30 passes in a loop under artificial load (oversubscribed CPU plus a
    concurrent full release build).
  - Full gate green with an isolated `CARGO_TARGET_DIR`.
- Verification:
  ```
  cd impulse-rs
  export CARGO_TARGET_DIR=<isolated dir>
  cargo build --workspace && cargo test --workspace && \
    cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
  python3 ../docs/validate_docs.py --all
  ```

## Findings

- **Root cause: a wall-clock precondition, not a leaked orphan.** Every
  recorded failure (the 2026-09-12 lane gates for PRs #54/#56 and the earlier
  #48 gate) panicked at the same line with
  `fake harness must publish wrapper and child pids before timeout: Os { code: 2, kind: NotFound }`.
  The test passed a fixed 2 s timeout to
  `harness_query_structured_with_timeout`, let it fire, and only then read the
  pid file the fake `sh` wrapper writes after `sleep 60 &`. Under load the
  wrapper had not reached its `printf` when the timeout killed the process
  group, so the pid file never existed. The property under test (the group is
  killed on timeout) was never the thing that failed; the test's own setup
  lost a race with the scheduler.
- **Not the cause, checked and ruled out:** the `ps`-based liveness probe
  and marker-collision theories in the task brief describe an older shape of
  this test. The current test already observes exact pids published by the
  child itself and waits for them with a bounded poll
  (`test_support::wait_for_pids_to_exit`), and tokio 1.49's `Reaper` pushes a
  killed-but-unreaped child onto the global orphan queue that any runtime's
  process driver drains on the next `SIGCHLD` watch change, so a zombie
  wrapper would clear within the 10 s bound. No recorded failure mentions
  survivors.
- **Reproduction.** 24 `yes` burners plus a concurrent workspace release
  build were not enough (30/30 passed). Adding 48 fork storms
  (`while :; do /bin/sh -c :; done`), which contend on exactly the
  fork+exec path the fake wrapper needs, pushed the 1-minute load average to
  120 on 10 cores and reproduced the recorded panic 3 times in 30 runs of the
  unfixed test binary.
- **Baseline note:** `python3 docs/validate_docs.py --all` exits 1 on `main`
  at `e470767` because three guides last updated in March 2026 exceed the
  120-day staleness threshold. That is pre-existing and unrelated to this
  lane's card, which validates clean.

## Decisions

- 2026-09-12: **Control the ordering instead of widening the bet.** A longer
  fixed timeout (10 s, 30 s) would only move the race and slow the suite. The
  rewritten test runs the query with an effectively infinite timeout (3600 s),
  waits (bounded, 30 s liveness guard) for the wrapper to publish both pids,
  then calls `tokio::time::pause()` and `tokio::time::advance(3600 s)`. The
  explicit advance (rather than relying on the paused clock's idle
  auto-advance) is the reviewer's P2: auto-advance is suppressed while any
  blocking task is alive on the runtime (`runtime/blocking/schedule.rs`
  bumps `auto_advance_inhibit_count`), so a future `spawn_blocking` or
  `tokio::fs` call in the code path would have turned the test into a silent
  one-hour hang; `Clock::advance` checks only that time is frozen, and the
  next runtime turn fires the expired timer. Either way the
  `HarnessTimedOut` branch can only fire after the pids are known.
  `tokio::time::resume()` restores real time before the pid liveness poll so
  its 10 s bound stays a real bound. The 30 s pid-file guard is the only
  remaining bound and it gates nothing the code under test does.
- 2026-09-12: **`tokio` `test-util` is a dev-dependency feature only.** It
  adds `pause`/`resume`/`advance`, routes `tokio::time::Instant::now()`
  through the runtime clock, and adds inhibit bookkeeping on blocking tasks;
  nothing observable changes while time is not paused.
  Feature unification means test builds of the workspace see it; release
  builds do not.
- 2026-09-12: **Sibling abort test shares the helpers.**
  `test_aborting_harness_query_kills_the_whole_process_group` had the same
  inline fake-harness script and pid-file poll; both now use
  `write_pid_publishing_fake_harness` and `wait_for_published_pids` so there
  is one definition of the fixture. Its logic is unchanged.
- 2026-09-12: **Left alone, flagged for the PR:**
  `test_harness_query_times_out_instead_of_hanging_forever` asserts
  `elapsed < 5 s` around a 300 ms timeout. That is the same class of
  wall-clock assertion but has never been observed failing; converting it is
  a separate, equally small change.

## Changes

- `impulse-rs/src/agent/mod.rs` (`mod tests` only): new
  `write_pid_publishing_fake_harness` and `wait_for_published_pids` helpers;
  `test_harness_query_kills_hung_child_instead_of_orphaning` rewritten to
  spawn the query with a 3600 s timeout, wait for the pids, pause the clock,
  await the typed `HarnessTimedOut { seconds: 3600 }`, resume, then prove
  both pids are gone (with a best-effort `kill -KILL` of survivors before
  panicking, mirroring the abort test);
  `test_aborting_harness_query_kills_the_whole_process_group` switched to the
  shared helpers.
- `impulse-rs/Cargo.toml`: `[dev-dependencies]` `tokio = { version = "1",
  features = ["test-util"] }`.
- This work card.

## Status

- 2026-09-12 22:14 ET: lane opened; root-cause investigation in progress.
- 2026-09-12 22:23 ET: flake reproduced with the unfixed binary under
  burners + 48 fork storms + a release build (load average 120): 27/30, the
  three failures all the recorded `NotFound` panic.
- 2026-09-12 22:30 ET: fixed binary under the same load: 30/30; a single run
  of both pid-file tests takes 0.43 s instead of ~2 s because nothing waits
  out a real timeout any more.
- 2026-09-12 22:31 ET: load torn down; full gate running in the isolated
  target dir.
- 2026-09-12 22:36 ET: full gate green in the isolated target dir: build 0,
  test 0 (34 binaries, 3055 passed / 0 failed / 9 ignored), clippy 0, fmt 0.
  Adversarial refutation pass in flight; PR opens as draft until it clears.
- 2026-09-12 22:39 ET: refutation pass (read-only reviewer against the tokio
  1.49 source) returned no P0/P1. Adopted: explicit `tokio::time::advance`
  instead of idle auto-advance (blocking-task inhibit hazard); the pid-file
  poll now requires the trailing newline (torn-read hardening); comments and
  this card corrected to match. Survived refutation: paused timeout firing,
  monotonic clock after `resume`, zombie wrapper reaped within the 10 s
  bound (`runtime/process.rs` reaps orphans on every park), ordering, and
  portability.
- 2026-09-12 23:22 ET: round 2 (`eb0a82e`) CI 4/4 green (Lint, Build
  release, Test ubuntu, Test macOS); re-gate green; 30/30 under load. PR #61
  marked ready for review. Card status → ready.
