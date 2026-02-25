---
status: active
phase: all
audience: builder
tags: [research, llm, failure-modes]
last_updated: 2026-02-20
---

# LLM Coding Problem Map: The Pain Landscape

> **Version:** 1.0 | **Status:** Research Complete | **Updated:** 2026-02-20
> **Purpose:** Document every pain point in current LLM coding workflows, rate solutions 1-5, and identify where Impulse fits
> **Inputs:** TERMINAL-LAYER-ANALYSIS.md, AGENT-HARNESS-ANALYSIS.md, SEARCH-LAYER-ANALYSIS.md, MEMORY-EXTRACTION-ANALYSIS.md

---

## Executive Summary

LLM coding agents have a fundamental architectural problem: **they are stateless between sessions**. Every session starts from zero context, re-discovers solved problems, re-debates settled decisions, and has no awareness of what other agents are doing. The tools attempting to fix this are fragmented across multiple layers, each solving one narrow slice while ignoring the others.

Our research across 11+ tools reveals eight distinct pain points. No single tool addresses more than three of them. The most effective approaches share a surprising pattern: **file-based persistence beats database-backed solutions** for the first 100+ sessions of use. The most overengineered approaches (vector databases, graph memory, WASM plugins) solve problems that don't exist until much later — if ever.

Impulse's unique position: it can address 6 of 8 problems with a file-first architecture, then add complexity only when usage data proves it's needed.

---

## The Eight Problems

### Problem 1: Memory Loss Between Sessions

**Description:** LLM coding agents forget everything when a session ends. Architectural decisions, debugging conclusions, coding preferences, project constraints — all lost. The next session starts from a blank slate, relying solely on codebase files and any static documentation.

**Severity: 5/5** — This is the single biggest productivity drain in LLM-assisted development.

**Real-world impact:**
- Developer spends 10 minutes explaining JWT token strategy that was decided 3 sessions ago
- Agent proposes SQLite when the team already chose PostgreSQL (debated and resolved last week)
- Project constraints (must support Python 3.8+, no external services in CI) must be re-stated every session

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **mem0** | LLM-as-memory-manager: extracts facts, stores in vector DB, retrieves by similarity | 4/5 | Best quality (+26% accuracy on LOCOMO), but requires infrastructure (Qdrant/Chroma + graph DB). Overkill for < 100 sessions. |
| **CLAUDE.md files** | Static markdown in project root, manually maintained | 3/5 | Works surprisingly well — Claude reads it every session. But it's manual, gets stale, and has no cross-session extraction. |
| **Cursor Memory** | User-curated memory entries + @memory references + MCP integration (2025+) | 3/5 | Tied to Cursor IDE. User must manually trigger memory saves. MCP integration is new and untested at scale. |
| **Windsurf Cascade** | Auto-generate toggle creates persistent memories from session context | 3/5 | Automated but quality varies. Categorized storage (preferences, decisions, patterns) is a good structural idea. |
| **Cline Memory Bank** | Markdown files (.clinerules, memory-bank/) with structured context handoff | 3/5 | File-based, portable, well-structured. Requires explicit `/memory` commands. Good model for Impulse. |
| **OneContext** | Trajectory recording + cross-device sync + session replay | 2/5 | Solves replay, not extraction. Doesn't distill sessions into knowledge — just stores them verbatim. |
| **Impulse (proposed)** | Session-end LLM extraction → GENOME.md + session-start injection | 4/5 (projected) | Automated extraction with zero infrastructure. Competitive with mem0 on prompt quality, weaker on contradiction resolution. |

**Key insight from research:** All four competitors (Cursor, Windsurf, Cline, aider) use **file-based persistence**, not vector databases, for cross-session context. This validates Impulse's "three files" architecture. The real differentiation is in extraction quality and automation, not storage backend.

---

### Problem 2: Context Window Limitations

**Description:** LLM context windows are finite (~128K-200K tokens). Long coding sessions hit the limit, triggering compaction that destroys earlier context. Critical decisions from early in the session are summarized or dropped entirely.

**Severity: 4/5** — Compaction is silent and destructive. Developers don't know what was lost.

