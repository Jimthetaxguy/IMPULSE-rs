# Cleanup Spec — 2026-04-04

> Codebase audit findings and remediation plan for Impulse workspace.

## Health Baseline

| Check | Status |
|-------|--------|
| `cargo clippy -- -D warnings` | PASS — 0 warnings |
| `cargo fmt --check` | PASS — 0 diffs |
| `cargo test` | PASS — 1,344 passed, 4 ignored, 0 failed |
| Production `unwrap()` | ZERO instances |
| `#[allow(dead_code)]` justifications | All 9 have comments |

## Phase 1: Policy Compliance Quick Wins

**Status: IN PROGRESS**

### 1.1 Fix Stale Counts

| Location | Field | Was | Now |
|----------|-------|-----|-----|
| `CLAUDE.md` line 68 | test count | 1,317 unit + 26 integration = 1,343 | 1,318 unit + 26 integration = 1,344 |
| `CLAUDE.md` line 343 | expected total | 1,343 passed | 1,344 passed |
| `CLAUDE.md` line 373-378 | pre-commit checklist | 1,343 | 1,344 |
| `CLAUDE.md` line 68 | LOC | 64,068 | 64,066 |
| `impulse-rs/README.md` | test count | 1,343 | 1,344 |

### 1.2 Rename ToolResult Collision

Two unrelated types share the name `ToolResult`:

- `tooling/traits.rs:155` — tool execution result (output, artifacts, metadata) — 60+ usages, KEEP
- `stewardship/types.rs:169` — transcript parsing (tool_use_id, content_chars) — 4 usages, RENAME

**Action:** Rename `stewardship::types::ToolResult` to `ParsedToolResult`.

Files to change:
- `src/stewardship/types.rs:169` — struct definition
- `src/stewardship/types.rs:180` — field type in `TranscriptMessage`
- `src/stewardship/analyzer.rs:103` — return type
- `src/stewardship/analyzer.rs:152` — constructor call

### 1.3 Dead Code Annotations

**No action needed.** All 9 instances already have `// dead_code:` justification comments:
- `error.rs:20,31,36` — Phase 2 IPC endpoint variants
- `anthropic.rs:85,95,234,348` — Serde deserialization fields
- `docs/fetch.rs:32` — OpenAI API schema matching
- `ops_workbench.rs:29` — Serde deserialization fields

## Phase 2: Error Context Coverage

**Target:** Add `.context("descriptive message")` to bare `?` operators on I/O and parse operations.

CLAUDE.md policy: "always chain `.context("what we were doing")` — never bare `?` on I/O or parse operations."

Current coverage: 164 of 989 error propagations (16.6%).

### Priority Files

| File | Bare `?` | Module | Why Priority |
|------|----------|--------|-------------|
| `retrieval/store.rs` | 122 | SQLite layer | Most opaque errors — "database disk image is malformed" |
| `handlers/direct_dispatch.rs` | 69 | CLI routing | User-facing — errors surface to terminal |
| `retrieval/indexer.rs` | 64 | Search indexing | I/O heavy — file reads, JSON parsing |
| `state/persistence.rs` | ~20 | Persistence | + 13 identical `map_err` lock error patterns |

### Context Message Convention

```rust
// Pattern: "Failed to <verb> <noun>"
.context("Failed to read session from database")?;
.context("Failed to parse config JSON")?;
.context("Failed to write index to disk")?;

// For lock errors, use helper:
fn lock_err<T: std::fmt::Display>(e: T) -> anyhow::Error {
    anyhow::anyhow!("Lock poisoned: {e}")
}
```

## Phase 3: Handler Deduplication

### 3.1 Injection Context Builder

Extract from `injection_handlers.rs` lines 83-111 and 140-165:

```rust
/// Build injection context from session state and config.
async fn build_injection_context(
    state: &SharedState,
    session_id: Option<&str>,
    inject_mode: Option<&str>,
) -> Result<(Option<Session>, Vec<String>, InjectionMode)> {
    let mode = parse_injection_mode(inject_mode)?;
    let sid = get_session_id(session_id.map(String::from));
    let session = match sid {
        Some(id) => state.get_session(&id).await?,
        None => None,
    };
    let mut query_parts = vec![];
    if let Some(s) = &session {
        query_parts.push(s.name.clone());
        if !s.active_files.is_empty() {
            query_parts.push(s.active_files.join(" "));
        }
        if !s.recent_tools.is_empty() {
            query_parts.push(s.recent_tools.join(" "));
        }
    }
    Ok((session, query_parts, mode))
}
```

### 3.2 Injection Result Applier

Extract from lines 120-126 and 168-173:

```rust
/// Apply injection result to target file if it was applied.
fn apply_injection_result(path: &Path, result: &InjectionRunResult) {
    if result.applied {
        if let Some(block) = &result.injected_block {
            if let Err(err) = orchestration::append_injected_context(path, block) {
                eprintln!("Warning: failed to append injected context: {err}");
            }
        }
    }
}
```

### 3.3 JSON Fallback Parser

Extract from `plugin_handlers.rs:62`, `daemon_dispatch.rs:436,494`:

```rust
/// Parse a string as JSON, falling back to `{"raw": input}` on failure.
fn parse_json_or_raw(input: &str) -> serde_json::Value {
    serde_json::from_str(input)
        .unwrap_or_else(|_| serde_json::json!({"raw": input}))
}
```

### 3.4 Lock Error Helper

Extract from `state/persistence.rs` (13 identical instances):

```rust
/// Convert a lock poison error to an anyhow error.
fn lock_err<T: std::fmt::Display>(e: T) -> anyhow::Error {
    anyhow::anyhow!("Lock poisoned: {e}")
}
```

## Phase 4: Module-Level Documentation

Add `//!` module docs to 9 critical modules using `daemon/mod.rs` as template:

| Module | Key Points to Document |
|--------|----------------------|
| `main.rs` | Entry point, mode dispatch (direct/daemon/gui), CLI parsing |
| `state/mod.rs` | Persistence layer, RwLock + dirty flag, session lifecycle |
| `retrieval/mod.rs` | Search indexing, SQLite FTS, embedding vectors, page index |
| `injection/mod.rs` | Context staging, injection modes (review/auto), scope rules |
| `ops_workbench.rs` | GUI workbench state adapter, telemetry store, snapshot builder |
| `memory/mod.rs` | Genome storage, decision tracking, memory search |
| `branding.rs` | Terminal theming, color constants, branded output helpers |
| `mcp/mod.rs` | Model Context Protocol server integration |
| `verify/mod.rs` | Verification pipeline, build checks, test runner |

## Phase 5: Structural Recommendations (Future)

These are documented for future work, not this cleanup pass:

- **Move IPC types to impulse-ops** — eliminate 200+ lines of manual JSON parsing in impulse-gui
- **Split retrieval/store.rs** (1,376 lines) — separate query from mutation methods
- **Split impulse-gui/views/terminals.rs** (1,579 lines) — extract grid + context rendering
- **Standardize builder patterns** — adopt fluent `with_*` methods across all config types
- **Update RUST-CANONICAL-CONTRACT.md** — add build hygiene + tooling commands

## Verification Gate

After all phases:

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Expected: 0 warnings, 0 errors, all tests pass, no format diffs.
