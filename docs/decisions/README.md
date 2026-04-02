---
status: active
phase: all
audience: builder
tags: [decisions, adr, architecture]
last_updated: 2026-02-20
---

# Architecture Decision Records

> Active decisions governing Impulse's architecture. Older superseded ADR references are mentioned for continuity, but the corresponding archive files are not fully present in the current workspace.

## Decision Log

| # | Title | Status | Date | Supersedes |
|---|-------|--------|------|------------|
| [0001](0001-claude-code-primary.md) | Claude Code as primary agent | **Accepted** | 2026-02-20 | historical `0001-opencode-first` |
| [0002](0002-file-first-memory.md) | File-first memory (no DB in Phase 1) | **Accepted** | 2026-02-20 | historical `0002-unified-steward` |
| [0003](0003-progressive-search.md) | Progressive search (FTS5 then vectors) | **Accepted** | 2026-02-20 | historical `0003-split-schema` |
| [0004](0004-extraction-strategy.md) | LLM extraction strategy | **Accepted** | 2026-02-20 | -- |
| [0005](0005-distribution-model.md) | npm distribution model | **Accepted** | 2026-02-20 | -- |

## ADR Format

Each ADR follows the template:

```markdown
# ADR-NNNN: Title

## Status
Accepted | Superseded | Proposed

## Context
What problem does this solve?

## Decision
What did we decide?

## Consequences
What follows from this decision?
```

## Adding New ADRs

1. Use the next sequential number
2. Create in `docs/decisions/`
3. Update this table
4. If superseding an existing ADR, preserve the old file only if the archive path actually exists in the workspace; otherwise record the historical filename in plain text
