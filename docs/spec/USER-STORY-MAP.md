---
title: User Story Map
description: Rust-first user stories and documentation baseline for the current Impulse product surface
version: '1.3'
updated: 2026-07-13
type: specification
category: core
phase: all
status: active
audience: builders
tags: [stories, acceptance, traceability, rust, roadmap]
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# User Story Map

> Purpose: turn the current Impulse documentation set into a single Rust-first story baseline.
> Contract anchor: [`RUST-CANONICAL-CONTRACT.md`](./RUST-CANONICAL-CONTRACT.md)
> Test map: [`TEST-TRACEABILITY.md`](./TEST-TRACEABILITY.md)

## Documentation Baseline

### Authoritative now

These documents describe the current product with the least drift:

- [`RUST-CANONICAL-CONTRACT.md`](./RUST-CANONICAL-CONTRACT.md) for product scope, interfaces, and roadmap
- [`../../VISION.md`](../../VISION.md) for the living product north star and complete vertical slice
- [`../INDEX.md`](../INDEX.md) for current Now/Next/Later navigation
- [`../guides/HOOK-VALIDATION-GUIDE.md`](../guides/HOOK-VALIDATION-GUIDE.md) for proof-of-truth validation
- [`../../AGENTS.md`](../../AGENTS.md) for contributor rules and test expectations

### Supporting orientation

These documents help readers enter or navigate the product, but the canonical contract remains the
implementation baseline:

- [`../../README.md`](../../README.md) is the product entry point and quick start
- `../SUMMARY.md` mirrors the `../SUMMARY.yaml` navigation source; use the canonical contract for implementation truth

### Legacy or drifted

The following documents do not match the current Rust implementation and should only be read as historical context:

- [`../guides/TESTING-FRAMEWORK.md`](../guides/TESTING-FRAMEWORK.md) still describes a TypeScript/Vitest `impulse/` layout instead of the live `impulse-rs` workspace
- [`../guides/TOOLS-STATUS.md`](../guides/TOOLS-STATUS.md) still reflects earlier TypeScript/Bun-era tool assumptions and should not be used as a current setup contract

## Story Status Scale

| Status | Meaning |
| --- | --- |
| Implemented | Present in the current Rust workspace and verified by the referenced tests |
| In progress | Active roadmap work with shipped partial behavior |
| Planned | On the roadmap but not yet a current delivery claim |
| Validation required | Implemented code exists, but product claims still depend on real-world proof |

## Story Groups

### A. Session Memory Core

#### ST-01 Start a tracked coding session

As a coding agent operator, I want to start a session with a name and platform so Impulse can create durable continuity from the first hook or command.

Status: Implemented

Acceptance criteria:

- `session-start` creates or updates project state under `.impulse/`
- a session ID is returned for downstream tracking
- the active session becomes visible through status and daemon surfaces

Primary interfaces:

- `session-start`
- daemon `CreateSession`

#### ST-02 End a session with a durable summary

As a coding agent operator, I want to end a session with a summary and optional verification so the next session can recall useful prior work.

Status: Implemented

Acceptance criteria:

- `session-end` records the session summary in history
- `session-end --verify` enforces the verification gate before completion
- completed session state is queryable later

Primary interfaces:

- `session-end --summary ...`
- `session-end --verify`
- daemon `EndSession`

#### ST-03 Track files and tools touched during work

As a coding agent operator, I want file writes and tool usage recorded during a session so Impulse can reconstruct what happened.

Status: Implemented

Acceptance criteria:

- `track-write` records touched files against the active session
- `track-tool` records tool usage against the active session
- tracked activity appears in status and history-derived outputs

Primary interfaces:

- `track-write`
- `track-tool`
- daemon `TrackFile`
- daemon `TrackTool`

### B. Memory Inspection and Retrieval

#### ST-04 Inspect project memory quickly

As a developer returning to a project, I want fast human-readable memory views so I can understand the current state before making changes.

Status: Implemented

Acceptance criteria:

- `status`, `history`, `genome`, `activity`, `summary`, and `health` return readable project state
- these commands tolerate partially populated `.impulse/` state without panicking

Primary interfaces:

- `status`
- `history`
- `genome`
- `activity`
- `summary`
- `health`
- `system`
- `analyze`

#### ST-05 Search prior work with safe fallback behavior

As a developer returning to a project, I want history and genome search to work even when advanced retrieval backends are unavailable.

Status: Implemented

Acceptance criteria:

- `index-memory` builds retrieval state without corrupting durable memory artifacts
- `search-history` and `search-genome` support keyword and semantic flows
- semantic mode falls back safely and explains what backend was actually used
- `retrieval-status --check --json` exposes retrieval health and fallback conditions

Primary interfaces:

- `index-memory`
- `search-history`
- `search-genome`
- `retrieval-status`

### C. Context Orchestration

#### ST-06 Stage context before injecting it

As a coding agent operator, I want review-first context injection so Impulse helps with continuity without silently modifying agent context.

Status: Implemented

Acceptance criteria:

- default injection mode is review, not silent apply
- staged injections produce durable review artifacts when configured to do so
- direct and daemon flows both respect `off|review|apply` overrides

Primary interfaces:

- `orchestrate --inject-mode ...`
- `handoff --inject-mode ...`
- `sync-context --inject-mode ...`
- daemon chat with injection overrides

#### ST-07 Produce handoff artifacts for the next agent or session

As an operator supervising multiple agent runs, I want explicit handoff artifacts so context movement is visible and auditable.

Status: Implemented

