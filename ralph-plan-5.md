# Ralph Plan 5 — Deep Code Review, Handler Tests, TUI/UX & Daemon-Truth

> **Created:** 2026-04-01
> **Previous:** Ralph Plan 4 (24 loops, validation + daemon-truth + TUI/UX — partial)
> **Previous:** Ralph Plan 3 (30 loops, codebase reduction + agent harness — COMPLETE)
> **Goal:** Close every quality gap found in the 2026-04-01 deep code review, complete remaining Ralph Plan 4 work, and push all test density targets to green
> **Baseline (2026-04-01):** 79,194 LOC, 1,025 tests, 13/19 handler files untested, 0.8 handler tests/KLOC

---

## Root: Primary Objective

Execute 100 loops to systematically close every gap identified in the 2026-04-01 deep code review audit:
1. Commit the 12 uncommitted files (DRY refactor + error handling + GUI fixes)
2. Add tests to all 13 untested handler files (push handler density from 0.8 to >=2.0 tests/KLOC)
3. Complete remaining TUI correctness fixes from Ralph Plan 4 (unwrap in renderer, unsafe env, backend tests)
4. Complete TUI/UX enhancements (StatusBar extraction, Copy button, welcome screen, animations)
5. Adopt property-based testing (proptest) for high-value combinatorial targets
6. Push core test density toward 3.0/KLOC (state, daemon, agent)
7. Add tests for 24 untested impulse-gui view/widget files
8. Complete daemon-truth EGUI deep integration
9. Refactor 9 TODO param-struct sites
10. Update all documentation to verified metrics and push to remote

---

## Root: User Vision

Transform Impulse from a codebase with strong architecture but uneven test coverage into a **comprehensively tested, fully documented, production-ready sidecar** where:

1. **Every handler is tested** — no user-facing CLI path runs without regression coverage
2. **Every hot path is safe** — no `unwrap()` in renderer loops, no `unsafe` env manipulation
3. **Property tests catch edge cases** — proptest validates path sanitization, config parsing, protocol serialization
4. **The operator console is polished** — extracted StatusBar, working Copy button, distinctive welcome screen
5. **Documentation matches reality** — every metric in CLAUDE.md, AGENTS.md, and RUST-CANONICAL-CONTRACT.md reflects verified test output
6. **Daemon-truth EGUI is wired** — terminal telemetry flows through the daemon as authoritative source

**Phase ranges:**
- Phase 1 (Loops 1-7): Commit + TUI Correctness — ship uncommitted work, fix hot-path safety
- Phase 2 (Loops 9-15): Handler Tests Batch 1 — 5 highest-priority untested handlers
- Phase 3 (Loops 17-23): Handler Tests Batch 2 + TUI/UX — remaining handlers + operator experience
- Phase 4 (Loops 25-31): TUI/UX Enhancements — animations, insights, agent themes
- Phase 5 (Loops 33-39): Property-Based Testing — proptest adoption across 6 targets
- Phase 6 (Loops 41-47): Core Test Density — state/daemon/agent toward 3.0/KLOC
- Phase 7 (Loops 49-57): GUI Test Coverage — 24 untested impulse-gui files
- Phase 8 (Loops 59-67): Daemon-Truth EGUI Integration — full publish/subscribe/overlay
- Phase 9 (Loops 69-77): TODO Refactors + Code Quality — 9 param-struct sites
- Phase 10 (Loops 79-87): Documentation & Alignment — all docs updated
- Phase 11 (Loops 89-97): Integration Tests & Final Verification
- Phase 12 (Loops 99-100): Ship + Archive

---

## Root: Iteration Contents

