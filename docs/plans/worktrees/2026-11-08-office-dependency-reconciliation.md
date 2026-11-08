---
title: Office Dependency Reconciliation
description: Patch the existing office ZIP dependency graph while preserving bounded readers and real test fixtures.
author: codex/Codex
updated: 2026-11-08
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, security, verification]
---

# Office Dependency Reconciliation

## Lane Facts
- Owner: codex/Codex; implementation and local gates; root agent independent review before commit.
- Branch: `codex/office-dependency-reconciliation-20261108`.
- Worktree: `.worktrees/office-dependency-reconciliation-20261108`.
- Base: fresh origin/main `c46e9ee28346356736b76d865a7ce0c647b9ffb4`; neither earlier reconciliation lane is stacked.
- Owned paths: root package Cargo.toml/lock, src/office/test_fixtures.rs, src/office/bounded.rs, src/ion_repl/tool_document.rs, src/office/excel.rs only if required for calamine compatibility, the existing cfg(test) parser regression region in src/office/mod.rs, and this card (Rust paths relative to impulse-rs/).
- Blocked paths: all other sources/instructions, GUI lockfile, original dirty trees, desktop/archived harness dependencies and runtime/provider configuration.
- Plan: approved bounded office proposal in the existing machine-wide dependency triage; canonical Rust contract applies.
- Verification: inspect every dependency delta; targeted real synthetic office/document fixtures, required isolated workspace build/check/test/strict all-target Clippy/format, no-default library tests/strict Clippy and declared MSRV assessment.
- Latest status: current-host and declared-floor gates passed; root source review passed; final workcard/result review pending before staging/commit.

## Decisions and acceptance
- Direct ZIP at least9.0.5 with defaults disabled/deflate; calamine0.37; modern rust_xlsxwriter0.100 dev-only; remove unused production docx dependency.
- Replace only the three existing docx fixture builders with real ZIP/OOXML containers through existing ZIP APIs, no production writer or new runtime dependency.
- Preserve current file/inflation/entry/sheet/cell/character/Word-section caps, streaming extraction, document semantics, path/capability policy and office feature contract. Stop for broader behavior changes or a new owned path.
- GLib optional Linux desktop, rand optional desktop build graph and archived Zod remain separate, not claimed remediated.
- Root package declares Rust1.96; managed minimal Rust1.96.0 is now installed for explicit floor verification. The default remains stable Rust1.98.1.

## Changes
- Targeted resolution selects ZIP9.0.5, calamine0.37 and test-only rust_xlsxwriter0.100; removes unused docx-rs and old ZIP versions. Unrelated dependency selections retained.
- Existing synthetic DOCX fixtures use real ZIP/OOXML packages; XLSX fixture APIs adapted. Reader filename decoding errors propagate as contextual failures; all numeric caps and tool policy remain unchanged.
- Root separately reviewed/approved the existing office/mod.rs test region after the new parser rejected a malformed shared-string reference with a normal error. Split that case into a named rejection test with its precise diagnostic; existing overflow and actual-panic containment cases remain unchanged. parse_document still returns its existing string error API.

## Tests
- Initial office pass found a stale panic-message expectation; root reviewed and approved the test-only split. Office then passed56 cases and document suite100, with workspace/no-default gates passing on the pre-final encoding configuration.
- Dependency-feature review caught that Word's quick-xml0.31 had inherited encoding support from old calamine. A real synthetic Windows-1252 DOCX regression failed without the explicit feature, proving the compatibility boundary. Retained the existing encoding feature explicitly; the regression passed and all final current-host gates passed.
- Final focused suites:56 office and101 document tests passed; workspace:4,019 passed, zero failed,11 ignored; no-default root library:2,977 passed, zero failed,6 ignored. Ignored tests were not exercised.
- Final isolated locked workspace build/check/test, strict all-target Clippy, format, no-default library tests and no-default strict workspace all-target Clippy all passed. Source scans/whitespace passed.
- Declared floor Rust1.96.0: locked root-package default check and tests passed (3,295 passed, zero failed,9 ignored); locked no-default check and library tests passed (2,977 passed, zero failed,6 ignored). These floor checks cover the changed root package; the full workspace/Clippy/format gates above used Rust1.98.1.
- Tool inventory: official rustup installed only cargo/rust-std/rustc for1.96.0 with the minimal profile. The installer automatically updated rustup1.29.1 to1.29.3. Active/default stable Rust1.98.1 stayed unchanged; no default/global/workspace override was set and no MCP registration or runtime adoption occurred. Official output, component inventory and before/after default checks are retained in the audit lane.
- No live providers/private files/exploit harnesses were used. Manual benchmarks and existing live tests remain ignored.

## Handoff Notes
- Exact current diff, source hashes, dependency graphs and test results require independent root review before staging/commit.
- No merges, runtime adoption, original retirement, suppression or vendor fork.
- Rollback uses exact preimages/reviewed reverse patch for owned paths; preserve concurrent work.
