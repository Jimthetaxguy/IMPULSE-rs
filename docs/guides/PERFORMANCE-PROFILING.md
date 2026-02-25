---
status: active
phase: 1-2
audience: builder
tags: [guide, performance, profiling]
last_updated: 2026-02-20
---

# Performance Profiling & Optimization Guide

> **Version:** 1.0 | **Status:** Framework | **Updated:** 2026-02-20

---

## Overview

This guide covers performance profiling, baseline measurement, and optimization strategies for Impulse.

**Phase 1 Key Metrics (Claude Code hooks):**
- SessionStart latency: < 30ms (file reads + stdout format)
- PostToolUse latency: < 100ms (JSON parse + LIVE_STATE update)
- SessionEnd latency: < 10s (JSONL parse + 1 LLM call + file writes)
- PreCompact latency: < 100ms (file read + format + stdout)
- Memory per hook: < 50MB peak (Bun process lifecycle)

**Phase 1.5+ Metrics (pattern detection — not yet implemented):**
- Event processing latency: < 50ms
- Vector search: < 200ms for 10k vectors
- Pattern detection: < 500ms
- Memory usage: < 20MB baseline (long-running daemon)

See [EFFICIENCY-ANALYSIS.md](../research/EFFICIENCY-ANALYSIS.md) for validated implementation patterns.

---

## Baseline Measurements

### Event Processing Pipeline

```
Event received (message.updated)
    ↓ [<10ms] - Parse + validate
    ↓ [<20ms] - Store to DB
    ↓ [<10ms] - Query recent events
    ↓ [<500ms] - Pattern detection (if needed)
    ↓ [<5ms] - Update metrics
    ↓ [<100ms] - LIVE.md update (debounced)
───────────────────
Total: <750ms (typical)
Target: <1s end-to-end
```

### Database Operations

| Operation | Target | Notes |
|-----------|--------|-------|
| Event insert | <50ms | Uses prepared statements |
| Event query (8 recent) | <10ms | Indexed by agent_id + created_at |
| Vector insert | <50ms | Float32Array, virtual table |
| Vector search (10k) | <200ms | Cosine distance, partitioned |
| Pattern query | <100ms | Indexed by created_at |
| Cleanup batch (1000) | <500ms | Background job |

### Memory Baseline

```
Startup: 5-8 MB
After 1000 events: 12-15 MB
Peak with 10k vectors: 18-22 MB
Target steady state: <20 MB
```

**Components:**
- Process overhead: 5 MB
- Events cache (in-memory): 2-3 MB
- Vector cache: 12 MB (10k * 384 * 4 bytes)
- LIVE.md buffer: 0.5 MB
- Metadata: <1 MB

### CPU Baseline

```
Idle: <1% CPU
Processing events (1 per second): 2-3% CPU
Pattern detection active: 3-5% CPU
Cleanup job (5 min duration): 4-6% CPU
```

---

## Profiling Tools & Techniques

### 1. Node.js Built-in Profiler

```typescript
// Start profiling
const { performance, PerformanceObserver } = require('perf_hooks');

const obs = new PerformanceObserver((items) => {
  items.getEntries().forEach((entry) => {
    console.log(`${entry.name}: ${entry.duration.toFixed(2)}ms`);
  });
});

obs.observe({ entryTypes: ['measure'] });

// Measure operation
performance.mark('event-storage-start');
await db.storeEvent(event);
performance.mark('event-storage-end');
performance.measure('event-storage', 'event-storage-start', 'event-storage-end');
```

### 2. Memory Profiling

```typescript
// Heap snapshot
function heapSnapshot() {
  const v8 = require('v8');
  const fs = require('fs');
  const filename = `/tmp/heap-${Date.now()}.heapsnapshot`;
  v8.writeHeapSnapshot(filename);
  console.log(`Heap snapshot: ${filename}`);
}

// Memory usage tracking
function memoryProfile() {
  const used = process.memoryUsage();
  return {
    heapUsed: `${Math.round(used.heapUsed / 1024 / 1024)}MB`,
    heapTotal: `${Math.round(used.heapTotal / 1024 / 1024)}MB`,
    external: `${Math.round(used.external / 1024 / 1024)}MB`,
    rss: `${Math.round(used.rss / 1024 / 1024)}MB`,
  };
}

setInterval(() => {
  logger.info('Memory profile', memoryProfile());
}, 60000);
```

