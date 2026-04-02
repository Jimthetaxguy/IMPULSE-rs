---
title: Database Architecture Guide
description: SQLite and sqlite-vec architecture for vector search and full-text search
version: '1.0'
updated: 2026-02-21
type: guide
category: development
phase: phase2
status: active
audience: builders
tags: [guide, database, sqlite, vector-search]
---

# Database Architecture Guide

> **Version:** 1.0 | **Status:** Phase 2+ | **Updated:** 2026-02-21
> ⚠️ **NOT APPLICABLE TO PHASE 1.** Phase 1 uses plain text files, no database.
> This guide covers Phase 2+ features: sqlite-vec vector search, FTS5 full-text search.
> See [RUST-CANONICAL-CONTRACT.md](../spec/RUST-CANONICAL-CONTRACT.md) for the current product contract. The older TypeScript-era Phase 1 spec is not present in this workspace.

---

## Overview

The SWARM harness uses **sqlite-vec** for vector similarity search and **better-sqlite3** for relational storage. This guide covers:

1. **Schema Design** - Tables, indexes, constraints
2. **Vector Operations** - Embedding storage and retrieval
3. **Performance Optimization** - Query patterns and tuning
4. **Migration Strategy** - Evolving schema safely
5. **Testing** - Database test patterns

---

## Schema Design

### Table: `events`

Stores all incoming events (messages, tool calls, etc.).

```sql
CREATE TABLE events (
  id TEXT PRIMARY KEY,                    -- "{type}-{agentId}-{timestamp}"
  type TEXT NOT NULL,                     -- 'message.updated', 'tool.execute'
  agent_id TEXT NOT NULL,                 -- Source agent identifier
  data TEXT NOT NULL,                     -- Full JSON event
  created_at INTEGER NOT NULL,            -- Unix timestamp (ms)
  expires_at INTEGER NOT NULL             -- TTL expiration (ms)
);

CREATE INDEX idx_events_agent ON events(agent_id);
CREATE INDEX idx_events_created ON events(created_at);
CREATE INDEX idx_events_expires ON events(expires_at);
```

**Row Lifespan:**

- Created: Event received
- Retained: 24 hours
- Purged: After expires_at via cleanup

**Query Examples:**

```sql
-- Get recent events for agent (last hour)
SELECT * FROM events
WHERE agent_id = ? AND created_at > ?
ORDER BY created_at DESC
LIMIT 8;

-- Remove expired events
DELETE FROM events WHERE expires_at < ?;
```

### Virtual Table: `vectors`

Stores 384-dimensional embeddings with fast cosine similarity search.

```sql
CREATE VIRTUAL TABLE vectors USING vec0(
  id TEXT PRIMARY KEY,                    -- Unique vector ID
  agent_id TEXT,                          -- Source agent
  partition TEXT,                         -- File path or topic
  embedding FLOAT32[384],                 -- 384-dimensional embedding
  confidence FLOAT,                       -- Confidence score (0-1)
  source_events TEXT,                     -- JSON array of event IDs
  created_at INTEGER                      -- Creation timestamp
);
```

**Vector Lifecycle:**

- Created: When pattern detector embeds event context
- Retained: Until confidence decays below threshold
- Indexed: By partition for scoped queries
- Queryable: Via cosine distance ordering

**Query Examples:**

```sql
-- Find most similar vectors from other agents
SELECT id, distance FROM vectors
WHERE partition MATCH ?
  AND agent_id != ?
ORDER BY distance
LIMIT 10;

-- Get vectors for agent in past hour
SELECT id, confidence FROM vectors
WHERE agent_id = ?
  AND created_at > ?;
```

### Table: `patterns`

Stores detected patterns (triggers for injections).

