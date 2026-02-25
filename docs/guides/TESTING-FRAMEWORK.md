---
status: active
phase: 1
audience: builder
tags: [guide, testing, vitest]
last_updated: 2026-02-20
---

# Testing Framework for Impulse

> **Version:** 1.1 | **Status:** Active | **Updated:** 2026-02-21
> **Note:** Test structure updated for `impulse/` (Claude Code hooks arch). The `harness/` paths below
> reference the pre-pivot skeleton — use `impulse/` for Phase 1 implementation.

---

## Overview

Impulse uses **Vitest** for unit tests and integration tests. The framework emphasizes:

1. **Factory Functions** (fixtures) for creating test objects with sensible defaults
2. **Custom Assertions** for hook-specific invariants (atomic writes, graceful degradation)
3. **Helper Utilities** for common test patterns (tmpDir, measureTime, mockStdin, etc.)
4. **Mocking Strategies** for external dependencies (LLM API, file system)

---

## Phase 1 Test Structure (impulse/)

```
impulse/
├── src/
│   ├── files.ts
│   ├── types.ts
│   ├── extraction.ts
│   ├── cli.ts
│   └── hooks/
│       ├── session-start.ts
│       ├── post-tool-use.ts
│       ├── session-end.ts
│       └── pre-compact.ts
├── tests/
│   ├── helpers.ts                     # tmpDir factory, mockStdin, measureTime
│   ├── fixtures.ts                    # Test data factories (GENOME.md samples, JSONL samples)
│   ├── files.test.ts                  # Core file ops tests (>90% coverage required)
│   ├── session-start.test.ts          # SessionStart hook tests
│   ├── post-tool-use.test.ts          # PostToolUse hook tests
│   ├── session-end.test.ts            # SessionEnd hook tests
│   ├── pre-compact.test.ts            # PreCompact hook tests
│   ├── cli.test.ts                    # CLI command tests
│   ├── extraction.test.ts             # LLM extraction + dedup tests
│   └── integration.test.ts            # End-to-end lifecycle tests
├── vitest.config.ts
└── package.json
```

## Pre-Pivot Test Structure Reference (harness/ — DO NOT USE FOR PHASE 1)

```
harness/
├── src/
│   ├── db/
│   │   ├── database.ts
│   │   └── database.test.ts          # Unit tests for DB layer (Phase 2+)
│   ├── pattern/
│   │   ├── detector.ts
│   │   └── detector.test.ts          # Unit tests for pattern detection (Phase 1.5+)
│   ├── hooks/
│   │   ├── subscriber.ts
│   │   └── subscriber.test.ts        # OpenCode hook subscription (superseded)
│   ├── live-md/
│   │   ├── writer.ts
│   │   └── writer.test.ts            # Unit tests for LIVE.md writer
│   ├── metrics/
│   │   ├── collector.ts
│   │   └── collector.test.ts         # Unit tests for metrics
│   ├── test/
│   │   ├── fixtures.ts               # Factory functions
│   │   └── helpers.ts                # Custom assertions + utilities
│   └── integration.test.ts           # End-to-end flows
├── vitest.config.ts                  # Vitest configuration
└── package.json
```

---

## Core Patterns

### 1. Fixtures: Factory Functions

Create test objects with sane defaults + per-test overrides.

```typescript
// ✅ Good: Factory function with overrides
const event = createMessageEvent({
  agentId: 'agent-1',
  content: 'Custom message',
});

// ✅ Good: Multiple events
const events = createMultipleEvents(5, ['agent-1', 'agent-2']);

// ✅ Good: Bulk pattern creation
const patterns = createMultiplePatterns(10);
```

**Files:**

- `src/test/fixtures.ts` - All factories

### 2. Custom Assertions

Verify Impulse-specific invariants.

```typescript
// ✅ Verify Impulse provenance header format
assertValidImpulseProvenance(pattern);
// Checks: [IMPULSE:{agent}:{confidence}] format

// ✅ Verify NOT a Impulse injection (echo test)
assertNotImpulseInjection(pattern);

// ✅ Verify vector dimensions
assertValidVector(vector, 384);

// ✅ Verify confidence decay formula
assertConfidenceDecay(0.95, 0.85, 8.5, 0.03);
// confidence_t = base * e^(-λ*t)

// ✅ Verify cosine similarity
assertCosineSimilarity(v1, v2, 0.92, 0.01);
```

**Files:**

- `src/test/helpers.ts` - All assertions

### 3. Utility Functions

Common test operations.

```typescript
// ✅ Wait for async condition with timeout
await waitFor(() => db.eventCount() > 10, 5000);

// ✅ Create temporary test database
const dbPath = createTempDbPath(); // /tmp/test-swarm-{uuid}.db

// ✅ Measure function execution time
const { result, durationMs } = await measureTime(async () => {
  return db.queryEvents();
});

expect(durationMs).toBeLessThan(100);
```

---

## Test Categories

### Unit Tests

**Database Tests** (`db/database.test.ts`)

- [ ] Schema initialization (idempotent)
- [ ] Event storage and retrieval
- [ ] Vector operations
- [ ] TTL cleanup
- [ ] Concurrent writes
- [ ] Error handling

**Pattern Detector Tests** (`pattern/detector.test.ts`)

- [ ] Similarity detection (cosine >0.88)
- [ ] Anti-echo (SWARM prefix filtering)
- [ ] Rate limiting (1 per 45s)
- [ ] Confidence decay formula
- [ ] File-scoped injection
- [ ] Runaway propagation check

