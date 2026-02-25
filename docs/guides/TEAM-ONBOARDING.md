---
status: active
phase: all
audience: contributor
tags: [guide, onboarding, team]
last_updated: 2026-02-20
---

# Team Onboarding Guide

> **Version:** 1.0 | **Status:** Ready | **Updated:** 2026-02-20
> **Estimated Duration:** 2-3 days | **Target Audience:** New developers

---

## Welcome to SWARM Development!

This guide will help new developers understand the project, set up their environment, and make their first contribution.

---

## Day 1: Fundamentals

### Morning (2-3 hours): Project Immersion

**1. Read Core Documentation** (30 min)
- [ ] Start here: `docs/session-logs/FRAMEWORKS-SUMMARY.md` (executive overview)
- [ ] Read: `CLAUDE.md` (project vision, architecture)
- [ ] Read: `docs/archive/ARCHITECTURE.md` (system topology)
- [ ] Skim: `docs/archive/SPEC-v1.1.md` (Phase 1 spec)

**2. Understand the Problem** (30 min)
- [ ] What is SWARM? (Multi-agent coordination harness)
- [ ] Why SWARM? (Agents working in same repo need awareness)
- [ ] How does it work? (Pattern detection + injection via hooks)
- [ ] Key innovation: Anti-echo + rate limiting + confidence decay

**3. Architecture Deep Dive** (60 min)
- [ ] 5-layer architecture (Ingestion → Storage → Detection → Writer → Instrumentation)
- [ ] Event flow (OpenCode hook → database → pattern detection → injection)
- [ ] Key components (Harness, Database, PatternDetector, LiveWriter, Metrics)
- [ ] Watch for: Invariants (anti-echo, rate limit, decay)

### Afternoon (2-3 hours): Technical Setup

**4. Environment Setup**
```bash
# Clone the repo
cd /path/to/impulse

# Install dependencies
cd harness && bun install

# Verify setup
bun run type-check  # Should pass
bun run lint        # Should pass
bun test           # Will fail (expected, needs implementation)
```

**5. Explore the Code**
- [ ] Open `harness/` in your editor
- [ ] Read `src/types.ts` (all type definitions + Zod schemas)
- [ ] Read `src/harness.ts` (main orchestration logic)
- [ ] Skim `src/db/database.ts` (database layer outline)
- [ ] Skim `src/pattern/detector.ts` (algorithm outline)

**6. Understand Testing Framework**
- [ ] Read `src/test/fixtures.ts` (factory functions)
- [ ] Read `src/test/helpers.ts` (assertions + utilities)
- [ ] Read `docs/guides/TESTING-FRAMEWORK.md` (full testing guide)
- [ ] Look at `src/db/database.test.ts` (example test structure)

---

## Day 2: Deep Dives

### Morning (3 hours): Database & Schema

**1. Study Database Design** (90 min)
- [ ] Read: `docs/guides/DATABASE-GUIDE.md` (complete guide)
- [ ] Understand: 4 tables (events, vectors, patterns, metadata)
- [ ] Understand: Vector partitioning by file/topic
- [ ] Understand: TTL-based expiration (24h)
- [ ] Understand: Index strategy (7 indexes for hot/warm/cold)

**2. Study sqlite-vec** (60 min)
- [ ] Read cloned-repos/sqlite-vec/ README
- [ ] Understand: 384-dimensional float vectors
- [ ] Understand: Cosine distance for similarity
- [ ] Understand: Virtual table API for search
- [ ] Example query: SELECT ... WHERE partition MATCH ? ORDER BY distance

**3. Hands-On: Schema Exploration**
```bash
# Open database CLI
sqlite3 live_state.db

# Explore schema
.schema events
.schema vectors
.schema patterns

# Test query
SELECT * FROM events LIMIT 1;
```

### Afternoon (3 hours): OpenCode Integration

