---
status: active
phase: all
audience: builder
tags: [research, tools, analysis]
last_updated: 2026-02-20
---

# Tool Stack Analysis: Impulse's Dependency Ecosystem

> **Version:** 1.0 | **Status:** Research Complete | **Updated:** 2026-02-20
> **Purpose:** Deep analysis of every tool in the Impulse stack — what it does, how it fits, integration risks, and study priorities

---

## Stack Overview

```
Layer 4: Memory Extraction     [mem0, sentence-transformers]      Phase 2+
Layer 3: Semantic Index         [sqlite-vec, watchdog]             Phase 2+
Layer 2: History Search         [claude-historian-mcp, claude-history]  Phase 1.5
Layer 1: Agent + Context        [OpenCode, OneContext]             Phase 1
Layer 0: Terminal Foundation    [Ghostty, Zellij, mise]            Phase 0
```

---

## Layer 0: Terminal Foundation

### Ghostty (ghostty-org/ghostty)

**What:** GPU-accelerated terminal emulator, Metal on macOS.

**Why for Impulse:** Fast rendering for multi-pane Zellij layouts. When 4-12 agents produce output simultaneously, GPU acceleration prevents the terminal from becoming a bottleneck.

**Key technical properties:**
- Zig implementation — ~70k lines, single binary
- Metal backend on macOS, Vulkan/OpenGL on Linux
- Native font shaping (no external harfbuzz dependency in some configs)
- Config file: `~/.config/ghostty/config` (simple key=value)

**Integration risk:** None — Ghostty is a passive terminal. Impulse doesn't depend on it programmatically. Any terminal works; Ghostty is recommended for performance.

**Study priority:** LOW. Just install and configure font/theme.

---

### Zellij (zellij-org/zellij)

**What:** Terminal multiplexer with WASM plugin system.

**Why for Impulse:** The workspace manager. Creates the visual layout where agents run in separate panes. Status bar and dashboard plugins are Rust compiled to WASM.

**Key technical properties:**
- Rust implementation
- Plugin system: Rust -> wasm32-wasip1 (WASI Preview 1)
- Layout DSL: KDL format (`.kdl` files)
- Pinned floating panes (0.42.0+) — useful for overlays
- Session resurrection across terminal restarts
- IPC: plugins communicate through Zellij events, not directly

**Critical for Impulse:**
- `layout.kdl` defines the multi-agent workspace
- Status bar plugin shows agent count, session timer, GENOME.md health
- Dashboard plugin (Phase 3) provides the Time Machine UI

**Integration risk:** MEDIUM.
- WASM plugin API is still evolving (breaking changes between versions)
- Plugin compilation requires Rust toolchain + `wasm32-wasip1` target
- Plugins cannot access the filesystem directly (WASI sandbox)
- Communication between panes requires Zellij event bus

**Study priority:** MEDIUM for Phase 0, HIGH for Phase 3.
- Phase 0: Just use KDL layouts, no custom plugins
- Phase 3: Build Rust WASM plugins for status bar and dashboard

**Key files to study:**
- `rust-plugin-example/` — minimal Zellij plugin template
- `zjstatus/` — production status bar plugin (interval polling pattern)

---

### mise (jdx/mise)

**What:** Polyglot dev tool version manager.

**Why for Impulse:** Manages all tool versions in a single `.mise.toml`. Ensures consistent Bun, Rust, Python versions across machines and team members.

**Key technical properties:**
- Rust implementation, single binary
- `.mise.toml` at project root pins tool versions
- Supports: node, bun, python, rust, go, java, and custom plugins
- Hooks: can run commands on `cd` into project directory

**Integration risk:** LOW. Declarative config, no runtime dependency.

**Study priority:** LOW. Configure once in `.mise.toml`.

**Recommended `.mise.toml`:**
```toml
[tools]
bun = "1.1"
node = "20"          # Some tools still need Node
python = "3.12"      # For Phase 2 memory pipeline
rust = "1.85"        # For Phase 3 Zellij plugins

[env]
IMPULSE_ENV = "development"
```