### 3. Database Query Analysis

```sql
-- Enable query profiling
PRAGMA optimize;
PRAGMA query_only = OFF;

-- Analyze query plan
EXPLAIN QUERY PLAN
SELECT * FROM events
WHERE agent_id = ?
  AND created_at > ?
ORDER BY created_at DESC
LIMIT 8;

-- Check index efficiency
PRAGMA index_info(idx_events_agent);
```

### 4. Latency Tracking

```typescript
const latencyHistogram = {
  buckets: [10, 50, 100, 200, 500, 1000],
  counts: [0, 0, 0, 0, 0, 0],
};

function recordLatency(durationMs: number) {
  for (let i = 0; i < latencyHistogram.buckets.length; i++) {
    if (durationMs <= latencyHistogram.buckets[i]) {
      latencyHistogram.counts[i]++;
      break;
    }
  }
}

function reportLatency() {
  logger.info('Latency distribution', {
    under10ms: latencyHistogram.counts[0],
    under50ms: latencyHistogram.counts[1],
    under100ms: latencyHistogram.counts[2],
    under500ms: latencyHistogram.counts[3],
    under1s: latencyHistogram.counts[4],
    over1s: latencyHistogram.counts[5],
  });
}
```

---

## Optimization Strategies

### 1. Database Optimization

**Connection Settings:**
```typescript
db.pragma('journal_mode = WAL');  // Better concurrency
db.pragma('synchronous = NORMAL'); // Faster writes
db.pragma('mmap_size = 30000000'); // Memory-mapped I/O
db.pragma('cache_size = -64000');  // 64MB cache
```

**Batch Operations:**
```typescript
// ✅ GOOD: Batch insert
db.transaction(() => {
  for (const event of events) {
    insertStmt.run(event.id, event.type, event.data);
  }
})();

// ❌ BAD: Individual inserts
for (const event of events) {
  db.run('INSERT INTO events ...');
}
```

**Index Strategy:**
```sql
-- Hot path (frequent queries)
CREATE INDEX idx_events_agent ON events(agent_id);
CREATE INDEX idx_events_created ON events(created_at);

-- Warm path (pattern queries)
CREATE INDEX idx_vectors_partition ON vectors(partition);

-- Cold path (cleanup)
CREATE INDEX idx_events_expires ON events(expires_at);
```

### 2. Memory Optimization

**Use Appropriate Data Structures:**
```typescript
// ✅ Float32Array (4 bytes per value)
const vector = new Float32Array(384);
// Memory: 384 * 4 = 1,536 bytes

// ❌ Array<number> (8 bytes per value)
const vector = Array(384).fill(0);
// Memory: 384 * 8 = 3,072 bytes (2x overhead)
```

**Cache Management:**
```typescript
// LRU cache for embeddings
const embeddingCache = new Map<string, Float32Array>();
const MAX_CACHE_SIZE = 1000;

function cacheEmbedding(key: string, vector: Float32Array) {
  if (embeddingCache.size >= MAX_CACHE_SIZE) {
    // Remove oldest entry
    const firstKey = embeddingCache.keys().next().value;
    embeddingCache.delete(firstKey);
  }
  embeddingCache.set(key, vector);
}
```

### 3. Pattern Detection Optimization

**Early Exit:**
```typescript
// Stop searching once similarity > 0.88
async detectWithEarlyExit(event: MessageEvent): Promise<Pattern[]> {
  const sourceVector = await this.embedContext(event);
  const patterns: Pattern[] = [];

  for (const otherVector of otherVectors) {
    const similarity = this.cosineSimilarity(sourceVector, otherVector.vector);

    if (similarity > this.threshold) {
      patterns.push(this.extractPattern(sourceVector, otherVector, similarity));
      // Early exit after first match (optional)
      if (patterns.length >= 3) break;
    }
  }

  return patterns;
}
```