**Real-world impact:**
- Agent forgets the database schema discussed in turn 3, generates incompatible migrations in turn 50
- Early architectural decisions are compacted away, agent re-proposes rejected approaches
- Compaction summaries are lossy — nuance and caveats are stripped

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **Claude Code PreCompact hook** | Shell command runs before compaction, can inject "must survive" content | 4/5 | Direct, precise, zero-latency injection. Claude receives the content as system context that survives compaction. |
| **OpenCode compacting hook** | `experimental.session.compacting` — can push to `output.context[]` | 3/5 | Works but experimental. Less control over what survives. No way to know what's being compacted. |
| **RAG retrieval** | Post-compaction, use RAG to re-retrieve compacted content on demand | 3/5 | Reactive, not proactive. Agent must know what to search for. Adds latency. Requires indexing infrastructure. |
| **CLAUDE.md** | Critical decisions in project root survive compaction because they're re-read on tool calls | 2/5 | Only works for static content. Doesn't help with session-specific context. |
| **Impulse (proposed)** | PreCompact hook injects GENOME.md top-50 lines + session-end extraction preserves decisions post-compaction | 4/5 (projected) | Combines proactive injection (pre-compaction) with retroactive preservation (post-session extraction). Belt and suspenders. |

**Key insight from research:** Compaction does NOT modify on-disk JSONL files. The original transcript is always recoverable. This means Impulse's session-end extraction can work on the full transcript even after compaction destroyed the in-session context.

---

### Problem 3: Multi-Agent Conflicts

**Description:** When multiple AI agents work on the same codebase simultaneously, they have no awareness of each other. Two agents may edit the same file, propose conflicting changes, or duplicate work.

**Severity: 3/5** — Becoming more common as developers adopt multi-agent workflows (Claude Code + OpenCode, or multiple Claude Code sessions).

**Real-world impact:**
- Agent A refactors auth.ts while Agent B adds a new auth method to the same file → merge conflict
- Both agents independently implement the same utility function
- Agent A decides on JWT, Agent B decides on session tokens → conflicting implementations

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **Git** | Merge conflicts force resolution after the fact | 2/5 | Reactive, not preventive. Conflicts are discovered too late. No real-time awareness. |
| **Zellij pane awareness** | Plugins can read pane metadata (titles, commands, exit codes) but NOT pane content | 2/5 | Knows which programs are running, but not what they're doing. Metadata is too coarse for coordination. |
| **File locking (OS-level)** | `flock` or similar — prevent concurrent writes | 1/5 | Too low-level. Prevents writes but doesn't enable coordination or intent sharing. |
| **LIVE_STATE.json (Impulse proposed)** | Agents self-report: what files they're editing, what they intend to do | 4/5 (projected) | Simple, zero false positives (file-path matching), enables intent-based coordination. Requires agent cooperation (reading state before acting). |

**Key insight from research:** Vector-similarity approaches to multi-agent coordination (like the original SWARM spec) produce false positives — matching "User Profile" text across unrelated SQL and React files. File-path matching is precise, zero false positives, and sufficient for Impulse's use case. Add vector similarity only if conceptual overlap detection proves necessary.

---

### Problem 4: No Searchable History

**Description:** Past coding sessions are stored as raw JSONL files but are effectively unsearchable. "What did we decide about the auth approach last week?" requires manually opening and reading through potentially megabytes of conversation logs.

**Severity: 4/5** — The history EXISTS but is inaccessible. This is a retrieval problem, not a storage problem.

**Real-world impact:**
- Developer spends 20 minutes re-figuring out a solution they already found 2 weeks ago
- Can't answer "what files did we modify when implementing feature X?"
- Can't trace the reasoning behind an architectural decision

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **claude-historian-mcp** | MCP server with multi-stage keyword scoring over raw JSONL. 10 tools, 4.7/5 quality score. | 4/5 | Best-in-class for JSONL search. Custom scoring achieves near-TF-IDF quality without the complexity. Zero persistent storage — streams directly from JSONL. |
| **claude-history** | Python CLI converting JSONL → readable Markdown | 2/5 | Makes history readable but not searchable. Good for manual inspection, not programmatic queries. |
| **FTS5 (SQLite)** | Full-text search over parsed session summaries | 4/5 | Handles 80% of queries (high keyword density in code conversations). Instant, zero infrastructure. |
| **sqlite-vec** | Vector similarity search over embedded session summaries | 3/5 | Catches synonyms FTS5 misses ("auth" vs "security middleware"). Adds < 100ms. Requires embedding model. |
| **HISTORY_INDEX.md (Impulse proposed)** | Chronological session summaries in searchable Markdown | 3/5 (Phase 1), 4/5 (Phase 2 with FTS5) | Good enough for < 100 sessions (grep works). Phase 2 adds FTS5 + sqlite-vec hybrid for scale. |

