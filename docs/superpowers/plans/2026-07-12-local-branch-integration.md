# Local Branch Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve every local IMPULSE-rs change, produce one verified aggregate history, and fast-forward local `main` to it without pushing.

**Architecture:** `codex/agent-cache-serialization` is the isolated integration base because it already descends from current `main`. Its dirty work is split into environment-isolation and harness-isolation commits, then its contracts/counts are reconciled. `codex/registry-platform-identity` is rebased onto that verified base; older truth branches are retained as historical checkpoints only after patch-equivalence audit proves their changes are represented.

**Tech Stack:** Git worktrees, Rust/Cargo, Tokio subprocess tests, Dioxus Desktop host, npm smoke harness.

## Global Constraints

- Preserve every local tracked change; never discard or overwrite concurrent work.
- Read and align live `main` before any mutation.
- Stage exact files only; never use `git add .` or `git add -A`.
- Do not push. Move local `main` only after the aggregate branch passes every required gate.
- Preserve protocol-v3 typed `Busy`, cancellation-safe process groups, registry-driven platform identity, and Claude's live keychain/file-read/pgrep fixes.
- Treat `codex/agent-truth-parity`, `codex/desktop-daemon-truth-wire`, and `codex/live-daemon-truth-integration` as historical checkpoints unless the equivalence audit finds a patch missing from the aggregate.

---

### Task 1: Audit branch preservation

**Files:**
- Inspect: all six linked worktrees and local branch refs
- Produce: evidence in the active working note and SDD progress ledger

**Interfaces:**
- Consumes: current `main`, cache branch, registry branch, and three historical truth branches
- Produces: a list of patches that must exist in the final aggregate tip

- [x] Run `git status --short --branch` in every linked worktree.
- [x] Run `git fetch origin` and confirm `main` matches `origin/main`.
- [x] Compare historical branches to registry with `git range-diff`, `git patch-id --stable`, and targeted tree diffs.
- [x] Record whether each historical patch is represented or must be replayed explicitly.

### Task 2: Commit Claude environment-file isolation

**Files:**
- Modify: `impulse-rs/src/handlers/common.rs`
- Modify: `impulse-rs/src/handlers/mod.rs`

**Interfaces:**
- Consumes: `persist_claude_env_var(key, value)` production wrapper
- Produces: `persist_claude_env_var_at(Option<&Path>, key, value)` for deterministic path-injected tests

- [x] Run `cargo test -p impulse-rs test_persist_claude_env_var --quiet` and confirm six passing tests.
- [x] Run `cargo clippy -p impulse-rs --all-targets -- -D warnings`.
- [x] Stage only the two handler files.
- [x] Commit as `fix(test): isolate Claude env persistence`.

### Task 3: Commit harness subprocess isolation

**Files:**
- Modify: `impulse-rs/src/agent/mod.rs`
- Modify: `impulse-rs/src/test_support.rs`
- Modify: `impulse-rs/src/tooling/builtin/bash_exec.rs`

**Interfaces:**
- Consumes: `ImpulseHarness::command`, `ProcessGroupGuard`, Tokio child lifecycle
- Produces: test-only exact executable injection and exact-PID/condition-based cleanup proofs

- [x] Run focused harness and process-kill tests repeatedly.
- [x] Run the root library test twice at normal parallelism and confirm `1,530 passed, 4 ignored` each time.
- [x] Stage only the three harness/process-test files.
- [x] Commit as `fix(test): isolate harness subprocess fixtures`.

### Task 3A: Repair noninteractive macOS Keychain writes

**Files:**
- Modify: `impulse-rs/Cargo.toml`
- Modify: `impulse-rs/Cargo.lock`
- Modify: `impulse-rs/src/credentials/keychain.rs`

**Interfaces:**
- Consumes: existing `com.impulse-rs` internet-password entries and the `CredentialProvider` contract
- Produces: native macOS Keychain set/get/delete operations with no secret-bearing argv and no interactive prompt

- [x] Add a failing regression that proves normal tests never invoke a live interactive Keychain mutation.
- [x] Replace the broken `security ... -w` stdin-prompt path with native internet-password APIs while preserving the existing server/account/item-class identity.
- [x] Keep the real macOS round trip as an explicit ignored integration test with unique cleanup-safe keys.
- [x] Run the deterministic Keychain tests and the full root-library test at normal parallelism.
- [x] Run strict all-target Clippy and formatting.
- [x] Stage only the manifest, lockfile, and Keychain provider.
- [x] Commit as `fix(credentials): use native macOS keychain writes`.

### Task 3B: Establish the control-plane product contract

