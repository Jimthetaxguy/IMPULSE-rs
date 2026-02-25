# Data Models: Formal Schema Specifications

> **Version:** 1.0
> **Database:** SQLite 3.40+
> **Vector Store:** sqlite-vec (C extension)
> **Vector Dimensions:** 384 (sentence-transformers all-MiniLM-L6-v2)

---

## Overview

All SWARM ephemeral state lives in a single SQLite database: `~/.impulse/live_state.db`

**Three table classes:**
1. **Regular tables** — Metadata, audit trail (standard UPSERT support)
2. **Virtual table** — Pattern embeddings (sqlite-vec, DELETE+INSERT only)
3. **Indices** — Query optimization

**Key constraint:** No UPSERT on virtual tables. Pattern updates use DELETE+INSERT pattern (see `live_patterns` schema).

---

## live_state.db Schema

### Table 1: active_agents

Tracks all agents currently active in the session.

```sql
CREATE TABLE active_agents (
  agent_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  agent_type TEXT NOT NULL,  -- 'opencode' | 'claude-code' | 'aider'
  status TEXT NOT NULL DEFAULT 'idle',  -- 'idle' | 'working' | 'compacting'
  last_heartbeat INTEGER NOT NULL,  -- Unix timestamp (sec)
  file_list TEXT,  -- JSON array of file paths recently touched
  created_at INTEGER NOT NULL,
  INDEX idx_session(session_id),
  INDEX idx_last_heartbeat(last_heartbeat)
);
```

**Lifecycle:**
- **INSERT**: When agent sends first message in session
- **UPDATE**: On each `message.updated` hook (update `last_heartbeat`, `file_list`, `status`)
- **DELETE**: On session shutdown (via cleanup routine)

**Queries:**
```sql
-- Find active agents (heartbeat <60s ago)
SELECT * FROM active_agents
WHERE (strftime('%s', 'now') - last_heartbeat) < 60;

-- Get agents' current files
SELECT agent_id, file_list FROM active_agents
WHERE session_id = ?;
```

---

### Table 2: live_patterns (virtual, sqlite-vec)

Stores vector embeddings of detected patterns. **Virtual table** — no standard INSERT/UPDATE/DELETE.

```sql
-- Create virtual table for vector storage
CREATE VIRTUAL TABLE live_patterns USING vec0(
  embedding(384)  -- 384-dim float32 vectors
);

-- Create companion regular table for metadata (UPSERT-able)
CREATE TABLE live_patterns_metadata (
  pattern_id TEXT PRIMARY KEY,
  vec_rowid INTEGER NOT NULL UNIQUE,  -- Foreign key to vec0 rowid
  source_agent TEXT NOT NULL,
  partition_key TEXT NOT NULL,  -- Same as source_agent (for per-agent queries)
  confidence REAL NOT NULL,  -- 0.0-1.0
  pattern_text TEXT NOT NULL,
  agents_seen TEXT NOT NULL,  -- JSON array
  first_seen INTEGER NOT NULL,  -- Unix timestamp
  last_updated INTEGER NOT NULL,
  last_injected INTEGER,  -- Unix timestamp of last time this pattern was injected
  created_at INTEGER NOT NULL,
  INDEX idx_partition(partition_key),
  INDEX idx_confidence(confidence),
  INDEX idx_last_updated(last_updated)
);
```

**Update pattern (confidence or agents_seen):**
```python
# Step 1: Delete from vector table
cursor.execute("DELETE FROM live_patterns WHERE rowid = ?", (vec_rowid,))

# Step 2: Re-insert embedding
cursor.execute("INSERT INTO live_patterns(embedding) VALUES (?)",
               (embedding_bytes,))  # Returns new rowid
new_rowid = cursor.lastrowid

# Step 3: UPSERT metadata
cursor.execute("""
    INSERT INTO live_patterns_metadata
    (pattern_id, vec_rowid, source_agent, confidence, agents_seen, last_updated)
    VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(pattern_id) DO UPDATE SET
        vec_rowid = excluded.vec_rowid,
        confidence = excluded.confidence,
        agents_seen = excluded.agents_seen,
        last_updated = excluded.last_updated
""", (pattern_id, new_rowid, source_agent, new_confidence, new_agents_seen, now()))

# Step 4: Update injection log
cursor.execute("""
    UPDATE live_patterns_metadata
    SET last_injected = ?
    WHERE pattern_id = ?
""", (now(), pattern_id))
```

