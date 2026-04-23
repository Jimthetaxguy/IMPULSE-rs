---
id: STORY-183
title: "@impulse wire-format detector + framer (toolkit-neutral)"
spec: SPEC-EGUI-RETIRE
status: ready
priority: high
created: 2026-04-23
---

# @impulse wire format (Plan 2 / Loop 183) — NEXT

## Goal

Detect `@impulse <verb> [args…]` on PTY input bytes and frame as a JSON
command struct. **No verb implementations** — this is the parser/framer
only. Lives in `impulse-term-core` so any future renderer can consume it.

## Design

Hook intercepts each line of PTY input. If it starts with `@impulse `,
parse the verb + args into:

```rust
pub struct ImpulseCommand {
    pub verb: String,
    pub args: Vec<String>,
    pub source_pane_id: Option<Uuid>, // from IMPULSE_WORKER_PANE_ID
}
```

Serialize as one JSON line, write to `IMPULSE_CMD_SOCKET`.

## Acceptance

- [ ] New module `impulse-term-core/src/impulse_cmd.rs`
- [ ] Round-trip test: feed bytes `@impulse summarize tab=2\n`, get parsed `ImpulseCommand { verb: "summarize", args: ["tab=2"], … }`
- [ ] Non-`@impulse` lines pass through unchanged
- [ ] Malformed `@impulse` lines: log warning, pass through (don't swallow user input)
- [ ] Serde round-trip test on `ImpulseCommand`
- [ ] All gates green
