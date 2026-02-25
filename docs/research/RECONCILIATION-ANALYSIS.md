---
status: active
phase: all
audience: builder
tags: [research, reconciliation, specs]
last_updated: 2026-02-20
---

# Reconciliation Analysis: Cross-Model Consensus & Divergences

> **Status:** Meta-Analysis | **Based On:** GPT-5.2 Thinking, Claude Opus 4.6 Thinking, Gemini 3 Pro
> **Purpose:** Bridge gaps between `ai_coding_impulse_prd_spec_v1.json`, `docs/archive/SPEC-v1.1.md`, and `docs/phases/IMPLEMENTATION-ROADMAP.md`
> **Date:** 2026-02-20

---

## Executive Summary

Three advanced reasoning models (GPT-5.2 Thinking, Claude Opus 4.6 Thinking, Gemini 3 Pro) analyzed the Impulse specification and framework documents. **High-confidence findings** show:

✅ **Consensus:**
- The JSON PRD is production-grade (unusually dense with success metrics, data models, test pyramid, risk register)
- Phased roadmap is coherent (Phase 0→4 progression makes sense)
- Success metrics are measurable (memory footprint, retrieval accuracy, switch latency)

⚠️ **Divergences:**
- **Sequencing conflict:** Docs-first alignment (GPT-5.2, Claude) vs risk-first build (Gemini)
- **Testing strategy:** Skepticism about terminal snapshot testing (GPT-5.2) vs synthetic event replay (Gemini)
- **Feature scope:** Vibe Control in Phase 1 vs Phase 2+ (Claude Opus insight)

🎯 **Unique discoveries:**
- **Source-of-truth hierarchy:** JSONL → embeddings cache → Mem0 (prevents data drift)
- **Synthetic SWARM testing:** Generate coordination events without LLM calls (fast, repeatable, CI-friendly)
- **Core vs optional services:** Separate packaging to protect "lightweight" goal

---

## Section 1: Where All Models Agree

### The JSON PRD is an All-in-One Artifact

All three models characterize `ai_coding_impulse_prd_spec_v1.json` as exceptionally dense:

**It contains:**
- Executive summary + strategic goals
- Technical architecture (5-tier memory system, 4 phases)
- Data models (MCP tool schemas, vector dimensions, embedding models)
- Success metrics (KPIs: <400MB RAM, <80MB binary, 95% retrieval accuracy, <500ms project switch)
- Test pyramid (unit, integration, stress, E2E)
- Dependency manifest (all tools, versions, licenses)
- Risk register (identified risks + mitigations)
- Evaluation criteria (how to measure success)

**Why this matters:**
- You don't need to invent additional structure. The JSON is already structured as PRD + SPEC + TESTPLAN + EVAL.
- Your `docs/archive/SPEC-v1.1.md` and `docs/phases/IMPLEMENTATION-ROADMAP.md` are derivations of the JSON, not competitors.
- Reconciliation is a mapping/cross-reference exercise, not a contradiction resolution.

### The Phased Roadmap is Coherent

```
Phase 0 (Done): Foundations
├─ Terminal stack (Ghostty, Zellij, mise)
├─ Agent integration (OpenCode primary, Claude Code via JSONL)
└─ Installation of reference tools (claude-historian-mcp, mem0)

Phase 1 (Core Harness): SWARM multi-agent coordination
├─ OpenCode plugin hooks
├─ sqlite-vec event + pattern storage
├─ LIVE.md cache (view layer)
└─ Zellij status bar + basic dashboard

Phase 1.5 (Live Coordination): Real-time pattern injection
├─ Embedding cache + runaway detection
├─ Anti-echo, rate limiting, file scoping
└─ Token budget response (compress when context full)

Phase 2 (Cross-Session Persistence): RAG + memory tiers
├─ Claude Code JSONL watcher
├─ Python indexing pipeline (chunks → embeddings → sqlite-vec)
├─ mem0 fact extraction (decisions, rules, preferences)
└─ MCP server exposing retrieval to agents

Phase 3 (Advanced UI): Time Machine, semantic routing, Aider
Phase 4 (Distribution): Packaging, single-binary, optional Tauri shell
```

**All models agree:** This sequence makes sense. You're not overcommitting in Phase 1 (no Mem0, no UI polish). You're validating the riskiest technical dependency (sqlite-vec + embeddings) before adding complexity.

