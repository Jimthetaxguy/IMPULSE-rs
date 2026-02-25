---
title: Phase 2 Migration Plan
description: Migration plan from file-based to search-based memory
version: '1.0'
updated: 2026-02-20
type: specification
category: phases
phase: phase2
status: active
audience: builders
tags: [phase, migration, upgrade]
---

# Phase 2 Migration Plan: From Files to Search

> **Version:** 1.0 | **Status:** Planning | **Updated:** 2026-02-20
> **Trigger:** HISTORY_INDEX.md exceeds 100 sessions (~25KB) OR grep becomes too slow

---

## Why Phase 2

Phase 1 ("Three Files and a Hook") works perfectly for:

- Single developer, single project
- <100 sessions of history
- Keyword-searchable decisions in GENOME.md

Phase 2 is needed when:

- **HISTORY_INDEX.md > 100 sessions** — grep becomes impractical
- **GENOME.md > 200 lines** — context injection exceeds token budget
- **Semantic queries** — "What was our approach to auth?" can't be answered by grep
- **Multi-project** — developer works across 3+ projects, wants cross-project recall

---

## Migration 1: FTS5 for HISTORY_INDEX.md

### What Changes

| Before (Phase 1)                 | After (Phase 2)                     |
| -------------------------------- | ----------------------------------- |
| Read full HISTORY_INDEX.md       | SQLite FTS5 search                  |
| Load last 3 sessions (hardcoded) | Query by keyword, date range, files |
| grep for specific text           | Full-text search with ranking       |

### Implementation

```typescript
// New file: impulse-plugin/src/search/history-db.ts

import Database from 'better-sqlite3';

interface HistoryDB {
  index(entry: HistoryEntry): void;
  search(query: string, limit?: number): HistoryEntry[];
  recentSessions(count: number): HistoryEntry[];
}

function createHistoryDB(dbPath: string): HistoryDB {
  const db = new Database(dbPath);

  // Create FTS5 virtual table
  db.prepare(
    `
    CREATE VIRTUAL TABLE IF NOT EXISTS sessions USING fts5(
      session_id,
      date,
      agents,
      files,
      summary,
      content='',
      tokenize='porter unicode61'
    )
  `
  ).run();

  return {
    index(entry) {
      db.prepare(
        `
        INSERT INTO sessions(session_id, date, agents, files, summary)
        VALUES (?, ?, ?, ?, ?)
      `
      ).run(
        entry.sessionId,
        entry.date,
        entry.agents.join(','),
        entry.files.join(','),
        entry.summary
      );
    },

    search(query, limit = 10) {
      return db
        .prepare(
          `
        SELECT *, rank FROM sessions
        WHERE sessions MATCH ?
        ORDER BY rank
        LIMIT ?
      `
        )
        .all(query, limit) as HistoryEntry[];
    },

    recentSessions(count) {
      return db
        .prepare(
          `
        SELECT * FROM sessions ORDER BY date DESC LIMIT ?
      `
        )
        .all(count) as HistoryEntry[];
    },
  };
}
```

### Migration Steps

1. Add `better-sqlite3` to impulse-plugin dependencies
2. Create `history.db` alongside HISTORY_INDEX.md
3. On session start, check if FTS index exists; if not, backfill from HISTORY_INDEX.md
4. session-end hook writes to BOTH HISTORY_INDEX.md (compatibility) and FTS index
5. session-start hook queries FTS instead of reading full file

### Backwards Compatibility

HISTORY_INDEX.md continues to be written and committed. The FTS index is a gitignored cache that can be rebuilt from HISTORY_INDEX.md at any time.

---

## Migration 2: sqlite-vec for Semantic Search

### What Changes

| Before (Phase 1)       | After (Phase 2)                                      |
| ---------------------- | ---------------------------------------------------- |
| No semantic search     | Embed conversation turns, cosine similarity          |
| Keyword grep only      | "What was our auth approach?" returns relevant turns |
| GENOME.md is flat text | Decisions have vector representations                |

### Prerequisites

