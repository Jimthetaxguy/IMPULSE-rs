# Cleanup Round 2 — Plan

> Continuing from CLEANUP-SPEC-2026-04-04.md. Round 1 completed Phases 1, 3, 4 and partial Phase 2.

## Round 1 Results (Completed)

- [x] Phase 1: ToolResult rename, stale count fixes, dead_code audit
- [x] Phase 2 partial: .context() in retrieval/store.rs (97 ops) + direct_dispatch.rs (57 ops)  
- [x] Phase 3: Handler deduplication (3 helpers extracted)
- [x] Phase 4: Module-level docs (9 modules)
- [x] Verification: 1,344 tests pass, 0 clippy warnings, clean fmt

## Round 2 Goals

### Stream A: Error Context Coverage (218 remaining bare `?`)

Continue adding `.context()` to the next 4 highest-priority files:

| Task | File | Bare `?` | Priority |
|------|------|----------|----------|
| A1 | `src/retrieval/indexer.rs` | 64 | CRITICAL — I/O heavy indexing |
| A2 | `src/handlers/daemon_dispatch.rs` | 32 | HIGH — IPC routing path |
| A3 | `src/state/persistence.rs` | 28 | HIGH — persistence layer |
| A4 | `src/ops_workbench.rs` | 24 | MEDIUM — workbench adapter |

Context message convention: "Failed to <verb> <noun>" — specific to module domain.

### Stream B: Documentation Enhancement

| Task | What | Impact |
|------|------|--------|
| B1 | Update RUST-CANONICAL-CONTRACT.md — add missing CLI commands (sweep, wipe, clean-all, sccache-setup, build-health, tooling-list/describe/run, panes), update test counts | HIGH — contract accuracy |
| B2 | Create `impulse-gui/README.md` — views, themes, testing, deps, IPC | MEDIUM — contributor onboarding |
| B3 | Create `impulse-term/README.md` — PTY, context lifecycle, rendering | MEDIUM — contributor onboarding |
| B4 | Add `///` doc comments to 9 undocumented handler files | MEDIUM — API navigability |

### Stream C: Next Goals

| Task | What |
|------|------|
| C1 | Write NEXT-GOALS.md — structural recommendations for future work |

## Execution Strategy

Tasks within each stream are independent (different files). Dispatch in parallel:
- **Wave 1**: A1 + A2 + B1 + B2 + B3 (5 parallel agents, zero file overlap)
- **Wave 2**: A3 + A4 + B4 (3 parallel agents, after Wave 1 verify)
- **Wave 3**: C1 (final synthesis) + verification gate

## Verification

After each wave:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```
