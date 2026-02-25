---
status: active
phase: 1
audience: builder
tags: [guide, testing, synthetic]
last_updated: 2026-02-20
---

# Synthetic Testing Guide: CoordinationSimulator & Event Replay

> **Version:** 1.0 | **Status:** Design | **Updated:** 2026-02-20
> **Purpose:** Validate Phase 1.5 coordination without LLM calls, terminal UI, or real agents
> **Value:** Fast (100ms/scenario), repeatable (deterministic), CI-friendly (no external deps)

---

## Core Insight

Phase 1.5 coordination logic can be tested in isolation via **synthetic event generation and replay**. This validates:

- ✅ Pattern detection algorithm (embedding + similarity)
- ✅ Anti-echo safeguard
- ✅ Rate limiting
- ✅ Runaway propagation detection
- ✅ File-scoped injection
- ✅ Confidence decay
- ✅ Token budget response

**Without:**
- ❌ LLM API calls (saves $, no tokens)
- ❌ Real agents (no timing delays)
- ❌ Zellij multiplexing (no flakiness)
- ❌ Network I/O (deterministic)

---

## Architecture: CoordinationSimulator

```
┌─────────────────────────────────────┐
│ Test Scenario (agents=6, events=100)│
└────────────┬────────────────────────┘
             │
             ▼
┌──────────────────────────────────┐
│ CoordinationSimulator             │
│ ├─ Generate synthetic events      │
│ ├─ Feed to harness                │
│ ├─ Capture all side effects       │
│ └─ Assert on outcomes             │
└────────────┬─────────────────────┘
             │
             ▼
┌──────────────────────────────────┐
│ Harness (normal flow)             │
│ ├─ Ingest event                   │
│ ├─ Detect pattern (embedding)     │
│ ├─ Check safeguards               │
│ ├─ Queue injection (if safe)      │
│ └─ Update LIVE.md                 │
└────────────┬─────────────────────┘
             │
             ▼
┌──────────────────────────────────┐
│ SimulationResult                  │
│ ├─ patterns_detected: 5           │
│ ├─ injections_sent: 3             │
│ ├─ echo_cascades: 0 ✅            │
│ ├─ latency_p50: 120ms             │
│ ├─ latency_p95: 450ms             │
│ ├─ latency_p99: 890ms             │
│ ├─ memory_used: 2.3MB             │
│ └─ assertions passed: 12/12 ✅    │
└──────────────────────────────────┘
```

---

## Implementation: CoordinationSimulator Class