**Vector Caching:**
```typescript
// Cache recent embeddings to avoid re-computation
const vectorCache = new Map<string, Float32Array>();

async getOrEmbedContext(agentId: string, events: Event[]): Promise<Float32Array> {
  const key = `${agentId}-${events[0].timestamp}`;

  if (vectorCache.has(key)) {
    return vectorCache.get(key)!;
  }

  const vector = await this.embedContext(events);
  vectorCache.set(key, vector);
  return vector;
}
```

---

## Benchmarking Framework

### Benchmark Template

```typescript
import { performance } from 'perf_hooks';

class Benchmark {
  private measurements: number[] = [];
  private name: string;

  constructor(name: string) {
    this.name = name;
  }

  start(): () => number {
    const start = performance.now();
    return () => {
      const duration = performance.now() - start;
      this.measurements.push(duration);
      return duration;
    };
  }

  report(): BenchmarkReport {
    const sorted = this.measurements.sort((a, b) => a - b);
    return {
      name: this.name,
      count: this.measurements.length,
      min: sorted[0],
      max: sorted[sorted.length - 1],
      mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
      p50: sorted[Math.floor(sorted.length * 0.5)],
      p95: sorted[Math.floor(sorted.length * 0.95)],
      p99: sorted[Math.floor(sorted.length * 0.99)],
    };
  }
}

// Usage
const benchmark = new Benchmark('Pattern Detection');
for (let i = 0; i < 100; i++) {
  const end = benchmark.start();
  await detector.detect(event);
  end();
}
console.log(benchmark.report());
```

### Baseline Measurement Test

```typescript
it('should meet performance baselines', async () => {
  const eventBench = new Benchmark('Event Storage');
  for (let i = 0; i < 100; i++) {
    const end = eventBench.start();
    await db.storeEvent(createMessageEvent());
    end();
  }

  const report = eventBench.report();
  expect(report.p95).toBeLessThan(50); // 95th percentile <50ms
  expect(report.p99).toBeLessThan(100); // 99th percentile <100ms

  logger.info('Event storage benchmark', report);
});
```

---

## Continuous Profiling

### Production Monitoring

```typescript
// Collect metrics continuously
class PerformanceMonitor {
  async start(): Promise<void> {
    setInterval(() => {
      const metrics = {
        timestamp: Date.now(),
        memory: process.memoryUsage(),
        uptime: process.uptime(),
        eventCount: this.metrics.eventsProcessed,
        errorRate: this.metrics.errorCount / this.metrics.eventsProcessed,
        avgLatency: this.metrics.avgLatencyMs,
      };

      logger.info('Performance snapshot', metrics);

      // Alert on threshold violations
      if (metrics.memory.heapUsed > 25 * 1024 * 1024) {
        logger.warn('High memory usage', { heap: metrics.memory.heapUsed });
      }
    }, 60000); // Every minute
  }
}
```

---

## Optimization Checklist

- [ ] Database indexes created (7 total)
- [ ] Connection settings tuned (WAL, synchronous, cache)
- [ ] Memory baseline measured (<20MB target)
- [ ] Latency baseline measured (<750ms e2e)
- [ ] CPU baseline measured (<5% peak)
- [ ] Vector caching implemented
- [ ] Batch operations used
- [ ] Float32Array for vectors
- [ ] Query plans verified (EXPLAIN QUERY PLAN)
- [ ] Monitoring dashboard created
- [ ] Alerts configured
- [ ] Profiling tools available (heap snapshots, perf traces)

---

## References

- Node.js perf_hooks: https://nodejs.org/api/perf_hooks.html
- SQLite PRAGMA: https://www.sqlite.org/pragma.html
- Benchmarking best practices: `docs/guides/BEST-PRACTICES.md`

---

_Created: 2026-02-20 | Status: Framework v1.0_
