# CLAUDE.md — Impulse

> Persistent memory for AI coding agents.

---

## What Impulse Is

Impulse is a **sidecar**, not a coding agent. It runs alongside AI coding agents (Claude Code, Codex, OpenCode) and remembers what they did across sessions.

```
 Coding Agent (does the work)
       │
       │ hooks
       ▼
 Impulse (remembers what was done)
```

The coding agent writes code. Impulse records which files changed, which tools were used, what decisions were made — and makes that context available next session.

---

## Principles

### 1. Never Panic, Always Return Result

Every function returns `Result<T>`. No `unwrap()` on production paths. Use `thiserror` for error enums, `anyhow` for application errors.

### 2. Atomic Writes

All file operations use temp file + rename. Temp file names include PID + timestamp to avoid collisions. Never write directly to the target path.

### 3. Input Validation at Boundaries

Sanitize user-supplied IDs before using as filesystem paths or SQL components. Validate protocol data on socket boundaries. Allowlist table names for PRAGMA queries.

### 4. Dirty Flag State Management

In-memory state tracks whether it's been modified. Sync to disk only when dirty. Always persist on Drop/exit.

### 5. Capability-Based Tool Access

Dynamic tools use deny-by-default capabilities. The registry enforces: exists → capability check → param validation → execute.

### 6. Review Before Apply

Context injection defaults to review mode — surface what *would* be injected and let the user decide. Never auto-inject without consent.

### 7. Build Optimal, Not Just Build

Before implementing, consider alternative approaches. Choose the simplest solution that works. Avoid over-engineering.

---

## Architecture

**Workspace (3 crates):**
- `impulse-rs/` — main CLI + daemon + TUI (30 modules, ~41K lines)
- `impulse-rs/impulse-term/` — custom terminal widget (PTY + vt100 + context bridge, ~2K lines, 39 tests)
- `impulse-rs/impulse-gui/` — egui native workbench (4 views + sidebar + status bar)

**Dual mode:**
- **Direct mode** — stateless, per-action (for hooks). Read → process → write → exit.
- **Daemon mode** — long-running, Unix socket IPC (for TUI/GUI). In-memory state with periodic sync.
- **GUI mode** — `impulse-gui` binary, connects to daemon via IPC, hosts terminal panes with context lifecycle.

**Data lives in `.impulse/`:**
- `HISTORY.jsonl` — append-only session log (committed)
- `GENOME.md` — permanent decisions and preferences (committed)
- `LIVE_STATE.json` — active session state (ephemeral)
- `config.json` — runtime configuration
- `retrieval.db` — search index (rebuildable)

---

## Code Style

| Convention | Rule |
|------------|------|
| Error handling | `thiserror` enums + `anyhow` application errors |
| File I/O | Atomic (temp + rename), unique temp names |
| State | `RwLock` + dirty flag + sync on Drop |
| Naming | `PascalCase` structs, `snake_case` functions, `SCREAMING_SNAKE` constants |
| Testing | Unit tests in `mod tests` per file, integration tests with `DaemonGuard` RAII |
| Feature flags | `office-support`, `monty-support`, `datafusion-support` (all opt-in) |

---

## Build & Test

```bash
cd impulse-rs

# Full workspace
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check

# Individual crates
cd impulse-term && cargo build && cargo test && cargo clippy -- -D warnings
cd impulse-gui && cargo build && cargo clippy -- -D warnings
```

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `IMPULSE_SESSION_ID` | Current session ID |
| `IMPULSE_HOME` | Custom `.impulse/` directory |
| `IMPULSE_SOCKET_PATH` | Custom Unix socket path |
| `ANTHROPIC_API_KEY` | For daemon chat |
| `IMPULSE_MODEL` | Chat model override |
