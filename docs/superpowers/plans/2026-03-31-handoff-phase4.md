# Handoff Prompt — Ralph Plan 3: Phase 4 Verification & Completion

> Copy everything below the line into a new session or /handoff.

---

## Context

You are continuing **Ralph Plan 3** for the Impulse project (`impulse-rs`). Phases 1-3 are **COMPLETE**. Phase 4 (Loops 24-30) remains.

**Plan document:** `ralph-plan-3.md` — read it first for the full iteration table and metrics.
**Implementation plan:** `docs/superpowers/plans/2026-03-30-codebase-reduction-agent-harness.md`

---

## What Was Accomplished (Phases 1-3)

### Phase 1: Dead Code Surgery (Loops 1-7) — COMPLETE
- Deleted 3 dead files from `llm_backends/` (−1,303 lines)
- Gated 6 unused agent methods behind `#[cfg(test)]` (later restored in Phase 3)
- Resolved 13 of 17 `#[allow(dead_code)]` markers (−73 lines)
- Documented intent stubs as Phase 3 targets
- Extracted handler helpers to `handlers/common.rs` (mod.rs 809→474)
- Removed 8 dead notification functions (−78 lines)

### Phase 2: Module Extraction (Loops 8-15) — COMPLETE
- Split `render_panels.rs` (2,139 lines) → 5 focused modules (max 682 lines)
- Split `daemon/mod.rs` (2,100 lines) → mod.rs (293) + protocol.rs (234) + handlers.rs (1,627)
- Restructured `config.rs` (1,509) → config.rs (407) + config_keys.rs (1,238) + 2 round-trip tests
- Split `main.rs` (1,548) → main.rs (66) + cli.rs (592) + daemon_dispatch.rs + direct_dispatch.rs
- Consolidated duplicate `atomic_write` implementations (−19 lines)
- Extracted 6 test helpers from `integration_tests.rs` (−164 lines)

### Phase 3: Agent Harness Wiring (Loops 16-23) — COMPLETE
- **Task 17:** `build_context_prompt()` + `query_with_context()` — ExtractedInsight→prompts pipeline
- **Task 18:** Intent classification activated at all 9 extraction sites + coordinator priority sorting
- **Task 19:** `CoordinationResult` + `run_full_coordination()` — full pipeline with pane summaries
- **Task 20:** `GetConflictHistory` + `ClearResolvedConflicts` IPC endpoints
- **Task 21:** Structured JSON harness protocol (`HarnessRequest`/`HarnessResponse`) with fallback
- **Task 22:** Session history tracking (5-turn bound, truncation, `cached_agent` persistence)
- **Task 23:** `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` IPC endpoints

### Metrics After Phase 3
| Metric | Baseline | Current |
|--------|----------|---------|
| Total LOC | 132,442 | **125,909 (−6,533)** — TARGET EXCEEDED |
| `#[allow(dead_code)]` markers | 17 | 2 (justified serde) |
| Agent harness wired features | 3/10 | **10/10** — ALL WIRED |
| Unused agent methods | 6+ | **0** — all restored + IPC-wired |
| Test count | 1,118 | 962+ (net gain from Phase 3, some consolidation in Phase 2) |

---

## What Remains: Phase 4 (Loops 24-30)

### Loop 24: Commit Phase 3
- Run: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
- `git add` all Phase 3 files (agent/, daemon/, context_lifecycle/, client/, ui/)
- Commit with descriptive message summarizing Phase 3

### Loop 25: Planning Checkpoint
- Measure: `find . -name "*.rs" | xargs wc -l | tail -1` (workspace LOC)
- `find src -name "*.rs" -exec wc -l {} + | sort -rn | head -15` (source LOC + largest files)
- Test count: `cargo test 2>&1 | grep "test result"`
- Agent feature matrix: all 10 features wired ✓
- Check remaining files >800 lines
- Count remaining `#[allow(dead_code)]` markers

### Loop 26: Add tests for Phase 3 features (15-25 new tests)
Target areas:
1. Context→prompt pipeline integration (mock insights → verify prompt enrichment)
2. Intent classification accuracy (representative agent output samples)
3. Coordinator full pipeline (multi-pane with conflicts + errors + summaries)
4. Conflict history IPC round-trip (create → resolve → query → verify)
5. Structured harness fallback paths
6. Session awareness multi-turn continuity
7. Specialized IPC endpoints (ReviewCode, AnalyzeError, SummarizePane)

### Loop 27: Add tooling module tests (20+ new tests)
- `src/tooling/` has 2,650 LOC but only ~8 tests (3.0 tests/KLOC)
- Test: capability enforcement (deny-by-default), param validation, tool registration, schema export
- This is the capability enforcement chain — critical security model

### Loop 28: Full workspace verification
- `cargo build --all-features`
- `cargo test` (all passing except known 2 flaky daemon socket tests)
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo fmt --check`
- Fix any regressions

### Loop 29: Update docs
- `CLAUDE.md` — update module counts, LOC, test counts, new architecture (harness.rs, config_keys/, etc.)
- `ROADMAP-PLAN.md` — mark agent harness as wired
- `MEMORY.md` — record Phase 3 outcomes

### Loop 30: Final metrics comparison
- LOC comparison: 132,442 baseline → current
- Test comparison: 1,118 baseline → current
- Agent feature matrix: 10/10 ✓
- Files >800 lines: list remaining
- `#[allow(dead_code)]`: count remaining
- Record all metrics in `ralph-plan-3.md` Working Log

---

## Critical Rules
1. `cargo build && cargo test && cargo clippy -- -D warnings` after every task
2. Don't break existing tests — the 2 flaky daemon socket tests are known pre-existing
3. Commit at loop 24 (Phase 3) and after loop 30 (Phase 4)
4. Update `ralph-plan-3.md` status column as each loop completes

---

## Bonus: Code Policy Enhancement (Post-Phase 4)

The stop hook from the Phase 3 session flagged this follow-up task. After Phase 4 is complete, run 10 iterations applying `rust-programming` skill patterns to enhance code policies across project docs:

1. **Testing standards** — codify test quality bar in CLAUDE.md (already started, verify complete)
2. **Error handling policy** — thiserror/anyhow rules, context chaining, Err path test requirements
3. **Lint/allow policy** — `#[allow(dead_code)]` justification rules, file-level vs item-level
4. **Serde round-trip requirements** — mandate for all Serialize+Deserialize types
5. **Unsafe code policy** — SAFETY comments, precondition validation, dedicated tests
6. **Test helper centralization** — extract shared factories to `#[cfg(test)]` modules
7. **Property-based testing** — proptest for path validation, config parsing, serialization
8. **Test pattern documentation** — DaemonGuard, TempDir patterns, mock tool patterns
9. **Verification gates** — pre-commit checks, CI requirements
10. **Cross-doc sync** — CLAUDE.md ↔ RUST-CANONICAL-CONTRACT.md ↔ AGENTS.md consistency

Files to update: `CLAUDE.md`, `docs/spec/RUST-CANONICAL-CONTRACT.md`, project MEMORY.md

---

## How to Start
1. Read `ralph-plan-3.md` for current status
2. Start with Loop 24 (commit Phase 3 work)
3. Use `superpowers:verification-before-completion` before committing
4. Loops 26 and 27 can run as parallel subagents (zero file overlap)
