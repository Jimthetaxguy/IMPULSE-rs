---
status: accepted
phase: 2
audience: builder
tags: [decision, search, fts5, vectors]
last_updated: 2026-02-20
---

# ADR-003: Progressive Search Complexity

> **Status:** Accepted
> **Date:** 2026-02-20
> **Supersedes:** historical ADR `0003-split-schema.md` (not present in the current workspace archive); content was reused in Phase 3

---

## Context

Impulse accumulates knowledge in two searchable stores: GENOME.md (decisions, preferences, constraints) and HISTORY_INDEX.md (session summaries). As these grow, the question becomes: when does simple file reading become insufficient, and what search infrastructure should replace it?

### The claude-historian-mcp Correction (SEARCH-LAYER-ANALYSIS.md §1)

Previous documentation described claude-historian-mcp as using "TF-IDF scoring with Naive Bayes query classification." This is incorrect. The actual source code reveals:

| Prior Claim | Reality |
|-------------|---------|
| TF-IDF scoring | Custom multi-stage weighted keyword scoring |
| Naive Bayes query classification | Simple keyword-to-category mapper (4 categories) |
| Edit-distance fuzzy matching | Positional character comparison with 60% threshold |
| Exponential time decay | Discrete 3-tier recency boosting (24h/7d/30d) |

Despite this simpler architecture, claude-historian-mcp achieves **4.7/5 quality score** across 27 benchmark queries. The lesson: handcrafted keyword scoring is sufficient for code conversations, which have naturally high keyword density.

### The "Pain to Rediscover" Weighting (SEARCH-LAYER-ANALYSIS.md §1)

claude-historian-mcp's most interesting scoring innovation is importance-based weighting:

| Content Type | Weight | Rationale |
|-------------|--------|-----------|
| Decisions ("decided to", "trade-off", "instead of") | 2.5x | Hardest to rediscover from code alone |
| Bugfixes ("fixed", "gotcha", "workaround") | 2.0x | Painful to re-encounter |
| Features ("implemented", "shipped", "built") | 1.5x | Moderate rediscovery cost |
| Discoveries ("learned", "insight", "found out") | 1.3x | Low rediscovery cost |

This directly informs Impulse's extraction prompt — decisions should be prioritized over routine changes.

### sqlite-vec Performance (SEARCH-LAYER-ANALYSIS.md §3)

At Impulse's projected scale (hundreds to low thousands of vectors):

| Scale | Write Latency | Read Latency (KNN) |
|-------|--------------|---------------------|
| 1,000 vectors | ~100ms | < 10ms |
| 10,000 vectors | ~788ms | < 100ms |

sqlite-vec provides 60x faster writes than Faiss, exact (100% recall) KNN search, and single-file storage. At 1 session/day with 10 chunks/session, it takes ~3 years to reach 10K vectors.

---

## Decision

**Search infrastructure is added progressively based on observed need, not upfront.**

### Phase 1: Zero Search Infrastructure

Agents read GENOME.md and HISTORY_INDEX.md directly as plain text. The SessionStart hook injects:
- Full GENOME.md content (< 200 lines)
- Last 3 HISTORY_INDEX.md entries (most recent context)

For ad-hoc queries, agents can `grep` the files.

**Why this is sufficient:**
- GENOME.md stays small (< 100 lines) in early usage
- HISTORY_INDEX.md has < 10 entries in the first weeks
- Code conversations have high keyword density — `grep` works well
- Zero infrastructure, zero latency, zero failure modes

### Phase 2: FTS5 Full-Text Search

**Trigger:** HISTORY_INDEX.md > 100 sessions OR `grep` response time > 500ms

Add SQLite FTS5 index over HISTORY_INDEX.md content:

```sql
CREATE VIRTUAL TABLE history_fts USING fts5(
  session_date,
  summary,
  decisions,
  files_modified,
  content='history_entries'
);
```

FTS5 handles 80% of coding queries at 1% of vector search complexity. Searches like "auth implementation" or "database migration" return precise results via tokenized keyword matching with BM25 ranking.

**What FTS5 catches:** Exact and near-exact keyword matches. "JWT auth", "PostgreSQL migration", "Zod validation" — all directly matchable.

**What FTS5 misses:** Semantic synonyms. "auth" will not find entries about "security middleware" or "login system." This gap is acceptable for most coding queries.

### Phase 3: sqlite-vec Semantic Search

**Trigger:** GENOME.md > 500 lines OR keyword search demonstrably fails on semantic queries

Add sqlite-vec alongside FTS5 for hybrid search:

