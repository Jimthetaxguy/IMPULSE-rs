---
status: superseded
phase: 1.5
audience: builder
tags: [phase, coordination, multi-agent]
last_updated: 2026-02-20
---

# Phase 1.5: Live Coordination Design

> **⚠️ Superseded for implementation authority.**
> This document is a TypeScript-era design study for fabricated OpenCode plugin interfaces.
> Keep as reference only. Active implementation contract is [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md).
>
> **Version:** 1.0 | **Status:** Design Study | **Updated:** 2026-02-21
> **Duration:** 2-3 weeks | **Dependencies:** Phase 1 complete
> ⚠️ **DESIGN STUDY — NOT IMPLEMENTATION PLAN.** This document describes a coordination model
> using vector similarity and real-time injection. This design was conceived for OpenCode's
> plugin SDK (which turned out to have fabricated interfaces). Phase 1.5 will need to be
> re-evaluated for the Claude Code hooks architecture before implementation.
>
> **Phase 1 already covers basic multi-agent awareness** via `LIVE_STATE.json` shared state.
> Phase 1.5 would add semantic overlap detection — but the mechanism needs new design.
> See [ADR-0001](../decisions/0001-claude-code-primary.md) for architectural context.

---

## Overview

Phase 1.5 upgrades basic LIVE_STATE.json awareness to semantic agent coordination: detecting when agents are working on overlapping topics and suggesting coordination.

**Core Goal:** Agents become **semantically aware of each other's work** based on detected content overlaps.

---

## Architecture: Pattern Detection → Injection

```
Event received (agent-1)
    ↓
Embed context (last 8 turns)
    ↓
Query similar vectors (agent-2, agent-3)
    ↓
Pattern detected (similarity > 0.88)
    ↓
Apply safeguards:
  ├── Anti-echo (skip if [SWARM: prefix)
  ├── Rate limit (1 per 45s)
  ├── Confidence decay (λ=0.03)
  ├── File scope (same file only)
  ├── Runaway check (>4 agents in <3 min)
  └── Token budget response
  ↓
Format injection: "[SWARM:agent:confidence] shared context"
    ↓
Queue for compaction hook
    ↓
Inject into agent-2's context (max 120 tokens)
    ↓
Agent autonomously uses injection
```

---

## Key Components to Implement

### 1. Embedding Caching

**Problem:** Embedding same context 6 times = 6x LLM API calls

**Solution:** Cache embeddings with LRU eviction

```typescript
class EmbeddingCache {
  private cache = new Map<string, Float32Array>();
  private readonly MAX_SIZE = 1000;

  async getOrEmbed(
    key: string,
    context: string,
  ): Promise<Float32Array> {
    if (this.cache.has(key)) {
      return this.cache.get(key)!;
    }

    const vector = await this.embedModel.embed(context);
    this.evictOldest();
    this.cache.set(key, vector);
    return vector;
  }

  private evictOldest(): void {
    if (this.cache.size >= this.MAX_SIZE) {
      const firstKey = this.cache.keys().next().value;
      this.cache.delete(firstKey);
    }
  }
}
```

**Impact:** Reduce embedding calls by ~80%

### 2. Runaway Propagation Check

**Problem:** Pattern → injection → new pattern → injection (feedback loop)

**Solution:** Track pattern cascade across agents + time window

```typescript
class RunawayDetector {
  private cascades = new Map<string, CascadeState>();

  async checkRunaway(pattern: Pattern): Promise<boolean> {
    const key = pattern.extractedTopic;
    const state = this.cascades.get(key) || { count: 0, firstSeen: Date.now() };

    // Count agents in last 3 minutes
    const windowMs = 3 * 60 * 1000;
    if (Date.now() - state.firstSeen > windowMs) {
      state.count = 1;
      state.firstSeen = Date.now();
    } else {
      state.count++;
    }

    this.cascades.set(key, state);

    // Alert if >4 agents on same pattern in <3 min
    if (state.count > 4) {
      logger.warn('Runaway propagation detected', {
        topic: key,
        agents: state.count,
      });
      return true; // Block injection
    }

    return false;
  }
}
```

