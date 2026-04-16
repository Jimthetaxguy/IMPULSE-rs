---
title: Agent Guidelines
description: Guidelines for AI coding agents working in this repository
version: '4.0'
authors:
  - name: James Pustorino
    email: James.s.Pustorino@gmail.com
    github: jamespustorino
---

# AGENTS.md — Impulse

> Guidelines for AI coding agents contributing to this project.
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=Rust core + Tauri desktop shell (Phase 0 docs reset), Next=egui boundary cleanup + static shell, Later=live terminal bridge + daemon parity

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

## Desktop Shell Status (as of 2026-04-15)

> **egui / impulse-gui is LEGACY.** It is frozen — no new features. It will be removed after Tauri shell reaches parity.

The chosen desktop stack is **Tauri 2.x + Dioxus + xterm.js terminal bridge**.

- See `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` for canonical layer boundaries
- See `docs/spec/DESKTOP-STACK-TRADEOFFS.md` for the full option evaluation
- See `docs/decisions/0007-desktop-shell-stack.md` for the ADR
- See `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md` for the build sequence

**Do not add new code to `impulse-gui`.** If you need to touch `impulse-term`, confirm that `eframe` is not re-introduced as a dependency.

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
- **Desktop shell** — Tauri + Dioxus webview backed by daemon snapshots and terminal bridge events. *(in migration)*
- **ratatui TUI** — standalone terminal-native operator surface. Remains first-class throughout migration.
- **egui workbench** — LEGACY. Frozen. Compile-maintenance only.

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
| egui imports | `impulse-gui` uses `eframe::egui::*`, NEVER bare `egui::*` — **legacy only** |

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

### Verification Gate (Non-Negotiable)

All changes must pass before any commit:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

**Expected (2026-04-01):** 1,025 tests passed, 0 failed, 3 ignored. Update counts in this file, CLAUDE.md, and RUST-CANONICAL-CONTRACT.md when they change.

### Code Requirements

1. New modules need unit tests in a `mod tests` block — not just one happy-path test
2. New CLI commands go in `src/main.rs` with clap derive
3. New dynamic tools implement the `DynamicTool` trait in `src/tooling/`
4. File operations must use atomic writes (temp + rename)
5. Error handling must use `Result<T>` — never `unwrap()` on user-facing paths

### Test Quality Requirements

New code must include tests for:

| What | How |
|------|-----|
| Happy path | At least one test proving the function works correctly |
| Error path | At least one test per `Result`-returning function exercising `Err` |
| Boundary conditions | Empty inputs, zero values, max values where applicable |
| Serde types | Round-trip test: `deserialize(serialize(val)) == val` |
| Error enums | `Display` output test: `assert!(format!("{e}").contains("expected"))` |

Tests must assert behavior — `println!` without assertions is not a test.

### Test Naming Convention

Use descriptive names: `test_<function>_<scenario>_<expected_result>`

```rust
// Good
#[test] fn test_parse_config_empty_input_returns_default() { ... }
#[test] fn test_guard_evaluate_blocked_action_returns_exit_1() { ... }

// Bad
#[test] fn test_parse() { ... }
#[test] fn test_guard_2() { ... }
```

### Test Density Targets

| Module Category | Target | Current (2026-04-01) | Why |
|---|---|---|---|
| **Core** (state, daemon, agent) | 3.0 tests/KLOC | ~1.5 | Data persistence, IPC safety |
| **Handlers** (CLI dispatch) | 2.0 tests/KLOC | ~0.8 (13/19 files untested) | User-facing entrypoints |
| **Tooling** (dynamic tools) | 2.0 tests/KLOC | ~17.1 | Capability enforcement, security |
| **UI/TUI** (terminal) | 1.0 tests/KLOC | ~0.4 | Layout/rendering correctness |
| **Integration** | Every stable CLI command | 26 tests | End-to-end verification |

New modules must ship meeting the target. Existing modules should trend toward targets.

**Workspace totals (2026-04-01):** 1,025 tests across 4 crates (impulse-rs: 999+26, ops: 4, term: 90, gui: 220).

High-risk untested modules (prioritize coverage):
- `src/handlers/daemon_dispatch.rs` (450 LOC) — routes all IPC, zero tests
- `src/handlers/direct_dispatch.rs` (465 LOC) — routes all CLI commands, zero tests
- `src/handlers/agent.rs` (145 LOC) — agent configuration and query, zero tests
- `src/handlers/guard.rs` (204 LOC) — action guardrails with `process::exit`, zero tests
- `src/handlers/injection_handlers.rs` (209 LOC) — context injection routing, zero tests
- `src/handlers/common.rs` (379 LOC) — shared helpers used by all handlers, zero tests

### Error Handling Patterns

**`thiserror` for typed errors:**
```rust
#[derive(Error, Debug)]
enum SessionError {
    #[error("No session with ID {id}")]
    NotFound { id: String },
    #[error("Failed to write state: {source}")]
    StateWrite { #[from] source: std::io::Error },
}
```

**`anyhow` with `.context()` chains (never bare `?` on I/O):**
```rust
let content = fs::read_to_string(&path)
    .context("Failed to read config file")?;
let config: Config = serde_json::from_str(&content)
    .context("Failed to parse config JSON")?;
```

### Lint Suppression Rules

| Suppression | Rule |
|-------------|------|
| `#[allow(dead_code)]` | Must include `// dead_code: <reason>` comment. Grep for callers first — if truly dead, delete it. |
| `#[allow(clippy::too_many_arguments)]` | Temporary only — add `// TODO: refactor to struct params` |
| `#![allow(...)]` (file-level) | Not acceptable in new code |
| Any `#[allow(clippy::*)]` | Must include `// clippy: <reason>` comment |

### Unsafe Code Rules

Any `unsafe` block requires all three:
1. `// SAFETY:` comment documenting every invariant the block relies on
2. Precondition validation **before** the unsafe block (never inside)
3. A dedicated test exercising the unsafe code path

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
