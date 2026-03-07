---
title: ADR 0006 - Hook Enhancement and Conflict Resolution
description: Design decisions for blocking hooks, embedding cache, and conflict resolution
status: accepted
date: 2026-03-02
authors:
  - name: James Pustorino
---

# ADR 0006: Hook Enhancement and Conflict Resolution

## Status

Accepted — 2026-03-02

## Context

This ADR documents the design decisions for three interconnected enhancements:
1. Adding structural blocking capability to Claude Code/OpenCode hooks
2. Making semantic search robust with caching and better fallbacks
3. Implementing conflict resolution (not just advisory detection)

## Decision

### D1: Blocking Is Opt-In

**Decision:** `conflict_blocking_enabled` defaults to `false`

**Alternatives considered:**
- Default ON — Rejected: Too breaking for existing users
- Configurable per-rule — Rejected: Adds complexity

**Rationale:**
- HONEST-ROADMAP identified blocking as a breaking change
- Users should explicitly opt-in to structural blocking
- Advisory mode (current) remains the default

### D2: Embedding Cache Is In-Memory Only

**Decision:** No persistence for embedding cache

**Alternatives considered:**
- Persist to disk — Rejected: Adds serialization complexity, versioning issues
- Redis/memcached — Rejected: Adds infrastructure dependency

**Rationale:**
- Embeddings are cheap to regenerate (sub-second)
- TTL provides automatic cleanup
- Simpler operational model

### D3: Conflict Detection Uses active_files

**Decision:** Check `session.active_files` in LIVE_STATE for conflicts

**Alternatives considered:**
- Track in-flight operations — Rejected: Requires coordination protocol
- Use files_touched — Rejected: Only available at session end

**Rationale:**
- Already populated by PostToolUse hooks
- Real-time accurate
- Simpler than tracking in-flight operations

### D4: Guardrail Config File

**Decision:** Add `.impulse/guardrail.json` for user configuration

**Alternatives considered:**
- Environment variables only — Rejected: Hard to manage complex rules
- Claude Code settings.json — Rejected: Bleeds into agent config

**Rationale:**
- Familiar pattern (like .impulse/config.json)
- Versionable, git-trackable
- Clear separation from agent config

### D5: Batch Embedding

**Decision:** Process multiple texts in single subprocess call

**Alternatives considered:**
- Individual calls — Rejected: Subprocess overhead per call
- Streaming — Rejected: Adds complexity for marginal benefit

**Rationale:**
- Reduces subprocess spawn overhead
- Simpler than streaming
- Works with existing embedding script

## Consequences

### Positive

- Blocking provides real protection, not just warnings
- Cache improves semantic search performance
- Conflict detection becomes actionable

### Negative

- More complex guardrail module
- Cache memory usage (mitigated by TTL + LRU)
- Potential for blocking to break workflows (mitigated by default OFF)

### Neutral

- Config file adds another file to manage
- Batch embedding changes indexer flow

## Implementation Notes

### Blocking Behavior

```rust
// Guard command exits with code 1 when blocking
if block_flag && results.iter().any(|r| r.should_block()) {
    eprintln!("Blocked by guardrail rule: {}", reason);
    std::process::exit(1);
}
```

### Cache Behavior

```rust
// LRU with TTL expiration
impl EmbeddingCache {
    fn get(&self, key: &str) -> Option<Vec<f32>> {
        match self.entries.get(key) {
            Some(entry) if entry.is_fresh() => Some(entry.vector.clone()),
            _ => None,
        }
    }
}
```

### Conflict Detection

```rust
// Check against active_files in LIVE_STATE
fn check_file_conflicts(file: &str, state: &State) -> Option<Conflict> {
    for session in state.active_sessions() {
        if session.active_files.contains(file) {
            return Some(Conflict { file, session_id: session.id });
        }
    }
    None
}
```

## Related ADRs

- ADR 0002: File-first memory — Uses `.impulse/` directory
- ADR 0003: Progressive search — Fallback chain
- ADR 0004: Extraction strategy — SessionEnd extraction
- ADR 0005: Distribution model — Binary distribution

---

*Created: 2026-03-02*