**Impact:** Prevent echo cascades >2 hops

### 3. File-Scoped Injection Refinement

**Problem:** Inject into agent working on unrelated file = noise

**Solution:** Extract file context from tool args + match partitions

```typescript
function extractFileScope(event: ToolExecuteEvent): string[] {
  const files = new Set<string>();

  // Extract from tool args
  if (event.toolArgs.filePath) {
    files.add(event.toolArgs.filePath);
  }
  if (event.toolArgs.files) {
    for (const file of event.toolArgs.files) {
      files.add(file);
    }
  }

  // Extract from tool name patterns
  if (event.toolName.includes('edit')) {
    // Assume editing same file from context
  }

  return Array.from(files);
}

// Inject only if target agent on related files
function isFileScopedMatch(
  sourceFiles: string[],
  targetFiles: string[],
): boolean {
  // Exact match
  if (sourceFiles.some((f) => targetFiles.includes(f))) {
    return true;
  }

  // Directory match (e.g., src/auth.ts matches src/auth-test.ts)
  const sourceDir = sourceFiles[0]?.split('/').slice(0, -1).join('/');
  const targetDir = targetFiles[0]?.split('/').slice(0, -1).join('/');
  return sourceDir === targetDir && sourceDir;
}
```

**Impact:** Reduce noise, improve signal-to-noise ratio

### 4. Token Budget Response (70% / 90%)

**Problem:** If context is full, don't send 120-token injection

**Solution:** Detect context usage % and adjust response

```typescript
async handleCompactionEvent(event: CompactionEvent): Promise<void> {
  const usage = event.currentContextTokens / event.maxContextTokens;

  let injection: string;

  if (usage < 0.70) {
    // Normal: Full injection (120 tokens)
    injection = await this.detector.detect(event);
  } else if (usage < 0.90) {
    // Compressed: Key insights only (60 tokens)
    injection = await this.compressInjection(
      await this.detector.detect(event),
      60,
    );
  } else {
    // Micro-summary: 3 sentences (30 tokens)
    injection = await this.summarizeInjection(
      await this.detector.detect(event),
      3,
    );
  }

  await this.sendInjection(event.agentId, injection);
}
```

**Impact:** Respect context limits, prevent failure

### 5. Confidence Scoring Refinement

**Problem:** High confidence for old patterns = noise

**Solution:** Apply decay + freshness bonus

```typescript
function calculateFinalConfidence(
  baseSimilarity: number,
  minutesSinceSeen: number,
  recencyBonus: number = 0.1,
): number {
  // Base: similarity score
  let confidence = baseSimilarity;

  // Apply decay
  confidence *= Math.exp(-0.03 * minutesSinceSeen);

  // Recency bonus (recent patterns slightly boosted)
  if (minutesSinceSeen < 5) {
    confidence = Math.min(1.0, confidence + recencyBonus);
  }

  return confidence;
}
```

**Impact:** Balance freshness + decay

---

## Safeguard Matrix

| Safeguard | Trigger | Action | Examples |
|-----------|---------|--------|----------|
| **Anti-Echo** | `[SWARM:` prefix | Skip pattern | Injection from agent-2 should not trigger again |
| **Rate Limit** | <45s since last | Block injection | Agent-1 max 1 injection per 45s |
| **Confidence Decay** | >23 min old | Reduce confidence | Pattern from 1 hour ago: ~1.5% confidence |
| **File Scope** | Different file | Block or demote | Auth module pattern ignored by API module |
| **Runaway Check** | >4 agents in <3 min | Block + alert | Pattern cascading across 5+ agents |
| **Token Budget** | >70% context used | Compress injection | Shrink from 120 → 60 → 30 tokens |
| **Entropy Check** | Low information | Block pattern | Generic "working on file X" blocked |
| **Duplicate Prevention** | Same pattern twice | Merge + deduplicate | Same topic from agent-1 and agent-2 → 1 injection |

**Total Combinations:** 36+ (all tested in Phase 1.5)

---

## Testing Strategy

### Unit Tests (Per Safeguard)

