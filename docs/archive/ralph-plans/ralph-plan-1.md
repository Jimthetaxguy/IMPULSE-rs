# Ralph Plan 1 — Repo Health Audit & Improvement

## Root: Primary Objective

Systematically analyze the Impulse Rust codebase, clean up refactoring artifacts, fix all build/lint/format violations, update stale documentation, and strengthen test coverage — bringing the repo to a production-ready state.

## Root: User Vision

The Impulse codebase has grown to ~55K lines across 35 modules with strong architecture and 875+ passing tests. However, a recent state/UI module split left orphaned files, 76 clippy violations, stale docs, and formatting drift. This plan resolves all technical debt in a single focused sweep, leaving the repo clean, passing all gates, and accurately documented.

## Root: Iteration Contents

| Loop | Focus | Type | Status |
|------|-------|------|--------|
| 1 | Delete orphaned refactoring artifacts (split_*.rs, mod_new.rs, tools_split*, out.txt, scripts) | work | completed |
| 2 | Fix clippy violations — unused imports, duplicate constants, dead code | work | completed |
| 3 | Fix cargo fmt + resolve remaining warnings | work | completed |
| 4 | Fix feature flag defaults + Cargo.toml hygiene | work | completed |
| 5 | Update CLAUDE.md — accurate module counts, line counts, architecture | work | completed |
| 6 | Add tests for context_lifecycle module + coverage gaps | work | completed |
| 7 | Reduce critical unwrap() on production paths (state, tooling, ops_workbench) | work | completed |
| 8 | Planning checkpoint — review progress, adjust remaining loops | planning | completed |
| 9 | Update HANDBOOK.md and stale documentation | work | completed |
| 10 | Final verification — full cargo build + clippy + fmt + test + commit | verification | completed |

---

### Loop 1 Plan
**Type:** work
**Objective:** Remove all orphaned refactoring artifacts from the incomplete state/UI module split
**Risk:** LOW — deleting untracked files that are not imported anywhere
**Sub-steps:**
1. Verify none of these files are imported/used: `split_config.rs`, `split_persistence.rs`, `split_session.rs`, `split_header.txt`, `split_state.sh`, `mod_new.rs`, `tools_split.rs`, `tools_split` dir, `tools_split_ui.py`, `out.txt`
2. Delete all confirmed orphaned files
3. Verify `cargo build` still succeeds after deletion
4. Run `cargo test` to confirm no regressions
**Inputs:** git status showing untracked files
**Outputs:** Clean working tree (only `.claude/` and `archive/` duplicates remain untracked)
**Status:** planned

### Loop 2 Plan
**Type:** work
**Objective:** Fix all 76 clippy violations to restore `cargo clippy -- -D warnings` to green
**Risk:** MEDIUM — import changes could break compilation if done incorrectly
**Sub-steps:**
1. Run `cargo clippy -- -D warnings 2>&1` to get full error list
2. Fix unused imports in `src/state/config.rs`, `persistence.rs`, `session.rs`
3. Remove duplicate constants (`LIVE_STATE_FILE`, `HISTORY_FILE`, `CONFIG_FILE`) — keep only in one canonical location
4. Fix unused imports in `src/ui/agent_terminal.rs`, `lifecycle.rs`, `render_panels.rs`, `runner.rs`, `types.rs`
5. Fix glob re-export issues in `src/handlers/mod.rs`
6. Run `cargo clippy -- -D warnings` to verify zero violations
7. Run `cargo test` to confirm no regressions
**Inputs:** Clean artifact state from Loop 1
**Outputs:** Zero clippy warnings
**Status:** planned

### Loop 3 Plan
**Type:** work
**Objective:** Fix all cargo fmt violations and resolve any remaining warnings
**Risk:** LOW — formatting changes don't affect behavior
**Sub-steps:**
1. Run `cargo fmt` across workspace
2. Run `cargo fmt --check` to verify clean
3. Check for any remaining compiler warnings with `cargo build 2>&1`
4. Fix any `#[allow(unused_imports)]` that are no longer needed
5. Run full `cargo test` to confirm no regressions
**Inputs:** Clean clippy state from Loop 2
**Outputs:** Zero fmt diffs, zero compiler warnings
**Status:** planned

### Loop 4 Plan
**Type:** work
**Objective:** Fix feature flag defaults and Cargo.toml hygiene
**Risk:** MEDIUM — changing default features affects downstream builds
**Sub-steps:**
1. Read `impulse-rs/Cargo.toml` for current feature configuration
2. Set `default = ["office-support"]` to match CLAUDE.md/MEMORY.md claims
3. Verify build with default features: `cargo build`
4. Verify build without: `cargo build --no-default-features`
5. Check for unused dependencies: `cargo +nightly udeps` or manual audit
6. Run `cargo test` with default features
**Inputs:** Clean build from Loop 3
**Outputs:** Correct feature flag defaults, verified both feature paths compile
**Status:** planned

