---
status: active
phase: 1
audience: builder
tags: [guide, conventions, coding]
last_updated: 2026-02-20
---

# Best Practices for Impulse Development

> **Version:** 1.0 | **Status:** Reference | **Updated:** 2026-02-20

---

## TypeScript Best Practices

### 1. Type Safety

**✅ DO:**

```typescript
// Use Zod for runtime validation
import { z } from 'zod';

export const ConfigSchema = z.object({
  databasePath: z.string().min(1),
  logLevel: z.enum(['debug', 'info', 'warn', 'error']),
});

export type Config = z.infer<typeof ConfigSchema>;

// Parse at system boundaries
const config = ConfigSchema.parse(configOverrides || {});
```

**❌ DON'T:**

```typescript
// Avoid: untyped config
const config: any = configOverrides || {};

// Avoid: unsafe type assertions
const config = configOverrides as Config;
```

### 2. Error Handling

**✅ DO:**

```typescript
export class ImpulseError extends Error {
  constructor(
    message: string,
    public code: string,
    public context?: Record<string, unknown>
  ) {
    super(message);
    this.name = 'ImpulseError';
  }
}

// Use discriminated unions for error types
export const ErrorCodes = {
  FILE_READ_FAILED: 'FILE_READ_FAILED',
  FILE_WRITE_FAILED: 'FILE_WRITE_FAILED',
  LLM_API_FAILED: 'LLM_API_FAILED',
  PARSE_FAILED: 'PARSE_FAILED',
} as const;

// Throw with context
throw new ImpulseError('Failed to read GENOME.md', ErrorCodes.FILE_READ_FAILED, {
  path: '.impulse/GENOME.md',
  error: error.message,
});
```

**❌ DON'T:**

```typescript
// Avoid: throwing strings
throw 'Connection failed';

// Avoid: silent failures
try {
  await connect();
} catch (e) {
  // Swallowing error
}
```

### 3. Logging

**✅ DO:**

```typescript
// Structured logging with context
logger.info('Database connected', {
  host: 'localhost',
  port: 5432,
  retries: 2,
});

// Use correct log levels
logger.debug('Processing event', { eventId: event.id }); // Development
logger.info('Agent connected', { agentId }); // Important transitions
logger.warn('Rate limit approaching', { agentId, remaining: 3 }); // Warnings
logger.error('Database error', { error, query }); // Failures
```

**❌ DON'T:**

```typescript
// Avoid: console.log
console.log('Event:', event);

// Avoid: unstructured messages
logger.info(`Agent ${agentId} connected`);

// Avoid: logging sensitive data
logger.info('User token', { token: apiKey }); // DON'T
```

---

> **Note:** Database operations are Phase 2+. Phase 1 uses file-based storage only.

## Database Best Practices (Phase 2+)

### 1. Connection Management

**✅ DO:**

```typescript
// Use connection with proper settings
const db = new Database(dbPath);
db.pragma('journal_mode = WAL');
db.pragma('synchronous = NORMAL');

// Prepare statements once, reuse many times
const insertStmt = db.prepare('INSERT OR REPLACE INTO events ...');
insertStmt.run(id, type, data);
insertStmt.run(id2, type2, data2); // Reuse same prepared statement
```

**❌ DON'T:**

```typescript
// Avoid: new connection per query
const db = new Database(dbPath);
db.run('INSERT INTO events ...'); // Inefficient pattern

// Avoid: string concatenation (injection risk)
const query = `INSERT INTO events VALUES ('${id}', '${data}')`;
```

### 2. Vector Operations

**✅ DO:**

```typescript
// Use Float32Array for vectors (memory efficient)
const vector = new Float32Array(384);
for (let i = 0; i < 384; i++) {
  vector[i] = Math.random();
}

// Use sqlite-vec virtual tables for fast search
db.run(`
  CREATE VIRTUAL TABLE IF NOT EXISTS vectors USING vec0(
    id TEXT PRIMARY KEY,
    embedding FLOAT32[384],
    confidence FLOAT
  );
`);

// Query with cosine distance ordering
const results = db
  .prepare(
    `
  SELECT id, distance FROM vectors
  WHERE id MATCH ?
  ORDER BY distance
  LIMIT 10
`
  )
  .all(targetVector);
```

### 3. Indexes

**✅ DO:**

```typescript
// Index frequently queried columns
db.run(`
  CREATE INDEX IF NOT EXISTS idx_events_agent ON events(agent_id);
  CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at);
  CREATE INDEX IF NOT EXISTS idx_vectors_confidence ON vectors(confidence);
`);

// Run statistics for query optimization
db.run('ANALYZE');
```

---

## Testing Best Practices

### 1. Test Structure

**✅ DO:**

