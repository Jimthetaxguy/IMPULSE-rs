---
title: Phase 2 Persistence
description: Cross-session persistence and memory tiers for persistent agent memory
version: '1.0'
updated: 2026-02-20
type: specification
category: phases
phase: phase2
status: active
audience: builders
tags: [phase, persistence, sqlite]
---

# Phase 2: Cross-Session Persistence & Memory Tiers

> **Version:** 1.0 | **Status:** Design | **Updated:** 2026-02-20
> **Duration:** 3-4 weeks | **Dependencies:** Phase 1 complete

---

## Overview

Phase 2 upgrades SWARM from **session-local coordination** (Phase 1) to **persistent, cross-session memory** that learns and improves coordination strategies across time.

**Core Goal:** Agents can reference decisions made in previous sessions, retrieve past coordination patterns, and apply learned rules across projects.

---

## Architectural Invariant: Source-of-Truth Hierarchy

All data flows follow this immutable priority:

```
LEVEL 1 (Immutable Truth):   Claude JSONL transcripts
                              ↓ (derive, never modify)
LEVEL 2 (Derived Cache):      sqlite-vec embeddings + patterns
                              ↓ (synthesize, with 0.93+ confidence)
LEVEL 3 (Semantic Cache):     mem0 facts, rules, preferences
                              ↓ (personalize)
LEVEL 4 (Working Memory):     Hot/Warm/Cold tiers (session-local)
                              ↓ (ephemeral)
LEVEL 5 (Durable Facts):      Cross-session decisions, agreements
```

**Invariant Contract:**

- LEVEL 1 is read-only and authoritative
- LEVEL 2 is regenerable from LEVEL 1 (no permanent mutations)
- LEVEL 3 is synthesized from LEVEL 2 with explicit confidence (≥0.93)
- LEVELS 4-5 are deleted on session end unless promoted via governance

**Benefit:** If a discrepancy appears, trace backward through the hierarchy to find the source. Never overwrite higher levels with lower ones.

---

## Integration Points

### 1. Claude Code JSONL Watcher (Read-Only)

OpenCode has plugin hooks; Claude Code does not. Instead, watch the JSONL file written to `~/.claude/projects/*/memory/` (or equivalent).

```typescript
// Phase 2: JSONL watcher (async, non-blocking)
class ClaudeCodeWatcher {
  private watched = new Map<string, ClaudeCodeSession>();

  async watch(projectPath: string): Promise<void> {
    const jsonlPath = this.discoverJSONLPath(projectPath);

    // Watch file size changes
    fs.watchFile(jsonlPath, async (curr, prev) => {
      if (curr.size > prev.size) {
        const newLines = await this.tailFile(jsonlPath, curr.size - prev.size);
        for (const line of newLines) {
          const event = this.parseJSONL(line);
          await this.harness.ingestClaudeCodeEvent(event);
        }
      }
    });
  }

  private discoverJSONLPath(projectPath: string): string {
    // Check standard locations
    return (
      path.join(projectPath, '.claude', 'projects', '*', 'memory', '*.jsonl') ||
      path.join(projectPath, '.claude', 'memory.jsonl')
    );
  }
}
```

**Polling Strategy:**

- Check file modification time every 500ms
- Tail only new lines (track file offset)
- Parse JSONL incrementally (one event at a time)
- Backpressure: queue events, process asynchronously

**Error Handling:**