**4. Study OpenCode Plugin System** (90 min)
- [ ] Read: `docs/archive/OPENCODE-INTEGRATION.md` (complete guide)
- [ ] Study: cloned-repos/opencode/packages/plugin/src/index.ts (hook interface)
- [ ] Understand: 3 primary hooks (message.updated, tool.execute.after, compaction)
- [ ] Understand: Webhook receiver pattern (REST API, non-blocking)
- [ ] Understand: Injection response (context + confidence)

**5. Study Error Handling** (60 min)
- [ ] Read: `docs/guides/ERROR-HANDLING-GUIDE.md` (complete guide)
- [ ] Understand: 50+ error codes (see ErrorCodes in src/types.ts)
- [ ] Understand: 5 recovery patterns (retry, circuit breaker, graceful degradation, etc.)
- [ ] Understand: Observability (metrics, alerting)

**6. Hands-On: Webhook Testing**
```bash
# Start mock OpenCode server (to be implemented)
node cloned-repos/opencode/mock-server.js

# Send test event
curl -X POST http://localhost:3000/api/plugin/subscribe \
  -H "Content-Type: application/json" \
  -d '{"hook": "message.updated", "webhookUrl": "http://localhost:3001/hook/message.updated"}'
```

---

## Day 3: First Contribution

### Morning (3 hours): Choose First Task

**1. Recommend First Task**

Start with **one of these**, in order of simplicity:

1. **Metrics Collector Tests** (easiest, isolated)
   - File: `src/metrics/collector.test.ts`
   - Why: Metrics are simple, no dependencies
   - Impact: Critical instrumentation
   - Effort: 1-2 hours

2. **Logger Implementation** (very simple)
   - File: `src/utils/logger.ts` (complete)
   - Task: Write tests in `src/utils/logger.test.ts`
   - Why: Already implemented, just test
   - Effort: 1 hour

3. **Database Schema Tests** (moderate)
   - File: `src/db/database.test.ts`
   - Why: Core infrastructure, well-designed
   - Impact: Foundation for everything else
   - Effort: 4-6 hours

4. **Type Validation Tests** (moderate)
   - File: `src/types.ts` (complete)
   - Task: Write tests for Zod schemas
   - Why: Catch bugs early
   - Effort: 2-3 hours

5. **Pattern Detection Anti-Echo** (advanced)
   - File: `src/pattern/detector.ts`
   - Task: Implement + test `isSWARMInjection()`
   - Why: Critical invariant
   - Effort: 2-3 hours

**2. Set Up Your First Branch**
```bash
git checkout -b feat/your-task-name
# e.g., feat/metrics-tests

# Create or edit file
# Commit frequently with clear messages
git add .
git commit -m "feat: add metrics collector tests"
```

**3. Development Workflow**
```bash
# Type check on save
bun run type-check

# Test your changes
bun test src/path/to/your.test.ts

# Lint before commit
bun run lint

# Full validation before PR
bun test && bun run lint && bun run type-check
```

### Afternoon (2-3 hours): Code Review Process

**4. Create Pull Request**
```bash
# Push your branch
git push origin feat/your-task-name

# Create PR via GitHub CLI or web
gh pr create --title "Add metrics tests" --body "..."
```

**5. Code Review Checklist**
Before requesting review, verify:
- [ ] All tests pass (`bun test`)
- [ ] No lint errors (`bun run lint`)
- [ ] TypeScript strict mode passes (`bun run type-check`)
- [ ] Test coverage >85%
- [ ] Functions documented
- [ ] No console.log in production code
- [ ] Commit messages clear

