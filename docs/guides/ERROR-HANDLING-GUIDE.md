---
status: superseded
phase: 1
audience: builder
tags: [guide, errors, patterns]
last_updated: 2026-02-20
---

# Error Handling & Recovery Guide

> **Historical TypeScript/Bun design — superseded.** The error taxonomy below is not the current
> Rust contract. Use the canonical Rust contract and typed errors in the live source.

> **Version:** 1.0 | **Status:** Design | **Updated:** 2026-02-21
> **Phase 1 note:** Phase 1 (Claude Code hooks) has a much simpler error model than described below.
> The hierarchy here was designed for Phase 1.5+ (database, pattern detection, OpenCode integration).
> For Phase 1, the only error handling rule is: **always exit 0, log to stderr, never crash the hook.**
> Phase 1 error types: `FILE_READ_FAILED`, `FILE_WRITE_FAILED`, `LLM_API_FAILED`, `PARSE_FAILED`, `PATH_TRAVERSAL`.
> That's it. The 50+ error codes below apply to Phase 1.5+ components.

---

## Error Architecture

### Error Hierarchy

```
HarnessError (base)
├── DatabaseError
│   ├── ConnectionError
│   ├── QueryError
│   └── SchemaError
├── HookError
│   ├── SubscriptionError
│   └── WebhookError
├── PatternError
│   ├── EmbeddingError
│   └── DetectionError
└── InjectionError
    ├── RateLimitError
    └── TimeoutError
```

### Error Codes (50+)

```typescript
export const ErrorCodes = {
  // Database (DB_xxxx)
  DB_INIT_FAILED: 'DB_INIT_FAILED',
  DB_CONNECTION_FAILED: 'DB_CONNECTION_FAILED',
  DB_QUERY_FAILED: 'DB_QUERY_FAILED',
  DB_WRITE_FAILED: 'DB_WRITE_FAILED',
  DB_SCHEMA_MISMATCH: 'DB_SCHEMA_MISMATCH',
  DB_MIGRATION_FAILED: 'DB_MIGRATION_FAILED',
  DB_CLEANUP_FAILED: 'DB_CLEANUP_FAILED',
  DB_VECTOR_INSERT_FAILED: 'DB_VECTOR_INSERT_FAILED',
  DB_VECTOR_SEARCH_FAILED: 'DB_VECTOR_SEARCH_FAILED',
  DB_TIMEOUT: 'DB_TIMEOUT',

  // Hooks (HOOK_xxxx)
  HOOK_SUBSCRIPTION_FAILED: 'HOOK_SUBSCRIPTION_FAILED',
  HOOK_WEBHOOK_FAILED: 'HOOK_WEBHOOK_FAILED',
  HOOK_PARSE_FAILED: 'HOOK_PARSE_FAILED',
  HOOK_CONNECTION_LOST: 'HOOK_CONNECTION_LOST',
  HOOK_TIMEOUT: 'HOOK_TIMEOUT',

  // Pattern Detection (PATTERN_xxxx)
  PATTERN_EMBEDDING_FAILED: 'PATTERN_EMBEDDING_FAILED',
  PATTERN_DETECTION_FAILED: 'PATTERN_DETECTION_FAILED',
  PATTERN_EXTRACTION_FAILED: 'PATTERN_EXTRACTION_FAILED',
  PATTERN_TIMEOUT: 'PATTERN_TIMEOUT',
  PATTERN_RATE_LIMITED: 'PATTERN_RATE_LIMITED',

  // Injection (INJECT_xxxx)
  INJECT_FAILED: 'INJECT_FAILED',
  INJECT_TIMEOUT: 'INJECT_TIMEOUT',
  INJECT_AUTH_FAILED: 'INJECT_AUTH_FAILED',
  INJECT_RATE_LIMITED: 'INJECT_RATE_LIMITED',

  // LIVE.md Writer (LIVE_xxxx)
  LIVE_WRITE_FAILED: 'LIVE_WRITE_FAILED',
  LIVE_PERMISSION_DENIED: 'LIVE_PERMISSION_DENIED',
  LIVE_DISK_FULL: 'LIVE_DISK_FULL',

  // Configuration (CONFIG_xxxx)
  CONFIG_INVALID: 'CONFIG_INVALID',
  CONFIG_MISSING_REQUIRED: 'CONFIG_MISSING_REQUIRED',

  // System (SYS_xxxx)
  MEMORY_LIMIT_EXCEEDED: 'MEMORY_LIMIT_EXCEEDED',
  OPERATION_TIMEOUT: 'OPERATION_TIMEOUT',
  UNKNOWN_ERROR: 'UNKNOWN_ERROR',
} as const;
```

---

## Error Handling Patterns

### 1. Try-Catch with Specific Logging

