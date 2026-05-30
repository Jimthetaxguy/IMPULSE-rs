---
status: active
phase: all
audience: builder
tags: [research, digest, summary]
last_updated: 2026-03-31
---

# Research Digest: Validated Technical Findings

> **Version:** 1.1 | **Status:** Reference | **Updated:** 2026-03-31
> **Sources:** Deep research, claude-memory plugin analysis, benchmark reports, Mem0 paper
> **Purpose:** Permanent reference for all validated technical decisions

---

## 1. Claude Code JSONL Storage Architecture

### Structure

```
~/.claude/
  history.jsonl                    -- Global index (every prompt, all projects)
  projects/
    <project-hash>/
      <session-id>.jsonl           -- Complete conversation transcript
```

### Key Properties

| Property | Value | Source |
|----------|-------|--------|
| **Format** | JSONL (one JSON object per line) | Claude Code docs |
| **Content ratio** | ~25% meaningful / ~75% tool metadata noise | claude-memory plugin analysis |
| **Incremental parsing** | Trivial (each line independent) | JSONL spec |
| **File size** | 5-10x larger than actual conversation | Reddit analysis |
| **Persistence** | Compaction does NOT modify on-disk JSONL | Claude platform docs |
| **Location** | `~/.claude/projects/<hash>/<uuid>.jsonl` | File system inspection |

### Noise Filtering Strategy

75% of JSONL content is tool call metadata that adds noise to retrieval. Filter aggressively:

**KEEP:** User messages, assistant reasoning (non-tool), error messages, file modification summaries
**DISCARD:** Raw tool call JSON, verbose stdout captures, intermediate execution state, tool results > 5000 chars

### Implication

JSONL is the Level 1 source of truth (immutable, authoritative). All derived stores (embeddings, mem0 facts, summaries) are regenerable from JSONL. Never mutate JSONL files.

---

## 2. Vector Store Comparison

### Benchmark Results

| Store | Write Latency | Read Latency | Recall | Operational Complexity | Size |
|-------|--------------|-------------|--------|----------------------|------|
| **SQLite-vec** | 788ms avg | Sub-100ms (KNN) | Good | Zero config, single file | ~3MB extension |
| **Faiss** | 47,640ms avg | Fastest at scale | Best | Library, no server | Varies |
| **Chroma** | Moderate | Moderate | Lowest | Client-server, reliability issues | Heavy |
| **pgvector** | Moderate | Good | Good | Requires PostgreSQL | N/A |

### SQLite-vec: Why It Wins

- **60x faster writes** than Faiss (788ms vs 47,640ms). Critical for continuous ingestion.
- **Single-file storage.** No server process. Zero configuration. ~3MB C extension.
- **Sub-100ms KNN queries** for hundreds to low thousands of vectors. Sufficient for months of coding sessions.
- **OLTP-optimized** (append-only writes). Faiss is OLAP-optimized (requires full rewrite on insert).

### SQLite-vec: Known Limitations

- No UPSERT on virtual tables. Must DELETE + INSERT.
- No incremental index updates. Full scan for small datasets is fine.
- Not suitable for millions of vectors (Faiss better at that scale).
- Requires C extension loading (platform-specific binaries).

### Verdict

SQLite-vec for Phase 2+. For Phase 1, no vector store needed (GENOME.md + grep is sufficient).

---

## 3. Mem0 Architecture Analysis

### How It Works

Mem0's breakthrough: **uses the LLM as an intelligent memory manager**, not just an embedder.

```
Conversation Turn
    |
    v
LLM: "Extract meaningful facts" (not raw chunks)
    |
    v
LLM: "Memory management decision" (ADD / UPDATE / DELETE / NONE)
    |
    v
Vector DB: Store/update embedding
Graph DB: Update entity relationships (optional)
SQLite: Audit trail of all operations
```

### Benchmark Performance (LOCOMO)

| Metric | Mem0 | OpenAI Memory | Improvement |
|--------|------|---------------|-------------|
| Accuracy | F1=28.64 | Baseline | +26% |
| Response time | Fast | Baseline | 91% faster |
| Token usage | Low | Full context | 90% reduction |

### Cost Model

Each memory operation triggers 2 LLM calls:
- Fact extraction: ~500 tokens (prompt) + conversation length
- Memory update decision: ~200 tokens (output)

For a 10-turn conversation: ~20 LLM calls for memory processing.
At GPT-4o-mini pricing: ~$0.02-0.05 per session.
Token savings during retrieval (90% reduction) offset cost in long-running projects.