- `sqlite-vec` extension (C, ~3MB)
- `sentence-transformers` or equivalent (Python or ONNX runtime)
- Embedding model: `all-MiniLM-L6-v2` (384 dims, 22MB)

### Schema

```sql
-- Conversation turn embeddings
CREATE VIRTUAL TABLE IF NOT EXISTS turn_vectors USING vec0(
  embedding float[384],
  +session_id TEXT,
  +turn_index INTEGER,
  +agent_id TEXT,
  +content TEXT,
  +timestamp TEXT
);

-- Decision embeddings
CREATE VIRTUAL TABLE IF NOT EXISTS decision_vectors USING vec0(
  embedding float[384],
  +decision_text TEXT,
  +date TEXT,
  +source_session TEXT
);
```

### Query Pattern

```sql
-- Semantic search: "auth approach"
SELECT content, distance
FROM turn_vectors
WHERE embedding MATCH ?  -- query vector
  AND k = 5              -- top 5 results
ORDER BY distance;
```

### Cost Model

| Operation              | Time          | When                      |
| ---------------------- | ------------- | ------------------------- |
| Embed one turn         | ~50ms (local) | Session end               |
| Index 100 turns        | ~5s           | Session end batch         |
| KNN query (k=5)        | <100ms        | Session start / on-demand |
| Model load (first use) | ~2s           | Cold start only           |

---

## Migration 3: MCP Server for Agent Access

### What Changes

| Before (Phase 1)           | After (Phase 2)                            |
| -------------------------- | ------------------------------------------ |
| Agents read files directly | Agents call MCP tools                      |
| No search capability       | `search_history`, `search_decisions` tools |
| File-based only            | Structured query API                       |

### MCP Tool Definitions

```typescript
const tools = [
  {
    name: 'search_history',
    description: 'Search past coding sessions by keyword or semantic similarity',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'Search query' },
        mode: { type: 'string', enum: ['keyword', 'semantic'], default: 'keyword' },
        limit: { type: 'integer', default: 5 },
        days_back: { type: 'integer', default: 30 },
      },
      required: ['query'],
    },
  },
  {
    name: 'search_decisions',
    description: 'Search project decisions from GENOME.md',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string' },
        category: { type: 'string', enum: ['architectural', 'preferences', 'constraints'] },
      },
      required: ['query'],
    },
  },
  {
    name: 'get_session_context',
    description: 'Get full context for a specific past session',
    inputSchema: {
      type: 'object',
      properties: {
        session_id: { type: 'string' },
      },
      required: ['session_id'],
    },
  },
];
```

---

## Phase 2 Dependencies (New)

| Package                         | Why                          | Size |
| ------------------------------- | ---------------------------- | ---- |
| `better-sqlite3`                | FTS5 + base for sqlite-vec   | ~7MB |
| `sqlite-vec`                    | Vector similarity            | ~3MB |
| `@anthropic-ai/sdk` or `openai` | Embedding API (if not local) | ~1MB |

**Total addition:** ~11MB (Phase 1 is ~5MB, Phase 2 total ~16MB)

---

## Phase 2 Does NOT Change

- GENOME.md, LIVE_STATE.json, HISTORY_INDEX.md — still the source of truth
- 4 hooks — same hooks, enhanced logic
- Bun runtime — no Python requirement (use ONNX for local embeddings instead)
- Graceful degradation — if FTS/vector fails, fall back to file reading

---

## Decision Points Before Starting Phase 2

| Question                       | Options                                    | Recommendation           |
| ------------------------------ | ------------------------------------------ | ------------------------ |
| Local vs API embeddings?       | ONNX runtime vs OpenAI API                 | Local (privacy, no cost) |
| FTS5 first or vectors first?   | FTS5 handles 80% of queries                | FTS5 first               |
| Separate search MCP or inline? | MCP is reusable by other agents            | MCP server               |
| Keep HISTORY_INDEX.md?         | Yes, for backwards compat + git visibility | Keep both                |

---

_Created: 2026-02-20 | Status: Planning v1.0_
