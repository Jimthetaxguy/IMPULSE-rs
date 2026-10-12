---
status: superseded
phase: 1-2
audience: builder
tags: [guide, integration, recipes]
last_updated: 2026-02-20
---

# Integration Cookbook: End-to-End Impulse Workflows

> **Historical TypeScript/Bun guide — superseded.** These phase-era recipes describe the former
> memory-plugin architecture. Use [`../../VISION.md`](../../VISION.md) and
> [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md) for the current system.

> **Version:** 1.1 | **Status:** Recipes | **Updated:** 2026-02-21
> **Purpose:** Show how Phase 1, 1.5, 2 frameworks work together in real scenarios

---

## Overview

This cookbook contains **real-world workflows** showing how Impulse manages cross-session memory and multi-agent awareness.

**Use this to:**
- Understand data flow across all components
- Debug integration issues
- Validate end-to-end correctness
- Teach new developers the full system

---

## Recipe 0: Phase 1 — Single Agent Session Lifecycle ⭐

**Scenario:** One developer using Claude Code with Impulse installed.

### Timeline

```
t=0s    Developer starts Claude Code in project
        └─ Claude Code fires: SessionStart hook → impulse-session-start
        └─ impulse-session-start stdin: { session_id, transcript_path, cwd }
        └─ Action: Read .impulse/GENOME.md
                   Read .impulse/LIVE_STATE.json
                   Read .impulse/HISTORY_INDEX.md (last 3 entries)
                   Register agent in LIVE_STATE.json
        └─ stdout: formatted context block (Claude receives this as system context)

t=0s    Claude now knows:
        - Previous architectural decisions (GENOME.md)
        - Whether another agent is active (LIVE_STATE.json)
        - What happened in the last 3 sessions (HISTORY_INDEX.md)

t=1s    Developer asks Claude to implement JWT auth
        Claude works, edits src/auth.ts, src/middleware/auth.ts

t=5s    Claude edits src/auth.ts
        └─ Claude Code fires: PostToolUse hook → impulse-post-tool-use
        └─ Action: Parse tool input → extract "src/auth.ts"
                   Update LIVE_STATE.json: agent.activeFiles = ["src/auth.ts"]
        └─ No stdout (fire-and-forget)

t=1800s Developer ends session (closes terminal, /clear, etc.)
        └─ Claude Code fires: SessionEnd hook → impulse-session-end
        └─ Action: Read transcript JSONL from transcript_path
                   Filter tool noise (remove 75% of content)
                   Sample: 30% beginning + 70% end
                   Read GENOME.md for contradiction awareness
                   Call LLM API (1 Haiku call):
                     → "Extract architectural decisions from this session"
                   Parse response: { decisions: [...], summary: "..." }
                   Deduplicate against GENOME.md
                   Append new decisions to GENOME.md
                   Prepend session summary to HISTORY_INDEX.md
                   Remove agent from LIVE_STATE.json
        └─ No stdout (fire-and-forget)

t=1800s GENOME.md now contains:
        - "2026-02-21: Using JWT with 15-minute expiry, HttpOnly cookies"
        - "2026-02-21: bcrypt for password hashing (not argon2, Node.js support)"

t=next  Next session starts — Claude immediately knows:
        "We use JWT with 15-minute expiry..." (from GENOME.md injection at SessionStart)
```

### Debugging Phase 1

| Symptom | Check |
|---------|-------|
| Context not appearing at session start | Check `.claude/settings.local.json` has SessionStart hook |
| GENOME.md not updating | Check SessionEnd hook timeout (should be 60s) |
| LIVE_STATE.json growing forever | Agent crash without SessionEnd — manually clean: `impulse status` |
| Extraction cost too high | Set `IMPULSE_MODEL=claude-haiku-4-5-20251001` |

---

## Recipe 0b: Phase 1 — Multi-Agent Awareness

**Scenario:** Two developers on the same repo, both using Claude Code + Impulse.