```typescript
describe('Runaway Propagation Check', () => {
  it('should allow pattern with <4 agents in 3 min', async () => {
    const detector = new RunawayDetector();
    expect(await detector.checkRunaway(pattern1)).toBe(false);
    expect(await detector.checkRunaway(pattern1)).toBe(false); // 2 agents
    expect(await detector.checkRunaway(pattern1)).toBe(false); // 3 agents
    expect(await detector.checkRunaway(pattern1)).toBe(false); // 4 agents
  });

  it('should block pattern with >4 agents in 3 min', async () => {
    const detector = new RunawayDetector();
    for (let i = 0; i < 4; i++) {
      expect(await detector.checkRunaway(pattern)).toBe(false);
    }
    expect(await detector.checkRunaway(pattern)).toBe(true); // 5th agent blocked
  });

  it('should reset counter after 3 minutes', async () => {
    const detector = new RunawayDetector();
    // Add 4 patterns
    // Fast-forward 4 minutes
    // Add 5th pattern should NOT be blocked (new window)
  });
});
```

### Integration Tests (6-Agent Scenario)

```typescript
describe('6-Agent Coordination', () => {
  it('should detect overlap without echo cascades >2 hops', async () => {
    const agents = createAgents(6);
    const results = await runCoordinationTest(agents, {
      eventCount: 50,
      durationMs: 60000,
    });

    expect(results.patternsDetected).toBeGreaterThan(0);
    expect(results.echoesDetected).toBe(0);
    expect(results.cascadesBlocked).toBeGreaterThan(0);
    expect(results.avgLatency).toBeLessThan(500);
  });
});
```

---

## Performance Targets (Phase 1.5)

| Operation | Target | Measurement |
|-----------|--------|-------------|
| Pattern detection | <500ms | End-to-end |
| Embedding cache hit | <10ms | Cached lookup |
| Runaway check | <50ms | Algorithm |
| File scope match | <20ms | String operations |
| Token budget calc | <5ms | Simple math |
| Injection format | <10ms | String building |
| **Memory overhead** | +5MB | Cache + state |

---

## Deployment Steps

### 1. Deploy Embedding Cache
- [ ] Implement EmbeddingCache class
- [ ] Add LRU eviction
- [ ] Test cache hit ratio

### 2. Deploy Runaway Detector
- [ ] Implement RunawayDetector class
- [ ] Test with 6-agent scenario
- [ ] Monitor false positive rate

### 3. Deploy File Scope Matching
- [ ] Extract file paths from tool args
- [ ] Implement matching algorithm
- [ ] Test accuracy

### 4. Deploy Token Budget Response
- [ ] Detect context usage in compaction hook
- [ ] Implement 3-tier response (120/60/30 tokens)
- [ ] Test with varying context usage

### 5. Comprehensive Testing
- [ ] 6-agent coordination test
- [ ] Echo cascade prevention test
- [ ] Performance baseline verification
- [ ] Stress testing (1000+ events)

---

## Success Metrics

| Metric | Target | Method |
|--------|--------|--------|
| **Echo Prevention** | 0 cascades >2 hops | 6-agent test |
| **Coordination Quality** | 5+ patterns detected | Integration test |
| **False Positives** | <5% | Metric tracking |
| **Latency** | <500ms pattern detection | Performance test |
| **Memory** | +5MB (cache) | Memory profile |
| **File Scope Accuracy** | >95% | Integration test |

---

## Open Questions

1. **Embedding Cache Strategy:** TTL-based vs LRU? (Current: LRU max 1000)
2. **Runaway Window:** 3 minutes optimal? (Could be 5 or 10)
3. **File Scope Heuristics:** How to extract files from all tool types?
4. **Confidence Bonus:** 0.1 recency bonus enough? (Could be 0.05-0.2)

---

## References

- Phase 1 Foundation: `docs/archive/ARCHITECTURE.md`
- Pattern Detection: `docs/phases/IMPLEMENTATION-ROADMAP.md` (Priority 3)
- Testing Framework: `docs/guides/TESTING-FRAMEWORK.md`

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Phase 1.5 Implementation_
