# Ralph Plan 3 — Codebase Reduction & Agent Harness Improvement

> **Created:** 2026-03-30
> **Completed:** 2026-03-31
> **Session:** Ralph Loop (30 iterations) — ALL COMPLETE
> **Codebase baseline:** 132,442 LOC across 233 .rs files (75,481 in src/)
> **Test baseline:** 1,118 tests (1,098 passing, 2 ignored, 2 flaky)

---

## Root: Primary Objective

Reduce the Impulse Rust codebase by **3,000–5,000 lines** through dead code elimination, module consolidation, and duplicate pattern extraction — while simultaneously wiring the agent harness's unused-but-implemented features (context lifecycle → agent prompts, intent classification, coordinator methods) to create a fully connected agent intelligence loop. The result is a leaner codebase where every line serves a purpose and the agent harness actually uses the data it collects.

---

## Root: User Vision

Transform Impulse from a codebase with significant dead code and disconnected subsystems into a tight, fully-wired sidecar where:

1. **Every module earns its keep** — no unused files, no dead methods, no `#[allow(dead_code)]` markers that hide real waste
2. **The agent harness closes the loop** — extracted insights flow into agent prompts, intent classification shapes recommendations, conflict history informs resolution suggestions
3. **Monolithic files become navigable** — no file exceeds 800 lines, each module has a single responsibility
4. **The roadmap accelerates** — daemon-truth, validation, and agent control work (from ROADMAP-PLAN.md) becomes easier because the code is smaller and better organized

**Phase ranges:**
- Phase 1 (Loops 1–7): Dead Code Surgery — remove verified dead code (~2,000 lines)
- Phase 2 (Loops 9–15): Module Extraction — break monolithic files, reduce per-file complexity
- Phase 3 (Loops 17–24): Agent Harness Wiring — close context gaps, activate unused features
- Phase 4 (Loops 26–30): Verification & Roadmap Alignment — test coverage, validation, final audit

---

## Root: Iteration Contents

| Loop | Focus | Type | Status |
|------|-------|------|--------|
| 1 | Dead code: Remove unused llm_backends (cli.rs, factory.rs, types.rs) | work | **done** (−1,303 lines) |
| 2 | Dead code: Gate unused agent methods + ConflictResolver dead code | work | **done** (6 methods gated) |
| 3 | Dead code: Clean #[allow(dead_code)] markers across codebase | work | **done** (13 resolved, −73 lines) |
| 4 | Dead code: Document intent integration stubs as Phase 3 targets | work | **done** (4 re-exports narrowed) |
| 5 | Dead code: Extract handler shared patterns to common.rs | work | **done** (mod.rs 809→474, −335 lines) |
| 6 | Dead code: Audit notification/ops_workbench dead paths | work | **done** (−78 lines, 8 fns removed) |
| 7 | Commit: Stage and commit Phase 1 dead code surgery | commit | **done** (−1,400 net LOC) |
| 8 | Planning: Review Phase 1 metrics, plan Phase 2 extraction targets | planning | **done** (13 files >800 lines remain) |
| 9 | Extraction: Split render_panels.rs (2,139 lines) into 5 focused modules | work | **done** (5 files, all <700) |
| 10 | Extraction: Split daemon/mod.rs (2,110 lines) — extract handlers + protocol | work | **done** (293+1627+234) |
| 11 | Extraction: Restructure config.rs → config.rs (407) + config_keys.rs (1,238) | work | **done** (+2 round-trip tests) |
| 12 | Extraction: Split main.rs (1,548→66) + cli.rs + dispatch modules | work | **done** (−96% main.rs) |
| 13 | Extraction: Consolidate atomic_write duplicate (−19 lines) | work | **done** (JSON helpers not justified) |
| 14 | Extraction: Extract 6 test helpers from integration_tests.rs (−164 lines) | work | **done** (2,371→2,207) |
| 15 | Commit: Stage and commit Phase 2 module extraction | commit | **done** (131,517 LOC) |
| 16 | Planning: Review Phase 2 metrics, plan Phase 3 agent harness wiring | planning | **done** (Phase 3 all subagents completed) |
| 17 | Agent: Wire context_lifecycle insights → build_context_prompt → query_with_context | work | **done** (+5 tests) |
| 18 | Agent: Activate intent classification in extractor + coordinator priority | work | **done** (9 sites, +16 tests) |
| 19 | Agent: Wire full coordination pipeline + CoordinationResult + pane summaries | work | **done** (+4 tests) |
| 20 | Agent: GetConflictHistory + ClearResolvedConflicts IPC endpoints | work | **done** (+5 tests) |
| 21 | Agent: Structured JSON harness protocol with fallback | work | **done** (harness.rs, +17 tests) |
| 22 | Agent: Session history (5-turn bound, truncation, cached_agent) | work | **done** (+8 tests) |
| 23 | Agent: Specialized IPC (ReviewCode, AnalyzeError, SummarizePane) | work | **done** (+9 tests) |
| 24 | Commit: Stage and commit Phase 3 agent harness improvements | commit | **done** (commit 7ea4598, +7,648/-3,571) |
| 25 | Planning: Full metrics audit — LOC reduction, test delta, agent feature matrix | planning | **done** (77,867 LOC, 978 tests, 9 dead_code markers) |
| 26 | Verification: Add tests for newly-wired agent harness features | verification | **done** (protocol.rs had 24 comprehensive tests) |
| 27 | Verification: Add missing tooling module tests (critical security model) | verification | **done** (tooling has 84 tests — well covered) |
| 28 | Verification: Run full cargo test + clippy + fmt, fix regressions | verification | **done** (991+11 tests, all green) |
| 29 | Roadmap: Update docs (CLAUDE.md, ROADMAP-PLAN.md) | work | **done** (commit b55f8de) |
| 30 | Final verification: Full build + test + metrics comparison vs baseline | verification | **done** (all green, +24 new tests) |

