# Handoff Prompt — Ralph Plan 3: Codebase Reduction & Agent Harness Improvement

> Copy everything below the line into a new session or /handoff.

---

## Context

You are executing **Ralph Plan 3** for the Impulse project (`impulse-rs`). Impulse is a Rust sidecar that runs alongside AI coding agents (Claude Code, Codex, OpenCode) and remembers what they did across sessions.

**Two plan documents exist — read both before starting:**
1. `ralph-plan-3.md` — Ralph Loop structure (iteration table, dependency graph, metrics targets, risk register)
2. `docs/superpowers/plans/2026-03-30-codebase-reduction-agent-harness.md` — Full implementation plan (30 tasks, 130+ bite-sized steps, exact file paths, code blocks, parallelization map)

Use `/superpowers:executing-plans` or `/superpowers:subagent-driven-development` to execute task-by-task.

---

## Primary Objective

Reduce the Impulse Rust codebase by **3,000–5,000 lines** through dead code elimination, module consolidation, and duplicate pattern extraction — while simultaneously wiring the agent harness's unused-but-implemented features into a fully connected intelligence loop.

---

## The Two Problems Being Solved

### Problem 1: Dead Code & Bloat (132K LOC across 233 .rs files)

The codebase contains:
- **~1,300 lines of entirely dead files** in `src/llm_backends/` — `cli.rs` (671 lines), `factory.rs` (268 lines), `types.rs` (362 lines) are never imported by any production code
- **~200-400 lines of unused agent methods** — `review_code()`, `analyze_error()`, `coordinate_llm()`, `summarize_pane()` in `src/agent/mod.rs` have zero daemon callers
- **17 `#[allow(dead_code)]` markers** hiding real waste
- **6 monolithic files >1,000 lines** — `render_panels.rs` (2,139), `integration_tests.rs` (2,371), `daemon/mod.rs` (2,110), `state/config.rs` (1,509), `main.rs` (1,548), `ops_workbench.rs` (1,028)
- **Duplicate serialization patterns** — 200+ sites with identical `.context("Failed to parse JSON")` wrappers

### Problem 2: Disconnected Agent Harness

The agent harness has fully-implemented subsystems that are never connected:
- **Context lifecycle extractor** produces `ExtractedInsight` structs — but the **agent builds prompts manually without them**
- **Intent classification** (`IntentCategory::from_keywords()`) exists — but `insight.intent` is **always `None`** everywhere
- **ConflictResolver** tracks resolution history — but **nobody queries it** via IPC
- **Coordinator** has `aggregate_pane_summaries()` — but it's **never called in production**
- **Specialized agent methods** (review_code, analyze_error, summarize_pane) exist — but have **no daemon IPC endpoints**
- **Harness mode** passes simple strings — no structured JSON, no context injection

---

## Four Phases (30 Loops Total)

### Phase 1: Dead Code Surgery (Loops 1–7) — Target: −2,000+ lines