**Queries:**
```sql
-- Find patterns similar to query embedding (cosine distance)
SELECT
  pm.pattern_id,
  pm.source_agent,
  pm.confidence,
  pm.pattern_text,
  distance
FROM live_patterns lp
JOIN live_patterns_metadata pm ON lp.rowid = pm.vec_rowid
WHERE lp.embedding MATCH vec_distance_cosine(?)
  AND pm.partition_key != ?  -- Exclude source agent's own patterns
ORDER BY distance ASC
LIMIT 10;

-- Find stale patterns (last_updated >30 min ago)
SELECT pattern_id, confidence FROM live_patterns_metadata
WHERE (strftime('%s', 'now') - last_updated) > 1800;
```

**Cleanup (Phase 1 end):**
```sql
-- Delete patterns not updated in >2 hours
DELETE FROM live_patterns_metadata
WHERE (strftime('%s', 'now') - last_updated) > 7200;

-- Vacuum to reclaim space
VACUUM;
```

---

### Table 3: pattern_metadata (redundant, for performance)

Already defined above as `live_patterns_metadata`. This is the only metadata table we need.

---

### Table 4: injection_log

Audit trail of all injections sent. Never cleaned up during session (archived on shutdown).

```sql
CREATE TABLE injection_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_agent TEXT NOT NULL,
  target_agent TEXT NOT NULL,
  pattern_id TEXT NOT NULL,
  confidence REAL NOT NULL,
  injection_text TEXT NOT NULL,
  context_usage_pct REAL,  -- 0-100
  decision_reason TEXT,  -- "ACCEPT" | reason for rejection
  status TEXT NOT NULL DEFAULT 'sent',  -- 'sent' | 'failed' | 'rejected'
  timestamp INTEGER NOT NULL,
  INDEX idx_target_agent(target_agent),
  INDEX idx_timestamp(timestamp),
  FOREIGN KEY (pattern_id) REFERENCES live_patterns_metadata(pattern_id)
);
```

**Insertion:**
```python
cursor.execute("""
    INSERT INTO injection_log
    (source_agent, target_agent, pattern_id, confidence, injection_text,
     context_usage_pct, decision_reason, status, timestamp)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
""", (
    source_agent,
    target_agent,
    pattern_id,
    confidence,
    injection_text,
    context_usage_pct,
    "ACCEPT",
    "sent",
    int(time.time())
))
```

**Queries:**
```sql
-- Find last injection to target agent (for rate limiting)
SELECT * FROM injection_log
WHERE target_agent = ? AND source_agent = ?
ORDER BY timestamp DESC LIMIT 1;

-- Count agents injected in last N minutes (runaway check)
SELECT COUNT(DISTINCT source_agent) as agent_count
FROM injection_log
WHERE pattern_id = ? AND timestamp >= ? AND status = 'sent';

-- Summary for session report
SELECT
  COUNT(*) as total_injections,
  COUNT(CASE WHEN status = 'sent' THEN 1 END) as sent,
  COUNT(CASE WHEN status = 'rejected' THEN 1 END) as rejected,
  AVG(confidence) as avg_confidence
FROM injection_log
WHERE timestamp >= ?;
```

---

## LIVE.md Template

**File:** Created at session start, updates on pattern changes

```markdown
# LIVE — {session_id}

> **Updated:** {ISO 8601 timestamp}
> **Agents active:** {count} | **Patterns detected:** {count}

## Active Agents

| Agent | Type | Files | Last Activity | Status |
|-------|------|-------|---------------|--------|
| opencode-1 | OpenCode | auth.ts, utils.ts | 2 min ago | working |
| claude-code-1 | Claude Code | auth.ts, db.ts | 1 min ago | idle |

## Shared Patterns

| Pattern | Confidence | Agents | First Seen | Status |
|---------|-----------|--------|-----------|--------|
| Exploring authentication handler for session management | 0.92 | opencode-1, claude-code-1 | 8 min ago | active |
| Database schema changes in progress | 0.78 | claude-code-1 (new) | 3 min ago | pending |

## Quick Reference

- Both agents exploring authentication — **share approach in next message**
- Database schema work detected — check for conflicts

> Generated by SWARM harness at {timestamp}
```

**Update frequency:** On every successful injection (or every 30s if no changes)

---

## Promotion Criteria: Live → Tier 2 (Phase 2)

When a pattern in `live_state.db` qualifies for promotion to persistent Tier 2 storage:

```python
def should_promote(pattern: Pattern) -> bool:
    now = time.time()

    # Criterion 1: Confidence ≥ 0.93
    if pattern.confidence < 0.93:
        return False

    # Criterion 2: Seen in ≥ 2 agents
    agents_seen = json.loads(pattern.agents_seen)
    if len(agents_seen) < 2:
        return False

    # Criterion 3: Age ≥ 10 minutes
    age_minutes = (now - pattern.first_seen) / 60
    if age_minutes < 10:
        return False

    # Criterion 4: Not an echo (no SWARM prefix)
    if "[SWARM" in pattern.pattern_text:
        return False

    # Criterion 5: Entropy ≥ 3.0 bits
    entropy = shannon_entropy(pattern.pattern_text)
    if entropy < 3.0:
        return False

    return True
```

