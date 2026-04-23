---
id: STORY-180
title: Audit impulse-gui views — classify wired/embedded/dead/orphan
spec: SPEC-EGUI-RETIRE
status: done
priority: high
created: 2026-04-23
completed: 2026-04-23
---

# View audit (Plan 2 / Loop 180)

## Outcome

`docs/research/2026-04-23-impulse-gui-view-audit.md` classifies all 15 view
files in `impulse-gui/src/views/`. Honest correction inside the doc:
3 truly dead files (1,148 LOC), not the originally-claimed 6 — the other
3 are `impl TerminalsView { … }` extension blocks.

## Acceptance

- [x] Each of 15 view files classified WIRED / EMBEDDED / DEAD / ORPHAN
- [x] Re-verifiable via `grep` commands listed in the audit doc
- [x] Final port-set: 9 files / ~4,602 LOC
- [x] Reclassification note documents the false-positive correction