- If file is locked (Claude Code writing), retry with exponential backoff
- If event parsing fails, log and skip (don't block the stream)
- If harness is overloaded, buffer up to 1000 events before dropping oldest

---

### 2. Tier 2: Python Indexing Pipeline

Convert raw JSONL → chunks → embeddings → sqlite-vec.

```python
# Phase 2: Indexing pipeline (Python 3.12+)
class IndexingPipeline:
    def __init__(self, jsonl_path: str, db_path: str):
        self.jsonl_path = jsonl_path
        self.db = sqlite3.connect(db_path)
        self.embedder = SentenceTransformer('all-MiniLM-L6-v2')  # 384-dim
        self.chunk_size = 512  # tokens
        self.chunk_overlap = 64  # tokens

    async def index_session(self, session_id: str) -> IndexResult:
        """
        Pipeline:
        1. Parse JSONL
        2. Chunk by turn (512-token max)
        3. Embed chunks
        4. Store vectors in sqlite-vec
        5. Update metadata
        """
        events = self.parse_jsonl(self.jsonl_path)
        chunks = self.chunk_events(events)

        vectors = []
        for i, chunk in enumerate(chunks):
            embedding = self.embedder.encode(chunk.text)
            vectors.append({
                'id': f'{session_id}-chunk-{i}',
                'vector': embedding,
                'metadata': {
                    'session_id': session_id,
                    'chunk_index': i,
                    'timestamp': chunk.timestamp,
                    'source_file': chunk.source_file,
                    'text': chunk.text[:200],  # Preview
                },
            })

        # Upsert into sqlite-vec
        self.upsert_vectors(vectors)

        return IndexResult(
            session_id=session_id,
            chunks_indexed=len(chunks),
            vectors_stored=len(vectors),
            status='success',
        )

    def chunk_events(self, events: list) -> list:
        """
        Chunk by turn, not by token boundary.
        Each turn is one chunk, unless >512 tokens (then split).
        """
        chunks = []
        current_chunk = []
        current_tokens = 0

        for event in events:
            tokens = len(event['content'].split())

            if current_tokens + tokens > self.chunk_size and current_chunk:
                # Save current chunk
                chunks.append(self.build_chunk(current_chunk))
                current_chunk = [event]
                current_tokens = tokens
            else:
                current_chunk.append(event)
                current_tokens += tokens

        if current_chunk:
            chunks.append(self.build_chunk(current_chunk))

        return chunks
```

**Embedding Model Selection:**

- `all-MiniLM-L6-v2` (22MB, 384-dim): Fast, locally hosted
- Alternative: OpenAI `text-embedding-3-small` (API, 1536-dim): Better quality, costs $$
- Decision: Use local for MVP (Phase 2), optional OpenAI for Phase 3+

**Indexing Schedule:**

- On-demand: User manually runs `swarm index-session <session-id>`
- Periodic: Background job every 6 hours for archived sessions
- Lazy: Index on first retrieval if not yet indexed

---

### 3. Tier 3: mem0 Integration (Fact Extraction)

mem0 extracts structured facts from unstructured conversation.

```python
# Phase 2: mem0 fact extraction
from mem0 import Memory

class Mem0Extractor:
    def __init__(self, db_path: str):
        self.memory = Memory.from_config({
            "llm": {
                "provider": "openai",
                "config": {
                    "model": "gpt-4o-mini",  # Cheap, fast
                    "api_key": os.getenv('OPENAI_API_KEY'),
                },
            },
            "embedder": {
                "provider": "openai",
            },
            "vector_store": {
                "provider": "sqlite",
                "config": {
                    "db_path": db_path,
                },
            },
        })

    async def extract_from_session(self, session_id: str, transcript: str) -> ExtractResult:
        """
        Extract facts, decisions, preferences, patterns from transcript.

        Filters:
        - Decisions (confidence ≥ 0.93)
        - Rules learned (used ≥ 2x)
        - Disagreements (flagged for manual review)
        - Preferences (agent-specific, user-specific)
        """
        # Add to mem0
        self.memory.add(
            messages=[
                {
                    "role": "system",
                    "content": f"Extract facts from this coordination session: {session_id}",
                },
                {
                    "role": "user",
                    "content": transcript,
                },
            ],
            metadata={
                "session_id": session_id,
                "type": "coordination",
            },
        )

        # Retrieve high-confidence facts
        facts = self.memory.search(
            query=f"decisions and patterns from session {session_id}",
            limit=50,
        )

        # Filter by confidence
        high_confidence = [f for f in facts if f.get('confidence', 0) >= 0.93]

        return ExtractResult(
            session_id=session_id,
            facts_extracted=len(high_confidence),
            decisions=[f for f in high_confidence if f.get('type') == 'decision'],
            rules=[f for f in high_confidence if f.get('type') == 'rule'],
            preferences=[f for f in high_confidence if f.get('type') == 'preference'],
        )
```

**mem0 Fact Categories:**
| Category | Example | Confidence Threshold | TTL |
|----------|---------|----------------------|-----|
| Decisions | "Split auth into token + session modules" | 0.93+ | Permanent |
| Rules | "When >2 agents work on same file, suggest split" | 0.93+ | 30 days (or learned ≥3x) |
| Disagreements | "Claude wants JWT-only, OpenCode wants session tokens" | 0.85+ | 7 days |
| Preferences | "User prefers semantic routing over keyword" | 0.90+ | 90 days |

---

## Promotion Flow: Live → Tier 2 → Tier 3

### Tier 1 → Tier 2 (Embedding)

```
Preconditions:
├─ Pattern detected in live_state.db (Phase 1)
├─ Used ≥2 times in single session, OR
└─ Used in ≥2 different sessions

Action:
├─ Extract context from JSONL
├─ Chunk and embed
└─ Store in sqlite-vec with metadata

Retention: 24 hours after session close
Expiration: Delete if not promoted to Tier 3 within 24h
```

### Tier 2 → Tier 3 (Learning)

```
Preconditions:
├─ Pattern stored in sqlite-vec for ≥2 sessions, AND
├─ Confidence ≥0.93 (manual review + voting), AND
└─ No conflicting rules (if exists, flag for arbitration)

Action:
├─ Extract structured fact via mem0
├─ Add to mem0 knowledge base
├─ Mark in sqlite-vec as "promoted"
└─ Trigger rule evolution

Retention: Permanent (subject to TTL in mem0)
Governance: Requires ≥2 agents in same session to vote "yes"
```

### Decision Arbitration (Tier 3 Conflict)

```
If conflicting rules exist (e.g., "use JWT" vs "use sessions"):

1. Flag both rules with "conflict" status
2. Store evidence (which agents, when, context)
3. Trigger manual review:
   "Two learned rules conflict. Review and choose one."
4. Winner becomes the authoritative rule
5. Loser is archived with "overridden" status
```

---

## Performance Targets (Phase 2)

| Operation                   | Target                     | Measurement                         |
| --------------------------- | -------------------------- | ----------------------------------- |
| JSONL polling               | <50ms                      | Check file modification time        |
| Event parsing               | <10ms per event            | Parse JSONL line incrementally      |
| Chunking                    | <100ms per 1000 tokens     | Grouping + tokenization             |
| Embedding (local)           | <200ms per chunk           | sentence-transformers on CPU        |
| Embedding (API)             | <500ms per chunk           | OpenAI API round-trip               |
| Vector insert (sqlite-vec)  | <50ms per 384-dim vector   | Upsert into virtual table           |
| Fact extraction (mem0)      | <2s per session            | LLM call (gpt-4o-mini)              |
| Session indexing end-to-end | <30s for 100-event session | JSONL → chunks → embeddings → DB    |
| Cross-session retrieval     | <200ms                     | sqlite-vec cosine similarity search |

---

## Testing Strategy

### Unit Tests

```typescript
describe('Tier 2: Python Indexing', () => {
  it('should chunk events respecting overlap', () => {
    const events = createMultipleEvents(50);
    const chunks = chunkEvents(events, { size: 512, overlap: 64 });
    expect(chunks[0].text).toContain('event 0 content');
    expect(chunks[1].text).toContain('event 30 content'); // Overlap
  });

  it('should embed chunks and store in sqlite-vec', async () => {
    const chunks = [{ text: 'test chunk', timestamp: now }];
    const result = await indexPipeline.indexSession('session-1', chunks);
    expect(result.vectors_stored).toBe(1);
    const retrieved = await db.search({ vector: testVector, limit: 10 });
    expect(retrieved).toHaveLength(≥1);
  });
});

describe('Tier 3: mem0 Extraction', () => {
  it('should extract decisions with confidence ≥0.93', async () => {
    const transcript = createSessionTranscript('decision: split auth module');
    const result = await mem0.extractFromSession('session-1', transcript);
    expect(result.decisions).toHaveLength(≥1);
    expect(result.decisions[0].confidence).toBeGreaterThanOrEqual(0.93);
  });

  it('should handle conflicting rules gracefully', async () => {
    const rule1 = createRule('JWT-only auth', 0.93, 'Claude');
    const rule2 = createRule('Session + JWT auth', 0.92, 'OpenCode');
    const conflict = await mem0.checkConflict([rule1, rule2]);
    expect(conflict.status).toBe('conflict_detected');
    expect(conflict.requiresReview).toBe(true);
  });
});
```

### Integration Tests (6-Session Cross-Session Scenario)

```typescript
describe('Promotion Flow: Live → Tier 2 → Tier 3', () => {
  it('should promote pattern from live (session 1) to Tier 2 (sessions 2-3)', async () => {
    // Session 1: Detect auth module refactor pattern
    const session1 = await runSimulation('session-1', 50);
    const pattern1 = session1.patterns[0];
    expect(pattern1.confidence).toBeGreaterThan(0.88);

    // Sessions 2-3: Same pattern detected again
    const session2 = await runSimulation('session-2', 50);
    const session3 = await runSimulation('session-3', 50);

    // Check Tier 2 (sqlite-vec)
    const tier2Pattern = await db.search({
      vector: pattern1.embedding,
      limit: 1,
    });
    expect(tier2Pattern).toHaveLength(1);
    expect(tier2Pattern[0].count_across_sessions).toBe(3); // Promoted
  });

  it('should promote confirmed pattern to Tier 3 (mem0)', async () => {
    // ... (after 6 sessions with consistent voting)
    const rule = await mem0.getRule('auth_split_responsibility');
    expect(rule).toBeDefined();
    expect(rule.confidence).toBe(0.93);
    expect(rule.learned_sessions).toEqual(['s1', 's2', 's3', 's4', 's5', 's6']);
  });
});
```

---

## MCP Server: Exposing sqlite-vec to Agents

Agents need access to embeddings for retrieval without direct DB calls.

```typescript
// MCP Server wrapping sqlite-vec
import Anthropic from '@anthropic-ai/sdk';

const server = new Anthropic.Server({
  name: 'sqlite-vec-retrieval',
  version: '1.0.0',
});

server.tool('search_patterns', {
  description: 'Search for similar coordination patterns across sessions',
  inputSchema: {
    type: 'object',
    properties: {
      query: {
        type: 'string',
        description: 'Query text (e.g., "database refactoring")',
      },
      session_id: {
        type: 'string',
        description: 'Current session ID (optional, for filtering)',
      },
      limit: {
        type: 'number',
        default: 10,
        description: 'Max results',
      },
    },
  },
  handler: async (input) => {
    const queryEmbedding = await embedQuery(input.query);
    const results = await db.search({
      vector: queryEmbedding,
      where: input.session_id ? `session_id != ?` : undefined,
      params: input.session_id ? [input.session_id] : [],
      limit: input.limit,
    });
    return results;
  },
});

server.tool('get_learned_rules', {
  description: 'Retrieve all learned coordination rules (from mem0)',
  inputSchema: {
    type: 'object',
    properties: {
      confidence_min: {
        type: 'number',
        default: 0.93,
      },
    },
  },
  handler: async (input) => {
    const rules = await mem0.getRules({
      confidenceMin: input.confidence_min,
    });
    return rules;
  },
});
```

---

## Open Decisions (Phase 2)

1. **Embedding Model Cost/Quality:** sentence-transformers (free, local) vs OpenAI API (better, $$)?
   - **Decision Point:** Phase 2 start. Recommend local for MVP, OpenAI for Phase 3+ if accuracy improves relevance.

2. **Tier 3 Promotion Quorum:** How many agents must vote to promote a rule?
   - **Options:** Any 1 agent, ≥2 agents, unanimous (all agents in session)
   - **Recommendation:** ≥2 agents (conservative, avoids single-agent quirks)

3. **Rule TTL:** Should learned rules expire? After how long?
   - **Options:** Never expire, expire after 30 days unused, expire after 90 days
   - **Recommendation:** 90 days (allows seasonal variation, prevents stale rules)

4. **Conflict Arbitration:** Automatic (prefer higher confidence) or manual?
   - **Recommendation:** Manual (developer reviews decision, learns intent)

5. **mem0 Model:** gpt-4o-mini (cheap, fast) vs gpt-4o (better) vs local (free)?
   - **Recommendation:** gpt-4o-mini for Phase 2 (cost-effective), upgrade if accuracy issues

---

## Dependencies

| Layer               | Required                          | Optional   |
| ------------------- | --------------------------------- | ---------- |
| Tier 2 (Embeddings) | sqlite-vec, sentence-transformers | OpenAI API |
| Tier 3 (Learning)   | mem0, OpenAI API                  | Custom LLM |
| MCP Server          | anthropic SDK                     | None       |

---

## References

- Phase 1: `docs/archive/ARCHITECTURE.md`, `docs/phases/PHASE1-CHECKLIST.md`
- JSONL Format: Claude Code documentation
- sqlite-vec: `cloned-repos/sqlite-vec/README.md`
- mem0: `cloned-repos/mem0/README.md`

---

_Created: 2026-02-20 | Status: Design v1.0 | Ready for Phase 2 Implementation_
