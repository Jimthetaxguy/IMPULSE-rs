---
title: Memory Extraction Analysis
description: Analysis of memory extraction approaches from raw sessions to persistent knowledge
version: '1.0'
updated: 2026-02-20
type: research
category: analysis
phase: phase2
status: active
audience: builders
tags: [research, memory, extraction, llm]
---

# Memory Extraction Analysis: From Raw Sessions to Persistent Knowledge

> **Version:** 1.0 | **Status:** Research Complete | **Updated:** 2026-02-20
> **Purpose:** Deep analysis of memory extraction approaches for Impulse
> **Builds on:** docs/research/RESEARCH-DIGEST.md (Section 3: Mem0 Analysis), docs/research/REALISTIC-FRAMEWORK.md (Principle 1)

---

## Executive Summary

Memory extraction -- converting raw coding session transcripts into useful, persistent knowledge -- is Impulse's core value proposition. This document analyzes the full pipeline from both ends of the complexity spectrum: mem0's production-grade 20+ LLM call pipeline and Impulse's single-call "lean extraction." The goal is to answer the central question: **is one LLM call at session end good enough, or do we need the heavy machinery?**

**Key findings:**

1. **mem0's pipeline uses 2 LLM calls per `add()` operation minimum** (fact extraction + memory management decision), plus 1 embedding per new fact, plus 3-4 additional LLM calls if graph memory is enabled. For a typical coding session producing 5-10 facts, that is 15-30 LLM calls per session.

2. **Impulse's single-call extraction is architecturally sound for Phase 1** but has identified failure modes: it cannot handle contradictions with existing knowledge, cannot update or delete stale decisions, and uses a naive 40-character substring deduplication that will produce both false positives and false negatives.

3. **The core value hypothesis -- that injecting past decisions into the system prompt improves agent behavior -- has strong indirect evidence** from context engineering research (17-32% productivity gains in debugging/refactoring tasks) but no direct A/B test specific to coding agents reading GENOME.md-style files.

4. **The competitive landscape validates our approach.** Cursor (Memories feature), Windsurf (Cascade Memories), Cline (Memory Bank), and aider (repo map) all converge on the same pattern: structured persistent files injected at session start. None use heavy vector-based memory for cross-session coding context in their core product.

5. **The upgrade trigger should be based on observed failure modes, not session count.** The "upgrade at 100 sessions" heuristic is reasonable but the real triggers are: (a) GENOME.md exceeds 200 lines, (b) contradictory decisions accumulate, (c) users report stale context hurting more than helping.

---

## 1. mem0 -- Production Memory Pipeline

### 1.1 Architecture (from source)

mem0's `Memory` class (file: `mem0/memory/main.py`) orchestrates a multi-component system:

```
Memory.__init__():
  - embedding_model (EmbedderFactory) -- generates vector embeddings
  - vector_store (VectorStoreFactory) -- stores/searches embeddings (Qdrant, Faiss, Chroma, etc.)
  - llm (LlmFactory) -- makes extraction and decision calls
  - db (SQLiteManager) -- audit trail of all memory operations
  - graph (MemoryGraph, optional) -- Neo4j knowledge graph for entity relationships
  - reranker (RerankerFactory, optional) -- re-ranks search results
```

The core `add()` method runs two parallel tracks via `ThreadPoolExecutor`:

1. **Vector store pipeline** (`_add_to_vector_store`) -- fact extraction + ADD/UPDATE/DELETE decisions
2. **Graph pipeline** (`_add_to_graph`) -- entity extraction + relationship mapping (if enabled)

This is significantly more infrastructure than a single LLM call. The system requires: an LLM provider, an embedding model, a vector database, SQLite for history, and optionally Neo4j + a reranker.

### 1.2 Fact Extraction Pipeline

The extraction phase uses carefully crafted prompts (file: `mem0/configs/prompts.py`). There are three distinct prompt strategies:

**USER_MEMORY_EXTRACTION_PROMPT** (for conversations where user provides info):

- Focuses on: personal preferences, important details, plans/intentions, health/wellness, professional details
- Critically emphasized: "GENERATE FACTS SOLELY BASED ON THE USER'S MESSAGES. DO NOT INCLUDE INFORMATION FROM ASSISTANT OR SYSTEM MESSAGES."
- Uses few-shot examples showing input/output pairs
- Output format: `{"facts": ["fact1", "fact2", ...]}`

**AGENT_MEMORY_EXTRACTION_PROMPT** (for remembering assistant characteristics):

- Focuses on: assistant preferences, capabilities, personality traits, approach to tasks
- Mirrors the user prompt structure but extracts from assistant messages only

**FACT_RETRIEVAL_PROMPT** (legacy, broader extraction):

- Less strict about source separation
- More general extraction of "relevant pieces of information"

The selection logic (`_should_use_agent_memory_extraction`) checks:

- Is `agent_id` present in metadata?
- Are there assistant-role messages?
- If both true, use agent extraction; otherwise use user extraction.

**Critical observation for Impulse:** mem0's extraction prompts are optimized for _conversational_ memory (preferences, personal details, plans). Impulse needs _technical decision_ memory (architecture choices, coding patterns, resolved debates). The prompts would need significant adaptation.

### 1.3 ADD/UPDATE/DELETE Decision Logic

After fact extraction, mem0 makes a second LLM call to decide what to do with each fact. This is the `DEFAULT_UPDATE_MEMORY_PROMPT` -- a long, carefully engineered prompt that:

1. **Retrieves similar existing memories** -- for each extracted fact, it embeds the fact and searches the vector store for the top 5 most similar existing memories.