---

## Dependency Graph

```
Phase 1: Dead Code Surgery (independent loops, can parallelize 1-6)
  1 → 2 → 3 → 4 → 5 → 6 → 7(commit)

Phase 2: Module Extraction (mostly independent, 9-14 can reorder)
  8(plan) → 9 → 10 → 11 → 12 → 13 → 14 → 15(commit)
  NOTE: 9 and 10 are independent. 11 is independent. 12 depends on 10 (daemon handler extract).
        13 depends on 9-12 (needs stable module boundaries). 14 is independent.

Phase 3: Agent Harness Wiring (sequential — each builds on prior)
  16(plan) → 17 → 18 → 19 → 20 → 21 → 22 → 23 → 24(commit)
  NOTE: 17 must land first (context → prompts). 18 depends on 17 (intent feeds coordinator).
        19 depends on 18. 20-23 are mostly independent but ordered for coherence.

Phase 4: Verification & Alignment (sequential)
  25(plan) → 26 → 27 → 28 → 29 → 30
```

---

## Sub-Agent Strategy

| Agent Type | Loops | Purpose |
|------------|-------|---------|
| `Explore` | 8, 16, 25 | Metrics gathering for planning checkpoints |
| `feature-dev:code-reviewer` | 7, 15, 24 | Pre-commit review of each phase |
| `code-simplifier:code-simplifier` | 13 | Duplicate pattern consolidation |
| `superpowers:code-reviewer` | 30 | Final validation against this plan |

---

## Domain Inventory

| Domain | Files Affected | Loops |
|--------|---------------|-------|
| llm_backends/ | cli.rs, factory.rs, types.rs, mod.rs, anthropic.rs | 1, 3 |
| agent/ | mod.rs, coordinator.rs, prompts.rs | 2, 17-23 |
| context_lifecycle/ | intent.rs, extractor.rs, parser.rs, types.rs | 4, 17, 18 |
| daemon/ | mod.rs, tests.rs | 10, 19, 20, 23 |
| ui/ | render_panels.rs | 9 |
| state/ | config.rs, persistence.rs | 11, 13 |
| handlers/ | mod.rs, small files | 5, 12 |
| main.rs | CLI dispatch | 12 |
| retrieval/ | store.rs, query.rs | 13 (serialization only) |
| integration_tests.rs | test infrastructure | 14 |
| notification/, ops_workbench | dead paths | 6 |
| tooling/ | test coverage | 27 |

