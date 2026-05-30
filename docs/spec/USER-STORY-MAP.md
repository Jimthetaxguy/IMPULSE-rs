---
title: User Story Map
description: Rust-first user stories and documentation baseline for the current Impulse product surface
version: '1.0'
updated: 2026-04-02
type: specification
category: core
phase: all
status: active
audience: builders
tags: [stories, acceptance, traceability, rust, roadmap]
authors:
  - name: James Pustorino
    role: Creator
---

# User Story Map

> Purpose: turn the current Impulse documentation set into a single Rust-first story baseline.
> Contract anchor: [`RUST-CANONICAL-CONTRACT.md`](./RUST-CANONICAL-CONTRACT.md)
> Test map: [`TEST-TRACEABILITY.md`](./TEST-TRACEABILITY.md)

## Documentation Baseline

### Authoritative now

These documents describe the current product with the least drift:

- [`RUST-CANONICAL-CONTRACT.md`](./RUST-CANONICAL-CONTRACT.md) for product scope, interfaces, and roadmap
- [`../ROADMAP-PLAN.md`](../ROADMAP-PLAN.md) for sequencing and active delivery lanes
- [`../plans/IMPLEMENTATION-HANDOFF.md`](../plans/IMPLEMENTATION-HANDOFF.md) for execution order
- [`../guides/HOOK-VALIDATION-GUIDE.md`](../guides/HOOK-VALIDATION-GUIDE.md) for proof-of-truth validation
- [`../../AGENTS.md`](../../AGENTS.md) for contributor rules and test expectations

### Useful but fragmented

These documents contain good material, but they are not a unified execution baseline:

- [`../../README.md`](../../README.md) explains the product at a high level
- [`../INDEX.md`](../INDEX.md) is the navigation hub
- `../SUMMARY.md` and `../SUMMARY.yaml` have been active navigation surfaces, but they drifted from the live Rust docs set and should not be treated as a source of truth until fully re-baselined
- [`../guides/TESTING-STRATEGY-ENHANCEMENTS.md`](../guides/TESTING-STRATEGY-ENHANCEMENTS.md) has reusable testing categories

### Legacy or drifted

The following document does not match the current Rust implementation and should only be read as historical context:

- [`../guides/TESTING-FRAMEWORK.md`](../guides/TESTING-FRAMEWORK.md) still describes a TypeScript/Vitest `impulse/` layout instead of the live `impulse-rs` workspace
- [`../guides/TOOLS-STATUS.md`](../guides/TOOLS-STATUS.md) still reflects earlier TypeScript/Bun-era tool assumptions and should not be used as a current setup contract
- `README.md` test-count statements are historically useful but not a verification signal; use the live Rust verification gate instead

## Story Status Scale

| Status | Meaning |
| --- | --- |
| Implemented | Shipped in the current Rust workspace and expected to stay working |
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
- JSON-line IPC protocol v2

#### ST-09 Observe work through the Tauri desktop shell

As an operator, I want Overview, Agents, Context, Memory, and Artifacts views in the Tauri+Dioxus desktop shell so I can supervise agent work without reading raw state files.

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

### F. Validation and Future Claims

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

#### ST-13 Add agent control after daemon truth is stable

As an operator, I want direct supervision affordances for running agents after the workbench state model is authoritative.

Status: Planned

Acceptance criteria:

- blocked-work indicators and conflict-review entry points exist
- restart and handoff controls map to daemon-backed state
- artifact actions and agent control flows do not fork the source of truth

Primary interfaces:

- future daemon and workbench control surfaces

## Story Priority

### Must stay stable now

- ST-01 through ST-08
- ST-10 and ST-11

### Active delivery lane

- ST-09
- ST-12

### Deferred until current lane is validated

- ST-13

## How To Use This Document

Use this file when deciding whether a change is:

- a regression against a current product claim
- a roadmap follow-on that should not be described as complete
- a documentation drift issue that must be corrected in the contract, roadmap, or test map