2. **Maps UUIDs to integers** -- a clever trick to prevent LLM hallucination of memory IDs. Real UUIDs are replaced with simple integer indices (0, 1, 2...) and mapped back after the LLM responds.

3. **Asks the LLM to classify each fact** into one of four operations:
   - **ADD**: New information not present in memory. LLM generates a new ID.
   - **UPDATE**: Existing memory needs modification. Same ID, new content. Requires `old_memory` field.
   - **DELETE**: New fact contradicts existing memory. Memory should be removed.
   - **NONE**: Fact already present or irrelevant. No change needed.

4. **Executes the operations** against the vector store:
   - ADD: `_create_memory()` -- embed + insert into vector store + SQLite history
   - UPDATE: `_update_memory()` -- re-embed + update vector store + SQLite history
   - DELETE: `_delete_memory()` -- remove from vector store + SQLite history

**The key insight:** This two-phase approach (extract facts, then decide what to do with them) is what enables contradiction resolution. A single-call system can only ADD -- it cannot update or delete stale information.

### 1.4 Contradiction Resolution

mem0 handles contradictions through the UPDATE and DELETE operations. From the prompt:

> "If the retrieved facts contain information that contradicts the information present in the memory, then you have to delete it."

Example from the prompt:

```
Old Memory: [{"id": "1", "text": "Loves cheese pizza"}]
New fact: ["Dislikes cheese pizza"]
Result: DELETE memory id 1
```

The LLM also handles semantic equivalence:

```
Old Memory: "Likes cheese pizza"
New fact: "Loves cheese pizza"
Result: NONE (conveys same information)
```

And additive updates:

```
Old Memory: "I really like cheese pizza"
New fact: "Loves chicken pizza"
Result: UPDATE to "Loves cheese and chicken pizza"
```

**Impulse gap:** Our current `deduplicateDecisions()` function uses a 40-character substring fingerprint for deduplication. It cannot detect contradictions, cannot update stale decisions, and cannot merge related decisions. If a project switches from JWT to session-based auth, both decisions would coexist in GENOME.md.

### 1.5 Graph Memory Component

The `MemoryGraph` class (file: `mem0/memory/graph_memory.py`) adds a knowledge graph layer on top of the vector store. It uses Neo4j and requires:

1. **Entity extraction** -- LLM call to identify entities and their types from text
2. **Relationship establishment** -- LLM call to define relationships between entities
3. **Graph search** -- Cosine similarity on entity embeddings stored in Neo4j
4. **Conflict detection** -- LLM call to identify relationships that should be deleted
5. **Entity management** -- Merge existing nodes, create new ones, track mention counts

For Impulse, this is firmly Phase 3+ territory. The graph adds 3-4 additional LLM calls per operation and requires Neo4j infrastructure. However, the _concept_ is valuable: tracking relationships between code entities (which modules depend on which, which pattern was chosen over which alternative) could be powerful for large, long-running projects.

### 1.6 OpenMemory MCP Server

The OpenMemory MCP server (file: `openmemory/api/app/mcp_server.py`) exposes mem0's memory through the Model Context Protocol. It provides five tools:

| Tool                  | Description                    | Trigger                                                |
| --------------------- | ------------------------------ | ------------------------------------------------------ |
| `add_memories`        | Store new memory               | "Everytime the user informs anything about themselves" |
| `search_memory`       | Search stored memories         | "EVERYTIME the user asks anything"                     |
| `list_memories`       | List all memories              | On demand                                              |
| `delete_memories`     | Delete specific memories by ID | On demand                                              |
| `delete_all_memories` | Delete all memories            | On demand                                              |

Key implementation details:

- **Lazy client initialization** -- memory client is created on first use, not at import time, so the server survives if Ollama/vector store is unavailable.
- **Permission system** -- memories are filtered by user ACL, preventing cross-user leakage.
- **Access logging** -- every memory access is logged with app ID, access type, and metadata.
- **Database-backed state** -- PostgreSQL tracks memory state (active/deleted), status history, and access logs.

**Impulse relevance:** The MCP approach is interesting for Phase 2+. Instead of injecting GENOME.md directly into the system prompt, Impulse could expose memory as MCP tools that agents query on demand. This would be more token-efficient for large knowledge bases.

### 1.7 Real Cost Analysis

Counting LLM calls for a single `memory.add()` with a typical coding session (producing 5-8 facts):

| Operation                      | LLM Calls | Embedding Calls | Notes                            |
| ------------------------------ | --------- | --------------- | -------------------------------- |
| Fact extraction                | 1         | 0               | JSON response format             |
| Per-fact vector search         | 0         | 5-8             | One embedding per new fact       |
| Memory management decision     | 1         | 0               | ADD/UPDATE/DELETE classification |
| Per-ADD memory creation        | 0         | 3-5             | Re-embed if not cached           |
| Per-UPDATE memory update       | 0         | 1-2             | Re-embed updated content         |
| **Subtotal (vector only)**     | **2**     | **9-15**        |                                  |
| Graph: entity extraction       | 1         | 0               | If graph enabled                 |
| Graph: relationship extraction | 1         | 0               | If graph enabled                 |
| Graph: deletion detection      | 1         | 0               | If graph enabled                 |
| Graph: entity embeddings       | 0         | 4-8             | Per entity pair                  |
| **Total (with graph)**         | **5**     | **13-23**       |                                  |

**Cost estimate at GPT-4o-mini pricing ($0.15/1M input, $0.60/1M output):**

