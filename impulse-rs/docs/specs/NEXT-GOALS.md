# Next Goals — Impulse Platform Enhancement

> Prioritized goals for continued platform improvement, derived from the April 2026 codebase audit.
> Last updated: 2026-04-04

## Completed (Rounds 1-2)

### Round 1 (2026-04-04)
- [x] Renamed `stewardship::ToolResult` to `ParsedToolResult` (name collision fix)
- [x] Fixed stale counts in CLAUDE.md + README (1,343 -> 1,344 tests, LOC correction)
- [x] Added `.context()` to `retrieval/store.rs` (~97 ops) and `handlers/direct_dispatch.rs` (~57 ops)
- [x] Extracted 3 helpers: `parse_json_or_raw()`, `apply_injection_result()`, `lock_err()`
- [x] Added `//!` module docs to 9 critical modules

### Round 2 (2026-04-04)
- [x] Added `.context()` to 4 more files: `retrieval/indexer.rs` (68), `daemon_dispatch.rs` (26), `state/persistence.rs` (17), `ops_workbench.rs` (22)
- [x] Updated RUST-CANONICAL-CONTRACT.md v1.6: missing commands + test counts
- [x] Created `impulse-gui/README.md` and `impulse-term/README.md`
- [x] Added `///` doc comments to 35 pub functions across 9 handler files
- [x] Fixed 2 test assertions broken by error context additions

---

## Next Priority: Error Context Completion

**Current coverage: ~55% (up from 16.6%)**

Remaining high-value files for `.context()` chains:

| File | Bare `?` | Domain |
|------|----------|--------|
| `handlers/system.rs` | ~24 | System utilities (sweep, wipe, build-health) |
| `handlers/tooling_handlers.rs` | ~23 | Dynamic tool dispatch |
| `stewardship/cross_project.rs` | ~23 | Cross-project analysis |
| `stewardship/analyzer.rs` | ~20 | Transcript parsing |
| `agent/mod.rs` | ~15 | Agent harness |
| `agent/coordinator.rs` | ~15 | Agent coordination |
| `daemon/handlers.rs` | ~30 | Daemon handler routing |
| `daemon/mod.rs` | ~20 | Daemon lifecycle |

**Target: 80%+ context coverage across all production code.**

---

## Next Priority: IPC Type Unification

**Impact: HIGH** — eliminates 248 lines of fragile JSON parsing

`impulse-gui/src/ipc/types.rs` defines 5 types (`Session`, `HistoryEntry`, `Genome`, `SearchResult`, `GuardRule`) with manual `from_value()` JSON extraction instead of serde derives.

### Plan

1. Define corresponding serde types in `impulse-ops/src/lib.rs`:
   - `IpcSession`, `IpcHistoryEntry`, `IpcGenome`, `IpcSearchResult`, `IpcGuardRule`
   - Each derives `Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]`
   - Include field aliases where the daemon uses inconsistent naming (`id` vs `session_id`)

2. Update daemon response serialization to use these shared types

3. Replace `impulse-gui/src/ipc/types.rs` manual parsers with:
   ```rust
   let session: IpcSession = serde_json::from_value(json)?;
   ```

4. Delete 248 lines of `from_value()` boilerplate

### Prerequisites
- Audit daemon response JSON shapes for each type
- Ensure all field aliases are captured in serde attributes

---

## Next Priority: Handler Test Coverage

**9 handler files have ZERO tests (2,399 LOC)**

### Priority Order (by risk * coverage gap)