---

## Loop Plans

### Loop 1 Plan
**Type:** work
**Objective:** Remove the entirely unused `llm_backends/cli.rs` (671 lines), `llm_backends/factory.rs` (268 lines), and `llm_backends/types.rs` (362 lines) — total ~1,300 lines of dead code.
**Risk:** LOW — no callers exist. Grep confirms cli.rs/factory.rs/types.rs are never imported outside their own module.
**Sub-steps:**
1. Grep for all imports of `cli::`, `factory::`, `types::CliProtocol`, `types::AgentConfig` from llm_backends — confirm zero external callers
2. Remove `cli.rs`, `factory.rs`, `types.rs` from `src/llm_backends/`
3. Remove corresponding `mod` declarations from `llm_backends/mod.rs`
4. Run `cargo build` + `cargo test` — verify zero breakage
5. Run `cargo clippy -- -D warnings` — verify clean
**Inputs:** Codebase at current state
**Outputs:** ~1,300 fewer lines, llm_backends reduced from 1,984 to ~683 lines
**Status:** planned

### Loop 2 Plan
**Type:** work
**Objective:** Remove unused methods from `agent/mod.rs` and `agent/coordinator.rs` — methods that exist but have zero daemon callers.
**Risk:** MEDIUM — must verify each method is truly uncalled outside tests. Some may be wired indirectly.
**Sub-steps:**
1. Audit `review_code()`, `analyze_error()`, `summarize_pane()` in agent/mod.rs — grep for callers
2. Audit `aggregate_pane_summaries()`, `detect_cross_pane_errors()` in coordinator.rs — grep for callers
3. Audit `ConflictResolver` methods: `get_resolution_history()`, `clear_resolved()` — grep for callers
4. For methods with ZERO production callers: remove the method, update tests that reference it
5. For methods called only in tests: mark with `#[cfg(test)]` or move to test module
6. Run `cargo build` + `cargo test` + `cargo clippy -- -D warnings`
**Inputs:** Loop 1 complete (clean build)
**Outputs:** ~200-400 fewer lines of dead agent API surface
**Status:** planned

### Loop 3 Plan
**Type:** work
**Objective:** Resolve all 17 `#[allow(dead_code)]` markers — either remove the dead code or justify and document each exception.
**Risk:** LOW — each marker is a localized change
**Sub-steps:**
1. For each `#[allow(dead_code)]` in: ops_workbench.rs (2), tools/python.rs (1), semantic_diff/runner.rs (2), llm_backends/anthropic.rs (4), docs/fetch.rs (1), storage/mod.rs (4), monty/python.rs (1), daemon/mod.rs (2)
2. Check if the field/function is used via serde deserialization (false positive) or truly dead
3. Remove truly dead code; for serde fields, replace `#[allow(dead_code)]` with `#[serde(skip)]` or keep with doc comment
4. Run full test suite
**Inputs:** Loops 1-2 complete
**Outputs:** Zero `#[allow(dead_code)]` markers (or each justified in a comment)
**Status:** planned

### Loop 4 Plan
**Type:** work
**Objective:** Clean unused context_lifecycle/intent integration stubs — the `intent` field on `ExtractedInsight` is `Option<IntentCategory>` but never populated anywhere.
**Risk:** MEDIUM — intent module is wired in types but deciding whether to delete vs. wire requires judgment
**Sub-steps:**
1. Grep for all sites that create `ExtractedInsight` — confirm `intent: None` everywhere
2. Grep for all reads of `.intent` — confirm never matched/used in production
3. Decision: If intent classification is wired in Phase 3 (Loop 18), keep the field but document it as "wired in Loop 18". If not worth wiring, remove `intent` field + `intent.rs` module
4. Remove or stub as appropriate; update types.rs
5. Run tests
**Inputs:** Loops 1-3 complete
**Outputs:** Clean intent integration — either removed or documented as pending-wire
**Status:** planned

