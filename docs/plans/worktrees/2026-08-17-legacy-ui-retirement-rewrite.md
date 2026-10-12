---
title: Legacy UI retirement rewrite
description: Work card for rewriting the three-track UI retirement plan on current main
updated: 2026-08-17
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, lane, handoff, egui, dioxus, retirement]
---

# Legacy UI retirement rewrite

## Lane Facts
- Owner: Cursor
- Role: implementer
- Branch: `cursor/legacy-ui-retirement-rewrite`
- Worktree: `.worktrees/legacy-ui-retirement-rewrite`
- Owned paths: `docs/plans/EGUI-DECOMMISSION.md`; `docs/ROADMAP-PLAN.md` (Cleanup row only); `AGENTS.md` (Desktop Shell Status bullets only); this work card
- Blocked/shared paths: `CLAUDE.md`; `CONTEXT.md`; `VISION.md`; `README.md`; `docs/INDEX.md`; `docs/SUMMARY.md`; `docs/SUMMARY.yaml`; `docs/validate_docs.py`; protocol/spec docs; daemon/ops Rust; `impulse-gui/` deletion; `.github` workflows; `codex/legacy-ui-retirement-plan` (PR #17); `codex/dioxus-egui-retirement`
- Plan/spec: rewrite the June 30 egui-only decommission plan as the three-track KEEP/MIGRATE/REMOVE contract on `origin/main` `99396f9`. No ADR. Close PR #17 after a replacement PR exists.
- Verification: `python3 docs/validate_docs.py --self-test && python3 docs/validate_docs.py --all` (accept known `RUST-MULTI-AGENT` failures only)
- Latest status: replacement PR https://github.com/Jimthetaxguy/IMPULSE-rs/pull/28 opened; GitHub draft #17 closed

## Decisions
- 2026-08-17: Do not rebase, cherry-pick, or conflict-resolve `codex/legacy-ui-retirement-plan`. That 21-commit GitHub diff is stale governed-runtime lineage.
- 2026-08-17: No ADR-0016 in this PR. Implementation later uses the next free ADR number (0016 as of this date).
- 2026-08-17: Do not flip `Legacy=egui compile-maintenance only` until physical removal.

## Changes
- Replaced `docs/plans/EGUI-DECOMMISSION.md` with the three-track plan on `99396f9`.
- Updated `docs/ROADMAP-PLAN.md` Cleanup row and `AGENTS.md` Desktop Shell Status.

## Tests
- Pending: docs validator.

## Handoff Notes
- Physical crate deletion remains a later implementation lane. Do not merge `codex/dioxus-egui-retirement` from this card.
