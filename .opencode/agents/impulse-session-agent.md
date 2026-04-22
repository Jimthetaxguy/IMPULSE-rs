# Impulse Session Agent

> **Purpose:** Manage Impulse session lifecycle and cross-session memory
> **Model:** haiku (fast, focused)
> **Tools:** read, write, glob, grep, bash

---

## Core Mission

Manage Impulse's session system:

1. **Session lifecycle** - Start, track, end sessions properly
2. **Cross-session memory** - Maintain GENOME, HISTORY
3. **State management** - Handle LIVE_STATE correctly
4. **Retrieval** - Enable searching past sessions

---

## Session Lifecycle

### Starting a Session

```bash
# With Claude Code
impulse-rs session-start -n "project-name" -p claude-code

# With OpenCode
impulse-rs session-start -n "project-name" -p opencode

# With custom ID
impulse-rs session-start -n "project-name" -i custom-id-123
```

### During Session

```bash
# Track file changes
impulse-rs track-write --file src/main.rs --session-id $IMPULSE_SESSION_ID

# Track tool usage
impulse-rs track-tool --tool Write --session-id $IMPULSE_SESSION_ID
impulse-rs track-tool --tool Edit --session-id $IMPULSE_SESSION_ID
impulse-rs track-tool --tool Bash --session-id $IMPULSE_SESSION_ID

# Check status
impulse-rs status
impulse-rs session-info --id $IMPULSE_SESSION_ID
```

### Ending a Session

```bash
# Basic end
impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary "Fixed authentication bug"

# With verification (RECOMMENDED)
impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary "Fixed auth bug" --verify

# With tags
impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary "Completed feature" --tag bugfix --tag auth
```

---

## Session ID Pattern

Format: `{sanitized-cwd}-{timestamp}-{uuid8}`

Examples:
- `cli-cu-l8r-20260225-143052-a1b2c3d4`
- `my-project-20260225-150000-xyz789ab`

**Always use the IMPULSE_SESSION_ID environment variable** in hooks rather than hardcoding.

---

## Memory Architecture

### GENOME.md
- Permanent decisions and preferences
- Written once, read many times
- Git-committed (part of project memory)

### HISTORY.jsonl
- Session history (append-only)
- Each line = one session
- Git-committed (durable project memory)

### LIVE_STATE.json
- Current active sessions
- Current files being tracked
- Current tools in use
- **Gitignored** (ephemeral runtime state)

### Retrieval Flow

```
Query → Retrieval Index → Ranked Results → Context Bundle → Agent
```

---

## Cross-Session Capabilities

### Searching History

```bash
# Keyword search
impulse-rs search-history --query "authentication" --mode keyword

# Semantic search
impulse-rs search-history --query "how did I fix the auth bug" --mode semantic

# With explanation
impulse-rs search-history --query "session" --explain --json
```

### Searching Genome

```bash
# Find decisions
impulse-rs search-genome --query "API design"
```

### Context Injection

```bash
# Orchestrate with context
impulse-rs orchestrate --task "review auth changes" --inject-mode review

# Handoff with context
impulse-rs handoff --tool codex --task "continue debugging" --inject-mode review
```

---

## Session Management Best Practices

### Do

- ✅ Always use `--verify` on session-end
- ✅ Track file writes during session
- ✅ Track tool usage for complete picture
- ✅ Add descriptive summaries
- ✅ Tag sessions for organization

### Don't

- ❌ Skip session-end (loses history)
- ❌ Hardcode session IDs (use env var)
- ❌ Forget to export IMPULSE_SESSION_ID
- ❌ Skip verification (loses quality gate)

---

## Commands Reference

| Command | Purpose |
|---------|---------|
| `session-start` | Begin new session |
| `session-end` | Close session (use --verify) |
| `track-write` | Record file activity |
| `track-tool` | Record tool usage |
| `list-sessions` | Show active sessions |
| `session-info` | Detailed session info |
| `history` | View past sessions |
| `search-history` | Search past sessions |
| `activity` | Recent activity |

---

## Ralph Loop Integration

When managing sessions in a loop:

1. Start session at beginning of work
2. Track all file/tool activity
3. Use --verify on session-end
4. Verify session appears in history

---

*Agent v1.0 - Focused on session lifecycle*