**Hook Subscriber Tests** (`hooks/subscriber.test.ts`)

- [ ] OpenCode connection (mocked)
- [ ] Event parsing
- [ ] Hook subscription
- [ ] Event emission

**LIVE.md Writer Tests** (`live-md/writer.test.ts`)

- [ ] File creation
- [ ] Template rendering
- [ ] Debounced updates
- [ ] File permissions

**Metrics Collector Tests** (`metrics/collector.test.ts`)

- [ ] Event counting
- [ ] Latency tracking (running average)
- [ ] Memory usage measurement
- [ ] Counter increments

### Integration Tests

**`integration.test.ts`** - End-to-end flows

- [ ] Event in → DB → LIVE.md (round-trip <5s)
- [ ] 6-agent coordination (0 runaway echoes)
- [ ] Echo loop prevention
- [ ] Token budget response (70%, 90%)
- [ ] Performance (RAM <20MB, latency <1s)

### Stress Tests (Phase 1.5+)

- [ ] 1000+ concurrent events
- [ ] Memory profiling
- [ ] Database query performance
- [ ] Vector similarity at scale

---

## Coverage Targets

| Component         | Target  | Status                             |
| ----------------- | ------- | ---------------------------------- |
| Database          | 90%     | skeleton                           |
| Pattern Detector  | 85%     | skeleton                           |
| Hook Subscriber   | 80%     | skeleton (depends on OpenCode SDK) |
| LIVE.md Writer    | 85%     | skeleton                           |
| Metrics Collector | 90%     | skeleton                           |
| **Overall**       | **85%** | skeleton                           |

---

## Running Tests

```bash
# From impulse/ directory
cd impulse && pnpm install

# Run all tests
pnpm test

# Run with coverage
pnpm test --coverage

# Run specific test file
pnpm vitest run tests/session-start.test.ts

# Watch mode
pnpm test:watch

# Debug single test
pnpm vitest run tests/session-start.test.ts --reporter=verbose
```

---

## Mocking Strategies

### 1. Database Mocking

```typescript
// Mock database for pattern detector
const mockDb = {
  getRecentEvents: vi
    .fn()
    .mockResolvedValue([
      createMessageEvent({ agentId: 'agent-1' }),
      createMessageEvent({ agentId: 'agent-1' }),
    ]),
  storeEvent: vi.fn().mockResolvedValue(undefined),
} as any;

const detector = new PatternDetector(mockDb, 384, 0.88);
```

### 2. Hook Mocking

```typescript
// Mock OpenCode hooks
const mockHooks = {
  on: vi.fn((event, handler) => {
    // Emit test events
    handler(createMessageEvent());
  }),
  connect: vi.fn().mockResolvedValue(undefined),
};
```

### 3. File System Mocking

```typescript
// Mock LIVE.md writes
vi.mock('fs', () => ({
  writeFileSync: vi.fn(),
}));
```

---

## Test Patterns to Implement

### Pattern: Idempotency Testing

```typescript
it('should be idempotent (same input = same state)', async () => {
  const event = createMessageEvent();

  await db.storeEvent(event);
  const state1 = db.snapshot();

  await db.storeEvent(event); // Duplicate
  const state2 = db.snapshot();

  expect(state1).toEqual(state2);
});
```

### Pattern: Rate Limit Testing

```typescript
it('should enforce rate limit', async () => {
  const agent = 'agent-1';

  const patterns1 = await detector.detect(createMessageEvent({ agentId: agent }));
  expect(patterns1).toHaveLength(1); // First injection allowed

  const patterns2 = await detector.detect(createMessageEvent({ agentId: agent }));
  expect(patterns2).toHaveLength(0); // Rate limited

  // Fast-forward 46s
  vi.useFakeTimers();
  vi.advanceTimersByTime(46000);

  const patterns3 = await detector.detect(createMessageEvent({ agentId: agent }));
  expect(patterns3).toHaveLength(1); // Allowed again
});
```

### Pattern: Performance Testing

```typescript
it('should process 1000 events in <5s', async () => {
  const events = createMultipleEvents(1000);

  const { durationMs } = await measureTime(async () => {
    for (const event of events) {
      await db.storeEvent(event);
    }
  });

  expect(durationMs).toBeLessThan(5000);
  expect(db.eventCount()).toBe(1000);
});
```

---

## Debugging Tests

### View test isolation

```bash
# Run single test in isolation
bun test src/db/database.test.ts --reporter=verbose
```

### Enable debug logging

```bash
LOG_LEVEL=debug bun test src/db/database.test.ts
```

### Inspect promises

```typescript
// Add to test to debug async flow
it('should work', async () => {
  const promise = db.storeEvent(event);
  console.log('Promise pending:', promise);
  await promise;
  console.log('Promise resolved');
});
```

---

## Next Steps

1. **Implement Unit Tests** (Phase 1) - Add test bodies
2. **Add Mocking** (Phase 1) - Mock DB, hooks, file system
3. **Performance Baselines** (Phase 1.5) - Establish latency/memory targets
4. **Echo Cascade Test** (Phase 1.5) - 6-agent coordination simulation
5. **Integration with CI** (Phase 2) - GitHub Actions pipeline

---

## References

- Vitest Docs: https://vitest.dev/
- Impulse Design: docs/spec/PRODUCT-SPEC-v2.md
- Phase 1 Spec: docs/phases/PHASE1-CHECKLIST.md