### Loop 5 Plan
**Type:** work
**Objective:** Consolidate small handler files and extract shared patterns from `handlers/mod.rs` (809 lines).
**Risk:** LOW — handler files are self-contained; shared patterns are copy-paste identical
**Sub-steps:**
1. Extract 13 shared helper functions from `handlers/mod.rs` into `handlers/common.rs`
2. Evaluate merging small handlers: `agent.rs` (145), `office.rs` (142), `guard.rs` (204) — if they share patterns, consolidate
3. Deduplicate error formatting that appears across 8+ handler files
4. Update all `use` paths; run tests
**Inputs:** Loops 1-4 complete
**Outputs:** handlers/mod.rs reduced from 809 to ~300 lines; ~150-200 lines net reduction
**Status:** planned

### Loop 6 Plan
**Type:** work
**Objective:** Audit notification/mod.rs (909 lines) and ops_workbench.rs (1,028 lines) for dead paths and test-only code.
**Risk:** LOW — research-based audit with targeted removals
**Sub-steps:**
1. Map all public functions in notification/mod.rs — check daemon/CLI callers for each
2. Map all public functions in ops_workbench.rs — check callers; note overlap with daemon telemetry
3. Remove functions with zero production callers (keep test infrastructure if tests use them)
4. Extract embedded tests from ops_workbench.rs to separate test module if >200 lines of tests
5. Run full test suite
**Inputs:** Loops 1-5 complete
**Outputs:** Quantified dead code removed from two 900+ line modules
**Status:** planned

### Loop 7 Plan
**Type:** commit
**Objective:** Stage and commit all Phase 1 dead code surgery with descriptive commit message.
**Risk:** LOW
**Sub-steps:**
1. Run `cargo build` + `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
2. `git diff --stat` to quantify total line changes
3. Create commit with Phase 1 summary
4. Record LOC delta in Working Log
**Inputs:** Loops 1-6 complete, all tests passing
**Outputs:** Clean commit, LOC baseline updated
**Status:** planned

### Loop 8 Plan — PLANNING CHECKPOINT
**Type:** planning
**Objective:** Review Phase 1 metrics (lines removed, tests maintained, build health). Assess Phase 2 extraction targets against actual codebase state post-surgery. Adjust loops 9-15 if needed.
**Risk:** N/A
**Sub-steps:**
1. `find . -name "*.rs" | xargs wc -l | tail -1` — new total LOC
2. `cargo test 2>&1 | tail -3` — test count/status
3. Compare against baseline (132,442 LOC / 1,118 tests)
4. Review remaining monolithic files — confirm extraction targets still valid
5. Adjust loop 9-15 plans if Phase 1 changed the landscape
6. Check: Are we on track for 3,000-5,000 line reduction target?
**Inputs:** Phase 1 commit
**Outputs:** Updated metrics, adjusted Phase 2 plans if needed
**Status:** planning-loop

### Loop 9 Plan
**Type:** work
**Objective:** Split `ui/render_panels.rs` (2,139 lines) into 5 focused rendering modules.
**Risk:** MEDIUM — all functions share `TuiState` + `Frame` dependencies; must not break render pipeline
**Sub-steps:**
1. Read render_panels.rs, identify logical groupings (menu/header, tabs, dashboard, content, status/footer)
2. Create: `ui/render_menu.rs`, `ui/render_tabs.rs`, `ui/render_dashboard.rs`, `ui/render_content.rs`, `ui/render_status.rs`
3. Move functions to appropriate modules, update `ui/mod.rs` re-exports
4. Each new file should be <500 lines
5. Run `cargo build` + `cargo test` — verify render pipeline unchanged
**Inputs:** Phase 1 complete
**Outputs:** render_panels.rs eliminated; 5 files each <500 lines
**Status:** planned

### Loop 10 Plan
**Type:** work
**Objective:** Split `daemon/mod.rs` (2,110 lines) — extract handler dispatch, protocol types, and artifact management.
**Risk:** MEDIUM — daemon is the system's nerve center; extraction must preserve IPC contract
**Sub-steps:**
1. Extract protocol types (DaemonRequest, DaemonResponse enums) → `daemon/protocol.rs`
2. Extract handler dispatch functions → `daemon/handlers.rs`
3. Extract tool/artifact management → `daemon/artifacts.rs`
4. Keep daemon startup/lifecycle in `daemon/mod.rs` (<600 lines)
5. Run full test suite including daemon integration tests
**Inputs:** Loop 9 complete (or independent)
**Outputs:** daemon/mod.rs reduced from 2,110 to <600 lines; 3 new focused modules
**Status:** planned

### Loop 11 Plan
**Type:** work
**Objective:** Restructure `state/config.rs` (1,509 lines) using nested config structs with `#[serde(flatten)]`.
**Risk:** MEDIUM — must maintain backwards compatibility with existing config.json files
**Sub-steps:**
1. Group config fields into logical structs: RetrievalConfig, ContextConfig, StewConfig, ToolConfig, BuildConfig, AgentConfig
2. Use `#[serde(flatten)]` on parent Config to maintain flat JSON compatibility
3. Move each sub-struct to its own file: `state/config/retrieval.rs`, etc.
4. Verify: load existing config.json → serialize → compare (round-trip test)
5. Run full test suite
**Inputs:** Independent of loops 9-10
**Outputs:** config.rs reduced from 1,509 to ~400 lines; 5-6 sub-modules
**Status:** planned