```typescript
describe('PatternDetector', () => {
  let detector: PatternDetector;

  beforeEach(() => {
    // Setup: Create fresh instance
    detector = new PatternDetector(mockDb, 384, 0.88);
  });

  describe('pattern detection', () => {
    it('should detect similar patterns above threshold', async () => {
      // Arrange: Set up test data
      const event = createMessageEvent({ content: 'test' });

      // Act: Perform operation
      const patterns = await detector.detect(event);

      // Assert: Verify outcome
      expect(patterns).toHaveLength(1);
      assertValidSWARMProvenance(patterns[0]);
    });
  });
});
```

**❌ DON'T:**

```typescript
// Avoid: Multiple responsibilities per test
it('should work', async () => {
  // Too much in one test
  const detector = new PatternDetector(...);
  const patterns = await detector.detect(event);
  expect(patterns).toBeDefined();
  const updated = await db.updatePattern(patterns[0]);
  expect(updated).toBe(true);
  // ... more assertions
});

// Avoid: Unclear test names
it('works', () => { ... });
```

### 2. Fixtures & Factories

**✅ DO:**

```typescript
// Create with sensible defaults
const event = createMessageEvent();
// Returns: { type: 'message.updated', agentId: 'agent-...', content: 'Test message', ... }

// Override specific fields
const customEvent = createMessageEvent({
  agentId: 'specific-agent',
  content: 'Custom content',
});

// Bulk creation
const events = createMultipleEvents(100, ['agent-1', 'agent-2']);
```

**❌ DON'T:**

```typescript
// Avoid: Manual object construction in every test
const event = {
  type: 'message.updated',
  timestamp: Date.now(),
  agentId: 'agent-1',
  role: 'assistant',
  content: 'Test',
  // ... more fields
};

// Avoid: Hardcoded test values
const agentId = 'very-specific-agent-id-that-appears-in-10-tests';
```

### 3. Custom Assertions

**✅ DO:**

```typescript
// Domain-specific assertions
assertValidImpulseProvenance(pattern);
assertValidGenomeFormat(genomeContent);
assertValidLiveState(liveState);

// These catch domain-specific bugs faster
// Error message: "[IMPULSE] GENOME.md format invalid: missing header"
```

**❌ DON'T:**

```typescript
// Avoid: Generic assertions without context
expect(pattern.suggestedInjection).toMatch(/^\[IMPULSE:/);
expect(confidence).toBeCloseTo(expected, 2);

// Less helpful error messages
```

**❌ DON'T:**

```typescript
// Avoid: Generic assertions without context
expect(pattern.suggestedInjection).toMatch(/^\[SWARM:/);
expect(confidence).toBeCloseTo(expected, 2);

// Less helpful error messages
```

---

> **Note:** Pattern detection is Phase 1.5+. Phase 1 has basic LIVE_STATE.json awareness only.

## Pattern Detection Best Practices (Phase 1.5+)

### 1. Similarity Calculation

**✅ DO:**

```typescript
// Use normalized cosine similarity (0-1 range)
function cosineSimilarity(v1: Float32Array, v2: Float32Array): number {
  let dotProduct = 0;
  let norm1Sq = 0;
  let norm2Sq = 0;

  for (let i = 0; i < v1.length; i++) {
    dotProduct += v1[i] * v2[i];
    norm1Sq += v1[i] * v1[i];
    norm2Sq += v2[i] * v2[i];
  }

  return dotProduct / (Math.sqrt(norm1Sq) * Math.sqrt(norm2Sq));
  // Returns value in [0, 1] for normalized vectors
}

// Use consistent threshold
const SIMILARITY_THRESHOLD = 0.88;
if (similarity > SIMILARITY_THRESHOLD) {
  // Detect pattern
}
```

**❌ DON'T:**

```typescript
// Avoid: Inconsistent thresholds
if (similarity > 0.85) {
  detect();
} // Test 1
if (similarity > 0.9) {
  detect();
} // Test 2

// Avoid: Unnormalized similarity calculation
const similarity = dotProduct; // Can be any magnitude!
```

### 2. Rate Limiting

**✅ DO:**

```typescript
// Per-agent, per-45s rate limit
private lastInjectionTime = new Map<string, number>();

private isRateLimited(agentId: string): boolean {
  const lastTime = this.lastInjectionTime.get(agentId);
  if (!lastTime) return false; // First injection always allowed
  return Date.now() - lastTime < 45000;
}

// Record injection time
if (patterns.length > 0) {
  this.lastInjectionTime.set(agentId, Date.now());
}
```

### 3. Confidence Decay

**✅ DO:**

```typescript
// Exponential decay: confidence_t = base * e^(-λ*t)
const DECAY_LAMBDA = 0.03; // Half-life approximately 23 minutes

function decayedConfidence(baseConfidence: number, minutesElapsed: number): number {
  return baseConfidence * Math.exp(-DECAY_LAMBDA * minutesElapsed);
}

// Timeline example:
// At t=0min:  100% confidence
// At t=23min: 50% confidence
// At t=120min: ~1.5% (effectively zero)
```

---

## Performance Best Practices

### 1. Memory Management

**✅ DO:**