---

## Layer 1: Agent + Context

### OpenCode (sst/opencode)

**What:** Terminal-native AI coding agent with plugin system.

**Why for Impulse:** The PRIMARY integration target. OpenCode's plugin SDK provides the 15+ hooks that Impulse subscribes to. Without OpenCode, Impulse has no way to observe or inject into agent sessions.

**Key technical properties:**
- TypeScript + Bun runtime
- Client/server architecture (server runs as daemon)
- 2 built-in agents: `build` (execute) and `plan` (reason)
- Plugin SDK exposes typed hook interface
- REST API for external tooling
- Config: `.opencode/config.json` in project root

**Critical OpenCode Plugin Hooks (validated from source):**

| Hook | Trigger | Impulse Use |
|------|---------|-------------|
| `session.start` | Agent session begins | Load GENOME.md + LIVE_STATE + HISTORY |
| `tool.execute.after` | Tool completes | Update LIVE_STATE.json |
| `session.end` | Session closes | LLM extraction -> GENOME.md, HISTORY |
| `experimental.session.compacting` | Context compresses | Inject GENOME.md for survival |
| `message.updated` | Any message sent | (Phase 2) Trigger embedding indexing |
| `experimental.chat.system.transform` | System prompt | (Phase 2) Dynamic context injection |

**Integration risk:** HIGH.
- Plugin SDK is pre-1.0 — hook signatures may change
- `experimental.*` hooks explicitly flagged as unstable
- `tool.execute.before` can modify args but CANNOT block
- Server process must be running for hooks to fire
- No official documentation for plugin SDK (read source)

**Study priority:** HIGHEST.
- Read: `packages/plugin/src/index.ts` (hook interface definition, L148-234)
- Read: `packages/opencode/src/plugin/index.ts` (how plugins are loaded)

---

### OneContext (TheAgentContextLab/OneContext)

**What:** Agent self-managed context layer. Records trajectory, loads context across sessions and devices.

**Why for Impulse:** Potential complementary layer for cross-device context sync. Could replace or augment HISTORY_INDEX.md with richer trajectory data.

**Key technical properties:**
- Node wrapper over Python CLI
- v0.8.3 (Feb 2026) added import of past Codex/Claude sessions
- Stores context locally with optional cloud sync
- Short alias: `oc`

**Integration risk:** MEDIUM.
- Node + Python dual dependency (heavier than Bun-only approach)
- Overlaps with Impulse's own HISTORY_INDEX.md
- v0.x — API unstable

**Study priority:** LOW for MVP, MEDIUM for Phase 2.
- Evaluate whether it replaces HISTORY_INDEX.md or complements it
- Watch for 1.0 stability before deep integration

---

## Layer 2: History Search

### claude-historian-mcp (Vvkmnn/claude-historian-mcp)

**What:** MCP server providing lexical search over Claude Code JSONL history. Zero persistent storage — streams raw JSONL directly.

**Why for Impulse:** The single best reference implementation for JSONL parsing. This is what we study before building our own.

**Key technical properties:**
- TypeScript + Node 20+
- Zero-install via `npx claude-historian-mcp`
- 6 MCP tools (search, list sessions, get turn, etc.)
- TF-IDF scoring with Naive Bayes query classification
- Edit-distance fuzzy matching
- Exponential time decay (recent results ranked higher)
- LRU cache for repeated queries

**Source layout (study guide):**

| File | Size | What to Learn |
|------|------|---------------|
| `src/universal-engine.ts` | 63KB | Core search engine: JSONL parsing, scoring, ranking |
| `src/search.ts` | 70KB | Search orchestration: query planning, result assembly |
| `src/parser.ts` | 25KB | JSONL parser: line-by-line streaming, noise filtering |
| `src/formatter.ts` | 36KB | Output formatting: markdown, truncation, highlights |
| `src/scoring-constants.ts` | Small | Tunable weights for TF-IDF, decay, boost factors |
| `src/index.ts` | 39KB | MCP server entry: tool definitions, request routing |