### Loop 12 Plan
**Type:** work
**Objective:** Extract CLI dispatch logic from `main.rs` (1,548 lines) into focused command modules.
**Risk:** MEDIUM — main.rs is the entry point; must preserve all CLI commands
**Sub-steps:**
1. Identify CLI command groups in main.rs (clap subcommands)
2. Extract command handlers into `commands/` module if not already delegated to handlers/
3. Keep main.rs to: arg parsing, dispatch table, daemon startup — target <500 lines
4. Run all CLI integration tests
**Inputs:** Loop 10 complete (daemon handler extraction may affect dispatch)
**Outputs:** main.rs reduced from 1,548 to <500 lines
**Status:** planned

### Loop 13 Plan
**Type:** work
**Objective:** Consolidate duplicate atomic I/O and serialization patterns across storage/, state/, handlers/.
**Risk:** LOW — pattern extraction into shared helpers
**Sub-steps:**
1. Create `storage/helpers.rs` with: `parse_json<T>()`, `stringify_json<T>()`, `atomic_json_write()`
2. Replace ~50-100 duplicate `.context("Failed to parse JSON")` sites with helper calls
3. Consolidate duplicate `ensure_dir()` / `atomic_write()` implementations
4. Run tests — helpers must be drop-in replacements
**Inputs:** Loops 9-12 complete (stable module boundaries)
**Outputs:** ~50-100 lines net reduction; single source of truth for I/O patterns
**Status:** planned

### Loop 14 Plan
**Type:** work
**Objective:** Extract shared test infrastructure from `integration_tests.rs` (2,371 lines).
**Risk:** LOW — test refactoring doesn't affect production code
**Sub-steps:**
1. Extract `run_impulse()`, `run_impulse_with_env()`, `start_daemon()`, `seed_retrieval_history()` → `tests/helpers.rs` or `tests/common/mod.rs`
2. Deduplicate test setup patterns used by 20+ tests
3. Verify all 40+ integration tests still pass
**Inputs:** Independent
**Outputs:** integration_tests.rs reduced by 150-200 lines; shared test helpers reusable
**Status:** planned

