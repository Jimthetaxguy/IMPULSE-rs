---
id: STORY-185
title: Tranche B — port 9 surviving views to Dioxus
spec: SPEC-EGUI-RETIRE
status: backlog
priority: medium
depends_on: STORY-184
created: 2026-04-23
---

# Tranche B — workbench views (Loops 185-194)

## Goal

Port the 9 surviving `impulse-gui` views to Dioxus inside
`impulse-supervisor`. Order set by audit to ship demonstrable user value
early:

1. `sidebar` + `status_bar` + `command_palette` (~650 LOC) — supervisor finally *looks* like an app
2. `memory.rs` (wrapper) + `sessions.rs` (739 LOC) — highest-value view
3. `genome` + `search` (554 LOC) — read-only memory views
4. `settings.rs` (823 LOC) — mostly forms
5. `terminals.rs` (1,767 LOC) — replaces `TerminalPanel` with `PtyTerminalView` + `BlockListView`
6. `overview.rs` (294 LOC) — the workbench dashboard

## Boundary

Each numbered item is one substory (STORY-186 through STORY-194). Split
out as work begins. Each must:

- Land behind a feature flag until parity with egui equivalent
- Hold the verification gate green
- Update SPEC-EGUI-RETIRE story list as items complete

## Acceptance

- [ ] All 9 views renderable from supervisor
- [ ] `impulse-gui` no longer needed for any user-facing flow