**Key insight from research:** claude-historian-mcp does NOT use TF-IDF, Naive Bayes, or edit-distance — contrary to all prior documentation. It uses a pragmatic, handcrafted multi-stage keyword scoring system that achieves 4.7/5 quality. The lesson: simple, well-tuned keyword matching beats theoretical algorithms for code conversations.

---

### Problem 5: Decision Amnesia (Re-Debating Solved Problems)

**Description:** The most frustrating problem: the agent re-proposes solutions that were already considered and rejected. "We already tried Redis for caching and it didn't work because of X" — but the agent doesn't know this, so it suggests Redis again.

**Severity: 5/5** — Emotionally draining. Makes developers feel like the AI isn't learning. Destroys trust.

**Real-world impact:**
- Same architectural debate happens 3 sessions in a row
- Developer must re-explain rejection rationale each time
- Agent wastes context window tokens on proposals that will be rejected
- Developers learn to front-load constraints ("don't suggest Redis, don't suggest X, don't suggest Y")

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **mem0** | Stores facts with contradiction resolution (ADD/UPDATE/DELETE). Detects when new info contradicts stored facts. | 4/5 | Best at contradiction handling. But requires infrastructure and 20+ LLM calls per session for full pipeline. |
| **CLAUDE.md** | Manual documentation of decisions and constraints | 3/5 | Works when maintained. But humans forget to update it, and it can't capture nuanced "we tried X because of Y and it failed because of Z" reasoning. |
| **Cursor Memory** | User-curated, user-triggered memory saves | 2/5 | Depends entirely on user discipline. Most users forget to save decisions. |
| **GENOME.md (Impulse proposed)** | Automated session-end extraction of decisions. Injected at next session start. Deduplication prevents repeats. | 4/5 (projected) | Automated capture is the key differentiator. Prompt improvements (few-shot examples, existing-knowledge feeding) close the gap with mem0. Weakness: 40-char fingerprint dedup needs upgrading to semantic dedup. |

**Key insight from research:** The "pain to rediscover" weighting from claude-historian-mcp is directly applicable: decisions are 2.5x more painful to reconstruct than feature implementations. Impulse's extraction prompt should prioritize decisions over routine changes, because those are exactly the things that cause decision amnesia.

---

### Problem 6: Tool Fragmentation (10 CLIs, No Unified Workspace)

**Description:** Modern AI-assisted development involves 5-10 CLI tools (terminal, multiplexer, AI agent, editor, version manager, search tools, memory tools) with no unified workspace. Each tool runs independently, unaware of the others.

**Severity: 3/5** — More of a productivity/UX issue than a correctness issue.

**Real-world impact:**
- Developer juggles tmux/Zellij + Claude Code + Helix/Neovim + git + various MCP servers
- Context switching between tools loses flow state
- No single view of "what's happening across all my tools"

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **Zellij** | Terminal multiplexer with KDL layouts, session resurrection, WASM plugins | 4/5 | Best unified workspace for terminal-native developers. KDL layouts define multi-pane workspaces declaratively. Session resurrection survives restarts. |
| **mise** | Polyglot tool version manager with cd hooks | 3/5 | Manages versions but not workspace layout. The `enter` hook is valuable for project-level initialization. |
| **VS Code** | IDE with integrated terminal, extensions, AI sidebars | 4/5 | Most unified experience, but locked to VS Code. Not terminal-native. |
| **Cursor** | VS Code fork + AI-native features | 4/5 | Best unified IDE experience. But locked to one editor and one AI provider. |
| **Impulse layout.kdl (proposed)** | Zellij layout template + mise auto-init | 3/5 (projected) | Defines the workspace structure. Combined with mise enter hooks, provides zero-friction project setup. |