```sql
CREATE TABLE patterns (
  id TEXT PRIMARY KEY,                    -- Unique pattern ID
  source_agents TEXT NOT NULL,            -- JSON array: ["agent1", "agent2"]
  similarity FLOAT NOT NULL,              -- Cosine similarity (0.88-1.0)
  extracted_topic TEXT NOT NULL,          -- Shared topic
  suggested_injection TEXT NOT NULL,      -- Injection message (≤120 tokens)
  confidence_score FLOAT NOT NULL,        -- Current confidence (0-1)
  detected_at INTEGER NOT NULL,           -- Detection timestamp
  file_scope TEXT,                        -- JSON array: ["src/auth.ts"]
  created_at INTEGER NOT NULL,            -- When added to DB
  expires_at INTEGER                      -- Optional expiration
);

CREATE INDEX idx_patterns_created ON patterns(created_at);
CREATE INDEX idx_patterns_agents ON patterns(source_agents);
```

**Pattern Lifecycle:**

- Created: When similarity > 0.88
- Confidence Decay: Applied on read (not persisted)
- Injected: Via compaction hook
- Expired: After 24-48 hours or manual purge

### Table: `metadata`

Stores system state and configuration.

```sql
CREATE TABLE metadata (
  key TEXT PRIMARY KEY,                   -- Config key
  value TEXT,                             -- JSON-encoded value
  updated_at INTEGER                      -- Last update timestamp
);

-- Example rows:
INSERT INTO metadata VALUES ('schema_version', '1', {timestamp});
INSERT INTO metadata VALUES ('last_cleanup', '{timestamp}', {timestamp});
INSERT INTO metadata VALUES ('vector_count', '{count}', {timestamp});
```

---

## Vector Storage & Retrieval

### Embedding Process

```
Agent sends message
    ↓
Get recent 8 events for agent
    ↓
Concatenate event texts
    ↓
Call embedding model (sentence-transformers)
    ↓
Get 384-dimensional float32 vector
    ↓
Store in vectors table with partition key
```

### Similarity Search

```sql
-- Query pattern: Find similar work from other agents
SELECT
  id,
  agent_id,
  confidence,
  distance
FROM vectors
WHERE partition = ?              -- Same file/topic
  AND agent_id != ?              -- Different agent
ORDER BY distance ASC            -- Closest match first
LIMIT 10;
```

**Cosine Distance in sqlite-vec:**

- Metric: Euclidean in embedding space
- Range: [0, ∞), where 0 = identical, >1 = dissimilar
- Threshold: Similarity > 0.88 → distance < threshold
- Conversion: `similarity = 1 - (distance / 2)`

### Partition Strategy

Vectors are partitioned by file path or topic for scoped injection:

```
vectors table
├── partition: "src/auth.ts"
│   ├── agent-1: vector_1
│   ├── agent-2: vector_2
│   └── agent-3: vector_3
├── partition: "src/api.ts"
│   ├── agent-1: vector_4
│   └── agent-2: vector_5
└── partition: "default"
    └── agent-1: vector_6
```

**Injection Scoping:**

- Only agents working on same partition receive injection
- Reduces noise, improves relevance
- Fallback to "default" if partition unclear

---

## Performance Optimization

### Index Strategy

```sql
-- Hot path: "Give me recent events for agent"
CREATE INDEX idx_events_agent ON events(agent_id);
CREATE INDEX idx_events_created ON events(created_at);

-- Warm path: "Find similar vectors"
CREATE INDEX idx_vectors_partition ON vectors(partition);

-- Cold path: "Remove expired entries"
CREATE INDEX idx_events_expires ON events(expires_at);
CREATE INDEX idx_patterns_created ON patterns(created_at);
```

**Index Size Estimate (10k vectors):**

- events table: 2-5 MB
- vectors table: 15-20 MB (384 \* 4 bytes per vector)
- indexes: 5-10 MB
- **Total:** ~25-35 MB

### Query Optimization

