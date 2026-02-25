---
title: Impulse Memory Architecture
description: Full conversation history RAG system design for cross-session memory
version: '1.0'
updated: 2026-02-20
type: research
category: architecture
phase: phase2
status: active
audience: builders
tags: [memory, rag, session-history, vector-search]
---

# Impulse Memory Architecture: Full Conversation History RAG System

## The Problem in Sharp Focus

Claude Code's auto-compaction rewrites your in-session context when you approach the token ceiling—typically around the 100k–150k threshold. The compacted summary replaces older messages, and while you see a notification, you can't scroll back past the compaction boundary. The raw data, however, is _never deleted_—every prompt, every assistant response, every tool call lives as JSONL on disk at `~/.claude/projects/<project-hash>/<session-id>.jsonl`. The conversation index at `~/.claude/history.jsonl` timestamps every prompt across all projects. This means there's a rich, untapped corpus of your entire development history sitting in plain text, and the only tools most people use to access it are `/resume` and `grep`.[^1][^2][^3][^4][^5]

The architecture below turns that dead data into a living memory system that flows context back into active sessions—automatically, selectively, and at the granularity you choose.

---

## Existing Tools Already in the Wild

Before building anything custom, it's worth mapping what already exists as off-the-shelf MCP servers for Claude Code history retrieval:

### claude-historian (Vvkmnn)

A TypeScript MCP server that provides six specialized tools for searching conversation history:[^6][^7]

- `search_conversations` — Full-text query across all sessions with TF-IDF scoring
- `find_file_context` — Track changes and discussions about specific files
- `get_error_solutions` — Pattern-match against past error resolutions
- `find_similar_queries` — Fuzzy match to find when you've asked something similar before
- `list_recent_sessions` — Chronological session browser
- `find_tool_patterns` — Discover your own tool-use habits (Read → Edit → Bash combos, etc.)

The architecture is notable: pure streaming JSON parser, LRU caching, Naive Bayes query classification (error/implementation/analysis/general), exponential time decay for recency bias, and edit-distance fuzzy matching. Zero persistent storage—it reads directly from `~/.claude/conversations/` on every query. MIT licensed, installable with one command: `claude mcp add claude-historian -- npx claude-historian`.[^7]

### claude-code-history-mcp (yudppp)

A more structured MCP server offering four tools:[^8][^9]

- `list_projects` — Discover all projects with history metadata (session counts, message counts, last activity)
- `list_sessions` — Browse sessions within a project
- `get_conversation_history` — Paginated retrieval with message type filtering (`user`, `assistant`, `tool_use`)
- `search_conversations` — Keyword search with result limits

This one is better for systematic bulk retrieval—pulling entire session transcripts for downstream processing rather than one-off searches.[^9]

### Historian (MCP Market)

Uses SQLite FTS5 (full-text search) with advanced features: prefix matching, stemming, fuzzy search, and time decay. This is the only existing tool that actually creates a persistent local index rather than streaming raw JSONL on every query. It tracks file discussions, error patterns, and tool usage analytics.[^10]

### claude-history (thejud)

Python CLI utility (not MCP) that parses JSONL and outputs chronological Markdown with optional assistant responses. Useful as a data extraction stage feeding into your own pipeline.[^11]

### Zen MCP Server Integration

The Zen MCP server has a community solution for piping full Claude Code context into external LLM calls—reading session JSONL and exporting clean JSON files that other tools can consume.[^12]

**Key takeaway**: These tools cover _retrieval_ well but none of them solve _intelligent injection back into active sessions_. That's the gap your impulse fills.

---

## Five-Tier Memory Architecture

Rather than a monolithic RAG pipeline, the design uses five tiers that operate independently but compose together. Each tier has different latency, storage cost, and intelligence characteristics.

### Tier 0: Ephemeral Session Memory (Already Built-In)

