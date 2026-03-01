---
title: Agent Guidelines
description: Guidelines for AI coding agents working in this repository
version: '3.0'
authors:
  - name: James Pustorino
    email: James.s.Pustorino@gmail.com
    github: jamespustorino
---

# AGENTS.md — Impulse

> Guidelines for AI coding agents contributing to this project.

---

## What This Project Is

Impulse is a **sidecar memory layer** for AI coding agents. It is NOT a coding agent itself.

```
 Coding Agent (Claude Code, Codex, OpenCode)
       │
       │ hooks auto-track files + tools
       ▼
 Impulse (persists memory across sessions)
```

**The distinction matters:** Impulse doesn't write code. It remembers what the coding agent did — sessions, file changes, decisions, tool usage — and makes that context available in future sessions.

---

## Core Principles

### Never Panic
Every function returns `Result<T>`. No `unwrap()` on production paths. Use `thiserror` for typed errors, `anyhow` for application-level errors.

### Atomic Writes
All file I/O uses temp file + rename with unique temp names (PID + timestamp). Never write directly to target paths.

### Validate at Boundaries
Sanitize user-supplied IDs before using as filesystem paths or SQL parameters. Validate protocol data on socket boundaries. Allowlist table names.

### Review Before Apply
Context injection defaults to review mode. Show what would be injected; let the user decide. No silent auto-injection.

### Capability-Based Access
Dynamic tools use deny-by-default. The registry enforces: exists → capability check → param validation → execute.

### Simplicity First
Choose the simplest solution that works. Prefer editing existing files over creating new ones. Don't add abstractions for one-time operations.

---

## Architecture

**Dual mode:**
- **Direct** — stateless per-action (hooks). Read → process → write → exit.
- **Daemon** — long-running with Unix socket IPC (TUI/chat). In-memory state with dirty-flag sync.

**Data in `.impulse/`:**

| File | Purpose | Persistence |
|------|---------|-------------|
| `HISTORY.jsonl` | Session log (append-only) | Committed |
| `GENOME.md` | Decisions & preferences | Committed |
| `LIVE_STATE.json` | Active session state | Ephemeral |
| `config.json` | Configuration | Committed |
| `retrieval.db` | Search index | Rebuildable |

---

## Code Conventions

| Area | Convention |
|------|-----------|
| Errors | `thiserror` enums, `anyhow` app errors, `Result<T>` everywhere |
| File I/O | Atomic writes (temp + rename), unique temp names |
| State | `RwLock` + dirty flag + sync on Drop |
| Naming | `PascalCase` types, `snake_case` functions, `SCREAMING_SNAKE` constants |
| Tests | Unit tests in `mod tests`, integration tests use `DaemonGuard` RAII |
| Features | `office-support`, `monty-support`, `datafusion-support` (all opt-in) |

---

## Build & Test

```bash
cd impulse-rs
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

---

## Contributing

1. All changes must pass `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`
2. New modules need unit tests in a `mod tests` block
3. New CLI commands go in `src/main.rs` with clap derive
4. New dynamic tools implement the `DynamicTool` trait in `src/tooling/`
5. File operations must use atomic writes
6. Error handling must use `Result<T>` — never `unwrap()` on user-facing paths

---

## Worktree Safety

This project uses git worktrees for parallel development. A pre-commit hook warns about:

- Force pushes (requires confirmation)
- Mass deletions (10+ files, requires confirmation)
- Unpushed commits in worktrees
- Uncommitted changes (50+ files)

### Before Any Mass Removal

**STOP and verify first:**

1. Check what's being deleted: `git diff --cached --name-status`
2. Check if files exist elsewhere (worktrees, branches)
3. Create a backup branch: `git branch backup-pre-delete`
4. If uncertain, restore first: `git restore <path>` instead of deleting

**Never run these without explicit user confirmation:**
- `rm -rf` on project directories
- `git clean -fd`
- `git reset --hard`
- Force push to main/master

**Before force pushing, always:**
```bash
git branch backup-pre-force  # Create backup
```