### Success Metrics Are Measurable

The JSON explicitly defines KPIs:

| Metric | Target | Measurement |
|--------|--------|-------------|
| Memory footprint | <400MB RAM | `top` / Task Manager |
| Binary size | <80MB | `du -sh /usr/local/bin/impulse` |
| Retrieval accuracy | 95% | Recall@10 on test corpus (Needle-in-Haystack) |
| Project switch latency | <500ms | Benchmark: load history, search, switch UI |
| Multi-agent overhead | <100ms per injection | End-to-end: detect → format → inject |
| Echo loop prevention | 0 cascades >2 hops | 6-agent stress test |

**All models agree:** These are testable. GPT-5.2 and Gemini both propose concrete measurement approaches (e.g., Needle-in-Haystack for retrieval, synthetic event replay for coordination).

---

## Section 2: Key Divergences (& Resolutions)

### Divergence 1: Docs-First vs Risk-First Sequencing

| Position | Advocates | Rationale |
|----------|-----------|-----------|
| **Docs-First** | GPT-5.2, Claude Opus | Reconcile JSON ↔ SPEC-v1.1 ↔ ROADMAP before building. Prevents wasted implementation effort. Aligns team on acceptance criteria. |
| **Risk-First** | Gemini 3 Pro | Implement Phase 1 Priority 1 (database layer) immediately. sqlite-vec + schema is the riskiest unknown and will validate/invalidate the entire approach. |

**Recommendation: DO BOTH (Parallel Streams)**

```
Stream A (Days 1-2): Reconciliation
├─ Create traceability matrix: JSON KPI → feature → acceptance criteria
├─ Map JSON phases → ROADMAP priorities
├─ Identify "core vs optional" services (see next section)
└─ Finalize STEWARD contract (operational definition)

Stream B (Days 1-10): Database Risk Validation
├─ Implement PHASE1-PRIORITY1: sqlite-vec schema + event store
├─ Test cosine similarity queries (latency, accuracy)
├─ Validate vector insertion performance (<50ms per vector)
└─ DECISION GATE: If sqlite-vec latency >200ms, reconsider approach
```

**Why parallel?** Reconciliation prevents scope creep. DB validation proves the foundation works before committing to 4 weeks of implementation.

---

### Divergence 2: Test Automation Strategy

| Position | Advocates | Concern |
|----------|-----------|---------|
| **Skeptical of snapshot testing** | GPT-5.2 | Terminal UI snapshot diffs (PNG comparisons) are flaky at scale. Zellij multiplexing adds non-determinism. Maintenance burden high. |
| **Prefers synthetic replay** | Gemini 3 Pro | Generate deterministic SWARM events (10 events, 50 events, 100 events). Replay coordination logic. Assert on DB state, not UI. Fast, repeatable, CI-friendly. |
| **Neutral, pyramid approach** | Claude Opus | Accept both. Heavy investment in logic tests + synthetic replay. Keep UI testing to golden-path smoke tests (bash/expect). |

**Recommendation: Synthetic Event Harness**

```typescript
// Synthetic event replay (no LLM calls, no Zellij)
class CoordinationSimulator {
  async runScenario(agents: number, events: number): Promise<SimResult> {
    const harness = new Harness();

    for (let i = 0; i < agents; i++) {
      // Generate synthetic events (edit file, execute tool, etc.)
      const synthEvents = this.generateEvents(agents, events);

      for (const event of synthEvents) {
        await harness.ingest(event);
      }
    }

    // Assert on:
    // ├─ Pattern detection (count, confidence)
    // ├─ Injection queuing (rate limit enforced)
    // ├─ Echo prevention (no cascades >2 hops)
    // ├─ Latency (pattern detection <500ms)
    // └─ Memory (working set <5MB)

    return harness.metrics();
  }
}
```

**Benefits:**
- No LLM token burn (save $)
- Deterministic (same seed → same outcome)
- Fast (50 events in <100ms)
- CI-friendly (no external dependencies)

**Scope:** Use for Phase 1 + 1.5 validation. Phase 2+ add E2E with real Claude Code JSONL.

---

### Divergence 3: Feature Scope for Phase 1

