---
status: active
phase: 2
audience: builder
tags: [research, search, fts5]
last_updated: 2026-02-20
---

# Search Layer Analysis: History, Retrieval, and Semantic Search for Impulse

> **Version:** 1.0 | **Status:** Research Complete | **Updated:** 2026-02-20
> **Purpose:** Deep-dive analysis of search and retrieval tools that will power Impulse's history search layer. Based on actual source code analysis of cloned repos, not documentation claims.
> **Informs:** Whether Impulse uses FTS5, TF-IDF, vectors, or a hybrid approach in each phase.

---

## Table of Contents

1. [claude-historian-mcp: Code Study](#1-claude-historian-mcp-code-study)
   - [Scoring Pipeline (What It Actually Does)](#scoring-pipeline-what-it-actually-does)
   - [Query Classification](#query-classification)
   - [Word Similarity (Not Edit-Distance)](#word-similarity-not-edit-distance)
   - [Recency Boosting (Not Exponential Decay)](#recency-boosting-not-exponential-decay)
   - [LRU Cache Strategy](#lru-cache-strategy)
   - [JSONL Noise Filtering Pipeline](#jsonl-noise-filtering-pipeline)
   - [Scoring Constants Reference](#scoring-constants-reference)
   - [Performance Benchmarks](#performance-benchmarks)
   - [MCP Tool Surface](#mcp-tool-surface)
2. [claude-history: The Simple Alternative](#2-claude-history-the-simple-alternative)
3. [sqlite-vec: Hands-On Analysis](#3-sqlite-vec-hands-on-analysis)
   - [Virtual Table Architecture](#virtual-table-architecture)
   - [The DELETE+INSERT Pattern](#the-deleteinsert-pattern)
   - [Partition Key Support](#partition-key-support)
   - [Bun Integration Pattern](#bun-integration-pattern)
   - [Performance at Scale](#performance-at-scale)
4. [sentence-transformers: Embedding Analysis](#4-sentence-transformers-embedding-analysis)
   - [Model Comparison](#model-comparison)
   - [M-Series Mac Performance](#m-series-mac-performance)
   - [Memory Footprint](#memory-footprint)
   - [Code-Specific Quality](#code-specific-quality)
5. [Impulse Phase Recommendations](#5-impulse-phase-recommendations)
6. [Corrections to Prior Documentation](#6-corrections-to-prior-documentation)

---

## 1. claude-historian-mcp: Code Study

**Source:** `cloned-repos/claude-historian-mcp/src/` (7 TypeScript files, ~5,500 lines total)

### CRITICAL CORRECTION

Previous documentation (TOOL-STACK-ANALYSIS.md, RESEARCH-DIGEST.md) described claude-historian-mcp as using "TF-IDF scoring with Naive Bayes query classification" and "edit-distance fuzzy matching." **These claims are incorrect.** The actual source code reveals a fundamentally different architecture:

| Prior Claim | Actual Implementation |
|-------------|----------------------|
| TF-IDF scoring | Custom multi-stage keyword scoring with weighted term matching |
| Naive Bayes query classification | Simple keyword-based classification into 4 categories |
| Edit-distance fuzzy matching | Character-by-character positional comparison with 60% threshold |
| Exponential time decay | Discrete recency tier boosting (24h/7d/30d) |
| 6 MCP tools | 10 MCP tools |

### Scoring Pipeline (What It Actually Does)

The scoring pipeline is a multi-stage weighted keyword matcher, **not** TF-IDF. There are no document frequency calculations, no inverse document frequency weighting, and no term frequency normalization. Instead, it uses a handcrafted scoring system with two distinct layers.

#### Layer 1: Base Relevance Score (`utils.ts:calculateRelevanceScore()`)

This is called during JSONL parsing for every message. It decomposes into 5 sub-functions:

```
calculateRelevanceScore(message, query, projectPath)
  |-- scoreCoreTerms(message, query)        -> EXACT_MATCH_SCORE=10 per tech term
  |-- scoreSupportingTerms(message, query)   -> SUPPORTING_TERM_SCORE=3 per 5+ char non-generic term
  |-- scoreToolUsage(message)                -> TOOL_USAGE_SCORE=5 for tool_use/tool_result types
  |-- scoreFileReferences(message)           -> FILE_REFERENCE_SCORE=3 for src/, .ts, .js refs
  +-- scoreProjectMatch(message, path)       -> PROJECT_MATCH_SCORE=5 if CWD matches project
```

**Core term matching is strict and binary.** If the query contains a term matching `CORE_TECH_PATTERN` (e.g., "react", "typescript", "docker") and the content does NOT contain that term, the entire message is rejected with a score of -1000. This is a hard gate, not a soft penalty.

The `matchesTechTerm()` function in `utils.ts` (lines 243-265) implements case-aware word-boundary matching:
- Allows: `react`, `React`, `REACT` (lowercase, uppercase, title case)
- Rejects: `ReAct`, `rEact` (mixed internal capitalization treated as different term)

```typescript
// From utils.ts - the case validation logic
const isNormalCase =
  cleanWord === cleanWord.toLowerCase() ||           // "react"
  cleanWord === cleanWord.toUpperCase() ||           // "REACT"
  cleanWord === cleanWord.charAt(0).toUpperCase()
    + cleanWord.slice(1).toLowerCase();               // "React"
```

**Supporting terms** are query words that are 5+ characters long, do NOT match `CORE_TECH_PATTERN`, and are NOT in the `GENERIC_TERMS` set (~187 common terms). Each match adds `SUPPORTING_TERM_SCORE=3`.

**Exact phrase bonus:** If the entire query string appears verbatim in the content, add `EXACT_PHRASE_BONUS=5`.

**Majority match bonus:** If >= 60% of query words match, add `MAJORITY_MATCH_BONUS=4`.

#### Layer 2: Claude Relevance Score (`search-helpers.ts:calculateClaudeRelevance()`)

Applied after Layer 1 during result ranking. This is a multiplicative scoring layer:

```
calculateClaudeRelevance(message, query)
  |-- importanceScore = calculateImportanceScore(content)
  |     decisions -> 2.5x, bugfixes -> 2.0x, features -> 1.5x, discoveries -> 1.3x
  |-- technicalBoosts (multiplicative per keyword found)
  |     code -> 2.0x, error -> 1.8x, function -> 1.5x, class -> 1.5x,
  |     import -> 1.3x, export -> 1.3x, const -> 1.2x, let -> 1.2x, var -> 1.2x
  |-- queryTermMatches -> 1.1x per matching term
  |-- recencyBoost -> <24h: 1.5x, <7d: 1.2x, <30d: 1.1x
  |-- toolUsageBoost -> 1.3x
  |-- fileReferenceBoost -> 1.2x
  |-- errorPatternBoost -> 1.4x
  |-- solutionBoost -> 1.6x (assistant messages with "solution"/"fixed"/"resolved")
  +-- cap at 10.0
```

**The "pain to rediscover" concept** (`calculateImportanceScore()`, search-helpers.ts lines 80-148) is the most interesting scoring innovation. It boosts content based on how hard it would be to find again:

```typescript
// Importance tiers from search-helpers.ts
// Decisions: "decided to", "trade-off", "rationale", "instead of" -> 2.5x
// Bugfixes: "fixed", "gotcha", "workaround", "edge case" -> 2.0x
// Features: "implemented", "shipped", "built", "created" -> 1.5x
// Discoveries: "learned", "discovered", "insight", "found out" -> 1.3x
```

This is the algorithm's key insight: **architectural decisions are 2.5x more valuable than random code changes** because they are the hardest to rediscover from raw code alone.

#### Quality Gate

Before any result is returned, it must pass (`search.ts`, lines ~145-150):
- `finalScore >= 1.5`
- `content.length >= 40`
- Not flagged as "low-value content"

### Query Classification

**NOT Naive Bayes.** The query classifier in `search.ts` (line ~642) is a simple keyword-to-category mapper:

```typescript
// Actual query classification logic (search.ts ~line 642)
// error/bug/fix/issue           -> "error"
// implement/create/build/add    -> "implementation"
// how/why/analyze               -> "analysis"
// everything else               -> "general"
```

There is no statistical model, no training data, no probability distribution. Each query type routes to a different search strategy:
- `"error"` -- prioritizes error pattern matching, solution context extraction
- `"implementation"` -- prioritizes file references, code snippets, tool patterns
- `"analysis"` -- prioritizes Claude insights, explanations
- `"general"` -- standard scoring with no type-specific boosts

### Word Similarity (Not Edit-Distance)

**NOT edit-distance.** The `isWordSimilar()` function in `search-helpers.ts` (lines 441-455) uses positional character comparison:

```typescript
// From search-helpers.ts:441-455
static isWordSimilar(word1: string, word2: string): boolean {
  if (Math.abs(word1.length - word2.length) > 3) return false;
  const minLen = Math.min(word1.length, word2.length);
  if (minLen < 4) return false;
  const shared = minLen * 0.6;
  let matches = 0;
  for (let i = 0; i < minLen; i++) {
    if (word1[i] === word2[i]) matches++;
  }
  return matches >= shared;
}
```

Key properties:
- Words must be within 3 characters of each other in length
- Minimum 4 characters required
- Compares characters at the same position (index-aligned)
- Requires 60% positional match

This means `"typescript"` and `"typescipt"` (typo) would match (8/9 = 88%), but `"typescript"` and `"javascript"` would NOT (only `s`, `c`, `r`, `i`, `p`, `t` at positions 4-9 = 6/10 = 60% -- borderline). This is simpler and faster than Levenshtein distance but less flexible for transposition errors.

### Recency Boosting (Not Exponential Decay)

**NOT exponential decay.** The recency model in `search-helpers.ts` (lines 190-196) uses three discrete tiers:

```typescript
// From search-helpers.ts:190-196
const daysDiff = (now.getTime() - timestamp.getTime()) / (1000 * 60 * 60 * 24);
if (daysDiff < 1) score *= 1.5;       // < 24 hours: 50% boost
else if (daysDiff < 7) score *= 1.2;   // < 7 days: 20% boost
else if (daysDiff < 30) score *= 1.1;  // < 30 days: 10% boost
// Beyond 30 days: no boost (1.0x)
```

An exponential decay function would be something like `score *= Math.exp(-lambda * daysDiff)`. The actual implementation is a step function with three tiers. This is simpler, easier to tune, and "good enough" for conversation search where recency matters but shouldn't dominate.

### LRU Cache Strategy

The cache in `search.ts` (line ~520) is a `Map<string, CompactMessage[]>` with a hard cap of 500 entries:

```typescript
// From search.ts
private messageCache: Map<string, CompactMessage[]>;
private static readonly MAX_CACHE_SIZE = 500;
```

Eviction strategy: When the cache exceeds 500 entries, it evicts the entry with the **lowest average relevance score** (not the least-recently-used, despite the naming). This means high-quality search results persist longer in cache than low-quality ones.

Cache key: The stringified combination of project directory + filename. Each JSONL file gets one cache entry containing all its parsed `CompactMessage[]` objects.

### JSONL Noise Filtering Pipeline

The parser (`parser.ts`, 730 lines) implements a multi-stage noise filtering pipeline:

**Stage 1: Line-Level Filtering**
- Empty lines are skipped
- Malformed JSON lines are caught in try-catch and warned (not fatal)
- Messages with no extractable content are discarded

**Stage 2: Content Extraction** (`utils.ts:extractContentFromMessage()`)
- String content: used directly
- Array content: `text` items kept, `tool_use` replaced with `[Tool: name]`, `tool_result` replaced with `[Tool Result]`
- Everything else: empty string (discarded)

**Stage 3: Smart Content Preservation** (`parser.ts:smartContentPreservation()`)
Adaptive truncation limits based on content type:

| Content Type | Max Length | Detection Criteria |
|-------------|-----------|-------------------|
| Code | 4000 chars | Contains ` ``` `, `function `, `const `, `import `, `export ` |
| Error | 3500 chars | Error/exception terms + line numbers or stack traces |
| Technical | 3500 chars | File extensions, `src/`, `./`, `tool_use` |
| Conversational | 3000 chars | Everything else |

**Stage 4: Context Extraction** (`parser.ts:extractContext()`)
Extracts structured metadata from each message:
- **File references:** 6+ regex patterns covering standard extensions, git status output, common config files, path prefixes
- **Tool usage:** 3 methods -- direct `tool_use` content blocks, assistant `tool_use` content, text pattern matching (`[Tool: Read]`, `Called the Read tool`, `mcp__` patterns)
- **Error patterns:** 12+ regex patterns covering Unix errors (ENOENT, EACCES), JS errors (TypeError, ReferenceError), common phrases (permission denied, module not found)
- **Claude insights:** Solution patterns, explanation patterns (assistant messages only)
- **Code snippets:** Code blocks (400 char cap) and inline code (10-120 chars)
- **Action items:** Next steps, commands, numbered/bulleted lists
- **Bash commands:** Extracted from tool input parameters (100 char cap)

**Stage 5: Noise Elimination in Sentence Scoring** (`parser.ts:extractMostValuableContent()`, lines 526-626)
Aggressively penalizes noise patterns with -50 score:
- `"this session is being continued"`
- `"caveat:"`
- `"command-name>"`
- `"generated by the user while running"`
- `"local-command-stdout"`
- `"analysis:"`
- `"command-message>"`
- `"system-reminder"`
- Content < 50 chars

### Scoring Constants Reference

All constants from `scoring-constants.ts` (210 lines):

**Weight Constants:**

| Constant | Value | Purpose |
|----------|-------|---------|
| `EXACT_MATCH_SCORE` | 10 | Core tech term exact match (react, docker, etc.) |
| `SUPPORTING_TERM_SCORE` | 3 | 5+ char non-generic, non-core term match |
| `WORD_MATCH_SCORE` | 2 | General word match |
| `EXACT_PHRASE_BONUS` | 5 | Full query phrase appears in content |
| `MAJORITY_MATCH_BONUS` | 4 | >= 60% of query words match |
| `TOOL_USAGE_SCORE` | 5 | Message is tool_use or tool_result type |
| `FILE_REFERENCE_SCORE` | 3 | Content contains file paths (src/, .ts, .js) |
| `PROJECT_MATCH_SCORE` | 5 | Message CWD matches query project path |

**CORE_TECH_PATTERN** (~50 terms):
```
webpack, docker, react, vue, angular, node, npm, yarn, typescript,
python, rust, go, java, kubernetes, aws, gcp, azure, postgres, mysql,
redis, mongodb, graphql, rest, grpc, oauth, jwt, git, github, gitlab,
jenkins, nginx, apache, eslint, prettier, babel, vite, rollup, esbuild,
jest, mocha, cypress, playwright, nextjs, nuxt, svelte, tailwind, sass,
less, vitest, pnpm, turborepo, prisma, drizzle, sequelize, sqlite,
leveldb, indexeddb
```

**GENERIC_TERMS** (~187 terms organized by category):
- Action words: config, setup, install, build, deploy, test, run, start, create, update, fix, add, remove, change, optimize, improve, make, write, read, delete, check
- Testing: testing, tests, mocks, mocking, mock, stubs, stubbing, specs, coverage
- Design/Architecture: design, responsive, architecture, pattern, patterns
- Performance: caching, cache, rendering, render, bundle, bundling, performance
- Process: strategy, approach, implementation, solution, feature, system, process, handler, manager
- Common nouns: files, file, folder, directory, path, code, data, error, function, class, method, variable, component, module, package, library
- Format/Display: format, style, layout, display, show, hide, rules, options, settings, params, parameters
- Generic technical: server, client, request, response, async, await, promise, callback, import, export, require, include, define, declare, return, output, input
- Database/Schema: database, schema, models, table, query, migration, index, field, column
- Deployment/Infra: deployment, container, service, cluster, instance, environment, manifest, resource
- Common programming: interface, types, typing, object, array, string, number, boolean, value, property

### Performance Benchmarks

From `PERF.md` (1111 lines of version-by-version benchmarks):

**v1.0.5 (latest benchmarked):**

| Metric | Value |
|--------|-------|
| Average query time | ~0.9s |
| Average quality score | 4.7/5.0 |
| Tool count | 10 |
| Benchmark queries | 27 |

**Quality improvement over versions:**

| Version | Average Score | Notes |
|---------|--------------|-------|
| v1.0.1 | 2.2/5 | Baseline |
| v1.0.2 | 4.4/5 | All 7 (then) tools >= 4.0 |
| v1.0.5 | 4.7/5 | 10 tools, optimized scoring |

**Quality scoring breakdown:**
- Actionability: 40% weight (does the result help the user act?)
- Relevance: 30% weight (is the result about the right topic?)
- Completeness: 20% weight (does it provide enough context?)
- Efficiency: 10% weight (was the result returned quickly?)

### MCP Tool Surface

10 tools exposed (from `index.ts`, 1156 lines):

| Tool | Purpose | Key Parameters |
|------|---------|----------------|
| `search_conversations` | Primary search across all history | query, project, timeframe, limit |
| `find_file_context` | Find conversations about specific files | filePath, project |
| `find_similar_queries` | Find past queries similar to current | query, threshold |
| `get_error_solutions` | Find past solutions to similar errors | errorMessage, project |
| `list_recent_sessions` | List sessions sorted by recency | project, limit |
| `extract_compact_summary` | Generate compact session summary | sessionId, project |
| `find_tool_patterns` | Find common tool usage patterns | toolName, project |
| `search_plans` | Search Claude plans directory | query, limit |
| `search_config` | Search Claude config/rules/skills files | query, category |
| `search_tasks` | Search Claude tasks directory | query, limit |

The server includes a "doctor" diagnostic mode that benchmarks all tools and reports performance metrics.

### Semantic Query Expansion

`search-helpers.ts:expandQuery()` provides pseudo-semantic expansion via a hardcoded synonym table:

```typescript
// Technical term expansions (search-helpers.ts lines 10-21)
error    -> [exception, fail, crash, bug, issue]
fix      -> [resolve, solve, repair, correct]
implement -> [create, build, develop, add]
optimize -> [improve, enhance, speed up, performance]
debug    -> [troubleshoot, diagnose, trace]
deploy   -> [publish, release, launch]
test     -> [verify, check, validate]
config   -> [configuration, settings, setup]
auth     -> [authentication, login, security]
api      -> [endpoint, service, request]
```

Pattern-based expansions:
- `.ts` in query adds `typescript`, `type`
- `.js` in query adds `javascript`
- `npm` in query adds `package`, `dependency`
- `git` in query adds `version control`, `commit`

### Architecture Implications for Impulse

The claude-historian-mcp scoring system reveals several important lessons:

1. **Keyword matching is sufficient for coding conversations.** The 4.7/5 quality score was achieved without any embeddings, TF-IDF, or statistical models. Code conversations have high keyword density -- when someone asks about "react hooks," the answer literally contains "react" and "hooks."

2. **The hard gate on core tech terms is brilliant.** By rejecting results that don't contain the specific framework/tool mentioned in the query, it eliminates the most common failure mode of generic text search (matching "error handling" when the user asked about "React error boundaries").

3. **The "pain to rediscover" importance scoring aligns with Impulse's GENOME.md philosophy.** Both systems prioritize architectural decisions (2.5x) over routine code changes (1.0x). Impulse's session-end LLM extraction should weight decisions similarly.

4. **Multiplicative scoring creates non-linear boost stacking.** A message that is a decision (2.5x), contains code (2.0x), references files (1.2x), and is recent (1.5x) gets: base * 2.5 * 2.0 * 1.2 * 1.5 = base * 9.0. This creates strong separation between high-value and low-value results.

---

## 2. claude-history: The Simple Alternative

**Source:** [thejud/claude-history](https://github.com/thejud/claude-history) (not in cloned repos; analysis based on README and public source)

### What It Is

A zero-dependency Python CLI that converts Claude Code JSONL files into readable Markdown. No search, no scoring, no indexing -- pure data extraction.

### Key Properties

| Property | Value |
|----------|-------|
| Language | Python 3.6+ |
| Dependencies | Zero (stdlib only) |
| Output | Chronological Markdown |
| Modes | Prompts only (default), Full turns (`--agent` flag) |
| Branching | Handles conversation branching |
| Install | `pip install claude-history` or `pipx install claude-history` |
| Use case | Debugging, inspection, test fixture generation |

### How It Works

1. Reads JSONL files from `~/.claude/projects/<hash>/`
2. Parses each line as JSON
3. Extracts human/assistant messages
4. Handles conversation branching (JSONL can have non-linear message sequences)
5. Outputs as chronological Markdown

### Comparison with claude-historian-mcp

| Dimension | claude-history | claude-historian-mcp |
|-----------|---------------|---------------------|
| **Purpose** | Data extraction | Search + retrieval |
| **Language** | Python | TypeScript |
| **Dependencies** | Zero | Node 20+ |
| **Output** | Markdown dump | Ranked search results |
| **Scoring** | None | Multi-stage weighted |
| **Performance** | Fast (sequential I/O) | ~0.9s per query |
| **MCP integration** | No | Yes (10 tools) |
| **Best for** | "What's in this JSONL?" | "Find the auth conversation" |
| **Branching support** | Yes | Limited |
| **Noise filtering** | Basic | Aggressive (5-stage pipeline) |

### Value for Impulse

**Phase 0 (now):** Use claude-history to inspect raw JSONL structure, validate assumptions about message format, and generate test fixtures for impulse-plugin tests.

**Phase 1 (MVP):** No direct integration needed. Impulse reads JSONL indirectly through GENOME.md and HISTORY_INDEX.md.

**Phase 2+:** claude-history's branching detection logic is worth studying before building Impulse's own JSONL parser. It handles the non-linear message sequencing that trips up naive parsers.

---

## 3. sqlite-vec: Hands-On Analysis

**Source:** `cloned-repos/sqlite-vec/` (C source, documentation, examples, benchmarks)

### Virtual Table Architecture

sqlite-vec implements a custom SQLite virtual table called `vec0`. The internal architecture uses **shadow tables** -- real SQLite tables that back the virtual table interface.

From `ARCHITECTURE.md`:

```
vec0 virtual table "my_vectors"
  |-- my_vectors_chunks          (chunk_id, size, validity BLOB, rowids BLOB)
  |-- my_vectors_rowids          (rowid, id, chunk_id, chunk_offset)
  |-- my_vectors_vector_chunksNN (rowid, vector BLOB)
  |-- my_vectors_auxiliary       (rowid, valueNN [type])
  |-- my_vectors_metadatachunksNN (rowid, data BLOB)
  +-- my_vectors_metadatatextNN  (rowid, data TEXT)
```

**Key architectural properties:**
- Vectors are stored in **chunks** (groups of rows), not individually
- Each chunk has a `validity` blob (bitmap of which rows are alive vs deleted)
- Chunk-based storage enables efficient batch KNN scanning
- Metadata columns are stored separately from vector data (no bloat on KNN scans)

**Three query plans** (determined at query planning time via `idxStr` encoding):

| Plan | Code | When Used |
|------|------|-----------|
| FULLSCAN | `'1'` | No WHERE clause, or non-indexed constraints |
| POINT | `'2'` | `WHERE rowid = ?` -- single row lookup |
| KNN | `'3'` | `WHERE embedding MATCH ?` -- vector similarity search |

### The DELETE+INSERT Pattern

sqlite-vec virtual tables do **NOT support UPSERT** (`INSERT OR REPLACE`). Updating a vector requires:

```sql
-- Step 1: Delete the old vector
DELETE FROM vec_items WHERE rowid = ?;

-- Step 2: Insert the new vector
INSERT INTO vec_items(rowid, embedding) VALUES (?, vec_f32(?));
```

**Why this matters for Impulse:**
- When re-indexing a session (e.g., after editing HISTORY_INDEX.md), Impulse must delete all old vectors for that session and re-insert
- This is a transaction-safe pattern (wrap both in `BEGIN/COMMIT`)
- For small datasets (< 10K vectors), the overhead is negligible
- For large datasets, batch DELETE+INSERT is more efficient than individual operations

**Practical cost:** The DELETE step is O(log n) for the rowid lookup plus O(1) to mark the validity bit. The INSERT step is O(1) amortized (append to current chunk). Total update cost: O(log n), which is acceptable for Impulse's scale.

### Partition Key Support

sqlite-vec supports **partition keys** -- columns that can be used to filter KNN queries without scanning the entire index. This is critical for multi-project search.

From `ARCHITECTURE.md`:

```
VEC0_IDXSTR_KIND_KNN_PARTITION_CONSTRAINT (']')
  - Second character: partition key index ('A' + partition_idx)
  - Third character: operator (subset of comparisons)
  - Fourth character: filler '_'
```

**Impulse usage pattern:**

```sql
-- Create table with project_id as partition key
CREATE VIRTUAL TABLE session_vectors USING vec0(
  project_id TEXT PARTITION KEY,
  embedding float[384]
);

-- KNN query scoped to a single project
SELECT rowid, distance
FROM session_vectors
WHERE project_id = 'my-project'
  AND embedding MATCH ?
ORDER BY distance
LIMIT 5;
```

**Partition keys are NOT the same as WHERE filters on regular columns.** Partition keys are built into the index structure and enable the KNN scan to skip entire chunks that don't match the partition value. Regular WHERE filters (metadata columns) are applied AFTER the KNN scan.

### Metadata Column Filtering

sqlite-vec also supports metadata columns that can be filtered during KNN queries:

```
VEC0_IDXSTR_KIND_METADATA_CONSTRAINT ('&')
  - Second character: metadata column index ('A' + metadata_idx)
  - Third character: constraint operator
  - Fourth character: filler '_'
```

This allows queries like:

```sql
-- KNN with metadata filter
SELECT rowid, distance
FROM session_vectors
WHERE embedding MATCH ?
  AND session_date > '2026-01-01'
ORDER BY distance
LIMIT 5;
```

**Important:** Metadata filtering happens during the KNN scan but AFTER distance computation. It filters results, not the search space. For large datasets, this means the KNN scan still touches all chunks, but post-filters the results.

### Bun Integration Pattern

From `examples/simple-bun/demo.ts`:

```typescript
import { Database } from "bun:sqlite";
// Optional: use system SQLite for broader extension compatibility
Database.setCustomSQLite("/usr/local/opt/sqlite3/lib/libsqlite3.dylib");

const db = new Database(":memory:");
db.loadExtension("../../dist/vec0");

// Check versions
const { sqlite_version, vec_version } = db
  .prepare("select sqlite_version() as sqlite_version, vec_version() as vec_version;")
  .get();

// Create virtual table
db.exec("CREATE VIRTUAL TABLE vec_items USING vec0(embedding float[4])");

// Insert with Float32Array
const insertStmt = db.prepare(
  "INSERT INTO vec_items(rowid, embedding) VALUES (?, vec_f32(?))"
);
const insertVectors = db.transaction((items) => {
  for (const [id, vector] of items) {
    insertStmt.run(BigInt(id), new Float32Array(vector));
  }
});

// KNN query
const rows = db.prepare(`
  SELECT rowid, distance
  FROM vec_items
  WHERE embedding MATCH ?
  ORDER BY distance
  LIMIT 3
`).all(new Float32Array(query));
```

**Key integration details for Impulse:**
- Bun's built-in `bun:sqlite` supports `loadExtension()` directly
- Vectors must be passed as `Float32Array` (not plain arrays)
- Rowids should be passed as `BigInt` for safety
- `vec_f32()` is the SQL function that converts binary to float vector
- The `MATCH` operator triggers KNN search plan
- `ORDER BY distance` is required for KNN results
- `LIMIT` controls the K in K-nearest-neighbors
- Transactions significantly speed up batch inserts

### Performance at Scale

From `benchmarks/exhaustive-memory/bench.py` (622 lines), which benchmarks sqlite-vec against faiss, numpy, hnswlib, chroma, duckdb, lancedb, usearch, and sentence-transformers:

**Validated benchmark numbers** (from RESEARCH-DIGEST.md, cross-referenced with benchmark source):

| Scale | Write Latency | Read Latency (KNN) | Notes |
|-------|--------------|--------------------|----|
| 100 vectors | < 10ms | < 1ms | Trivial at this scale |
| 1,000 vectors | ~100ms | < 10ms | Sweet spot for Impulse |
| 10,000 vectors | ~788ms avg | < 100ms | Still acceptable |
| 100,000+ vectors | Seconds | Hundreds of ms | Consider Faiss at this scale |

**Comparison at 10K vectors:**

| Store | Write (10K batch) | Read (KNN k=10) | Recall |
|-------|-------------------|-----------------|--------|
| sqlite-vec | ~788ms | Sub-100ms | Good (exact search) |
| Faiss | ~47,640ms | Fastest | Best at scale |
| Chroma | Moderate | Moderate | Lowest |

**Why sqlite-vec wins for Impulse:**
1. **60x faster writes than Faiss.** Impulse writes at session-end, so write latency directly impacts session close time.
2. **Exact (exhaustive) KNN search.** No approximate neighbors -- 100% recall. At Impulse's scale (hundreds to low thousands of vectors), exact search is fast enough.
3. **Single-file storage.** No server process. The entire vector index lives in one `.sqlite` file alongside the rest of Impulse's data.
4. **Zero operational complexity.** No configuration, no tuning, no memory allocation parameters.

**Scale ceiling for Impulse:** At 1 session per day with 10 chunks per session, it takes ~3 years to reach 10K vectors. sqlite-vec is comfortable at this scale. Only consider alternatives if Impulse needs to index across dozens of projects simultaneously.

---

## 4. sentence-transformers: Embedding Analysis

**Source:** Public documentation and benchmarks (not in cloned repos; sentence-transformers is a Python library)

### Model Comparison

Two primary candidates for local embedding generation:

| Property | all-MiniLM-L6-v2 | nomic-embed-text |
|----------|-------------------|------------------|
| Dimensions | 384 | 768 |
| Model size | 22MB | ~270MB |
| Sequence length | 256 tokens | 8192 tokens |
| Parameters | 22.7M | 137M |
| Quality (MTEB avg) | ~62% | ~70% |
| Speed (relative) | Fast (1.0x) | Medium (~0.4x) |
| Offline | Yes | Yes |
| License | Apache 2.0 | Apache 2.0 |
| Cost | Free | Free |

### M-Series Mac Performance

Approximate latencies for embedding generation on Apple Silicon (based on published benchmarks and community reports):

| Model | M1 Pro | M2 Pro | M3 Pro | Batch of 100 |
|-------|--------|--------|--------|--------------|
| all-MiniLM-L6-v2 | ~5ms/text | ~4ms/text | ~3ms/text | ~300ms |
| nomic-embed-text | ~15ms/text | ~12ms/text | ~10ms/text | ~1000ms |

**For Impulse's use case** (embedding 10-50 conversation turns at session end):
- MiniLM: 50-250ms total (negligible)
- Nomic: 150-750ms total (acceptable)

Both are fast enough for session-end batch processing. The question is quality, not speed.

### Memory Footprint

| Model | Model Load | Per-Inference | Peak |
|-------|-----------|--------------|------|
| all-MiniLM-L6-v2 | ~100MB | ~50MB | ~200MB |
| nomic-embed-text | ~600MB | ~200MB | ~900MB |

**Impulse impact:** MiniLM adds ~200MB to session-end memory usage for a few seconds. nomic-embed-text adds ~900MB. On a 16GB M-series Mac, both are acceptable, but MiniLM is clearly lighter.

### Code-Specific Quality

The critical question: **do 384-dim embeddings from MiniLM capture enough semantic information for code conversations?**

**Arguments for MiniLM being sufficient:**
1. Code conversations have high keyword density. When someone discusses "React hooks for authentication," the words "React," "hooks," and "authentication" appear literally. Keyword matching (which MiniLM captures well) handles 80%+ of queries.
2. Impulse's primary search target is HISTORY_INDEX.md summaries, not raw code. Summaries are natural language with concentrated semantic content -- exactly what MiniLM was trained on.
3. The sqlite-vec benchmark shows that at Impulse's scale, recall is 100% (exact search). The quality question is about the embedding space, not the search algorithm.

**Arguments for nomic-embed-text:**
1. 768 dimensions capture finer-grained semantic relationships. "Authentication middleware" and "login security" are closer in 768-dim space than in 384-dim space.
2. 8192 token context window means entire session summaries can be embedded without truncation. MiniLM's 256 tokens requires chunking longer summaries.
3. Better quality on MTEB benchmarks (~70% vs ~62%) translates to fewer false positives in retrieval.

**Recommendation for Impulse:**
- **Phase 2 start:** Use all-MiniLM-L6-v2. It is faster, lighter, and sufficient for keyword-heavy code conversations.
- **Phase 2 evaluation:** After 100+ sessions, measure retrieval quality. If semantic queries ("what was the auth approach?") return poor results, switch to nomic-embed-text.
- **The 256-token limit is the real risk.** If session summaries in HISTORY_INDEX.md consistently exceed 256 tokens, nomic-embed-text's 8192-token window becomes a strong argument.

### Code Example: Embedding Generation

```python
from sentence_transformers import SentenceTransformer

# Load model (22MB download on first use)
model = SentenceTransformer('all-MiniLM-L6-v2')

# Embed session summaries
summaries = [
    "Implemented JWT auth with refresh tokens. Used bcrypt for hashing.",
    "Fixed CORS issue by adding allowed origins to middleware config.",
    "Refactored database layer to use connection pooling. 3x performance gain."
]

# Generate embeddings (returns numpy array, shape: [3, 384])
embeddings = model.encode(summaries)

# Convert to list for sqlite-vec insertion
for i, embedding in enumerate(embeddings):
    # embedding is a 384-dim float32 numpy array
    # Convert to bytes for sqlite-vec: struct.pack(f'{len(embedding)}f', *embedding)
    pass
```

**Integration path for Impulse (Phase 2):**
1. Python subprocess called at session-end (after LLM extraction)
2. Receives: new HISTORY_INDEX.md entry as stdin
3. Returns: 384-dim float32 vector as stdout (binary)
4. Impulse's Bun process inserts vector into sqlite-vec
5. Total added latency: < 500ms on M-series Mac

---

## 5. Impulse Phase Recommendations

Based on the source code analysis above, here are the specific recommendations for each phase:

### Phase 1: Plain Files + Grep (Current)

**Search approach:** None. Agents read GENOME.md and HISTORY_INDEX.md directly as plain text.

**Why this is sufficient:**
- GENOME.md stays small (< 100 lines) in early usage
- HISTORY_INDEX.md has < 10 entries for the first weeks
- The session-start hook injects the most recent 3-5 entries from HISTORY_INDEX.md
- Agents can `grep` HISTORY_INDEX.md for keywords if needed

**What to steal from claude-historian-mcp:**
- The "pain to rediscover" importance scoring concept. Impulse's session-end LLM extraction prompt should ask for decisions (highest value) over routine changes.
- The noise filtering patterns. Impulse should strip the same noise from JSONL if it ever reads raw conversation data.
- The `GENERIC_TERMS` set. When Impulse builds search in Phase 2, this set prevents false-positive matches on common words.

### Phase 2: FTS5 + sqlite-vec Hybrid (when HISTORY > 100 sessions)

**Search approach:** Dual-track search combining FTS5 keyword search and sqlite-vec semantic search.

**Architecture:**

```
User query: "how did we handle auth?"
  |
  |-- FTS5 path: Full-text search over HISTORY_INDEX.md text
  |   Matches: entries containing "auth", "authentication", "login"
  |   Fast, exact, no false positives
  |
  |-- sqlite-vec path: KNN search over embedded summaries
  |   Matches: entries semantically similar to "auth handling"
  |   Catches: "security middleware", "JWT implementation"
  |
  +-- Merge: Reciprocal Rank Fusion or simple score combination
      Return top 5 results with source attribution
```

**Why hybrid beats either alone:**
- FTS5 handles 80% of queries perfectly (high keyword density in code conversations)
- sqlite-vec catches the 20% where synonyms matter ("auth" vs "security middleware")
- FTS5 is instant; sqlite-vec adds < 100ms for KNN at Impulse's scale
- Both live in the same SQLite database file

**Phase 2 trigger:** When `HISTORY_INDEX.md` exceeds 100 entries OR when `grep` takes > 500ms to search it.

### Phase 3: Full Pipeline (when GENOME > 500 lines)

**Search approach:** Add mem0-style fact extraction on top of hybrid search.

**Why Phase 3 and not earlier:**
- Full mem0 requires 2 LLM calls per memory operation (20 calls per 10-turn session)
- Impulse's lean alternative (1 LLM call at session-end) achieves ~70-80% of mem0's accuracy
- The accuracy gap only matters when GENOME.md becomes too large for direct injection into context
- At 500+ lines, GENOME.md exceeds what can fit in a system prompt injection, requiring selective retrieval

### Search Approach Decision Matrix

| Query Type | Phase 1 | Phase 2 | Phase 3 |
|-----------|---------|---------|---------|
| "What was the last thing I worked on?" | Read HISTORY_INDEX.md tail | Same | Same |
| "How did we implement auth?" | grep HISTORY_INDEX.md | FTS5 + vec hybrid | Same + mem0 facts |
| "What's James's preferred testing approach?" | Read GENOME.md | Same | mem0 graph query |
| "Find all sessions about database migrations" | Manual grep | FTS5 search | FTS5 + metadata filter |
| "What architectural decisions have we made?" | Read GENOME.md decisions section | FTS5 filtered to decisions | mem0 decision facts |

---

## 6. Corrections to Prior Documentation

This section documents specific corrections to claims in existing documentation, based on actual source code analysis.

### TOOL-STACK-ANALYSIS.md Corrections

| Line/Section | Original Claim | Correction |
|-------------|---------------|------------|
| L179 (claude-historian-mcp) | "TF-IDF scoring with Naive Bayes query classification" | Custom multi-stage weighted keyword scoring. No TF-IDF. No Naive Bayes. |
| L180 | "Edit-distance fuzzy matching" | Positional character comparison with 60% threshold. Not edit-distance. |
| L181 | "Exponential time decay (recent results ranked higher)" | Discrete 3-tier recency boosting: <24h=1.5x, <7d=1.2x, <30d=1.1x |
| L179 | "6 MCP tools" | 10 MCP tools |

### RESEARCH-DIGEST.md Corrections

| Section | Original Claim | Correction |
|---------|---------------|------------|
| Section 5, Pattern 2 | "MCP Tool Search" implies TF-IDF | Search uses custom keyword scoring, not TF-IDF |
| Section 7 | Compaction description is accurate | No correction needed (independently validated) |
| Section 9 | "FTS5-powered keyword search is sufficient" | This claim is validated by claude-historian-mcp's 4.7/5 score using keyword scoring alone |

### CLAUDE.md Corrections

| Section | Original | Correction |
|---------|----------|------------|
| Cloned Repos table | claude-historian-mcp: "TF-IDF scoring, JSONL parsing" | Should read: "Multi-stage keyword scoring, JSONL parsing" |

---

## Appendix A: Source File Index

All files analyzed for this document:

| File | Lines | Purpose |
|------|-------|---------|
| `cloned-repos/claude-historian-mcp/src/scoring-constants.ts` | 210 | Scoring weights, CORE_TECH_PATTERN, GENERIC_TERMS |
| `cloned-repos/claude-historian-mcp/src/search.ts` | ~1953 | HistorySearchEngine class, query classification, cache |
| `cloned-repos/claude-historian-mcp/src/search-helpers.ts` | 594 | Query similarity, importance scoring, recency boost |
| `cloned-repos/claude-historian-mcp/src/parser.ts` | 730 | ConversationParser, noise filtering, context extraction |
| `cloned-repos/claude-historian-mcp/src/utils.ts` | 506 | calculateRelevanceScore, file system utilities |
| `cloned-repos/claude-historian-mcp/src/universal-engine.ts` | ~1922 | Universal engine wrapper (Desktop support disabled) |
| `cloned-repos/claude-historian-mcp/src/types.ts` | 113 | TypeScript interfaces |
| `cloned-repos/claude-historian-mcp/src/index.ts` | 1156 | MCP server, 10 tool definitions |
| `cloned-repos/claude-historian-mcp/PERF.md` | 1111 | Version-by-version benchmarks |
| `cloned-repos/sqlite-vec/ARCHITECTURE.md` | 123 | Shadow tables, idxStr encoding, query plans |
| `cloned-repos/sqlite-vec/README.md` | 160 | Overview, usage, supported platforms |
| `cloned-repos/sqlite-vec/examples/simple-bun/demo.ts` | 53 | Bun integration example |
| `cloned-repos/sqlite-vec/benchmarks/exhaustive-memory/bench.py` | 622 | 10+ store comparison benchmark |

---

## Appendix B: Glossary

| Term | Definition |
|------|-----------|
| **FTS5** | SQLite Full-Text Search extension v5. Provides tokenized keyword search with BM25 ranking. |
| **KNN** | K-Nearest Neighbors. Given a query vector, find the K vectors closest to it by distance (cosine, L2, etc.). |
| **CORE_TECH_PATTERN** | claude-historian-mcp's regex matching ~50 framework/tool names. Triggers hard-gate rejection if unmatched. |
| **GENERIC_TERMS** | claude-historian-mcp's set of ~187 common terms excluded from "supporting term" scoring. |
| **vec0** | sqlite-vec's virtual table implementation. |
| **Shadow tables** | Real SQLite tables that back a virtual table's data storage. |
| **Partition key** | A sqlite-vec column that enables scoped KNN search (skip chunks that don't match). |
| **MTEB** | Massive Text Embedding Benchmark. Standard benchmark for comparing embedding model quality. |
| **Reciprocal Rank Fusion** | A method for combining ranked results from multiple search systems. |

---

_Created: 2026-02-20 | Status: Research Complete v1.0_
_Based on actual source code analysis of cloned-repos/, not documentation claims._
