# Impulse Handbook — Full Project Reference

> **"Your AI remembers. Silently."**
>
> This is the detailed reference for Impulse. For core principles and quick orientation,
> see `CLAUDE.md` (Claude Code) or `AGENTS.md` (OpenCode/other agents).

---

## Table of Contents

- [Architecture](#architecture)
- [Implementation Status](#implementation-status)
- [Command Reference](#command-reference)
- [Key Files](#key-files)
- [Dynamic Tooling System](#dynamic-tooling-system)
- [ImpulseAgent](#impulseagent)
- [TUI Reference](#tui-reference)
- [Code Style (Detailed)](#code-style-detailed)
- [Security Hardening](#security-hardening)
- [Platform Integration](#platform-integration)
- [Environment Variables](#environment-variables)
- [Roadmap](#roadmap)
- [Ralph Loop](#ralph-loop)
- [Quick Start](#quick-start)
- [What NOT to Use](#what-not-to-use)
- [Contract Ownership](#contract-ownership)
- [Release & Distribution](#release--distribution)

---

## Architecture

### Dual Mode

```
┌─────────────────────────────────────────────────────────────┐
│  Direct Mode (per-action, stateless)                         │
│  - For Claude Code/OpenCode hooks                          │
│  - Each invocation: read → process → write → exit          │
│  - cargo run -- track-write --file $CLAUDE_FILE ...        │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Daemon Mode (long-running)                                │
│  - For TUI, interactive chat                              │
│  - Unix socket IPC at .impulse/sockets/impulse.sock       │
│  - In-memory state with periodic file sync                │
│  - cargo run -- daemon                                     │
└─────────────────────────────────────────────────────────────┘
```

### Direct Mode (for hooks)

```bash
# Example: Claude Code hook
impulse-rs track-write --file $CLAUDE_FILE --session-id $IMPULSE_SESSION_ID

# Example: Session start
SESSION_ID=$(impulse-rs session-start -n "project-x" -p claude-code)
export IMPULSE_SESSION_ID=$SESSION_ID
```

### Daemon Mode

```bash
# Start daemon (in background)
impulse-rs daemon &

# Interact via socket
impulse-rs --daemon status
impulse-rs --daemon session-start -n "chat-session"
```

### Session ID Pattern

- Format: `{sanitized-cwd}-{timestamp}-{uuid8}`
- Example: `cli-cu-l8r-20260223-143052-a1b2c3d4`
- Auto-generated if not provided
- Can be set via env var: `IMPULSE_SESSION_ID`

### The Data Files

| File                                  | Purpose                                    | Persistence            |
| ------------------------------------- | ------------------------------------------ | ---------------------- |
| `.impulse/LIVE_STATE.json`            | Active sessions, files, tools              | Ephemeral (gitignored) |
| `.impulse/HISTORY.jsonl`              | Session history (append-only)              | Git-committed          |
| `.impulse/GENOME.md`                  | Permanent decisions, preferences           | Git-committed          |
| `.impulse/config.json`                | Runtime configuration                      | Git-committed          |
| `.impulse/context/*`                  | Handoff + shared context files             | Runtime artifacts      |
| `.impulse/context/injections/*`       | Review/apply injection bundles + audit log | Runtime artifacts      |
| `.impulse/retrieval.db`               | Retrieval index/cache (SQLite)             | Rebuildable cache      |
| `.impulse/retrieval_index_state.json` | Retrieval index metadata                   | Rebuildable metadata   |
| `.impulse/embeddings/*`               | Embedding temp artifacts                   | Runtime cache          |
| `.impulse/retrieval.lock`             | Retrieval indexing lock guard              | Runtime safety         |
| `.impulse/CONFLICTS.jsonl`            | File conflict audit trail (append-only)    | Git-committed          |
| `.impulse/sockets/impulse.sock`       | Daemon Unix socket                         | Runtime                |
| `.impulse/sockets/impulse.pid`        | Daemon PID file                            | Runtime                |

### Honest Limitations

| Limitation | Status | Workaround |
|------------|--------|------------|
| Contradiction resolution | Open | Append-only; prune manually |
| Structural agent conflict prevention | Open | Advisory only; agents may ignore |
| Guaranteed extraction quality | Open | LLM may miss implicit decisions |
| Team privacy separation | Open | All GENOME content goes to git |

---

## Implementation Status

| Component                            | Status         | Location                              |
| ------------------------------------ | -------------- | ------------------------------------- |
| **Core CLI**                         | Complete    | `impulse-rs/src/main.rs`              |
| **Storage layer**                    | Complete    | `impulse-rs/src/storage/mod.rs`       |
| **State management**                 | Complete    | `impulse-rs/src/state/mod.rs`         |
| **LLM providers**                    | Complete    | `impulse-rs/src/agent/`               |
| **Daemon socket IPC**                | Complete    | `impulse-rs/src/daemon/mod.rs`        |
| **Claude Code hooks**                | Complete    | `impulse-rs/.claude/hooks/hooks.json` |
| **OpenCode integration**             | Complete    | `impulse-rs/.opencode/impulse.json`   |
| **Tests**                            | 1,698 passing (4 ignored) | `impulse-rs/src/*/tests.rs` |
| **Branding**                         | Complete    | `impulse-rs/src/branding.rs`          |
| **Phase 2 (chat context)**           | Complete    | Session context in daemon chat        |
| **Retrieval foundation**             | Complete    | `impulse-rs/src/retrieval/`           |
| **Context injection (review-first)** | Complete    | `impulse-rs/src/injection/`           |
| **Credentials management**           | Complete    | `impulse-rs/src/credentials/`         |
| **Documentation fetcher**            | Complete    | `impulse-rs/src/docs/`               |
| **Tool management**                  | Complete    | `impulse-rs/src/tools/`               |
| **Stewardship**                      | Complete    | `impulse-rs/src/stewardship/`         |
| **Token tracker**                    | Complete    | `impulse-rs/src/token_tracker/`       |
| **Build hygiene**                    | Complete    | `impulse-rs/src/build_hygiene/`       |
| **Dynamic tooling (23 tools)**       | Complete    | `impulse-rs/src/tooling/`             |
| **Agent discovery**                  | Complete    | `impulse-rs/src/agent_discovery/`     |
| **Notification bus**                 | Complete    | `impulse-rs/src/notification/`        |
| **Orchestration + Handoff**          | Complete    | `impulse-rs/src/orchestration/`       |
| **TUI + PTY + Pane Manager**         | Complete    | `impulse-rs/src/ui/`                  |
| **Branding (Shockwave sprites)**     | Complete    | `impulse-rs/src/branding.rs`          |
| **ImpulseAgent (LLM coordination)** | Complete    | `impulse-rs/src/impulse_agent/`       |
| **Context Lifecycle**                | Complete    | `impulse-rs/src/context_lifecycle/`   |
| **Intent Detection**                 | Complete    | `impulse-rs/src/intent/`              |
| **Guardrail engine**                 | Complete    | `impulse-rs/src/guardrail/`           |
| **Plugin system**                    | Complete    | `impulse-rs/src/plugin/`              |
| **Semantic diff**                    | Complete    | `impulse-rs/src/semantic_diff/`       |
| **Desktop host (Dioxus Desktop)**  | Scaffold + bridge parity complete | `impulse-rs/impulse-desktop/`         |
| **GUI (egui native workbench)**     | Legacy / frozen | `impulse-rs/impulse-gui/` compile-maintenance only |
| **Terminal widget (PTY + vt100)**   | Complete    | `impulse-rs/impulse-term/`            |
| **IPC protocol versioning**          | Complete    | Daemon + GUI version negotiation      |
| **Signal bus (GUI)**                 | Complete    | `impulse-gui/src/widgets/signal_bus.rs` |
| **Supervisor permissions**           | Complete    | `impulse-ops/` + GUI settings view    |
| **Direct-mode chat**                 | Complete    | `handlers/system.rs` (async LLM call) |
| **Stale socket cleanup**            | Complete    | `daemon/mod.rs` (startup detection)   |
| **Conflict audit trail**            | Complete    | `state/persistence.rs` (CONFLICTS.jsonl) |
| **Structured logging**              | Complete    | `daemon/mod.rs` (tracing-subscriber)  |

### Codebase Metrics

| Metric | Value |
|--------|-------|
| Total Rust source files | 161 (main) + 50+ (impulse-term, impulse-gui) |
| Total lines of code | ~69K+ (53K main + 2.7K term + 13K gui) |
| Source modules | 35 declared in `main.rs` |
| Tests passing (`cargo test`) | 1,698 (1368+26 impulse-rs + 31 ops + 114 term + 159 desktop, 4 ignored; impulse-gui frozen/excluded) |
| Feature flags | `office-support` (default), `monty-support`, `datafusion-support` |
| DynamicTools registered | 23 |

---

## Command Reference

```bash
# Build & Run
cd impulse-rs
cargo build
cargo run -- --help

# Install globally
cargo install --path .

# Session management
cargo run -- init
cargo run -- session-start -n "my-project" -p claude-code
cargo run -- session-end --session-id <id> --summary "Fixed bug"
cargo run -- session-end --session-id <id> --summary "Fixed bug" --verify
cargo run -- track-write --file src/main.rs --session-id <id>
cargo run -- track-tool --tool Write --session-id <id>

# Info & status
cargo run -- status                  # Full system status with splash
cargo run -- list-sessions
cargo run -- session-info --id <id>
cargo run -- history
cargo run -- activity --limit 20
cargo run -- genome
cargo run -- summary
cargo run -- health
cargo run -- system
cargo run -- analyze

# Build hygiene
cargo run -- build-health [--json]
cargo run -- sweep [--dry-run true|false] [--path <dir>] [--days <n>]
cargo run -- wipe [--dry-run true|false] [--path <dir>]
cargo run -- clean-all [--dry-run true|false]
cargo run -- sccache-setup [--check] [--json]

# Retrieval & search
cargo run -- index-memory [--scope history|genome|all] [--rebuild]
cargo run -- search-history --query "auth" --mode keyword --backend auto --explain --json
cargo run -- search-genome --query "decision" --mode semantic --backend rust-cosine --explain --json
cargo run -- retrieval-status --check --json

# Agent discovery & coordination
cargo run -- agent-discover          # Write capabilities manifest to .impulse/
cargo run -- agent-discover --json   # Print manifest as JSON (pipe to agent)
cargo run -- agent-discover --summary # Brief overview of capabilities

# ImpulseAgent (LLM coordination)
cargo run -- agent-configure --provider anthropic --api-key $ANTHROPIC_API_KEY
cargo run -- agent-configure --harness claude-code
cargo run -- agent-status [--json]
cargo run -- agent-query "Review cross-pane activity" [--json]

# TUI (terminal multiplexer)
cargo run -- run                     # Launch TUI with PTY panes
# Ctrl+B c = new shell, Ctrl+B C = Claude, Ctrl+B X = Codex, Ctrl+B O = OpenCode
# Ctrl+B n/p = next/prev pane, Ctrl+B x = close, Ctrl+B [ = scroll mode
# Ctrl+B s = toggle sidebar, Ctrl+B i = chat input, Ctrl+B ? = help overlay

# Orchestration & context
cargo run -- verify
cargo run -- orchestrate --task "review auth changes" --inject-mode review --inject-explain
cargo run -- handoff --tool codex --task "continue debugging" --inject-mode review --inject-explain
cargo run -- sync-context --inject-mode review --inject-explain

# Stewardship
cargo run -- steward status
cargo run -- steward analyze [--transcript <path>] [--session-id <id>] [--json]
cargo run -- steward compact [--session-id <id>]
cargo run -- steward approve --id <proposal-id>
cargo run -- steward reject --id <proposal-id>

# Tool & model management
cargo run -- tools list
cargo run -- tools init --tool <name>
cargo run -- tools update [--dry-run]
cargo run -- docs list [--provider <name>]
cargo run -- docs fetch [--provider <name>] [--force]
cargo run -- model [--provider <name>] [--model <name>]
cargo run -- credentials set --provider <name> --key <key>
cargo run -- credentials list

# Dynamic tooling (agent-invokable tools)
cargo run -- tooling-list [--category system|utility|analysis|document]
cargo run -- tooling-describe <tool-id>
cargo run -- tooling-run <tool-id> --params '{"key":"value"}' [--json]
cargo run -- tooling-schema [--format json|markdown]

# Monty Commands (Computed Routing & Context Injection)
cargo run -- compute-injection --query "<query>" [--limit N] [--json]
cargo run -- extract --content "<text>" [--session-id <id>] [--json]
cargo run -- swarm --agent-a <name> --agent-b <name> [--threshold 0.88] [--json]

# DataFusion Commands (Optional)
cargo run -- analytics --metric <name> --group-by <field>  # requires datafusion-support

# Utilities
cargo run -- calc --expression "2+2"
cargo run -- exec --code "print('hi')"

# Config
cargo run -- config                    # Show all
cargo run -- config <key>              # Get value
cargo run -- config <key> --value <val>  # Set value

# Guardrails
cargo run -- guard --action "git push --force" --target bash --json
cargo run -- guard --list
cargo run -- guard --enable <rule-id>
cargo run -- guard --disable <rule-id>

# Semantic diff/blame/impact
cargo run -- sem-diff --base HEAD~1 --head HEAD [--json]
cargo run -- sem-blame --file src/main.rs [--json]
cargo run -- sem-impact --entity handle_chat [--json]
cargo run -- sem-status [--json]

# Conflict analytics
cargo run -- conflict-history
cargo run -- analytics conflicts [--json] [--period day|week|month|all]

# Debug (daemon internal state snapshot)
cargo run -- debug
cargo run -- --daemon debug

# Plugins
cargo run -- plugin-list [--json]
cargo run -- plugin-invoke <name> [--path <p>] [--query <q>] [--json]

# ATCC command discovery
cargo run -- describe                  # Full command registry
cargo run -- schema <command>          # JSON Schema for a command

# Daemon (for TUI/chat)
cargo run -- daemon                    # Start daemon
cargo run -- run                       # Start TUI
cargo run -- --daemon status          # Query daemon
cargo run -- --daemon chat --session-id <id> --message "hi" --inject-mode review --inject-explain

# Check the Dioxus Desktop host scaffold
cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop

# With custom impulse directory
cargo run -- -c /path/to/.impulse <command>

# With custom socket path
cargo run -- --socket /path/to/socket.sock <command>

# Feasibility benchmark (PageIndex research track)
python3 memory-pipeline/pageindex_feasibility_benchmark.py --root . --queries memory-pipeline/pageindex_eval_queries.sample.json --out docs/research/pageindex-feasibility-report.json
python3 memory-pipeline/retrieval_perf_harness.py --root . --impulse-dir .impulse --out docs/research/retrieval-perf-report.json

# Test
cargo test
```

---

## Legacy GUI Workbench (impulse-gui)

Legacy/frozen egui application that provided the earlier visual workbench for Impulse. Active desktop work now lives in `impulse-rs/impulse-desktop/` as the Dioxus Desktop host; `impulse-gui` is retained for compile-maintenance only.

### Views

| View | Shortcut | Description |
|------|----------|-------------|
| Overview | Ctrl+1 | Connection stats, session summary, signal history |
| Terminals | Ctrl+2 | PTY multiplexer with context lifecycle indicators |
| Context | Ctrl+3 | Context telemetry, injection preview (Essential/Critical/Minimal) |
| Memory | Ctrl+4 | Session history, genome decisions, search |
| Artifacts | — | Artifact browser with render modes (Markdown, RawJson, Error) |
| Settings | — | Config editor, supervisor permissions (interactive toggles), agent detection |

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+N | New terminal tab |
| Ctrl+W | Close current tab |
| Ctrl+Tab | Cycle tabs |
| Ctrl+L | Focus agent panel |
| Ctrl+B | Toggle sidebar |
| Ctrl+K | Focus search |
| Ctrl+S | Explicit session save |
| Ctrl+E | Toggle agent panel |
| Ctrl+/ | Shortcuts help overlay |

### Signal Bus

The signal bus collects, debounces, and routes GUI events:
- **ContextThreshold** — context window pressure alerts
- **ErrorEncountered** — errors from agent panes
- **TaskCompleted** — task completion events
- **CompactionDetected** — context compaction events
- **FileConflict** — cross-pane file conflicts

Signals appear as tab badges, toast notifications, and in the Signal History section of Overview.

### Daemon Connection Health

The status bar shows real-time connection health:
- RTT color coding: green (<10ms), yellow (<100ms), red (>=100ms)
- Protocol version mismatch warnings
- Disconnect count tracking

---

## Key Files

| File                                | Purpose                                        |
| ----------------------------------- | ---------------------------------------------- |
| `impulse-rs/src/main.rs`            | CLI entry, command routing (2,764 lines)       |
| `impulse-rs/src/storage/mod.rs`     | Atomic file operations (11 tests)              |
| `impulse-rs/src/state/`             | In-memory state + Drop sync (mod.rs + config.rs + persistence.rs + session.rs, 13 tests) |
| `impulse-rs/src/daemon/mod.rs`      | Unix socket server, chat wired (15 tests)      |
| `impulse-rs/src/client/mod.rs`      | Daemon client                                  |
| `impulse-rs/src/agent/`             | LLM provider trait + Anthropic/OpenAI/Minimax (8 tests) |
| `impulse-rs/src/memory/mod.rs`      | GENOME, Genome struct                          |
| `impulse-rs/src/branding.rs`        | UI branding constants                          |
| `impulse-rs/src/error.rs`           | Global error type definitions                  |
| `impulse-rs/src/session/mod.rs`     | Session lifecycle management                   |
| `impulse-rs/src/orchestration/mod.rs` | Multi-step orchestration logic (3 tests)     |
| `impulse-rs/src/ui/`                | Terminal UI with ratatui (mod.rs + types.rs + runner.rs + agent_terminal.rs + lifecycle.rs + render_panels.rs + pane_manager + visualization, 6 tests) |
| `impulse-rs/src/verify/mod.rs`      | Verification gate logic (2 tests)              |
| `impulse-rs/src/retrieval/`         | SQLite FTS5, embeddings, indexer (7 files, 12 tests) |
| `impulse-rs/src/injection/`         | Context injection engine (9 tests)             |
| `impulse-rs/src/credentials/`       | Keychain, socket proxy, CLI proxy (2 tests)    |
| `impulse-rs/src/docs/`              | Doc fetcher, models, cache (10 tests)          |
| `impulse-rs/src/tools/`             | Tool init, list, update, benchmark, health, python, system (15 tests) |
| `impulse-rs/src/stewardship/`       | Context stewardship, token monitoring (7 files, 38 tests) |
| `impulse-rs/src/token_tracker/`     | Token tracking algorithm, compaction metrics (6 files, 13 tests) |
| `impulse-rs/src/build_hygiene/`     | Rust build artifact management (9 files, 63 tests) |
| `impulse-rs/src/build_hygiene/native.rs` | Native filesystem cleaning, no external tool deps (18 tests) |
| `impulse-rs/src/tooling/`           | Dynamic tool registry, DynamicTool trait, 19 built-in tools (72 tests) |
| `impulse-rs/src/office/`            | Office doc parsing — feature-flagged (4 files, 11 tests) |
| `impulse-rs/src/monty/`             | Computed routing, dynamic injection (6 files, 17 tests) |
| `impulse-rs/src/impulse_agent/`     | Dual-mode LLM agent: API + CLI harness coordination (3 files, 26 tests) |
| `impulse-rs/src/context_lifecycle/` | Context window monitor, injector, extractor, detector (7 files, 64 tests) |
| `impulse-rs/src/intent/`            | Intent detection from agent PTY output (4 files, 11 tests) |
| `impulse-rs/src/integration_tests.rs` | Integration test suite with DaemonGuard RAII |

---

## Dynamic Tooling System

The `tooling/` module provides a unified, agent-invokable tool surface. Every Impulse capability is wrapped as a `DynamicTool` with parameter validation, capability enforcement, and structured JSON output.

### Architecture

```
DynamicTool trait (async, Send+Sync)
    │
    ├── ToolRegistry (register, get, list, execute)
    │       └── Capability enforcement (deny-by-default)
    │
    ├── Built-in tools (19 wrappers, zero code duplication)
    │       ├── Analysis: benchmark, genome_read, memory_search, session_query
    │       ├── System: build_health, clean_all, config_get, health_check,
    │       │           sccache_setup, sccache_status, steward_status, sweep,
    │       │           system_info, tool_availability, wipe
    │       ├── Utility: calculator, file_read, python_exec
    │       └── Document: document_extract
    │
    └── CLI commands
            ├── tooling-list      — List available tools
            ├── tooling-describe  — Show tool parameters
            ├── tooling-run       — Execute with JSON params
            └── tooling-schema    — Export Claude tool-calling schema
```

### Agent Integration

**Via CLI (subprocess):**
```bash
impulse-rs tooling-schema --format json  # Discover tools
impulse-rs tooling-run python_exec --params '{"code":"print(42)"}' --json
```

**Via Daemon IPC (Unix socket):**
```json
{"type":"ListTools","data":{"category":"analysis"}}
{"type":"DescribeTool","data":{"name":"memory_search"}}
{"type":"InvokeTool","data":{"name":"session_query","params":{"query":"auth","limit":5}}}
{"type":"ToolSchema","data":{}}
```

**Schema export** produces Claude-compatible tool definitions with `name`, `description`, and `input_schema` (JSON Schema with `properties` and `required`).

---

## ImpulseAgent

The ImpulseAgent is the always-on tech lead. It manages, monitors, and augments other coding agents. Agents primarily live in the terminal and login/attach as CLI-TUI agents (e.g. "claude code", "codex cli", "cursor cli", and equivalents). The goal is that these agents continue to act normally on tasks while:

1. Impulse helps manage/monitor them and augments their context.
2. A light UI lets you pick different folders/workspaces in one place and cycle between one or many project spaces + one or many agents per space — without different interfaces per project/agent.
3. Agents wired into Impulse are augmented with extra tools and capabilities (effectively a built-in, type-safe Rust plugin/extension that is efficient).
4. Subagents and workflows are used to scale capabilities and reduce the load that coding agents consume on the machine.

It operates in two modes:

| Mode | Description | Use Case |
|------|-------------|----------|
| **API mode** | Direct LLM API calls (Anthropic, OpenAI, Minimax) | Full autonomy, needs API key |
| **Harness mode** | Delegates to a CLI harness (Claude Code, Codex, Cursor, etc.) via subprocess + PTY | Uses existing agent session under supervision |

### How It Works

1. **Context Lifecycle** monitors all PTY panes for context window usage and agent output
2. **Extractor** pulls insights from agent sessions: files modified, errors encountered, decisions made
3. **Coordinator** aggregates insights and detects cross-pane conflicts, error cascades, sync opportunities
4. **ImpulseAgent** generates actionable recommendations via LLM or forwards to CLI harness

### Commands

```bash
# Configure with API provider
impulse-rs agent-configure --provider anthropic --api-key $ANTHROPIC_API_KEY

# Configure with CLI harness
impulse-rs agent-configure --harness claude-code

# Check agent status
impulse-rs agent-status [--json]

# Query the agent (requires configuration)
impulse-rs agent-query "Review cross-pane activity" [--json]
```

### Context Lifecycle Module

The context lifecycle pipeline (`src/context_lifecycle/`) manages bidirectional context flow:

| Component | File | Purpose |
|-----------|------|---------|
| Types | `types.rs` | `PaneContextState`, `ExtractedInsight`, `InsightType`, `ContextTier` |
| Monitor | `monitor.rs` | Tracks context window usage, triggers injection at 45/60/80% thresholds |
| Injector | `injector.rs` | Pushes relevant context at spawn and at threshold boundaries |
| Extractor | `extractor.rs` | Pulls key info from agent output (file mods, errors, decisions) |
| Detector | `detector.rs` | Detects compaction events and context window resets |
| Templates | `templates.rs` | Injection prompt templates for different agent kinds |

### Coordination Types

The coordinator detects four types of cross-pane coordination needs:

- **FileConflict**: Multiple agents modifying the same file
- **ErrorAssist**: Error in one pane possibly caused by another pane's changes
- **CrossPaneSync**: Knowledge in one pane that would benefit another
- **TaskComplete**: Agent finished a task, notify others

---

## TUI Reference

### Features (9 tabs)

| Tab | Name      | Description                                |
| --- | --------- | ------------------------------------------ |
| 0   | Dashboard | Overview with stats and activity sparkline |
| 1   | Sessions  | Manage sessions with filtering (press 'f') |
| 2   | Timeline  | Visual timeline of sessions                |
| 3   | History   | Past sessions list                         |
| 4   | Genome    | Decisions & preferences                    |
| 5   | Search    | Full-text search (press '/')               |
| 6   | Analytics | Metrics, platform breakdown, trends        |
| 7   | Chat      | Chat with context (press 'i')              |
| 8   | Config    | Help & shortcuts                           |

### Keyboard Shortcuts

- `n` - New session
- `e` - End session
- `t/T` - Track file/tool
- `f` - Filter sessions
- `r` - Refresh
- `s` - Select session
- `/` - Search (in search tab)
- `g/a/h` - Go to Genome/Analytics/History
- `0-8` - Go to specific tab
- `q` - Quit

### In-TUI Commands

- `/track <path>` - Track file to session
- `/tool <name>` - Track tool to session
- `/tag <name>` - Add tag to session
- `/search <query>` - Search sessions
- `/session <name>` - Create new session

---

## Code Style (Detailed)

### Core Rules

- **Strict** — No `any`, proper types
- **thiserror** — For error enums
- **anyhow** — For application errors
- **Result<T>** — Never panic, return Result<T, E>
- **Atomic writes** — Always temp file + rename (unique temp names)
- **Dirty flag** — Track state changes, sync on Drop
- **Input validation** — Sanitize user-supplied IDs before using as path/SQL components

### Naming Conventions

| Type          | Convention           | Example               |
| ------------- | -------------------- | --------------------- |
| Modules       | kebab-case           | `state/mod.rs`        |
| Structs/Enums | PascalCase           | `Session`, `Platform` |
| Functions     | snake_case           | `create_session`      |
| Constants     | SCREAMING_SNAKE_CASE | `SOCKET_NAME`         |

### File Operations

Atomic writes — always use temp file + rename:
```rust
let temp_path = path.with_extension("tmp");
let mut file = File::create(&temp_path)?;
file.write_all(content.as_bytes())?;
drop(file);
fs::rename(&temp_path, &path)?;
```

- **JSONL append** — Use `OpenOptions` with append mode
- **Result wrapping** — All file ops return `Result<T>`

### State Management

- **tokio::sync::RwLock** — For async state
- **std::sync::RwLock** — For sync state with try_read/try_write
- **Dirty flag** — Track when state needs sync to disk
- **Sync on exit** — Always persist dirty state before exit

---

## Security Hardening

Comprehensive code review (25 findings across 3 review agents, 2026-02-24):

| Category | Fix | Files |
|----------|-----|-------|
| Path traversal | ID sanitization on all user-supplied filesystem path components | `stewardship/approval.rs`, `stewardship/cross_project.rs` |
| Code injection | JSON encoding for untrusted data passed to Python code generation | `office/extraction.rs` |
| UTF-8 panic | Char-boundary-safe string slicing in document chunking | `office/extraction.rs` |
| Protocol injection | Whitespace/newline validation on socket credential protocol | `credentials/socket.rs` |
| SQL injection | Table name allowlist for PRAGMA queries | `retrieval/store.rs` |
| Unwrap panics | Replaced `unwrap()` with proper error handling on production paths | `daemon/mod.rs`, `client/mod.rs` |
| Unbounded buffer | Added 10MB size limit on daemon IPC request lines | `daemon/mod.rs` |
| Atomic writes | Fixed non-atomic write to `~/.cargo/config.toml` | `build_hygiene/sccache.rs` |
| Temp file collision | Unique temp file names using PID + timestamp | `storage/mod.rs` |
| Heap waste | Direct arithmetic instead of string allocation for token estimation | `stewardship/analyzer.rs` |
| Vault key ignored | Fixed credential vault `get()` to use the `key` parameter | `credentials/cli_proxy.rs` |
| Timeout enforcement | Implemented real process timeout for Python execution | `tools/python.rs` |

---

## Platform Integration

### Claude Code Hooks

Hooks are auto-generated at `.claude/hooks/hooks.json`:

```json
{
  "matchers": [
    { "type": "tool", "name": "session_start" },
    { "type": "tool", "name": "session_end" },
    { "type": "tool", "name": "Write" },
    { "type": "tool", "name": "Edit" },
    { "type": "tool", "name": "Bash" }
  ],
  "hooks": [
    {
      "type": "command",
      "command": "impulse-rs track-write --file $CLAUDE_FILE --session-id $IMPULSE_SESSION_ID"
    }
  ]
}
```

### OpenCode Integration

Config at `.opencode/impulse.json` with equivalent hooks.

---

## Environment Variables

| Variable             | Purpose                | Default                  |
| -------------------- | ---------------------- | ------------------------ |
| `ANTHROPIC_API_KEY`  | Anthropic API for chat | Required for daemon chat |
| `IMPULSE_MODEL`      | Chat model             | claude-sonnet-4-20250514 |
| `IMPULSE_SESSION_ID` | Current session ID     | Auto-generated           |

---

## Roadmap

| When       | Focus                                                                  | Tech        | Status       |
| ---------- | ---------------------------------------------------------------------- | ----------- | ------------ |
| **Now**    | Core CLI + direct mode                                                 | Rust        | **Complete** |
| **Now**    | Daemon + socket IPC                                                    | Rust        | **Complete** |
| **Now**    | Enhanced TUI (9 tabs)                                                  | Rust        | **Complete** |
| **Now**    | Visualization module                                                   | Rust        | **Complete** |
| **Now**    | Search & Analytics tabs                                                | Rust        | **Complete** |
| **Now**    | Session tagging                                                        | Rust        | **Complete** |
| **Now**    | Filter system                                                          | Rust        | **Complete** |
| **Now**    | Tests (1,698 passing)                                                  | Rust        | **Complete** |
| **Now**    | Dioxus Desktop host scaffold                                           | Rust        | **Complete** |
| **Now**    | Terminal bridge in Dioxus host                                         | xterm.js    | **Complete** |
| **Now**    | Context stewardship                                                    | Rust        | **Complete** |
| **Now**    | Token tracking algorithm                                               | Rust        | **Complete** |
| **Now**    | Tool/model/credential management                                       | Rust        | **Complete** |
| **Now**    | System utilities (calc, exec, health, system)                          | Rust        | **Complete** |
| **Next**   | Retrieval + review-first context injection (feature-flagged, additive) | Rust+Python | **Complete** |
| **Next**   | Monty integration (computed routing, dynamic injection, KDB, SWARM)   | Rust+Python | **Complete** |
| **Later**  | Desktop bundle                                                         | Dioxus Desktop | **Open**  |
| **Later**  | DataFusion analytics (optional)                                        | Rust        | **Open**     |
| **Future** | Agent VCS + dashboard                                                  | +Rust       | Vision       |

### Phase Details

**Now (Phase 1-3 - Complete):**
- Direct mode: `session-start`, `session-end`, `track-write`, `track-tool`
- Daemon mode: Unix socket server at `.impulse/sockets/impulse.sock`
- TUI: 9-tab interface (Dashboard, Sessions, Timeline, History, Genome, Search, Analytics, Chat, Config)
- File persistence: atomic writes (temp + rename)
- Session IDs: `{working-dir}-{timestamp}-{uuid8}`
- Platform detection: `--platform claude-code` or `--platform opencode`
- Config, hooks, chat with session context, Dioxus Desktop host migration
- Context stewardship, token tracking, tool/model/credential management
- System utilities: health check, system info, Python calc/exec

**Next (Retrieval + Monty - Complete):**
- Retrieval index (`retrieval.db`) with FTS5 keyword search
- Feature-flagged semantic path with fallback-safe behavior
- Review-first context injection on daemon chat and direct orchestration
- Monty Python interpreter integration (computed routing, dynamic injection, KDB, SWARM)

**Later (Coordination UX):**
- Advanced coordination UX and agent dashboarding

### What We Learned

| Finding | Validation |
|---------|------------|
| SessionStart hook stdout → system context | Works (production) |
| PreCompact survival | Works (production) |
| TypeScript → Rust pivot | Solved binary distribution |
| Hook spawn overhead (50-200/hr) | Resolved via daemon/socket IPC |
| GENOME.md all-or-nothing loading | Still open - no selective loading |

---

## Historical Ralph Loop Protocol

This section is historical methodology from the archived Ralph plans. Current agent process guidance lives in `AGENTS.md` and `docs/guides/COLLABORATIVE-AGENTIC-CODING.md`.

Public contributors do not need to follow the archived Ralph loop protocol for
new work; use the active roadmap, ADRs, and collaboration guide instead. The
details below remain only to preserve provenance for older work logs.

### Requirements

Historical Ralph Loop agents (both Claude and legacy OpenCode) followed these rules:

#### (i) Reflect on Work

At regular intervals (every 5-10 iterations), agents MUST:
- Review what was built vs what was planned
- Identify gaps or issues
- Adjust approach if needed
- Document findings in session log

#### (ii) Build Optimal, Not Just Build

Before implementing, agents MUST:
- Consider alternative approaches
- Evaluate trade-offs (performance, maintainability, complexity)
- Choose the simplest solution that works
- Avoid over-engineering

### Historical Ralph Loop Template

```
# Ralph Loop Session - [Feature Name]

## Iteration X: [What was done]
- Built: [list]
- Issues found: [list]
- Reflection: [what should change]

## Next Steps
- Continue: [what's next]
- Pivot: [if approach not working]
```

---

## Quick Start

1. Build: `cd impulse-rs && cargo build`
2. Initialize: `cargo run -- init`
3. Create session: `cargo run -- session-start -n "my-project" -p claude-code`
4. Track files: `cargo run -- track-write --file src/main.rs --session-id <id>`
5. End session with gate: `cargo run -- session-end --session-id <id> --summary "Fixed bug" --verify`

---

## What NOT to Use

| Path            | Status        | Reason                 |
| --------------- | ------------- | ---------------------- |
| `impulse/`      | Deprecated    | Old TypeScript project |
| `harness/`      | Deprecated    | Pre-pivot reference    |
| `docs/archive/` | Reference     | Old docs               |

---

## Contract Ownership

Source-of-truth files for contract changes:

- `docs/spec/RUST-CANONICAL-CONTRACT.md` (authoritative)
- `AGENTS.md`
- `CLAUDE.md`
- `HANDBOOK.md`
- `docs/INDEX.md`
- `docs/SUMMARY.md`

Required verification before merge:

```bash
python3 docs/validate_docs.py
python3 docs/validate_docs.py --contract
cd impulse-rs && cargo test
```

---

## Release & Distribution

### Creating a Release

1. Update version in `impulse-rs/Cargo.toml`
2. Update version in `impulse-rs/impulse-desktop/Cargo.toml`
3. Commit: `git commit -m "chore: bump version to vX.Y.Z"`
4. Tag: `git tag vX.Y.Z`
5. Push: `git push && git push --tags`
6. GitHub Actions builds and publishes the release automatically.

### CI Pipeline

Runs on every push to `main` and all PRs (`.github/workflows/ci.yml`):
- `cargo test` on Ubuntu and macOS
- `cargo clippy -- -D warnings` (warnings = errors)
- `cargo fmt --check`
- `cargo build --release`

### Release Pipeline

Triggers on `v*` tags (`.github/workflows/release.yml`):

| Artifact | Platform | Runner |
|----------|----------|--------|
| `impulse-rs-darwin-aarch64` | macOS Apple Silicon | macos-latest |
| `impulse-rs-darwin-x86_64` | macOS Intel | macos-13 |
| `impulse-rs-linux-x86_64` | Linux x86_64 | ubuntu-latest |
| `impulse-rs-linux-aarch64` | Linux ARM64 | ubuntu-latest (cross) |
| `Impulse-vX.Y.Z-macos-aarch64.dmg` | macOS Desktop App | macos-latest |

All artifacts include SHA256 checksums.

### Installing from Release

```bash
# macOS Apple Silicon
curl -L https://github.com/jamespustorino/IMPULSE-rs/releases/latest/download/impulse-rs-darwin-aarch64 -o impulse-rs
chmod +x impulse-rs
sudo mv impulse-rs /usr/local/bin/

# Linux x86_64
curl -L https://github.com/jamespustorino/IMPULSE-rs/releases/latest/download/impulse-rs-linux-x86_64 -o impulse-rs
chmod +x impulse-rs
sudo mv impulse-rs /usr/local/bin/
```

### Installing from Source

```bash
cd impulse-rs
cargo install --path .
```

---

_Last updated: 2026-03-09_