| Loop | Focus | Phase | Type | Status |
|------|-------|-------|------|--------|
| 1 | Commit: Stage and commit 12 uncommitted files | Commit + TUI | commit | **done** (6ef25a4) |
| 2 | TUI Correctness: Fix unwrap in renderer hot path (impulse-term/renderer.rs) | Commit + TUI | work | **done** (already fixed in RP4) |
| 3 | TUI Correctness: Safe env var manipulation (impulse-term/panel.rs) | Commit + TUI | work | **done** (already fixed in RP4) |
| 4 | TUI Correctness: Backend.rs test coverage (312 LOC, zero tests) | Commit + TUI | work | **done** (+10 tests) |
| 5 | Handler Tests: guard.rs (204 LOC — action guardrails with process::exit) | Commit + TUI | work | **done** (+16 tests) |
| 6 | Handler Tests: agent.rs (145 LOC — agent config/query) | Commit + TUI | work | **done** (+16 tests) |
| 7 | Handler Tests: injection_handlers.rs (209 LOC — context injection) | Commit + TUI | work | **done** (+18 tests) |
| **8** | **Planning Checkpoint: Review Phase 1 metrics, plan Phase 2** | **Checkpoint** | **planning** | **done** (1,076 tests, 9/19 handlers tested) |
| 9 | Handler Tests: daemon_dispatch.rs (450 LOC — routes all IPC) | Handler Tests B1 | work | in-progress (sub-agent dispatched) |
| 10 | Handler Tests: direct_dispatch.rs (465 LOC — routes all CLI) | Handler Tests B1 | work | in-progress (sub-agent dispatched) |
| 11 | Handler Tests: common.rs (379 LOC — shared helpers) | Handler Tests B1 | work | in-progress (sub-agent dispatched) |
| 12 | Handler Tests: stewardship_handlers.rs (365 LOC) | Handler Tests B1 | work | pending |
| 13 | Handler Tests: tooling_handlers.rs (270 LOC) | Handler Tests B1 | work | pending |
| 14 | Handler Tests: build.rs (256 LOC) | Handler Tests B1 | work | pending |
| 15 | Commit: Stage and commit handler tests batch 1 + verify | Handler Tests B1 | commit | pending |
| **16** | **Planning Checkpoint: Review handler density, plan Phase 3** | **Checkpoint** | **planning** | **pending** |
| 17 | Handler Tests: semantic_diff_handlers.rs (164 LOC) | Handler Tests B2 | work | pending |
| 18 | Handler Tests: office.rs (142 LOC, behind feature flag) | Handler Tests B2 | work | pending |
| 19 | Handler Tests: plugin_handlers.rs (95 LOC) | Handler Tests B2 | work | pending |
| 20 | Handler Tests: retrieval.rs (84 LOC) | Handler Tests B2 | work | pending |
| 21 | TUI/UX: Extract StatusBar from TerminalPanel (panel.rs → status_bar.rs) | Handler Tests B2 | work | pending |
| 22 | TUI/UX: Fix Copy button + compact budget bar in tab bar | Handler Tests B2 | work | pending |
| 23 | TUI/UX: Welcome screen overhaul | Handler Tests B2 | work | pending |
| **24** | **Planning Checkpoint: Review TUI/UX, plan Phase 4** | **Checkpoint** | **planning** | **pending** |
| 25 | TUI/UX: Subtle animations (fade-in, pulse on insight) | TUI/UX | work | pending |
| 26 | TUI/UX: Backend.rs error logging on silent failures | TUI/UX | work | pending |
| 27 | UX: Insights overlay — virtualization + scroll | TUI/UX | work | pending |
| 28 | UX: Configurable agent spawn delays | TUI/UX | work | pending |
| 29 | UX: Agent-specific color themes | TUI/UX | work | pending |
| 30 | UX: Context history drill-down view | TUI/UX | work | pending |
| 31 | Commit: Stage and commit TUI/UX enhancements + verify | TUI/UX | commit | pending |
| **32** | **Planning Checkpoint: Review UX work, plan Phase 5** | **Checkpoint** | **planning** | **pending** |
| 33 | Proptest: Path sanitization (sanitize_path, sanitize_id) | Proptest | work | pending |
| 34 | Proptest: Config round-trips with random data | Proptest | work | pending |
| 35 | Proptest: Session ID validation invariants | Proptest | work | pending |
| 36 | Proptest: Protocol serialization (DaemonRequest/Response) | Proptest | work | pending |
| 37 | Proptest: Context extraction (ExtractedInsight fields) | Proptest | work | pending |
| 38 | Proptest: Tool parameter validation (capability enforcement) | Proptest | work | pending |
| 39 | Commit: Stage and commit proptest adoption + verify | Proptest | commit | pending |
| **40** | **Planning Checkpoint: Review proptest coverage, plan Phase 6** | **Checkpoint** | **planning** | **pending** |
| 41 | Core Tests: state/session lifecycle corners (rapid start/end, duplicate IDs) | Core Density | work | pending |
| 42 | Core Tests: state/config corruption recovery | Core Density | work | pending |
| 43 | Core Tests: daemon reconnection/recovery (socket errors) | Core Density | work | pending |
| 44 | Core Tests: daemon protocol edge cases | Core Density | work | pending |
| 45 | Core Tests: agent harness error cases (missing context, malformed JSON) | Core Density | work | pending |
| 46 | Core Tests: delegation system round-trips | Core Density | work | pending |
| 47 | Commit: Stage and commit core density tests + verify | Core Density | commit | pending |
| **48** | **Planning Checkpoint: Review core density, plan Phase 7** | **Checkpoint** | **planning** | **pending** |
| 49 | GUI Tests: views/artifacts.rs | GUI Coverage | work | pending |
| 50 | GUI Tests: views/context.rs | GUI Coverage | work | pending |
| 51 | GUI Tests: views/genome.rs + views/memory.rs | GUI Coverage | work | pending |
| 52 | GUI Tests: views/overview.rs + views/search.rs | GUI Coverage | work | pending |
| 53 | GUI Tests: views/sessions.rs + views/terminal_context.rs | GUI Coverage | work | pending |
| 54 | GUI Tests: views/terminal_insights.rs | GUI Coverage | work | pending |
| 55 | GUI Tests: widgets/sidebar.rs + widgets/status_bar.rs | GUI Coverage | work | pending |
| 56 | GUI Tests: widgets/command_palette.rs + widgets/conflict_banner.rs | GUI Coverage | work | pending |
| 57 | Commit: Stage and commit GUI tests + verify | GUI Coverage | commit | pending |
| **58** | **Planning Checkpoint: Review GUI coverage, plan Phase 8** | **Checkpoint** | **planning** | **pending** |
| 59 | Daemon-Truth: Enrich OpsSnapshot with live terminal data | Daemon-Truth | work | pending |
| 60 | Daemon-Truth: Real-time terminal telemetry overlay | Daemon-Truth | work | pending |
| 61 | Daemon-Truth: Agent pool display in Overview | Daemon-Truth | work | pending |
| 62 | Daemon-Truth: Artifact browsing from daemon state | Daemon-Truth | work | pending |
| 63 | Daemon-Truth: Context injection visualization | Daemon-Truth | work | pending |
| 64 | Daemon-Truth: Stewardship dashboard wiring | Daemon-Truth | work | pending |
| 65 | Daemon-Truth: Delegation tracking UI | Daemon-Truth | work | pending |
| 66 | Daemon-Truth: Conflict resolution display | Daemon-Truth | work | pending |
| 67 | Commit: Stage and commit daemon-truth work + verify | Daemon-Truth | commit | pending |
| **68** | **Planning Checkpoint: Review daemon-truth, plan Phase 9** | **Checkpoint** | **planning** | **pending** |
| 69 | Refactor: handlers/memory.rs param structs (x2 sites) | Refactors | work | pending |
| 70 | Refactor: token_tracker/algorithm.rs param struct | Refactors | work | pending |
| 71 | Refactor: daemon/mod.rs + daemon/handlers.rs param structs | Refactors | work | pending |
| 72 | Refactor: retrieval/store.rs param structs (x2 sites) | Refactors | work | pending |
| 73 | Refactor: ui/terminal_pane.rs + ui/pane_manager.rs param structs | Refactors | work | pending |
| 74 | Code Quality: Eliminate remaining println-only tests | Refactors | work | pending |
| 75 | Code Quality: Audit and fix remaining #[allow(dead_code)] markers | Refactors | work | pending |
| 76 | Code Quality: Enforce .context() on all bare ? I/O operations | Refactors | work | pending |
| 77 | Commit: Stage and commit refactors + verify | Refactors | commit | pending |
| **78** | **Planning Checkpoint: Review refactors, plan Phase 10** | **Checkpoint** | **planning** | **pending** |
| 79 | Docs: Update CLAUDE.md with final verified metrics | Docs | work | pending |
| 80 | Docs: Update AGENTS.md with final verified metrics | Docs | work | pending |
| 81 | Docs: Update RUST-CANONICAL-CONTRACT.md | Docs | work | pending |
| 82 | Docs: Update ROADMAP-PLAN.md (mark completed items) | Docs | work | pending |
| 83 | Docs: Update IPC-PROTOCOL.md (new endpoints from daemon-truth) | Docs | work | pending |
| 84 | Docs: Update ARCHITECTURE docs | Docs | work | pending |
| 85 | Docs: Cross-doc consistency verification (all 6 contract docs) | Docs | verification | pending |
| 86 | Docs: Update project memory files | Docs | work | pending |
| 87 | Commit: Stage and commit documentation + verify | Docs | commit | pending |
| **88** | **Planning Checkpoint: Review docs, plan Phase 11** | **Checkpoint** | **planning** | **pending** |
| 89 | Integration: Full CLI command coverage (every stable command) | Integration | work | pending |
| 90 | Integration: Daemon IPC round-trip for all endpoint groups | Integration | work | pending |
| 91 | Integration: Agent harness end-to-end pipeline | Integration | work | pending |
| 92 | Integration: Context injection pipeline (review → apply) | Integration | work | pending |
| 93 | Integration: Hook validation end-to-end (SessionStart, PreCompact) | Integration | work | pending |
| 94 | Integration: GUI → daemon → state flow | Integration | work | pending |
| 95 | Commit: Stage and commit integration tests + verify | Integration | commit | pending |
| 96 | Final Verification: Full workspace gate (build + test + clippy + fmt) | Final | verification | pending |
| 97 | Final Verification: Metrics comparison vs baseline | Final | verification | pending |
| **98** | **Planning Checkpoint: Final review** | **Checkpoint** | **planning** | **pending** |
| 99 | Ship: Final commit + push to origin/main | Ship | commit | pending |
| 100 | Archive: Ralph Plan 5 completion, metrics summary, lessons learned | Ship | verification | pending |

