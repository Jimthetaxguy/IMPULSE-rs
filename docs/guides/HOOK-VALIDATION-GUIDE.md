---
title: Hook Validation Guide
description: Prove the real Claude Code memory loop before trusting higher-level GUI behavior
version: '1.0'
updated: 2026-03-05
type: guide
category: validation
phase: now
status: active
audience: builder
tags: [hooks, validation, claude-code, memory-loop]
last_updated: 2026-03-05
---

# Hook Validation Guide

Impulse should not claim cross-session memory works until the real Claude hook loop is proven with evidence.

## Goal

Validate this sequence with a real Claude Code session:

1. `SessionStart` injects prior context that Claude can actually use
2. The session does real work
3. `SessionEnd` records enough information to persist memory
4. The next session recalls the persisted summary or decisions

## Generate the kit

From `impulse-rs/`:

```bash
cargo run -- validate-hooks --platform claude-code
```

This writes `.impulse/validation/claude-code/` with:

- `session-start-sentinel.sh`
- `session-end-capture.sh`
- `settings.local.json`
- `README.md`
- `evidence.md`

## Run the validation

1. Start the daemon:

```bash
cargo run -- daemon
```

2. Register the generated `settings.local.json` in the Claude project.
3. Start a fresh Claude Code session in the project.
4. Ask Claude what `IMPULSE_HOOK_SENTINEL` means.
5. Do a small real task that touches files.
6. End the session.
7. Start a second session.
8. Ask Claude what happened in the last Impulse session.
9. Compare Claude's answer with `.impulse/HISTORY.jsonl` and `GENOME.md`.

## Pass criteria

- Claude explains the sentinel as startup context, not as random shell text.
- `session-end.log` records a transcript path.
- `.impulse/HISTORY.jsonl` gains a new session entry.
- The next session recalls facts that match the persisted summary or decisions.

## If it fails

- Treat hook truth as canonical.
- Do not trust richer GUI memory claims.
- Reduce product messaging to headless daemon/hooks + read-only inspection until the hook path is fixed.
