---
title: CLI Quick Reference — Hooks, Search, and Conflict Commands
description: Command-line interface reference for Phase 1-3 enhancements
version: '1.0'
updated: 2026-03-02
type: doc
category: reference
status: active
audience: builder
tags: [cli, reference, commands]
---

# CLI Quick Reference — Enhanced Commands

> **Generated:** 2026-03-02 | **Applies to:** impulse-rs v0.1.1+

---

## New Commands

### Guardrail Commands

```bash
# Evaluate action against rules (existing)
impulse-rs guard --action "git push --force" --target bash

# Evaluate with blocking (NEW)
impulse-rs guard --action "rm -rf" --target bash --block

# Check conflicts before write (NEW)
impulse-rs guard --action "Write src/main.rs" --target file --check-conflicts

# Manage guard config (NEW)
impulse-rs guard config list
impulse-rs guard config edit
impulse-rs guard config validate
```

### Retrieval Commands (Enhanced)

```bash
# Search with fallback explanation (ENHANCED)
impulse-rs search-history --query "auth" --explain
impulse-rs search-genome --query "database" --explain

# Search with specific backend (ENHANCED)
impulse-rs search-history --query "login" --backend rust-cosine

# Clear embedding cache (NEW)
impulse-rs retrieval clear-cache

# Retrieval status with cache stats (ENHANCED)
impulse-rs retrieval-status --check --json
```

### Conflict Commands (NEW)

```bash
# Check for active conflicts
impulse-rs conflict status

# List active sessions and their files
impulse-rs conflict sessions

# Suggest handoff between sessions
impulse-rs conflict handoff --session-a <id> --session-b <id>

# Auto-handoff on conflict (NEW flag)
impulse-rs orchestrate --task "add feature" --auto-handoff
```

---

## Configuration

### Guardrail Config (`.impulse/guardrail.json`)

```json
{
  "version": 1,
  "enabled": true,
  "conflict_check": true,
  "conflict_blocking_enabled": false,
  "conflict_warn_enabled": true,
  "rules": [
    {
      "id": "block-force-push",
      "pattern": "git.*push.*--force",
      "action": "block",
      "target": "bash",
      "reason": "Force push is dangerous"
    }
  ]
}
```

### Retrieval Config (`.impulse/config.json`)

```json
{
  "embedding_cache_enabled": true,
  "embedding_cache_ttl_secs": 300,
  "embedding_cache_max_entries": 1000,
  "retrieval_fallback_chain": ["sqlite-vec", "rust-cosine", "keyword"]
}
```

---

## Hook Configuration

### Claude Code Hooks (Generated)

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "impulse-rs guard --action \"$INPUT\" --target file --check-conflicts --block"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "impulse-rs track-write --file \"$INPUT\" --session-id $IMPULSE_SESSION_ID"
          }
        ]
      }
    ]
  }
}
```

---

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `IMPULSE_GUARD_BLOCK` | Enable blocking globally | false |
| `IMPULSE_CACHE_TTL` | Cache TTL in seconds | 300 |
| `IMPULSE_CONFLICT_WARN` | Enable conflict warnings | true |

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Blocked by guardrail |
| 2 | Configuration error |
| 3 | Storage error |

---

## Related Documents

- [ROADMAP-PLAN.md](./ROADMAP-PLAN.md)
- [IMPLEMENTATION-HANDOFF.md](./plans/IMPLEMENTATION-HANDOFF.md)
- [TESTING-STRATEGY-ENHANCEMENTS.md](./guides/TESTING-STRATEGY-ENHANCEMENTS.md)

---

*Generated: 2026-03-02*
