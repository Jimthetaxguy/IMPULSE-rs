# Ralph Plan 6 - Impulse Platform Stabilization And Sub-Agent Loop Map

> **Created:** 2026-05-22
> **Supersedes by reference:** Ralph Plan 5 (stale EGUI/GUI-centered execution plan)
> **Current verification baseline:** `cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, and `python3 docs/validate_docs.py --all` passed on the dirty tree.
> **Observed test baseline:** 1,483 passed, 4 ignored.

---

## Root: Primary Objective

Create a 24-loop stabilization plan for the current Impulse platform transition so future Ralph loops can safely reconcile the dirty tree, preserve existing work, retire stale EGUI-centered planning, harden the Tauri+Dioxus desktop path, clarify OpenCode compatibility status, and verify the Rust/docs gates before handoff.

The concrete deliverables are:
1. A new Ralph plan that treats `impulse-desktop`, `impulse-term`, `impulse-ops`, daemon snapshots, and docs as the active work surfaces.
2. A lane ownership map for sub-agent loops that prevents shared-file collisions.
3. A documented compatibility stance for OpenCode after global stack deprecation.
4. A verification path that preserves the current green Rust and docs baseline.

## Root: User Vision

Impulse should move from a stale GUI/EGUI transition plan into a current, lane-safe platform stabilization campaign. The target platform is a Rust memory sidecar with a Tauri+Dioxus desktop shell, xterm.js terminal bridge, daemon-backed workbench truth, frozen legacy `impulse-gui`, and compatibility-aware handling of older OpenCode surfaces.

The session should produce a plan future agents can execute without rediscovering the same platform facts:
- EGUI is legacy and frozen.
- `impulse-desktop` is the active desktop shell crate.
- `impulse-term` must become framework-neutral at the core boundary.
- Desktop state must flow from daemon snapshots and runtime events, not a second UI-owned truth store.
- OpenCode is no longer a primary platform in James's global stack, but existing Impulse compatibility references must be audited before removal.
- Existing dirty files are presumed user/agent work and must not be reverted.

## Root: Iteration Contents

| Loop | Focus | Type | Status |
|------|-------|------|--------|
| 1 | Reconcile: dirty tree, owned/shared files, work cards | work | completed |
| 2 | Docs Audit: Tauri+Dioxus truth, retired egui, OpenCode status | work | completed |
| 3 | Desktop Hygiene: duplicate Tauri command file, exports, README contract | work | completed |
| 4 | Terminal Boundary: isolate egui-only `impulse-term` paths | work | completed |
| 5 | Runtime Bridge: audit spawn/write/resize/focus/close events | work | completed |
| 6 | Daemon Truth: map snapshots, telemetry, artifacts, supervisor events | work | completed |
| 7 | Sub-Agent Lanes: document execution lanes and verification gates | work | completed |
| 8 | Planning Checkpoint: reconcile loops 1-7 and plan loops 9-15 | planning | completed |
| 9 | Protocol Parity: artifact list/get shared DTOs | work | planned |
| 10 | Desktop Daemon Client: workbench request adapter | work | planned |
| 11 | Telemetry Publish: runtime report adapter and cadence | work | planned |
| 12 | UI Rendering: daemon `ops_update` panels | work | planned |
| 13 | Docs Reconciliation: validator, indexes, metadata | work | planned |
| 14 | Cargo Integration: manifests, lockfile, workspace gate | work | planned |
| 15 | Compatibility Checkpoint: OpenCode audit and Loop 16 prep | planning | planned |
| 16 | Planning Checkpoint: reconcile loops 9-15 and plan loops 17-23 | planning | pending |
| 17 | Pending: selected by Loop 16 | work | pending |
| 18 | Pending: selected by Loop 16 | work | pending |
| 19 | Pending: selected by Loop 16 | work | pending |
| 20 | Pending: selected by Loop 16 | work | pending |
| 21 | Pending: selected by Loop 16 | work | pending |
| 22 | Pending: selected by Loop 16 | work | pending |
| 23 | Pending: selected by Loop 16 | work | pending |
| 24 | Final Verification: full Rust/docs gate and handoff | verification | pending |

## Dependency Graph

```text
Loop 1 (reconcile current state)
  -> Loop 2 (docs/product truth audit)
  -> Loop 3 (desktop hygiene)
  -> Loop 4 (terminal boundary)
  -> Loop 5 (runtime bridge audit)
  -> Loop 6 (daemon truth audit)
  -> Loop 7 (lane map)
  -> Loop 8 (planning checkpoint)

