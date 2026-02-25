# Memory Pipeline — Impulse Knowledge Database

Unified SQLite FTS5 knowledge base for all Impulse project documentation. Replaces sparse markdown file searching with structured, queryable full-text search.

## Quick Start

```bash
# All tools accessible from project root via ./kdb dispatcher:
./kdb "hooks architecture"           # Full-text search
./kdb --summary                      # Project overview
./kdb status                         # One-line health check
./kdb session-start --task "hooks"   # Session bootstrap
./kdb contribute --inline '...'      # Bulk JSON contributions
./kdb session-end --session "s1" --summary "..."  # Session wrap-up
./kdb --rebuild                      # Rebuild after changing docs
```

## Commands

| Command                        | Description                                           |
| ------------------------------ | ----------------------------------------------------- |
| `kdb "search terms"`           | Full-text search across all docs, sections, decisions |
| `kdb --summary`                | Quick project overview                                |
| `kdb --decisions`              | Active ADR decisions                                  |
| `kdb --all-decisions`          | Include superseded (archived) ADRs                    |
| `kdb --risks`                  | Open risks by severity                                |
| `kdb --concepts`               | All tracked concepts with mention counts              |
| `kdb --concepts technology`    | Filter concepts by category                           |
| `kdb --stats`                  | Full database statistics                              |
| `kdb --phase 1`                | Documents for a specific phase                        |
| `kdb --tag hooks`              | Documents with a specific tag                         |
| `kdb --doc "PRODUCT-SPEC"`     | Find document by partial name                         |
| `kdb --related "PRODUCT-SPEC"` | Find related docs (shared concepts + links)           |
| `kdb --xref "GENOME.md"`       | Cross-references mentioning a target                  |
| `kdb --json "search terms"`    | JSON output (for agent consumption)                   |
| `kdb --rebuild`                | Rebuild database from `docs/`                         |

## What's Indexed

| Entity           | Count | Source                                                                     |
| ---------------- | ----- | -------------------------------------------------------------------------- |
| Documents        | 68    | All `.md` files in `docs/` + `CLAUDE.md`                                   |
| Sections         | 1,800 | `##` headings within documents                                             |
| Concepts         | 68    | Known technologies, patterns, principles, hooks, files                     |
| Concept Mentions | 4,522 | Where each concept appears (1,037 doc-level + 3,485 section-level)         |
| ADR Decisions    | 9     | Structured extraction from `docs/decisions/` and `docs/archive/decisions/` |
| Findings         | 95    | Severity-tagged items from Ralph Loop sessions                             |
| Risks            | 10    | Open risks from HONEST-ROADMAP and phase docs                              |
| Tags             | 84    | From YAML frontmatter                                                      |
| Cross-References | 193   | Markdown links between documents (98% resolved)                            |

## Architecture

```
docs/**/*.md ──→ extract.py ──→ knowledge.db ──→ query.py / kdb
                    │                │
                 Parses:          Contains:
                 - frontmatter    - FTS5 full-text indexes
                 - sections       - Structured ADR data
                 - ADR structure  - Concept tracking
                 - cross-refs     - Cross-reference graph
                 - concepts       - Convenience views
                 - findings       - Sync triggers
                 - risks
```

## Files

| File                 | Purpose                                                          |
| -------------------- | ---------------------------------------------------------------- |
| `kdb`                | Shell wrapper — entry point for query commands                   |
| `kdb-session-start`  | Shell wrapper — session bootstrap (replaces 4 manual commands)   |
| `kdb-session-end`    | Shell wrapper — session wrap-up (replaces 3 manual commands)     |
| `kdb-contribute`     | Shell wrapper — bulk JSON contribution tool                      |
| `kdb-status`         | Shell wrapper — one-line health check                            |
| `schema.sql`         | SQLite schema with FTS5 virtual tables, triggers, views, indexes |
| `extract.py`         | Python extraction pipeline — parses markdown, populates DB       |
| `query.py`           | Python query CLI — 12+ search modes, human and JSON output       |
| `kdb_common.py`      | Shared Python module — DB access, staleness, stats (importable)  |
| `session_start.py`   | Session bootstrap logic (used by kdb-session-start)              |
| `session_end.py`     | Session wrap-up logic (used by kdb-session-end)                  |
| `bulk_contribute.py` | Bulk contribution logic (used by kdb-contribute)                 |
| `status.py`          | Health check logic (used by kdb-status)                          |
| `knowledge.db`       | Generated database (~4.2 MB) — do not edit manually              |

## Concept Categories

| Category     | Examples                                                              |
| ------------ | --------------------------------------------------------------------- |
| `technology` | TypeScript, Bun, SQLite, FTS5, Claude Code, Rust, WASM                |
| `pattern`    | atomic writes, graceful degradation, Result&lt;T&gt;, fire-and-forget |
| `principle`  | file-first memory, progressive search, atomic writes                  |
| `hook`       | SessionStart, SessionEnd, PostToolUse, PreCompact                     |
| `file`       | GENOME.md, LIVE_STATE.json, HISTORY_INDEX.md                          |
| `agent`      | impulse, assistant                                                    |