---

## Dependency Graph

```
Phase 1: Commit + TUI Correctness (sequential start, then parallel)
  1(commit) → [2, 3, 4](parallel TUI fixes) → [5, 6, 7](parallel handler tests)

Phase 2: Handler Tests Batch 1 (mostly parallel after 8)
  8(plan) → [9, 10, 11](parallel) → [12, 13, 14](parallel) → 15(commit)

Phase 3: Handler Tests Batch 2 + TUI/UX (parallel tracks)
  16(plan) → [17, 18, 19, 20](parallel handlers) → [21, 22, 23](parallel TUI/UX)

Phase 4: TUI/UX Enhancements (21 must land first, then parallel)
  24(plan) → [25, 26, 27, 28, 29, 30](parallel) → 31(commit)

Phase 5: Proptest Adoption (all parallel — separate modules)
  32(plan) → [33, 34, 35, 36, 37, 38](parallel) → 39(commit)

Phase 6: Core Test Density (some sequential)
  40(plan) → [41, 42](parallel state) → [43, 44](parallel daemon) → [45, 46](parallel agent) → 47(commit)

Phase 7: GUI Test Coverage (all parallel — separate files)
  48(plan) → [49, 50, 51, 52, 53, 54, 55, 56](parallel) → 57(commit)

Phase 8: Daemon-Truth EGUI (sequential — each builds on prior)
  58(plan) → 59 → 60 → 61 → [62, 63, 64, 65, 66](parallel) → 67(commit)

Phase 9: Refactors (all parallel — separate files)
  68(plan) → [69, 70, 71, 72, 73](parallel) → [74, 75, 76](parallel) → 77(commit)

Phase 10: Documentation (parallel docs, then verify)
  78(plan) → [79, 80, 81, 82, 83, 84](parallel) → 85(verify) → 86 → 87(commit)

Phase 11: Integration Tests (parallel)
  88(plan) → [89, 90, 91, 92, 93, 94](parallel) → 95(commit)

Phase 12: Ship
  96(verify) → 97(verify) → 98(plan) → 99(commit) → 100(archive)
```

