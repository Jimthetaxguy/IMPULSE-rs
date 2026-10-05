---
title: Desktop close and launch-failure kill order
description: Work card for desktop-close-order-20261005
updated: 2026-10-05
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, desktop, runtime, review]
---

# Desktop close and launch-failure kill order

## Lane Facts
- Owner: claude (Opus 5.5, under James's standing goal "continue cleaning up and reviewing the
  code"), following up the desktop half of a P1 from the terminal core review
  (`docs/plans/worktrees/2026-10-05-claude-term-context-fixes.md` on
  `claude/term-context-fixes-20261005`)
- Role: implementer
- Branch: `claude/desktop-close-order-20261005`, stacked on `claude/code-cleanup-20261004` at
  `044fc5a`, because that branch rewrites `impulse-desktop/src/runtime.rs`; merge it after the
  cleanup branch
- Worktree: `../IMPULSE-rs.wt-cleanup` (the cleanup lane's worktree, switched to this branch to
  reuse its build cache)
- Owned paths: `close_agent` and the governed launch-failure paths in
  `impulse-desktop/src/runtime.rs`, and this card
- Blocked paths: `impulse-desktop/{Cargo.toml,src/ui.rs,src/views.rs,tests/desktop_contract.rs}`,
  `Cargo.toml`, `Cargo.lock`, `CONTEXT.md`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: implemented; gate in progress. Not merged; needs a PR after the cleanup branch.

## What changed
- `close_agent` held the lifecycle ordering lock while it killed the pane. Every pane's output
  callback takes that lock, so output and lifecycle calls froze in every pane for as long as the
  kill took: at least 200 ms for a child that ignores SIGHUP (portable-pty waits that long before
  SIGKILL), and with the cleanup branch's unbounded `kill()`, indefinitely for a child that cannot
  finish exiting. The record now leaves the runtime under the lock, so none of the pane's output
  is delivered after it, and the kill runs outside the lock, which is taken again to enqueue the
  exit or reinsert the record if the kill failed.
- The governed launch-failure paths killed the backend before opening the launch gate, while the
  pane's callbacks were parked on it, so nothing drained the PTY of the child being killed. The
  gate now opens first: the callbacks find no runtime record (none is installed yet) and return.

## Evidence
- `test_close_does_not_hold_the_lifecycle_lock_while_killing` closes a pane that ignores SIGHUP
  and, meanwhile, an ordinary pane; the ordinary close finishes first. With the kill moved back
  under the lock it fails ("closing the ordinary pane waited for the slow kill").
- The gate reorder has no test that fails without it here: on this machine a killed child is
  reaped within about 240 ms even while its output is parked, so the stuck exit the review
  reproduced elsewhere does not occur. The existing launch-failure tests pass.
- `impulse-desktop` tests: 177 unit, 83 contract, and the rest pass; clippy clean with default
  features and with `--no-default-features`; fmt clean.
- Gate: pending.
