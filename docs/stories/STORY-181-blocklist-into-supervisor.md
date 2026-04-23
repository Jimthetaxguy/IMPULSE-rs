---
id: STORY-181
title: Wire BlockListView into supervisor worker panes
spec: SPEC-EGUI-RETIRE
status: done
priority: high
created: 2026-04-23
completed: 2026-04-23
commit: 1d64f2b
---

# BlockListView in supervisor (Plan 2 / Loop 181)

## Outcome

`PtyTerminalView` gets a `show_blocks: bool` prop (default false, preserves
existing callers). When true, renders `BlockListView` beneath the live grid,
fed by the same `PtySource` — one PTY child drives both surfaces with no
duplicated state. Supervisor's `LiveWorkerPane` opts in.

## Acceptance

- [x] New prop is opt-in (default false)
- [x] No second PTY child spawned
- [x] New test: `test_props_show_blocks_can_be_enabled`
- [x] `cargo test -p impulse-term-dioxus --features desktop` 83 → 84
