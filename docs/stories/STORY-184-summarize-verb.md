---
id: STORY-184
title: "@impulse summarize — first verb implementation"
spec: SPEC-EGUI-RETIRE
status: backlog
priority: high
depends_on: STORY-183
created: 2026-04-23
---

# @impulse summarize (Plan 2 / Loop 184)

## Goal

End-to-end demo of the wire format. Worker pane types
`@impulse summarize tab=2`. Daemon receives JSON command, reads the
target pane's recent block history (from `block_store()`), produces a
short text summary, returns it for display in the supervisor pane.

## Acceptance

- [ ] Daemon dispatches `summarize` verb to a handler
- [ ] Handler reads `block_store().blocks` for the target pane
- [ ] Summary is plain text, ≤ 200 words
- [ ] Worker that emitted the command sees the result echoed in its own pane (smoke loop)
- [ ] Integration test: spawn 2 panes, type `@impulse summarize tab=1` in pane 2, assert summary appears