### The Lean Alternative (Phase 1)

Instead of full Mem0 pipeline, use a single session-end LLM call:

```
Prompt: "Extract architectural decisions, preferences, and constraints
         from this session. Format as markdown bullet points."
Cost: 1 LLM call (~$0.01)
Quality: ~70-80% of Mem0 accuracy (good enough for MVP)
```

Add full Mem0 only when:
- Contradictions emerge between sessions (Mem0 resolves automatically)
- Entity relationships matter (Mem0 uses graph DB)
- 100+ sessions make manual GENOME.md maintenance unwieldy

---

## 4. MCP Server Patterns

### Architecture

```
Your Tool (MCP Client)
    |
    | JSON-RPC over stdio/HTTP
    |
    v
MCP Server (exposes tools)
    |
    v
Backend (SQLite, API, etc.)
```

### Tool Exposure Pattern

```typescript
// MCP server exposes tools that Claude can call
const tools = [
  {
    name: 'search_conversation_history',
    description: 'Search past coding sessions by keyword or semantic similarity',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string' },
        project: { type: 'string' },
        limit: { type: 'integer', default: 10 },
      },
    },
  },
];
```

### Interceptor/Middleware Pattern

MCP interceptors chain like middleware:
- Outer interceptor wraps inner interceptor wraps actual tool handler
- Enables: logging, authentication, retry logic, context injection
- Keeps core retrieval logic clean

### Phase 1 Alternative

Agents can read .impulse/ files directly without MCP. MCP is only needed when:
- Search across 100+ sessions (file reading is too slow)
- Multiple agent types need standardized access
- External tools need to query impulse data

---

## 5. Retrieval Patterns (Three Complementary)

### Pattern 1: Session Resume (Zero-Shot Context)

On session start, inject compressed context from most recent related session.

```
On startup:
  Read last session summary from HISTORY_INDEX.md
  Read permanent facts from GENOME.md
  Inject both into system prompt
  Agent has context without any tool calls
```

Best for: Project continuity (Monday morning "where was I?")

### Pattern 2: MCP Tool Search (Agentic Retrieval)

Expose search as MCP tool. Claude decides when to search.

```
User: "What API endpoint did we use for auth last week?"
Claude: [calls search_history("auth API endpoint", days_back=7)]
Result: Exact conversation snippet with implementation details
```

Best for: On-demand recall of specific decisions/implementations.

### Pattern 3: Mem0 Persistent Memory (Cross-Session Learning)

Extract and store high-level decisions that persist indefinitely:
- "James prefers TypeScript for backend services"
- "Project uses PostgreSQL 14 with pgvector extension"

These survive compaction and inform future sessions without re-retrieving raw conversations.

Best for: Accumulated project knowledge that shouldn't require search.

### Phase Mapping

| Pattern | Phase 1 | Phase 2 | Phase 3 |
|---------|---------|---------|---------|
| Session Resume | GENOME.md + HISTORY_INDEX.md | Same + FTS5 | Same + Mem0 |
| MCP Search | Agents read files directly | MCP server + sqlite-vec | MCP + graph queries |
| Persistent Memory | Session-end LLM extraction | Same + contradiction detection | Full Mem0 pipeline |

---

## 6. Chunking Strategy for Conversations

### What Doesn't Work

| Strategy | Problem |
|----------|---------|
| Fixed-size chunks (512 tokens) | Breaks mid-conversation, loses context |
| Recursive splitting | Same problem at different granularity |
| Pure semantic splitting | Loses turn boundaries, confuses retrieval |

### What Works: Turn-Level Chunking

Each user message + assistant response + tool calls = one chunk.

```
Chunk 1: [User asks about auth] + [Assistant explains JWT approach]
Chunk 2: [User asks about session refresh] + [Assistant implements]
Chunk 3: [User reports bug] + [Assistant debugs and fixes]
```

Preserves: conversational coherence, decision context, cause-and-effect
Embed: cleaned text (tool noise removed) with metadata (timestamp, files, branch)

### Session-Level Summaries

Store auto-generated summaries as separate chunks with higher retrieval weight.
Provides high-level context before drilling into specific turns.

---

## 7. Compaction Behavior (What Survives)

### Key Insight

Compaction only affects the in-session context window, NOT on-disk JSONL.