| Item | JSON/PRD | Claude Opus Recommendation | Final Recommendation |
|------|----------|---------------------------|----------------------|
| Multi-project switching | Phase 1 | Phase 2+ (defer) | DEFER: Focus on single-project coordination first |
| Vibe Control (themes) | Phase 1 | Phase 2+ (polish) | DEFER: Phase 2+ (not core to coordination) |
| Time Machine (float pane) | Phase 1 | Phase 1 (but minimal) | PHASE 1.5 (MVP version, full in Phase 2) |
| Semantic routing (not keyword) | Phase 2 | Phase 2 | KEEP (Phase 2) |
| Learning system (mem0) | Phase 2 | Phase 2 | KEEP (Phase 2) |

**Action:** Revise `docs/phases/IMPLEMENTATION-ROADMAP.md` Phase 1 to remove "Vibe Control" and "Multi-project switching." These are Phase 2+ enhancements, not core Phase 1 requirements.

---

## Section 3: Unique Discoveries Worth Implementing

### Discovery 1: Source-of-Truth Hierarchy (GPT-5.2)

**The Problem:**
Without explicit governance, you'll accumulate data in multiple stores (JSONL, sqlite-vec, mem0, LIVE.md, Hot/Warm/Cold tiers). These can diverge:
- "Transcript says Claude + OpenCode were on auth module"
- "Memory says only Claude was there"
- "Cache was cleared but LIVE.md still shows old state"

→ Debugging nightmare. Which is correct?

**The Solution: Immutable Hierarchy**

```
Level 1 (Authority):    Claude JSONL transcript
  ↓ derive, never mutate
Level 2 (Cache):        sqlite-vec embeddings + patterns (regenerable)
  ↓ synthesize with ≥0.93 confidence
Level 3 (Semantic):     mem0 facts, rules, preferences
  ↓ personalize
Level 4 (Session):      Hot/Warm/Cold tiers (ephemeral)
  ↓ delete on session end unless promoted
Level 5 (Durable):      Cross-session decisions/agreements
```

**Invariants:**
1. **Read upstream when doubting downstream.** If LIVE.md contradicts JSONL, trust JSONL.
2. **Never mutate upstream.** If you fix a fact in mem0, verify it aligns with JSONL first.
3. **Regenerate from truth on conflict.** If sqlite-vec cache is stale, re-embed from JSONL.

**Implementation:** Add this to `docs/phases/PHASE2-PERSISTENCE.md` § "Architectural Invariant" ✓ (already done)

---

### Discovery 2: Synthetic SWARM Testing (Gemini 3 Pro)

**The Benefit:**
Test Phase 1.5 coordination (anti-echo, rate limiting, runaway detection) without:
- Waiting for real agents
- Burning LLM tokens
- Dealing with network delays
- Flaky Zellij multiplexing

**The Pattern:**

```typescript
// Generate 6 concurrent agents, 100 events each, same file
const scenario = {
  agents: 6,
  events_per_agent: 100,
  file: 'src/auth.ts',
  start_time: now(),
  event_rate: 1000, // ms between events
};

const result = await simulator.runScenario(scenario);

// Assert:
assert.equal(result.patterns_detected, expect); // Should be 5-8
assert.equal(result.echo_cascades, 0);          // Must be 0
assert.equal(result.injections_sent, expect);   // Should be 12-15
assert(result.latency_p99 < 500);               // <500ms detection
```

**Scope for Phase 1.5:**
- Unit test all safeguards (anti-echo, rate limit, runaway)
- Integration test 6-agent scenario (no echoes)
- Stress test (1000+ events, verify memory stays <5MB)

**Implementation:** Add `CoordinationSimulator` to harness test utilities ✓ (ready for Priority 3)

---

### Discovery 3: Core vs Optional Services (GPT-5.2 + Claude)

**The Problem:**
Impulse can bloat. TypeScript harness (Bun) + Rust plugins (WASM) + Python pipeline (embeddings) + MCP server + optional mem0. This violates the "<80MB binary" goal.

**The Solution: Modular Packaging**

```
CORE (Required, Phase 1):
├─ SWARM harness (TypeScript + Bun)
├─ Zellij plugin (Rust)
└─ LIVE.md writer
Total: ~20MB

TIER 2 (Optional, Phase 2):
├─ Python indexing pipeline
├─ sqlite-vec (C extension, 3MB)
└─ MCP server
Total: +15MB (optional)

TIER 3 (Optional, Phase 3+):
├─ mem0 integration
├─ OpenAI API integration
└─ Advanced UI
Total: +10MB (optional)
```