Loop 8 defined loops 9-15.
Loop 16 defines loops 17-23.
Loop 24 depends on all completed work loops and produces final verification evidence.
```

## Sub-Agent Strategy

| Loop Slot | Main Agent Work | Sub-Agent 1 | Sub-Agent 2 |
|-----------|-----------------|-------------|-------------|
| Loop 1 | Integrator reconciles dirty tree and work cards | Explore: classify changed files by subsystem | - |
| Loop 2 | Docs agent audits platform truth | Explore: scan stale `egui`/`impulse-gui` refs | Explore: scan OpenCode compatibility refs |
| Loop 3 | Desktop implementer cleans crate hygiene | Reviewer: inspect public DTO/export impact | - |
| Loop 4 | Rust core agent isolates terminal core from egui surfaces | Explore: map `eframe` dependency paths | Reviewer: verify no new desktop coupling |
| Loop 5 | Runtime explorer audits terminal event semantics | Reviewer: inspect command DTO compatibility | - |
| Loop 6 | Daemon explorer maps snapshot and telemetry truth | Explore: trace `ProjectOpsSnapshot` producers | Explore: trace artifact/supervisor flows |
| Loop 7 | Integrator writes lane execution map | Reviewer: check shared-file ownership | - |
| Loop 8 | Planner reconciles and plans next batch | Explore: summarize metrics and risks | - |
| Loop 24 | Verifier runs full gate and handoff | Reviewer: final diff and docs consistency | - |

## Domain Inventory

| Domain | Current Evidence | Plan Handling |
|--------|------------------|---------------|
| Planning state | `ralph-plan-5.md` is structurally healthy but stale against current desktop contract | Plan 6 supersedes it by reference; Plan 5 Root docs remain unchanged |
| Collaboration lanes | `docs/guides/COLLABORATIVE-AGENTIC-CODING.md` and worktree card exist in dirty tree | Loop 1 and Loop 7 keep lane cards current |
| Desktop shell | `impulse-rs/impulse-desktop/` has Dioxus shell, runtime, Tauri command tests, and native island DTOs | Loops 3 and 5 harden crate hygiene and bridge semantics |
| Terminal core | `impulse-term` has optional `egui` feature and new `paste` module, but still contains egui input/panel paths | Loop 4 finishes the framework-neutral boundary plan |
| Daemon truth | `ops_workbench` and daemon protocol docs define snapshot/telemetry surfaces | Loop 6 maps the authoritative data flow before further desktop work |
| OpenCode compatibility | Repo still contains OpenCode docs/tests/types while global stack deprecation is complete | Loop 2 audits and Loop 8 decides precise compatibility cleanup |

## Loop Plans

### Loop 1 Plan
**Type:** work
**Objective:** Reconcile the current dirty tree, worktrees, and lane ownership before any implementation work continues.
**Risk:** MEDIUM
**Sub-steps:**
1. Run `git status --short`, `git branch --show-current`, and `git worktree list`.
2. Classify dirty files into docs, desktop shell, terminal core, daemon/runtime, and unrelated WIP buckets.
3. Update or create work cards under `docs/plans/worktrees/` with owned paths, shared paths, and verification gates.
**Inputs:** Current dirty tree and collaborative coding guide.
**Outputs:** Reconciled lane facts and updated work card state.
**Status:** completed

### Loop 2 Plan
**Type:** work
**Objective:** Audit documentation against the current platform truth: Tauri+Dioxus active, EGUI frozen, OpenCode compatibility legacy.
**Risk:** MEDIUM
**Sub-steps:**
1. Scan docs and agent entrypoints for stale `EGUI`, `egui`, `impulse-gui`, `OpenCode`, and `opencode` claims.
2. Separate product-truth fixes from historical/research references that should remain.
3. Produce a docs audit summary with exact files and recommended edits.
**Inputs:** Loop 1 ownership map and current canonical contract.
**Outputs:** Docs drift inventory and cleanup recommendations.
**Status:** completed

### Loop 3 Plan
**Type:** work
**Objective:** Clean `impulse-desktop` crate hygiene without changing the desktop architecture.
**Risk:** MEDIUM
**Sub-steps:**
1. Resolve `impulse-rs/impulse-desktop/src/tauri_commands 2.rs` as an iCloud duplicate or intentional file, using archive/ignore discipline if removal is needed.
2. Review public exports in `impulse-desktop/src/lib.rs` against runtime and Tauri command tests.
3. Align `impulse-desktop/README.md` with the Root platform contract.
**Inputs:** Loop 1 lane facts and Loop 2 docs audit.
**Outputs:** Clean desktop crate file set and passing `cargo test -p impulse-desktop`.
**Status:** completed

### Loop 4 Plan
**Type:** work
**Objective:** Finish the `impulse-term` framework-neutral boundary plan by making egui surfaces optional and core PTY/paste/backend APIs independent.
**Risk:** MEDIUM
**Sub-steps:**
1. Trace all `eframe` and `egui` references in `impulse-term`.
2. Keep egui-only modules behind the `egui` feature while preserving default compatibility.
3. Verify `impulse-desktop` can depend on `impulse-term` with `default-features = false`.
**Inputs:** Existing optional `egui` feature changes and `paste` module.
**Outputs:** Documented boundary plus passing `cargo test -p impulse-term` and workspace checks.
**Status:** completed

### Loop 5 Plan
**Type:** work
**Objective:** Audit desktop runtime command semantics for agent spawn/write/resize/focus/close and event emission.
**Risk:** MEDIUM
**Sub-steps:**
1. Review `DesktopRuntime`, `TerminalBridge`, and `tauri_commands` DTO behavior.
2. Add or adjust focused tests for invalid dimensions, missing sessions, focus exclusivity, and supervisor confirmation.
3. Confirm event names match the desktop shell interop script.
**Inputs:** Clean desktop crate from Loop 3.
**Outputs:** Runtime bridge audit notes and focused regression coverage.
**Status:** completed

### Loop 6 Plan
**Type:** work
**Objective:** Map daemon/workbench truth before connecting more desktop UI to live state.
**Risk:** HIGH
**Sub-steps:**
1. Trace `ProjectOpsSnapshot`, terminal telemetry reports, artifact actions, and supervisor-local actions.
2. Identify which surfaces are daemon-owned, runtime-owned, and UI-rendered.
3. Produce a daemon truth map that blocks duplicate state ownership in desktop work.
**Inputs:** Current `ops_workbench`, daemon protocol docs, and desktop runtime DTOs.
**Outputs:** Daemon truth inventory and integration sequencing notes.
**Status:** completed

### Loop 7 Plan
**Type:** work
**Objective:** Convert the audit findings into sub-agent execution lanes with owned paths and verification gates.
**Risk:** LOW
**Sub-steps:**
1. Write or update a Ralph Plan 6 work card under `docs/plans/worktrees/`.
2. Assign lanes for Integrator, Docs, Desktop, Rust Core, Daemon, Reviewer, and Verifier work.
3. Record blocked/shared paths and commands required before handoff.
**Inputs:** Loops 1-6 outputs.
**Outputs:** Execution lane map ready for Loop 8 planning.
**Status:** completed

### Loop 8 Plan
**Type:** planning
**Objective:** Reconcile loops 1-7, compare progress against this Root vision, and decide detailed loop plans for loops 9-15.
**Risk:** LOW
**Sub-steps:**
1. Run plan integrity validation and reconcile statuses.
2. Capture metrics: dirty-file count, test count, ignored tests, docs validation status, and known platform risks.
3. Replace loops 9-15 pending rows with concrete loop assignments.
**Inputs:** Working Logs from loops 1-7.
**Outputs:** Updated Iteration Contents, new loop plans for 9-15, and planning Working Log.
**Status:** completed

### Loop 9 Plan
**Type:** work
**Objective:** Close the shared protocol parity gap for artifact list/get/action requests before desktop artifact panels depend on daemon workbench requests.
**Risk:** HIGH
**Sub-steps:**
1. Add `ListArtifacts` and `GetArtifact` variants to `impulse_ops::WorkbenchDaemonRequest` without changing daemon ownership of artifact state.
2. Update `impulse-rs/src/daemon/protocol.rs` compatibility naming/tests so shared requests deserialize into daemon protocol variants.
3. Verify artifact request parity with focused `impulse-ops`, daemon protocol, and `ops_workbench` tests.
**Inputs:** Loop 6 daemon truth artifact and Loop 7 lane map.
**Outputs:** Shared DTO/protocol parity for artifact list/get/action.
**Status:** planned

### Loop 10 Plan
**Type:** work
**Objective:** Add a desktop daemon-client adapter for workbench requests without inventing a second project/artifact/supervisor state owner.
**Risk:** HIGH
**Sub-steps:**
1. Add or extend a desktop-side daemon request adapter under `impulse-rs/impulse-desktop/src/**`.
2. Cover `GetOpsSnapshot`, `SubscribeOps`, `PublishTerminalOps`, `ListArtifacts`, `GetArtifact`, and `RunArtifactAction` request construction with focused desktop tests.
3. Keep Cargo manifests blocked unless Loop 14 grants ownership for dependency wiring.
**Inputs:** Loop 9 shared DTO/protocol parity.
**Outputs:** Desktop request plumbing for daemon-owned workbench truth.
**Status:** planned

### Loop 11 Plan
**Type:** work
**Objective:** Build the desktop runtime telemetry adapter that publishes `TerminalOpsReport` from runtime facts on the correct cadence.
**Risk:** HIGH
**Sub-steps:**
1. Map `AgentRuntimeSnapshot` into `impulse_ops::AgentRuntime` records inside a desktop-owned adapter.
2. Publish telemetry on spawn, resize, focus, exit, and a heartbeat below the daemon stale threshold.
3. Test that publish triggers report runtime facts as telemetry input only; panels still consume daemon truth.
**Inputs:** Loop 10 daemon-client adapter and Loop 6 telemetry boundary.
**Outputs:** Runtime telemetry publishing path and focused regression tests.
**Status:** planned

### Loop 12 Plan
**Type:** work
**Objective:** Render desktop workbench panels from daemon `ops_update` / snapshot payloads while keeping xterm glyph rendering separate.
**Risk:** HIGH
**Sub-steps:**
1. Replace placeholder agent/artifact/supervisor panel content with render-only projections of daemon payloads.
2. Preserve xterm byte rendering as runtime output, not a source for artifact/supervisor/project truth.
3. Add UI contract tests proving panels render daemon payloads and do not patch daemon snapshots client-side.
**Inputs:** Loops 10-11 request and telemetry adapters.
**Outputs:** UI panels aligned to daemon truth boundaries.
**Status:** planned

### Loop 13 Plan
**Type:** work
**Objective:** Reconcile docs validator, docs indexes, summary metadata, and targeted plan docs after implementation truth settles.
**Risk:** MEDIUM
**Sub-steps:**
1. Update `docs/validate_docs.py` and docs metadata so active docs validate against current platform truth.
2. Reconcile docs indexes and summaries with verified Tauri+Dioxus, daemon truth, and legacy egui/OpenCode status.
3. Preserve historical/research references unless a doc is actively routed as current product truth.
**Inputs:** Loops 9-12 implementation results.
**Outputs:** Docs contract/index consistency and green docs validation.
**Status:** planned

### Loop 14 Plan
**Type:** verification
**Objective:** Own Cargo manifest/lockfile integration and run the broad workspace Rust gate after feature lanes settle.
**Risk:** HIGH
**Sub-steps:**
1. Apply any manifest or lockfile changes required by loops 9-12 in one integration lane.
2. Fix only mechanical compile/test issues caused by accepted dependency wiring.
3. Run the full Rust workspace check, test, clippy, and fmt gates.
**Inputs:** Loops 9-13 handoffs and any recorded dependency needs.
**Outputs:** Integrated Cargo state and workspace verification evidence.
**Status:** planned

### Loop 15 Plan
**Type:** planning
**Objective:** Audit OpenCode compatibility debt and prepare Loop 16 planning without removing compatibility code by default.
**Risk:** MEDIUM
**Sub-steps:**
1. Run a read-only OpenCode/opencode scan across root docs, active docs, and Rust crates.
2. Classify references as legacy compatibility, historical/research, active product-truth drift, or removal candidates.
3. Prepare Loop 16 recommendations for loops 17-23, including whether compatibility cleanup is documentation-only or code-affecting.
**Inputs:** Loops 9-14 completion state and current global OpenCode deprecation context.
**Outputs:** Compatibility audit artifact and Loop 16 prep notes.
**Status:** planned

## Loop 7 Future Lane Execution Map For Loops 9-15

Loop 8 adopted this map into the Root Iteration Contents and detailed loop plans above.

| Loop | Focus | Agent Role | Owned Paths | Blocked/Shared Paths | Dependencies | Verification Gates |
|---|---|---|---|---|---|---|
| Loop 9 | Shared protocol parity for artifact list/get/action DTOs | Rust protocol implementer | `impulse-rs/impulse-ops/src/lib.rs`; `impulse-rs/src/daemon/protocol.rs`; focused tests in those crates | `impulse-rs/Cargo.toml`; `Cargo.lock`; `impulse-rs/impulse-desktop/**`; docs indexes/spec docs | Loop 6 artifact DTO gap; Loop 8 adoption | `cargo test -p impulse-ops`; `cargo test -p impulse-rs daemon::protocol::tests::test_shared_workbench_requests_deserialize_into_daemon_protocol`; `cargo test -p impulse-rs ops_workbench`; plan validation; `git diff --check` |
| Loop 10 | Desktop daemon client adapter for workbench requests | Desktop runtime implementer | `impulse-rs/impulse-desktop/src/**`; `impulse-rs/impulse-desktop/tests/**` | Shared DTO/protocol files from Loop 9; Cargo files unless Loop 14 grants ownership | Loop 9 green | `cargo test -p impulse-desktop`; focused daemon-client tests; plan validation; `git diff --check` |
| Loop 11 | Runtime telemetry adapter and publish cadence | Desktop runtime implementer with daemon reviewer | `impulse-rs/impulse-desktop/src/runtime.rs`; telemetry adapter module/tests under `impulse-rs/impulse-desktop/**` | Shared protocol/DTO files; UI panel state files unless Loop 12 starts after handoff | Loops 9-10 green | `cargo test -p impulse-desktop --test runtime`; focused publish-trigger tests; plan validation; `git diff --check` |
| Loop 12 | UI ops-update rendering and artifact/supervisor panels | Desktop UI implementer | `impulse-rs/impulse-desktop/src/ui.rs`; UI/panel modules and desktop contract tests under `impulse-rs/impulse-desktop/**` | Runtime telemetry internals unless handed off; shared DTO/protocol files; docs specs/indexes | Loops 10-11 green | `cargo test -p impulse-desktop --test desktop_contract --test tauri_surface`; render-only daemon payload tests; plan validation; `git diff --check` |
| Loop 13 | Docs validator and index reconciliation after implementation semantics settle | Docs contract implementer | `docs/validate_docs.py`; `docs/INDEX.md`; `docs/SUMMARY.md`; `docs/SUMMARY.yaml`; `docs/metadata.yaml`; targeted plan docs | `AGENTS.md`; `CLAUDE.md`; `README.md`; `docs/spec/**`; Rust source/Cargo files unless explicitly claimed | Loops 9-12 define implementation truth | `python3 docs/validate_docs.py --contract`; `python3 docs/validate_docs.py --all`; plan validation; `git diff --check` |
| Loop 14 | Cargo integration and workspace verification | Integration implementer/verifier | `impulse-rs/Cargo.toml`; `impulse-rs/Cargo.lock`; crate manifests only if required by loops 9-12 | Feature source paths except mechanical manifest-driven compile fixes; docs specs/indexes unless Loop 13 hands off | Loops 9-12 declare final dependency needs | `cargo check --workspace`; `cargo test --workspace`; `cargo clippy --workspace -- -D warnings`; `cargo fmt --check`; plan validation; `git diff --check` |
| Loop 15 | OpenCode compatibility cleanup decision and Loop 16 prep | Reviewer/planner with docs support | `docs/plans/worktrees/**`; `ralph-plan-6.md`; optional compatibility audit artifact under `docs/plans/worktrees/**` | OpenCode code/tests/docs; root guidance docs; specs; validator; Cargo files unless Loop 16 claims them | Loops 9-14 complete or have exact blockers | Plan validation; `git diff --check`; optional read-only OpenCode reference scan |

**Shared-File Sequencing:**
- `impulse-rs/Cargo.toml` and `impulse-rs/Cargo.lock`: Loop 14 owns both. Loops 9-12 record dependency needs but do not update manifests or lockfile opportunistically.
- `impulse-rs/impulse-ops/src/lib.rs`: Loop 9 owns shared `WorkbenchDaemonRequest` parity. Later lanes read it unless Loop 9 hands off a blocker.
- `impulse-rs/src/daemon/protocol.rs`: Loop 9 owns compatibility-test updates paired with DTO parity. Desktop lanes must not change daemon protocol tests to make UI code pass.
- `impulse-rs/impulse-desktop/**`: Loops 10-12 are sequential, not parallel. Loop 10 owns request plumbing, Loop 11 owns runtime telemetry publishing, and Loop 12 owns render-only UI panels.
- Docs indexes/spec docs: Loop 13 owns validator/index reconciliation after code truth settles. `docs/spec/**`, root guidance docs, and docs indexes stay blocked for code lanes.
- Plan files: each loop may update its own work card and `ralph-plan-6.md` log/status only. Root loop assignments for 17-23 stay reserved for Loop 16.

**Blocked-Path Rules:**
- Future worker scopes are disjoint unless a work card records an explicit handoff.
- No loop edits another active loop's owned files.
- No loop touches Cargo files before Loop 14 without explicit ownership transfer.
- No desktop lane creates a second artifact, supervisor, telemetry, or project truth store; daemon snapshots and ops updates remain authoritative.
- No docs lane rewrites broad specs to match incomplete code.
- No worker normalizes, formats, stages, reverts, archives, or deletes unrelated dirty files.

See `docs/plans/worktrees/2026-05-22-sub-agent-lane-map-loop7.md` for the full Loop 7 lane card.

## Verification Plan

Every implementation checkpoint must run:

```bash
python3 docs/validate_docs.py --all
cd impulse-rs && cargo check --workspace
cd impulse-rs && cargo test --workspace
cd impulse-rs && cargo clippy --workspace -- -D warnings
cd impulse-rs && cargo fmt --check
bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md
```

Focused checks should be added as loops demand:
- `cargo test -p impulse-desktop`
- `cargo test -p impulse-term`
- `rg -n "OpenCode|opencode|EGUI|egui|impulse-gui|eframe" AGENTS.md CLAUDE.md README.md docs impulse-rs --glob '!target/**'`
- docs diff review for canonical contract, roadmap, traceability, and summary files

## Assumptions

- The target platform is `/Users/jamespustorino/Desktop/VibeCode_Prime/CLI_CU_L8R`.
- There is no active `.Codex/ralph-loop.local.md`; this is a new planning session.
- Existing dirty files are presumed user/agent work and must not be reverted.
- OpenCode inside Impulse is treated as legacy compatibility until James explicitly confirms product-level removal.

## Loop 8 Working Log

**Type:** planning
**Status:** completed
**Objective:** Reconcile loops 1-7, adopt concrete loop plans for loops 9-15, and prepare the next implementation batch.

**Files Changed:**
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Reviewed the Loop 7 lane map and promoted it into the Root Iteration Contents for loops 9-15.
- Added detailed Loop 9-15 plans for shared protocol parity, desktop daemon-client work, telemetry publishing, UI rendering, docs reconciliation, Cargo integration, and OpenCode compatibility checkpointing.
- Repaired the non-Root lane-map table heading from `Agent Type` to `Agent Role` so the Ralph plan validator no longer treats the lane role column as a loop type column.
- Preserved shared-file sequencing: `impulse-rs/impulse-ops/src/lib.rs` and `impulse-rs/src/daemon/protocol.rs` belong to Loop 9, desktop source to Loops 10-12 in sequence, docs validator/indexes to Loop 13, Cargo files to Loop 14, and compatibility decisions to Loop 15.

**Key Decisions:**
- Adopt Loop 9 as the first code lane because shared artifact list/get DTO parity blocks clean desktop artifact panel work.
- Keep Loops 10-12 sequential because they overlap in `impulse-rs/impulse-desktop/**`.
- Keep Loop 14 as the only planned Cargo manifest/lockfile owner for the next batch.
- Keep OpenCode removal out of scope until Loop 15 audits compatibility references and Loop 16 decides follow-up work.

**Verification:**
- Passed: `python3 docs/validate_docs.py --all`
- Passed: `cd impulse-rs && cargo check --workspace`
- Passed: `cd impulse-rs && cargo test --workspace` (1,483 tests passed, 4 ignored)
- Passed: `cd impulse-rs && cargo clippy --workspace -- -D warnings`
- Passed: `cd impulse-rs && cargo fmt --check`
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 9 can start after the Loop 8 final gate is green.
- Future workers must update their work card and `ralph-plan-6.md` log/status, then run plan validation and `git diff --check` before handoff.

## Loop 7 Working Log

**Type:** work
**Status:** completed
**Objective:** Convert the Loop 1-6 findings into a concrete, lane-safe sub-agent execution map for loops 9-15.

**Files Changed:**
- `docs/plans/worktrees/2026-05-22-sub-agent-lane-map-loop7.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- `ralph-plan-6.md`

**Work Completed:**
- Added a dedicated Loop 7 lane map artifact with loop focus, agent type, owned paths, blocked/shared paths, dependencies, and verification gates for loops 9-15.
- Sequenced the future code lanes so shared protocol parity comes before desktop daemon-client work, runtime telemetry publishing, and UI panel rendering.
- Reserved `impulse-rs/Cargo.toml` and `impulse-rs/Cargo.lock` for a later integration lane instead of allowing incidental manifest churn across feature workers.
- Reserved docs validator/index reconciliation for a later docs lane after code truth settles.
- Added explicit blocked-path rules that keep future scopes disjoint unless a work card records an ownership handoff.

**Key Decisions:**
- Keep Loop 7 docs-only and avoid claiming Rust source, Cargo files, docs validators, root guidance docs, specs, indexes, or unrelated dirty files.
- Treat Loop 9 as the first recommended code lane because `WorkbenchDaemonRequest` artifact list/get parity must exist before desktop artifact panels depend on shared requests.
- Treat Loop 14 as the only Cargo integration lane for loops 9-15.
- Leave Root loop assignments for 9-15 pending so Loop 8 can formally adopt or revise the map.

**Verification:**
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 8 should review the non-Root Loop 7 future lane map and decide whether to update the Root Iteration Contents for loops 9-15.
- Future code work should start with Loop 9 protocol parity, then proceed through desktop request adapter, telemetry publishing, and render-only UI panels.
- `impulse-rs/Cargo.toml`, `Cargo.lock`, `impulse-rs/impulse-ops/src/lib.rs`, `impulse-rs/src/daemon/protocol.rs`, `impulse-rs/impulse-desktop/**`, docs validators/indexes/specs, root guidance docs, and unrelated dirty files were not edited in Loop 7.

## Loop 6 Working Log

**Type:** work
**Status:** completed
**Objective:** Map daemon/workbench truth boundaries so future desktop work does not duplicate daemon-owned state.

**Files Changed:**
- `docs/plans/worktrees/2026-05-22-daemon-truth-boundary-loop6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- `ralph-plan-6.md`

**Work Completed:**
- Added a Loop 6 boundary artifact separating daemon-owned `ProjectOpsSnapshot`, artifact store/actions, supervisor policy/action approval, and terminal telemetry overlay store from desktop-runtime PTY mechanics.
- Documented desktop-runtime ownership of PTY handles, write queue, resize/focus, output fanout, and future `TerminalOpsReport` generation.
- Documented UI-rendered surfaces: xterm glyphs/layout from `terminal_output`, and workbench panels from daemon `ops_update` / snapshot truth.
- Identified the concrete `WorkbenchDaemonRequest` parity gap: shared DTO lacks `ListArtifacts` and `GetArtifact` even though daemon protocol handlers already support them.
- Wrote the follow-up code plan for adding shared DTO variants, updating protocol compatibility tests, and verifying artifact list/get/action parity.
- Wrote the desktop publish/subscribe plan: publish `TerminalOpsReport` from runtime snapshots, subscribe to daemon ops updates, render daemon truth, and avoid client-side snapshot patching.

**Key Decisions:**
- Keep Loop 6 docs-only because the boundary deliverable did not require touching the optional shared `impulse-rs/impulse-ops/src/lib.rs` path.
- Treat `PublishTerminalOps` as telemetry input to daemon reconciliation, not as desktop-owned durable state.
- Require future artifact panel work to close the shared DTO list/get gap before wiring ad hoc desktop JSON requests.

**Verification:**
- Passed: `cd impulse-rs && cargo test -p impulse-rs ops_workbench`
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 7 should use `docs/plans/worktrees/2026-05-22-daemon-truth-boundary-loop6.md` as the source for assigning protocol, desktop-runtime, and UI-panel lanes.
- A future shared protocol lane should add `ListArtifacts` and `GetArtifact` to `WorkbenchDaemonRequest` and extend `daemon::protocol` compatibility tests before desktop artifact panels depend on the shared DTO.
- A future desktop lane should build a daemon client plus telemetry adapter that publishes `TerminalOpsReport`, then consumes daemon `SubscribeOps` responses as the source for `ops_update` rendering.
- No Rust source, Cargo files, protocol docs, or unrelated dirty files were edited.

## Loop 5 Working Log

**Type:** work
**Status:** completed
**Objective:** Audit and harden desktop runtime bridge semantics for agent spawn/write/resize/focus/close, supervisor-confirmed input, and xterm.js input serialization.

**Files Changed:**
- `impulse-rs/impulse-desktop/src/ui.rs`
- `impulse-rs/impulse-desktop/tests/desktop_contract.rs`
- `impulse-rs/impulse-desktop/tests/runtime.rs`
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Added missing-session runtime coverage for write, resize, focus, and close commands.
- Added invalid-dimension coverage for spawn and resize requests.
- Added focus exclusivity coverage across multiple live desktop runtime agents.
- Extended supervisor-local input coverage so unconfirmed sends are rejected and confirmed sends route to the agent write path.
- Changed the Dioxus xterm interop script to encode `terminal.onData` strings with `TextEncoder` before invoking Rust `agent_write`.
- Exposed the interop script through the public `ui` module for focused contract testing.
- Added contract tests proving Rust accepts byte-array `AgentWriteRequest.data` payloads and rejects raw JavaScript string payloads.

**Key Decisions:**
- Keep the Rust DTO as `Vec<u8>` and fix the UI-side serialization boundary rather than weakening Rust to accept strings.
- Test the interop script as a stable contract string because the Dioxus/Tauri shell is not fully live in the focused runtime test suite.
- Avoid Cargo manifests, terminal-core files, shared docs, and unrelated dirty files during Loop 5.

**Verification:**
- Passed: `cd impulse-rs && cargo test -p impulse-desktop --test runtime --test tauri_surface --test desktop_contract` (16 tests passed, 0 failed)
- Passed: `cd impulse-rs && cargo fmt -p impulse-desktop`
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 6 can rely on focused desktop runtime bridge coverage for missing sessions, invalid dimensions, focus exclusivity, supervisor confirmation, and xterm input byte serialization.
- `agent_write` still expects byte arrays; any future UI/client surface that sends terminal input must perform the same string-to-byte conversion before crossing the Rust command boundary.
- `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, `impulse-rs/impulse-term/**`, shared docs, and unrelated dirty files were not edited.

## Loop 4 Working Log

**Type:** work
**Status:** completed
**Objective:** Isolate the `impulse-term` terminal core boundary so backend, context, and paste APIs remain usable without egui while preserving the default egui-compatible feature set.

**Files Changed:**
- `impulse-rs/impulse-term/README.md`
- `impulse-rs/impulse-term/tests/boundary_tests.rs`
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Confirmed the existing `impulse-term` feature split keeps `eframe` optional and preserves `default = ["egui"]` compatibility.
- Confirmed `input`, `panel`, `renderer`, `status_bar`, and `theme` are gated behind the `egui` feature in `src/lib.rs`.
- Confirmed `backend`, `context`, and `paste` remain available without default features.
- Added `tests/boundary_tests.rs` to compile and exercise the framework-neutral public exports without importing any egui types.
- Updated the `impulse-term` README to document the core boundary, optional egui modules, and the no-default-features verification command.

**Key Decisions:**
- Preserve default egui compatibility rather than flipping default features during Loop 4.
- Keep bracketed paste as a framework-neutral `paste` module and let egui input re-export it for compatibility.
- Avoid root `Cargo.toml`, `Cargo.lock`, `impulse-desktop`, and shared docs edits in this loop.

**Verification:**
- Passed: `cd impulse-rs && cargo test -p impulse-term --no-default-features` (54 tests passed, 0 failed)
- Passed: `cd impulse-rs && cargo test -p impulse-term` (114 tests passed, 0 failed)
- Passed: `cargo fmt -p impulse-term`
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 5 can audit runtime bridge semantics against a terminal crate whose core API compiles without egui.
- The no-default-features assertion now lives in both the required verification command and `tests/boundary_tests.rs`.
- `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, `impulse-rs/impulse-desktop/**`, and unrelated dirty files were not edited.

## Loop 3 Working Log

**Type:** work
**Status:** completed
**Objective:** Clean `impulse-desktop` crate hygiene by archiving the stale duplicate Tauri command file, confirming the active command surface remains runtime-backed, and clarifying the desktop README contract.

**Files Changed:**
- `impulse-rs/impulse-desktop/README.md`
- `impulse-rs/impulse-desktop/_archive-2026-05-22-loop3/tauri_commands-2.rs`
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Archived the untracked stale duplicate `impulse-rs/impulse-desktop/src/tauri_commands 2.rs` to `impulse-rs/impulse-desktop/_archive-2026-05-22-loop3/tauri_commands-2.rs` instead of deleting it.
- Preserved the active command surface at `impulse-rs/impulse-desktop/src/tauri_commands.rs`.
- Confirmed `impulse-rs/impulse-desktop/src/lib.rs` exports `pub mod tauri_commands;`, which resolves to the active `src/tauri_commands.rs` module.
- Confirmed the active Tauri command functions route through `DesktopRuntime` for agent spawn/write/resize/focus/close, terminal open/write/resize/close/focus, snapshots, and supervisor local actions.
- Confirmed the archived duplicate used a stale static `InMemoryTerminalBridge` surface and is no longer in the source module path.
- Updated `impulse-rs/impulse-desktop/README.md` to state that `impulse-desktop` is the active Tauri+Dioxus shell path and not the retired `impulse-gui`/egui workbench path.

**Key Decisions:**
- Keep the archived duplicate inside the `impulse-desktop` crate boundary for discoverability during this stabilization plan, but outside `src/` so Cargo does not treat it as an active module candidate.
- Avoid Cargo manifest, lockfile, terminal-core, and shared docs edits in this loop.

**Verification:**
- Passed: `cd impulse-rs && cargo test -p impulse-desktop` (10 tests passed, 0 failed)
- Passed: `cd impulse-rs && cargo check -p impulse-desktop --features tauri-runtime`
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check`

**Handoff Notes:**
- Loop 4 can proceed to terminal boundary work without relying on the archived duplicate command file.
- `impulse-rs/Cargo.toml`, `impulse-rs/Cargo.lock`, `impulse-rs/impulse-term/**`, `docs/validate_docs.py`, and unrelated dirty files were not edited.
- If a later cleanup wants the archived duplicate outside the crate, move it through the same archive-don't-delete pattern rather than deleting it.

## Loop 2 Working Log

**Type:** work
**Status:** completed
**Objective:** Fix active documentation/platform-truth drift so current docs route users to Tauri+Dioxus desktop work, frozen legacy `impulse-gui`, and Claude Code/Codex primary platform support with legacy OpenCode compatibility.

**Files Changed:**
- `README.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/spec/RUST-CANONICAL-CONTRACT.md`
- `docs/guides/COLLABORATIVE-AGENTIC-CODING.md`
- `impulse-rs/QUICKSTART.md`
- `impulse-rs/impulse-gui/README.md`
- `docs/plans/IMPLEMENTATION-HANDOFF.md`
- `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md`
- `docs/metadata.yaml`
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Updated top-level README platform wording so Claude Code and Codex are active, while OpenCode is explicitly legacy compatibility.
- Updated `AGENTS.md` and `CLAUDE.md` roadmap banners to remove stale "Phase 0 docs reset" framing and keep egui as compile-maintenance only.
- Reframed the canonical contract's platform section from Claude/OpenCode parity to primary Claude Code/Codex support plus legacy OpenCode compatibility.
- Adjusted the Tauri desktop shell capability status from Phase 0 docs reset to an in-migration scaffold with pending live bridge and daemon parity.
- Changed collaborative lane prefix guidance so `opencode` is historical/legacy only, not a current peer prefix.
- Replaced the quickstart's `impulse-gui` launch path with the current ratatui TUI path and marked the Tauri+Dioxus shell as in migration.
- Added a legacy/frozen banner to `impulse-rs/impulse-gui/README.md` and marked its OpenCode terminal support as legacy.
- Reworked handoff docs so Phase 0 is a baseline/drift-cleanup context rather than the current implementation phase.
- Updated `docs/metadata.yaml` to reflect Claude Code/Codex hooks, Tauri+Dioxus desktop migration, and frozen egui status.

**Key Decisions:**
- Preserve historical/research/ADR references to OpenCode and egui where they describe prior decisions or compatibility surfaces.
- Do not edit Rust source, Cargo files, docs indexes, roadmap archives, or historical research docs from this docs-only loop.
- Treat `impulse-gui` as already frozen for new features; later parity work should plan removal, not another freeze step.

**Verification:**
- Failed with pre-existing/out-of-scope validator drift: `python3 docs/validate_docs.py --all`
  - `docs/validate_docs.py` still requires the old roadmap marker text containing `Phase 0 docs reset` in `AGENTS.md` and `CLAUDE.md`.
  - The same run reports 30 stale docs last updated `2026-02-20` across research/spec/vision/decision/guide/phase files outside Loop 2 ownership.
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check -- README.md AGENTS.md CLAUDE.md docs/spec/RUST-CANONICAL-CONTRACT.md docs/guides/COLLABORATIVE-AGENTIC-CODING.md impulse-rs/QUICKSTART.md impulse-rs/impulse-gui/README.md docs/plans/IMPLEMENTATION-HANDOFF.md docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md docs/metadata.yaml ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Handoff Notes:**
- Loop 3 can proceed with desktop crate hygiene; this loop made no Rust/Cargo/source edits.
- `docs/INDEX.md`, `docs/SUMMARY.md`, `docs/SUMMARY.yaml`, and `docs/ROADMAP-PLAN.md` still contain pre-existing dirty changes outside Loop 2 ownership and were not normalized.
- Existing OpenCode code/tests/types remain compatibility surfaces; product-level removal is still a future explicit decision.
- A future docs-validation lane should update `docs/validate_docs.py` contract markers and decide whether stale historical docs should be marked archive/superseded or have their freshness threshold relaxed.

## Loop 1 Working Log

**Type:** work
**Status:** completed
**Objective:** Reconcile the current dirty tree, worktrees, and lane ownership before implementation work continues.

**Files Changed:**
- `ralph-plan-6.md`
- `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Work Completed:**
- Captured the required repository state: branch `main`, primary worktree at `/Users/jamespustorino/Desktop/VibeCode_Prime/CLI_CU_L8R`, and sibling worktrees `.worktrees/gui-roadmap` plus `.worktrees/impulse-1.0-memory-loop`.
- Classified the dirty tree into shared guidance/root docs, docs indexes/specs/protocols, collaboration docs, research docs, Rust workspace metadata, Rust implementation files, prior plan state, and current Plan 6 lane files.
- Narrowed Loop 1 ownership to `ralph-plan-6.md` and the Plan 6 work card; `ralph-plan-5.md` is explicitly blocked and preserved.
- Recorded blocked/shared paths in the work card, including Rust code, manifests, `AGENTS.md`, `CLAUDE.md`, `README.md`, docs indexes/specs, protocol docs, and all existing dirty files outside this lane.
- Updated the Loop 1 status in Iteration Contents and the Loop 1 Plan without changing Root: Primary Objective or Root: User Vision.

**Verification:**
- Passed: `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`
- Passed: `git diff --check -- ralph-plan-6.md docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`

**Handoff Notes:**
- Implementation work remains blocked until the next lane claims its paths.
- The current dirty tree includes broad pre-existing/shared changes; do not bulk format, stage, revert, or normalize it from this lane.
- Future lanes should update or add dedicated `docs/plans/worktrees/*.md` cards before touching shared files.
