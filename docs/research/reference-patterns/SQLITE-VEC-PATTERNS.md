---
title: sqlite-vec Reference Patterns
description: Reference patterns for sqlite-vec vector search extension
version: '1.0'
updated: 2026-02-21
type: reference
category: patterns
phase: phase2
status: active
audience: builders
tags: [sqlite-vec, vector-search, knn, embeddings, sqlite, reference-pattern]
extracted_from: cloned-repos/sqlite-vec (v0.1.7-alpha.10)
---

# sqlite-vec Reference Patterns

Extracted patterns from `sqlite-vec` by Alex Garcia -- an extremely small, "fast enough" vector search SQLite extension written in pure C with zero dependencies. Successor to `sqlite-vss`. Mozilla Builders sponsored project.

**Source repo:** `cloned-repos/sqlite-vec/` (can be deleted after this doc is verified)

---

## 1. Overview

sqlite-vec adds vector search capabilities directly to SQLite via a `vec0` virtual table. Key properties:

- **Pure C, zero dependencies** -- runs anywhere SQLite runs (Linux, macOS, Windows, WASM, Raspberry Pi)
- **Three vector types:** `float32`, `int8`, and `bit` (binary)
- **Exhaustive KNN search** -- no approximate indexing (HNSW etc.) in Phase 1; brute-force scan with chunked storage
- **Metadata + partition key columns** -- filter during KNN, not after
- **Auxiliary columns** (prefixed with `+`) -- store non-vector data alongside vectors
- **Pre-v1** -- expect breaking changes

The core thesis matches Impulse's: start with the simplest correct thing (exhaustive search), add complexity (ANN indexes) only when scale demands it.

---

## 2. Virtual Table API

### Basic Creation

```sql
CREATE VIRTUAL TABLE vec_items USING vec0(
  embedding float[4]
);
```

The dimension is declared in square brackets. Supported element types:

| Type Declaration | Bytes/Element | Use Case                                   |
| ---------------- | ------------- | ------------------------------------------ |
| `float[N]`       | 4             | Standard embeddings (OpenAI, Cohere, etc.) |
| `int8[N]`        | 1             | Quantized embeddings, 4x storage reduction |
| `bit[N]`         | 1/8           | Binary quantization, 32x reduction         |

### With Primary Key

```sql
CREATE VIRTUAL TABLE vec_sentences USING vec0(
  id INTEGER PRIMARY KEY,
  sentence_embedding FLOAT[1536]
);
```

### With Partition Keys (Multi-Tenant Sharding)

Partition keys internally shard the vector index. KNN queries scoped to a partition only scan that partition's chunks.

```sql
CREATE VIRTUAL TABLE vec_chunks USING vec0(
  user_id INTEGER PARTITION KEY,
  contents_embedding float[1024]
);
```

### With Auxiliary Columns

Auxiliary columns (prefixed with `+`) store non-vector data inline. They are returned in query results but are NOT used for filtering during KNN.

```sql
CREATE VIRTUAL TABLE vec_chunks USING vec0(
  user_id INTEGER PARTITION KEY,
  +contents TEXT,
  contents_embedding float[1024]
);
```

### With Metadata Columns (Filterable During KNN)

Metadata columns (no prefix, not partition key, not vector) ARE filtered during KNN scan. This is important: filtering happens during the scan, not as a post-filter.

```sql
CREATE VIRTUAL TABLE vec_movies USING vec0(
  movie_id INTEGER PRIMARY KEY,
  synopsis_embedding float[768],
  genre TEXT,             -- metadata: filtered during KNN
  num_reviews INT,        -- metadata: filtered during KNN
  mean_rating FLOAT,      -- metadata: filtered during KNN
  +title TEXT             -- auxiliary: returned but not filtered
);
```

### Chunk Size Configuration

```sql
CREATE VIRTUAL TABLE vec_items USING vec0(
  embedding float[128],
  chunk_size=8            -- default varies; smaller = less memory per scan
);
```

---

## 3. KNN Query Syntax

### Basic KNN

The `MATCH` operator triggers KNN search. `ORDER BY distance` is required. `LIMIT` controls k.

```sql
SELECT rowid, distance
FROM vec_items
WHERE embedding MATCH '[0.890, 0.544, 0.825, 0.961]'
ORDER BY distance
LIMIT 5;
```

