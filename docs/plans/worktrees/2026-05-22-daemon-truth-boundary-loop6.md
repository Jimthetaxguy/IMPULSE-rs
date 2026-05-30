---
title: Daemon Truth Boundary Loop 6
description: Boundary artifact for Ralph Plan 6 Loop 6 daemon/workbench truth ownership.
updated: 2026-05-22
type: doc
category: planning
phase: all
status: active
audience: builders
tags: [worktree, daemon, desktop, ops, artifacts, supervisor, telemetry]
---

# Daemon Truth Boundary Loop 6

## Lane Facts

- Owner: Codex Loop 6 worker
- Role: Daemon truth boundary mapper
- Branch: `main`
- Worktree: `/Users/jamespustorino/Desktop/VibeCode_Prime/CLI_CU_L8R`
- Owned paths: this artifact, `ralph-plan-6.md`, `docs/plans/worktrees/2026-05-22-ralph-plan-6-platform-stabilization.md`
- Read-only evidence: `impulse-rs/src/ops_workbench.rs`, `impulse-rs/src/daemon/handlers.rs`, `impulse-rs/src/daemon/protocol.rs`, `impulse-rs/impulse-ops/src/lib.rs`, `impulse-rs/impulse-desktop/src/runtime.rs`, `impulse-rs/impulse-desktop/src/ui.rs`
- Optional shared path not touched: `impulse-rs/impulse-ops/src/lib.rs`
- Blocked/shared paths: Cargo files, desktop source, terminal source, protocol docs/specs, validation scripts, root guidance docs, and unrelated dirty files
- Verification: `cd impulse-rs && cargo test -p impulse-rs ops_workbench`; `bash /Users/jamespustorino/.agents/skills/ralph-plan/scripts/validate-plan.sh --strict-v2 ralph-plan-6.md`; `git diff --check`

## Boundary Summary

The daemon owns project truth. The desktop runtime owns live PTY mechanics. The UI renders daemon truth and PTY glyph output, but does not become a second owner for project, artifact, supervisor, or telemetry state.

## Daemon-Owned Truth

These surfaces are authoritative in daemon/workbench code:

| Surface | Owner | Evidence | Boundary Rule |
|---|---|---|---|
| `ProjectOpsSnapshot` | Daemon | `ops_workbench::build_snapshot` builds from `SharedState`, `.impulse` files, retrieval metadata, artifacts, and terminal telemetry overlays | Desktop must consume `GetOpsSnapshot` or `SubscribeOps`; it must not synthesize independent project truth for agents, memory, retrieval, artifacts, or interventions |
| Artifact store | Daemon | `list_artifacts`, `get_artifact`, `run_artifact_action`, `save_artifact`, and daemon handlers for `ListArtifacts`, `GetArtifact`, `RunArtifactAction` | Desktop may request list/get/action results; artifact status and payload changes must be written through daemon-owned artifact actions |
| Supervisor policy | Daemon | `SupervisorPermissionPolicy`, `SupervisorPermissionState`, `GetSupervisorPermissions`, `SupervisorChat`, `RunSupervisorAction` | Desktop may display proposals and request actions; permission resolution, confirmation requirements, and policy changes stay daemon-side |
| Supervisor action approval | Daemon policy plus desktop local execution for PTY-only effects | Daemon returns `SupervisorActionResult` with `requires_confirmation`, `blocked`, `local_action`; desktop runtime currently enforces confirmation for `SendInput` before PTY write | The daemon decides whether an action is permitted and whether confirmation is required; desktop may execute only the returned local PTY action after confirmation |
| Terminal telemetry overlay store | Daemon | `TerminalOpsTelemetryStore` stores `TerminalOpsReport` by project/source, marks stale after 10s, purges after 60s, and overlays reports in `overlay_terminal_reports` | Desktop publishes telemetry reports; daemon merges and expires them. Desktop must not directly mutate `ProjectOpsSnapshot` agents/interventions/context |
| Ops update stream | Daemon | `SubscribeOps` returns `OpsSubscription { snapshot, events, next_seq }`; `GetOpsSnapshot` returns current snapshot | `ops_update` events emitted into desktop should carry daemon snapshot/subscription payloads, not UI-reconstructed panel state |

## Desktop-Runtime-Owned Mechanics

These surfaces are live runtime mechanics, not project truth:

| Surface | Owner | Evidence | Boundary Rule |
|---|---|---|---|
| PTY handles | Desktop runtime | `DesktopRuntime` owns `TerminalBackend` records keyed by agent id | Runtime may spawn/kill PTYs; daemon snapshot remains the durable workbench truth and receives terminal state only through `TerminalOpsReport` |
| Write queue | Desktop runtime | `write_agent` writes `AgentWriteRequest.data` into `TerminalBackend::write_queue` | UI input and supervisor local send-input must become bytes before crossing into Rust runtime; daemon does not own raw PTY stdin buffering |
| Resize/focus | Desktop runtime | `resize_agent`, `focus_agent`, `close_agent`, `snapshot_agents` | Runtime owns active dimensions and focused pane mechanics; published telemetry reflects those facts into daemon truth |
| Output event fanout | Desktop runtime | `DesktopEvent::TerminalOutput`, `TerminalExit`, `AgentRuntimeUpdate`; Tauri sink emits named events | Runtime emits byte streams to xterm and runtime snapshots to telemetry/report builders; UI must not infer project status from glyph content |
| `TerminalOpsReport` generation | Desktop runtime integration | `TerminalOpsReport` DTO lives in `impulse-ops`; daemon accepts it through `PublishTerminalOps` | Desktop should transform `snapshot_agents()` plus context/intervention observations into reports and publish them at heartbeat/change boundaries |

