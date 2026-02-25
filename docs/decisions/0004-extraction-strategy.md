---
status: accepted
phase: 1
audience: builder
tags: [decision, extraction, llm]
last_updated: 2026-02-20
---

# ADR-004: Single LLM Call with Progressive Enhancement

> **Status:** Accepted
> **Date:** 2026-02-20

---

## Context

Impulse's core value proposition is extracting useful knowledge from coding sessions and persisting it for future sessions. The extraction pipeline runs at session end: it reads the conversation transcript, identifies decisions, and appends them to GENOME.md with a summary to HISTORY_INDEX.md.

### Current Implementation (impulse-plugin/src/utils/extraction.ts)

The current `buildExtractionPrompt()` makes one LLM call with a simple prompt:
- Asks for DECISIONS (architectural choices, technology selections) and SUMMARY
- Truncates transcript to last 8,000 characters (`transcript.slice(-maxContextChars)`)
- Parses the response via regex (`DECISIONS:\n...\nSUMMARY:\n...`)
- Deduplicates against existing GENOME.md using 40-character substring fingerprint

### mem0 Comparison (MEMORY-EXTRACTION-ANALYSIS.md §1)

mem0's production pipeline uses 2 LLM calls minimum per `add()` operation (fact extraction + ADD/UPDATE/DELETE classification), plus 1 embedding per new fact, plus 3-4 additional LLM calls if graph memory is enabled. For a typical session producing 5-8 facts, that's 15-30 LLM calls.

**Cost comparison:**
| Approach | LLM Calls | Cost/Session | Infrastructure |
|----------|-----------|-------------|----------------|
| Impulse single-call | 1 | ~$0.0015 | None (API key only) |
| mem0 vector-only | 2 + embeddings | ~$0.002 | Vector DB + embedding model |
| mem0 with graph | 5 + embeddings | ~$0.005 | + Neo4j |

The cost difference is 1.3-4x, not 10-20x. The real cost of mem0 is infrastructure, not LLM calls.

### Quality Gap Analysis (MEMORY-EXTRACTION-ANALYSIS.md §2.1-2.5)

The current prompt has four identified weaknesses:

1. **No few-shot examples** — mem0 provides 5-6 input/output examples. Impulse provides zero. This is the single biggest improvement opportunity.
2. **No structured output format** — Freeform text with regex parsing. JSON mode would be more reliable.
3. **Front-only truncation** — `transcript.slice(-maxContextChars)` keeps only the LAST 8,000 characters. Critical decisions made early in long sessions are lost. The most important decisions happen at the start (setting direction) and end (finalizing choices).
4. **No awareness of existing knowledge** — The LLM has no idea what's already in GENOME.md, so it can't identify contradictions or avoid restating known decisions.

---

## Decision

**Impulse uses a single LLM call at session end, enhanced with four specific improvements that close ~50% of the quality gap with mem0 at zero infrastructure cost.**

### Improvement 1: Few-Shot Examples (HIGH impact, LOW cost)

Add 2-3 input/output examples to the extraction prompt:

```
Example session excerpt:
"Let's set up the database. PostgreSQL with pgvector is the right choice
for embeddings. We should use Prisma as the ORM. I considered Drizzle
but Prisma has better pgvector support..."

Example extraction:
{
  "decisions": [
    "2026-02-20: PostgreSQL with pgvector for embeddings storage",
    "2026-02-20: Prisma as ORM (over Drizzle — better pgvector support)"
  ],
  "summary": "Set up database infrastructure. Chose PostgreSQL with pgvector and Prisma ORM. Schema not yet defined."
}
```

Few-shot examples are the highest-leverage prompt engineering technique. They define both the granularity (one decision per line) and the quality bar (include rationale when available).

### Improvement 2: JSON Response Format (HIGH impact, LOW cost)

Replace freeform text + regex parsing with structured JSON:

```json
{
  "decisions": ["YYYY-MM-DD: description", ...],
  "summary": "3-5 line summary",
  "contradictions": ["description of contradiction with existing decision", ...]
}
```

Most modern LLM APIs support `response_format: { type: "json_object" }` which guarantees valid JSON output. This eliminates the regex failure modes where the LLM outputs "Decision:" (singular) or uses different heading styles.

### Improvement 3: Beginning+End Transcript Sampling (MEDIUM impact, LOW cost)

Replace tail-only truncation with weighted sampling:

```typescript
const beginning = transcript.slice(0, maxContextChars * 0.3);  // Direction-setting
const ending = transcript.slice(-maxContextChars * 0.7);        // Conclusions
const sampled = beginning + "\n[...middle of session omitted...]\n" + ending;
```

This captures both the direction-setting phase (beginning, 30%) and the conclusion phase (end, 70%). The middle of most coding sessions is debugging noise — precisely what we want to skip.

### Improvement 4: GENOME-Aware Contradiction Flagging (MEDIUM impact, MEDIUM cost)

Feed existing GENOME.md content into the extraction call:

```
Here are the decisions already recorded for this project:
${existingGenomeContent}

When extracting new decisions:
- If a decision CONTRADICTS an existing one, add it to the "contradictions" array
- If a decision is already recorded, SKIP it (do not re-extract)
- Only extract genuinely NEW decisions
```

This enables basic contradiction detection within a single LLM call. It cannot UPDATE or DELETE existing entries (that requires mem0-style two-phase classification), but it can FLAG contradictions for human review. Flagged contradictions are appended to GENOME.md with a `[UPDATED]` prefix.

### LLM Access Strategy

Claude Code hooks are shell commands — they cannot directly access an LLM. The SessionEnd hook CLI:

1. Reads `transcript_path` from stdin JSON
2. Parses the JSONL file, filtering tool noise (~75% of content)
3. Samples beginning (30%) + end (70%) of remaining text
4. Reads existing `.impulse/GENOME.md`
5. Makes one API call to Anthropic (or OpenAI) via the SDK
6. Parses JSON response
7. Appends new decisions to GENOME.md, summary to HISTORY_INDEX.md
8. Cleans up LIVE_STATE.json (removes this agent's entry)

**API key source:** `IMPULSE_API_KEY` environment variable, or `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` fallback.

**Model choice:** Claude Haiku or GPT-4o-mini (fast, cheap, sufficient for extraction). Configurable via `IMPULSE_MODEL`.

### Failure Modes and Recovery

| Failure | Impact | Recovery |
|---------|--------|----------|
| API call fails (network, rate limit) | No extraction for this session | Write `extraction_pending: true` to LIVE_STATE.json. Next SessionStart spawns impulse-session-end as a BACKGROUND process (non-blocking). |
| Process killed during extraction | Partial or no write | Same deferred extraction pattern. GENOME.md append is atomic (write to temp file + rename). |
| LLM returns invalid JSON | No extraction | Fall back to regex parsing of raw text. Log warning. |
| Transcript too short (< 500 chars) | Nothing to extract | Skip extraction. Write "no-op session" to HISTORY_INDEX.md. |
| GENOME.md is locked by another process | Cannot append | Retry with exponential backoff (3 attempts, 100ms/200ms/400ms). |

---

## Consequences

### Positive

- **$0.0015/session cost** — One LLM call using a fast model. No embeddings, no vector searches.
- **Zero infrastructure beyond an API key** — No vector database, no embedding model, no background service.
- **Few-shot + JSON format close ~50% of quality gap** — These two changes alone bring extraction quality significantly closer to mem0's multi-call pipeline.
- **Contradiction awareness without UPDATE/DELETE** — Flagging contradictions (even without resolving them) gives users visibility into knowledge drift.
- **Deferred extraction handles failures gracefully** — If SessionEnd extraction fails, SessionStart detects `extraction_pending` and spawns extraction as a BACKGROUND process. Does NOT block. No data loss. SessionStart maintains <30ms latency target.

### Negative

- **Cannot UPDATE or DELETE existing GENOME.md entries** — Only ADD + flag contradictions. Stale decisions persist until manually pruned or LLM-assisted pruning is added (Phase 2).
- **40-character fingerprint deduplication remains brittle** — "Use Zod for validation" and "Runtime validation with Zod schemas" will both be added. Semantic dedup requires embeddings (Phase 2).
- **Requires external API key** — Unlike mem0 (which can use Ollama locally), Impulse's extraction needs a cloud LLM API. Configurable to support local models via LiteLLM/Ollama in the future.
- **Single-call extraction misses implicit decisions** — If the agent chose React over Vue by just writing React code (never explicitly stating the decision), the extraction will miss it. This is inherent to text analysis.

---

## Alternatives Considered

### Alternative 1: mem0 Full Pipeline

Deferred to Phase 3 because:
- Infrastructure cost is disproportionate for < 100 sessions
- The four prompt improvements close ~50% of the quality gap at zero infrastructure cost
- Migration path exists: when GENOME.md exceeds 500 lines, mem0 can take over extraction with ADD/UPDATE/DELETE

### Alternative 2: No LLM Call (Rule-Based Extraction)

Rejected because:
- Rule-based extraction (regex for "decided", "chose", "will use") has ~30-40% recall
- LLM extraction has ~75-85% recall — a 2x improvement
- The $0.0015/session cost is negligible
- Rule-based can't summarize sessions or detect decision nuance

### Alternative 3: Agent Hook (type: "agent") for Extraction

Claude Code supports `type: "agent"` hooks that spawn a multi-turn subagent with tool access. This could perform extraction using Claude itself (no external API needed).

**This alternative deserves serious re-evaluation.** The original dismissal ("can take up to 50 turns — overkill") conflates maximum with typical. A well-designed extraction agent hook takes 2-3 turns:
1. Turn 1: Read transcript using `Read` tool (accessing `transcript_path` from hook input)
2. Turn 2: Write decisions to GENOME.md using `Edit`/`Write` tool
3. Done

**Benefits over SDK call:**
- **Zero API key required** — Claude Code already pays for the session; the agent hook uses the same model
- **Native file access** — Uses Claude's built-in tools, no custom file I/O code
- **Same extraction model** — No separate model configuration or cost estimation

**Include in the Pre-Phase 1 spike** (see PHASE1-CHECKLIST.md v2.1): test a 2-turn agent hook against the SDK call approach. Use whichever demonstrates better quality/cost ratio.

### Alternative 4: Two-Phase Extraction (Extract, then Classify)

A "poor man's mem0" — first call extracts facts, second call classifies each as ADD/UPDATE/DELETE/SKIP against existing GENOME.md.

Deferred to Phase 1.5 because:
- Doubles the LLM cost ($0.003/session)
- Improvement #4 (GENOME-aware prompting) provides basic contradiction detection in a single call
- Add the second call only when contradiction accumulation becomes a measured problem

---

## References

- MEMORY-EXTRACTION-ANALYSIS.md §1: mem0 pipeline architecture (2+ LLM calls, infrastructure requirements)
- MEMORY-EXTRACTION-ANALYSIS.md §2.1: Current `buildExtractionPrompt()` weaknesses (no few-shot, no JSON, front-truncation)
- MEMORY-EXTRACTION-ANALYSIS.md §2.5: Ranked improvements (#1 few-shot, #2 JSON, #3 sampling, #4 GENOME-aware)
- MEMORY-EXTRACTION-ANALYSIS.md §5: Quality comparison and upgrade triggers
- AGENT-HARNESS-ANALYSIS.md §2.2: Claude Code provides `transcript_path` to SessionEnd hook
- impulse-plugin/src/utils/extraction.ts: Current implementation being improved