---

## Sub-Agent Strategy

| Agent Type | Loops | Purpose |
|------------|-------|---------|
| `Explore` | 8, 16, 24, 32, 40, 48, 58, 68, 78, 88, 98 | Metrics gathering for planning checkpoints |
| `feature-dev:code-reviewer` | 15, 31, 39, 47, 57, 67, 77, 87, 95 | Pre-commit review of each phase |
| `feature-dev:code-explorer` | 9, 10, 59-66 | Deep analysis for dispatch handlers + daemon-truth |
| `superpowers:code-reviewer` | 96, 100 | Final validation against this plan |
| `code-simplifier:code-simplifier` | 69-76 | Param-struct refactors |
| `pr-review-toolkit:pr-test-analyzer` | 97 | Test coverage completeness review |
| `general-purpose` | 2-7, 11-14, 17-23, 25-30, 33-38, 41-46, 49-56, 79-86, 89-94 | Primary work execution |

**Parallelization targets (maximum 3 concurrent sub-agents):**
- Loops [5, 6, 7]: 3 handler test files in parallel
- Loops [9, 10, 11]: 3 handler test files in parallel
- Loops [12, 13, 14]: 3 handler test files in parallel
- Loops [17, 18, 19]: 3 handler test files in parallel
- Loops [33, 34, 35]: 3 proptest targets in parallel
- Loops [49, 50, 51]: 3 GUI test files in parallel
- Loops [79, 80, 81]: 3 doc files in parallel
- Loops [89, 90, 91]: 3 integration test targets in parallel

