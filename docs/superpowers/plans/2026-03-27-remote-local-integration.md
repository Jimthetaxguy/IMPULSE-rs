# Remote ↔ Local Integration Plan — IMPULSE-rs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate `origin/main` (6 commits ahead from PR #7 merge + PR #8) into the local workspace, verify build health, clean up artifacts, and validate all new modules compile and test correctly.

**Architecture:** This is a clean fast-forward integration — local `feature/ralph-plan-sweep` is the merge base for `origin/main`. No conflicts exist. The integration introduces a new `delegation/` module, structured output parser, extended impulse-ops types, and 4 new daemon IPC variants. Post-merge verification ensures everything compiles, tests pass, and duplicate macOS files are cleaned.

**Tech Stack:** Rust workspace (cargo), git (fast-forward merge), clippy, rustfmt

---

## Analysis Summary

### What's on remote `main` but not local

| Commit | Description | Impact |
|--------|-------------|--------|
| `ab016ea` | Merge PR #7 (feature/ralph-plan-sweep) | Merges current branch into main |
| `5640cae` | impulse-ops shared types for OpenSquirrel | 129 LOC added to `impulse-ops/src/lib.rs` — AgentStatus, AgentRole, DiffSummary, DelegationSummary, MachineTarget |
| `6814eea` | Structured parser + delegation module + orchestration | **1,590 LOC added** — `parser.rs` (656), `delegation/` (661), extractor refactored, session model extended, 4 new daemon IPC variants |
| `6d4bb9a` | Agent harness analysis doc update | Docs only — desloppify v0.9.10 comparison |
| `df8120c` | Long-range enhancement roadmap | Docs only — 522 LOC, 33 PRs across 8 lanes |
| `c3873ea` | Merge PR #8 | Merge commit |

### What's local but not remote

- **Untracked `.claude/` directory** — Claude Code project memory (not for git)
- **7 macOS duplicate files** (`" 2."` suffix) — Finder copy artifacts, safe to delete
- **No uncommitted code changes** — working tree is clean for tracked files

### Conflict Risk: **NONE**

Merge base `438ea49` = HEAD of current branch. `origin/main` is a strict fast-forward superset.

---

### Task 1: Clean Up macOS Duplicate Artifacts

**Files:**
- Delete: `archive/harness/package 2.json`
- Delete: `archive/harness/tsconfig 2.json`
- Delete: `archive/harness/vitest.config 2.ts`
- Delete: `docs/guides/HOOK-VALIDATION-GUIDE 2.md`
- Delete: `impulse-rs/src/ops_workbench 2.rs`
- Delete: `impulse-rs/src/tooling/executor 2.rs`
- Delete: `impulse-rs/src/tooling/external 2.rs`

These are macOS Finder duplicate files (identical to originals — verified via `diff`). They are untracked and will never compile or be used.

- [ ] **Step 1: Verify each duplicate matches its original**

```bash
cd <legacy-worktree>
diff "archive/harness/package 2.json" "archive/harness/package.json"
diff "archive/harness/tsconfig 2.json" "archive/harness/tsconfig.json"
diff "archive/harness/vitest.config 2.ts" "archive/harness/vitest.config.ts"
diff "docs/guides/HOOK-VALIDATION-GUIDE 2.md" "docs/guides/HOOK-VALIDATION-GUIDE.md"
diff "impulse-rs/src/ops_workbench 2.rs" "impulse-rs/src/ops_workbench.rs"
diff "impulse-rs/src/tooling/executor 2.rs" "impulse-rs/src/tooling/executor.rs"
diff "impulse-rs/src/tooling/external 2.rs" "impulse-rs/src/tooling/external.rs"
```

Expected: All empty output (identical files).

- [ ] **Step 2: Delete all duplicate files**

```bash
rm "archive/harness/package 2.json"
rm "archive/harness/tsconfig 2.json"
rm "archive/harness/vitest.config 2.ts"
rm "docs/guides/HOOK-VALIDATION-GUIDE 2.md"
rm "impulse-rs/src/ops_workbench 2.rs"
rm "impulse-rs/src/tooling/executor 2.rs"
rm "impulse-rs/src/tooling/external 2.rs"
```