```
During session:
  Claude's context window fills up (~150K tokens)
  Compaction triggers:
    1. Generate summary of conversation so far
    2. Replace early messages with summary
    3. Continue from summary forward

On disk:
  JSONL file remains UNCHANGED
  All original turns preserved
  RAG can retrieve anything compaction removed
```

### Implication for Impulse

The compaction hook is the most valuable integration point:
- Before compaction, inject "must survive" content (from GENOME.md)
- After compaction, RAG can reintroduce compacted content on-demand
- JSONL is always the authoritative source (Level 1 truth)

---

## 8. Performance Engineering Guidelines

### Embedding Model Selection

| Model | Dimensions | Speed | Quality | Cost | Offline |
|-------|-----------|-------|---------|------|---------|
| all-MiniLM-L6-v2 | 384 | Fast | Good | Free | Yes |
| nomic-embed-text | 768 | Medium | Better | Free | Yes |
| text-embedding-3-small | 1536 | API latency | Best | $$  | No |

**Recommendation:** Start with 384-dim local model. Upgrade only if retrieval quality suffers.

### Indexing Cadence

| Strategy | Latency | Completeness | Battery Impact |
|----------|---------|-------------|----------------|
| Background (session end) | 5-10s sync on exit | Always current on restart | Zero during coding |
| Real-time (per turn) | 50-100ms per turn | Immediately current | Moderate |
| On-demand (manual) | User-triggered | May be stale | Zero |

**Recommendation:** Background (session end). Zero performance impact during coding. Terminal-native feel.

### Retrieval Limits

- K=3-5 for focused queries ("how did we implement auth?")
- K=10-15 for exploratory searches ("what did we work on last week?")
- Beyond 15: show list for user selection rather than dumping into context

---

## 9. Production Implementation Reference: claude-memory Plugin

### What It Proves

A production implementation already exists that validates the architectural approach:

1. **Automatic context injection on session start** works via hooks
2. **FTS5-powered keyword search** is sufficient for most queries
3. **Raw JSONL is too slow** for frequent use (preprocessing required)
4. **Conversation branch detection** matters (JSONL has non-linear structures)
5. **Single-exchange sessions should be filtered** (noise reduction)

### Architecture

```
Session End:
  JSONL parsed --> Tool noise stripped --> Branches detected
  --> Cleaned text stored in SQLite --> FTS5 index updated

Session Start:
  Hook queries SQLite for recent sessions in same project
  Filters out single-exchange sessions
  Injects cleaned context into system prompt
  Zero manual intervention
```

### Key Takeaway

The claude-memory plugin is proof that the "Three Files and a Hook" approach works. It's essentially the same architecture with a SQLite backend instead of plain files.

---

## 10. Open Research Questions (For Future Phases)

| Question | Current Best Answer | Confidence |
|----------|--------------------|------------|
| Do 384-dim embeddings suffice for code? | Probably (high keyword density) | Medium |
| Does Mem0 improve on session-end LLM extraction? | Yes (+26% accuracy), but at 20x cost | High |
| Is graph DB (Neo4j) needed for code projects? | Unlikely for single-developer, maybe for teams | Low |
| Can FTS5 replace vector search for coding contexts? | For keyword queries, yes (80% of use cases) | High |
| Does vector injection help beyond file-locking? | Unknown (need A/B test with real agents) | Low |

---

## 11. Documentation Frameworks

### What Works

| Approach | Why It Works |
|----------|--------------|
| **Diataxis as the top-level taxonomy** | Separates learning content from task execution and low-level reference, so readers do not have to guess whether a page should teach, explain, or specify |
| **One canonical contract plus supporting guides** | Keeps product truth in a single place while allowing operational guides, roadmaps, and research notes to evolve without pretending they are specs |
| **Generated navigation backed by human-owned source docs** | Lets `SUMMARY.md` stay mechanically consistent while authors still edit the real documents directly |
| **Explicit status + phase metadata in frontmatter** | Makes it easier to sort docs by roadmap relevance and identify stale material during audits |

### Key Findings

1. **Diataxis is the right organizing framework for this repo, but only if each document has a single job.** Tutorials, how-to guides, explanations, and references should not be blended into one long page when the user intent is different.

2. **A canonical contract is more valuable than a large volume of narrative docs.** For Impulse, `docs/spec/RUST-CANONICAL-CONTRACT.md` should remain the source of truth, while plans, handoffs, and research docs should point back to it instead of restating product behavior.

3. **Navigation should be generated; judgment should stay manual.** `docs/SUMMARY.md` is useful as a machine-consistent index, but stale-doc detection and category decisions still need human review because auto-generated summaries cannot detect semantic drift.