- Fact extraction: ~2K input + 500 output = ~$0.0006
- Memory management: ~3K input + 1K output = ~$0.001
- Total per session (vector only): ~$0.002
- Total per session (with graph): ~$0.005
- Embedding calls (text-embedding-3-small at $0.02/1M): ~$0.0005

**Total: $0.002-0.006 per session** (not $0.02-0.05 as previously estimated -- the earlier estimate was too high).

**Impulse single-call cost:**

- One GPT-4o-mini call: ~8K input (transcript) + 500 output = ~$0.0015

The cost difference is 1.3-4x, not 10-20x. The real cost of mem0 is not the LLM calls -- it is the infrastructure (vector store, embeddings model, optional Neo4j, SQLite).

### 1.8 Where Quality Actually Matters

Having read the full pipeline, the quality difference between mem0 and a single extraction call comes from three specific capabilities:

**1. Contradiction resolution (HIGH impact for coding)**
When a project pivots from PostgreSQL to MongoDB, mem0 would DELETE the old "Using PostgreSQL" memory and ADD "Using MongoDB." Impulse would append both, leaving the agent confused. This matters significantly for coding projects that evolve.

**2. Semantic deduplication (MEDIUM impact)**
mem0 uses vector similarity to find near-duplicate memories before asking the LLM to decide. Impulse uses 40-character substring matching. For coding decisions, where the same concept gets worded differently across sessions ("Use Zod for validation" vs "Runtime validation with Zod schemas"), mem0's approach is substantially better.

**3. Granular fact decomposition (LOW impact for coding)**
mem0 breaks conversations into individual facts. For personal preferences ("likes pizza, lives in SF, works at Google"), this is essential. For coding sessions, the granularity is less critical because decisions are typically already discrete ("we chose JWT," "we use PostgreSQL").

**4. Graph relationships (LOW impact for Phase 1, HIGH for Phase 3)**
Understanding that "auth module depends on JWT library which uses crypto package" is powerful for large codebases but overkill for early Impulse usage.

---

## 2. Impulse's Single-Call Extraction

### 2.1 Current buildExtractionPrompt Analysis

The extraction prompt (file: `impulse-plugin/src/utils/extraction.ts`) asks the LLM to extract two things from the session transcript:

```
DECISIONS (architectural choices, technology selections, resolved debates):
- Format: "YYYY-MM-DD: One-line decision description"
- Only include genuine decisions, not debugging steps or temporary experiments
- If no decisions were made, write "None"

SUMMARY (3-5 lines covering what was accomplished):
- What was the main goal?
- What was achieved?
- What files were modified?
- Any unresolved issues?
```

**Strengths:**

- Clear extraction target: "decisions" not "facts" -- correctly scoped for coding context
- Explicit exclusion: "not debugging steps or temporary experiments" -- reduces noise
- Date-stamped format: enables temporal reasoning about when decisions were made
- Files list: provides grounding context that may improve extraction accuracy
- Token budget: only processes last 8000 characters -- avoids overwhelming the LLM with full transcripts

**Weaknesses:**

- **No few-shot examples.** mem0 provides 5-6 input/output examples. Impulse provides zero. This is the single biggest improvement opportunity.
- **No structured output format.** mem0 uses `response_format: {"type": "json_object"}`. Impulse uses freeform text with regex parsing. JSON mode would be more reliable.
- **No category guidance.** The prompt says "architectural choices, technology selections, resolved debates" but does not define what these mean with examples. An LLM might miss coding preferences, constraint decisions, or tool selections.
- **No context about existing knowledge.** The LLM has no idea what is already in GENOME.md, so it cannot prioritize genuinely new decisions over restating known ones.
- **Truncation from the front.** `transcript.slice(-maxContextChars)` keeps only the LAST 8000 characters. If a critical decision was made early in a long session, it will be lost. The most important decisions often happen at the start (setting direction) and end (finalizing choices) -- the middle is debugging noise.

### 2.2 Parsing Logic

The `parseExtraction()` function uses regex to find DECISIONS and SUMMARY sections:

```typescript
const decisionsMatch = response.match(/DECISIONS:\s*\n([\s\S]*?)(?=\nSUMMARY:|$)/i);
const summaryMatch = response.match(/SUMMARY:\s*\n([\s\S]*?)$/i);
```

**Failure modes:**

- If the LLM outputs "DECISION:" (singular) instead of "DECISIONS:" -- no match
- If the LLM uses a different heading style ("## Decisions") -- no match
- If the LLM puts the summary before decisions -- the regex still works (good)
- If the LLM adds extra sections ("NOTES:" after SUMMARY:) -- summary regex captures everything after SUMMARY: until end of string, including the extra content

The bullet parsing is more robust:

```typescript
if (trimmed.startsWith('-') || trimmed.startsWith('*')) {
  const decision = trimmed.replace(/^[-*]\s*/, '').trim();
```

This handles both `-` and `*` bullet styles, which covers most LLM output variations.

### 2.3 Failure Modes

Analyzing what kinds of decisions the current system would MISS or CORRUPT:

| Failure Mode                             | Severity | Example                                                                                      | Cause                                          |
| ---------------------------------------- | -------- | -------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| **Early-session decisions lost**         | HIGH     | "Let's use PostgreSQL" said in minute 1 of a 2-hour session, truncated at 8K chars           | Front-truncation of transcript                 |
| **Implicit decisions missed**            | HIGH     | Agent chose React over Vue by just starting to write React code, never explicitly stating it | No explicit decision statement to extract      |
| **Contradictions accumulate**            | HIGH     | Session 5: "Use REST API." Session 12: "Switch to GraphQL." Both persist in GENOME.md        | No UPDATE/DELETE mechanism                     |
| **Stale decisions persist forever**      | MEDIUM   | Decision from 6 months ago is no longer relevant but still injected every session            | No pruning, no TTL, no relevance decay         |
| **Near-duplicates accumulate**           | MEDIUM   | "Use Zod for validation" and "Runtime validation via Zod" both added                         | 40-char fingerprint misses semantic similarity |
| **Multi-agent conflict**                 | MEDIUM   | Agent A decides "use tabs" and Agent B decides "use spaces" in same session                  | No conflict detection between agents           |
| **LLM format variation**                 | LOW      | LLM outputs "Decision:" instead of "DECISIONS:"                                              | Regex brittleness                              |
| **Empty extraction on routine sessions** | LOW      | Session was purely debugging with no decisions, but LLM hallucinates a decision              | No "was this session decision-free?" filter    |

### 2.4 Extraction Quality Evaluation Framework

To properly evaluate extraction quality, Impulse needs a testing methodology:

**Step 1: Build a ground-truth dataset**

- Record 10-20 real coding sessions (or use synthetic transcripts)
- Manually annotate each session with the "correct" decisions
- Include sessions with: 0 decisions, 1 decision, 5+ decisions, contradictions

**Step 2: Define metrics**

| Metric                      | Definition                                                  | Target            |
| --------------------------- | ----------------------------------------------------------- | ----------------- |
| **Precision**               | (correct decisions extracted) / (total decisions extracted) | > 0.80            |
| **Recall**                  | (correct decisions extracted) / (total actual decisions)    | > 0.70            |
| **Noise rate**              | (hallucinated or trivial decisions) / (total extracted)     | < 0.15            |
| **Contradiction detection** | (contradictions identified) / (total contradictions)        | > 0.50 (Phase 2+) |
| **Dedup accuracy**          | 1 - (duplicate decisions in GENOME.md) / (total decisions)  | > 0.90            |

**Step 3: Evaluation protocol**

```
For each test session:
  1. Run buildExtractionPrompt() -> get LLM response
  2. Run parseExtraction() -> get structured result
  3. Compare against ground truth annotations
  4. Score precision, recall, noise rate
  5. Run deduplicateDecisions() against existing GENOME.md
  6. Score dedup accuracy
```

**Step 4: Iteration**

- Adjust prompt wording, add few-shot examples, change truncation strategy
- Re-run evaluation after each change
- Track metrics over prompt versions

### 2.5 Improvement Opportunities

Ranked by expected impact and implementation cost:

**1. Add few-shot examples to the extraction prompt (HIGH impact, LOW cost)**

```typescript
// Add to buildExtractionPrompt():
const examples = `
Example session transcript:
"Let's set up the database. I think PostgreSQL with pgvector is the right
choice for our embeddings. We should use Prisma as the ORM..."

Example output:
DECISIONS:
- 2026-02-20: PostgreSQL with pgvector for embeddings storage
- 2026-02-20: Prisma as the ORM layer

SUMMARY:
Set up database infrastructure. Chose PostgreSQL with pgvector for embedding
storage and Prisma as the ORM. Database schema not yet defined.
`;
```

**2. Use JSON response format (HIGH impact, LOW cost)**

Instead of freeform text with regex parsing, request JSON:

```typescript
const prompt = `...respond in this exact JSON format:
{
  "decisions": ["YYYY-MM-DD: description", ...],
  "summary": "3-5 line summary"
}`;
```

Most modern LLM APIs support `response_format: { type: "json_object" }` which guarantees valid JSON output.

**3. Smarter transcript truncation (MEDIUM impact, LOW cost)**

Instead of taking the last 8000 characters, sample from beginning AND end:

```typescript
const beginning = transcript.slice(0, maxContextChars * 0.3);
const ending = transcript.slice(-maxContextChars * 0.7);
const truncatedTranscript = beginning + '\n[...middle of session omitted...]\n' + ending;
```

This captures both the direction-setting phase (beginning) and the conclusion phase (end).

**4. Feed existing GENOME.md to the extraction call (MEDIUM impact, MEDIUM cost)**

```typescript
const prompt = `...
Here are the decisions already recorded for this project:
${existingGenome}

Only extract decisions that are NEW or that CONTRADICT existing decisions.
If a decision contradicts an existing one, prefix it with "UPDATED:" ...`;
```

This single change would enable basic contradiction detection without needing mem0's full pipeline.

**5. Confidence scoring (LOW impact, MEDIUM cost)**

Ask the LLM to rate confidence for each extracted decision:

```
DECISIONS:
- 2026-02-20: Use PostgreSQL [confidence: HIGH]
- 2026-02-20: Maybe try Redis for caching [confidence: LOW]
```

Only persist HIGH confidence decisions. This reduces noise from tentative discussions.

---

## 3. The Core Value Hypothesis: Does GENOME.md Help?

### 3.1 Evidence for Context Injection Improving LLM Performance

**Direct evidence (specific to coding):**

- Research in IEEE Transactions on Software Engineering reports **17-32% productivity improvements** for debugging and refactoring tasks when LLMs are given structured context about the codebase.
- A study on SWE-Bench with context engineering showed **+3.3% (Gemini 2.5 Flash) and +1.7% (Claude Sonnet 4)** gains in grounding score when structured context was injected.
- In-context learning research shows performance "heavily depends on the quality of demonstration examples" -- suggesting that high-quality GENOME.md content could meaningfully improve agent behavior, but low-quality content (stale, contradictory, noisy) could hurt.

**Indirect evidence (from LLM context research):**