### KNN with `k =` Syntax (Alternative)

Instead of `LIMIT`, you can use `AND k = N` in the WHERE clause:

```sql
SELECT rowid, distance
FROM vec_items
WHERE embedding MATCH '[0.890, 0.544, 0.825, 0.961]'
  AND k = 5
ORDER BY distance;
```

### KNN with Partition Key Filter

```sql
SELECT rowid, user_id, contents, distance
FROM vec_chunks
WHERE contents_embedding MATCH ?
  AND user_id = 123
  AND k = 5;
```

### KNN with Metadata Filters

All metadata filters are applied DURING the scan, not post-hoc:

```sql
SELECT movie_id, title, genre, num_reviews, mean_rating, distance
FROM vec_movies
WHERE synopsis_embedding MATCH '[15.5]'
  AND genre = 'scifi'
  AND num_reviews BETWEEN 100 AND 500
  AND mean_rating > 3.5
  AND k = 5;
```

### KNN with `rowid IN (...)` Pre-filter

```sql
SELECT rowid, distance
FROM vec_items
WHERE embedding MATCH ?
  AND rowid IN (1, 2, 3, 10, 20)
  AND k = 5
ORDER BY distance;
```

### JOIN Pattern (Vector Table + Data Table)

A common pattern: store vectors in `vec0`, store full data in a regular table, JOIN on results:

```sql
SELECT
  vec_sentences.id,
  distance,
  sentence
FROM vec_sentences
LEFT JOIN sentences ON sentences.id = vec_sentences.id
WHERE sentence_embedding MATCH ?
  AND k = 3
ORDER BY distance;
```

---

## 4. Embedding Storage Format

### Input Formats

Vectors can be inserted as:

1. **JSON text** -- `'[0.1, 0.2, 0.3, 0.4]'`
2. **Raw bytes (BLOB)** -- packed binary, 4 bytes per float32 element

### Python Serialization

```python
from struct import pack
from typing import List

def serialize_float32(vector: List[float]) -> bytes:
    """Pack floats into raw bytes for sqlite-vec."""
    return pack("%sf" % len(vector), *vector)

def serialize_int8(vector: List[int]) -> bytes:
    """Pack int8s into raw bytes for sqlite-vec."""
    return pack("%sb" % len(vector), *vector)
```

### TypeScript/Bun Serialization

```typescript
// Float32Array buffer IS the raw bytes format
const vector = [0.1, 0.2, 0.3, 0.4];
insertStmt.run(BigInt(id), new Float32Array(vector));

// For Deno (needs Uint8Array wrapper)
new Uint8Array(new Float32Array(vector).buffer);
```

### Go Serialization

```go
import sqlite_vec "github.com/asg017/sqlite-vec-go-bindings/ncruces"

v, err := sqlite_vec.SerializeFloat32([]float32{0.1, 0.2, 0.3})
stmt.BindBlob(2, v)
```

### Utility Functions

| Function                   | Purpose                                    |
| -------------------------- | ------------------------------------------ |
| `vec_f32(x)`               | Construct float32 vector from JSON or BLOB |
| `vec_int8(x)`              | Construct int8 vector from JSON or BLOB    |
| `vec_bit(x)`               | Construct bit vector from BLOB             |
| `vec_to_json(x)`           | Convert any vector to JSON text            |
| `vec_length(x)`            | Number of elements in vector               |
| `vec_type(x)`              | Returns `'float32'`, `'int8'`, or `'bit'`  |
| `vec_normalize(x)`         | L2 normalization (float32 only)            |
| `vec_slice(x, start, end)` | Subset extraction (Matryoshka embeddings)  |
| `vec_each(x)`              | Table function: iterate elements           |

### Distance Functions

| Function                     | Vectors       | Notes              |
| ---------------------------- | ------------- | ------------------ |
| `vec_distance_L2(a, b)`      | float32, int8 | Euclidean distance |
| `vec_distance_cosine(a, b)`  | float32, int8 | Cosine distance    |
| `vec_distance_hamming(a, b)` | bit only      | Hamming distance   |

### Quantization

| Function                 | Purpose                                             |
| ------------------------ | --------------------------------------------------- |
| `vec_quantize_binary(x)` | float32/int8 to bit vector (positive=1, negative=0) |
| `vec_quantize_i8(x)`     | float32 to int8 (details TBD in pre-v1)             |

