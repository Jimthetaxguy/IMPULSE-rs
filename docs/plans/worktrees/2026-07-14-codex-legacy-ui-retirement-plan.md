---
title: Legacy UI retirement planning lane
description: Replace the stale EGUI decommission plan with a current, evidence-backed three-track retirement contract
updated: 2026-07-14
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, cleanup, egui, dioxus, tauri, release]
---

# Legacy UI Retirement Planning Lane

## Lane Facts

- Owner: Codex (`codex/legacy-ui-retirement-plan`).
- Base: `agent/governed-runtime-producers` at `52bb410`.
- Scope: documentation and read-only inventory only.
- Canonical output: [`../EGUI-DECOMMISSION.md`](../EGUI-DECOMMISSION.md).
- No UI source, Cargo dependency, release job, compatibility adapter, branch history, or local
  generated artifact was removed or rewritten in this lane.

## Goal

Turn the old EGUI-only plan into an executable retirement contract for every nonfunctional UI path
without weakening the active Dioxus cockpit, ratatui surface, CLI, PTY backend, daemon authority, or
stacked governed-runtime work.

## Acceptance Criteria

- Current file counts, dependency edges, release wiring, Dioxus bridge status, and automation drift
  are recorded from the live branch.
- EGUI removal, retained-shell dead-affordance cleanup, and Tauri compatibility removal have
  separate entry gates and commit boundaries.
- The release pipeline cannot continue claiming a GUI artifact it does not build.
- KEEP, MIGRATE, REMOVE, stop conditions, recovery proof, critical path, and verification commands
  are explicit.
- Historical documents remain provenance rather than being rewritten to force a literal zero-hit
  repository scan.
- Existing roadmap markers continue to describe current code until physical removal lands.

## Audit Decisions

- `impulse-gui` is already excluded and is not a reliable fallback; its removal is gated on release
  truth, not on retaining Tauri forever.
- The Dioxus live eval bridge exists, but current browser smoke substitutes a test transport. A
  packaged real-bridge acceptance test is required before Tauri compatibility can be removed.
- `impulse-term`'s EGUI modules have no nonlegacy consumer. Preserve backend/context/paste and the
  boundary tests; remove the presentation files, including the otherwise unused theme module.
- The stale project skill, EGUI view guide, and running Ralph state are resurrection vectors and
  belong in the implementation plan.
- Disabled “coming soon” controls and artifact actions that only record local intent are not
  functional UI; the plan applies a wire-it-or-remove-it contract to retained shells.
- Static aggregate test counts are not acceptance criteria; live verification output is.

## Verification

- `python3 docs/validate_docs.py --self-test`
- `python3 docs/validate_docs.py --all`
- `git diff --check`

## Handoff

Implementation begins with R0 recovery/external-consumer proof and R1 release truth. It must not
combine EGUI deletion with Tauri deletion, and it must not flip the canonical `Legacy=...` marker
before the corresponding source and dependency removal is real.