- Agentic Context Engineering (ACE) framework treats contexts as "evolving playbooks that accumulate, refine, and organize strategies" -- this is precisely what GENOME.md is.
- Context quality matters "significantly more than quantity" -- a focused 50-line GENOME.md outperforms a bloated 500-line one.
- RAG-based context injection is the standard approach for knowledge-grounded generation, with consistent improvements over no-context baselines.

**Counter-evidence (reasons for caution):**

- No published A/B test specifically measures "coding agent with vs. without GENOME.md-style injection."
- "More context does not guarantee better reasoning; excessive irrelevant tokens increase the chance that the model anchors on the wrong fragment."
- Context degradation syndrome: as contexts grow, "attention concentrates on the beginning and end of the input, so information in middle positions gets less reliable processing."

**Assessment:** The hypothesis is well-supported but unproven for the specific Impulse use case. The improvement is likely real but the magnitude is unknown. Building the evaluation framework (Section 2.4) and running even informal A/B tests would substantially de-risk this.

### 3.2 Information Value Hierarchy (decisions > preferences > constraints > facts)

Not all persistent knowledge is equally valuable for coding agents. Based on how agents actually use context, here is a proposed hierarchy:

| Tier       | Type                    | Example                                                 | Value for Next Session                                           |
| ---------- | ----------------------- | ------------------------------------------------------- | ---------------------------------------------------------------- |
| **S-tier** | Architectural decisions | "Using PostgreSQL + pgvector, not standalone vector DB" | Critical -- prevents agent from re-debating or contradicting     |
| **A-tier** | Active constraints      | "API responses must be < 50ms (SLA)"                    | High -- shapes all implementation decisions                      |
| **B-tier** | Coding preferences      | "TypeScript strict mode, Zod for validation"            | Moderate -- saves time on style questions                        |
| **C-tier** | Project facts           | "The auth module is in src/auth/"                       | Low -- agent discovers from file tree                            |
| **D-tier** | Historical events       | "On Feb 18 we debugged the JWT issue"                   | Minimal -- only useful for understanding why decisions were made |

**Implication for GENOME.md structure:** The default template already separates "Architectural Decisions," "Coding Preferences," and "Project Constraints" -- this aligns well with the hierarchy. The extraction prompt should be tuned to preferentially extract S-tier and A-tier content.

### 3.3 Information Decay and Pruning

**The problem:** GENOME.md will grow indefinitely. After 50 sessions, it could contain 200+ decisions. After 200 sessions, 500+. At some point, old decisions become noise:

- "2025-06-15: Using Express.js for the API" is useless if the project migrated to Fastify in September.
- "2025-03-01: Team uses 2-space indentation" is low-value noise that wastes context window tokens.

**Research findings on information decay in LLM contexts:**

- JetBrains research: "Agent-generated context quickly turns into noise instead of being useful information."
- Context degradation syndrome: LLMs show "primacy and recency bias" -- they attend more to information at the beginning and end of context, with the middle becoming a dead zone.
- Redis engineering blog: "System instruction erosion" occurs as conversation history grows and system-level context is pushed further from attention.

**Proposed pruning strategies for GENOME.md:**

**Strategy 1: Time-based decay (simple, imprecise)**

- Decisions older than 90 days are moved to GENOME-ARCHIVE.md
- Archived decisions are not injected into the system prompt
- Users can manually promote archived decisions back

**Strategy 2: Reference-based survival (better, requires tracking)**

- Track which GENOME.md decisions the agent "references" (mentions, uses as basis for choices)
- Decisions referenced in the last 30 days survive
- Unreferenced decisions decay to archive after 60 days

**Strategy 3: LLM-assisted pruning (best, costs 1 LLM call)**

- Every 20 sessions, pass GENOME.md to an LLM: "Which of these decisions are still relevant to the current state of the project? Which are stale?"
- LLM marks stale decisions for archival
- This is essentially mem0's DELETE operation but at a lower frequency

**Recommendation for Phase 1:** No automated pruning. Set `genomeWarnLines: 200` in config (already done). When the warning fires, prompt the user to manually review. Add Strategy 3 as the Phase 2 pruning approach.

### 3.4 Optimal Injection Format

How should GENOME.md content be presented in the system prompt? Options:

**Option A: Raw markdown (current approach)**

```
## Project Knowledge (from .impulse/GENOME.md)
# Project Genome
## Architectural Decisions
- 2026-02-18: Using JWT with 15-minute expiry...
```

- Pro: Simple, human-readable, easy to debug
- Con: Unstructured, no priority signaling, wastes tokens on markdown formatting

**Option B: Structured sections with priority markers**

```
## Project Memory (auto-loaded from .impulse/GENOME.md)
[CRITICAL] Architecture: PostgreSQL + pgvector for embeddings
[CRITICAL] Architecture: JWT auth with 15-min expiry, HttpOnly cookies
[IMPORTANT] Preference: TypeScript strict mode, Zod validation
[NOTE] History: Auth module was split on 2026-02-20
```

- Pro: Clear priority hierarchy, machine-parseable
- Con: Requires additional formatting logic, may look "robotic" to users

**Option C: Narrative summary**

```
This project uses PostgreSQL with pgvector for embeddings and JWT with
15-minute expiry for authentication. The team prefers TypeScript strict
mode with Zod validation. Key constraint: API responses must be under 50ms.
```

- Pro: Natural reading for the LLM, compact
- Con: Loses temporal information, hard to update incrementally

**Option D: Semantic grouping (recommended)**