4. **Frontmatter is not cosmetic metadata.** Status, phase, audience, tags, and update dates make documentation auditable. Without them, stale-but-plausible docs are hard to spot during roadmap transitions.

5. **Research docs should end in decisions or open questions, not just collected notes.** The strongest documents in this repo translate raw analysis into implications, phase impact, or unresolved risks. That pattern should be preserved.

### Implications for Impulse Docs

- Keep the spec/reference surface small and authoritative.
- Move task-oriented operational material into guides or handoffs, not specs.
- Treat generated indexes as navigational views, not editorial truth.
- Prefer adding short "Implication" or "Decision" subsections when a research note would otherwise stop at description.

---

## 12. Rust UI and UX Best Practices

### Framework Fit

| Surface | Best Fit | Why |
|--------|----------|-----|
| **Operator desktop shell / dashboard** | Tauri + Dioxus + xterm.js | Webview UI gives better product-shell layout while xterm.js owns terminal rendering and Rust owns PTY/session state |
| **Terminal-native workflows** | `ratatui` | Constraint-based layouts, keyboard-first flows, and low-latency rendering fit terminal operations well |
| **Embedded PTY terminal widgets** | Dedicated crate (`impulse-term`) | The PTY surface has correctness and rendering constraints that should not be buried inside broader app views |

### Key Findings

1. **Separate correctness work from cosmetic work.** In the current plan, the highest-leverage UI work is fixing hot-path correctness issues first (`unwrap` removal, backend error visibility, thread/test gaps). UX polish should sit on top of a stable terminal core, not compensate for it.

2. **Information density only works when hierarchy is obvious.** Status bars, budget indicators, and context overlays should expose live agent state without forcing the user to parse a wall of equal-weight signals. Promote only the few metrics that change decisions.

3. **Small, dedicated view modules scale better than monolithic render functions.** `TerminalPanel::show()`-style accumulation makes UI changes risky. Extracting focused widgets like `StatusBar` lowers review cost, testing surface, and future UX iteration risk.

4. **The welcome state is a product surface, not filler.** Empty or generic launch screens waste the moment when the operator needs orientation. Good Rust tooling UIs should use the initial state to show recent projects, live system status, and the next useful action.

5. **Keyboard-first interaction should remain the default even in richer native UI.** Mouse support is useful, but operator tooling still benefits most from predictable shortcuts, focus clarity, and commandable actions.

6. **Animations should confirm state changes, not decorate the screen.** Subtle fades, pulses, and delayed repaints are enough. If motion does not reveal freshness, focus, or success/failure state, it adds noise.

7. **Backend failures must surface in the UI as signals, not silent degradation.** For PTY-backed Rust interfaces, the worst UX failure is a dead or stale panel that looks healthy. Error counts, reconnect state, and degraded-mode indicators are worth the screen space.

8. **Choose UI technology per interaction model, not by ecosystem enthusiasm.** Tauri+Dioxus is now the active desktop product shell, xterm.js owns desktop terminal rendering, and `ratatui` remains first-class for terminal-native workflows. `egui` remains historical/legacy context only.

### Practical Rules for This Repo

- Extract reusable widgets before adding more panel-level complexity.
- Expose health, lag, and error state explicitly in status surfaces.
- Keep welcome/empty states operational: recent context, quick actions, current system truth.
- Add motion only with existing primitives unless a real interaction gap justifies more dependencies.
- Test state logic aggressively even when the UI framework itself is hard to unit test.

---

## References

- claude-memory plugin: Reddit analysis
- SQLite-vec benchmarks: GitHub meta-llama/llama-stack issues
- Chroma reliability: BlueTeam AI vector benchmarking report
- Mem0 LOCOMO benchmark: arXiv paper 2504.19413
- Mem0 architecture: Southbridge AI technical analysis
- MCP patterns: Stytch blog, LangChain docs
- JSONL storage: Kent Gigger blog post
- Compaction behavior: Anthropic platform cookbook
- Documentation frameworks: `docs/AI build_complete_guide.md` (§MD-21.4), `docs/SUMMARY.md`, `docs/DOC-PLAN.md`
- Rust UI/UX guidance: `ralph-plan-4.md`, `docs/research/TERMINAL-LAYER-ANALYSIS.md`, `docs/research/cross-model-consensus.md`

---

_Created: 2026-02-20 | Updated: 2026-03-31 | Status: Permanent Reference v1.1_
