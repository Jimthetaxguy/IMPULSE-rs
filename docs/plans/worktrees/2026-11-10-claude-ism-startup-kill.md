---
title: Refused Launch With Unconfirmed Kill
description: Keep a governed launch's PTY child managed when the daemon refuses Running and termination cannot be confirmed.
created: 2026-11-10
updated: 2026-11-10
author: James Pustorino
agent: claude/Claude Code
type: doc
status: active
summary: Inventory priority 1. On origin/main a refused or invalid Running acknowledgement plus a failed kill returned an error and dropped the backend, leaving the child unowned and, for a cross-wired acknowledgement, the task Running with no owner. The runtime now keeps the process managed with input stopped and records the launch abort only after a close confirms termination.
next_steps:
  - Review the draft PR; decide whether the desktop UI should render the Blocked refused-launch state differently from other blocked agents.
remaining:
  - Durable exit acknowledgement retry after a confirmed close (inventory priority 2 follow-up from PR #163).
open_questions:
  - Should a refused launch whose kill keeps failing escalate after N retries, or stay operator-driven?
---

# Refused Launch With Unconfirmed Kill

## Lane Facts

- Owner: Claude Code
- Role: Bug fix with a test-only terminator seam
- Branch: `agent/claude-ism-startup-kill-20261110`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs-ism-startup-kill`
- Owned paths: `impulse-rs/impulse-desktop/src/runtime.rs`, `impulse-rs/impulse-desktop/src/bridge.rs` (one new error variant), `impulse-rs/impulse-term/src/backend.rs` (`process_id` accessor), this work card
- Blocked/shared paths: `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, protocol/spec files
- Plan/spec: implicit-state inventory reproduction priority 1 (rejected Running acknowledgement with unconfirmed termination)
- Acceptance: the refused launch returns `LaunchTeardownUnconfirmed { agent_id, pid, message }`; the process stays listed, alive, `Blocked`, with input refused; no exit or launch failure is recorded while termination is unconfirmed; a failed retry keeps it managed; a confirmed close records `LaunchFailed` (rejected ack) or `RuntimeExited` with a non-operator exit kind (cross-wired ack)
- Verification: `cargo fmt --all --check`, `cargo clippy -p impulse-desktop -p impulse-term --all-targets -- -D warnings`, `cargo test -p impulse-desktop -p impulse-term`, `python3 docs/validate_docs.py --all`

## Findings on origin/main

- Both refusal branches in `spawn_agent` returned `GovernedTaskFailed` with an honest "no exit state was recorded" message, then dropped the backend. `TerminalBackend::drop` retried the kill once and only logged a failure, so a child that resisted termination became an orphan nothing owned. No retry handle remained.
- With a cross-wired acknowledgement the daemon had already moved the task to `Running`. Even when the drop-time kill succeeded, nothing recorded an exit, so the task stayed `Running` with no runtime.

## Decisions

- 2026-11-10: Reuse the existing "remains managed, input stopped" contract from `close_agent` rather than a separate quarantine list. The record carries `ConfirmedExit::LaunchAbort { reason }`, so close, natural EOF and desktop shutdown all record the launch abort once termination is confirmed, never an operator cancellation.
- 2026-11-10: The seam is an injectable terminator on `DesktopRuntimeInner` (production is always `TerminalBackend::kill`; the builder override is `#[cfg(test)]`). No `impulse-term` behavior changed.
