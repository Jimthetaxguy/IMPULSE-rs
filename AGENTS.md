---
title: Agent Guidelines
description: Guidelines for AI coding agents working in this repository
version: '4.1'
authors:
  - name: Impulse Maintainers
    email: impulse-rs@users.noreply.github.com
    github: Jimthetaxguy/IMPULSE-rs
---

# AGENTS.md — Impulse

> Guidelines for AI coding agents contributing to this project.
> Product north star: [`VISION.md`](VISION.md)
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=control-plane foundations + governed runtime producers + accepted-run review candidates; Next=stronger same-user actor authorization + full launched Builder/Supervisor proof; Later=explicit memory promotion/dismissal + general roles + negotiated runtimes + multi-project routing; Legacy=egui compile-maintenance only.

---

## What This Project Is

Impulse is a terminal-native **local control plane and harness manager** for AI coding agents.

```
 Dioxus cockpit / TUI / CLI
              │
              ▼
 Impulse daemon + control-plane services
              │
       runtime adapters / PTYs
       ├── external CLI harnesses
       └── Ion native coding runtime
```

Memory is one first-class service, not the whole product. Impulse also owns process lifecycle, workbench truth, capability-checked tools, telemetry, messaging/handoffs, credentials, artifacts, policy, and verification. External runtimes retain their proprietary internal loops; Impulse governs the environment around them.

**Keep these identities separate:** a role defines obligations and permissions; a runtime is the execution engine; an agent instance is one running identity; a session is bounded work history; a task is an assignment and its completion criteria; a pane is only a UI/terminal viewport. Never infer a role from the model, executable, or pane position.

---

## Desktop Shell Status (as of 2026-07-13)

> **egui / impulse-gui is LEGACY.** It is frozen — no new features. It will be removed after the Dioxus desktop host reaches parity.

The chosen desktop stack is **Dioxus Desktop + xterm.js terminal bridge**. It is the cockpit, not the source of operational truth: authoritative workbench state and policy live in Rust daemon/control-plane contracts. Tauri-shaped code is retained only as a temporary compatibility adapter.

- See `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` for canonical layer boundaries
- See `docs/spec/DESKTOP-STACK-TRADEOFFS.md` for the full option evaluation
- See `docs/decisions/0008-dioxus-desktop-host.md` for the active ADR
- See `docs/decisions/0007-desktop-shell-stack.md` for the superseded Tauri-era ADR
- See `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md` for historical migration context; do not use it as the next product goal
- See `docs/plans/EGUI-DECOMMISSION.md` for the active, gated plan to fully remove the egui surface (frozen `impulse-gui` crate + `impulse-term` egui rendering layer) once the Dioxus host is operationally authoritative

**Do not add new code to `impulse-gui`.** If you touch `impulse-term`, keep new PTY/process behavior framework-neutral and do not expand its optional egui rendering surface.

---

## Collaborative Agentic Coding

Before mutating code or docs, read [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md) and follow its lane rules.

Minimum required operating state:

- inspect `git status --short`, current branch, and `git worktree list`
- identify owner, role, branch, worktree, owned paths, blocked/shared paths, and verification commands
- create or update a lane work card under `docs/plans/worktrees/<date>-<lane-slug>.md` for parallel or multi-session work
- treat `Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `CLAUDE.md`, docs indexes, validator files, and protocol/spec docs as shared files that require explicit ownership
- hand off before overlapping another lane's files

Parallel orchestrators are allowed only when lane ownership is explicit and documented.

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

**Execution surfaces:**
- **Direct** — stateless per-action (hooks). Read → process → write → exit.
- **Daemon/control plane** — long-running Unix socket authority for workbench snapshots, bounded managed-agent turns, supervisor actions, artifacts, and telemetry overlays.
- **Desktop shell** — Dioxus cockpit backed by daemon snapshots, Rust host commands, PTY lifecycle, and xterm.js events. It must not create a competing state store or policy authority.
- **ratatui TUI** — standalone terminal-native operator surface. Remains first-class throughout migration.
- **egui workbench** — LEGACY. Frozen. Compile-maintenance only.

**Live versus direction:**
- Live foundations include PTY lifecycle, daemon-owned workbench truth and telemetry, registry-driven desktop platform identity, Ion as a launchable platform, capability-checked tool registries, supervisor-specific permission policy, reviewable artifacts, and the profiled governed-task producer lifecycle. A profiled Builder launch records exact acceptance criteria and a daemon-attested clean Git `HEAD`; the daemon derives claims, runs fixed detached Rust verification, and requests strict API-only Supervisor review. Only an operator decision can accept a governed task; process exit and model recommendation never do.
- `rust_workspace_v1` runs host-trusted Rust code in a detached Git worktree with scrubbed environment, timeouts, output digests, and process-group cleanup. It is not an OS sandbox. Dioxus renders evidence and guides operators to `"$IMPULSE_CONTROL_CLI" --daemon governed-verify` / `"$IMPULSE_CONTROL_CLI" --daemon governed-review`; it does not yet expose producer buttons. The packaged executable is `impulse-rs`; `--daemon` is a required global flag before every governed producer subcommand. Persisted receipts plus the per-task lock cover replay/concurrency, not daemon crash between side effect and receipt; durable producer reservations remain future work.
- A general `RoleContract`, common runtime-adapter trait, and capability-negotiation contract are product direction, not implemented facts. Do not claim every runtime is structurally governed.

**Data in `.impulse/`:**

| File | Purpose | Persistence |
|------|---------|-------------|
| `HISTORY.jsonl` | Session log (append-only) | Committed |
| `GENOME.md` | Decisions & preferences | Committed |
| `LIVE_STATE.json` | Active session state | Ephemeral |
| `config.json` | Configuration | Committed |
| `GOVERNED_TASKS.json` | Governed task records + idempotency receipts | Durable local control-plane state |
| `DESKTOP_GOVERNED_LIFECYCLE_OUTBOX.json` | Ambiguous launch/exit mutations awaiting daemon reconciliation | Durable local recovery state |
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
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

---

## Contributing

### Verification Gate (Non-Negotiable)

All changes must pass before any commit:
```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

**Final-gate evidence:** Do not rely on a checked-in aggregate test count. Run the complete gate on
the current checkout and record passed, ignored, and failed totals in the commit/PR evidence.
Default tests must use tracked source/fixtures and remain portable across fresh clones, linked
worktrees, and CI.

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

| Module Category | Target | Last measured baseline (2026-04-01) | Why |
|---|---|---|---|
| **Core** (state, daemon, agent) | 3.0 tests/KLOC | ~1.5 | Data persistence, IPC safety |
| **Handlers** (CLI dispatch) | 2.0 tests/KLOC | ~0.8 (13/19 files untested) | User-facing entrypoints |
| **Tooling** (dynamic tools) | 2.0 tests/KLOC | ~17.1 | Capability enforcement, security |
| **UI/TUI** (terminal) | 1.0 tests/KLOC | ~0.4 | Layout/rendering correctness |
| **Integration** | Every stable CLI command | 26 tests | End-to-end verification |

New modules must ship meeting the target. Existing modules should trend toward targets. The
baseline above predates substantial handler and control-plane coverage added since April; recompute
it before presenting it as current. `docs/spec/TEST-TRACEABILITY.md` owns the current qualitative
gap assessment.

**Workspace totals:** Derive totals from the final `cargo test --workspace` run rather than a static
projection. The legacy `impulse-gui` crate is frozen and excluded from the canonical workspace gate;
verify it separately only when legacy compile-maintenance changes require it.

High-risk coverage priorities (despite substantial source-local unit coverage):
- End-to-end parity between `daemon_dispatch` and `direct_dispatch` for every stable command.
- Agent configuration/query failures across provider, credential, timeout, and daemon boundaries.
- Guard and confirmation behavior at real dispatch boundaries, including process-exit semantics.
- Injection/stewardship acceptance evidence across retrieval, review, persistence, and output.
- Shared handler-helper failure paths under concurrent and cancellation-heavy execution.

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