- [ ] **Step 3: Verify clean**

```bash
git status --short
```

Expected: Only `.claude/` directory remains as untracked (which is correct — it's Claude Code config, not project code).

---

### Task 2: Fast-Forward Local `main` to Remote `main`

**Files:**
- No manual file changes — git merge handles everything

The current branch (`feature/ralph-plan-sweep`) has been merged to remote main via PR #7. Remote main has 6 additional commits. We need to bring local main up to date.

- [ ] **Step 1: Switch to local main branch**

```bash
git checkout main
```

Expected: `Switched to branch 'main'`

- [ ] **Step 2: Fast-forward merge remote main**

```bash
git merge origin/main --ff-only
```

Expected:
```
Updating 23e0d23..c3873ea
Fast-forward
 docs/INDEX.md                                 |   1 +
 docs/LONG-RANGE-ENHANCEMENTS.md               | 522 +++
 ...
 20 files changed, 2411 insertions(+), 169 deletions(-)
```

The `--ff-only` flag ensures this is a clean fast-forward. If it fails (shouldn't), it means local main has diverged and needs manual investigation.

- [ ] **Step 3: Verify local main matches remote**

```bash
git log --oneline -5
git diff origin/main
```

Expected: Log shows `c3873ea` at HEAD. Diff is empty.

---

### Task 3: Delete Merged Feature Branch

**Files:**
- No file changes — branch management only

The `feature/ralph-plan-sweep` branch is fully merged into main (both local and remote). Keeping it around adds clutter.

- [ ] **Step 1: Verify branch is merged**

```bash
git branch --merged main | grep ralph-plan-sweep
```

Expected: `feature/ralph-plan-sweep` appears in the list (confirming it's merged).

- [ ] **Step 2: Delete local branch**

```bash
git branch -d feature/ralph-plan-sweep
```

Expected: `Deleted branch feature/ralph-plan-sweep (was 438ea49).`

- [ ] **Step 3: Optionally delete remote branch**

```bash
git push origin --delete feature/ralph-plan-sweep
```

Expected: `To https://github.com/Jimthetaxguy/IMPULSE-rs.git - [deleted] feature/ralph-plan-sweep`

Note: Only do this if you're sure no one else is working from this branch.

- [ ] **Step 4: Clean up other fully-merged remote branches**

Check which remote branches are merged:

```bash
git branch -r --merged main
```

Candidates for deletion (all merged): `origin/claude/add-semantic-diffs-a5bIF`, `origin/claude/compare-opensquirrel-repo-hLr1f`, `origin/audit/codebase-strengthening`, `origin/feature/identity-memory-loop-audit`, `origin/feature/gui-roadmap`, `origin/feature/conflict-detection-notification-quality`

Only delete after confirming with the user — these are historical branches.

---

### Task 4: Build Verification — Full Workspace

**Files:**
- Verify: `impulse-rs/` (main crate)
- Verify: `impulse-rs/impulse-ops/` (shared types — newly extended)
- Verify: `impulse-rs/impulse-term/` (terminal widget)
- Verify: `impulse-rs/impulse-gui/` (GUI workbench)

- [ ] **Step 1: Build the full workspace**

```bash
cd impulse-rs && cargo build 2>&1
```

Expected: Zero errors, zero warnings. Watch especially for:
- `impulse-ops` new types (`AgentStatus`, `AgentRole`, `DiffSummary`, `DelegationSummary`, `MachineTarget`) compiling
- `delegation/` module (new: `mod.rs`, `types.rs`, `detector.rs`, `tracker.rs`) compiling
- `context_lifecycle/parser.rs` (new, 656 LOC) compiling
- Refactored `extractor.rs` compiling against new `parser` module

- [ ] **Step 2: Run clippy with deny warnings**

```bash
cargo clippy -- -D warnings 2>&1
```

Expected: Zero warnings. The PR #8 commit message claims "clippy clean" — verify that holds with current toolchain.

- [ ] **Step 3: Check formatting**

```bash
cargo fmt --check 2>&1
```

Expected: Zero diffs.

- [ ] **Step 4: Run all tests**

```bash
cargo test 2>&1
```

Expected: ~863+ tests pass (PR #8 commit claims 863). Known: 2 ignored tests (embedding subprocess race, pre-existing), 2 flaky integration tests (daemon socket race under concurrent load — pass individually).

Track the exact count — if it's significantly different from 863, investigate.

- [ ] **Step 5: Build individual subcrates**

```bash
cd impulse-term && cargo build && cargo test && cargo clippy -- -D warnings 2>&1
cd ../impulse-gui && cargo build && cargo clippy -- -D warnings 2>&1
```

Expected: All clean. impulse-gui may reference new `impulse-ops` types transitively.

---

### Task 5: Validate New Modules — Smoke Test

**Files:**
- Read: `impulse-rs/src/delegation/types.rs` — DelegationSpec, DelegationState, TrackedDelegation
- Read: `impulse-rs/src/delegation/detector.rs` — detect_delegation(), detect_delegation_natural()
- Read: `impulse-rs/src/delegation/tracker.rs` — DelegationTracker
- Read: `impulse-rs/src/context_lifecycle/parser.rs` — LineClassification, parse_output()
- Read: `impulse-rs/impulse-ops/src/lib.rs` — new shared types

- [ ] **Step 1: Verify delegation module tests pass in isolation**

```bash
cd <legacy-worktree>/impulse-rs
cargo test delegation:: -- --nocapture 2>&1
```

Expected: All delegation tests pass (detector tests: JSON block parsing, natural language detection, edge cases; tracker tests: lifecycle management).

- [ ] **Step 2: Verify parser module tests pass in isolation**

```bash
cargo test context_lifecycle::parser:: -- --nocapture 2>&1
```

Expected: Parser tests pass — line classification for diffs, code fences, headings, errors, tool invocations, delegation markers.

- [ ] **Step 3: Verify extractor still works with new parser integration**

```bash
cargo test context_lifecycle::extractor:: -- --nocapture 2>&1
```

Expected: Extractor tests pass. The extractor was refactored to use the structured parser — verify it produces the same insight types (FileModified, ToolInvocation, DiffDetected, ErrorDetected, etc.).

- [ ] **Step 4: Verify new daemon IPC variants**

```bash
cargo test daemon:: -- --nocapture 2>&1
```

Expected: Daemon tests pass. The 4 new IPC variants (RegisterDelegation, CompleteDelegation, ListDelegations, GetAgentPool) have stub handlers — verify they return responses without panicking.

- [ ] **Step 5: Verify session model extensions**

```bash
cargo test state::session:: -- --nocapture 2>&1
```

Expected: Session tests pass. New fields (`role`, `parent_session_id`, `delegation_id`, `target`) are `#[serde(default)]` so they shouldn't break existing serialization.

---

### Task 6: Verify Documentation Integrity

**Files:**
- Read: `docs/LONG-RANGE-ENHANCEMENTS.md` — 33 PRs across 8 lanes
- Read: `docs/research/AGENT-HARNESS-ANALYSIS.md` — updated with desloppify
- Read: `docs/INDEX.md` — cross-references updated
- Read: `docs/ROADMAP-PLAN.md` — cross-references updated

- [ ] **Step 1: Verify doc cross-references are valid**

```bash
cd <legacy-worktree>
# Check that files referenced in LONG-RANGE-ENHANCEMENTS.md exist
head -30 docs/LONG-RANGE-ENHANCEMENTS.md
ls docs/ROADMAP-PLAN.md docs/HONEST-ROADMAP.md docs/spec/RUST-CANONICAL-CONTRACT.md 2>&1
```

Expected: All referenced documents exist.

- [ ] **Step 2: Verify INDEX.md includes new entry**

```bash
grep -i "long-range" docs/INDEX.md
```

Expected: Entry for LONG-RANGE-ENHANCEMENTS.md present.

---

### Task 7: Update Project Memory

**Files:**
- Modify: `.claude/projects/<project-slug>/memory/MEMORY.md`

- [ ] **Step 1: Update MEMORY.md with integration results**

Add entry documenting:
- Integration date (2026-03-27)
- What was integrated (PR #7 merge + PR #8: OpenSquirrel feature integration)
- New modules: `delegation/` (types, detector, tracker), `parser.rs`
- Extended types: impulse-ops (AgentStatus, AgentRole, DiffSummary, etc.)
- New daemon IPC: RegisterDelegation, CompleteDelegation, ListDelegations, GetAgentPool
- Test count after merge
- Any build issues encountered

- [ ] **Step 2: Commit the integration**

```bash
cd <legacy-worktree>
git add -A
git commit -m "chore: integrate remote main — PR #8 OpenSquirrel features + cleanup

Integrate 6 commits from origin/main:
- delegation/ module (types, detector, tracker — 661 LOC)
- context_lifecycle/parser.rs (structured output parser — 656 LOC)
- impulse-ops extended types (AgentStatus, AgentRole, DiffSummary, MachineTarget)
- 4 new daemon IPC variants (RegisterDelegation, CompleteDelegation, ListDelegations, GetAgentPool)
- Long-range enhancement roadmap (33 PRs across 8 lanes)
- Agent harness analysis update (desloppify v0.9.10)

Also: removed 7 macOS Finder duplicate files.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Build failure after merge | Very Low | `--ff-only` ensures clean merge; PR #8 claims 863 tests passing |
| Clippy regression with newer toolchain | Low | Run `cargo clippy -- -D warnings` immediately after merge |
| Flaky integration tests | Known | 2 daemon socket race tests — pass individually, skip if blocking |
| impulse-gui build failure from new impulse-ops types | Low | impulse-gui references ops types transitively — verify build |
| Duplicate file deletion removes needed file | Very Low | Verified all duplicates are identical to originals via `diff` |

## New Module Summary (Post-Integration)

After integration, these are the new capabilities available in the codebase:

### `delegation/` — Coordinator/Worker Delegation Tracking
- **types.rs**: `DelegationSpec` (task + target files + constraints + depth limit + restricted tools), `DelegationState` (Pending → InProgress → Completed/Failed), `TrackedDelegation` (full lifecycle: id, spec, state, coordinator, timestamps, context snapshot)
- **detector.rs**: `detect_delegation()` — parses ````delegate JSON code blocks; `detect_delegation_natural()` — matches natural language delegation phrases
- **tracker.rs**: `DelegationTracker` — manages delegation lifecycle (register → start → complete/fail), enforces MAX_DELEGATION_DEPTH=2

### `context_lifecycle/parser.rs` — Structured Output Parser
- `LineClassification` enum: Diff, CodeFence, Heading, Bullet, ThinkingBlock, SystemMessage, ErrorLine, ToolInvocation, DelegationMarker, PlainText
- `ParsedOutput`: aggregated stats (tool invocations, diff summary, error count, delegation detected)
- `parse_output()`: stateful line-by-line classifier replacing brittle string-prefix matching

### Extended `impulse-ops` Types
- `AgentStatus` (Idle, Active, Paused, Error, Offline)
- `AgentRole` (Coordinator, Worker, Observer)
- `DiffSummary` (files_changed, lines_added, lines_removed)
- `DelegationSummary` (total, active, completed, failed)
- `ToolInvocationRecord` (tool_name, target, timestamp, duration_ms, success)
- `MachineTarget` (Local, Remote with host + connection_type)

### Daemon IPC Extensions
- `RegisterDelegation` — stub handler (returns `delegation_tracking_not_yet_wired`)
- `CompleteDelegation` — stub handler
- `ListDelegations` — stub handler
- `GetAgentPool` — returns sessions grouped by role (fully wired)