| Loop | What to Do | Key Files | Risk |
|------|-----------|-----------|------|
| 1 | Delete `src/llm_backends/{cli,factory,types}.rs` (1,300 lines). Verify zero callers first. Remove `#![allow(dead_code)]` from `llm_backends/mod.rs`. | `llm_backends/cli.rs`, `factory.rs`, `types.rs`, `mod.rs` | LOW |
| 2 | Gate unused agent methods behind `#[cfg(test)]` — `review_code()`, `analyze_error()`, `coordinate_llm()`, `summarize_pane()`. Also audit `ConflictResolver::get_resolution_history()`, `clear_resolved()`. **Do NOT delete** — Phase 3 re-enables them. | `agent/mod.rs`, `agent/coordinator.rs` | MEDIUM |
| 3 | Resolve all 17 `#[allow(dead_code)]` markers. For each: serde deserialization field → document. Truly dead → remove. Test-only → `#[cfg(test)]`. | `ops_workbench.rs`, `llm_backends/anthropic.rs`, `storage/mod.rs`, `daemon/mod.rs`, + 4 more | LOW |
| 4 | Document `ExtractedInsight.intent` field as "wired in Phase 3 Task 18". Audit `intent.rs` types — gate unused ones behind `#[cfg(test)]`. | `context_lifecycle/types.rs`, `context_lifecycle/intent.rs` | MEDIUM |
| 5 | Extract 13 shared helper functions from `handlers/mod.rs` (809 lines) into `handlers/common.rs`. Deduplicate error formatting across 8+ handler files. | `handlers/mod.rs`, `handlers/common.rs` (new) | LOW |
| 6 | Audit `notification/mod.rs` (909 lines) and `ops_workbench.rs` (1,028 lines) for dead paths — map every pub fn, check callers, remove or gate. | `notification/mod.rs`, `ops_workbench.rs` | LOW |
| 7 | **COMMIT.** Run: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`. Record LOC delta. | — | LOW |

**Parallelization:** Tasks 1, 3, 4, 6 share ZERO files — run them as 4 parallel subagents. Task 2 must complete before Task 5 (handlers import agent/). Task 7 depends on all.

### Phase 2: Module Extraction (Loops 8–15) — Target: No file >800 lines

| Loop | What to Do | Key Files | Risk |
|------|-----------|-----------|------|
| 8 | **PLANNING CHECKPOINT.** Measure LOC, test count. Verify extraction targets are still valid post-Phase 1. | — | — |
| 9 | Split `render_panels.rs` (2,139 lines) → `render_menu.rs`, `render_tabs.rs`, `render_dashboard.rs`, `render_content.rs`, `render_status.rs`. Update `ui/mod.rs` re-exports. | `ui/render_panels.rs` (delete), 5 new files, `ui/mod.rs` | MEDIUM |
| 10 | Split `daemon/mod.rs` (2,110 lines) → extract `daemon/protocol.rs` (DaemonRequest/Response enums), `daemon/handlers.rs` (handler functions). Keep lifecycle in mod.rs (~600 lines). | `daemon/mod.rs`, `daemon/protocol.rs` (new), `daemon/handlers.rs` (new) | MEDIUM |
| 11 | Restructure `state/config.rs` (1,509 lines) with nested sub-structs (`RetrievalConfig`, `ContextConfig`, `StewardshipConfig`, `ToolConfig`, `BuildHygieneConfig`, `AgentConfig`). Use `#[serde(flatten)]` for backwards-compatible flat JSON. **MUST add round-trip test.** | `state/config.rs` → `state/config/mod.rs` + sub-modules | MEDIUM |
| 12 | Extract inline CLI command handlers from `main.rs` (1,548 lines) to handler modules. Target: main.rs <500 lines. Depends on Task 10 (daemon handler extraction). | `main.rs`, `handlers/` | MEDIUM |
| 13 | Create `storage/helpers.rs` with `parse_json<T>()`, `to_json_pretty<T>()`. Replace highest-frequency duplicate sites (50-100 identical patterns). Depends on 9-12 (stable module boundaries). | `storage/helpers.rs` (new), multiple consumer files | LOW |
| 14 | Extract shared test helpers from `integration_tests.rs` (2,371 lines) — `run_impulse()`, `start_daemon()`, `seed_retrieval_history()`. Deduplicate 20+ tests' setup patterns. | `integration_tests.rs` | LOW |
| 15 | **COMMIT.** Full verification. Record LOC delta. Check remaining files >800 lines. | — | LOW |

**Parallelization:** Tasks 9, 11, 14 share ZERO files — run in parallel. Task 10 is independent of 9/11. Task 12 depends on 10. Task 13 depends on all (9-12).

### Phase 3: Agent Harness Wiring (Loops 16–24) — Close the Intelligence Loop

| Loop | What to Do | Key Files | Risk |
|------|-----------|-----------|------|
| 16 | **PLANNING CHECKPOINT.** Review agent module state. Map the exact data flow to wire: extractor → insights → prompts → coordinator → IPC. | — | — |
| 17 | **CORE TASK: Wire context → prompts.** Add `build_context_prompt(insights)` to `agent/prompts.rs`. Add `query_with_context()` to `ImpulseAgent`. Update daemon handler to pass `PaneContextState.extracted_insights` to agent. Write test first (TDD). | `agent/prompts.rs`, `agent/mod.rs`, `daemon/handlers.rs` | HIGH |
| 18 | **Activate intent classification.** At every `ExtractedInsight` creation site in `extractor.rs`, call `IntentCategory::from_keywords()` to populate `insight.intent` (currently always `None`). Update coordinator to use intent for recommendation priority. | `context_lifecycle/extractor.rs`, `agent/coordinator.rs` | MEDIUM |
| 19 | **Wire coordinator production paths.** Verify `run_local_coordination()` calls `detect_cross_pane_errors()` (research suggests it already does — verify). Wire `aggregate_pane_summaries()` into daemon coordination flow. | `agent/coordinator.rs`, `daemon/handlers.rs` | MEDIUM |
| 20 | Add `GetConflictHistory` and `ClearResolvedConflicts` IPC endpoints to daemon. Wire to `ConflictResolver.get_resolution_history()` / `clear_resolved()`. | `daemon/protocol.rs`, `daemon/handlers.rs` | LOW |
| 21 | Upgrade harness mode: define `HarnessRequest`/`HarnessResponse` JSON structs. Try structured stdin first, fallback to `--print`. Test both paths. | `agent/mod.rs` | HIGH |
| 22 | Add `session_history: Vec<(String, String)>` to `ImpulseAgent` (bounded to 5 turns). Include history in prompt construction. Add `clear_session()`. | `agent/mod.rs` | MEDIUM |
| 23 | Remove `#[cfg(test)]` gates from Task 2. Add `AgentReviewCode`, `AgentAnalyzeError`, `AgentSummarizePane` IPC endpoints. | `agent/mod.rs`, `daemon/protocol.rs`, `daemon/handlers.rs` | MEDIUM |
| 24 | **COMMIT.** Full verification. | — | LOW |

**Parallelization:** Tasks 17→18→19 are SEQUENTIAL (each builds on prior). After 19 lands, Tasks 20/21/22/23 are INDEPENDENT — run as 4 parallel subagents.