**Files:**
- Add: `VISION.md`
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`
- Modify: `CONTEXT.md`
- Modify: `docs/ARCHITECTURE-CLARIFICATION.md`
- Modify: `docs/INDEX.md`
- Modify: `docs/spec/RUST-CANONICAL-CONTRACT.md`
- Modify: `docs/superpowers/specs/2026-03-27-supervisor-as-terminal-design.md`

**Interfaces:**
- Consumes: the user-confirmed product hierarchy and live daemon/ops/desktop/Ion foundations
- Produces: stable control-plane vocabulary with explicit live-versus-direction boundaries

- [x] Define Impulse as a local coding-agent control plane and harness manager; keep memory as one first-class platform service.
- [x] Lead the README with `IMPULSE — Feed the impulse to build`, then connect the creative urge to the product's acceleration-and-governance model without overstating current performance or implementation completeness.
- [x] Add `VISION.md` as the living product north star: double meaning, differentiation, target architecture, resource philosophy, complete supervisor/worker loop, success criteria, non-goals, and the unresolved decisions that block separate role/runtime schemas.
- [x] Define roles separately from runtimes, agent instances, sessions, tasks, and panes; define Dioxus as the cockpit rather than the authority.
- [x] Record the live foundations without claiming generalized runtime-independent role enforcement or capability negotiation already exists.
- [x] Replace the stale architecture module inventory with a current boundary matrix.
- [x] Mark the old prompt-driven supervisor-as-terminal spec as retained exploration, not the active product contract.
- [x] Keep `CONTEXT.md` within its L0/L1 budget and reserve the concrete role/adapter schema for a later ADR.
- [x] Run docs validation and targeted stale-framing searches; distinguish known pre-existing status/freshness findings.
- [x] Link the vision from the README, contributor entry points, canonical contract, context glossary, and docs index.
- [x] Stage only the nine documentation files.
- [x] Commit as `docs: define agent control plane boundaries`.

### Task 4: Verify and reconcile the cache branch

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`
- Modify: `CONTEXT.md`
- Modify: `HANDBOOK.md`
- Modify: `docs/INDEX.md`
- Modify: `docs/ROADMAP-PLAN.md`
- Modify: `docs/SUMMARY.md`
- Modify: `docs/SUMMARY.yaml`
- Modify: `docs/spec/USER-STORY-MAP.md`
- Modify: `docs/spec/RUST-CANONICAL-CONTRACT.md`
- Modify: `docs/validate_docs.py`
- Modify: `VISION.md`
- Modify: `impulse-rs/src/daemon/protocol.rs`
- Add: `docs/superpowers/plans/2026-07-12-local-branch-integration.md`

**Interfaces:**
- Consumes: observed post-repair test totals and protocol-v3 implementation names
- Produces: one internally consistent cache-branch contract and verification record

- [x] Run workspace build, strict Clippy, formatting, and isolated workspace tests with the archive-only proof filtered.
- [x] Run standalone `impulse-gui` tests and `cargo check --locked`.
- [x] Confirm managed-turn references use `try_lock_agent_for_turn` and protocol-version-three names.
- [x] Update every stale active/current verification-count reference from observed evidence.
- [x] Replace overclaims that Impulse can attach arbitrary live runtimes or enforce external-CLI filesystem isolation; distinguish launch/cwd scoping from runtime- or sandbox-backed enforcement.
- [x] State that session verification is optional at the API level even though verification-before-completion is the contributor policy.
- [x] Align the active docs index, generated summaries, roadmap banner, and handbook banner with `VISION.md` and the canonical roadmap.
- [x] Replace the validator's obsolete desktop-migration roadmap markers with the control-plane roadmap and require `VISION.md`, so active docs no longer carry stale compatibility prose.
- [x] Update the active user-story IPC reference from protocol v2 to v3 while preserving historical v2 release notes as history.
- [x] Preserve branch-relative aggregate wording until Task 5 lands, then make its removal a blocking Task 6 reconciliation.
- [x] Commit the exact docs/protocol files as `docs(test): reconcile managed-turn verification`.

### Task 5: Stack registry identity onto cache safety

**Files:**
- Rebase: `codex/registry-platform-identity`
- Resolve only files reported by Git as conflicts

**Interfaces:**
- Consumes: verified cache tip and the six registry commits after `1927149`
- Produces: one aggregate registry/cache branch containing both contracts

- [ ] Run `git rebase --onto codex/agent-cache-serialization 1927149 codex/registry-platform-identity`.
- [ ] Resolve conflicts by preserving protocol-v3 Busy and registry/desktop identity behavior together.
- [ ] Verify no historical branch patch identified in Task 1 is absent.

### Task 6: Verify aggregate and fast-forward local main

**Files:**
- Modify only verification-count documentation if observed totals differ
- Move: local `main` ref by fast-forward after all gates pass

**Interfaces:**
- Consumes: aggregate registry/cache tip
- Produces: clean local `main` containing every preserved local change

- [ ] Run `cargo build --workspace`.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run isolated workspace tests and capture exact totals.
- [ ] Run real Ion sibling, supported desktop feature, legacy GUI, and `npm run dioxus:host:smoke` gates.
- [ ] Run docs validation and distinguish known pre-existing status/freshness failures.
- [ ] Commit any final count reconciliation on the aggregate branch.
- [ ] Convert every `[aggregate]`/`pending local aggregate` marker to live local-main wording and keep remote-release status distinct.
- [ ] Confirm no active document still presents Dioxus launch as the next product phase or memory as the whole product.
- [ ] Confirm every worktree is clean.
- [ ] Fast-forward local `main` to the aggregate tip; do not push.
