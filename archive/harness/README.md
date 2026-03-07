---
status: pre-pivot-reference
phase: superseded
audience: reference
tags: [archive, pre-pivot, opencode, swarm]
last_updated: 2026-02-21
---

# harness/ — Pre-Pivot Reference Code

> ⚠️ **SUPERSEDED** — This directory contains skeleton code written against the original
> OpenCode plugin SDK architecture. It does NOT reflect the current Impulse spec.
>
> **Current spec:** See [`docs/spec/PRODUCT-SPEC-v2.md`](../docs/spec/PRODUCT-SPEC-v2.md)
> **Build target:** [`impulse/`](../impulse/) (not yet created — see checklist)

---

## Why This Exists

This code was written during Phase 0 when the project was planned as a long-running daemon that connected to OpenCode via a plugin SDK. The architecture was:

```
Long-running daemon (harness/)
  → Connect to OpenCode REST API
  → Subscribe to hooks (message.updated, tool.execute.after)
  → Store events in SQLite + sqlite-vec
  → Detect patterns via cosine similarity
  → Inject back via OpenCode compaction hook
```

## Why It Was Superseded

Phase 0 research (see [ADR-0001](../docs/decisions/0001-claude-code-primary.md)) revealed:

1. The `opencode-plugin-sdk` interfaces (`PluginSDK`, `SessionContext`, etc.) were fabricated — they don't exist in the real OpenCode source code
2. Claude Code hooks (shell commands invoked by Claude Code itself) map 1:1 to all four Impulse requirements with zero workarounds
3. The "daemon" model adds unnecessary complexity — Claude Code already manages the process lifecycle

## Current Architecture (What to Build Instead)

The new spec uses 4 shell scripts invoked by Claude Code hooks:

```
Claude Code Session
  → SessionStart hook → impulse-session-start (reads 3 files → stdout → context)
  → PostToolUse hook  → impulse-post-tool    (updates LIVE_STATE.json)
  → PreCompact hook   → impulse-pre-compact  (reads GENOME.md top 50 lines → stdout)
  → SessionEnd hook   → impulse-session-end  (transcript → LLM → appends to files)
```

Files managed:
- `.impulse/GENOME.md` — Permanent architectural decisions
- `.impulse/LIVE_STATE.json` — Active agent registry
- `.impulse/HISTORY_INDEX.md` — Session summaries

## What to Salvage

Some algorithmic work here is useful as reference:

| File | What's Still Useful |
|------|---------------------|
| `src/types.ts` | Zod schema patterns, error code organization |
| `src/pattern/detector.ts` | Anti-echo logic (`isSWARMInjection`), rate limiting pattern |
| `src/db/database.ts` | WAL + NORMAL pragma settings, TTL pattern |
| `src/test/fixtures.ts` | Factory function patterns for tests |
| `src/utils/logger.ts` | Pino logger setup |

## Known Bugs (Do Not Repeat)

1. **Wrong import in writer.ts:** `DatabaseConnection` imported from `../types.js` but defined in `../db/database.ts`
2. **Non-atomic writes:** `writeFileSync` used instead of temp-file + rename (corrupts on crash)
3. **Export name shadow:** `export const Database = DatabaseConnection` shadows the `better-sqlite3` import
4. **sqlite-vec schema:** Metadata columns (`agent_id`, `partition`) don't work in `vec0` virtual tables — need a shadow table
5. **Random embeddings:** `embedContext()` uses `Math.random()` — similarity comparisons are meaningless

## Dependencies (Reference Only)

| Package | Status |
|---------|--------|
| `opencode-plugin-sdk` | Fictitious — does not exist |
| `better-sqlite3` | Valid, but Phase 1 doesn't need a DB |
| `sqlite-vec` | Valid for Phase 2+, not Phase 1 |
| `zod` | Valid — keep in new implementation |
| `pino` | Valid — keep in new implementation |

---

_Pre-pivot reference created: 2026-02-20 | Superseded: 2026-02-21 | See PRODUCT-SPEC-v2.md_