### Phase 4: Verification & Alignment (Loops 25–30)

| Loop | What to Do | Key Files | Risk |
|------|-----------|-----------|------|
| 25 | **PLANNING CHECKPOINT.** Full metrics audit: LOC delta, test delta, agent feature matrix (7 features wired vs. baseline 3). | — | — |
| 26 | Add 15-25 tests for Phase 3 features: context→prompt pipeline, intent accuracy, coordinator full pipeline, conflict history round-trip, structured harness fallback, session awareness, specialized IPC. | Agent + daemon test modules | LOW |
| 27 | Add 20+ tests for `tooling/` module (2,650 LOC, only 8 tests = 3.0/KLOC). Test: capability enforcement, param validation, tool registration, schema export. Critical security model. | `tooling/` test modules | LOW |
| 28 | Full workspace verification: `cargo build --all-features && cargo test && cargo clippy --all-features --all-targets -- -D warnings && cargo fmt --check`. Fix any regressions. | All files | LOW |
| 29 | Update docs: `CLAUDE.md` (module counts, LOC, architecture), `ROADMAP-PLAN.md` (mark completed items), `MEMORY.md` (record outcomes). | Docs | LOW |
| 30 | Final metrics comparison vs baseline. Record in `ralph-plan-3.md` Working Log. | `ralph-plan-3.md` | LOW |

**Parallelization:** Tasks 26 and 27 share ZERO files — run in parallel.

---

## Metrics Targets

| Metric | Baseline | Target |
|--------|----------|--------|
| Total LOC (workspace) | 132,442 | <128,000 (−3,000+) |
| Source LOC (src/) | 75,481 | <71,000 (−4,000+) |
| Files >1,000 lines | 12 | ≤4 |
| `#[allow(dead_code)]` markers | 17 | 0 (or each justified) |
| Test count | 1,118 | ≥1,150 (net gain) |
| Agent harness wired features | 3 of 10 | 10 of 10 |
| Unused agent methods | 6+ | 0 |
| Largest file (lines) | 2,371 | <800 |

---

## Critical Rules

1. **Always verify before deleting.** Grep for callers before removing any function/file. The earlier research had errors (e.g., coordinator methods reported as uncalled were actually called).
2. **`cargo build && cargo test && cargo clippy -- -D warnings` after every task.** No exceptions.
3. **Config.rs restructure (Task 11) needs a round-trip test.** Existing `config.json` files must deserialize identically after restructure. Use `#[serde(flatten)]` + exact field name matching.
4. **Agent methods gated in Task 2 are restored in Task 23.** Don't delete them — they're temporarily `#[cfg(test)]` gated.
5. **Commit at phase boundaries only** (Loops 7, 15, 24). Don't commit mid-phase.
6. **Each phase should leave the codebase in a buildable, testable state.** Never leave broken imports across a phase boundary.

---

## Workspace Structure

```
impulse-rs/                    ← Main crate (CLI + daemon + TUI, ~53K lines)
├── src/
│   ├── main.rs                ← CLI entry + dispatch (1,548 lines)
│   ├── agent/                 ← Agent harness (mod.rs, coordinator.rs, prompts.rs)
│   ├── llm_backends/          ← LLM providers (mod.rs, anthropic.rs, cli.rs*, factory.rs*, types.rs*)
│   ├── context_lifecycle/     ← Context extraction (extractor.rs, intent.rs, types.rs)
│   ├── daemon/                ← Unix socket daemon (mod.rs 2,110 lines, tests.rs)
│   ├── handlers/              ← CLI command handlers (14 files, mod.rs 809 lines)
│   ├── state/                 ← Config + persistence (config.rs 1,509 lines)
│   ├── ui/                    ← TUI rendering (render_panels.rs 2,139 lines)
│   ├── tooling/               ← Dynamic tool registry (2,650 lines, 8 tests)
│   ├── notification/          ← Notification system (mod.rs 909 lines)
│   ├── ops_workbench.rs       ← Telemetry ops (1,028 lines)
│   ├── integration_tests.rs   ← Integration tests (2,371 lines)
│   └── ... (35 modules total)
│
├── impulse-ops/               ← Shared types (SupervisorAction, OpsSnapshot, 882 lines)
├── impulse-term/              ← Terminal widget (PTY + vt100, ~2,700 lines)
└── impulse-gui/               ← egui native workbench (~14,600 lines)

*Files marked with * are dead code targeted for deletion in Task 1.
```

---

## How to Start

1. Read `ralph-plan-3.md` for the iteration table and dependency graph
2. Read `docs/superpowers/plans/2026-03-30-codebase-reduction-agent-harness.md` for the full step-by-step plan with code blocks
3. Start with Phase 1. For maximum speed, dispatch Tasks 1, 3, 4, 6 as parallel subagents (they share zero files)
4. After each task: `cargo build && cargo test && cargo clippy -- -D warnings`
5. At planning checkpoints (Loops 8, 16, 25): gather metrics, compare vs. baseline, adjust if needed
6. Update `ralph-plan-3.md` Iteration Contents status column as you complete each loop
