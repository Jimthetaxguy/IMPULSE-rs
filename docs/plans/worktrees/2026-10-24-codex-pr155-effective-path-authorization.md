---
title: PR 155 Effective Path Authorization
description: Close default-path and read-only-path bypasses in build-hygiene tools.
created: 2026-10-24
updated: 2026-10-24
author: James Pustorino
agent: codex/Codex
type: work-card
status: active
summary: Rebase the scan-path sandbox change onto the merged CI foundation, validate the exact paths each operation consumes, and verify both bypasses through ToolExecutor.
next_steps:
  - Obtain a read-only follow-up review, push the refreshed branch, and rerun PR checks.
remaining:
  - Retarget the PR to main only after the fix and review are clean.
open_questions:
  - Residual symlink traversal under artifact roots needs a separate verified disposition.
---

# PR 155 Effective Path Authorization

## Lane Facts

- Owner: Codex
- Role: Integration and security-fix lane
- Branch: `fix/scan-paths-sandbox`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs-scanpaths`
- Owned paths: `impulse-rs/src/build_hygiene/mod.rs`, `impulse-rs/src/tooling/builtin/build_health.rs`, this work card
- Blocked/shared paths: `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, protocol/spec files, other worktrees
- Plan/spec: PR 155 plus the adversarial review findings
- Verification: focused Rust tests, workspace build/test/clippy/fmt, then fresh PR checks
- Latest status: rebased locally onto the merged CI foundation; effective-path authorization fix passes the full Rust workspace gate and an independent follow-up review

## Decisions

- 2026-10-24: Validate the paths actually consumed by each operation. Destructive tools require write-root authorization even when the executor's generic combined-capability check accepts a path through a read root.
- 2026-10-24: Check configured path candidates before existence filtering so absent defaults cannot become unchecked if they appear later.
- 2026-10-24: Enable the singular `path` override only for Sweep/Wipe; BuildHealth/CleanAll must authorize their `scan_paths` even if a request carries an undeclared extra `path` property.

## Changes

- `scan_path_candidates()` preserves expanded defaults before existence filtering; `parse_hygiene_config()` validates defaults or singular overrides against the required read/write roots.
- Added executor-boundary checks for both reported bypasses.

## Tests

- Added executor-boundary regressions for default scan roots and a singular wipe path located only under a read root.
- `CARGO_TARGET_DIR=/Users/jamespustorino/.cargo-target-lanes/codex-pr155 cargo test -p impulse-rs tooling::builtin::build_health::tests --lib --manifest-path impulse-rs/Cargo.toml`: 15 passed, 0 failed (2026-10-24).
- `CARGO_TARGET_DIR=/Users/jamespustorino/.cargo-target-lanes/codex-pr155 cargo check --workspace --manifest-path impulse-rs/Cargo.toml`: passed (2026-10-24).
- `CARGO_TARGET_DIR=/Users/jamespustorino/.cargo-target-lanes/codex-pr155 cargo test --workspace --manifest-path impulse-rs/Cargo.toml`: passed; existing ignored tests remain ignored (2026-10-24).
- `CARGO_TARGET_DIR=/Users/jamespustorino/.cargo-target-lanes/codex-pr155 cargo clippy --workspace --all-targets --manifest-path impulse-rs/Cargo.toml -- -D warnings`: passed (2026-10-24).
- `cargo fmt --manifest-path impulse-rs/Cargo.toml --all -- --check`: passed (2026-10-24).
- `git diff --check`: passed (2026-10-24).

## Review

- 2026-10-24: Independent security follow-up found and closed a mixed-input bypass: an undeclared `path` could previously authorize a different `scan_paths` list in BuildHealth/CleanAll. Re-review of the fix found no remaining blockers.

## Handoff Notes

- PR 155 must remain unmerged until a follow-up security review and fresh checks pass.