---

## 5. Multi-Language Bindings

### Python (Primary -- Best Documented)

```bash
pip install sqlite-vec
```

```python
import sqlite3
import sqlite_vec

db = sqlite3.connect(":memory:")
db.enable_load_extension(True)
sqlite_vec.load(db)
db.enable_load_extension(False)

# Create table, insert, query as normal SQL
db.execute("CREATE VIRTUAL TABLE v USING vec0(embedding float[1536])")
```

The Python package also provides:

- `serialize_float32()` / `serialize_int8()` helpers
- `register_numpy()` for zero-copy NumPy array registration as static blob tables

### Node.js

```bash
npm install sqlite-vec
```

```javascript
import * as sqliteVec from 'sqlite-vec';
// Load into your SQLite connection (better-sqlite3, etc.)
sqliteVec.load(db);
```

### Bun (Native)

```typescript
import { Database } from 'bun:sqlite';
db.loadExtension('path/to/vec0');
// Use Float32Array for binary vector format
insertStmt.run(BigInt(id), new Float32Array(vector));
```

### Deno

```typescript
import { Database } from 'jsr:@db/sqlite@0.11';
import * as sqliteVec from 'npm:sqlite-vec@0.0.1-alpha.9';
sqliteVec.load(db);
```

### Rust

```bash
cargo add sqlite-vec
```

```rust
// Register as auto-extension with rusqlite
unsafe {
    sqlite3_auto_extension(Some(
        std::mem::transmute(sqlite3_vec_init as *const ())
    ));
}
let conn = Connection::open_in_memory().unwrap();
```

### Go

```go
import sqlite_vec "github.com/asg017/sqlite-vec-go-bindings/ncruces"
// Auto-registers on import
```

### Also Available

- **Ruby:** `gem install sqlite-vec`
- **Datasette:** `datasette install datasette-sqlite-vec`
- **rqlite:** `rqlited -extensions-path=sqlite-vec.tar.gz`
- **sqlite-utils:** `sqlite-utils install sqlite-utils-sqlite-vec`

---

## 6. Performance Characteristics

### Search Algorithm

- **Exhaustive (brute-force) KNN** -- scans all vectors in the relevant partition/chunk
- No HNSW, IVF, or other approximate nearest-neighbor index
- Vectors stored in contiguous chunks (shadow tables: `_chunks`, `_vector_chunksNN`)
- Chunk-based scanning means memory usage is bounded per query

### Shadow Table Architecture

For a virtual table `xyz`, sqlite-vec creates:

| Shadow Table           | Purpose                                                  |
| ---------------------- | -------------------------------------------------------- |
| `xyz_chunks`           | Chunk metadata (chunk_id, size, validity bitmap, rowids) |
| `xyz_rowids`           | Rowid-to-chunk mapping (rowid, chunk_id, chunk_offset)   |
| `xyz_vector_chunksNN`  | Raw vector data per chunk                                |
| `xyz_auxiliary`        | Auxiliary column values                                  |
| `xyz_metadatachunksNN` | Metadata column chunk data                               |
| `xyz_metadatatextNN`   | Text metadata values                                     |

### Query Plans (Internal)

Three query plan types:

1. `FULLSCAN` -- scan all rows
2. `POINT` -- single rowid lookup
3. `KNN` -- nearest-neighbor with optional partition/metadata constraints

### Benchmark Context (from `benchmarks/exhaustive-memory/`)

The repo includes benchmarks comparing sqlite-vec against:

- NumPy brute-force
- FAISS (IndexFlatL2)
- hnswlib (brute-force mode)
- usearch
- DuckDB
- LanceDB
- ChromaDB
- sentence-transformers

sqlite-vec is positioned as "fast enough" -- not the fastest, but the most portable and the simplest to deploy. The trade-off is intentional: zero dependencies and SQLite-native storage vs. raw throughput.

### Known Limits

- No approximate indexing (yet) -- O(n) per query
- Pre-v1: API may change
- `chunk_size` parameter affects memory-per-query trade-off
- PRAGMA `page_size` affects I/O characteristics

---

## 7. Application to Impulse Phase 2

### Why sqlite-vec Fits Impulse

