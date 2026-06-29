# CLAUDE.md — Impulse

> Persistent memory for AI coding agents.
> Contract: [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
> Collaboration playbook: [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md)
> Canonical stack: Rust (impulse-rs)
> Roadmap contract: Now=Rust core + Dioxus desktop host; Dioxus Desktop launch scaffold + terminal bridge parity=Complete (see docs/ROADMAP-PLAN.md); Next=Daemon parity in desktop shell + agent control + artifact polish; Legacy=egui compile-maintenance only; Tauri=legacy compatibility adapter only

---

## What Impulse Is

Impulse is a **sidecar**, not a coding agent. It runs alongside primary AI coding agents (Claude Code and Codex) and remembers what they did across sessions. Legacy OpenCode compatibility remains for older Impulse surfaces, but OpenCode is not a peer active platform.

```
 Coding Agent (does the work)
       │
       │ hooks
       ▼
 Impulse (remembers what was done)
```

The coding agent writes code. Impulse records which files changed, which tools were used, what decisions were made — and makes that context available next session.

---

## Collaborative Agentic Coding

Read [`docs/guides/COLLABORATIVE-AGENTIC-CODING.md`](docs/guides/COLLABORATIVE-AGENTIC-CODING.md) before mutating the repository.

Required operating facts for any non-trivial lane:

- owner, role, branch, and worktree path
- owned files/directories and blocked/shared paths
- plan/spec link and acceptance criteria
- verification commands
- lane work card under `docs/plans/worktrees/<date>-<lane-slug>.md`

Multiple orchestrators may run in parallel only when their lanes have disjoint ownership or an explicit handoff/integration lane. Do not infer ownership from silence.

---

## Principles

### 1. Never Panic, Always Return Result

Every function returns `Result<T>`. No `unwrap()` on production paths. Use `thiserror` for error enums, `anyhow` for application errors.

**Error handling rules:**
- `thiserror` enums: every variant must have a `#[error("...")]` with meaningful context. Test `Display` output in `mod tests`.
- `anyhow` usage: always chain `.context("what we were doing")` — never bare `?` on I/O or parse operations.
- `unwrap()` is only acceptable in: tests, `Default` impls where failure is impossible, and `main()` after argument parsing.
- `expect("msg")` is acceptable in `main()` and test setup — never in library code.
- Every `Result`-returning function must have at least one test exercising the `Err` path.

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

**Workspace (Rust-first, desktop shell in migration):**
- `impulse-rs/` — main CLI + daemon + ratatui TUI
- `impulse-rs/impulse-ops/` — operations library (shared types: SupervisorAction, TerminalOpsReport, OpsSnapshot, WorkbenchDaemonRequest/Response, DAEMON_PROTOCOL_VERSION, 31 tests)
- `impulse-rs/impulse-term/` — PTY/session/context core (PTY + vt100 + WriteQueue + context bridge)
- `impulse-rs/impulse-desktop/` — Dioxus desktop shell scaffold and typed host bridge contracts

**Legacy:** `impulse-gui` / egui is frozen. It receives compile-maintenance only until the Dioxus desktop host reaches parity. Tauri-shaped code is also compatibility-only, not a new product scaffold target.

**Execution surfaces:**
- **Direct mode** — stateless, per-action (for hooks). Read → process → write → exit.
- **Daemon mode** — long-running, Unix socket IPC (for TUI and future desktop shell). In-memory state with periodic sync.
- **Desktop mode** (in migration) — Dioxus Desktop host with xterm.js terminal bridge, backed by Rust daemon/runtime state. Tauri-shaped command/event code is compatibility-only.

**IPC Protocol (PROTOCOL_VERSION = 2):**

The daemon exposes a JSON-line Unix socket protocol. Key endpoint groups:

| Group | Endpoints | Purpose |
|-------|-----------|---------|
| Agent Coordination | `AgentAssist` | AI coordination with context enrichment via extracted insights |
| Agent Specialized | `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` | Per-task agent assistance |
| Delegation | `RegisterDelegation`, `CompleteDelegation`, `ListDelegations` | Phase 1B cross-agent delegation tracking |
| Conflict Resolution | `GetConflictHistory`, `ClearResolvedConflicts` | File conflict tracking and resolution |
| Agent Pool | `GetAgentPool` | All sessions grouped by role (Phase 2B) |

Responses use `AgentAssistResult` (with `recommendations` + `pane_summaries`) or `AgentSpecializedResult` (for review/analyze/summarize). Full protocol spec: [`docs/IPC-PROTOCOL.md`](docs/IPC-PROTOCOL.md).

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
| Feature flags | `office-support` (default), `monty-support`, `datafusion-support` (opt-in) |

---

## Testing Standards

### Test Quality Bar

Every test must assert observable behavior — not just "doesn't panic." Tests that only `println!` output without assertions are not acceptable. Every `#[test]` function must contain at least one `assert!`, `assert_eq!`, `assert_ne!`, or `assert!(result.is_err())`.

### Required Test Patterns

| Pattern | When Required | Example |
|---------|--------------|---------|
| **Happy path** | Every public function | `assert_eq!(parse("valid"), Ok(expected))` |
| **Error cases** | Every function returning `Result<T>` | `assert!(parse("").is_err())` |
| **Boundary conditions** | Numeric inputs, collections, strings | Empty vec, zero, max value, empty string |
| **Serde round-trip** | Every type with `Serialize + Deserialize` | `assert_eq!(from_json(to_json(&val)), val)` |
| **Display/From impls** | Every `thiserror` enum | `assert!(format!("{}", err).contains("expected text"))` |

### Serde Round-Trip Requirement

All types deriving `Serialize` and `Deserialize` must have a round-trip test proving `deserialize(serialize(value)) == value`. This catches field renames, missing defaults, and `#[serde(flatten)]` breakage. Pattern:

```rust
#[test]
fn round_trip_my_type() {
    let original = MyType::default();
    let json = serde_json::to_string(&original).unwrap();
    let recovered: MyType = serde_json::from_str(&json).unwrap();
    assert_eq!(original, recovered);
}
```

### Unsafe Code Policy

All `unsafe` blocks must have:
1. A `// SAFETY:` comment documenting every invariant the block relies on
2. Precondition validation before the unsafe call (never inside the block)
3. A dedicated test that exercises the unsafe path (not just the precondition checks)

### `#[allow(...)]` Policy

Lint suppressions must be justified. Rules:

| Suppression | Acceptable When | Must Include |
|-------------|----------------|--------------|
| `#[allow(dead_code)]` | Serde deserialization fields, Phase-gated features | `// dead_code: <reason>` comment |
| `#[allow(clippy::too_many_arguments)]` | Temporary — track in a cleanup issue | `// TODO: refactor to struct params` comment |
| `#[allow(clippy::*)]` (other) | False positive or intentional design | `// clippy: <reason>` comment |
| `#![allow(...)]` (file-level) | Never acceptable in new code | Must be broken into per-item allows |

New `#[allow(dead_code)]` requires proof: grep the codebase for callers first. If truly dead, delete it instead of allowing it.

### Property-Based Testing

Use `proptest` for functions with combinatorial input spaces. Add `proptest` as a `[dev-dependencies]` entry when first used.

**When to use:** any function where behavior should hold for ANY valid input, not just specific test cases.

```rust
use proptest::proptest;

// Path sanitization: never produces traversal sequences
proptest! {
    #[test]
    fn test_sanitize_path_never_contains_traversal(path in "[a-zA-Z0-9/_.-]+") {
        let result = sanitize_path(&path).unwrap();
        prop_assert!(!result.contains(".."));
        prop_assert!(!result.contains("//"));
    }
}

// Config round-trip with random data
proptest! {
    #[test]
    fn test_config_roundtrip_random(
        sessions in prop::collection::vec("[a-z]+", 0..10),
        max_age in 1u64..1000,
    ) {
        let config = Config { sessions, max_age };
        let json = serde_json::to_string(&config)?;
        let recovered: Config = serde_json::from_str(&json)?;
        prop_assert_eq!(config, recovered);
    }
}
```

**Strategy reference:**
- `any::<u64>()` — any u64 value
- `"[a-zA-Z0-9]+"` — regex string strategy
- `prop::collection::vec(any::<String>(), 0..100)` — vector of random strings
- `(any::<u32>(), "[a-z]+")` — tuple combining strategies

### Test Helpers

Centralize shared test utilities. Do not duplicate factory functions across modules.

| Helper Type | Location | Purpose |
|-------------|----------|---------|
| State factories | `#[cfg(test)]` in owning module | `test_state() -> (TempDir, Arc<State>)` |
| Mock tools | `src/tooling/` test module | `EchoTool`, `WriteTool` |
| Daemon guards | `src/integration_tests.rs` | `DaemonGuard` RAII cleanup |
| Assertion helpers | Near usage site | `assert_error_contains()` |

When a helper is used by 3+ modules, extract to a shared `#[cfg(test)]` module.

### Test Naming Convention

Use descriptive names: `test_<function>_<scenario>_<expected_result>`

```rust
// Good
#[test] fn test_parse_config_empty_input_returns_default() { ... }
#[test] fn test_guard_evaluate_blocked_action_returns_exit_1() { ... }
#[test] fn test_agent_error_display_includes_provider_name() { ... }

// Bad
#[test] fn test_parse() { ... }
#[test] fn test_guard_2() { ... }
```

### Test Density Targets

| Module Type | Target | Current (as of 2026-06-14) |
|-------------|--------|---------|
| Core (state, daemon, agent) | 3.0 tests/KLOC | ~1.5 (state ~80 tests, agent harness +24, daemon protocol +2) |
| Handlers | 2.0 tests/KLOC | ~32 tests/KLOC (362 tests across 12/19 files, 11,183 LOC) — target exceeded |
| Tooling | 2.0 tests/KLOC | ~17.1 (84 tests, 4,920 LOC) |
| UI/TUI | 1.0 tests/KLOC | ~0.4 |

**Why tooling is well-tested (17.1/KLOC):** Dynamic tools execute arbitrary user commands. Failure → data corruption or security breach. High density catches parameter injection, output parsing bugs, and rollback failures.

**Why core is low (1.2/KLOC):** Core modules are critical but large. Trend toward 3.0/KLOC by adding: session lifecycle corner cases (rapid start/end, duplicate IDs), daemon reconnection/recovery (socket errors), agent harness error cases (missing context, malformed JSON).

**Why handlers now exceed target (~32/KLOC):** The dispatch routers and shared helpers are heavily covered — `direct_dispatch.rs` (117 tests), `common.rs` (84), `daemon_dispatch.rs` (69), `injection_handlers.rs` (18), `guard.rs` (17), `agent.rs` (16), `session.rs` (12), `config.rs` (12), `memory.rs` (7), `describe.rs` (4), `mod.rs` (4), `system.rs` (2). The remaining **7 zero-test files are all thin CLI print-wrappers** that delegate to already-tested modules (`build_hygiene`, `semantic_diff`, `tooling`, etc.): `build.rs`, `office.rs`, `plugin_handlers.rs`, `retrieval.rs`, `semantic_diff_handlers.rs`, `stewardship_handlers.rs`, `tooling_handlers.rs`. Adding "does not panic" tests to these would be the println-only anti-pattern called out above — prefer testing the underlying modules, or extract any non-trivial decision logic out of the handler before testing it.
| Integration | Covers CLI commands + daemon IPC | 26 tests (4 files under `tests/`) |

New modules must ship with tests meeting the target density. Existing modules should trend toward targets during regular development.

### Coverage Priority (Highest Risk, Lowest Coverage)

| Module | Risk | Why |
|--------|------|-----|
| `src/state/` | HIGH | Persistence layer — corruption means data loss. Well-tested (~80 tests covering conflict detection, audit trail, config corruption, session lifecycle, config keys). |
| `src/handlers/` | MEDIUM | User-facing CLI paths — 12 of 19 files tested (362 tests, ~32/KLOC). Remaining 7 zero-test files are thin print-wrappers; test their underlying modules instead. |
| `src/error.rs` | LOW | All 8 `AgentError` variants have Display tests. |
| `src/ui/` | MEDIUM | TUI rendering — complex layout logic, limited coverage. |

### Codebase Examples

**Good: Error Display test** (exists in `src/error.rs:AgentError`):
```rust
#[test]
fn test_agent_error_missing_api_key_display() {
    let err = AgentError::MissingApiKey { provider: "Anthropic".into() };
    assert!(format!("{err}").contains("Anthropic"));
    assert!(format!("{err}").contains("No API key"));
}
```

**Good: Serde round-trip** (exists in `src/build_hygiene/tests.rs`):
```rust
#[test]
fn test_config_round_trip() {
    let config = BuildHygieneConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let recovered: BuildHygieneConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(recovered.enabled, config.enabled);
}
```

**Good: `.context()` chains** (from `src/client/mod.rs`):
```rust
serde_json::to_string(&request).context("Failed to serialize daemon request")?;
```

**Bad: println-only test** (exists in codebase — do not replicate):
```rust
#[test]
fn test_system_info() {
    let info = SystemInfo::collect();
    println!("System info: {:?}", info);  // No assertions — not a real test
}
```

### Error Handling Patterns

**Use `.context()` on all I/O and parse operations:**
```rust
// Good — context says what we were doing
let content = fs::read_to_string(&path)
    .context("Failed to read config file")?;
let config: Config = serde_json::from_str(&content)
    .context("Failed to parse config JSON")?;

// Bad — bare ? gives unhelpful "No such file or directory"
let content = fs::read_to_string(&path)?;
```

**Use `bail!`/`ensure!` for precondition checks:**
```rust
use anyhow::{bail, ensure};

ensure!(!id.is_empty(), "Session ID must not be empty");
if id.contains("..") {
    bail!("Session ID must not contain path traversal: {id}");
}
```

**Audit checklist for error handling compliance:**
```bash
# Find bare ? on I/O operations (should have .context())
cargo clippy 2>&1 | grep -i "unwrap\|expect"
# Find bare fs:: calls without .context()
git grep -n "fs::read\|fs::write\|fs::remove" -- "*.rs" | grep -v "context\|test"
# Find unwrap() outside tests and main
git grep -n "\.unwrap()" -- "*.rs" | grep -v "#\[test\]\|mod tests\|fn main\|impl Default"
```

---

## Build & Test

### Verification Gate

Run before every commit (copy-paste ready):
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

**Expected output (update when counts change):**
- `cargo test --workspace`: 1,381 unit + 26 integration (impulse-rs) + 31 (impulse-ops) + 114 (impulse-term) + 159 (impulse-desktop) = 1,711 passed, 5 ignored, 0 failed (verified 2026-06-29; +10 EmbeddingProvider/Ollama tests. Note: `impulse-term::test_with_parser_reads_screen_size` is parallelism-flaky under full `--workspace` load — passes in isolation/serial)
- `cargo clippy`: 0 warnings
- `cargo fmt --check`: no output (clean)

**Quick health check** (for mid-session verification):
```bash
# Note: impulse-rs is a binary crate (no lib target), so use --bins not --lib.
cd impulse-rs && cargo check && cargo test --bins -- --quiet 2>&1 | tail -5
```

### Full Workspace

```bash
cd impulse-rs

cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check

# Individual crates
cd impulse-term && cargo build && cargo test && cargo clippy -- -D warnings
cd impulse-ops && cargo build && cargo test && cargo clippy -- -D warnings
```

### Test Count Verification

To verify test counts match expectations:
```bash
cd impulse-rs && cargo test 2>&1 | grep "test result:" | awk '{sum += $4} END {print "Total: " sum " passed"}'
```
Expected: 1,711 passed across the 4 crates (impulse-rs, impulse-ops, impulse-term, impulse-desktop). If this changes, update both this section and the Architecture section.

### Pre-Commit Checklist

1. `cargo build` — zero warnings
2. `cargo test` — all tests pass (1,711 workspace total expected: 1381+26 impulse-rs, 31 ops, 114 term, 159 desktop; verify with `cargo test --workspace 2>&1 | grep "test result:"`)
   - **If count changes**: update this line and the Architecture section above
3. `cargo clippy -- -D warnings` — zero warnings
4. `cargo fmt --check` — zero diffs
5. No new `#[allow(...)]` without justification comment
6. New `Serialize + Deserialize` types have round-trip tests
7. New `Result`-returning functions have `Err` path tests

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `IMPULSE_SESSION_ID` | Current session ID |
| `IMPULSE_HOME` | Custom `.impulse/` directory |
| `IMPULSE_SOCKET_PATH` | Custom Unix socket path |
| `ANTHROPIC_API_KEY` | For daemon chat |
| `IMPULSE_MODEL` | Chat model override |