Acceptance criteria:

- `handoff` writes a handoff artifact in `.impulse/context/`
- `sync-context` refreshes shared current-task artifacts
- routing and injection decisions are auditable in append-only logs

Primary interfaces:

- `handoff`
- `sync-context`
- `.impulse/context/handoff-*.md`
- `.impulse/context/routing-log.jsonl`
- `.impulse/context/injections/injection-log.jsonl`

### D. Daemon and Operator Surfaces

#### ST-08 Use a daemon as the long-lived source of truth

As an operator, I want a daemon-backed control plane so long-lived sessions, tools, and workbench surfaces share one authoritative state model.

Status: Implemented

Acceptance criteria:

- the Unix socket protocol serves session, tool, guard, steward, and workbench requests
- daemon lifecycle operations avoid unsafe crashes on malformed input
- direct CLI and daemon workflows can coexist against the same project state

Primary interfaces:

- `daemon`
- JSON-line IPC protocol v3

#### ST-09 Observe work through the Dioxus desktop host

As an operator, I want Overview, Agents, Context, Memory, and Artifacts views in the Dioxus Desktop shell so I can supervise agent work without reading raw state files.

Status: In progress

Acceptance criteria:

- `impulse-desktop` renders the current workbench surfaces
- daemon snapshots provide the durable read model for the active workbench surfaces
- live terminal telemetry can be overlaid without replacing durable daemon truth

Primary interfaces:

- `impulse-desktop`
- `GetOpsSnapshot`
- `SubscribeOps`
- `PublishTerminalOps`
- `ListArtifacts`
- `GetArtifact`
- `RunArtifactAction`

### E. Stewardship and Safety

#### ST-10 Review risky context and stewardship actions explicitly

As an operator, I want stewardship flows to require explicit review so context compaction and approval decisions stay intentional.

Status: Implemented

Acceptance criteria:

- `steward status` exposes pending work
- `steward analyze` produces reviewable proposals
- `steward approve` and `steward reject` apply operator intent explicitly

Primary interfaces:

- `steward status`
- `steward analyze`
- `steward compact`
- `steward approve`
- `steward reject`

#### ST-11 Enforce verification-before-completion

As an operator, I want completion-sensitive flows to verify state before claiming success so Impulse does not normalize false confidence.

Status: Implemented

Acceptance criteria:

- `verify` produces an explicit result
- session-ending verification can fail hard when requirements are not met
- invalid direct requests return consistent error behavior

Primary interfaces:

- `verify`
- `session-end --verify`

### F. Validation and Governed Agent Control

#### ST-12 Prove the real hook memory loop before expanding claims

As the product owner, I want real hook-loop evidence so roadmap claims stay tied to demonstrated behavior instead of internal assumptions.

Status: Validation required

Acceptance criteria:

- SessionStart evidence can be captured and inspected
- PreCompact evidence can be captured and inspected
- the documentation records pass or fail honestly

Primary interfaces:

- `validate-hooks --platform claude-code`
- validation artifacts under `.impulse/validation/`

#### ST-13 Complete one governed supervisor-and-builder vertical slice

As an operator, I want one supervisor and one builder to complete a scoped task under explicit
backend policy so Impulse proves that its existing controls compose into a governed workflow.

Status: In progress

Acceptance criteria:

- the operator launches registered supervisor and builder runtimes against an explicit workspace target
- the supervisor observes daemon-owned workbench truth and can focus the builder or send confirmed input under `SupervisorPermissionPolicy`
- the builder receives a bounded assignment and produces reviewable implementation and verification evidence
- completion distinguishes worker claims, observed evidence, supervisor judgment, and operator approval
- terminal launch, write, focus, close, and lifecycle telemetry remain projections of daemon/control-plane truth rather than a competing state store
- role-specific permissions are enforced by the backend for this slice, with unsupported enforcement reported honestly

Primary interfaces:

- `AgentRegistry` / `AgentPlatformId`
- `AgentRoleId` / `AgentRoleAssignment` / `RoleCompatibility`
- `DesktopRuntime::{spawn_agent, write_agent, focus_agent, close_agent}`
- daemon `PublishTerminalOps`, `SubscribeOps`, and `RunSupervisorAction`
- `SupervisorPermissionPolicy` / `SupervisorPermissionState`

Live boundary:

- the Dioxus Builder launcher now requires an explicit bounded task and product-role assignment, previews trusted static compatibility, and fails closed when its platform catalog is unavailable
- the backend canonicalizes the workspace, repeats compatibility evaluation, blocks unsatisfied mandatory requirements before agent-id reservation or PTY creation, and preserves task/assignment/result telemetry
- one end-to-end supervisor/builder governed-run lifecycle carrying evidence through verification, supervisor judgment, and operator approval is not yet complete
- generalized role composition, a common dynamic runtime-adapter trait, and capability negotiation across arbitrary runtimes remain target architecture; this narrow preflight must not imply that every runtime has equal or continuous enforcement

## Story Priority

### Must stay stable now

- ST-01 through ST-08
- ST-10 and ST-11

### Active delivery lane

- ST-09
- ST-12
- ST-13

### Later architecture beyond this story map

- generalized runtime-independent role contracts
- common runtime-adapter operations and enforcement-strength negotiation
- typed multi-agent delegation and cross-project messaging

## How To Use This Document

Use this file when deciding whether a change is:

- a regression against a current product claim
- a roadmap follow-on that should not be described as complete
- a documentation drift issue that must be corrected in the contract, roadmap, or test map