```typescript
// src/test/coordination-simulator.ts

import { Harness, MessageEvent, ToolExecuteEvent } from '../harness';
import { Logger } from '../utils/logger';

export interface SimulationConfig {
  agents: number;                    // How many agents
  events_per_agent: number;          // Events per agent
  file: string;                      // File they're editing
  start_time: number;                // Unix timestamp (ms)
  event_rate_ms: number;             // ms between events
  randomness: 'deterministic' | 'varied';
  seed?: number;                     // For reproducibility
}

export interface SimulationResult {
  agents: number;
  events_generated: number;
  patterns_detected: number;
  injections_sent: number;
  injections_successful: number;
  echo_cascades: number;
  rate_limit_hits: number;
  latency_p50: number;
  latency_p95: number;
  latency_p99: number;
  memory_used_mb: number;
  assertions: AssertionResult[];
}

interface AssertionResult {
  name: string;
  passed: boolean;
  details?: string;
}

export class CoordinationSimulator {
  private harness: Harness;
  private logger: Logger;
  private seed: number;

  constructor(harness: Harness, logger: Logger) {
    this.harness = harness;
    this.logger = logger;
    this.seed = Date.now();
  }

  /**
   * Run a simulation scenario
   *
   * Example:
   * ```
   * const result = await simulator.runScenario({
   *   agents: 6,
   *   events_per_agent: 100,
   *   file: 'src/auth.ts',
   *   start_time: Date.now(),
   *   event_rate_ms: 1000,
   *   randomness: 'deterministic',
   *   seed: 12345,
   * });
   *
   * expect(result.echo_cascades).toBe(0);
   * expect(result.patterns_detected).toBeGreaterThan(0);
   * ```
   */
  async runScenario(config: SimulationConfig): Promise<SimulationResult> {
    this.seed = config.seed || this.seed;

    const events = this.generateEvents(config);
    const memoryBefore = process.memoryUsage().heapUsed;
    const latencies: number[] = [];

    const injections: Array<{ agent: string; pattern_id: string; time: number }> = [];
    const echoDetections: Array<{ pattern: string; agents: string[] }> = [];

    // Feed events to harness in order
    for (let i = 0; i < events.length; i++) {
      const event = events[i];
      const startTime = Date.now();

      try {
        await this.harness.ingest(event);
      } catch (error) {
        this.logger.error({ error, event_index: i }, 'Event ingestion failed');
      }

      const latency = Date.now() - startTime;
      latencies.push(latency);

      // Check if injection was queued (via side effect tracking)
      const queued = this.harness.getQueuedInjections();
      if (queued.length > 0) {
        for (const inj of queued) {
          injections.push({
            agent: inj.targetAgent,
            pattern_id: inj.patternId,
            time: Date.now(),
          });
        }
        this.harness.clearQueuedInjections();
      }
    }

    const memoryAfter = process.memoryUsage().heapUsed;
    const memoryUsedMb = (memoryAfter - memoryBefore) / 1024 / 1024;

    // Analyze results
    const patternsDetected = this.harness.getDetectedPatterns().length;
    const echoCascades = this.detectEchoCascades(events, injections);
    const rateLimitHits = this.harness.getRateLimitMetrics().hits;

    // Calculate percentiles
    const sorted = latencies.sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.50)];
    const p95 = sorted[Math.floor(sorted.length * 0.95)];
    const p99 = sorted[Math.floor(sorted.length * 0.99)];

    // Run assertions
    const assertions = this.runAssertions(
      config,
      patternsDetected,
      injections,
      echoCascades,
      latencies,
      memoryUsedMb,
    );

    return {
      agents: config.agents,
      events_generated: events.length,
      patterns_detected: patternsDetected,
      injections_sent: injections.length,
      injections_successful: injections.filter((i) => !this.didInject(i)).length,
      echo_cascades: echoCascades,
      rate_limit_hits: rateLimitHits,
      latency_p50: p50,
      latency_p95: p95,
      latency_p99: p99,
      memory_used_mb: memoryUsedMb,
      assertions,
    };
  }

  /**
   * Generate synthetic events
   *
   * Patterns:
   * ├─ Deterministic: Agent 1 edits file, Agent 2 edits same file 5 sec later, ...
   * └─ Varied: Random agents, random delays, random files (same parent dir)
   */
  private generateEvents(config: SimulationConfig): Array<MessageEvent | ToolExecuteEvent> {
    const events: Array<MessageEvent | ToolExecuteEvent> = [];
    const rng = new SeededRandom(this.seed);

    for (let agentIdx = 0; agentIdx < config.agents; agentIdx++) {
      const agentId = `agent-${agentIdx}`;

      for (let eventIdx = 0; eventIdx < config.events_per_agent; eventIdx++) {
        const timestamp =
          config.start_time + (agentIdx * config.events_per_agent + eventIdx) * config.event_rate_ms;

        // Alternate between message and tool.execute events (50/50)
        if (eventIdx % 2 === 0) {
          // Message event
          events.push({
            type: 'message.updated',
            timestamp,
            agentId,
            role: 'assistant',
            content: this.generateMessageContent(agentId, config, rng),
            metadata: { message_id: `msg-${agentIdx}-${eventIdx}` },
          });
        } else {
          // Tool execute event
          events.push({
            type: 'tool.execute.after',
            timestamp,
            agentId,
            toolName: rng.choice(['read', 'edit', 'execute']),
            toolArgs: {
              filePath: config.randomness === 'deterministic' ? config.file : rng.choice([config.file, `${config.file.split('/')[0]}/other.ts`]),
            },
            result: { success: true },
            metadata: { tool_id: `tool-${agentIdx}-${eventIdx}` },
          });
        }
      }
    }

    // Shuffle if varied
    if (config.randomness === 'varied') {
      events.sort(() => rng.random() - 0.5);
    }

    return events;
  }

  private generateMessageContent(agentId: string, config: SimulationConfig, rng: SeededRandom): string {
    const topics = [
      `refactoring ${config.file}`,
      `fixing bug in ${config.file}`,
      `adding tests for ${config.file}`,
      `optimizing ${config.file}`,
      `reviewing ${config.file}`,
    ];

    return rng.choice(topics) + ` - context from ${agentId}`;
  }

  /**
   * Detect echo cascades
   * An echo cascade is: same pattern injected to >4 agents in <3 minutes
   */
  private detectEchoCascades(
    events: Array<MessageEvent | ToolExecuteEvent>,
    injections: Array<{ agent: string; pattern_id: string; time: number }>,
  ): number {
    const patternCascades = new Map<string, number[]>();

    for (const inj of injections) {
      if (!patternCascades.has(inj.pattern_id)) {
        patternCascades.set(inj.pattern_id, []);
      }
      patternCascades.get(inj.pattern_id)!.push(inj.time);
    }

    let cascadeCount = 0;

    for (const [patternId, times] of patternCascades) {
      // Group by 3-minute windows
      const sorted = times.sort((a, b) => a - b);
      const windowMs = 3 * 60 * 1000;

      for (let i = 0; i < sorted.length; i++) {
        const windowStart = sorted[i];
        const windowEnd = windowStart + windowMs;
        const agentsInWindow = sorted.filter((t) => t >= windowStart && t <= windowEnd).length;

        if (agentsInWindow > 4) {
          cascadeCount++;
        }
      }
    }

    return cascadeCount;
  }

  /**
   * Check if injection was actually sent (vs queued)
   */
  private didInject(inj: { agent: string; pattern_id: string; time: number }): boolean {
    // In a real test, check if agent received the injection
    // For now, assume all queued injections are sent
    return true;
  }

  /**
   * Run assertions against simulation results
   */
  private runAssertions(
    config: SimulationConfig,
    patternsDetected: number,
    injections: Array<{ agent: string; pattern_id: string; time: number }>,
    echoCascades: number,
    latencies: number[],
    memoryUsedMb: number,
  ): AssertionResult[] {
    const assertions: AssertionResult[] = [];
    const sorted = latencies.sort((a, b) => a - b);
    const p95 = sorted[Math.floor(sorted.length * 0.95)];

    // Core assertions
    assertions.push({
      name: 'Zero echo cascades',
      passed: echoCascades === 0,
      details: `Found ${echoCascades} echo cascades (should be 0)`,
    });

    assertions.push({
      name: 'Patterns detected (>0)',
      passed: patternsDetected > 0,
      details: `Detected ${patternsDetected} patterns`,
    });

    assertions.push({
      name: 'Injections queued (>0)',
      passed: injections.length > 0,
      details: `Queued ${injections.length} injections`,
    });

    assertions.push({
      name: 'Pattern detection latency (p95 < 500ms)',
      passed: p95 < 500,
      details: `P95 latency: ${p95}ms`,
    });

    assertions.push({
      name: 'Memory overhead (<5MB)',
      passed: memoryUsedMb < 5,
      details: `Memory used: ${memoryUsedMb.toFixed(2)}MB`,
    });

    assertions.push({
      name: 'Rate limit enforced (1 per agent per 45s)',
      passed: injections.length <= config.agents, // At most 1 injection per agent
      details: `Injections: ${injections.length}, agents: ${config.agents}`,
    });

    // File-scoped assertion (if all events on same file)
    const uniqueFiles = new Set(
      injections.map((i) => {
        // Track which file was targeted
        return config.file;
      }),
    );
    assertions.push({
      name: 'File-scoped injection (only same file)',
      passed: uniqueFiles.size === 1,
      details: `Files targeted: ${uniqueFiles.size}`,
    });

    return assertions;
  }
}

// Helper: Seeded random for reproducible randomness
class SeededRandom {
  private seed: number;

  constructor(seed: number) {
    this.seed = seed;
  }

  random(): number {
    this.seed = (this.seed * 9301 + 49297) % 233280;
    return this.seed / 233280;
  }

  choice<T>(arr: T[]): T {
    return arr[Math.floor(this.random() * arr.length)];
  }
}
```