**Key algorithms to extract:**
1. **Noise filtering** — how they strip tool metadata (75% of JSONL is noise)
2. **TF-IDF scoring** — term frequency / inverse document frequency for code conversations
3. **Time decay** — exponential decay function for ranking recent results
4. **Query classification** — Naive Bayes to detect keyword vs semantic vs temporal queries

**Integration risk:** LOW.
- Read-only MCP server, zero side effects
- Can install alongside Impulse without conflicts
- Valuable as-is, even without Impulse running

**Study priority:** HIGH.
- `universal-engine.ts` is the blueprint for our own Phase 2 indexing
- Scoring constants directly inform our retrieval tuning

---

### claude-history (thejud/claude-history)

**What:** Simple Python CLI for extracting Claude Code JSONL into readable Markdown. No dependencies beyond Python stdlib.

**Why for Impulse:** Data extraction stage. Useful for feeding JSONL into our own pipeline, and for debugging what the JSONL actually contains.

**Key technical properties:**
- Python 3.6+, zero external dependencies
- Outputs chronological Markdown
- Two modes: prompts only, or full turns (with `--agent` flag)
- Handles conversation branching

**Integration risk:** NONE. Standalone CLI tool, no runtime dependency.

**Study priority:** MEDIUM.
- Quick way to inspect JSONL structure
- Validates our parser's assumptions about JSONL format
- Good for generating test fixtures

---

## Layer 3: Semantic Index (Phase 2+)

### sqlite-vec (asg017/sqlite-vec)

**What:** SQLite extension for vector similarity search. Single-file, ~3MB C extension.

**Why for Impulse:** Phase 2 vector store. When FTS5 keyword search isn't enough (semantic queries like "what was the auth approach?"), sqlite-vec provides cosine similarity over embedded conversation turns.

**Key technical properties:**
- C implementation, MIT licensed
- Loads as SQLite extension (`SELECT load_extension('vec0')`)
- 384-dim float vectors (matches all-MiniLM-L6-v2)
- Virtual table API — queries via SQL
- No UPSERT on virtual tables (DELETE + INSERT required)
- No incremental index updates (full scan for small datasets)
- Sub-100ms KNN queries for hundreds to low thousands of vectors

**Benchmark results (validated):**

| Metric | sqlite-vec | Faiss | Chroma |
|--------|-----------|-------|--------|
| Write latency | 788ms avg | 47,640ms | Moderate |
| Read latency | Sub-100ms | Fastest at scale | Moderate |
| Operational complexity | Zero config | Library | Client-server |
| Storage | Single file | Varies | Heavy |