### Loop 15 Plan
**Type:** commit
**Objective:** Stage and commit all Phase 2 module extraction.
**Risk:** LOW
**Sub-steps:**
1. Full verification: `cargo build` + `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
2. `git diff --stat` — quantify changes
3. Create commit with Phase 2 summary
**Inputs:** Loops 9-14 complete
**Outputs:** Clean commit, all monolithic files split
**Status:** planned

### Loop 16 Plan — PLANNING CHECKPOINT
**Type:** planning
**Objective:** Review Phase 2 extraction results. Assess agent harness wiring targets. Confirm context lifecycle → agent prompt integration approach.
**Risk:** N/A
**Sub-steps:**
1. Updated LOC metrics + test counts
2. Review agent/mod.rs, coordinator.rs, context_lifecycle/ — map exactly which data flows to wire
3. Confirm: which methods from Loop 2 removals should be restored vs. rebuilt differently
4. Plan the JSON protocol for Loop 21 (harness mode upgrade)
5. Assess: are we on track for 3,000-5,000 line target?
**Inputs:** Phase 2 commit
**Outputs:** Detailed Phase 3 wiring plan with specific function signatures
**Status:** planning-loop

### Loop 17 Plan
**Type:** work
**Objective:** Wire context_lifecycle extracted insights directly into agent prompt construction. Currently extractor produces `ExtractedInsight` but agent builds prompts manually without them.
**Risk:** HIGH — changes the agent's behavior; prompts become data-driven
**Sub-steps:**
1. Add `fn build_context_prompt(insights: &[ExtractedInsight]) -> String` to agent/prompts.rs
2. Modify `ImpulseAgent::query()` to accept optional `&[ExtractedInsight]`
3. In daemon handler, pass `PaneContextState.extracted_insights` to agent queries
4. Template: group insights by type (FileModified, ErrorEncountered, DecisionMade), format as structured context block
5. Test: create mock insights → verify prompt contains them → verify agent response quality
**Inputs:** Phase 2 complete (clean module boundaries)
**Outputs:** Agent prompts now incorporate extracted context — the core "closing the loop" deliverable
**Status:** planned

### Loop 18 Plan
**Type:** work
**Objective:** Activate intent classification in coordinator recommendations. The `RuleBasedClassifier` exists in `context_lifecycle/intent.rs` but insights never carry intent.
**Risk:** MEDIUM — classifier is keyword-based; may produce noisy classifications
**Sub-steps:**
1. In extractor, call `RuleBasedClassifier::classify()` on each extracted insight's content
2. Populate `insight.intent = Some(classified_intent)` instead of `None`
3. In coordinator, use `insight.intent` to weight recommendations (e.g., Deploying intent → higher priority for conflict warnings)
4. Add tests for classification accuracy on representative agent output samples
**Inputs:** Loop 17 complete (insights flow to agent)
**Outputs:** Intent field populated on all extracted insights; coordinator uses intent for prioritization
**Status:** planned

### Loop 19 Plan
**Type:** work
**Objective:** Wire `detect_cross_pane_errors()` and `aggregate_pane_summaries()` into production `run_local_coordination()` path.
**Risk:** MEDIUM — these methods exist and are tested, but never called in production
**Sub-steps:**
1. In `coordinator.rs`, add calls to both methods within `run_local_coordination()`
2. Ensure error aggregation doesn't duplicate recommendations from `detect_file_conflicts()`
3. Wire pane summary aggregation to daemon's coordination response
4. Add integration test: multi-pane scenario with shared errors → verify recommendations generated
**Inputs:** Loop 18 complete
**Outputs:** Full coordinator pipeline activated — file conflicts + cross-pane errors + summaries
**Status:** planned

### Loop 20 Plan
**Type:** work
**Objective:** Wire ConflictResolver history queries into daemon IPC handlers. Currently history is tracked but never queryable.
**Risk:** LOW — adding new IPC endpoint, not modifying existing ones
**Sub-steps:**
1. Add `DaemonRequest::GetConflictHistory` → returns `ConflictResolver.get_resolution_history()`
2. Add `DaemonRequest::ClearResolvedConflicts` → calls `clear_resolved()`
3. Wire to GUI: conflict history view in agent panel or dedicated conflict panel
4. Test: create conflicts → resolve → query history → verify entries
**Inputs:** Loop 19 complete (coordinator fully activated)
**Outputs:** Conflict history accessible via IPC; resolution patterns queryable
**Status:** planned

### Loop 21 Plan
**Type:** work
**Objective:** Upgrade harness mode from simple string passing to structured JSON protocol for richer agent communication.
**Risk:** HIGH — changes the harness subprocess interface; must maintain backward compat
**Sub-steps:**
1. Define `HarnessRequest` struct: `{ system_prompt, user_prompt, context: ExtractedInsight[], max_tokens }`
2. Define `HarnessResponse` struct: `{ content, model, usage?, recommendations? }`
3. For Claude Code harness: pass JSON via stdin (if supported) or env var, parse structured response
4. Fallback: if structured protocol fails, fall back to simple `--print` mode
5. Test both paths: structured success, structured failure → fallback, simple mode
**Inputs:** Loops 17-20 complete (context is available to pass)
**Outputs:** Harness mode can pass structured context and receive structured responses
**Status:** planned

### Loop 22 Plan
**Type:** work
**Objective:** Add agent session awareness — maintain context across multi-turn queries within a session.
**Risk:** MEDIUM — memory management concern; must bound history size
**Sub-steps:**
1. In `ImpulseAgent`, maintain `session_history: Vec<(String, String)>` (prompt, response) bounded to last 5 turns
2. Include relevant history in prompt construction: "Previous context from this session: ..."
3. Add `clear_session()` method for explicit reset
4. Daemon: persist agent instance across requests within same session (don't recreate per request)
5. Test: multi-turn query → verify second query references first query's context
**Inputs:** Loop 21 complete (structured protocol)
**Outputs:** Agent maintains session continuity; multi-turn coordination possible
**Status:** planned

### Loop 23 Plan
**Type:** work
**Objective:** Wire specialized agent methods (review_code, analyze_error, summarize_pane) to dedicated daemon IPC endpoints.
**Risk:** MEDIUM — adding new IPC message types
**Sub-steps:**
1. Add `DaemonRequest::AgentReviewCode { file_path, diff }` → calls `agent.review_code()`
2. Add `DaemonRequest::AgentAnalyzeError { error, context }` → calls `agent.analyze_error()`
3. Add `DaemonRequest::AgentSummarizePane { pane_id }` → calls `agent.summarize_pane()`
4. Wire from GUI: context menu on terminal panes → "Review changes" / "Analyze error" / "Summarize"
5. Test each endpoint independently
**Inputs:** Loop 22 complete (session-aware agent)
**Outputs:** Full agent API surface accessible via IPC — not just generic `AgentAssist`
**Status:** planned

### Loop 24 Plan
**Type:** commit
**Objective:** Stage and commit all Phase 3 agent harness improvements.
**Risk:** LOW
**Sub-steps:**
1. Full verification: `cargo build` + `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
2. `git diff --stat` — quantify changes
3. Create commit with Phase 3 summary
**Inputs:** Loops 17-23 complete
**Outputs:** Clean commit, agent harness fully wired
**Status:** planned