```
t=0s    Dev-A starts Claude Code session
        └─ impulse-session-start:
           LIVE_STATE.json: { agents: [{ id: "session-abc", ... }] }
           stdout: "### Other Active Agents\nNone"

t=60s   Dev-B starts Claude Code session
        └─ impulse-session-start:
           LIVE_STATE.json: { agents: [{ id: "session-abc", activeFiles: ["src/auth.ts"] }] }
           stdout: "### Other Active Agents\n- session-abc: editing src/auth.ts (1m ago)"

t=61s   Dev-B's Claude now knows: "Dev-A is editing src/auth.ts"
        → Claude will avoid conflicting changes to src/auth.ts
        → Or coordinate explicitly: "I see another agent is in src/auth.ts, let me work on tests first"
```

This is the Phase 1 coordination model — file-based awareness via LIVE_STATE.json. No real-time injection, no vector similarity. Simple and effective for 2-4 agents.

---

---

## Recipe 1: Live Coordination Workflow (Phase 1.5)

**Scenario:** Two agents (Claude Code + OpenCode) are both refactoring the auth module.

### Timeline

```
t=0s      Claude-Code edits src/auth.ts (token validation logic)
          └─ Event: message.updated + tool.execute.after
          └─ Harness: Embed context, store in live_state.db

t=5s      OpenCode edits src/auth.ts (session refresh logic)
          └─ Event: message.updated + tool.execute.after
          └─ Harness: Embed context, query similar vectors
          └─ Similarity: 0.92 (high! both editing auth)
          └─ Check safeguards:
             ├─ Anti-echo: [SWARM:...] not in context ✓
             ├─ Rate limit: >45s since last injection ✓
             ├─ Confidence decay: 0.92 * e^(-0.03*0) = 0.92 ✓
             ├─ File scope: src/auth.ts == src/auth.ts ✓
             └─ Runaway check: 2 agents in <3 min ✓
          └─ Action: Queue injection

t=6s      (Compaction hook fires in OpenCode)
          └─ Event: experimental.session.compacting
          └─ Context usage: 45%
          └─ Harness: Send FULL injection (120 tokens)
             "[SWARM:claude-code:0.92] Detected: Both refactoring auth.
              Claude focuses on token validation. OpenCode handles session
              refresh. Consider splitting responsibilities to avoid duplication."
          └─ OpenCode receives injection in chat context
          └─ OpenCode acknowledges and adjusts approach

t=8s      Aider edits src/auth.test.ts (writing tests)
          └─ Event: tool.execute.after
          └─ Harness: Query similar vectors
          └─ File scope check: src/auth.test.ts ≠ src/auth.ts (different file)
          └─ Decision: File-scoped match (same directory)
          └─ Inject contextual suggestion about test implications

t=15s     (Session metrics update)
          └─ LIVE.md refresh:
             - Active patterns: 1 (auth refactoring)
             - Injections sent: 2
             - Echo loops: 0
             - Avg confidence: 0.92
```

### Harness Implementation (Pseudocode)