```
## Project Context (loaded from .impulse/GENOME.md)

### Stack & Architecture
- PostgreSQL 14 + pgvector for embeddings (decided 2026-02-19)
- JWT with 15-minute expiry, HttpOnly cookies (decided 2026-02-18)

### Active Constraints
- API response time < 50ms (SLA)
- No Python deps in production

### Coding Standards
- TypeScript strict mode, no implicit any
- Zod for runtime validation
- Result<T, E> pattern for error handling
```

- Pro: Organized by how agents use the information, temporal context preserved, scannable
- Con: Requires extraction prompt to categorize decisions, or post-processing to categorize

**Recommendation:** Option A for Phase 1 (it works, it is simple). Migrate to Option D when extraction quality is validated and categories are understood.

### 3.5 Experiment Design for Validation

To validate the core hypothesis, run this experiment:

**Setup:**

- Select 5 real coding tasks of moderate complexity (e.g., "add a new API endpoint with auth")
- Create two variants of each task: with GENOME.md injection and without
- Use the same LLM model for both variants

**Protocol:**

```
For each task:
  Variant A (control): Start agent session with NO GENOME.md injection
  Variant B (treatment): Start session with GENOME.md containing relevant decisions

  Measure:
  1. Does the agent follow existing architectural decisions? (Y/N)
  2. Does the agent ask questions that GENOME.md already answers? (count)
  3. Does the agent produce code consistent with recorded preferences? (Y/N)
  4. Time to first meaningful code output (seconds)
  5. Number of course-corrections needed from human (count)
```

**Expected outcome:** Variant B should show fewer redundant questions, higher consistency with existing patterns, and faster convergence. If Variant B performs worse (e.g., agent over-constrains itself based on stale GENOME.md content), that is valuable negative signal.

**Minimum viable test:** Even running this informally on 3 tasks would provide useful data. The hypothesis does not need a p-value; it needs directional confidence.

---

## 4. Competitive Extraction Approaches

### 4.1 Cursor Memory

**How it works:** Cursor introduced a "Memories" feature in 2025 that transforms how the AI assistant understands project context. Memories can be auto-generated by the AI or manually created by users, and persist across chat sessions.

**Architecture:**

- Memories are stored as persistent records associated with a project
- Integrated via Model Context Protocol (MCP) -- Basic Memory MCP server
- Memory project state is cached and persists across sessions
- Global rules + project-specific rules hierarchy

**Key insight for Impulse:** Cursor's memory is primarily user-driven (users tell it what to remember) with some auto-generation. Impulse is primarily auto-generated (extraction at session end). Cursor's approach has lower noise (users curate) but higher friction (users must actively manage memory). Impulse's approach has higher noise risk but zero friction.

**Pattern worth adopting:** Cursor's separation of global rules vs. project rules. GENOME.md is project-scoped, but there could be a global `~/.impulse/GENOME.md` for user-wide preferences ("I always use TypeScript strict mode").

### 4.2 Windsurf/Codeium Context

**How it works:** Windsurf's Cascade has an automatic memory system that identifies useful context during conversations and stores it for future sessions.

**Architecture:**

- Memories stored at `~/.codeium/windsurf/memories/`
- Auto-generate toggle: Cascade autonomously generates memories, or only when explicitly asked
- Global rules (`global_rules.md`) and project rules (`.windsurfrules.md`)
- Structured records covering: user stories, architectural decisions, process changes, technical standards, troubleshooting steps

**Key insight for Impulse:** Windsurf's auto-generate toggle is a good idea. Some users want full automation; others want control. Impulse could add a `autoExtract: boolean` config option (default: true). When false, the session-end hook still runs but only writes to HISTORY_INDEX.md, not GENOME.md.

**Pattern worth adopting:** Windsurf stores memories in categorized structures (user stories, decisions, standards, troubleshooting). This maps directly to the semantic grouping recommended in Section 3.4.

### 4.3 Cline Context Management

**How it works:** Cline uses a "Memory Bank" system -- structured markdown files that track project details, technical decisions, progress milestones, and active session state.

**Architecture:**