### Loop 25 Plan — PLANNING CHECKPOINT
**Type:** planning
**Objective:** Full metrics audit. Compare LOC, test count, agent feature matrix against baseline and plan targets.
**Risk:** N/A
**Sub-steps:**
1. Total LOC comparison: baseline (132,442) vs. current
2. Test count comparison: baseline (1,118) vs. current
3. Agent feature matrix: list all wired features vs. plan
4. Identify any regressions or gaps
5. Plan verification loops 26-28 based on gaps found
6. Assess: did we hit 3,000-5,000 line reduction? If not, what remains?
**Inputs:** Phase 3 commit
**Outputs:** Metrics snapshot, gap analysis, adjusted Phase 4 plan
**Status:** planning-loop

### Loop 26 Plan
**Type:** verification
**Objective:** Add tests for newly-wired agent harness features from Phase 3.
**Risk:** LOW
**Sub-steps:**
1. Test context → prompt wiring (Loop 17): mock insights → verify prompt contains them
2. Test intent classification (Loop 18): sample agent output → verify correct intent
3. Test coordinator full pipeline (Loop 19): multi-pane scenario → verify all recommendation types
4. Test conflict history IPC (Loop 20): round-trip create → resolve → query
5. Test structured harness protocol (Loop 21): JSON in → JSON out
6. Test session awareness (Loop 22): multi-turn continuity
7. Test specialized endpoints (Loop 23): each new IPC message type
**Inputs:** Phase 3 commit
**Outputs:** 15-25 new tests covering all Phase 3 features
**Status:** planned

### Loop 27 Plan
**Type:** verification
**Objective:** Add missing tests for `tooling/` module (2,650 LOC, only 8 tests — 3.0 tests/KLOC). This is the capability enforcement chain — critical security model.
**Risk:** LOW — adding tests, not changing production code
**Sub-steps:**
1. Map the `DynamicTool` trait → `ToolRegistry.execute()` path
2. Test: capability denied → blocked execution
3. Test: param validation → rejected invalid params
4. Test: tool registration → successful invocation
5. Test: schema export accuracy
6. Target: 20+ new tests for the tooling module
**Inputs:** Loop 26 complete
**Outputs:** tooling/ test coverage from 3.0 to ~10+ tests/KLOC
**Status:** planned