**Key insight from research:** Zellij's `FileSystemUpdate` events enable reactive plugin updates without polling. When combined with `zellij pipe` for CLI-to-plugin communication, the terminal multiplexer becomes a lightweight message bus connecting all tools. This is more powerful than previously assessed.

---

### Problem 7: No Visibility Into Agent State

**Description:** When an AI agent is working, there's no real-time visibility into what it's doing, what files it's touching, or what its current intent is. The developer must wait for output or read tool call logs.

**Severity: 2/5** — Annoying but not blocking. Most agents stream their thinking.

**Real-world impact:**
- Can't tell if the agent is stuck, thinking, or actively coding
- Can't see which files are being modified without watching git status
- In multi-agent setups, can't see the overall coordination picture

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **Zellij WASM plugins** | Status bar showing agent metrics. Can read /host files reactively. | 3/5 | Can display LIVE_STATE.json data in real-time via FileSystemUpdate events. But requires Rust/WASM plugin development. |
| **LIVE_STATE.json (Impulse proposed)** | JSON file with per-agent status, file locks, intents. Updated by PostToolUse hook. | 4/5 (projected) | Simple, readable (`tail -f`), no infrastructure. Zellij plugins can read it reactively in Phase 3. |
| **Claude Code streaming** | Agent streams thoughts and tool calls to terminal | 2/5 | Shows what's happening NOW but has no persistent state. Can't compare across agents. |

**Key insight from research:** The `tail -f .impulse/LIVE_STATE.json` approach is surprisingly effective for Phase 1 — it provides real-time agent state visibility with zero plugin code. Upgrade to Zellij WASM plugin only when interactive controls are needed.

---

### Problem 8: Prompt Engineering Is Manual

**Description:** Optimizing the system prompt, context injection, and agent behavior requires manual prompt engineering. Each new project requires setting up CLAUDE.md, configuring hooks, and tuning extraction prompts.

**Severity: 2/5** — One-time setup cost per project, but compounds across many projects.

**Real-world impact:**
- New projects start with suboptimal AI behavior until CLAUDE.md is tuned
- Best practices from one project don't automatically transfer to another
- No way to A/B test different prompt strategies

**Tools that attempt to solve it:**

| Tool | Approach | Effectiveness (1-5) | Justification |
|------|----------|---------------------|---------------|
| **Claude Code hooks** | Shell-command hooks for SessionStart, PostToolUse, etc. Configurable per-project or globally. | 3/5 | Programmatic prompt injection. But requires manual configuration. |
| **OpenCode system.transform** | `experimental.chat.system.transform` hook mutates system prompt every LLM call. | 3/5 | Powerful but experimental. Less control than Claude Code hooks. |
| **GENOME.md (Impulse proposed)** | Automatically injected at session start. Grows over time. Self-tuning via extraction. | 4/5 (projected) | The key insight: GENOME.md is a self-evolving system prompt. Each session adds new decisions, effectively tuning the prompt automatically. No manual engineering needed after initial setup. |

**Key insight from research:** GENOME.md as "self-evolving system prompt" is the most underappreciated aspect of Impulse's design. Unlike static CLAUDE.md files, GENOME.md grows and adapts through automated extraction. This is analogous to how human teams build institutional knowledge — not through documentation mandates, but through accumulated decisions.

---

## Synthesis: Where Impulse Fits

### Problem Coverage Matrix

| Problem | Severity | Impulse Coverage | Phase |
|---------|----------|-----------------|-------|
| 1. Memory loss between sessions | 5/5 | **Direct solve** — GENOME.md + automated extraction | Phase 1 |
| 2. Context window limitations | 4/5 | **Direct solve** — PreCompact hook + post-session extraction | Phase 1 |
| 3. Multi-agent conflicts | 3/5 | **Direct solve** — LIVE_STATE.json file-lock awareness | Phase 1 |
| 4. No searchable history | 4/5 | **Partial solve** — HISTORY_INDEX.md + grep (Phase 1), FTS5 + sqlite-vec (Phase 2) | Phase 1-2 |
| 5. Decision amnesia | 5/5 | **Direct solve** — GENOME.md injection prevents re-debating | Phase 1 |
| 6. Tool fragmentation | 3/5 | **Partial solve** — Zellij layout + mise auto-init | Phase 1 |
| 7. No agent state visibility | 2/5 | **Direct solve** — LIVE_STATE.json + tail -f | Phase 1 |
| 8. Manual prompt engineering | 2/5 | **Indirect solve** — GENOME.md as self-evolving system prompt | Phase 1 |