```sql
-- ✅ GOOD: Filtered query with index
SELECT * FROM events
WHERE agent_id = ?
  AND created_at > ?
LIMIT 8;
-- Estimated: <10ms (uses index)

-- ❌ BAD: Unfiltered query (full scan)
SELECT * FROM events;
-- Estimated: >100ms (10k rows)

-- ✅ GOOD: Limit before processing
SELECT * FROM vectors
WHERE partition = ?
ORDER BY distance
LIMIT 10;
-- Estimated: <200ms (vector search)

-- ❌ BAD: Get all then filter in app
SELECT * FROM vectors;
for (const v of vectors) {
  if (v.partition === target) process(v);
}
-- Estimated: >1000ms
```

### Connection Settings

```typescript
const db = new Database(dbPath);

// Write-Ahead Logging (WAL) mode
db.pragma('journal_mode = WAL');
// Benefit: Better concurrency, faster writes

// Synchronous mode NORMAL
db.pragma('synchronous = NORMAL');
// Benefit: Faster writes, acceptable durability

// Memory-mapped I/O (optional)
db.pragma('mmap_size = 30000000'); // 30MB
// Benefit: Faster reads on large databases

// Temporary storage location
db.pragma('temp_store = MEMORY');
// Benefit: Faster temporary operations
```

**Performance Tuning Baseline:**

- Event insert: <50ms
- Vector search: <200ms (10k vectors)
- Pattern query: <100ms
- Cleanup batch: <500ms (1000 rows)

---

## Migration Strategy

### Version Control

```sql
-- metadata table tracks schema version
SELECT value FROM metadata WHERE key = 'schema_version';

-- Before operations:
const version = getSchemaVersion();
if (version < EXPECTED_VERSION) {
  throw new Error(`Schema version mismatch: ${version} < ${EXPECTED_VERSION}`);
}
```

### Migration Patterns

**Adding a Column:**

```sql
-- Safe: Does not break existing queries
ALTER TABLE patterns ADD COLUMN status TEXT DEFAULT 'active';
ALTER TABLE patterns ADD COLUMN last_injected_at INTEGER;
```

**Adding an Index:**

```sql
-- Safe: Improves performance, no schema change
CREATE INDEX IF NOT EXISTS idx_patterns_status ON patterns(status);
```

**Renaming a Column:**

```sql
-- Pattern: Create new column, copy data, drop old
ALTER TABLE patterns ADD COLUMN new_name TEXT;
UPDATE patterns SET new_name = old_name;
ALTER TABLE patterns DROP COLUMN old_name;
```

**Changing Vector Dimension:**

```
WARNING: Cannot change virtual table schemas.
Solution: Create new_vectors table with vec1 (new dimension)
Backfill: SELECT and re-embed all stored events
Migration: Point code to new_vectors, drop old_vectors
```

### Migration Workflow

```typescript
async function migrate(): Promise<void> {
  const currentVersion = getSchemaVersion();
  const targetVersion = 2;

  if (currentVersion === targetVersion) return; // Already current

  // Start transaction
  const transaction = db.transaction(() => {
    // Apply migration steps
    if (currentVersion < 2) {
      db.run('ALTER TABLE patterns ADD COLUMN status TEXT DEFAULT "active"');
      db.run('CREATE INDEX idx_patterns_status ON patterns(status)');
    }

    // Update version
    db.prepare('INSERT OR REPLACE INTO metadata (key, value, updated_at) VALUES (?, ?, ?)').run(
      'schema_version',
      targetVersion,
      Date.now()
    );
  });

  transaction();
  logger.info('Database migrated', { from: currentVersion, to: targetVersion });
}
```

---

## Testing Database Operations

### Test Patterns

**1. Setup & Teardown**

```typescript
let db: Database;
let dbPath: string;

beforeEach(() => {
  dbPath = createTempDbPath(); // /tmp/test-swarm-{uuid}.db
  db = new Database(dbPath);
  db.pragma('synchronous = OFF'); // Faster for tests
  initializeSchema(db);
});

afterEach(() => {
  db.close();
  // Note: temp file auto-cleaned by OS
});
```