```typescript
// Use appropriate data structures
// - Float32Array for vectors (not Array<number>)
// - Map for fast lookups (not array.find())
// - Set for uniqueness (not array.includes())

const vectorCache = new Map<string, Float32Array>();
const uniqueAgents = new Set<string>();

// Profile memory usage
const memUsage = process.memoryUsage();
const heapUsedMB = memUsage.heapUsed / 1024 / 1024;
logger.info('Memory usage', { heapUsedMB });

// Track maximum during session
let maxHeapMB = 0;
setInterval(() => {
  const current = process.memoryUsage().heapUsed / 1024 / 1024;
  maxHeapMB = Math.max(maxHeapMB, current);
}, 1000);
```

### 2. Latency Control

**✅ DO:**

```typescript
// Measure and track latency
async function withLatency<T>(name: string, fn: () => Promise<T>): Promise<T> {
  const start = Date.now();
  const result = await fn();
  const duration = Date.now() - start;
  logger.debug(`${name} completed`, { durationMs: duration });
  return result;
}

// Use in performance-critical paths
const patterns = await withLatency('pattern detection', () => detector.detect(event));

// Target latencies:
// - Event storage: <50ms
// - Vector search: <200ms (for 10k vectors)
// - Pattern detection: <500ms
// - LIVE.md write: <2s
```

### 3. Batch Operations

**✅ DO:**

```typescript
// Batch inserts for better database performance
db.transaction(() => {
  for (const event of events) {
    insertStmt.run(event.id, event.type, event.data);
  }
})();

// Query efficiently using indexes
// ✅ Good: `SELECT * FROM vectors WHERE agent_id = ?` (uses index)
// ❌ Bad: `SELECT * FROM vectors` (full table scan)
```

---

## Error Recovery Best Practices

### 1. Retry Logic

**✅ DO:**

```typescript
// Exponential backoff for transient failures
async function retryWithBackoff<T>(
  operation: () => Promise<T>,
  maxRetries = 3,
  baseDelayMs = 1000
): Promise<T> {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      return await operation();
    } catch (error) {
      if (attempt === maxRetries - 1) throw error;
      const delay = baseDelayMs * Math.pow(2, attempt);
      logger.warn(`Retrying after ${delay}ms`, { attempt, error });
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }
  throw new Error('Max retries exceeded');
}

// Use in critical paths
await retryWithBackoff(() => db.storeEvent(event));
```

### 2. Graceful Degradation

**✅ DO:**

```typescript
// When pattern detection times out, gracefully degrade
const patterns = await Promise.race([
  detector.detect(event),
  new Promise<[]>((_, reject) => setTimeout(() => reject(new Error('Timeout')), 500)),
]).catch((error) => {
  logger.warn('Pattern detection timeout', { error });
  return []; // Return empty patterns, continue operation
});
```

---

## Documentation Best Practices

### 1. Function Documentation

**✅ DO:**

```typescript
/**
 * Detect patterns when agent sends a new message
 *
 * Algorithm:
 * 1. Skip if message is SWARM injection (anti-echo)
 * 2. Get recent events for this agent (last 8 messages)
 * 3. Create vector embedding of recent context
 * 4. Query database for similar vectors from other agents
 * 5. Apply safeguards: rate limit, confidence decay, file scope
 *
 * @param event - Message event from OpenCode
 * @returns Array of detected patterns, or empty array if:
 *   - Agent is rate-limited (within 45s of last injection)
 *   - No similar patterns found (similarity < 0.88)
 *   - Pattern detection timeout
 */
async detect(event: MessageEvent): Promise<Pattern[]> {
  // Implementation here
}
```

**❌ DON'T:**

```typescript
// Bad: Obvious or outdated comments
async detect(event: MessageEvent): Promise<Pattern[]> {
  // Check if SWARM injection
  if (this.isSWARMInjection(event.content)) {
    // Skip
    return [];
  }
  // Implementation
}

// Bad: No documentation at all
// Just implementation with no explanation
```

### 2. Type Documentation

**✅ DO:**

```typescript
export interface Pattern {
  /** Unique pattern identifier */
  id: string;

  /** Which agents are participating (e.g., ['agent-1', 'agent-2']) */
  sourceAgents: string[];

  /** Cosine similarity score between agent contexts (0.88-1.0) */
  similarity: number;

  /** Extracted topic or theme (e.g., 'authentication module') */
  extractedTopic: string;

  /** Suggested injection message, max 120 tokens */
  suggestedInjection: string;

  /** Confidence after decay applied (0-1) */
  confidenceScore: number;

  /** Timestamp when pattern was detected */
  detectedAt: number;

  /** File paths involved, for scoped injection */
  fileScope?: string[];
}
```

---

## References

- Linting: `impulse/.eslintrc` (to be created)
- TypeScript: `impulse/tsconfig.json` (strict mode enabled)
- Testing: `impulse/vitest.config.ts`
- Phase 1 Spec: `docs/spec/PRODUCT-SPEC-v2.md`
- Testing Guide: `docs/guides/TESTING-FRAMEWORK.md`

---

_Created: 2026-02-20 | Status: Reference Guide v1.0_