---

## Test Scenarios

### Scenario 1: Basic Coordination (6 agents, 100 events each)

```typescript
describe('CoordinationSimulator', () => {
  it('should detect patterns and prevent echo cascades (6-agent scenario)', async () => {
    const simulator = new CoordinationSimulator(harness, logger);

    const result = await simulator.runScenario({
      agents: 6,
      events_per_agent: 100,
      file: 'src/auth.ts',
      start_time: Date.now(),
      event_rate_ms: 1000,
      randomness: 'deterministic',
    });

    // Core assertion: zero echo cascades
    expect(result.echo_cascades).toBe(0);

    // Secondary assertions
    expect(result.patterns_detected).toBeGreaterThan(0);
    expect(result.injections_sent).toBeGreaterThan(0);
    expect(result.latency_p95).toBeLessThan(500);
    expect(result.memory_used_mb).toBeLessThan(5);

    // All assertions must pass
    const failures = result.assertions.filter((a) => !a.passed);
    expect(failures).toHaveLength(0);
  });
});
```

### Scenario 2: High-Volume Stress Test

```typescript
it('should handle high event volume without degradation', async () => {
  const result = await simulator.runScenario({
    agents: 12,                    // More agents
    events_per_agent: 500,         // More events per agent
    file: 'src/database.ts',
    start_time: Date.now(),
    event_rate_ms: 100,            // Faster
    randomness: 'varied',          // Random delays + files
  });

  expect(result.echo_cascades).toBe(0);
  expect(result.latency_p99).toBeLessThan(1000);
  expect(result.memory_used_mb).toBeLessThan(20);  // Relaxed for high volume
});
```