| Impulse Need                | sqlite-vec Capability                                      |
| --------------------------- | ---------------------------------------------------------- |
| **File-first architecture** | Vectors stored in SQLite DB file -- one file, git-friendly |
| **No external services**    | Pure C extension, no server, no network calls              |
| **Bun/TypeScript runtime**  | npm package available; Bun can load native extensions      |
| **Progressive complexity**  | Start with FTS5 (Phase 1), add vectors alongside (Phase 2) |
| **Multi-tenant isolation**  | Partition keys isolate per-project or per-user data        |
| **Metadata filtering**      | Filter by session, date, type during KNN scan              |

### Proposed Integration Architecture

```
Phase 1 (current):  FTS5 full-text search on GENOME.md + HISTORY_INDEX.md
Phase 2 (planned):  FTS5 + sqlite-vec hybrid

┌─────────────────────────────────────────┐
│ knowledge.db (single SQLite file)       │
│                                         │
│  ┌─ FTS5 tables (existing) ──────────┐  │
│  │ docs_fts, concepts, decisions...  │  │
│  └───────────────────────────────────┘  │
│                                         │
│  ┌─ vec0 tables (Phase 2) ───────────┐  │
│  │ vec_sessions: session summaries   │  │
│  │ vec_facts: extracted GENOME facts │  │
│  │ vec_queries: user query cache     │  │
│  └───────────────────────────────────┘  │
│                                         │
│  Search: FTS5 for keyword → vec0 for   │
│  semantic similarity → merge + rank    │
└─────────────────────────────────────────┘
```

### Proposed Schema

```sql
-- Session embedding index
CREATE VIRTUAL TABLE vec_sessions USING vec0(
  session_id INTEGER PRIMARY KEY,
  summary_embedding FLOAT[384],    -- MiniLM-L6-v2 or similar small model
  +summary TEXT,                    -- human-readable summary
  session_date TEXT                 -- metadata: filter by date range
);

-- Fact/decision embedding index
CREATE VIRTUAL TABLE vec_facts USING vec0(
  fact_id INTEGER PRIMARY KEY,
  fact_embedding FLOAT[384],
  +content TEXT,
  fact_type TEXT,                   -- metadata: 'decision', 'preference', 'pattern'
  confidence FLOAT                 -- metadata: filter by confidence
);
```

### Hybrid Search Pattern

```sql
-- 1. FTS5 keyword search
SELECT doc_id, rank FROM docs_fts WHERE docs_fts MATCH 'hooks architecture';

-- 2. Vector semantic search
SELECT fact_id, distance FROM vec_facts
WHERE fact_embedding MATCH ?   -- embedding of "hooks architecture"
  AND k = 10;

-- 3. Merge: union results, reciprocal rank fusion
```

### Embedding Model Considerations

For Impulse Phase 2, embeddings must be generated locally (no API calls per ADR-0004). Options:

| Model               | Dimensions | Size   | Notes                             |
| ------------------- | ---------- | ------ | --------------------------------- |
| `all-MiniLM-L6-v2`  | 384        | 80 MB  | Good baseline, widely used        |
| `nomic-embed-text`  | 768        | 274 MB | Better quality, Ollama-compatible |
| `mxbai-embed-large` | 1024       | 670 MB | High quality, heavier             |

Related projects from the same author:

- **`sqlite-lembed`** -- Generate embeddings locally from `.gguf` models directly in SQLite
- **`sqlite-rembed`** -- Generate embeddings from remote APIs (OpenAI, Ollama, Nomic)

### Integration Checklist (When Phase 2 Starts)

- [ ] Verify `npm install sqlite-vec` works with Bun native SQLite
- [ ] Decide embedding model (local-only constraint from ADR-0004)
- [ ] Design schema: which Impulse data gets embeddings (sessions, facts, both?)
- [ ] Implement hybrid search: FTS5 keyword + vec0 semantic + reciprocal rank fusion
- [ ] Benchmark: is exhaustive KNN fast enough for expected data size? (likely yes for <100K vectors)
- [ ] Add `vec0` table creation to `impulse init` flow
- [ ] Test: embedding generation latency during SessionEnd hook (must stay under budget)

---

_Extracted from `cloned-repos/sqlite-vec/` (commit at clone time). The cloned repo can be removed once this document is validated._