```sql
-- Vector table (reuses ADR-0003-split-schema.md design)
CREATE VIRTUAL TABLE session_vectors USING vec0(
  project_id TEXT PARTITION KEY,
  embedding float[384]
);

-- Metadata table (regular SQLite)
CREATE TABLE session_metadata (
  rowid INTEGER PRIMARY KEY,
  session_date TEXT NOT NULL,
  summary TEXT NOT NULL,
  source TEXT DEFAULT 'history',
  confidence REAL DEFAULT 0.8
);
```

Hybrid search combines FTS5 (keyword) and sqlite-vec (semantic) results via score merging. The split schema from the original ADR-0003 applies here — vec0 virtual tables don't support UPSERT, so updates require DELETE + INSERT on the vector table paired with UPSERT on the metadata table.

**Embedding model:** all-MiniLM-L6-v2 (22MB, 384 dimensions, < 5ms/text on Apple Silicon). Upgrade to nomic-embed-text (768 dimensions, 8192 token context) only if retrieval quality is insufficient.

---

## Search Decision Matrix

| Query Type | Phase 1 | Phase 2 | Phase 3 |
|-----------|---------|---------|---------|
| "What was the last thing I worked on?" | Read HISTORY tail | Same | Same |
| "How did we implement auth?" | `grep HISTORY` | FTS5 search | FTS5 + vec hybrid |
| "What's the preferred testing approach?" | Read GENOME.md | Same | mem0 graph query |
| "Find all sessions about database migrations" | Manual grep | FTS5 search | FTS5 + metadata filter |
| "auth" (wants "security middleware" results too) | Miss | Miss | sqlite-vec catches it |

---

## Consequences

### Positive

- **Zero search infrastructure in Phase 1** — No databases, no embedding models, no indexing pipelines. One less thing to install, configure, and debug.
- **FTS5 is built into SQLite** — No external dependencies. Ships with every SQLite installation. Adds maybe 100KB to the binary.
- **Progressive complexity** — Each phase adds value independently. Phase 2 doesn't require Phase 3. Phase 3 doesn't obsolete Phase 2. Hybrid search combines both.
- **Measurable triggers** — Upgrade decisions are based on observable metrics (line count, grep latency, search quality), not guesswork.

### Negative

- **No semantic search until Phase 3** — "auth" will not find "security middleware" in Phase 1 or 2. Developers must use precise keywords.
- **FTS5 requires a sync step** — HISTORY_INDEX.md is the source of truth; FTS5 index must be rebuilt when the file changes. Adds complexity to the SessionEnd pipeline.
- **Embedding model adds a Python dependency** — sentence-transformers runs in Python. Phase 3 introduces a Python subprocess for embedding generation, breaking the "Bun-only" constraint.

---

## Alternatives Considered

### Alternative 1: sqlite-vec from Day 1 (original ADR-003)

Rejected for Phase 1 because:
- Requires an embedding model (Python dependency)
- Adds 200ms+ latency to SessionEnd (embedding generation)
- At < 100 sessions, vector search returns the same results as grep
- The split schema complexity (DELETE + INSERT for updates) is unnecessary when there's nothing to search

The original ADR-003's split schema design is preserved for Phase 3 use.

### Alternative 2: claude-historian-mcp as Search Backend

Deferred (complementary, not competitive) because:
- claude-historian-mcp searches raw JSONL — it's a read-only search tool
- Impulse needs write + read (extract AND inject) — different lifecycle
- They can coexist: claude-historian-mcp searches raw transcripts, Impulse searches extracted knowledge
- Installing claude-historian-mcp as an MCP server (`claude mcp add claude-historian -- npx claude-historian-mcp`) provides immediate value with zero conflict

### Alternative 3: External Search Service (Elasticsearch, Meilisearch)

Rejected because:
- Requires running a separate service (daemon, port, configuration)
- Violates "zero infrastructure" constraint
- Overkill for < 10,000 documents
- FTS5 + sqlite-vec provide equivalent quality at Impulse's scale

---

## References

- SEARCH-LAYER-ANALYSIS.md §1: claude-historian-mcp scoring pipeline correction (not TF-IDF)
- SEARCH-LAYER-ANALYSIS.md §1: "pain to rediscover" importance weighting (decisions 2.5x)
- SEARCH-LAYER-ANALYSIS.md §3: sqlite-vec performance (60x faster writes than Faiss)
- SEARCH-LAYER-ANALYSIS.md §5: Phase recommendations (zero search in Phase 1, FTS5 in Phase 2)
- 0003-split-schema.md: Split schema design preserved for Phase 3 vec0 + metadata tables