**Impulse directly addresses 6 of 8 problems in Phase 1**, with the remaining 2 partially addressed and fully addressed by Phase 2.

### What No Single Tool Gets Right

1. **No tool combines extraction + injection + coordination.** mem0 does extraction. CLAUDE.md does injection. Nothing does coordination. Impulse aims to do all three.

2. **Every tool over-indexes on one layer.** claude-historian-mcp is search-only. mem0 is extraction-only. Zellij is workspace-only. Impulse is the integration layer that connects them.

3. **The file-based approach is undervalued.** Research confirmed that all four major competitors (Cursor, Windsurf, Cline, aider) use file-based persistence. The industry consensus is clear: files beat databases for < 100 sessions.

4. **Session-end extraction is the highest-leverage intervention.** It's the single moment where a session's knowledge can be captured. Tools that miss this moment (OneContext, claude-history) provide replay but not learning.

### What Impulse Should NOT Try to Be

1. **Not a general-purpose memory system.** Impulse is for coding agents, not chatbots. The extraction prompt is tuned for architectural decisions and coding preferences, not general facts.

2. **Not a replacement for CLAUDE.md.** GENOME.md captures emergent knowledge. CLAUDE.md captures intentional configuration. They're complementary, not competitive.

3. **Not a search engine.** Phase 1 doesn't need search. Phase 2 adds search only when HISTORY_INDEX.md grows beyond what grep can handle.

4. **Not a team collaboration tool.** Phase 1 is single-developer. Multi-developer coordination (merge conflicts in GENOME.md, team-wide knowledge) is Phase 3+.

---

## The Competitive Landscape (Simplified)

```
                    AUTOMATED EXTRACTION
                         ▲
                    HIGH │
                         │
              mem0 ●     │      ● Impulse (proposed)
                         │
                         │  ● Windsurf
                         │
              ● Cursor   │  ● Cline
                         │
                    LOW  │  ● aider
                         │
                         └──────────────────────►
                         LOW              HIGH
                              PORTABILITY
                       (locked to one editor/agent)
```

Impulse's unique quadrant: **high extraction automation + high portability** (agent-agnostic via Claude Code hooks primary + OpenCode adapter).

---

## Implications for ADRs

This problem map directly informs several Architecture Decision Records:

| ADR | Informed by Problem | Key Implication |
|-----|-------------------|-----------------|
| ADR-001: Platform strategy | Problem 1, 2, 3, 5 | Claude Code hooks are the primary target (1:1 mapping to needs). OpenCode adapter is Phase 1.5. |
| ADR-002: Memory format | Problem 1, 5, 8 | GENOME.md (file-based) is validated by competitive analysis. Add structured DB only when file-based breaks down. |
| ADR-003: Search strategy | Problem 4 | FTS5 before vectors. Plain grep for Phase 1. Add hybrid search only when HISTORY > 100 sessions. |
| ADR-004: Distribution model | Problem 6, 8 | Single installable package that configures hooks. Zero manual prompt engineering after setup. |
| ADR-005: Multi-agent coordination | Problem 3, 7 | File-lock awareness via LIVE_STATE.json. No vector similarity. No SWARM injection. |

---

## Open Questions

1. **Does GENOME.md injection actually change agent behavior?** (The core value hypothesis — research supports it but it's unproven for this specific use case)
2. **How fast does GENOME.md grow in real use?** (Determines when Phase 2 triggers)
3. **How often do multi-agent conflicts actually occur?** (Determines LIVE_STATE.json value)
4. **What's the false-positive rate of the extraction prompt?** (How many "decisions" are actually debugging notes?)
5. **Does the 40-character fingerprint dedup cause real problems?** (Needs empirical testing)

---

_Created: 2026-02-20 | Synthesized from 4 research documents (3,328 lines total)_
_Ratings are based on: existing documentation analysis, source code review, competitive research, and architectural assessment._