---

## Metrics Targets

| Metric | Baseline (2026-04-01) | Target | Phase |
|--------|----------------------|--------|-------|
| Total tests (workspace) | 1,025 | >=1,350 | All |
| Handler test density | 0.8 tests/KLOC | >=2.0 tests/KLOC | 2-3 |
| Handler files untested | 13/19 | 0/19 | 2-3 |
| Core test density | 1.5 tests/KLOC | >=2.5 tests/KLOC | 6 |
| GUI files untested | 24/44 | <=10/44 | 7 |
| Proptest count | 0 | >=12 | 5 |
| TODO refactors remaining | 9 | 0 | 9 |
| `unwrap()` in hot paths | >=1 | 0 | 1 |
| `unsafe` env manipulation | 1 block | 0 | 1 |
| CLAUDE.md metric accuracy | 6 stale values | 0 stale | 10 |
| Integration test commands | ~26 | >=40 | 11 |

---

## Phase 1 Detailed Plans (Loops 1-7)

### Loop 1 Plan
**Type:** commit
**Objective:** Commit 12 uncommitted files (DRY refactor, error handling, GUI fixes)
**Risk:** LOW
**Sub-steps:**
1. Run verification gate: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`
2. Stage all 12 files: `git add impulse-ops/src/lib.rs impulse-gui/src/ipc/types.rs impulse-gui/src/app.rs impulse-gui/src/state.rs impulse-gui/src/views/terminals.rs impulse-gui/src/widgets/command_palette.rs impulse-gui/src/agent_panel/chat.rs src/daemon/protocol.rs src/ops_workbench.rs src/orchestration/mod.rs impulse-term/src/renderer.rs tests/hook_validation_session_start.rs`
3. Commit with descriptive message summarizing: shared types to impulse-ops, .context() error chains, atomic writes, protocol compatibility tests, GUI fixes
4. Push to origin/main
**Inputs:** 12 uncommitted files verified clean in today's audit
**Outputs:** Clean commit on origin/main
**Status:** pending

### Loop 2 Plan
**Type:** work
**Objective:** Fix `unwrap()` in renderer hot path (`impulse-term/src/renderer.rs`)
**Risk:** MEDIUM
**Sub-steps:**
1. Read `impulse-term/src/renderer.rs` — find `runs.last_mut().unwrap()` in `build_runs()`
2. Replace with `if let Some(last) = runs.last_mut()` pattern
3. Add test: `test_build_runs_empty_row_does_not_panic`
4. Run `cargo test -p impulse-term`
**Inputs:** Renderer.rs code (from RP4 Loop 7 analysis)
**Outputs:** Safe renderer, no unwrap in hot path, +1 test
**Status:** pending

### Loop 3 Plan
**Type:** work
**Objective:** Eliminate `unsafe` env var manipulation in `impulse-term/src/panel.rs`
**Risk:** MEDIUM
**Sub-steps:**
1. Read `impulse-term/src/panel.rs` lines 74-83 — the unsafe env var block
2. Create `EnvGuard` struct with snapshot-and-restore pattern (no unsafe needed)
3. Replace unsafe block with `EnvGuard::new()` + Drop impl
4. Add test: `test_env_guard_restores_on_drop`
5. Run `cargo test -p impulse-term`
**Inputs:** Panel.rs unsafe block (from RP4 Loop 9 analysis)
**Outputs:** No unsafe in env manipulation, +1 test
**Status:** pending

### Loop 4 Plan
**Type:** work
**Objective:** Add test coverage for `impulse-term/src/backend.rs` (312 LOC, zero tests)
**Risk:** LOW
**Sub-steps:**
1. Read `impulse-term/src/backend.rs` — understand TerminalBackend API
2. Add tests for: spawn → is_alive → kill lifecycle
3. Add test for screen_text/scrollback_text with known vt100 content
4. Add test for reader thread error path (close slave → alive goes false)
5. Run `cargo test -p impulse-term`
**Inputs:** Backend.rs code (from RP4 Loop 10 analysis)
**Outputs:** Backend.rs tested, +4-6 tests
**Status:** pending

### Loop 5 Plan
**Type:** work
**Objective:** Add tests for `src/handlers/guard.rs` (204 LOC, zero tests)
**Risk:** LOW
**Sub-steps:**
1. Read `src/handlers/guard.rs` — map public functions
2. Add test: `handle_guard` with `list=true` (empty rules + builtin rules)
3. Add test: `handle_guard` with enable/disable (unknown rule fails, known succeeds)
4. Add test: `handle_guard` action evaluation (passes when no match, blocked by match)
5. Add test: `handle_analytics` (unknown subcommand, "conflicts" subcommand)
6. Use `test_state()` factory pattern from `session.rs`
7. Run `cargo test`
**Inputs:** guard.rs code, test_state() pattern from handlers/session.rs
**Outputs:** guard.rs tested, +4-6 tests
**Status:** pending

### Loop 6 Plan
**Type:** work
**Objective:** Add tests for `src/handlers/agent.rs` (145 LOC, zero tests)
**Risk:** LOW
**Sub-steps:**
1. Read `src/handlers/agent.rs` — map 3 public functions
2. Add test: `handle_agent_configure` — set provider/model/harness, verify config
3. Add test: `handle_agent_configure` — invalid provider returns error
4. Add test: `handle_agent_status` — unconfigured returns "not configured"
5. Add test: `handle_agent_status` — json output has expected keys
6. Add test: `handle_agent_query` — unconfigured agent returns error
7. Run `cargo test`
**Inputs:** agent.rs code, test_state() pattern
**Outputs:** agent.rs tested, +4-5 tests
**Status:** pending

### Loop 7 Plan
**Type:** work
**Objective:** Add tests for `src/handlers/injection_handlers.rs` (209 LOC, zero tests)
**Risk:** LOW
**Sub-steps:**
1. Read `src/handlers/injection_handlers.rs` — map 4 public functions
2. Add test: `handle_orchestrate` — basic task routing
3. Add test: `handle_handoff` — writes handoff file
4. Add test: `handle_sync_context` — creates context file
5. Add test: `handle_compute_injection` — returns error when unavailable
6. Run `cargo test`
**Inputs:** injection_handlers.rs code, test_state() pattern
**Outputs:** injection_handlers.rs tested, +4 tests
**Status:** pending
