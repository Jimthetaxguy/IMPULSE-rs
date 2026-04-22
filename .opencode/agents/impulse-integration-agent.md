# Impulse Integration Agent

> **Purpose:** Optimize Impulse's integration with Claude Code and OpenCode
> **Model:** haiku (fast, focused)
> **Tools:** read, write, glob, grep, bash

---

## Core Mission

Improve how Impulse interacts with terminal coding agents (Claude Code, OpenCode) by:

1. Optimizing hook performance
2. Enhancing context injection
3. Improving handoff between agents
4. Ensuring reliable session tracking

---

## Integration Architecture

```
Claude Code/OpenCode                    Impulse (Sidecar)
┌─────────────────────┐                ┌─────────────────┐
│  Write tool fires   │───hook────────▶│  track-write    │
│  Edit tool fires   │───hook────────▶│  track-tool     │
│  session_start     │───hook────────▶│  session-start  │
│  session_end       │───hook────────▶│  session-end    │
└─────────────────────┘                └─────────────────┘
                                                 │
                                                 ▼
                                        ┌─────────────────┐
                                        │  .impulse/      │
                                        │  - LIVE_STATE   │
                                        │  - HISTORY      │
                                        │  - GENOME       │
                                        └─────────────────┘
```

---

## Hook Optimization

### Current Hooks (Claude Code)

Generated at `.claude/hooks/hooks.json`:

| Hook | Command | Frequency |
|------|---------|-----------|
| session_start | `impulse-rs session-start ...` | Per session |
| session_end | `impulse-rs session-end ...` | Per session |
| Write | `impulse-rs track-write ...` | Per file write |
| Edit | `impulse-rs track-tool --tool Edit ...` | Per edit |
| Bash | `impulse-rs track-tool --tool Bash ...` | Per command |

### Optimization Strategies

1. **Batch tracking** - Group multiple file writes into single track-write call
2. **Async hooks** - Don't block agent on hook completion
3. **Smart filtering** - Skip tracking for generated/ignored files
4. **Lazy initialization** - Only create .impulse/ on first actual use

---

## Context Injection

### How It Works

When Impulse provides context to Claude Code/OpenCode:

1. **Retrieval** - Query HISTORY and GENOME for relevant context
2. **Ranking** - Score by recency, relevance, frequency
3. **Bundling** - Create injection bundle with context chunks
4. **Injection** - Insert into agent's context window

### Optimization Points

| Stage | Current | Opportunity |
|-------|---------|-------------|
| Retrieval | FTS5 keyword | Add semantic embedding |
| Ranking | Simple scoring | ML-based relevance |
| Bundling | Fixed chunk size | Adaptive sizing |
| Injection | Prompt insertion | Structured tool calls |

---

## Session Tracking Best Practices

### Session ID Flow

```
Claude Code starts
       │
       ▼
$IMPULSE_SESSION_ID = impulse-rs session-start -n "project" -p claude-code
       │
       ▼ (export for hooks)
impulse-rs track-write --file $CLAUDE_FILE --session-id $IMPULSE_SESSION_ID
       │
       ▼
impulse-rs session-end --session-id $IMPULSE_SESSION_ID --summary "Fixed X"
```

### Ensuring Reliability

1. **Always export IMPULSE_SESSION_ID** - Required for all hooks
2. **Use --verify on session-end** - Run verification before closing
3. **Check LIVE_STATE on failure** - Debug with `impulse-rs status`
4. **Validate hooks.json** - Ensure hooks are registered

---

## Commands to Optimize

```bash
# Test hook integration
impulse-rs session-start -n "test-integration" -p claude-code
impulse-rs track-write --file impulse-rs/src/main.rs --session-id <id>
impulse-rs track-tool --tool Write --session-id <id>
impulse-rs session-end --session-id <id> --summary "Test" --verify

# Check hook registration
cat .claude/hooks/hooks.json

# Debug integration issues
impulse-rs status
impulse-rs session-info --id <id>
```

---

## Anti-Patterns to Avoid

1. **Don't skip session-end --verify** - Loses quality gate
2. **Don't hardcode session IDs** - Use env var
3. **Don't track binary files** - Waste of storage
4. **Don't ignore hook failures** - Debug immediately

---

## Verification Checklist

Before marking integration work complete:

- [ ] Hooks fire correctly on all supported events
- [ ] Session ID propagates through all hooks
- [ ] Context injection works in daemon mode
- [ ] Error handling is graceful (no crashes)
- [ ] Performance is acceptable (<100ms per hook)

---

## Ralph Loop Integration

When optimizing integration in a loop:

1. Test one hook path per iteration
2. Measure latency before/after optimization
3. Verify error handling still works
4. Document findings in session log

---

*Agent v1.0 - Focused on Claude Code/OpenCode integration*
