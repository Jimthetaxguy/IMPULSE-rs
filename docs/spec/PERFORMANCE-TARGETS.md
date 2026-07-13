---
status: superseded
phase: 1-2
audience: builder
tags: [spec, performance, benchmarks]
last_updated: 2026-02-20
---

# Performance Targets: Impulse Plugin

> **Historical reference — superseded.** These hook/plugin budgets do not define the current Rust
> control plane. Use [`RUST-CANONICAL-CONTRACT.md`](RUST-CANONICAL-CONTRACT.md) for implemented
> boundaries; new control-plane budgets require measured baselines and an explicit contract update.

> **Version:** 1.0 | **Updated:** 2026-02-20
> **Principle:** The plugin must be invisible. Any latency the developer can feel is a bug.

---

## Hook Performance Budgets

| Hook | Budget | What It Does | Main Cost |
|------|--------|-------------|-----------|
| `session.start` | < 200ms | Read 3 files + inject prompt | File I/O (3 reads) |
| `tool.execute.after` | < 50ms | Parse JSON + write JSON | File I/O (1 read + 1 write) |
| `session.end` | < 5s | LLM call + write 2 files | LLM latency (dominant) |
| `session.compacting` | < 100ms | Read 1 file + inject | File I/O (1 read) |

### Why These Budgets

- **session.start (200ms):** Runs once. Reads 3 small files (<50KB total). File I/O on SSD is <1ms per file. JSON parse is <1ms. Most of the budget is safety margin.

- **tool.execute.after (50ms):** Runs on EVERY tool call. Must be imperceptible. JSON parse + serialize + atomic write is ~5-10ms. The remaining budget handles disk flush latency.

- **session.end (5s):** Runs once at exit. The LLM extraction call dominates (2-4s typical). File writes add <50ms. User doesn't notice because they're already leaving.

- **session.compacting (100ms):** Runs during compaction (already a slow operation). Read 1 file + string slice is <5ms. Budget is generous.

---

## File Size Limits

| File | Healthy | Warning | Danger | Action at Danger |
|------|---------|---------|--------|-----------------|
| GENOME.md | < 200 lines (~5KB) | 200-500 lines | > 500 lines | Archive old decisions |
| LIVE_STATE.json | < 5KB | 5-20KB | > 20KB | Clean stale agents |
| HISTORY_INDEX.md | < 100 sessions (~25KB) | 100-500 | > 500 | Migrate to FTS5 (Phase 2) |

### Memory Budget

| Component | Budget |
|-----------|--------|
| Plugin runtime | < 20MB |
| File read buffers | < 1MB |
| JSON parse overhead | < 500KB |
| Total | < 25MB |

---

## Benchmark Test Script

```typescript
// benchmarks/hook-timing.ts
import { performance } from 'perf_hooks';

async function benchmarkHook(name: string, fn: () => Promise<void>, iterations: number = 100) {
  const times: number[] = [];

  for (let i = 0; i < iterations; i++) {
    const start = performance.now();
    await fn();
    times.push(performance.now() - start);
  }

  times.sort((a, b) => a - b);
  const p50 = times[Math.floor(times.length * 0.5)]!;
  const p95 = times[Math.floor(times.length * 0.95)]!;
  const p99 = times[Math.floor(times.length * 0.99)]!;
  const avg = times.reduce((a, b) => a + b, 0) / times.length;

  console.log(`${name}: avg=${avg.toFixed(1)}ms p50=${p50.toFixed(1)}ms p95=${p95.toFixed(1)}ms p99=${p99.toFixed(1)}ms`);

  return { avg, p50, p95, p99 };
}
```

### Expected Results (M1 Mac, NVMe SSD)

| Hook | p50 | p95 | p99 |
|------|-----|-----|-----|
| session.start (3 files exist) | ~3ms | ~8ms | ~15ms |
| session.start (no files) | ~1ms | ~3ms | ~5ms |
| tool.execute.after | ~5ms | ~12ms | ~20ms |
| session.compacting | ~2ms | ~5ms | ~10ms |
| session.end (mock LLM) | ~10ms | ~20ms | ~35ms |
| session.end (real LLM) | ~2500ms | ~4000ms | ~5000ms |

---

## Monitoring

The plugin logs timing data to stderr with `[impulse]` prefix:

```
[impulse] Loaded 45 lines of project memory (3ms)
[impulse] 2 active agent(s) (1ms)
[impulse] Injected ~850 tokens of context (total: 5ms)
[impulse] Updated LIVE_STATE: src/auth.ts locked by agent-1 (8ms)
[impulse] Saved 3 new decisions to GENOME.md (12ms)
[impulse] Injected 50 lines for compaction survival (2ms)
```

---

_Created: 2026-02-20 | Status: Targets Defined v1.0_