```typescript
// When OpenCode event arrives at t=5s
async function handleOpenCodeEvent(event: ToolExecuteEvent) {
  // 1. Store event
  await db.storeEvent(event);

  // 2. Embed last 8 turns of OpenCode's context
  const recentContext = await db.getRecentEvents(event.agentId, 8);
  const embedding = await embedder.embed(contextToString(recentContext));
  await db.storeVector({
    id: `vector-${event.id}`,
    vector: embedding,
    metadata: { agentId: event.agentId, file: event.toolArgs.filePath },
  });

  // 3. Query similar patterns (cosine similarity)
  const similarVectors = await db.search({
    vector: embedding,
    threshold: 0.88,
    limit: 10,
  });

  // 4. For each similar vector, check safeguards
  for (const similar of similarVectors) {
    if (similar.agentId === event.agentId) continue; // Skip self
    if (shouldSkipPattern(similar.metadata.content)) continue; // Anti-echo

    const timeSinceLastInjection = getTimeSince(similar.timestamp);
    if (timeSinceLastInjection < 45000) continue; // Rate limit

    const confidence = similar.similarity * Math.exp(-0.03 * minutesSince(similar));
    if (confidence < 0.88) continue;

    const files = extractFiles(similar.metadata);
    if (!filesMatch(files, event.toolArgs.filePath)) continue;

    if (isRunawayPattern(similar.metadata.topic)) continue;

    // 5. All safeguards passed → queue injection
    const injection = formatInjection({
      sourceAgent: event.agentId,
      targetAgent: similar.agentId,
      confidence,
      context: generateContext(similar),
    });

    await queueInjection({
      targetAgent: similar.agentId,
      content: injection,
      tokens: injection.length / 4, // Approximate
    });

    // 6. Update LIVE.md
    await liveWriter.addPatternDetection({
      patternId: similar.id,
      confidence,
      injectionQueued: true,
    });
  }
}

// At compaction time
async function handleCompactionHook(event: CompactionEvent) {
  const usage = event.contextTokens / event.maxContextTokens;

  const injection = await queuedInjections.pop();
  if (!injection) return;

  if (usage > 0.90) {
    // Micro-summarize (3 sentences, ~30 tokens)
    const summary = await summarizer.summarize(injection.content, 3);
    await sendToAgent(event.agentId, summary);
  } else if (usage > 0.70) {
    // Compressed (60 tokens)
    const compressed = await compressor.compress(injection.content, 60);
    await sendToAgent(event.agentId, compressed);
  } else {
    // Full (120 tokens)
    await sendToAgent(event.agentId, injection.content);
  }

  // Log the injection
  await db.logInjection({
    agentId: event.agentId,
    content: injection.content,
    tokens: injection.tokens,
    contextUsage: usage,
    timestamp: Date.now(),
  });
}
```

### Expected Outcomes

✅ Patterns detected: 2 (both on auth module, test scope)
✅ Injections sent: 2 (one to OpenCode, one to Aider)
✅ Echo cascades: 0
✅ Avg confidence: 0.92
✅ LIVE.md shows real-time coordination status

---

## Recipe 2: Pattern to Rule Learning (Phase 2)

**Scenario:** The "auth refactoring" pattern from Recipe 1 reappears in Sessions 2 & 3.

### Timeline (Sessions 1→2→3)

```
Session 1:
  t=0-15s   Auth module refactoring detected (confidence 0.92)
            Pattern stored in LIVE.md
            Session ends → LIVE.md archived (disposable)

Session 2 (next day):
  t=0-12s   Same pattern detected (confidence 0.91)
            Query sqlite-vec (Tier 2): Previous pattern found!
            Mark as "seen before" in LIVE.md
            Session ends

Session 3 (later):
  t=0-18s   Pattern detected again (confidence 0.93)
            Count: 3 sessions, all confidence >0.88
            Decision: PROMOTE to Tier 3 (mem0)

Promotion Trigger (after Session 3):
  1. Collect evidence from 3 sessions
  2. Extract structured fact via mem0:
     "When multiple agents refactor auth module simultaneously,
      split responsibilities: one handles tokens, one handles sessions."
  3. Create learned rule:
     - Rule ID: auth_split_responsibility
     - Confidence: 0.93 (consensus from 3 sessions)
     - Trigger: files=['src/auth.ts'] AND agents>1 AND window<10min
     - Action: Inject split suggestion
  4. Store in mem0 knowledge base
  5. Mark in sqlite-vec as "promoted"

Session 4+ (future):
  New refactoring attempt on auth module
  → Check learned rules (mem0 lookup)
  → Find auth_split_responsibility rule
  → Proactively inject suggestion (before overlap detected!)
  → Mark as "using learned rule: auth_split_responsibility"
```

### Implementation (Phase 2)