### Loop 5 Plan
**Type:** work
**Objective:** Update CLAUDE.md with accurate module counts, line counts, and current architecture
**Risk:** LOW — documentation-only changes
**Sub-steps:**
1. Count actual modules: `ls -d impulse-rs/src/*/` and check main.rs declarations
2. Count total lines: `find impulse-rs/src -name "*.rs" | xargs wc -l`
3. Update CLAUDE.md: module count (35), line count (~55K), crate descriptions
4. Update test counts in CLAUDE.md
5. Ensure Build & Test section reflects current workspace structure
6. Update MEMORY.md project memory with current stats
**Inputs:** Accurate codebase from Loops 1-4
**Outputs:** CLAUDE.md accurately reflects current codebase state
**Status:** planned

### Loop 6 Plan
**Type:** work
**Objective:** Add test coverage for context_lifecycle module and other gaps
**Risk:** MEDIUM — new tests may reveal bugs that need fixing
**Sub-steps:**
1. Identify modules with zero tests: context_lifecycle/mod.rs, injection/mod.rs, semantic_diff/mod.rs
2. Add unit tests for `context_lifecycle` — monitor thresholds, extractor output, injector behavior
3. Add integration-level tests for context lifecycle flow (spawn → monitor → extract → inject)
4. Verify existing tests still pass: `cargo test`
5. Report final test count delta
**Inputs:** Clean, documented codebase from Loop 5
**Outputs:** context_lifecycle has test coverage, test count increases
**Status:** planned

### Loop 7 Plan
**Type:** work
**Objective:** Convert critical unwrap() calls on production paths to proper Result handling
**Risk:** MEDIUM — changing error handling can affect control flow
**Sub-steps:**
1. Identify highest-risk unwrap() calls: `grep -rn "\.unwrap()" impulse-rs/src/ | grep -v test | grep -v "#\[cfg(test)\]"` — focus on state/, tooling/, ops_workbench.rs
2. Prioritize: file I/O unwraps > parsing unwraps > constructor unwraps
3. Convert top 20-30 critical unwraps to `?` or `.context("reason")?`
4. Run `cargo clippy -- -D warnings` and `cargo test` after each batch
5. Document which unwraps were intentionally left (e.g., mutex locks that indicate programming errors)
**Inputs:** Test-covered codebase from Loop 6
**Outputs:** Critical production paths use proper error handling
**Status:** planned

### Loop 8 Plan
**Type:** planning
**Objective:** Review progress through Loops 1-7, assess remaining work, adjust Loops 9-10
**Risk:** LOW — read-only analysis
**Sub-steps:**
1. Run full verification suite: `cargo build && cargo clippy -- -D warnings && cargo fmt --check && cargo test`
2. Count remaining issues: clippy warnings, fmt diffs, test failures, unwrap counts
3. Compare against Loop 1 baseline
4. Assess if Loop 9 (HANDBOOK.md) and Loop 10 (final verification) are still the right priorities
5. Update Iteration Contents with actual status
**Inputs:** All previous loop Working Logs
**Outputs:** Metrics snapshot, updated plan for Loops 9-10
**Status:** planned

### Loop 9 Plan
**Type:** work
**Objective:** Update HANDBOOK.md and remaining stale documentation
**Risk:** LOW — documentation-only changes
**Sub-steps:**
1. Read current HANDBOOK.md sections
2. Update module architecture descriptions to reflect state/UI split
3. Update any references to old module structure
4. Check docs/ directory for stale content
5. Verify documentation links are valid
**Inputs:** Planning checkpoint from Loop 8
**Outputs:** All documentation accurately reflects current codebase
**Status:** planned

### Loop 10 Plan
**Type:** verification
**Objective:** Final end-to-end verification and commit
**Risk:** LOW — verification only
**Sub-steps:**
1. Full gate check: `cargo build && cargo clippy -- -D warnings && cargo fmt --check && cargo test`
2. Run workspace builds: `cd impulse-term && cargo test && cd ../impulse-gui && cargo test`
3. `git diff --stat` to review all changes
4. Stage and commit with descriptive message
5. Final `git status` to confirm clean state
**Inputs:** All improvements from Loops 1-9
**Outputs:** Clean commit with all improvements, all gates passing
**Status:** planned