**Promotion flow (Phase 2):**
```
1. Scan live_patterns_metadata for promotable patterns (query every 5 min)
2. For each qualified pattern:
   a. Insert into Tier 2 persistent store (sqlite-vec with different path)
   b. Register with mem0 for fact extraction
   c. Mark pattern.status = 'promoted' in live_patterns_metadata
3. Cleanup old patterns (>2 hours) from live_state.db
```

---

## Steward Working Set (In-Memory)

**Not persistent.** Rebuilt on each compaction hook.

```typescript
interface WorkingSet {
  hot: CompactionCandidate[];      // Last 2-3 turns
  warm: CompactionCandidate[];     // Last 4-8 turns (prunable)
  cold: CompactionCandidate[];     // Older (only summarized)
  max_tokens: number;              // Determined by token budget model
}

interface CompactionCandidate {
  source_agent: string;
  pattern_id: string;
  confidence: number;
  text: string;
  age_minutes: number;
  relevance_score: number;  // 0-1, based on file overlap
}
```

**Construction (on compaction hook):**
```typescript
function buildWorkingSet(context_usage_pct: number): WorkingSet {
  const patterns = queryLivePatterns().sortByRelevance();

  const ws: WorkingSet = { hot: [], warm: [], cold: [], max_tokens: 120 };

  if (context_usage_pct < 0.70) {
    ws.max_tokens = 120;
    ws.hot = patterns.slice(0, 3);
    ws.warm = patterns.slice(3, 8);
  } else if (context_usage_pct < 0.90) {
    ws.max_tokens = 60;
    ws.hot = patterns.slice(0, 2);
    ws.warm = [];  // Dropped
  } else {
    ws.max_tokens = 20;
    ws.hot = [];
    ws.cold = [summarizePatterns(patterns)];
  }

  return ws;
}
```

---

## Indexing Strategy

**Queries most frequently on:**
1. Find patterns by source_agent (phase 1.5)
2. Find patterns by last_updated (stale cleanup)
3. Find injections by target_agent (rate limiting)
4. Find recent injections by pattern_id (runaway check)

```sql
CREATE INDEX idx_live_patterns_source_agent
  ON live_patterns_metadata(source_agent);

CREATE INDEX idx_live_patterns_last_updated
  ON live_patterns_metadata(last_updated DESC);

CREATE INDEX idx_injection_log_target_and_timestamp
  ON injection_log(target_agent, timestamp DESC);

CREATE INDEX idx_injection_log_pattern_and_timestamp
  ON injection_log(pattern_id, timestamp DESC);
```

---

## Storage Estimates

**Live session (8 hours, 6 agents):**

```
active_agents: 6 rows × 500 bytes = 3 KB
live_patterns: 500 patterns × (384 * 4 bytes + 200 bytes metadata) = 804 KB
pattern_metadata: 500 rows × 300 bytes = 150 KB
injection_log: 2000 injections × 250 bytes = 500 KB

Total: ≈ 1.5 MB per live_state.db
```

**On disk (with indices + overhead): ≈ 5-8 MB**

**RAM (Steward process): ≈ 25 MB** (working set buffer + sqlite in-memory cache)

---

## Maintenance

### Per-Session
- Flush injection_log to `~/.impulse/archives/live_session_{timestamp}.jsonl` on shutdown
- Archive live_state.db to `~/.impulse/archives/live_state_{timestamp}.db`
- Cleanup stale patterns (>2 hours old)

### Per-Project
- Promote patterns meeting criteria (Phase 2) to persistent Tier 2 store
- Delete promotional records from live_state.db after successful promotion

### Administrative
```bash
# Inspect live_state.db
sqlite3 ~/.impulse/live_state.db "SELECT * FROM active_agents;"

# Export injection log as JSON
sqlite3 ~/.impulse/live_state.db \
  ".mode json" \
  "SELECT * FROM injection_log ORDER BY timestamp DESC LIMIT 100;"

# Vacuum to reclaim space
sqlite3 ~/.impulse/live_state.db "VACUUM;"
```

---

## Cross-References

| Document | Purpose |
|----------|---------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Three state domains, promotion flow |
| [STEWARD.md](STEWARD.md) | Config schema (database path, cleanup intervals) |
| [SPEC-v1.1.md](SPEC-v1.1.md) | Phase 1.5 acceptance criteria for DB |
| [BENCHMARKS.md](BENCHMARKS.md) | Query performance tests |