**Integration risk:** MEDIUM.
- C extension requires platform-specific binaries
- No UPSERT means DELETE+INSERT for updates (more complex writes)
- Not suitable for millions of vectors (but we'll never hit that)
- Available via pip (`pip install sqlite-vec`) and npm

**Study priority:** LOW for MVP (not needed), HIGH for Phase 2.

---

### sentence-transformers (UKPLab/sentence-transformers)

**What:** Python library for generating text embeddings using transformer models.

**Why for Impulse:** Phase 2 embedding generation. Converts conversation turns into 384-dim vectors for sqlite-vec similarity search.

**Key technical properties:**
- Python, Apache 2.0
- Model: `all-MiniLM-L6-v2` (22MB download, 384 dims, fast)
- Alternative: `nomic-embed-text` (768 dims, better quality)
- Runs locally — no API calls, no cost, no data leaves machine

**Integration risk:** LOW (Python-only, Phase 2).
- First model download is 22MB (one-time)
- Requires Python 3.8+ and PyTorch
- Memory: ~200MB during inference (model loaded)

**Study priority:** LOW for MVP, MEDIUM for Phase 2.

---

### watchdog (Python)

**What:** Python filesystem monitoring library.

**Why for Impulse:** Phase 3 background indexing. Watches `~/.claude/projects/` for new JSONL writes, triggering the indexing pipeline automatically.

**Study priority:** LOW. Phase 3 only.

---

## Layer 4: Memory Extraction (Phase 2+)

### mem0 (mem0ai/mem0)

**What:** Production-ready memory extraction and management. Uses LLM as intelligent memory manager.

**Why for Impulse:** Phase 2+ knowledge extraction. Instead of raw text search, mem0 extracts semantic facts ("James prefers TypeScript for backends") and manages them with ADD/UPDATE/DELETE operations.

**Key technical properties:**
- Python/JS, Apache 2.0, 42k+ stars
- YC S24 company
- LOCOMO benchmark: F1=28.64, +26% accuracy over OpenAI Memory
- 91% faster, 90% fewer tokens than full-context retrieval
- Each memory op triggers 2 LLM calls (fact extraction + update decision)
- Cost: ~$0.02-0.05 per session at GPT-4o-mini pricing
- Self-hosted: Docker Compose with Postgres pgvector + Neo4j
- Minimal: just Qdrant/Chroma locally
- OpenMemory MCP server included (exposing mem0 to Claude Code)

**Architecture (validated):**
```
Conversation Turn
  -> LLM: "Extract meaningful facts"
  -> LLM: "Memory management decision" (ADD/UPDATE/DELETE/NONE)
  -> Vector DB: Store/update embedding
  -> Graph DB: Update entity relationships (optional)
  -> SQLite: Audit trail
```

**The lean alternative (MVP):**
Instead of full mem0 pipeline, use a single session-end LLM call:
- Cost: 1 LLM call (~$0.01)
- Quality: ~70-80% of mem0 accuracy
- This is exactly what Impulse's session-end hook does

**Integration risk:** MEDIUM.
- Full stack requires Postgres + Neo4j (heavy)
- Minimal mode still needs a vector DB (Qdrant or Chroma)
- 2 LLM calls per memory operation adds latency
- Worth it only at 100+ sessions when manual GENOME.md breaks down

**Study priority:** LOW for MVP, HIGH for Phase 2+.
- Read: `mem0/memory/main.py` — core memory management logic
- Read: MCP server — how mem0 exposes tools to agents

---

## Integration Map

```
Phase 0 (Now):
  Install: Ghostty, Zellij, mise
  Study: claude-historian-mcp (universal-engine.ts), OpenCode (plugin SDK)

Phase 1 (MVP):
  Build: impulse-plugin/ (4 hooks, 3 files)
  Integrate: OpenCode plugin SDK
  Use: Zellij KDL layouts

Phase 1.5:
  Add: claude-historian-mcp (MCP search alongside Impulse)
  Add: claude-history (JSONL inspection tooling)

Phase 2:
  Add: sqlite-vec + sentence-transformers (vector search)
  Add: mem0 (when GENOME.md exceeds 500 lines)
  Add: OneContext (evaluate for cross-device sync)

Phase 3:
  Add: Zellij WASM plugins (Rust status bar + dashboard)
  Add: watchdog (background JSONL indexing)
```

---

## Risk Matrix

| Tool | Maturity | API Stability | Impulse Dependency | Risk |
|------|----------|--------------|-------------------|------|
| Ghostty | Stable | N/A (passive) | None | LOW |
| Zellij | Stable | KDL stable, WASM evolving | Layout only (MVP) | LOW |
| mise | Stable | Stable | Dev tooling only | LOW |
| OpenCode | Pre-1.0 | Experimental hooks | CRITICAL | HIGH |
| OneContext | v0.x | Unstable | Optional | MEDIUM |
| claude-historian | v1.x | MCP standard | Reference only | LOW |
| claude-history | Stable | N/A (CLI) | Utility only | LOW |
| sqlite-vec | v0.x | Stable extension API | Phase 2 | MEDIUM |
| sentence-transformers | Stable | Stable | Phase 2 | LOW |
| mem0 | Active dev | Python API stable | Phase 2+ | MEDIUM |

**Critical dependency: OpenCode plugin SDK.** If OpenCode's hooks change, all 4 Impulse hooks must be updated. Mitigation: abstract hook registration behind our own interface (`impulse-plugin/src/index.ts:register()`).

---

_Created: 2026-02-20 | Status: Complete v1.0_