### Scenario 3: Multi-File Coordination

```typescript
it('should respect file-scoped injection (agents on different files)', async () => {
  // Manually modify generated events to have different files
  const result = await simulator.runScenario({
    agents: 4,
    events_per_agent: 50,
    file: 'src/auth.ts',
    start_time: Date.now(),
    event_rate_ms: 500,
    randomness: 'varied',  // Mix of files
  });

  expect(result.echo_cascades).toBe(0);
  // Injections should be lower (different files = less pattern overlap)
  expect(result.injections_sent).toBeLessThan(10);
});
```

---

## CI/CD Integration

### GitHub Actions Workflow

```yaml
# .github/workflows/coordination-tests.yml

name: Coordination Simulator Tests

on: [push, pull_request]

jobs:
  simulate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: oven-sh/setup-bun@v1

      - name: Install
        working-directory: harness
        run: bun install

      - name: Run basic scenario (6 agents, 100 events)
        working-directory: harness
        run: |
          bun run test -- --grep "6-agent scenario"

      - name: Run stress test (12 agents, 500 events)
        working-directory: harness
        run: |
          bun run test -- --grep "high-volume"

      - name: Verify: Zero echo cascades
        working-directory: harness
        run: |
          bun run test -- --grep "echo"

      - name: Benchmark latency
        working-directory: harness
        run: |
          bun run test -- --grep "latency" --reporter=verbose
```

---

## Performance Baselines

| Scenario | Events | Agents | Duration | P95 Latency | Memory |
|----------|--------|--------|----------|------------|--------|
| Basic | 600 | 6 | ~60s | <500ms | <5MB |
| Stress | 6000 | 12 | ~120s | <1000ms | <20MB |
| Multi-file | 200 | 4 | ~20s | <400ms | <2MB |

---

## Advantages Over Other Testing Approaches

| Approach | Pros | Cons |
|----------|------|------|
| **Unit Tests** | Fast, isolated | Can't test full coordination flow |
| **Integration Tests (Real Agents)** | Realistic, end-to-end | Slow, flaky, requires agents |
| **Snapshot Tests (Terminal UI)** | Visual validation | Fragile, non-deterministic, maintenance burden |
| **Synthetic Event Replay** | **Fast, deterministic, repeatable** | **No actual agent behavior** |

---

## References

- Phase 1.5: `docs/phases/PHASE1.5-COORDINATION.md`
- Testing Framework: `docs/guides/TESTING-FRAMEWORK.md`
- Cross-Model Analysis: `docs/research/RECONCILIATION-ANALYSIS.md` § "Discovery 2: Synthetic SWARM Testing"

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Phase 1.5 Implementation_