```typescript
// Python indexing pipeline
class PromotionWatcher {
  async checkForPromotion(): Promise<void> {
    // 1. Find patterns seen in ≥2 sessions
    const candidates = await db.searchPatterns({
      where: 'session_count >= 2',
      order: 'last_seen DESC',
      limit: 100,
    });

    for (const pattern of candidates) {
      // 2. Check confidence across all sessions
      const confidences = pattern.sessionConfidences; // [0.92, 0.91, 0.93, ...]
      const avgConfidence = confidences.reduce((a, b) => a + b) / confidences.length;

      if (avgConfidence < 0.93) continue; // Threshold

      // 3. Check for conflicting rules
      const conflicting = await mem0.checkConflict(pattern.topic);
      if (conflicting.length > 0) {
        await this.handleConflict(pattern, conflicting);
        continue;
      }

      // 4. Extract structured rule via mem0
      const transcript = await this.reconstructTranscript(pattern);
      const rule = await mem0.extract({
        transcript,
        type: 'coordination_rule',
        confidence: avgConfidence,
      });

      // 5. Store in mem0
      await mem0.add({
        id: `rule-${pattern.id}`,
        type: 'rule',
        content: rule.description,
        trigger_conditions: pattern.triggerConditions,
        action: pattern.suggestedAction,
        confidence: avgConfidence,
        learned_from_sessions: pattern.sessionIds,
      });

      // 6. Mark as promoted
      await db.updatePattern(pattern.id, {
        promoted_to_tier3: true,
        promoted_at: Date.now(),
        rule_id: `rule-${pattern.id}`,
      });

      logger.info(
        {
          pattern_id: pattern.id,
          rule_id: `rule-${pattern.id}`,
          confidence: avgConfidence,
          sessions: pattern.sessionIds,
        },
        'Pattern promoted to Tier 3 (learned rule)',
      );
    }
  }

  private async handleConflict(pattern: Pattern, conflicting: Rule[]): Promise<void> {
    // Flag for manual review
    await mem0.flagConflict({
      pattern_id: pattern.id,
      conflicting_rules: conflicting.map((r) => r.id),
      evidence: [pattern, ...conflicting],
      status: 'requires_developer_review',
    });

    logger.warn(
      {
        pattern_id: pattern.id,
        conflicting_count: conflicting.length,
      },
      'Conflict detected during promotion - requires review',
    );
  }

  private async reconstructTranscript(pattern: Pattern): Promise<string> {
    // Rebuild conversation from JSONL
    const sessions = pattern.sessionIds;
    let transcript = '';

    for (const sessionId of sessions) {
      const events = await db.getSessionEvents(sessionId);
      const relevant = events.filter((e) =>
        e.metadata.files?.some((f) => pattern.files.includes(f)),
      );

      for (const event of relevant) {
        transcript += `[${event.timestamp}] ${event.agentId}: ${event.content}\n`;
      }
    }

    return transcript;
  }
}
```

### Expected Outcomes

✅ Rule created: auth_split_responsibility (confidence 0.93)
✅ Stored in mem0 knowledge base
✅ Pattern marked as promoted in sqlite-vec
✅ Future sessions can use this rule proactively

---

## Recipe 3: Cross-Session Memory Retrieval (Phase 2+)

**Scenario:** New session starts. Developer mentions "auth module" → SWARM retrieves past patterns & rules.

### Timeline

```
Session N (new project, different repo):
  t=0s    Developer: "Let me refactor the authentication logic"
          Harness: Route to mem0 search
          mem0: Query "authentication logic refactoring"
          Results:
            - rule-auth_split_responsibility (confidence 0.93)
            - 3 past sessions (sessions 1, 2, 3)
            - Evidence: "splitting reduces duplication by 180 lines"

  t=1s    Present to developer:
          "🔍 Past coordination rule found:
           auth_split_responsibility (93% confidence)
           When multiple agents refactor auth:
           - One handles token validation
           - One handles session refresh
           → Reduced code duplication by 180 lines in past sessions
           Use this rule? [Yes] [No] [Custom]"

  t=2s    Developer clicks [Yes]
          Harness: Activate rule for this session
          When rules detected, proactively inject suggestions
```