## UI-Rendered Surfaces

The UI is a renderer and command initiator:

| Surface | Render Source | Rule |
|---|---|---|
| xterm glyphs | `terminal_output` byte events from runtime | xterm renders bytes/layout only; glyph content is not parsed into authoritative agent/artifact/context state |
| Terminal tabs and pane layout | Dioxus component state plus runtime focus/resize commands | Layout state may be UI-local, but agent status and workbench panels must come from daemon snapshot/ops updates where applicable |
| Agent/workbench panels | Daemon `ops_update` / `ProjectOpsSnapshot` | Agent list, memory health, retrieval summary, interventions, artifacts, and supervisor permissions are rendered from daemon truth |
| Artifacts panel | Daemon `ListArtifacts`, `GetArtifact`, or snapshot `artifacts` | The UI may cache for rendering only; refresh must reconcile from daemon responses |
| Supervisor panel | Daemon `GetSupervisorPermissions`, `SupervisorChat`, `RunSupervisorAction` | The UI displays proposals, confirmation prompts, and results; it does not decide policy |

## `WorkbenchDaemonRequest` Artifact Gap

Current state:

- `DaemonRequest` includes `ListArtifacts { limit }`, `GetArtifact { artifact_id }`, and `RunArtifactAction { artifact_id, action_id, params }`.
- `impulse_ops::WorkbenchDaemonRequest` includes `RunArtifactAction`, but does not include `ListArtifacts` or `GetArtifact`.
- `impulse-rs/src/daemon/protocol.rs` has a compatibility test that proves shared `WorkbenchDaemonRequest` variants deserialize into `DaemonRequest`; that test cannot cover the missing list/get variants until the shared DTO adds them.

Concrete plan:

1. In a shared protocol/DTO lane, add these variants to `impulse-rs/impulse-ops/src/lib.rs`:
   - `ListArtifacts { #[serde(default)] limit: Option<usize> }`
   - `GetArtifact { artifact_id: String }`
2. Update `request_variant_name` and `test_shared_workbench_requests_deserialize_into_daemon_protocol` in `impulse-rs/src/daemon/protocol.rs` so shared DTO parity covers list/get/action.
3. Run targeted verification:
   - `cd impulse-rs && cargo test -p impulse-rs daemon::protocol::tests::test_shared_workbench_requests_deserialize_into_daemon_protocol`
   - `cd impulse-rs && cargo test -p impulse-rs ops_workbench`
   - `cd impulse-rs && cargo test -p impulse-ops`
4. Only after shared DTO parity is green, wire desktop artifact list/get calls through `WorkbenchDaemonRequest` instead of bespoke JSON request construction.

Loop 6 did not make this code change because the requested boundary artifact can be completed without claiming the optional shared path. The gap is concrete and low-risk, but it belongs in a code-owning protocol lane with targeted tests.

## TerminalOpsReport Publish/Subscribe Plan

Desktop must publish runtime telemetry and subscribe to daemon truth without becoming a second state owner:

1. Runtime report builder:
   - Add a desktop adapter that maps `DesktopRuntime::snapshot_agents()` into `TerminalOpsReport.agents`.
   - Set `source_id` to a stable desktop instance id, not a pane label.
   - Set `published_at` with daemon-compatible RFC3339 time.
   - Include context/intervention overlays only when the desktop runtime has direct runtime facts; do not duplicate daemon memory/retrieval/artifact summaries.
2. Publish cadence:
   - Publish `TerminalOpsReport` on spawn, resize, focus, exit, and periodic heartbeat.
   - Keep heartbeat below the daemon stale threshold; daemon currently marks telemetry stale after 10 seconds and purges after 60 seconds.
   - Treat `PublishTerminalOps` success as acceptance, not durable truth; the next `SubscribeOps` snapshot is the rendered truth.
3. Subscribe path:
   - Desktop calls `SubscribeOps { since_seq }` at startup and after each publish cycle.
   - Tauri emits `ops_update` with the returned `OpsSubscription` or snapshot payload.
   - Dioxus panels render from the daemon payload and store only render cache plus `next_seq`.
4. Conflict rule:
   - If runtime local snapshot and daemon `ops_update` disagree, xterm continues rendering live bytes, but panels prefer daemon truth.
   - Local runtime facts are reconciled only by publishing the next `TerminalOpsReport`; panels must not patch daemon snapshot client-side.

## Implementation Sequence For Later Loops

1. Protocol parity: close the `WorkbenchDaemonRequest` list/get artifact gap.
2. Daemon client adapter: add a desktop-side daemon request client that can call `GetOpsSnapshot`, `SubscribeOps`, `PublishTerminalOps`, `ListArtifacts`, `GetArtifact`, and `RunArtifactAction`.
3. Runtime telemetry adapter: map `AgentRuntimeSnapshot` into `TerminalOpsReport`.
4. Event bridge: emit `ops_update` only from daemon responses, while keeping `terminal_output` as byte-stream fanout to xterm.
5. UI panels: replace placeholder panel content with render-only projections of daemon snapshot truth.

## Non-Goals

- No Cargo manifest or lockfile changes in Loop 6.
- No changes to `impulse-desktop`, `impulse-term`, daemon handlers, shared DTOs, or protocol docs in Loop 6.
- No attempt to remove OpenCode compatibility in this loop.
- No normalization of unrelated dirty files.

## Handoff Notes

- The daemon already has the authoritative artifact handlers; the shared DTO is the mismatch.
- Desktop runtime already has enough snapshot data to build `TerminalOpsReport`, but the publishing/subscription adapter is not yet implemented.
- Future desktop panel work should start from this boundary before adding stateful UI stores.