## Contributing to the DB

Agents and developers can add findings, risks, and concepts without a full rebuild:

```bash
# Add a finding (bug, quality issue, observation)
kdb --add-finding HIGH "JSONL format unvalidated" "Need to test against real Claude Code transcripts"

# Add a risk
kdb --add-risk MEDIUM "DB grows unbounded with --add-* commands" "Run --rebuild periodically"

# Add a concept
kdb --add-concept "Ralph Loop" "pattern" "Iterative AI dev loop with same prompt"

# Log a session for traceability
kdb --log "Session 7" "Built knowledge database pipeline with FTS5"
```

These writes go directly to SQLite — no rebuild needed. They're immediately searchable.

**Severity levels:** `CRITICAL`, `HIGH`, `MEDIUM`, `LOW`
**Concept categories:** `technology`, `pattern`, `principle`, `hook`, `file`, `agent`, `constraint`, `tool`

## Staleness Check

```bash
kdb --check    # Exit 0 = current, Exit 1 = stale
```

Compares file modification times of all `docs/**/*.md` and `CLAUDE.md` against the DB build timestamp. Use before starting work to ensure you have current data.

## Rebuilding

Run `kdb --rebuild` after:

- Adding or editing any markdown file in `docs/`
- Changing `CLAUDE.md`
- Adding new concepts to `KNOWN_CONCEPTS` in `extract.py`
- `kdb --check` reports stale

The database is rebuilt from scratch each time (~2 seconds for 68 files). Incremental additions via `--add-*` are preserved until the next rebuild.

## Automation Tools

Three tools that reduce agent compliance from 6+ manual commands to 3 single-command calls.

### `kdb-session-start` — Session Bootstrap

Replaces `kdb --check` + `kdb "topic"` + `kdb --decisions` + `kdb --risks` with one call.

```bash
# Human-readable (includes staleness, ADRs, risks, recent sessions)
./kdb-session-start

# With task-relevant search (searches docs, decisions, findings for your topic)
./kdb-session-start --task "hooks implementation"

# JSON output for agents
./kdb-session-start --task "hooks" --json
```

### `kdb-session-end` — Session Wrap-Up

Replaces `kdb --log` + `kdb --rebuild` + `kdb --check` with one call.

```bash
# Log session and check staleness
./kdb-session-end --session "s7" --summary "Built automation tools"

# Log + auto-rebuild if stale
./kdb-session-end --session "s7" --summary "Built tools" --rebuild

# JSON output
./kdb-session-end --session "s7" --summary "Built tools" --json
```

### `kdb-contribute` — Bulk JSON Contributions

Replaces multiple `kdb --add-*` calls with a single JSON pipe.

```bash
# Pipe JSON (preferred for agents)
echo '{
  "session": "s7",
  "findings": [{"severity": "HIGH", "title": "Bug found", "description": "Details"}],
  "risks": [{"severity": "MEDIUM", "description": "Potential issue", "mitigation": "Plan"}],
  "concepts": [{"name": "NewTool", "category": "tool", "description": "What it does"}]
}' | ./kdb-contribute

# Inline JSON
./kdb-contribute --inline '{"findings":[{"severity":"LOW","title":"Minor note"}]}'
```

**Output:** `{"inserted": {"findings": 1, "risks": 1, "concepts": 1}, "skipped": {"concepts": 0}, "errors": []}`

- All inserts in a single transaction
- Duplicate concepts silently skipped (reported as `skipped`, not `errors`)
- Invalid severity/category reported in `errors` array
- Contributions tracked in `session_contributions` table for auditing

### `kdb-status` — Quick Health Check

One-line summary of database state. Useful for prompts, CI, or quick checks.

```bash
# Human-readable (one line)
./kdb-status
# Output: KDB: CURRENT | 68 docs | 95 findings | 10 risks | 4.5MB

# Machine-parseable (space-separated, no labels)
./kdb-status --porcelain
# Output: CURRENT 68 95 10 4464.0

# JSON (full stats)
./kdb-status --json
```

Exit codes: `0` = CURRENT, `1` = STALE, `2` = MISSING.

### Project-Root Dispatcher

All tools are also accessible from the project root via the `./kdb` dispatcher:

```bash
./kdb session-start --task "hooks"     # → kdb-session-start
./kdb session-end --session "s1" ...   # → kdb-session-end
./kdb contribute --inline '...'        # → kdb-contribute
./kdb status                           # → kdb-status
./kdb "search terms"                   # → kdb (original query)
./kdb --decisions                      # → kdb (original query)
```

## For Agents

### Required Agent Workflow

See `CLAUDE.md` section "Knowledge Database — Agent Requirements" for the full protocol. In short:

1. **Before work:** `kdb-session-start --task "your topic"`
2. **During work:** `kdb-contribute` (bulk) or `kdb --add-*` (single items)
3. **After work:** `kdb-session-end --session "name" --summary "what was done"`

Use `--json` for machine-readable output on any tool:

```bash
kdb-session-start --json               # Structured session context
kdb --json "search terms"              # Search results as JSON array
kdb-session-end --session x --summary y --json  # Wrap-up report as JSON
```