### MCP Tool Integration

```typescript
// MCP server exposed to agents
server.tool('search_patterns', {
  description: 'Search coordination patterns from past sessions',
  inputSchema: {
    type: 'object',
    properties: {
      query: { type: 'string' }, // "auth refactoring", "test generation", etc.
      confidence_min: { type: 'number', default: 0.85 },
    },
  },
  handler: async (input) => {
    // Search sqlite-vec (Tier 2)
    const vectorResults = await db.search({
      query: input.query,
      threshold: input.confidence_min,
      limit: 10,
    });

    // Search mem0 (Tier 3)
    const ruleResults = await mem0.search({
      query: input.query,
      type: 'rule',
      limit: 10,
    });

    return {
      patterns: vectorResults.map((r) => ({
        id: r.id,
        topic: r.metadata.topic,
        confidence: r.similarity,
        sessions: r.metadata.sessionIds,
        evidence: r.metadata.excerpt,
      })),
      rules: ruleResults.map((r) => ({
        id: r.id,
        description: r.content,
        confidence: r.confidence,
        trigger_conditions: r.triggerConditions,
        learned_from: r.learnedFromSessions,
      })),
    };
  },
});

server.tool('activate_rule', {
  description: 'Activate a learned coordination rule for current session',
  inputSchema: {
    type: 'object',
    properties: {
      rule_id: { type: 'string' },
      custom_tweaks: { type: 'object', optional: true },
    },
  },
  handler: async (input) => {
    const rule = await mem0.getRule(input.rule_id);

    const activation = {
      rule_id: input.rule_id,
      session_id: getCurrentSessionId(),
      trigger_conditions: rule.trigger_conditions,
      action: rule.action,
      custom_tweaks: input.custom_tweaks || {},
      activated_at: Date.now(),
    };

    await db.logRuleActivation(activation);

    return {
      success: true,
      rule_id: input.rule_id,
      message: `Activated rule: ${rule.description}`,
    };
  },
});
```

### Expected Outcomes

✅ Past patterns searchable via MCP
✅ Learned rules available for reuse
✅ Proactive suggestions based on history
✅ Developers can choose to activate/ignore

---

## Recipe 4: Debugging a Coordination Failure

**Scenario:** SWARM sent an injection, but the agent didn't use it. Why?

### Investigation Workflow

```
Symptom: Agent seemed uncoordinated with its partner
Time: Session N, between t=120-180s

Step 1: Check LIVE.md
  → Pattern detected? Yes (confidence 0.91)
  → Injection queued? Yes (2 attempts)
  → Status: "Acknowledged by agent"

Step 2: Check logs
  command: tail logs/swarm.log | grep "session-N"

  Find:
    [t=125s] Pattern detected: auth refactoring (0.91)
    [t=125s] Safeguard checks: PASS
    [t=125s] Injection queued: msg-456
    [t=126s] Compaction hook fired, context=45%
    [t=126s] Sent FULL injection (120 tokens)
    [t=126s] Awaiting acknowledgment...
    [t=160s] NO ACKNOWLEDGMENT - timeout after 30s
    [t=160s] Pattern still active, requeue? [decision made: skip, rate limit approaching]

Step 3: Check agent's JSONL
  Look at agent's response after t=126s
  → Agent received injection? Check for [SWARM:...]
  → Agent acknowledged? Look for "thank you" or pattern application
  → Found: Agent got distracted, focused on different task
  → Decision: Injection was sent, agent just wasn't paying attention

Step 4: Verify with metrics
  await metrics.getInjectionMetrics({
    session_id: 'session-N',
    pattern_id: 'auth-refactor',
  });

  Result:
    sent: 2
    acknowledged: 1
    applied: 0
    reason_not_applied: "agent distracted"

Root cause: Injection was correct, agent was working on unrelated task

Solution:
  - No code bug
  - Tuning: Increase confidence threshold for this agent
  - Or: Accept that not all injections will be used (ok!)
```

