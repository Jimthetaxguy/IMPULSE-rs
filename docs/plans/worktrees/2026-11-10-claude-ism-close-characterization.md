---
title: Close Outcome Characterization Tests
description: Pin the desktop runtime's operator-close outcomes, including a failed durable exit acknowledgement.
created: 2026-11-10
updated: 2026-11-10
author: James Pustorino
agent: claude/Claude Code
type: doc
status: active
summary: Tests-only lane from the implicit-state inventory. Characterizes close_agent for a governed runtime when the durable exit is recorded and when the daemon rejects it.
next_steps:
  - Review the draft PR; decide whether a retained retry receipt for a failed durable exit is wanted.
remaining:
  - Rejected Running acknowledgement combined with unconfirmed PTY termination still needs a backend kill-failure seam.
open_questions:
  - Should a failed durable exit keep a retry handle, given the session is already released?
---

# Close Outcome Characterization Tests

## Lane Facts

- Owner: Claude Code
- Role: Characterization tests (no runtime behavior change)
- Branch: `agent/claude-ism-close-characterization-20261110`
- Worktree: `/Users/jamespustorino/code/_worktrees/ism-impulse`
- Owned paths: `impulse-rs/impulse-desktop/src/runtime.rs` (test module only), this work card
- Blocked/shared paths: `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, protocol/spec files
- Plan/spec: implicit-state inventory reproduction priority 2 (close failure and durable-exit acknowledgement failure as separate outcomes)
- Acceptance: operator close records `RuntimeExited`/`OperatorCancelled`; a rejected durable exit returns `GovernedTaskFailed`, releases the session, leaves the task `Running`, and a second close returns `MissingTerminalSession`
- Verification: `cargo fmt --all --check`, `cargo clippy -p impulse-desktop --all-targets -- -D warnings`, `cargo test -p impulse-desktop`

## Decisions

- 2026-11-10: Tests only. The current behavior is truthful (no fabricated exit, error names the closed runtime), so no projection or retry record was added without a product decision.
- 2026-11-10: Priority 1 (rejected Running ack plus failed kill) was skipped: the PTY backend has no kill-failure seam, and adding one is not a cheap test-only change.