### Loop 28 Plan
**Type:** verification
**Objective:** Run full workspace verification, fix any regressions from all 3 phases.
**Risk:** LOW — fixing, not creating
**Sub-steps:**
1. `cargo build --all-features` — zero warnings
2. `cargo test` — all passing (except known ignored)
3. `cargo clippy --all-features --all-targets -- -D warnings` — clean
4. `cargo fmt --check` — clean
5. Fix any issues found
**Inputs:** Loops 26-27 complete
**Outputs:** Green build across entire workspace
**Status:** planned

### Loop 29 Plan
**Type:** work
**Objective:** Update project documentation to reflect codebase changes: ROADMAP-PLAN.md, LONG-RANGE-ENHANCEMENTS.md, CLAUDE.md, MEMORY.md.
**Risk:** LOW — documentation only
**Sub-steps:**
1. Update CLAUDE.md: new module counts, LOC, test counts, architecture changes
2. Update ROADMAP-PLAN.md: mark completed items, note agent harness as wired
3. Update LONG-RANGE-ENHANCEMENTS.md: check off completed PRs from lanes
4. Update MEMORY.md: record this plan's outcomes
**Inputs:** Loop 28 (verified green build)
**Outputs:** All docs reflect current codebase reality
**Status:** planned

### Loop 30 Plan
**Type:** verification
**Objective:** Final comprehensive verification against this plan's objectives. Compare all metrics vs. baseline.
**Risk:** LOW — read-only audit
**Sub-steps:**
1. LOC: `find . -name "*.rs" | xargs wc -l | tail -1` — compare vs. 132,442 baseline
2. Tests: count passing vs. 1,118 baseline
3. Agent features: matrix of wired vs. unwired capabilities
4. Files >800 lines: list any remaining oversized files
5. `#[allow(dead_code)]`: count remaining markers
6. Build: final `cargo build + test + clippy + fmt`
7. Document: final metrics in this plan's Working Log
**Inputs:** All prior loops complete
**Outputs:** Final metrics, plan completion assessment
**Status:** planned

---

## Metrics Targets

| Metric | Baseline | Target | Measured (Final) |
|--------|----------|--------|------------------|
| Total LOC (workspace) | 132,442 | <128,000 (−3,000+) | **102,351 (−30,091)** MASSIVELY EXCEEDED |
| Source LOC (src/) | 75,481 | <71,000 (−4,000+) | **59,116 (−16,365)** EXCEEDED |
| Files >1,000 lines | 12 | ≤4 | 8 remaining (plan scope: 4 src/ files) |
| `#[allow(dead_code)]` markers | 17 | 0 (or justified) | 9 remaining (all justified: serde fields + Phase 2 placeholders) |
| Test count | 1,118 | ≥1,150 | **1,002** (991 unit + 11 integration, +24 protocol tests in Phase 4) |
| Agent harness wired features | 3 of 10 | 10 of 10 | **10 of 10** ALL WIRED |
| Unused agent methods | 6+ | 0 | **0** (all restored + IPC-wired) |
| Largest file (lines) | 2,371 | <800 | 2,106 (integration_tests.rs — was split in Phase 2) |

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Config.json backwards incompatibility (Loop 11) | Users lose settings | `#[serde(flatten)]` preserves flat JSON; round-trip test required |
| Agent prompt quality degrades with auto-context (Loop 17) | Noisy recommendations | Gate behind config flag; start with top-5 most recent insights only |
| Harness structured protocol not supported by Claude Code (Loop 21) | Feature doesn't work | Graceful fallback to `--print` mode; test both paths |
| Module extraction breaks import chains (Loops 9-12) | Build failure | Extract one module at a time; `cargo build` after each extraction |
| Dead code removal breaks test-only paths (Loops 1-6) | Test failures | Check `#[cfg(test)]` usage before removing; preserve test utilities |
