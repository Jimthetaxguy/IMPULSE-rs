---
title: Code cleanup and review sweep
description: Work card for code-cleanup-20261004
updated: 2026-10-04
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, cleanup, review]
---

# Code cleanup and review sweep

## Lane Facts
- Owner: claude (Opus 5.5, session with James; goal "continue cleaning up and reviewing the code")
- Role: reviewer and implementer
- Branch: `claude/code-cleanup-20261004`, from `origin/main` at `7481457`
- Worktree: `../IMPULSE-rs.wt-cleanup`
- Owned paths: this card; fixes for review findings in modules the pending branch stack does not
  touch (see Blocked paths)
- Shared paths edited: `CLAUDE.md` (Architecture section only)
- Blocked paths (edited by the unmerged stack: photon, blackboard, model provider, Dioxus 0.7):
  `impulse-rs/src/ion_repl/**`, `impulse-rs/src/llm_backends/**`, `impulse-rs/src/model_endpoint/**`,
  `impulse-rs/src/state/config.rs`, `impulse-rs/src/handlers/config.rs`, `impulse-rs/src/test_support.rs`,
  `impulse-rs/src/lib.rs`, `impulse-rs/impulse-desktop/{Cargo.toml,src/ui.rs,src/views.rs,tests/desktop_contract.rs}`,
  `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `VISION.md`, `CONTEXT.md`
- Verification: `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings` (default and `--no-default-features`),
  `cargo fmt --all -- --check`, `python3 docs/validate_docs.py`
- Latest status: in progress

## Decisions
- 2026-10-04: review `main` module by module with read-only reviewers, verify every finding against
  the code before fixing, and fix only what reproduces. Findings inside blocked paths are recorded
  here for the owning branch instead of being fixed on this one.
- 2026-10-04: `CLAUDE.md`'s Ion bullet and daemon paragraph described superseded code
  (`checkout_agent`/`checkin_agent`, a `security`-CLI Keychain writer, "no FileWrite guardrail rule",
  a two-tool confirmation gate). Rewritten as current invariants with pointers; chronology stays in
  Git history and `impulse-rs/impulse-ion/TUI_SPEC.md`.

## Handoff Notes
- Restriction-lint audit (`clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`,
  `dbg_macro`, `undocumented_unsafe_blocks`) over non-test code at `7481457`: 27 sites, all
  `expect`/`unwrap` backed by local invariants (piped stdio, generated IDs, guarded `is_empty`).
- Dependabot "security update not possible" failures on `main` (2026-09-27): the three open alerts
  are transitive pins, not lockfile bumps. `rand` 0.7.3 comes from `phf_generator` 0.8 (build-time
  codegen under `selectors`), `lru` 0.12.5 from `ratatui` 0.28.1, `glib` 0.18.5 from the GTK stack
  of `wry`/`tao` (Linux webview, both Dioxus desktop and the optional legacy Tauri adapter).