**2. Idempotency Testing**

```typescript
it('should be idempotent (same input = same state)', async () => {
  const event = createMessageEvent();

  await db.storeEvent(event);
  const state1 = db.getEventCount();

  await db.storeEvent(event); // Duplicate (same ID)
  const state2 = db.getEventCount();

  expect(state1).toBe(state2); // Count unchanged
  expect(state2).toBe(1); // Only one event
});
```

**3. Performance Testing**

```typescript
it('should insert 1000 events in <500ms', async () => {
  const events = createMultipleEvents(1000);

  const { durationMs } = await measureTime(async () => {
    for (const event of events) {
      await db.storeEvent(event);
    }
  });

  expect(durationMs).toBeLessThan(500);
});
```

**4. Concurrent Write Testing**

```typescript
it('should handle concurrent writes safely', async () => {
  const promises = [];
  for (let i = 0; i < 100; i++) {
    const event = createMessageEvent({
      agentId: `agent-${i % 5}`, // 5 agents
      content: `Message ${i}`,
    });
    promises.push(db.storeEvent(event));
  }

  await Promise.all(promises);

  expect(db.getEventCount()).toBe(100);
});
```

---

## Monitoring & Maintenance

### Health Checks

```typescript
async function healthCheck(): Promise<HealthStatus> {
  try {
    // Can write?
    const testId = uuid();
    db.storeEvent(createMessageEvent({ agentId: testId }));
    db.deleteEvent(testId); // Cleanup

    // Can query?
    const count = db.getEventCount();

    // Size check
    const sizeBytes = fs.statSync(dbPath).size;
    const sizeMB = sizeBytes / 1024 / 1024;

    return {
      status: 'healthy',
      eventCount: count,
      databaseSizeMB: sizeMB,
      timestamp: Date.now(),
    };
  } catch (error) {
    return {
      status: 'unhealthy',
      error: error.message,
      timestamp: Date.now(),
    };
  }
}
```

### Cleanup Job

```typescript
// Run every 6 hours
async function cleanupExpiredData(): Promise<void> {
  const now = Date.now();

  // Remove expired events
  const eventCount = db.prepare('DELETE FROM events WHERE expires_at < ?').run(now).changes;

  // Remove old patterns
  const patternCount = db.prepare('DELETE FROM patterns WHERE expires_at < ?').run(now).changes;

  // Optimize database
  db.run('VACUUM');

  logger.info('Database cleanup complete', {
    eventsDeleted: eventCount,
    patternsDeleted: patternCount,
  });
}

// Schedule in harness
setInterval(() => cleanupExpiredData(), 6 * 60 * 60 * 1000);
```

### Monitoring Metrics

```typescript
interface DatabaseMetrics {
  eventCount: number;
  vectorCount: number;
  patternCount: number;
  databaseSizeMB: number;
  avgQueryLatencyMs: number;
  lastCleanupAt: number;
}

function getMetrics(): DatabaseMetrics {
  return {
    eventCount: db.prepare('SELECT COUNT(*) as count FROM events').get().count,
    vectorCount: db.prepare('SELECT COUNT(*) as count FROM vectors').get().count,
    patternCount: db.prepare('SELECT COUNT(*) as count FROM patterns').get().count,
    databaseSizeMB: fs.statSync(dbPath).size / 1024 / 1024,
    avgQueryLatencyMs: metrics.avgLatencyMs,
    lastCleanupAt: getMetadataValue('last_cleanup'),
  };
}
```

---

## References

- sqlite-vec: https://github.com/asg017/sqlite-vec
- better-sqlite3: https://github.com/WiseLibs/better-sqlite3
- SQL Optimization: `docs/guides/BEST-PRACTICES.md` → Database section
- Phase 1 Spec: `docs/archive/SPEC-v1.1.md`

---

_Created: 2026-02-20 | Status: Design v1.0 Ready for Phase 1 Implementation_
