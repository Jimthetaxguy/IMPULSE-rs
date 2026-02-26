---
title: Contract Alignment Sprint 1
description: Rust-canonical contract alignment, supersession labeling, and drift checks
version: '1.0'
updated: 2026-02-23
type: doc
category: session_logs
phase: all
status: active
audience: builders
tags: [session-log, contract, rust, governance]
---

# Contract Alignment Sprint 1

## Summary

Implemented Rust-canonical contract alignment for Cockpit to remove ambiguity between legacy TypeScript-era planning docs and current `cockpit-rs` implementation.

## Delivered

- Added canonical contract document:
  - `docs/spec/RUST-CANONICAL-CONTRACT.md`
- Updated source-of-truth routing in:
  - `AGENTS.md`
  - `CLAUDE.md`
  - `docs/INDEX.md`
  - `docs/SUMMARY.md`
- Added superseded banners/status to conflicting TypeScript-era docs:
  - `docs/spec/PRODUCT-SPEC-v2.md`
  - `docs/phases/PHASE1-CHECKLIST.md`
  - `docs/phases/PHASE1.5-COORDINATION.md`
- Added drift-prevention checks in `docs/validate_docs.py` with line-precise contract failures
- Added workflow command in `mise.toml`:
  - `mise run docs-contract`
- Added release governance template:
  - `docs/guides/RELEASE-NOTES-TEMPLATE.md`

## Verification

- Documentation validation command executed.
- Contract validation command executed.
- Rust tests executed to verify no regressions.

## Follow-up

- Align additional legacy phase/spec docs to canonical status labels over time.
- Add CI job wiring for `docs-contract` if not already present in pipeline.

