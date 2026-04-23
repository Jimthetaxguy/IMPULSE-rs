---
id: STORY-195
title: Tranche C — cutover, archive impulse-gui + impulse-term
spec: SPEC-EGUI-RETIRE
status: backlog
priority: medium
depends_on: STORY-185
created: 2026-04-23
---

# Tranche C — cutover (Loops 195-199)

## Goal

Make the Dioxus supervisor the default desktop binary. Remove the egui
crates from the workspace.

## Steps

1. Promote `experimental-runtime` to default `desktop` feature in `impulse-supervisor`
2. Update `default-members` in workspace `Cargo.toml` to drop `impulse-gui` + `impulse-term`
3. Move `impulse-gui/` and `impulse-term/` to `_archive-YYYY-MM-DD-LXX/`
4. Update top-level `CLAUDE.md` workspace inventory
5. Smoke test: `cargo run -p impulse-supervisor` launches the new binary
6. Update `README.md` + docs to reference the new binary

## Acceptance

- [ ] No remaining workspace crate depends on `egui` or `eframe`
- [ ] `cargo run -p impulse-supervisor` opens a usable workbench
- [ ] All gates green
- [ ] Test count reflects the deletion (record delta in this story)
- [ ] SPEC-EGUI-RETIRE moves to `status: done`