```typescript
async function storeEvent(event: HarnessEvent): Promise<void> {
  try {
    const result = await db.prepare(
      'INSERT INTO events (...) VALUES (...)',
    ).run(event.id, event.type, event.data);

    logger.debug('Event stored', { eventId: event.id });
  } catch (error) {
    // Log with context
    logger.error('Failed to store event', {
      error: error instanceof Error ? error.message : String(error),
      code: ErrorCodes.DB_WRITE_FAILED,
      eventId: event.id,
      eventType: event.type,
      stack: error instanceof Error ? error.stack : undefined,
    });

    // Rethrow as HarnessError
    throw new HarnessError(
      `Failed to store event: ${event.id}`,
      ErrorCodes.DB_WRITE_FAILED,
      { originalError: error, event },
    );
  }
}
```

### 2. Retry with Exponential Backoff

```typescript
async function retryWithBackoff<T>(
  operation: () => Promise<T>,
  maxRetries = 3,
  baseDelayMs = 1000,
  isRetryable: (error: unknown) => boolean = () => true,
): Promise<T> {
  let lastError: Error | null = null;

  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      return await operation();
    } catch (error) {
      lastError = error as Error;

      // Don't retry non-retryable errors
      if (!isRetryable(error)) {
        throw error;
      }

      if (attempt === maxRetries - 1) {
        // Last attempt failed
        break;
      }

      // Exponential backoff: 1s, 2s, 4s
      const delay = baseDelayMs * Math.pow(2, attempt);
      logger.warn(`Retrying after ${delay}ms`, {
        attempt: attempt + 1,
        maxRetries,
        error: lastError.message,
      });

      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }

  throw new HarnessError(
    `Operation failed after ${maxRetries} retries: ${lastError?.message}`,
    ErrorCodes.OPERATION_TIMEOUT,
    { lastError },
  );
}

// Use in critical paths
async function connectToOpenCode(): Promise<void> {
  await retryWithBackoff(
    () => hookSubscriber.connect(),
    3,
    2000,
    (error) => {
      // Only retry transient errors (not auth errors)
      return !(error instanceof Error && error.message.includes('401'));
    },
  );
}
```

### 3. Graceful Degradation

```typescript
async function detectPatterns(
  event: MessageEvent,
): Promise<Pattern[]> {
  try {
    return await patternDetector.detect(event);
  } catch (error) {
    logger.warn('Pattern detection failed, returning empty', { error });
    // Return empty patterns, continue operation
    return [];
  }
}

// In compaction hook: if injection times out, still respond
const patterns = await Promise.race([
  detector.detect(event),
  new Promise<[]>((_, reject) =>
    setTimeout(
      () => reject(new Error('Detection timeout')),
      500,
    ),
  ),
]).catch((error) => {
  logger.warn('Pattern detection timeout', { error });
  return []; // Graceful fallback
});
```

### 4. Circuit Breaker

```typescript
class CircuitBreaker {
  private state: 'closed' | 'open' | 'half-open' = 'closed';
  private failureCount = 0;
  private lastFailureTime = 0;
  private readonly failureThreshold = 5;
  private readonly resetTimeoutMs = 60000; // 1 minute

  async call<T>(fn: () => Promise<T>): Promise<T> {
    // Open circuit if too many failures
    if (this.state === 'open') {
      if (Date.now() - this.lastFailureTime > this.resetTimeoutMs) {
        this.state = 'half-open';
        logger.info('Circuit breaker half-open, attempting recovery');
      } else {
        throw new Error('Circuit breaker is open');
      }
    }

    try {
      const result = await fn();

      // Success: reset
      if (this.state === 'half-open') {
        this.state = 'closed';
        this.failureCount = 0;
        logger.info('Circuit breaker closed, recovered');
      }

      return result;
    } catch (error) {
      this.failureCount++;
      this.lastFailureTime = Date.now();

      if (this.failureCount >= this.failureThreshold) {
        this.state = 'open';
        logger.warn('Circuit breaker opened', {
          failures: this.failureCount,
          error,
        });
      }

      throw error;
    }
  }
}

// Use for unreliable external services
const openCodeBreaker = new CircuitBreaker();
const injection = await openCodeBreaker.call(() =>
  hookSubscriber.sendInjection(payload),
);
```

---

## Recovery Strategies

### Database Recovery

| Error | Cause | Recovery |
|-------|-------|----------|
| DB_CONNECTION_FAILED | Socket error | Retry with backoff (up to 3x) |
| DB_SCHEMA_MISMATCH | Version mismatch | Log error, fail fast (manual fix) |
| DB_QUERY_FAILED | Malformed SQL | Log query, rethrow (code bug) |
| DB_WRITE_FAILED | Disk full | Cleanup old events, retry |
| DB_TIMEOUT | Lock contention | Retry with exponential backoff |

