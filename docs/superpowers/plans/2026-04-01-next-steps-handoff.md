# Next Steps Handoff — Ralph Plan 5 Execution

> **Date:** 2026-04-01
> **From:** Deep code review + Ralph Plan 5 creation session
> **To:** Next session(s) executing the 100-loop plan
> **Plan file:** `ralph-plan-5.md` (root of CLI_CU_L8R)

---

## What Was Done This Session

### 1. Deep Code Review & Remote Alignment
- Verified local `main` is **fully in sync** with `origin/main` (https://github.com/Jimthetaxguy/IMPULSE-rs.git)
- Full build/test/clippy/fmt verification: **all green** (1,025 tests, 0 warnings, 0 diffs)
- Identified 12 uncommitted files: DRY refactor (shared types to impulse-ops) + error handling + GUI fixes

### 2. Audit Findings
- **13 of 19 handler files have zero tests** (CRITICAL gap — 0.8 tests/KLOC vs 2.0 target)
- **State module**: ~80 tests (better than CLAUDE.md's claim of 47)
- **impulse-term**: 90 tests (better than CLAUDE.md's claim of 55)
- **9 TODO refactors** for `clippy::too_many_arguments` param structs
- **TUI correctness issues** from Ralph Plan 4 still pending (unwrap in renderer, unsafe env, backend untested)

### 3. Documentation Updates (10 iterations)
Updated all three contract docs with verified metrics:
- **CLAUDE.md**: LOC (59,356), tests (1,025), handler gap (13/19), state tests (~80), impulse-term (3.7K/90)
- **AGENTS.md**: Test density targets with current values, workspace totals, high-risk module list
- **RUST-CANONICAL-CONTRACT.md**: Gap column, workspace totals, policy compliance audit commands

### 4. Ralph Plan 5 Created
100-loop plan in `ralph-plan-5.md` covering 12 phases with:
- Full iteration table (100 rows)
- Dependency graph with parallelization opportunities
- Sub-agent strategy (which agent types for which loops)
- Metrics targets (baseline → target for 11 metrics)
- First 7 loops detailed with sub-steps

---

## How to Start Next Session

### Quick Start (copy-paste)

```
Execute Ralph Plan 5. Read ralph-plan-5.md. Start with Loop 1 (commit 12 uncommitted files), then execute Loops 2-7 using sub-agents for parallel work. Use /ralph-loop for iteration tracking.
```

### Detailed Instructions

1. **Read the plan**: `ralph-plan-5.md` — focus on Root docs + Phase 1 detailed plans
2. **Start Ralph Loop**: Use `/ralph-loop` with `max_iterations: 100`
3. **Loop 1 (commit)**: Stage + commit + push the 12 uncommitted files
4. **Loops 2-4 (parallel TUI fixes)**:
   - Sub-agent 1: Fix unwrap in `impulse-term/src/renderer.rs`
   - Sub-agent 2: Safe env vars in `impulse-term/src/panel.rs`
   - Sub-agent 3: Backend.rs tests in `impulse-term/src/backend.rs`
5. **Loops 5-7 (parallel handler tests)**:
   - Sub-agent 1: `src/handlers/guard.rs` tests
   - Sub-agent 2: `src/handlers/agent.rs` tests
   - Sub-agent 3: `src/handlers/injection_handlers.rs` tests
6. **Loop 8 (planning checkpoint)**: Gather metrics, verify, plan loops 9-15
7. **Continue through phases** per the dependency graph

### Verification Gate (run after every batch)

```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Expected: 1,025+ tests passed (will grow as loops add tests), 0 warnings, 0 diffs.

---

## Key Files to Read

| File | Purpose |
|------|---------|
| `ralph-plan-5.md` | The 100-loop plan (iteration table, dependencies, sub-agent strategy) |
| `ralph-plan-4.md` | Previous plan (loops 7-24 pending — subsumed into RP5 Phase 1 + Phase 4) |
| `ralph-plan-3.md` | Completed plan (30 loops — codebase reduction + agent harness) |
| `CLAUDE.md` | Project instructions with updated metrics |
| `AGENTS.md` | Agent guidelines with updated test targets |
| `docs/spec/RUST-CANONICAL-CONTRACT.md` | Product contract with policy compliance audit commands |
| `docs/superpowers/plans/2026-03-30-handoff-prompt.md` | Original handoff from RP3 (context for how we got here) |

---

## Parallelization Map (Maximum Speed)

Each phase has identified parallel tracks. To maximize throughput:

| Phase | Parallel Groups | Max Concurrent |
|-------|----------------|----------------|
| 1 (Loops 2-7) | [2,3,4] then [5,6,7] | 3 |
| 2 (Loops 9-14) | [9,10,11] then [12,13,14] | 3 |
| 3 (Loops 17-23) | [17,18,19,20] then [21,22,23] | 3 |
| 4 (Loops 25-30) | [25,26,27,28,29,30] | 3 |
| 5 (Loops 33-38) | [33,34,35] then [36,37,38] | 3 |
| 6 (Loops 41-46) | [41,42] then [43,44] then [45,46] | 2 |
| 7 (Loops 49-56) | [49,50,51] then [52,53,54] then [55,56] | 3 |
| 8 (Loops 59-66) | 59→60→61 then [62,63,64,65,66] | 3 |
| 9 (Loops 69-76) | [69,70,71,72,73] then [74,75,76] | 3 |
| 10 (Loops 79-86) | [79,80,81] then [82,83,84] then 85→86 | 3 |
| 11 (Loops 89-94) | [89,90,91] then [92,93,94] | 3 |

---

## Current State Summary

| Metric | Value |
|--------|-------|
| Build | Clean |
| Tests | 1,025 pass, 3 ignored, 0 fail |
| Clippy | 0 warnings |
| Fmt | 0 diffs |
| Remote | In sync with origin/main |
| Uncommitted | 12 files (+280/-138) — DRY refactor |
| Handler coverage | 6/19 files tested (38 tests) |
| Core density | ~1.5 tests/KLOC |
| Proptest | 0 (not yet adopted) |
| TODO refactors | 9 sites |

---

## Skills to Use

| Skill | When |
|-------|------|
| `ralph-loop:ralph-loop` | Start the loop execution engine |
| `ralph-plan update` | After each loop completes |
| `superpowers:dispatching-parallel-agents` | For parallel sub-agent batches |
| `superpowers:verification-before-completion` | Before each commit loop |
| `superpowers:executing-plans` | For executing the overall plan |
| `rust-programming` | For Rust-specific implementation guidance |
| `rust-testing-patterns` | For test structure and patterns |
| `rust-error-handling` | For .context() and thiserror patterns |