**Distribution Strategy:**
```bash
# Minimal install (10MB, Phase 1 only)
brew install impulse --minimal

# Full install (45MB, all phases)
brew install impulse

# Custom
brew install impulse --with-indexing --with-memory
```

**Action:** Add to `docs/phases/PHASE1-CHECKLIST.md` § "Packaging" (ensures Phase 1 stays lean)

---

## Section 4: Recommended Immediate Actions

### Action 1: Create Traceability Matrix (Days 1-2)

Map every KPI in the JSON to:
1. **Feature** (e.g., "pattern detection")
2. **Acceptance Criteria** (e.g., "<500ms latency")
3. **Test** (e.g., "6-agent stress test")
4. **Metrics** (e.g., "measure with Stopwatch")

**Output:** Excel or Markdown table in `docs/TRACEABILITY-MATRIX.md`

**Why:** Prevents ambiguity. When Phase 1 is done, you can verify every KPI is met.

---

### Action 2: Implement PHASE1-PRIORITY1 (Days 1-10)

Follow `docs/phases/IMPLEMENTATION-ROADMAP.md` Priority 1 exactly:

```
[ ] 1. Database schema (sqlite-vec + events table)
[ ] 2. Event insertion + query tests
[ ] 3. Vector search (cosine similarity)
[ ] 4. Latency baseline (<50ms insert, <200ms search)
[ ] 5. Decision gate: If latency >200ms, escalate
```

**Why:** sqlite-vec is the riskiest unknown. If it doesn't work, the entire approach fails. Knowing this in Days 1-10 saves weeks of wasted effort.

---

### Action 3: Operationalize SWARM Contract (Days 2-3)

Document the "Harness-to-Agent" contract:

```typescript
/**
 * SWARM Harness Contract
 *
 * What the harness commits to:
 * - Max 1 injection per agent per 45s
 * - Max 120 tokens per injection
 * - Provenance tagged: [SWARM:source:confidence]
 * - Anti-echo: skip if pattern contains [SWARM: prefix
 * - Confidence decay: e^(-0.03 * minutes_old)
 *
 * What agents should expect:
 * - Injections arrive via experimental.session.compacting hook
 * - Content is context-aware (file-scoped)
 * - They can ignore/accept autonomously
 */
```

**Why:** Prevents silent assumptions. If agents don't understand what SWARM does, they can't use it effectively.

---

### Action 4: Revise Phase 1 Scope (Days 1-2)

Remove from Phase 1:
- ❌ Vibe Control / themes (Phase 2+)
- ❌ Multi-project switching (Phase 2+)
- ✅ Keep: Core coordination, LIVE.md, basic dashboard, pattern detection, injection

**Why:** Protects the Phase 1 timeline. Core coordination is what matters. Polish comes later.

---

## Section 5: Final Recommendation Summary

| Dimension | Recommendation | Why |
|-----------|-----------------|-----|
| **Sequencing** | Parallel: Reconciliation (Days 1-2) + DB validation (Days 1-10) | De-risks both alignment and technology |
| **Testing** | Synthetic event replay, minimal UI snapshot tests | Fast, repeatable, CI-friendly |
| **Feature Scope** | Defer Vibe Control + Multi-project to Phase 2+ | Keeps Phase 1 lean and focused |
| **Data Governance** | Implement source-of-truth hierarchy (Level 1→5) | Prevents debugging nightmares |
| **Packaging** | Core/Tier2/Tier3 split (keep core <20MB) | Protects lightweight goal |
| **First Priority** | Implement sqlite-vec schema + latency tests | Validates riskiest dependency |

---

## References

- JSON PRD: `ai_coding_impulse_prd_spec_v1.json`
- Specifications: `docs/archive/SPEC-v1.1.md`
- Implementation: `docs/phases/IMPLEMENTATION-ROADMAP.md`
- Phase 2: `docs/phases/PHASE2-PERSISTENCE.md`
- Traceability: `docs/TRACEABILITY-MATRIX.md` (to be created)

---

_Created: 2026-02-20 | Cross-Model Analysis | Ready for Decision_
