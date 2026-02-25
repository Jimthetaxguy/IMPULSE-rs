---
status: accepted
phase: 1
audience: builder
tags: [decision, memory, files]
last_updated: 2026-02-20
---

# ADR-002: Three Plain Files, Not a Database

> **Status:** Accepted
> **Date:** 2026-02-20
> **Supersedes:** [0002-unified-steward.md](0002-unified-steward.md)

---

## Context

Impulse needs to persist two kinds of knowledge across sessions:

1. **Long-term knowledge** — Architectural decisions, coding preferences, project constraints (survives across weeks/months)
2. **Ephemeral state** — Which agents are active, what files they're editing, their current intent (survives only within a session)

The original ADR-002 proposed a "Unified Steward" with four operational modes backed by `live_state.db` (SQLite), vector embeddings, and a pattern detection engine. Phase 0 research revealed this is overengineered for the first 100+ sessions of real use.

### Competitive Validation (MEMORY-EXTRACTION-ANALYSIS.md §4)

All four major competitors use file-based persistence for cross-session coding context:

| Competitor | Storage | Format | Infrastructure |
|-----------|---------|--------|---------------|
| **Cursor Memory** | Files | Persistent records + MCP | Zero (file I/O only) |
| **Windsurf Cascade** | `~/.codeium/windsurf/memories/` | Categorized markdown | Zero |
| **Cline Memory Bank** | `memory-bank/` in project | Structured markdown | Zero |
| **aider** | Repository map (generated) | Tree-sitter symbol index | Zero |

None use vector databases, knowledge graphs, or SQLite for their core cross-session memory. The industry has converged on files.

### Infrastructure Cost of mem0 (MEMORY-EXTRACTION-ANALYSIS.md §1.7)

mem0's production pipeline requires: an LLM provider, an embedding model, a vector database (Qdrant/Chroma/Faiss), SQLite for audit history, and optionally Neo4j for graph relationships. For < 100 sessions, the infrastructure cost dwarfs the quality benefit. The LLM call cost difference is only 1.3-4x ($0.0015 vs $0.002-0.006 per session) — the real cost is operational complexity.

### Multi-Agent Coordination (LLM-CODING-PROBLEMS.md §3)

Vector-similarity approaches to multi-agent detection produce false positives — matching "User Profile" text across unrelated SQL and React files. File-path matching (comparing which files each agent is editing) is precise with zero false positives and no infrastructure.

---

## Decision

**Impulse persists knowledge in three plain-text files in `.impulse/`.** No database, no embeddings, no vector store in Phase 1. Multi-agent coordination uses LIVE_STATE.json file-path matching.

### The Three Files

#### 1. `.impulse/GENOME.md` — Permanent Knowledge

```markdown
# Project Genome
> Project: my-project | Created: 2026-02-20

## Architectural Decisions
- 2026-02-20: PostgreSQL with pgvector for embeddings storage
- 2026-02-20: JWT auth with 15-minute expiry, HttpOnly cookies

## Coding Preferences
- TypeScript strict mode, no implicit any
- Zod for all runtime validation

## Project Constraints
- API response time < 50ms (SLA)
- Must support Node 20+
```

- **Git status:** Committed (team-shared knowledge)
- **Updated by:** SessionEnd hook (automated extraction)
- **Read by:** SessionStart hook (injected as context), PreCompact hook (survival injection)
- **Growth rate:** ~2-5 lines per session (decisions only, not debugging noise)

#### 2. `.impulse/LIVE_STATE.json` — Ephemeral Agent Status

```json
{
  "agents": [
    {
      "id": "session-abc123",
      "startedAt": "2026-02-20T14:30:00Z",
      "lastActivity": "2026-02-20T14:35:22Z",
      "activeFiles": ["src/auth/jwt.ts", "src/middleware/cors.ts"],
      "recentTools": ["Write", "Edit", "Bash"]
    }
  ],
  "lastUpdated": "2026-02-20T14:35:22Z"
}
```

- **Git status:** Gitignored (ephemeral, session-only)
- **Updated by:** PostToolUse hook (every file-modifying tool call)
- **Read by:** SessionStart hook (shows other active agents), Zellij plugin (Phase 3 dashboard)
- **Multi-agent coordination:** Before editing a file, agents see if another agent has it in `activeFiles`

#### 3. `.impulse/HISTORY_INDEX.md` — Session Summaries

```markdown
# Session History

## Session 2026-02-20T14:30:00Z
**Duration:** 34 minutes | **Files modified:** 5
Implemented JWT authentication with refresh token rotation.
Chose bcrypt for password hashing over argon2 (Node.js native support).
Unresolved: CORS configuration for staging environment.

## Session 2026-02-19T10:15:00Z
...
```

- **Git status:** Committed (searchable team history)
- **Updated by:** SessionEnd hook (appended summary)
- **Read by:** SessionStart hook (last 3 entries injected as recent context)
- **Searchable by:** `grep` in Phase 1, FTS5 in Phase 2

### Phase Triggers to Add Complexity

Impulse adds infrastructure only when file-based persistence breaks down:

| Trigger | Signal | Upgrade Path |
|---------|--------|-------------|
| GENOME.md > 200 lines | Injection consuming too much context window | Add LLM-assisted pruning (1 call every 20 sessions) |
| Contradictory decisions accumulate | Agent confused by conflicting guidance | Add GENOME-aware extraction with UPDATE/DELETE classification |
| HISTORY_INDEX.md > 100 sessions | `grep` takes > 500ms | Add FTS5 full-text search index |
| GENOME.md > 500 lines | Keyword search fails semantically | Add sqlite-vec semantic search (see ADR-003) |
| 3+ agents regularly active | File-path matching insufficient | Add proper conflict resolution |

---

## Consequences

### Positive

- **Zero infrastructure** — No databases, no embedding models, no background services. `cat .impulse/GENOME.md` shows full memory state.
- **Git-trackable** — GENOME.md and HISTORY_INDEX.md are committed. Changes visible in `git log`. Team members see the same knowledge.
- **Human-readable** — Anyone can read, edit, or fix the files with a text editor. No opaque binary formats.
- **Debuggable** — When something goes wrong, `cat` the files. No need to query databases or decode embeddings.
- **Portable** — Copy `.impulse/` to a new machine and you have full project memory. No data migration.

### Negative

- **No contradiction resolution in Phase 1** — If a project switches from JWT to session-based auth, both decisions persist in GENOME.md until the next extraction detects the contradiction (requires ADR-004 improvement #4).
- **40-character substring deduplication is brittle** — "Use Zod for validation" and "Runtime validation with Zod schemas" are semantically identical but have different 40-char fingerprints. Duplicate entries will accumulate. Upgrade path: semantic dedup in Phase 2.
- **No structured querying** — Cannot ask "what decisions were made about auth?" without reading the entire GENOME.md. Acceptable at < 200 lines; requires search at scale.
- **Concurrent write risk** — If two SessionEnd hooks fire simultaneously, one write may clobber the other. Mitigated by LIVE_STATE.json tracking which agent sessions are active.
- **Git merge conflicts** — Two developers using Impulse on the same repo will create merge conflicts on GENOME.md. **Mitigation:** `impulse init` must add `.gitattributes` entries:
  ```
  .impulse/GENOME.md merge=union
  .impulse/HISTORY_INDEX.md merge=union
  ```
  The `union` merge strategy keeps all lines from both versions — correct for append-only files.
- **Privacy/team boundary missing** — All extracted content (including personal preferences) goes to git. **Phase 1.5 fix:** Split into `.impulse/PROJECT.md` (committed, team decisions) and `.impulse/PERSONAL.md` (gitignored, individual preferences). The extraction prompt must classify each entry.
- **Multi-agent coordination is advisory** — LIVE_STATE.json coordination depends on agents following the protocol. There is no structural enforcement. Agents that don't receive the LIVE_STATE instruction in their system prompt will not coordinate. **Phase 1.5 fix:** PreToolUse hook provides structural blocking. See PHASE1.5-COORDINATION.md.

---

## Alternatives Considered

### Alternative 1: Unified Steward with SQLite (original ADR-002)

Rejected because:
- Four operational modes (Live Coordination, Micro-Compaction, Cross-Session Persistence, Pre-Compaction Governance) are overengineered for Phase 1
- SQLite `live_state.db` provides no benefit over JSON for < 10 concurrent agents
- Vector embeddings add latency and complexity without clear quality improvement at < 100 sessions
- Pattern detection engine solves a problem that doesn't exist until multi-agent is a daily workflow

### Alternative 2: mem0 as Memory Backend

Deferred to Phase 3 because:
- Infrastructure cost (vector DB + embedding model + SQLite + optional Neo4j) is disproportionate to < 100 sessions
- LLM cost difference is only 1.3-4x ($0.002 vs $0.0015 per session)
- The real value of mem0 — contradiction resolution via ADD/UPDATE/DELETE — can be approximated in the extraction prompt (ADR-004, improvement #4)
- Migration path exists: mem0 can consume GENOME.md as input when it's adopted

### Alternative 3: CLAUDE.md as the Persistence Layer

Rejected as sole solution because:
- CLAUDE.md is static — it requires manual maintenance
- CLAUDE.md cannot capture emergent knowledge from sessions
- CLAUDE.md is complementary to GENOME.md, not a replacement: CLAUDE.md holds intentional configuration, GENOME.md holds extracted knowledge

---

## References

- MEMORY-EXTRACTION-ANALYSIS.md §4: Competitive extraction approaches (Cursor, Windsurf, Cline, aider all use files)
- MEMORY-EXTRACTION-ANALYSIS.md §1.7: mem0 real cost analysis ($0.002-0.006/session, infrastructure is the real cost)
- MEMORY-EXTRACTION-ANALYSIS.md §5: Trigger conditions for upgrading from single-call to pipeline
- LLM-CODING-PROBLEMS.md §3: Vector-similarity multi-agent detection produces false positives; file-path matching is precise
- REALISTIC-FRAMEWORK.md: 5-tier memory collapses to 2 tiers for Phase 1
