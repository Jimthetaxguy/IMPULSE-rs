---
title: Codex Host Readiness Smoke
description: Work card for codex-host-readiness-smoke
updated: 2026-06-14
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, dioxus, host, xterm, smoke]
---

# Codex Host Readiness Smoke

## Lane Facts
- Owner: Codex
- Role: Post-merge implementation lane for Impulse desktop host-readiness confidence
- Branch: agent/codex-dioxus-host-goal-cleanup, based on origin/main after PR #9
- Worktree: <repo>
- Owned paths:
  - impulse-rs/impulse-desktop/scripts/
  - impulse-rs/impulse-desktop/examples/
  - impulse-rs/impulse-desktop/package.json
  - impulse-rs/impulse-desktop/README.md
  - impulse-rs/impulse-desktop/tests/desktop_contract.rs
  - _working-files/20260613-codex-phase-ab-live-bridge-workspaces.md
  - _working-files/20260613-impulse-interface-dioxus-roadmap-spec.html
- Blocked/shared paths:
  - Cargo.toml and Cargo.lock unless a dependency change becomes unavoidable
  - AGENTS.md, CLAUDE.md, docs/INDEX.md, docs/SUMMARY.*
  - Any egui/impulse-gui path
- Plan/spec: _working-files/20260613-impulse-interface-dioxus-roadmap-spec.html, Phase F host-readiness slice
- Verification:
  - cargo fmt --all -- --check
  - node --check scripts/vendor_xterm_assets.mjs scripts/visual_smoke.mjs scripts/host_readiness_smoke.mjs
  - CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run visual:smoke
  - CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run host:smoke
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo clippy -p impulse-desktop -- -D warnings
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --workspace
  - git diff --check
- Latest status: complete for bounded host-readiness smoke; Dioxus Desktop launch scaffold supersedes Tauri scaffold as next frontier

## Decisions
- 2026-06-14: Treat the post-PR #9 frontier as host/package readiness, not another Dioxus shell conversion pass.
- 2026-06-14: Keep the first host-readiness gate webview-like and deterministic: local xterm assets, mocked host IPC, terminal interop script, and browser-observable assertions for input, resize, output, and exit.
- 2026-06-14: Do not claim packaged app readiness from this smoke. It proves the asset/interop seam before a real Dioxus Desktop launch scaffold exists.

## Changes
- Added `examples/emit_terminal_interop_script.rs` to expose the exact Rust-owned terminal interop script to smoke tooling.
- Added `scripts/host_readiness_smoke.mjs`, covering xterm mount, `agent_write`, `agent_resize`, `terminal_output`, and `terminal_exit`.
- Added `npm run host:smoke`; it now defaults to the Dioxus-native adapter, with `npm run legacy:host:smoke` covering the compatibility adapter.
- Documented the visual/host smoke distinction in `impulse-desktop/README.md`.

## Tests
- Passed: `node --check scripts/host_readiness_smoke.mjs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop test_host_readiness_smoke_script_is_declared`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run host:smoke`.
- Passed: `cargo fmt --all -- --check`.
- Passed: `node --check scripts/vendor_xterm_assets.mjs && node --check scripts/visual_smoke.mjs && node --check scripts/host_readiness_smoke.mjs`.
- Passed: HTML embedded script syntax check for `_working-files/20260613-impulse-interface-dioxus-roadmap-spec.html`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run visual:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run host:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo clippy -p impulse-desktop -- -D warnings`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop --features legacy-tauri-runtime --locked`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo build --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo clippy --workspace -- -D warnings`.
- Passed: `python3 docs/validate_docs.py --contract`.
- Passed: `python3 docs/validate_docs.py --all`.
- Passed: `git diff --check`.

## Handoff Notes
- Host-readiness is now covered by a webview-like Chromium fixture, not a packaged app boot. The follow-on lane changed direction after user feedback: add the smallest real Dioxus Desktop launch scaffold and treat legacy Tauri-shaped code as compatibility only.