**Implementation:**
```typescript
async function withDatabaseRecovery<T>(
  operation: () => Promise<T>,
): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    if (error instanceof Error && error.message.includes('SQLITE_FULL')) {
      // Disk full: cleanup and retry
      logger.warn('Disk full, triggering cleanup');
      await cleanupExpiredEvents();
      return await operation(); // Retry once
    }
    throw error;
  }
}
```

### Hook Recovery

| Error | Cause | Recovery |
|-------|-------|----------|
| HOOK_CONNECTION_LOST | Network | Reconnect with backoff |
| HOOK_SUBSCRIPTION_FAILED | Auth | Log error, fail fast |
| HOOK_WEBHOOK_FAILED | Handler crash | Log error, skip event |
| HOOK_TIMEOUT | Slow handler | Return early, process async |

**Implementation:**
```typescript
async reconnect(): Promise<void> {
  await retryWithBackoff(
    () => this.connect(),
    5, // 5 attempts
    3000, // 3s base delay
  );
}

// Webhook handler (non-blocking)
app.post('/hook/:type', (req, res) => {
  res.json({ status: 'received' }); // Return immediately
  setImmediate(() => handleEvent(req.body)); // Process async
});
```

### Pattern Detection Recovery

| Error | Cause | Recovery |
|-------|-------|----------|
| PATTERN_EMBEDDING_FAILED | Model unavailable | Log warning, skip pattern |
| PATTERN_DETECTION_FAILED | Database error | Propagate error (let DB layer handle) |
| PATTERN_TIMEOUT | Slow computation | Timeout at 500ms, return empty |

**Implementation:**
```typescript
async detect(event: MessageEvent): Promise<Pattern[]> {
  try {
    return await detectWithTimeout(event, 500);
  } catch (error) {
    if (error instanceof Error && error.message.includes('Timeout')) {
      logger.warn('Pattern detection timeout', { agentId: event.agentId });
      return [];
    }
    throw error;
  }
}
```

---

## Observability

### Error Metrics

```typescript
class ErrorMetrics {
  private errorCounts = new Map<string, number>();
  private errorHistory: Array<{
    code: string;
    timestamp: number;
    message: string;
  }> = [];

  recordError(code: string, error: Error): void {
    this.errorCounts.set(
      code,
      (this.errorCounts.get(code) || 0) + 1,
    );

    this.errorHistory.push({
      code,
      timestamp: Date.now(),
      message: error.message,
    });

    // Keep only last 100 errors
    if (this.errorHistory.length > 100) {
      this.errorHistory.shift();
    }
  }

  getTopErrors(limit = 5): Array<[string, number]> {
    return Array.from(this.errorCounts.entries())
      .sort(([, a], [, b]) => b - a)
      .slice(0, limit);
  }

  getErrorRate(): number {
    const recent = this.errorHistory.filter(
      (e) => Date.now() - e.timestamp < 60000, // Last minute
    );
    return recent.length;
  }
}
```

### Alerting

```typescript
// Alert on error spikes
setInterval(() => {
  const rate = errorMetrics.getErrorRate();
  if (rate > 10) {
    // More than 10 errors in last minute
    logger.error('High error rate detected', {
      errorsPerMinute: rate,
      topErrors: errorMetrics.getTopErrors(3),
    });
    // TODO: Send alert to ops team
  }
}, 60000);
```

---

## Testing Error Paths

### Simulate Database Error

```typescript
it('should handle database write failures', async () => {
  // Mock database to throw error
  vi.spyOn(db, 'storeEvent').mockRejectedValue(
    new Error('Database connection failed'),
  );

  // Should rethrow as HarnessError
  await expect(
    harness.handleEvent(createMessageEvent()),
  ).rejects.toThrow(HarnessError);

  // Verify error code
  expect(harness.getLastError()?.code).toBe(
    ErrorCodes.DB_WRITE_FAILED,
  );
});
```

### Simulate Timeout

```typescript
it('should timeout pattern detection after 500ms', async () => {
  // Mock detector to hang
  vi.spyOn(detector, 'detect').mockImplementation(
    () =>
      new Promise((resolve) =>
        setTimeout(() => resolve([]), 1000),
      ),
  );

  const patterns = await detector.detect(event);

  // Should return empty (timeout)
  expect(patterns).toHaveLength(0);
});
```

---

## References

- Error Codes: `src/types.ts` (ErrorCodes constant)
- Best Practices: `docs/guides/BEST-PRACTICES.md` (Error Handling section)
- Phase 1 Spec: `docs/archive/SPEC-v1.1.md`

---

_Created: 2026-02-20 | Status: Design v1.0 Ready for Phase 1 Implementation_