- Memory Bank files live in the project directory (like Impulse's `.impulse/`)
- `.clinerules` file defines when to perform context handoff
- `new_task` tool enables clean session creation with preloaded context
- Adaptive context window management: `maxAllowedSize = Math.max(contextWindow - 40_000, contextWindow * 0.8)`

**Key insight for Impulse:** Cline's `new_task` tool (starting a new session with structured context preloaded from memory) is essentially what Impulse's session-start hook does. But Cline goes further: it defines _when_ to create a new context (at 50% window usage), not just what to inject. Impulse could add a similar threshold: if estimated injection + conversation so far exceeds 80% of context window, trigger compaction proactively.

**Pattern worth adopting:** The `.clinerules` concept -- user-defined rules that modify memory behavior. For Impulse, this could be a `.impulse/rules.md` file where users specify: "Always remember database decisions," "Never remember debugging steps," "Priority: auth patterns > UI choices."

### 4.4 aider Context/Memory

**How it works:** aider takes a fundamentally different approach: instead of extracting memories from conversations, it builds a **repository map** -- a structural summary of the entire codebase using tree-sitter to extract symbol definitions.

**Architecture:**

- Repo map is generated from source code using tree-sitter
- Graph ranking algorithm (like PageRank) identifies most important files
- Map size dynamically adjusts based on chat state (larger when no files in context, smaller when specific files are loaded)
- Default token budget: ~1K tokens for the map
- No persistent memory of past conversations

**Key insight for Impulse:** aider solves a different problem -- it gives the LLM structural awareness of the codebase (what classes exist, what functions are defined) rather than historical awareness (what decisions were made). These are complementary. GENOME.md stores _why_ the code is structured a certain way. aider's repo map shows _how_ it is structured now.

**Pattern worth adopting:** aider's dynamic sizing based on context state. Impulse's `maxInjectionTokens: 2000` is a fixed budget. It could be dynamic: inject more GENOME.md content when the conversation is short (agent needs more context), less when the conversation is long (save room for actual work).

### 4.5 Patterns Worth Adopting

Synthesizing across all four competitors:

| Pattern                                      | Source                    | Impulse Applicability                                    | Phase   |
| -------------------------------------------- | ------------------------- | -------------------------------------------------------- | ------- |
| Few-shot examples in extraction prompt       | Cursor, Windsurf          | Direct -- add to `buildExtractionPrompt`                 | Phase 1 |
| User control toggle (auto vs. manual memory) | Windsurf                  | Config option: `autoExtract: boolean`                    | Phase 1 |
| Categorized memory structure                 | Windsurf, Cline           | Semantic grouping in GENOME.md                           | Phase 1 |
| Global + project rules hierarchy             | Cursor, Windsurf          | `~/.impulse/GENOME.md` + `.impulse/GENOME.md`            | Phase 2 |
| Dynamic injection sizing                     | aider                     | Adjust `maxInjectionTokens` based on conversation length | Phase 2 |
| User-defined extraction rules                | Cline (.clinerules)       | `.impulse/rules.md` for extraction preferences           | Phase 2 |
| MCP-based memory access                      | Cursor (via Basic Memory) | Expose GENOME.md as MCP tool instead of prompt injection | Phase 3 |
| Structural code map                          | aider (repo map)          | Complement GENOME.md with auto-generated code structure  | Phase 3 |

---

## 5. Single-Call vs Pipeline: When to Upgrade

### 5.1 Quality Comparison (estimated)

| Dimension                    | Single-Call (Impulse)                    | mem0 Pipeline                                       | Delta       |
| ---------------------------- | ---------------------------------------- | --------------------------------------------------- | ----------- |
| **Fact extraction accuracy** | ~75-85% (LLMs are good at summarization) | ~85-90% (few-shot + JSON format + dedicated prompt) | +5-15%      |
| **Deduplication**            | ~60-70% (40-char substring match)        | ~90-95% (vector similarity + LLM classification)    | +20-35%     |
| **Contradiction resolution** | 0% (cannot detect)                       | ~70-80% (LLM-based DELETE/UPDATE)                   | +70-80%     |
| **Knowledge freshness**      | Degrades over time (no pruning)          | Maintained (UPDATE replaces stale facts)            | Significant |
| **Noise rate**               | ~15-25% (no validation)                  | ~5-10% (two-phase filtering)                        | -10-15%     |
| **Overall utility**          | Good for 0-50 sessions                   | Good for 0-1000+ sessions                           |             |

**Note:** These are estimates based on architectural analysis, not empirical measurements. Building the evaluation framework (Section 2.4) would produce actual numbers.

### 5.2 Cost Comparison

| Dimension                | Single-Call          | mem0 (vector only)                      | mem0 (with graph)     |
| ------------------------ | -------------------- | --------------------------------------- | --------------------- |
| **LLM cost per session** | ~$0.0015             | ~$0.002                                 | ~$0.005               |
| **Infrastructure**       | None (file I/O)      | Vector store + SQLite + embedding model | + Neo4j               |
| **Dependencies**         | 1 (Zod)              | ~20+ Python packages                    | + neo4j + rank_bm25   |
| **Disk usage**           | ~10KB (3 text files) | ~50-200MB (vector indices + SQLite)     | + Neo4j data          |
| **Setup complexity**     | Zero config          | Moderate (API keys, model selection)    | High (Neo4j instance) |
| **Latency per session**  | ~1-2s (one LLM call) | ~3-8s (multiple calls + embeddings)     | ~10-20s               |

### 5.3 Trigger Conditions for Upgrading

Do NOT upgrade based on session count alone. Upgrade when you observe these symptoms:

| Trigger                                | Signal                                                          | Upgrade Path                                                       |
| -------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------ |
| **GENOME.md > 200 lines**              | File getting large, injection consuming too much context window | Add pruning (Section 3.3, Strategy 3)                              |
| **Contradictory decisions accumulate** | Agent gets confused by conflicting guidance                     | Add contradiction detection to extraction prompt (Section 2.5, #4) |
| **Users report stale context**         | "The AI keeps suggesting we use X but we switched to Y"         | Add GENOME.md-aware extraction + UPDATE logic                      |
| **HISTORY_INDEX.md > 100 sessions**    | Need to search past sessions, linear scan too slow              | Add FTS5 (Phase 2 search)                                          |
| **3+ agents regularly active**         | File locking in LIVE_STATE.json becomes insufficient            | Add proper conflict resolution (possibly mem0-style)               |
| **Cross-project knowledge needed**     | Same patterns/decisions apply to multiple projects              | Add global GENOME.md (Phase 2)                                     |
| **GENOME.md > 500 lines**              | Semantic search needed to find relevant decisions               | Add sqlite-vec embeddings (Phase 2)                                |

### 5.4 Migration Path

The migration from single-call to pipeline should be incremental, not a rewrite:

**Step 1: Improve the single call (0 new dependencies)**

- Add few-shot examples to extraction prompt
- Switch to JSON response format
- Add beginning+end transcript sampling
- Feed existing GENOME.md context into extraction call
- These changes get 60-70% of mem0's quality at 0% of the infrastructure cost.

**Step 2: Add contradiction detection (0 new dependencies)**

- When extraction prompt receives existing GENOME.md, ask it to identify:
  - Decisions that CONTRADICT existing entries (mark for UPDATE)
  - Decisions that are already recorded (mark as SKIP)
  - Genuinely new decisions (mark for ADD)
- This is a "poor man's mem0" -- one LLM call doing extraction + classification.

**Step 3: Add semantic deduplication (1 new dependency: embedding model)**

- Before appending to GENOME.md, embed the new decision
- Compare embedding against existing GENOME.md entries
- Only append if cosine similarity < 0.85 with all existing entries
- This requires an embedding API call (~$0.0001 per decision) but no vector store.

**Step 4: Add persistent search (1 new dependency: better-sqlite3)**

- When HISTORY_INDEX.md exceeds 100 entries, create an FTS5 index
- Enable `search_history(query)` function for agents
- Still no vector store -- FTS5 handles keyword search well.

**Step 5: Full mem0 integration (many dependencies, only if needed)**

- Deploy mem0 as a sidecar or use the hosted API
- Replace GENOME.md append logic with mem0's ADD/UPDATE/DELETE pipeline
- Keep GENOME.md as the human-readable "view" of mem0's vector store
- Add MCP server for agent-side memory queries.

---

## Key Findings

1. **mem0's quality advantage comes from contradiction resolution and semantic deduplication, not fact extraction.** The extraction step itself is a single LLM call in both systems -- mem0 just has better prompts and JSON formatting.

2. **Impulse's extraction prompt needs few-shot examples and JSON output.** These two changes alone would close ~50% of the quality gap with zero infrastructure cost.

3. **The 40-character fingerprint deduplication is a known weakness.** It will produce duplicates for semantically identical but differently worded decisions, and false positives for decisions that share opening text.

4. **Front-truncation of transcripts will lose early-session decisions.** The fix (beginning+end sampling) is trivial to implement.

5. **Feeding existing GENOME.md into the extraction call enables basic contradiction detection** within a single LLM call. This is the highest-leverage improvement for Phase 1.

6. **All four competitors (Cursor, Windsurf, Cline, aider) use file-based persistence, not vector databases, for cross-session coding context.** This validates Impulse's "three files" architecture.

7. **The core value hypothesis is supported by research but unproven for the specific Impulse use case.** Building even a minimal evaluation framework would provide valuable confidence.

8. **Information decay is real and will be a problem after 50+ sessions.** Plan for pruning before it becomes urgent.

9. **The cost difference between single-call and mem0 is 1.3-4x, not 10-20x.** The real cost of mem0 is infrastructure, not LLM calls.

10. **The upgrade path from single-call to pipeline is incremental.** Each step adds value independently, and no step requires abandoning previous work.

---

## Implications for Impulse

### Immediate Actions (Phase 1, before shipping)

1. **Add 2-3 few-shot examples to `buildExtractionPrompt()`** -- highest impact, lowest cost improvement
2. **Switch to JSON response format** in the extraction prompt and update `parseExtraction()` to parse JSON
3. **Implement beginning+end transcript sampling** instead of tail-only truncation
4. **Feed existing GENOME.md content** into the extraction call with instructions to identify contradictions

### Short-term Actions (Phase 1.5, after initial usage)

5. **Build the evaluation framework** (Section 2.4) with 5-10 annotated test sessions
6. **Run the core value hypothesis test** (Section 3.5) even informally
7. **Add the `autoExtract` config toggle** for users who want manual control
8. **Track GENOME.md line count** and warn when approaching 200 lines

### Medium-term Actions (Phase 2, based on observed triggers)

9. **Add embedding-based deduplication** when near-duplicate decisions are observed
10. **Implement LLM-assisted pruning** (Strategy 3 from Section 3.3) every 20 sessions
11. **Add global GENOME.md** for user-wide preferences
12. **Add FTS5 for HISTORY_INDEX.md** when it exceeds 100 sessions

---

## Quality Metrics and Evaluation Framework

### Metrics to Track from Day 1

| Metric                  | How to Measure                       | Healthy Range                                                  |
| ----------------------- | ------------------------------------ | -------------------------------------------------------------- |
| Decisions per session   | Count at extraction time             | 0-5 (most sessions), 5-15 (architecture sessions)              |
| GENOME.md line count    | Read at session start                | < 200 lines                                                    |
| Dedup rejection rate    | Count in `deduplicateDecisions()`    | 10-40% (lower = good extraction, higher = too much repetition) |
| Extraction failure rate | Count catch blocks in `onSessionEnd` | < 5%                                                           |
| Injection token count   | Already logged in `onSessionStart`   | < 1500 tokens (of 2000 budget)                                 |
| Session summary length  | Measure `summary` field length       | 100-500 characters                                             |

### Metrics to Add in Phase 2

| Metric                    | How to Measure                                  | Healthy Range                                |
| ------------------------- | ----------------------------------------------- | -------------------------------------------- |
| Contradiction rate        | Extraction prompt identifies contradictions     | < 10% of sessions                            |
| GENOME.md "read" rate     | How much of GENOME.md does the agent reference? | > 30% of injected content should be relevant |
| Decision staleness        | Age of oldest unreferenced decision             | < 90 days                                    |
| Cross-session consistency | Does agent behavior match GENOME.md guidance?   | > 80% consistency                            |

---

_This analysis is based on source code review of mem0 v0.x (cloned at `cloned-repos/mem0/`), Impulse's `impulse-plugin/src/utils/extraction.ts`, and competitive research conducted on 2026-02-20. Cost estimates use GPT-4o-mini pricing as of February 2026. Quality estimates are architectural projections, not empirical measurements._
