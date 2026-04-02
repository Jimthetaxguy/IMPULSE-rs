---
status: accepted
phase: 1
audience: builder
tags: [decision, agent, claude-code]
last_updated: 2026-02-20
---

# ADR-001: Claude Code as Primary Integration Target

> **Status:** Accepted
> **Date:** 2026-02-20
> **Supersedes:** historical ADR `0001-opencode-first.md` (not present in the current workspace archive)

---

## Context

Impulse requires four lifecycle hooks to function:

1. **Session start** — Inject persistent knowledge (GENOME.md) into agent context
2. **Post-tool-use** — Track file activity for multi-agent awareness (LIVE_STATE.json)
3. **Session end** — Extract decisions from the transcript into GENOME.md
4. **Pre-compaction** — Preserve critical knowledge through context window compaction

The impulse-plugin was originally built against OpenCode's Plugin SDK. Phase 0 research (AGENT-HARNESS-ANALYSIS.md) revealed that the plugin's interfaces (`PluginSDK`, `SessionContext`, `ToolContext`, `CompactionContext`) are entirely fabricated — none exist in the real OpenCode SDK.

### The OpenCode Gap (AGENT-HARNESS-ANALYSIS.md §1.3)

| Required Hook | OpenCode Reality | Severity |
|---------------|------------------|----------|
| `session.start` | **Does not exist.** Closest workaround: `experimental.chat.system.transform` (fires every LLM call, not just session start) | CRITICAL |
| `session.end` | **Does not exist.** No lifecycle event when a session terminates. Workaround: "extract on next start" with 1-session delay | CRITICAL |
| `tool.execute.after` | Exists, but signature differs from assumed `ToolContext` | HIGH |
| `experimental.session.compacting` | Exists, but uses `output.context[]` mutation, not `injectPreCompaction()` | HIGH |

The `PluginSDK.on()` registration model, `appendSystemPrompt()`, `getSessionTranscript()`, and `getModifiedFiles()` — all used in impulse-plugin — returned zero matches across the entire OpenCode codebase.

### The Claude Code Match (AGENT-HARNESS-ANALYSIS.md §2.1-2.2)

Claude Code provides all four hooks with a 1:1 mapping:

| Impulse Need | Claude Code Hook | Input | Output Mechanism |
|--------------|-----------------|-------|------------------|
| Session start | `SessionStart` | `{ session_id, transcript_path, cwd }` via stdin JSON | stdout → injected as context |
| Post-tool-use | `PostToolUse` | `{ tool, args, output }` via stdin JSON | Fire-and-forget (file I/O) |
| Session end | `SessionEnd` | `{ session_id, transcript_path, reason }` via stdin JSON | Fire-and-forget (extraction + file I/O) |
| Pre-compaction | `PreCompact` | `{ session_id }` via stdin JSON | stdout → survives compaction |

Key advantages:
- `transcript_path` is provided natively — no fabricated `getSessionTranscript()` needed
- `SessionEnd` fires on all exit reasons (`clear`, `logout`, `prompt_input_exit`)
- Hooks are shell commands, so core logic can be written in any language
- 16 hook events total, stable and documented API

---

## Decision

**Claude Code is the primary integration target for Impulse Phase 1.** OpenCode support is deferred to Phase 1.5 as a thin adapter layer.

### Implementation Approach

Four shell scripts (invoking Bun CLIs) configured in `.claude/settings.local.json`:

```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{
        "type": "command",
        "command": "impulse-session-start"
      }]
    }],
    "PostToolUse": [{
      "matcher": "Write|Edit|Bash",
      "hooks": [{
        "type": "command",
        "command": "impulse-post-tool-use"
      }]
    }],
    "SessionEnd": [{
      "hooks": [{
        "type": "command",
        "command": "impulse-session-end"
      }]
    }],
    "PreCompact": [{
      "hooks": [{
        "type": "command",
        "command": "impulse-pre-compact"
      }]
    }]
  }
}
```

Each CLI reads JSON from stdin, performs its operation, and (for SessionStart/PreCompact) prints context to stdout.

---

## Consequences

### Positive

- **1:1 hook mapping** — No workarounds, no fabricated interfaces, no heuristic-based substitutes
- **Stable API** — Claude Code hooks are documented with 16 events; not experimental
- **`transcript_path` provided natively** — SessionEnd can read the full JSONL transcript directly
- **Language-agnostic** — Shell-command model means Bun/Node/Python/Rust all work as hook implementations
- **Larger user base** — Claude Code is more widely deployed than OpenCode

### Negative

- **Out-of-process spawn latency** — Each hook invocation spawns a new process (~10-50ms vs sub-ms in-process). Acceptable for file I/O operations.
- **No direct LLM access from hooks** — SessionEnd extraction must call an external API (e.g., Anthropic API via `curl` or SDK). Cannot use a subagent.
- **Anthropic-specific** — Impulse-on-Claude-Code only works for Claude Code users. OpenCode adapter extends reach.
- **JSON stdin/stdout contract** — Hook scripts must parse JSON from stdin and format JSON/text to stdout. More ceremony than in-process function calls.

---

## Alternatives Considered

### Alternative 1: OpenCode Primary (original ADR-001)

Rejected because:
- 2 of 4 required hooks do not exist (`session.start`, `session.end`)
- All 4 context interfaces are fabricated and must be rewritten
- The `session.end` gap requires a "extract on next start" workaround with 1-session delay and slower first prompts
- Estimated 3-5 days of rewrite work vs 2-3 days for Claude Code

### Alternative 2: Dual-Platform Simultaneously

Rejected for Phase 1 because:
- Doubles the integration surface area
- Claude Code covers the larger user base
- The core logic (file ops, extraction, formatting) is shared — only the hook adapters differ
- Phase 1.5 adds OpenCode adapter once Claude Code hooks are proven

### Alternative 3: MCP Server Instead of Hooks

Claude Code supports MCP servers that expose tools to the agent. Impulse could provide `impulse_read_genome`, `impulse_update_genome`, etc.

Deferred (not rejected) because:
- MCP provides on-demand tools, not lifecycle events
- No MCP equivalent of "run at session start" or "run at session end"
- MCP tools complement hooks — Phase 2 could add MCP for agent-initiated memory queries
- Hooks handle the automated lifecycle; MCP handles interactive queries

### Alternative 4: OneContext as Foundation

Rejected because:
- Dual Node+Python dependency violates "Bun-only" constraint
- v0.x status (2 weeks old at time of evaluation)
- Philosophical mismatch: OneContext replays trajectories, Impulse extracts knowledge
- No hook system — OneContext IS the lifecycle layer, not a plugin for existing agents

---

## References

- AGENT-HARNESS-ANALYSIS.md §1.3: SDK Gap Analysis (6 fabricated interfaces)
- AGENT-HARNESS-ANALYSIS.md §2.1-2.2: Claude Code hook types and SessionStart/SessionEnd capabilities
- AGENT-HARNESS-ANALYSIS.md §4: Comparison matrix (OpenCode vs Claude Code vs OneContext)
- AGENT-HARNESS-ANALYSIS.md §5: Recommended Platform Strategy