**6. Respond to Feedback**
- [ ] Read all comments carefully
- [ ] Ask for clarification if needed
- [ ] Make changes in new commits (don't amend)
- [ ] Push and request re-review
- [ ] Don't take feedback personally!

---

## Key Concepts to Understand

### 1. Anti-Echo Invariant

**Rule:** Never re-score patterns containing `[SWARM:` prefix

**Why?** Prevents feedback loops where injected content triggers new patterns

```typescript
// Skip SWARM injections
if (event.content.startsWith('[SWARM:')) {
  return []; // No pattern detection
}
```

### 2. Rate Limiting (45 seconds)

**Rule:** Max 1 injection per agent per 45 seconds

**Why?** Prevents flooding agents with too many suggestions

```typescript
// Check if rate limited
if (Date.now() - lastInjection < 45000) {
  return []; // Rate limited
}
```

### 3. Confidence Decay (λ=0.03)

**Rule:** Exponential decay: `confidence_t = base * e^(-0.03 * minutes)`

**Why?** Older patterns become less relevant

```typescript
// Half-life: ~23 minutes (50% confidence after 23 min)
const decayed = baseConfidence * Math.exp(-0.03 * minutesElapsed);
```

### 4. Vector Similarity (0.88 threshold)

**Rule:** Patterns detected when cosine similarity > 0.88

**Why?** Threshold filters noise, only detects strong overlaps

```typescript
if (similarity > 0.88) {
  // Detect pattern
}
```

---

## Debugging Tips

### 1. Enable Debug Logging

```bash
LOG_LEVEL=debug bun test src/db/database.test.ts
```

### 2. Inspect Database

```bash
sqlite3 /path/to/test.db
SELECT COUNT(*) FROM events;
SELECT * FROM patterns LIMIT 1;
```

### 3. Trace Execution

```typescript
// Add markers
logger.debug('Starting pattern detection', { agentId });
// ... code ...
logger.debug('Pattern detection complete', { patternCount });
```

### 4. Check Test Output

```bash
bun test --reporter=verbose src/your.test.ts
```

---

## Common Mistakes to Avoid

| Mistake | Fix |
|---------|-----|
| Forgetting to run `bun install` | Always install after pulling |
| Using `any` type | Use Zod schemas or explicit types |
| Not writing tests | Every feature needs tests |
| Committing console.log | Use logger.debug() instead |
| Breaking type check | Run `bun run type-check` before commit |
| Not reading docs | Read TESTING-FRAMEWORK.md first |
| Making assumptions | Ask in Discord/email, don't guess |

---

## Resources

**Must Read (in order):**
1. docs/session-logs/FRAMEWORKS-SUMMARY.md (overview)
2. docs/archive/ARCHITECTURE.md (system design)
3. docs/guides/TESTING-FRAMEWORK.md (how to test)
4. docs/guides/BEST-PRACTICES.md (code style)

**Reference (as needed):**
- docs/guides/DATABASE-GUIDE.md (database operations)
- docs/archive/OPENCODE-INTEGRATION.md (hook integration)
- docs/guides/ERROR-HANDLING-GUIDE.md (error codes)
- harness/README.md (project structure)

**External:**
- Zod: https://zod.dev/ (type validation)
- Vitest: https://vitest.dev/ (testing)
- SQLite: https://www.sqlite.org/ (database)
- Bun: https://bun.sh/ (runtime)

---

## Questions?

- **Technical:** Ask in code comments or create discussion PR
- **Architecture:** Tag maintainers in PR
- **Setup Issues:** Check CLAUDE.md → Environment Setup
- **Time Blocking:** Estimated in each task

---

## Success Criteria for Day 3

✅ Completed first task
✅ Created PR with tests
✅ Received code review feedback
✅ Understood 4 key invariants
✅ Know where to find documentation
✅ Know how to run tests locally

---

## What's Next?

After completing your first task:

1. **Pick your next task** (aim for 3-5 tasks in first week)
2. **Gradually increase complexity** (metrics → types → database → patterns)
3. **Study one module deeply** (become the expert on one component)
4. **Help review others' PRs** (learn by reading code)

---

_Created: 2026-02-20 | Status: Ready | Last Updated: 2026-02-20_
