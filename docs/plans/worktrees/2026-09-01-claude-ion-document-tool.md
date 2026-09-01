---
title: Ion Document Read Tool
description: Work card for claude-ion-document-tool-20260901 (bounded, pageable document analysis inside Ion's tool loop)
updated: 2026-09-01
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, ion, document-analysis, tools, primitives]
---

# Ion Document Read Tool

## Lane Facts

- Owner: Claude (Fable 5.1), iteration 2 of goal `impulse-primitives-meta-harness-2026-09`.
- Role: implementation lane for one beyond-software capability.
- Branch: `claude/ion-document-tool-20260901`, stacked on `claude/loop-contract-20260901`
  (PR #39) so the two lanes share the rustfmt fix and the loop contract the tool is designed for.
- Worktree: `.worktrees/ion-document-tool-20260901` (repository-relative).
- Owned paths:
  - `impulse-rs/src/ion_repl/tool_document.rs` (new)
  - `impulse-rs/src/ion_repl/registry.rs`, `impulse-rs/src/ion_repl/mod.rs` (module line and
    help test), `impulse-rs/src/ion_repl/chat.rs` (gate comment and one test)
  - `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`
  - `CONTEXT.md` (one glossary entry), this work card
- Blocked/shared paths: everything owned by the live Codex lanes (`impulse-rs/src/daemon/*`,
  `impulse-rs/impulse-desktop/*`, `.github/workflows/*`, `impulse-rs/scripts/*`); `Cargo.toml`,
  `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, canonical contract; `src/office/*` and
  `src/tooling/document/*` (read, not modified).
- Plan/spec: `docs/superpowers/specs/2026-09-01-ion-document-tool-design.md`.
- Verification (isolated `CARGO_TARGET_DIR`): `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `python3 docs/validate_docs.py --all`.
- Latest status: implementation complete and gated on this lane: `cargo build --workspace`
  clean, `cargo test --workspace` 2282 passed / 0 failed / 9 ignored (21 new tests over the
  loop-contract lane), strict Clippy clean, rustfmt clean. `docs/validate_docs.py --all` reports
  only failures that pre-exist on `main`; all lane docs validate. Pushed; stacked draft PR
  [#40](https://github.com/Jimthetaxguy/IMPULSE-rs/pull/40) open on top of PR #39.

## Decisions

- 2026-09-01: Build an Ion-native `ReplTool` over `office::parse_document` rather than bridging
  the existing `document_parse` dynamic tool. The dynamic tools return whole documents in one
  payload; a tool loop under ADR-0017 needs an outline plus character-offset paging so one large
  workbook cannot flood the context budget.
- 2026-09-01: Read-only, so ungated like `file_read`. Relative paths resolve against the REPL's
  launch directory; absolute paths are accepted because everyday documents live outside repos.
- 2026-09-01: Sheet names come from the parser's own `=== Sheet: name ===` headers so the
  section table matches the chunk list exactly without reopening the workbook.
- 2026-09-01: `max_chars` above the cap is clamped, not rejected; zero is rejected.

## Changes

- `document_read` tool: `outline`, `sheet`, `offset`, `max_chars`; payload `DocumentWindow`
  with a section table, totals, `truncated`, and `next_offset`; rendered text carries an explicit
  continuation hint.
- Registered in `ReplToolRegistry::with_defaults` (six tools now).

## Tests

- Unit: schema, argument parsing and clamping, character windows (including multi-byte),
  sheet-name recovery, sheet selection errors, path resolution errors, serde round trips, render.
- Fixture-backed (`office-support`): CSV read and paging loop, outline mode, generated XLSX with
  two sheets and sheet selection, generated DOCX paragraphs, argument and path errors. These are
  the first end-to-end tests of the `office` parsers in the repository.
- Executor: `document_read` never consults the confirmation hook and returns a bounded window.

## Handoff Notes

- The first `cargo test --lib -- ion_repl` run on this lane failed eleven tests at temp-directory
  creation, including pre-existing `tool_verify` tests; two immediate reruns and the full gate
  passed. Treated as a transient host temp-dir hiccup, recorded here in case it recurs.
- `src/tooling/builtin/document_extract.rs` still returns "Python extraction not yet wired" for
  its default path; it was left untouched and is a candidate for a later iteration.
