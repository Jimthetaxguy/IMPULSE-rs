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
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=Rust core + EGUI workbench, Next=daemon-truth EGUI + hook validation, Later=agent control + artifact polish

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
- **EGUI workbench** — native operator console backed by daemon snapshots and published terminal telemetry.

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
| egui imports | `impulse-gui` uses `eframe::egui::*`, NEVER bare `egui::*` |

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
cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

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

| Module Category | Target | Why |
|---|---|---|
| **Core** (state, daemon, agent) | 3.0 tests/KLOC | Data persistence, IPC safety |
| **Handlers** (CLI dispatch) | 2.0 tests/KLOC | User-facing entrypoints |
| **Tooling** (dynamic tools) | 2.0 tests/KLOC | Capability enforcement, security |
| **UI/TUI** (terminal) | 1.0 tests/KLOC | Layout/rendering correctness |

New modules must ship meeting the target. Existing modules should trend toward targets.

High-risk modules (prioritize coverage):
- `src/state/` — persistence layer, corruption means data loss
- `src/handlers/daemon_dispatch.rs` — routes all IPC
- `src/handlers/direct_dispatch.rs` — routes all CLI commands

### Test Pattern Examples

```rust
// Happy path
#[test]
fn test_parse_config_valid_json_returns_config() {
    let json = r#"{"timeout_ms": 5000}"#;
    let config = parse_config(json).unwrap();
    assert_eq!(config.timeout_ms, 5000);
}

// Error path
#[test]
fn test_parse_config_invalid_json_returns_error() {
    assert!(parse_config("not json").is_err());
}

// Boundary condition
#[test]
fn test_parse_config_empty_string_returns_error() {
    assert!(parse_config("").is_err());
}

// Serde round-trip
#[test]
fn test_session_info_roundtrip() {
    let original = SessionInfo { id: "abc".into(), active: true };
    let json = serde_json::to_string(&original).unwrap();
    let recovered: SessionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(original, recovered);
}

// Error enum Display
#[test]
fn test_session_error_display_contains_id() {
    let err = SessionError::NotFound("abc".into());
    assert!(format!("{err}").contains("abc"));
}
```

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

**`unwrap()` / `expect()` rules:**
- `unwrap()` — only in: tests, `Default` impls (failure impossible), `main()` after arg parsing
- `expect("msg")` — only in: `main()` and test setup, never in library code
- Every `Result`-returning function needs at least one `is_err()` test

### Lint Suppression Rules

| Suppression | Rule |
|-------------|------|
| `#[allow(dead_code)]` | Must include `// dead_code: <reason>` comment. Grep for callers first — if truly dead, delete it. |
| `#[allow(clippy::too_many_arguments)]` | Temporary only — add `// TODO: refactor to struct params` |
| `#![allow(...)]` (file-level) | Not acceptable in new code |
| Any `#[allow(clippy::*)]` | Must include `// clippy: <reason>` comment |

**Auditing existing suppressions:**
```bash
# Find suppressions missing required comments
git grep -n "#\[allow" -- "*.rs" | grep -v "// dead_code:\|// TODO:\|// clippy:\|// serde"

# Before adding #[allow(dead_code)], prove no callers exist
git grep -w "function_name" -- "*.rs"
# If zero callers → delete the code, don't allow it
```

**Judging clippy false positives:**
- Run `cargo clippy --fix --allow-staged` to see auto-fixes
- If the fix breaks intent, document why in `// clippy: <reason>`
- If clippy is right, fix the code instead of allowing

### Serde Round-Trip Requirements

Every `#[derive(Serialize, Deserialize)]` type needs a round-trip test:

```rust
#[test]
fn round_trip_my_type() {
    let original = MyType::default();
    let json = serde_json::to_string(&original).unwrap();
    let recovered: MyType = serde_json::from_str(&json).unwrap();
    assert_eq!(original, recovered);
}
```

**Special cases:**
- `#[serde(skip)]` fields: exclude from equality check (not serialized)
- `#[serde(default)]` fields: include in test, verify the default is sensible
- `#[serde(flatten)]` fields: test that flat JSON still deserializes (catches restructure breakage)
- Multiple formats: test each format separately if type serializes to both JSON and TOML

**Why:** Catches field renames, missing defaults, and `#[serde(flatten)]` breakage that would silently corrupt persisted data.

### Property-Based Testing

Use `proptest` for functions with combinatorial input spaces. Add `proptest` to `[dev-dependencies]` when first used.

```rust
use proptest::proptest;

// Path sanitization must never produce traversal
proptest! {
    #[test]
    fn test_sanitize_path_never_contains_traversal(path in "[a-zA-Z0-9/_.-]+") {
        let result = sanitize_path(&path).unwrap();
        prop_assert!(!result.contains(".."));
    }
}

// Config roundtrip with random data
proptest! {
    #[test]
    fn test_config_roundtrip_random(
        sessions in prop::collection::vec("[a-z]+", 0..10),
        max_age in 1u64..1000,
    ) {
        let config = Config { sessions, max_age };
        let json = serde_json::to_string(&config).unwrap();
        let recovered: Config = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(config, recovered);
    }
}
```

**When to use proptest:** path validation, numeric arithmetic, serialization, config parsing — any function where behavior should hold for ANY valid input.

### Unsafe Code Rules

Any `unsafe` block requires all three:
1. `// SAFETY:` comment documenting every invariant the block relies on
2. Precondition validation **before** the unsafe block (never inside)
3. A dedicated test exercising the unsafe code path

```rust
fn read_cstring(ptr: *const u8) -> Result<String> {
    // Precondition: validate before entering unsafe
    ensure!(!ptr.is_null(), "pointer must not be NULL");

    // SAFETY: ptr is non-null (validated above) and points to
    // a valid NUL-terminated C string per this function's contract.
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr as *const c_char) };
    Ok(cstr.to_str()?.to_owned())
}

#[test]
fn test_read_cstring_null_returns_error() {
    assert!(read_cstring(std::ptr::null()).is_err());
}

#[test]
fn test_read_cstring_valid_pointer_returns_string() {
    let data = b"hello\0";
    assert_eq!(read_cstring(data.as_ptr()).unwrap(), "hello");
}
```

**When unsafe is acceptable:** FFI calls, verified pointer arithmetic, layout assumptions.
**Never for:** convenience, avoiding `Result`, error handling shortcuts.

### Test Helper Centralization

| Helper Type | Location | Purpose |
|---|---|---|
| State factories | `#[cfg(test)]` in owning module | `test_state() -> (TempDir, Arc<State>)` |
| Mock tools | `src/tooling/` test module | `EchoTool`, `WriteTool`, `CapturingTool` |
| Daemon guards | `src/integration_tests.rs` | `DaemonGuard` RAII cleanup |
| Assertion helpers | Near first usage | `assert_error_contains()` |

**Rule:** If a helper is used by 3+ modules, extract to a shared `#[cfg(test)]` module. Don't duplicate factory functions across files.

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