**What it is**: Claude Code's native context window + compaction mechanism.
**Latency**: 0ms (it's the active context).
**Storage**: In-memory, auto-compacted when approaching token threshold.[^13][^3]
**Smart move**: Use `/compact focus on X` to guide what survives compaction. Put persistent instructions in `CLAUDE.md` since it survives all compactions.[^1]

### Tier 1: Session Transcript Search (Install Today)

**What it is**: claude-historian or Historian FTS5 MCP server reading raw JSONL files.
**Latency**: 50–200ms per query (streaming parse or FTS5 index lookup).
**Storage**: Zero additional (reads existing files) or ~5MB for FTS5 index.
**How Claude uses it**: Agentic tool calls. When Claude encounters an unfamiliar pattern or the user references "what we did last week," it calls `search_conversations` or `get_error_solutions` automatically.[^7]
**Installation**:

```bash
claude mcp add claude-historian -- npx claude-historian
```

### Tier 2: Semantic Vector Index (Custom Build — The Core Innovation)

**What it is**: An embedding-powered vector store over chunked conversation history, exposed as an MCP tool.
**Latency**: 10–50ms retrieval after initial indexing.
**Storage**: ~50–200MB depending on history size (embeddings + SQLite-vec database).
**Why it matters**: FTS5/TF-IDF from Tier 1 finds _lexical_ matches. Tier 2 finds _semantic_ matches. When you ask "how did we handle the authentication flow?" it retrieves conversations about "OAuth token refresh," "JWT validation," and "session cookie management" even though none of those contain the word "authentication."

**Technical design**:

**Embedding model**: `all-MiniLM-L6-v2` via sentence-transformers (22MB model, runs locally, 384-dimensional vectors). Alternative: `nomic-embed-text` via Ollama for slightly better quality at the cost of ~500MB model weight.[^14][^15]

**Vector store**: SQLite-vec — a single-file SQLite extension (~3MB) that adds vector similarity search. The entire index lives in one `.db` file under your project directory. LangChain has a native `SQLiteVec` integration:[^16][^14]

```python
from langchain_community.embeddings.sentence_transformer import SentenceTransformerEmbeddings
from langchain_community.vectorstores import SQLiteVec

embedding = SentenceTransformerEmbeddings(model_name="all-MiniLM-L6-v2")
db = SQLiteVec(table="conversations", db_file=".impulse/memory.vec.db", embedding=embedding)

# Index a chunk
db.add_texts(["We fixed the Redis connection pool by switching from ioredis to redis-om..."],
             metadatas=[{"session_id": "abc123", "timestamp": "2026-02-18T14:30:00", "project": "my-project"}])

# Retrieve
results = db.similarity_search("connection pooling issues", k=5)
```

**Chunking strategy**: Use _sentence-based semantic chunking_ rather than fixed-size splits. For conversation data specifically:[^17]

- Each **user prompt + assistant response pair** forms a natural chunk (preserves the question/answer unit)
- Long assistant responses get split at paragraph boundaries with 10–15% overlap[^18][^17]
- Tool call results (file reads, bash outputs) get summarized to ~200 tokens before embedding, since raw tool output is high-volume/low-signal
- Compaction summaries themselves are high-value chunks—they represent Claude's own distillation of what mattered[^13]

The _late chunking_ technique from Jina AI is particularly interesting here: embed the entire session transcript first using a long-context embedding model, then split into chunks that preserve the global context in each chunk's embedding. This avoids the classic problem where a chunk about "the fix" loses context about what was being fixed.[^19]

### Tier 3: Extracted Memory Layer (Mem0 / A-MEM — Distilled Knowledge)

**What it is**: An LLM-powered extraction layer that reads conversations and distills them into discrete _facts_, _decisions_, _preferences_, and _patterns_.
**Latency**: Extraction is async (30–60s per session), retrieval is 10–20ms.
**Storage**: ~5–20MB (extracted memories are compact text + embeddings).
**Why it matters**: Raw conversation chunks contain a lot of noise—debugging dead ends, thinking out loud, abandoned approaches. Tier 3 extracts only what's _worth remembering_.[^20]

**Two competing architectures**:

**Option A — Mem0 (Production-proven, 42k GitHub stars)**:[^21][^22]

- Apache 2.0 licensed, self-hostable via Docker Compose[^23]
- Three-line API: `memory.add(messages)`, `memory.search(query)`, `memory.get_all(user_id)`[^22]
- Hybrid storage: vector DB for semantic search + optional Neo4j graph for relationship modeling[^24][^23]
- Automatic fact extraction: pass raw conversation, Mem0 uses an LLM to extract discrete memories[^20]
- Multi-level scoping: `user_id` (you), `agent_id` (which Claude instance), `run_id` (session)[^23]
- OpenMemory MCP server available for cross-tool memory sharing[^22]
- Outperforms raw RAG with F1 of 28.64 on LOCOMO multi-session benchmark[^25]
- Self-hosted stack: Postgres pgvector + Neo4j + FastAPI, or minimal mode with just Qdrant/Chroma[^23]

**Option B — A-MEM (Research-grade, Zettelkasten-inspired)**:[^26]

- MIT licensed, 637 stars, Python-based
- Uses Zettelkasten note-linking principles: every memory gets structured attributes (content, tags, context, keywords), then the system automatically finds and links related memories[^26]
- ChromaDB vector storage with automatic metadata handling
- Memory _evolution_: when you add a new memory, the system re-analyzes existing related memories and updates their connections, tags, and context[^26]
- Supports local deployment via Ollama (no cloud API required)[^26]
- Better for building an interconnected _knowledge graph_ of your development history rather than flat memory retrieval

**Creative hybrid**: Run Mem0 for its proven extraction quality and OpenMemory MCP integration, but feed extracted memories _into_ A-MEM's Zettelkasten structure for cross-linking. This gives you both battle-tested extraction and evolving knowledge networks.

### Tier 4: Project Genome (Cross-Session Intelligence — The Creative Leap)

**What it is**: A synthesized, continuously-updated project knowledge document that represents Claude's cumulative understanding of your project.
**Latency**: Read is instant (it's a file), writes happen async after each session.
**Storage**: 10–50KB per project (compressed knowledge).
**Why it matters**: Tiers 1–3 retrieve past _conversations_. Tier 4 retrieves past _understanding_.

**How it works**:

1. After each session ends (detected by a Zellij `session-close` hook or a background watcher on the JSONL directory), a background script runs
2. It feeds the new session transcript + the existing Project Genome file to an LLM with the prompt: _"Update this project knowledge document with any new facts, decisions, patterns, or architectural changes from this session. Remove anything contradicted. Keep it under 5000 tokens."_
3. The updated Genome gets written to `.impulse/genome.md` in the project root
4. On session start, the Genome is automatically injected into `CLAUDE.md` (or loaded via a bootstrapping MCP tool)

**What the Genome contains**:

- **Architecture decisions** with rationale ("We chose SQLite-vec over Chroma because single-file portability matters for the impulse's lightweight constraint")
- **Active bugs and their status** ("Redis connection pool leak — partially fixed, needs load testing")
- **User preferences and patterns** ("Prefer Rust for performance-critical paths, TypeScript for MCP servers")
- **Dependency map** ("Project depends on Zellij 0.42+, Ghostty, mise, onecontext-ai")
- **Current sprint/focus** ("Working on Layer 2 RAG pipeline for conversation history")

This is essentially what OneContext (Junde Wu's version) does with its GCC (Git Context Controller) pattern—but more opinionated and tailored to the impulse's needs.[^27]

---

## Three Novel Retrieval Patterns

### Pattern 1: The Déjà Vu Trigger

Instead of waiting for you or Claude to explicitly search history, this pattern runs a _background similarity check_ on every user prompt against Tier 2's vector index. If a new prompt has cosine similarity > 0.85 with a past conversation chunk, it silently injects a context hint:

```
💡 Similar conversation found from Feb 12 (session auth-refactor-3):
"We solved this by wrapping the retry logic in an exponential backoff decorator..."
```

This surfaces as a floating notification in Zellij (not blocking the main pane). You can ignore it or ask Claude to pull in the full context. The key insight: _the system reminds you of things you've already solved without you needing to remember to ask_.

**Implementation**: A thin MCP proxy that intercepts outgoing prompts, embeds them, queries SQLite-vec, and appends a `system` message with the match if the score exceeds the threshold. The proxy sits between OpenCode and the Anthropic API.

### Pattern 2: The Time Machine

A Zellij floating pane widget that visualizes your conversation history as a navigable timeline. Each node represents a session, color-coded by project. Click/select a node to see the session summary. Press Enter to inject that session's key context into the active Claude instance via MCP tool call.

**Visual concept**:

```
─── Feb 19 ───── Feb 18 ───── Feb 17 ───── Feb 16 ──────
   │                │              │             │
   ├─ Agent proj    ├─ Impulse     ├─ CLI tool   ├─ DB proj
   │  (47 msgs)     │  RAG arch    │  (23 msgs)  │  schema
   │  ★ current     │  (89 msgs)   │             │  (61 msgs)
   │                │              │             │
```

Built as a Zellij Rust plugin (WASM) using the new floating pane coordinate APIs from Zellij 0.42. The plugin reads `sessions-index.json` for metadata and renders with the built-in UI components + mouse hover support.[^28][^29]

### Pattern 3: The Interrogation Room

Rather than dumping raw history into context (wasteful), this pattern lets Claude _ask questions_ about past sessions through a structured MCP tool. The tool returns only the answer, not the full transcript:

```
Tool: query_past_sessions
Input: {
  "question": "What embedding model did we decide on for the vector store?",
  "project_filter": "impulse",
  "time_range": "last_7_days"
}
Output: {
  "answer": "all-MiniLM-L6-v2 was chosen for its 22MB footprint and 384-dim vectors.
   Nomic-embed-text via Ollama was considered but rejected due to 500MB model weight.",
  "source_sessions": ["session-abc123", "session-def456"],
  "confidence": 0.92
}
```

This is essentially RAG with a _generation step on the retrieval side_—the MCP server retrieves relevant chunks, feeds them to a local LLM (or the same Claude instance via a nested call), and returns a synthesized answer. The active session only consumes tokens for the answer, not the raw history. This is the pattern Anthropic's own research showed yields 84% token reduction while improving accuracy by 39%.[^30][^31]

---

## Impulse Integration: Where Everything Lives

### Zellij Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Tab 1: Proj-A   Tab 2: Impulse   Tab 3: Proj-B  Tab 4: Misc│
├─────────────────────────┬───────────────────────────────────┤
│                         │  File Tree (floating, pinned)     │
│                         │  ├── src/                         │
│  OpenCode / Claude Code │  │   ├── memory/                  │
│  Main Editor Pane       │  │   │   ├── indexer.py           │
│                         │  │   │   ├── retriever.py         │
│                         │  │   │   └── genome.py            │
│                         │  │   └── mcp/                     │
│                         │  │       └── history_server.ts    │
│                         │  └── .impulse/                    │
│                         │      ├── memory.vec.db            │
│                         │      ├── genome.md                │
│                         │      └── config.toml              │
├─────────────────────────┴───────────────────────────────────┤
│  Memory Status Bar (Zellij plugin, bottom)                  │
│  🧠 Tier1: ✓ historian  Tier2: 1,247 chunks indexed        │
│  Tier3: 89 memories  Tier4: genome updated 2h ago           │
│  🔔 Sound: ON  │  📊 Context: 47% used  │  ⏱ Session: 34m │
└─────────────────────────────────────────────────────────────┘
```

### Floating Pane: Memory Inspector

Toggled with a hotkey (e.g., `Ctrl+m`), this pinned floating pane shows:

- **Last 5 memories extracted** by Tier 3 (Mem0)
- **Déjà Vu alerts** from Pattern 1
- **Context usage meter** showing how close you are to compaction
- **Quick actions**: "Inject genome", "Search history", "Export session"

Built using Zellij's `change_floating_panes_coordinates` API and `pinned: Some(true)` to keep it always-on-top.[^28]

### Notification System

Three notification channels, all configurable via `.impulse/config.toml`:

1. **Audio alerts** (macOS): `afplay /System/Library/Sounds/Funk.aiff` on task completion. Linux: `paplay` or `aplay`. The `BEL` character (`\a`) works universally but is subtle.[^32][^33][^34]

2. **Visual bell**: The Zellij status bar plugin flashes or changes color. The Memory Status Bar turns green when Claude finishes, amber during compaction, red on errors.

3. **System notifications**: `osascript -e 'display notification "Claude finished refactoring auth module" with title "Impulse"'` on macOS. Linux: `notify-send`. These appear even when the terminal is backgrounded.[^33]

The pattern from the Claude Code community that works cleanly: add to your `CLAUDE.md`:[^34]

```
## IMPORTANT: Completion Notification
After completing any task, run: afplay /System/Library/Sounds/Funk.aiff
```

For a more sophisticated approach, use Claude Code _hooks_ (event-based scripts that fire on specific lifecycle events like task completion, permission requests, subagent spawns). These can trigger different sounds for different event types.[^34]

---

## The Indexing Pipeline: From JSONL to Searchable Memory

### Stage 1: Watcher

A lightweight daemon (Python `watchdog` or Rust `notify` crate) monitors `~/.claude/projects/` for new or modified JSONL files. On change:

```python
# Pseudocode for the watcher
from watchdog.observers import Observer
from watchdog.events import FileSystemEventHandler

class SessionHandler(FileSystemEventHandler):
    def on_modified(self, event):
        if event.src_path.endswith('.jsonl'):
            queue.put(('index', event.src_path))
    def on_created(self, event):
        if event.src_path.endswith('.jsonl'):
            queue.put(('index', event.src_path))
```

### Stage 2: Chunker

Reads the JSONL, parses message pairs, and applies the hybrid chunking strategy:

- **Turn-level chunks**: Each user message + assistant response = 1 chunk (metadata: timestamp, session_id, project, message_index)
- **Semantic sub-chunks**: Long assistant responses get split using sentence-boundary detection with semantic similarity threshold (using the `SemanticChunker` from LangChain with percentile_threshold=0.85)[^35][^17]
- **Tool result summaries**: Bash outputs > 500 tokens get LLM-summarized. File reads get path + first/last 5 lines. This prevents tool noise from drowning semantic signal
- **Compaction chunks**: When a compaction block is detected in the JSONL, it gets indexed as a _high-priority chunk_ (these are Claude's own summaries of what mattered)

### Stage 3: Embedder

Batch-embeds chunks using `all-MiniLM-L6-v2`. On an M-series Mac, this processes ~1000 chunks/second locally. Each chunk gets:

- 384-dimensional embedding vector
- Metadata: `{session_id, project_hash, timestamp, chunk_type, message_index, token_count}`

### Stage 4: Indexer

Upserts into SQLite-vec with deduplication (hash-based on content + timestamp to avoid re-indexing unchanged sessions).[^16][^15]

### Stage 5: Memory Extractor (Async)

Feeds new session transcripts to Mem0's `memory.add()` API. Mem0 internally:[^22]

1. Sends the conversation to an LLM with extraction prompts
2. Identifies discrete facts, preferences, decisions
3. Deduplicates against existing memories (updates rather than duplicates)
4. Stores in its hybrid vector + graph store[^24]

### Stage 6: Genome Updater (Async)

Takes the new session + existing `genome.md` and produces an updated synthesis. Runs after Stage 5 completes, so it can also incorporate newly extracted memories.

---

## MemGPT-Inspired Virtual Context Management

The MemGPT paper introduces an OS-inspired approach to context management that's directly applicable here. The key insight: treat the LLM's context window like _main memory_ and external storage like _disk_, with an intelligent page-in/page-out system.[^36]

Applied to the impulse:

- **Main memory** = Claude's active context window (~200k tokens for Sonnet)
- **L1 cache** = `CLAUDE.md` + Genome file (always present, ~2-5k tokens)
- **L2 cache** = Most recent Mem0 memories relevant to current task (~1-3k tokens, injected on session start)
- **Disk** = Full SQLite-vec index + raw JSONL files (unlimited)

The MCP tools act as _page fault handlers_: when Claude's active context lacks information it needs, it calls `search_history` or `query_past_sessions` to "page in" relevant data from disk. Context editing (Anthropic's compaction) acts as the _page eviction policy_, removing stale data to make room.[^2][^30]

The creative addition: a **prefetch heuristic**. When you switch Zellij tabs to a different project, the system automatically pre-warms L2 cache by running `memory.search(project_name)` and injecting the top 5 memories into `CLAUDE.md` before you even type a prompt. This mimics CPU cache prefetching—anticipating what data will be needed based on spatial locality (project context).

---

## CLI Tool Manager Integration

Since the impulse already plans to use `mise` for dev tool versioning, the memory system's dependencies get managed through it:[^37]

```toml
# .mise.toml (project-level)
[tools]
python = "3.12"
node = "22"

[tasks.memory-index]
run = "python .impulse/scripts/index.py"
description = "Re-index conversation history into vector store"

[tasks.memory-status]
run = "python .impulse/scripts/status.py"
description = "Show memory tier health and stats"

[tasks.genome-update]
run = "python .impulse/scripts/genome.py update"
description = "Regenerate project genome from recent sessions"

[tasks.memory-search]
run = "python .impulse/scripts/search.py"
description = "Interactive semantic search over conversation history"
```

The indexer itself has minimal dependencies:

- `sentence-transformers` (for embeddings)
- `sqlite-vec` (for vector storage)
- `mem0ai` (for memory extraction, optional Tier 3)
- `watchdog` (for file system monitoring)

Total footprint: ~150MB (mostly the embedding model). Compare to VS Code's 400–800MB—well within the "lighter weight" constraint.

---

## Implementation Roadmap

### Phase A: Instant Value (30 minutes)

1. Install claude-historian: `claude mcp add claude-historian -- npx claude-historian`
2. Add to `CLAUDE.md`: notification sound command + instruction to search history when referencing past work
3. Start using `/compact focus on X` to guide compaction quality

### Phase B: Vector Index (1–2 days)

1. Write the JSONL → chunk → embed → SQLite-vec pipeline in Python (~200 lines)
2. Wrap as an MCP server with `search_semantic` and `get_similar_sessions` tools
3. Add file watcher for automatic incremental indexing
4. Register alongside claude-historian (they complement each other: lexical + semantic)

### Phase C: Memory Extraction (1 day)

1. Self-host Mem0 via Docker Compose (Postgres pgvector + FastAPI)[^23]
2. Or use `pip install mem0ai` with local Ollama backend for zero-cloud operation[^38]
3. Wire extraction into the indexing pipeline (Stage 5)
4. Expose as `search_memory` and `add_memory` MCP tools

### Phase D: Genome + Prefetch (1 day)

1. Build the genome updater script
2. Wire into Zellij session hooks (on tab switch, on session close)
3. Implement prefetch heuristic for tab-switch context warming

### Phase E: Zellij UI (2–3 days)

1. Build Memory Status Bar as Rust WASM plugin[^29]
2. Build Memory Inspector floating pane
3. Implement Time Machine timeline visualization
4. Wire notification system (audio + visual + system)

### Phase F: Déjà Vu + Interrogation Room (2 days)

1. Build the MCP proxy for background similarity checking
2. Implement the `query_past_sessions` synthesis tool
3. Tune similarity thresholds and retrieval parameters

---

## Configuration Schema

```toml
# .impulse/config.toml

[memory]
enabled = true
vector_db_path = ".impulse/memory.vec.db"
embedding_model = "all-MiniLM-L6-v2"
chunk_overlap_pct = 0.15
max_chunk_tokens = 512
index_tool_outputs = false  # Skip raw tool results to reduce noise

[memory.tiers]
tier1_historian = true
tier2_semantic = true
tier3_mem0 = true
tier4_genome = true

[memory.retrieval]
deja_vu_threshold = 0.85    # Cosine similarity for auto-surfacing
max_injection_tokens = 2000  # Cap on tokens injected per retrieval
prefetch_on_tab_switch = true

[memory.extraction]
backend = "ollama"           # or "openai", "anthropic"
model = "llama3.2:3b"        # Local model for extraction
extract_after_session = true
genome_update_interval = "after_each_session"

[notifications]
sound_enabled = true
sound_command = "afplay /System/Library/Sounds/Funk.aiff"
system_notify = true
visual_bell = true

[notifications.sounds]
task_complete = "Funk.aiff"
compaction = "Submarine.aiff"
error = "Basso.aiff"
deja_vu = "Pop.aiff"
```

---

## Security and Privacy Considerations

Everything runs locally. No conversation data ever leaves your machine unless you explicitly configure a cloud LLM for extraction:

- Embeddings: computed locally via sentence-transformers
- Vector store: SQLite file on local disk
- Mem0: self-hosted or local-only mode[^38][^23]
- Memory extraction: can use Ollama (fully offline) for the LLM calls
- JSONL files: never copied, only read in place

The `.impulse/` directory should be added to `.gitignore` to prevent accidental commits of your conversation index.

---

## Performance Budget

| Component                      | RAM        | Disk       | CPU (idle) | CPU (indexing) |
| ------------------------------ | ---------- | ---------- | ---------- | -------------- |
| Embedding model (loaded)       | ~200MB     | 22MB       | 0%         | 30-50% burst   |
| SQLite-vec database            | ~5MB       | 50-200MB   | 0%         | <5%            |
| File watcher daemon            | ~10MB      | 0          | <1%        | <1%            |
| Mem0 (Docker, self-hosted)     | ~500MB     | ~1GB       | <2%        | 10-20% burst   |
| Mem0 (pip, local mode)         | ~50MB      | ~100MB     | <1%        | 10-20% burst   |
| Zellij plugins (WASM)          | ~5MB each  | <1MB each  | <1%        | <1%            |
| **Total (minimal: Tiers 0-2)** | **~215MB** | **~72MB**  | **<2%**    | **~55% burst** |
| **Total (full: all tiers)**    | **~770MB** | **~1.3GB** | **<5%**    | **~75% burst** |

The minimal configuration (Tiers 0–2 only) adds almost nothing to system overhead. The full configuration with self-hosted Mem0 is heavier but still well under VS Code's baseline resource consumption, and the Docker containers can be started/stopped on demand.

---

## References

1. [Claude Code's hidden conversation history (and how to actually use it)](https://kentgigger.com/posts/claude-code-conversation-history) - Claude Code saves every conversation locally in ~/.claude/history.jsonl. Here's how to find old sess...

2. [Context editing - Claude API Docs](https://platform.claude.com/docs/en/build-with-claude/context-editing) - Automatically manage conversation context as it grows with context editing.

3. [Compaction - Claude API Docs](https://platform.claude.com/docs/en/build-with-claude/compaction) - Server-side context compaction for managing long conversations that approach context window limits.

4. [Compaction clears screen - can't scroll up to see previous context](https://github.com/anthropics/claude-code/issues/18204) - When compaction happens, the screen clears and users can't scroll up to see the previous conversatio...

5. [Logging AI conversations (Claude Code) - ADUG Forums](https://forums.adug.org.au/t/logging-ai-conversations-claude-code/61087) - Claude Code automatically records sessions in your local environment, typically stored in JSONL form...

6. [An MCP server for Claude Code conversation history. : r/ClaudeCode](https://www.reddit.com/r/ClaudeCode/comments/1lzeh9l/vvkmnnclaudehistorian_an_mcp_server_for_claude/) - 22 votes, 27 comments. Hello Reddit, This is claude-historian - an MCP server that gives Claude acce...

7. [GitHub - Vvkmnn/claude-historian: 🤖 An MCP server for Claude Code conversation history](https://github.com/Vvkmnn/claude-historian) - 🤖 An MCP server for Claude Code conversation history - Vvkmnn/claude-historian

8. [Claude Code History | Awesome MCP Servers](https://mcpservers.org/servers/yudppp/claude-code-history-mcp) - Claude Code History MCP Server. An MCP server for retrieving and analyzing Claude Code conversation ...

9. [Claude Code History MCP Server - LobeHub](https://lobehub.com/mcp/yudppp-claude-code-history-mcp) - An MCP server for retrieving and analyzing Claude Code conversation history. This server reads Claud...

10. [Historian: AI Code Conversation History Search for Claude](https://mcpmarket.com/server/historian)

11. [GitHub - thejud/claude-history: A command-line utility to extract and format conversation history from Claude Code session files](https://github.com/thejud/claude-history) - A command-line utility to extract and format conversation history from Claude Code session files - t...

12. [Access Full Claude Code Conversation History Context for External ...](https://github.com/BeehiveInnovations/zen-mcp-server/issues/155) - Problem Currently, external LLMs (Gemini, O3, etc.) only receive tool-specific context through Zen's...

13. [Automatic context compaction - Claude Developer Platform](https://platform.claude.com/cookbook/tool-use-automatic-context-compaction) - Manage context limits in long-running agentic workflows by automatically compressing conversation hi...

14. [使用 SQLiteVec 將 SQLite 作為向量儲存](https://langchain-python.dev.org.tw/docs/integrations/vectorstores/sqlitevec/)

15. [SQLiteVec integration - Docs by LangChain](https://docs.langchain.com/oss/python/integrations/vectorstores/sqlitevec) - Integrate with the SQLiteVec vector store using LangChain Python.

16. [Local Vector Search with llama.cpp Embeddings and sqlite_vec](https://www.timestretch.com/2025/05/26/local_vector_search_with_llama_cpp_embeddings_and_sqlite_vec.html) - The general pipeline will take a query string, embed it, then match it against every vector in the d...

17. [Best Chunking Strategies for RAG in 2025 - Firecrawl](https://www.firecrawl.dev/blog/best-chunking-strategies-rag-2025) - Testing different chunking strategies. You can test how chunking strategy affects retrieval by chang...

18. [Chunking Strategies for LLM Applications](https://www.pinecone.io/learn/chunking-strategies/) - In the context of building LLM-related applications, chunking is the process of breaking down large ...

19. [Smarter Retrieval for RAG: Late Chunking with Jina Embeddings v2 ...](https://milvus.io/blog/smarter-retrieval-for-rag-late-chunking-with-jina-embeddings-v2-and-milvus.md) - Boost RAG accuracy using Late Chunking and Milvus for efficient, context‑aware document embeddings a...

20. [Build an Agentic RAG Chatbot With Memory Using LangGraph and ...](https://mem0.ai/blog/agentic-rag-chatbot-with-memory) - Learn how to build a personalized RAG chatbot with AI memory so it remembers users across sessions u...

21. [mem0ai/mem0: Universal memory layer for AI Agents; ...](https://github.com/mem0ai/mem0) - Universal memory layer for AI Agents; Announcing OpenMemory MCP - local and secure memory management...

22. [GitHub - mem0ai/mem0: Memory for AI Agents; SOTA in AI Agent Memory; Announcing OpenMemory MCP - local and secure memory management.](https://github.com/mem0ai/mem0/) - Memory for AI Agents; SOTA in AI Agent Memory; Announcing OpenMemory MCP - local and secure memory m...

23. [Self-Host Mem0: Open-Source Persistent AI Memory Layer](https://www.self-host.app/services/mem0) - Self-Host Mem0: Open-Source Persistent AI Memory Layer

24. [Mem0: An open-source memory layer for LLM applications ...](https://www.azalio.io/mem0-an-open-source-memory-layer-for-llm-applications-and-ai-agents/) - The stateless nature of large language models has fundamentally constrained the landscape of AI appl...

25. [[PDF] Mem0: Building Production-Ready AI Agents with - arXiv](https://arxiv.org/pdf/2504.19413.pdf)

26. [A-MEM: Agentic Memory for LLM Agents](https://arxiv.org/pdf/2502.12110.pdf) - ... and context-aware memory management. Empirical experiments on six
    foundation models show superio...

27. [npm i -g onecontext-ai And open with](https://x.com/JundeMorsenWu/status/2020161412593774922)

28. [Stacked Resize, Pinned Floating Panes, New Theme Spec - Zellij](https://zellij.dev/news/stacked-resize-pinned-panes/) - This version of Zellij introduces an innovative new way of managing multiple panes. When resizing pa...

29. [Developing a Zellij plugin using Rust](https://zellij.dev/tutorials/developing-a-rust-plugin/) - This tutorial will walk you through developing a Zellij plugin with rust, using specialized Zellij t...

30. [Managing context on the Claude Developer Platform](https://www.claude.com/blog/context-management) - New context editing and memory tools enable Claude agents to handle long-running tasks without hitti...

31. [How Claude Code Got Better by Protecting More Context - Hyperdev](https://hyperdev.matsuoka.com/p/how-claude-code-got-better-by-protecting) - Combining the memory tool with context editing improved performance by 39% over baseline, with conte...

32. [Alert on operation completion in windows cmd - Stack Overflow](https://stackoverflow.com/questions/30775261/alert-on-operation-completion-in-windows-cmd) - I want to know whether there is way by which I can make changes to cmd such that it gives me a signa...

33. [Does anybody figure out how to play notification/sound after IDE terminal command is complete?](https://www.reddit.com/r/IntelliJIDEA/comments/xdyvfd/does_anybody_figure_out_how_to_play/)

34. [Simple way to get notified when claude code finishes : r/ClaudeAI](https://www.reddit.com/r/ClaudeAI/comments/1lfvz30/simple_way_to_get_notified_when_claude_code/) - The solution: Claude Code Audio Hooks gives you distinct audio cues for 9 different events — task co...

35. [Smarter RAG Retrieval with Max–Min Semantic Chunking - Milvus](https://milvus.io/blog/embedding-first-chunking-second-smarter-rag-retrieval-with-max-min-semantic-chunking.md) - Learn how Max–Min Semantic Chunking boosts RAG accuracy using an embedding-first approach that creat...

36. [MemGPT: Towards LLMs as Operating Systems](https://arxiv.org/pdf/2310.08560.pdf) - ...
    (Memory-GPT), a system that intelligently manages different memory tiers in
    order to effectively...

37. [Dev Tools | mise-en-place](https://mise.jdx.dev/dev-tools/) - mise is a tool that manages installations of programming language runtimes and other tools for local...

38. [Mem0 Open Source Overview](https://docs.mem0.ai/open-source/overview) - Self-host Mem0 with full control over your infrastructure and data