### Using LIVE.md for Debugging

```markdown
# LIVE.md (Session N)

## Pattern: auth refactoring
- Confidence: 0.91
- Detected: t=125s
- Status: INJECTED

### Timeline
- 12:45:00 Claude-Code: editing src/auth.ts (token validation)
- 12:45:05 OpenCode: editing src/auth.ts (session refresh)
  └─ MATCH DETECTED (0.91) → Pattern recognized
  └─ Anti-echo: PASS | Rate limit: PASS | File scope: PASS
  └─ Runaway: PASS (2 agents) | Confidence: 0.91 → INJECT
- 12:45:06 Compaction hook: context 45%, FULL injection (120 tokens)
- 12:45:06 → OpenCode: "[SWARM:Claude-Code:0.91] Detected: Both refactoring auth..."
- 12:46:00 OpenCode: Did not apply injection (working on error tests instead)

## Metrics
- Active agents: 2 (Claude-Code, OpenCode)
- Patterns detected: 1
- Injections sent: 1
- Injections applied: 0
- Confidence avg: 0.91
- Echo loops: 0 ✅
- Memory: 2.3MB
```

---

## Recipe 5: Deployment Day Checklist (Phase 1 Release)

**Scenario:** First SWARM release to staging.

### Pre-Deployment Verification

```bash
# 1. Code quality
make lint              # ESLint must pass
make typecheck         # TypeScript strict must pass
make test              # Unit tests: 85%+ coverage
make test:integration  # Integration tests: all pass
make test:stress       # 6-agent scenario: 0 echo cascades

# 2. Performance
make benchmark         # Latency p95 < 500ms
make benchmark:memory  # Memory < 512MB

# 3. Docker
make docker:build      # Build image
du -sh build/image     # Verify < 100MB

# 4. Kubernetes
helm lint impulse-helm/
helm template impulse-helm/ > /tmp/manifest.yaml
kubectl validate -f /tmp/manifest.yaml

# 5. Smoke tests
./scripts/deploy-staging.sh
./scripts/smoke-tests.sh  # Wait 2 min, verify metrics

# 6. Sign-off
✅ All checks pass
✅ LIVE.md working (shows real-time metrics)
✅ Dashboard responsive (<100ms)
✅ Logs structured + ingested into Loki

→ Deploy to production
```

---

## Framework Cross-Reference

| Recipe | Frameworks Used | Key Files |
|--------|-----------------|-----------|
| 1 (Live Coordination) | Phase 1.5, Testing | ../phases/PHASE1.5-COORDINATION.md, SYNTHETIC-TESTING-GUIDE.md |
| 2 (Learning) | Phase 2, mem0 | ../phases/PHASE2-PERSISTENCE.md, DEPLOYMENT-FRAMEWORK.md |
| 3 (Retrieval) | Phase 2+, MCP | ../phases/PHASE2-PERSISTENCE.md, ../vision/CLI-ARCHITECTURE.md |
| 4 (Debugging) | Observability | DEPLOYMENT-FRAMEWORK.md § Observability |
| 5 (Deployment) | CI/CD, All | DEPLOYMENT-FRAMEWORK.md, ../phases/PHASE1-CHECKLIST.md |

---

## How to Use This Document

1. **For developers starting Phase 1:** Read Recipe 1 (live coordination)
2. **For debugging:** Read Recipe 4 (investigation workflow)
3. **For Phase 2 planning:** Read Recipes 2-3 (learning & retrieval)
4. **For deployment:** Read Recipe 5 + `DEPLOYMENT-FRAMEWORK.md`
5. **For teaching:** Walk through recipes 1→5 sequentially

---

## References

- ../phases/PHASE1.5-COORDINATION.md
- ../phases/PHASE2-PERSISTENCE.md
- DEPLOYMENT-FRAMEWORK.md
- SYNTHETIC-TESTING-GUIDE.md
- ../vision/CLI-ARCHITECTURE.md

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Developer Onboarding_