| # | File | Lines | Pub Items | Risk | Approach |
|---|------|-------|-----------|------|----------|
| 1 | `session.rs` | 504 | 8 | HIGH — session lifecycle | Unit tests with mock state |
| 2 | `system.rs` | 523 | 11 | HIGH — sweep/wipe/build-health | Integration with temp dirs |
| 3 | `stewardship_handlers.rs` | 365 | 4 | HIGH — transcript ops | Unit tests with sample JSONL |
| 4 | `build.rs` | 256 | 6 | HIGH — build health | Unit tests with mock state |
| 5 | `tooling_handlers.rs` | 272 | 6 | MEDIUM — tool dispatch | Unit tests with mock registry |
| 6 | `guard.rs` | 466 | 2 | MEDIUM — guardrails | Unit tests with rule fixtures |
| 7 | `describe.rs` | 622 | 2 | LOW — output formatting | Snapshot tests |
| 8 | `retrieval.rs` | 84 | 2 | LOW — thin wrapper | Integration only |
| 9 | `office.rs` | 142 | 1 | LOW — feature-gated | Conditional tests |

### Test Pattern per Handler

```rust
#[tokio::test]
async fn test_handle_<command>_happy_path() {
    let temp = TempDir::new().unwrap();
    let state = Arc::new(State::new(temp.path().to_path_buf()).unwrap());
    // Set up preconditions
    let result = handle_<command>(state.clone(), &args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_handle_<command>_missing_required_arg() {
    let temp = TempDir::new().unwrap();
    let state = Arc::new(State::new(temp.path().to_path_buf()).unwrap());
    let result = handle_<command>(state.clone(), &empty_args).await;
    assert!(result.is_err());
}
```

---

## Structural Improvements (Future)

### File Splits

| File | Current Lines | Recommendation |
|------|---------------|----------------|
| `retrieval/store.rs` | 1,376 | Split: `store_query.rs` (search ops) + `store_mutation.rs` (upsert/delete) + `store_schema.rs` (DDL/migration) |
| `impulse-gui/views/terminals.rs` | 1,579 | Split: `terminal_grid.rs` (grid layout) + `terminal_context_render.rs` (context display) |
| `impulse-gui/widgets/signal_bus.rs` | 804 | Split: `signal_bus_events.rs` (event definitions) |
| `daemon/handlers.rs` | 1,783 | Monitor — approaching split threshold |

### Builder Pattern Standardization

Current state: 3 different construction patterns across the codebase.

**Target pattern** (fluent builder):
```rust
impl Agent {
    pub fn new() -> Self { Self::default() }
    pub fn with_model(mut self, model: &str) -> Self { self.model = model.into(); self }
    pub fn build(self) -> Result<Self> { validate(self) }
}
```

Apply to: `Config`, `StewardshipConfig`, `ToolContext`, `ToolDescriptor`, `InjectionConfig`.

### serde_json::Value Reduction

495 usages of `serde_json::Value` across the codebase. Many could be replaced with typed structs for:
- Better IDE support and refactoring safety
- Compile-time validation of field names
- Clearer APIs

Focus areas: `daemon/handlers.rs`, `handlers/daemon_dispatch.rs`, `agent/coordinator.rs`.

---

## Documentation Maintenance

### Completed
- [x] Module-level `//!` docs for 9 critical modules
- [x] Handler `///` doc comments for 35 pub functions
- [x] impulse-gui/README.md
- [x] impulse-term/README.md
- [x] RUST-CANONICAL-CONTRACT.md v1.6

### Remaining
- [ ] impulse-ops/README.md (shared types crate — lower priority)
- [ ] Update CLAUDE.md test density table (handlers improved from 0.8 to ~2.5)
- [ ] Review docs/ARCHITECTURE.md for staleness
- [ ] Add `cargo doc --document-private-items` to CI for doc coverage tracking

---

## Quality Metrics Tracking

| Metric | Round 1 Start | Round 2 End | Target |
|--------|--------------|-------------|--------|
| `.context()` coverage | 16.6% | ~55% | 80%+ |
| Module `//!` docs | 52.8% | ~60% | 90%+ |
| Handler `///` coverage | 10% | ~85% | 100% |
| Handler test coverage | 6/19 files | 6/19 files | 15/19 files |
| Clippy warnings | 0 | 0 | 0 |
| Test count | 1,344 | 1,344 | 1,400+ |
| README coverage | 1/4 crates | 3/4 crates | 4/4 |
