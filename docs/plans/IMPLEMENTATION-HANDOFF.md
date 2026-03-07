---
title: Implementation Handoff — EGUI Daemon Truth
description: Execution handoff for the EGUI roadmap reset and daemon-truth workbench pass
version: '1.1'
updated: 2026-03-05
type: doc
category: handoff
phase: all
status: active
audience: builder
tags: [handoff, implementation, egui, daemon, telemetry]
authors:
  - name: James Pustorino
    role: Creator
---

# Implementation Handoff Document

> **Updated:** 2026-03-05
> **Purpose:** Capture the actual next implementation sequence for Impulse.
> **Risk register:** [`../HONEST-ROADMAP.md`](../HONEST-ROADMAP.md)
> **Roadmap anchor:** [`../ROADMAP-PLAN.md`](../ROADMAP-PLAN.md)

---

## Executive Summary

The old handoff sequence was stale. The real next sequence is:

1. documentation reset
2. daemon-truth EGUI pass
3. parallel hook/compaction validation
4. agent-control and artifact-polish follow-ons

This is EGUI-only work for Impulse. No web UI, no TUI expansion, and no broad retrieval redesign in this pass.

---

## Why The Sequence Changed

The current codebase already has a Rust-native operator workbench:

- `impulse-gui` has `Overview`, `Agents`, `Context`, `Memory`, `Artifacts`, and `Settings`
- the daemon already exposes workbench snapshot and artifact endpoints
- the shared Rust ops model exists and is being consumed by the GUI

So the product is no longer in a “dashboard someday” phase. The remaining gap is authority and consistency:

- some terminal/context telemetry still originates in the GUI
- roadmap docs still describe EGUI as future work
- hook/compaction claims still need evidence from the honest roadmap

---

## Scope

### In Scope

- documentation re-baseline
- daemon-owned workbench state for EGUI surfaces
- terminal telemetry publication and merge rules
- artifact action feedback through daemon snapshots
- validation documentation for open hook/compaction risks

### Out of Scope

- TUI work
- web UI
- retrieval architecture redesign
- structural conflict blocking
- new coordination engines

---

## Shared Interface Contract

### Canonical Read Model

`ProjectOpsSnapshot` remains the only workbench read model for:

- `Overview`
- `Agents`
- `Context`
- `Artifacts`
- sidebar alerts
- status bar summaries

`Memory` can keep its current dedicated history/genome/search IPC path in this phase.

### New Publication Path

Daemon IPC request:

```rust
PublishTerminalOps { report: TerminalOpsReport }
```

Shared model payload:

```rust
TerminalOpsReport {
    source_id: String,
    published_at: String,
    agents: Vec<AgentRuntime>,
    context: ContextHealthSummary,
    interventions: Vec<InterventionRecommendation>,
}
```

---

## Implementation Track 1: Documentation Reset

Update these files together:

- `docs/spec/RUST-CANONICAL-CONTRACT.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/INDEX.md`
- `docs/SUMMARY.yaml`
- `docs/SUMMARY.md`
- `docs/ROADMAP-PLAN.md`
- `docs/plans/IMPLEMENTATION-HANDOFF.md`
- `impulse-rs/docs/IMPULSE_TERM_STATUS.md`

Required outcomes:

- EGUI is described as in progress.
- “Dashboard / Advanced UX” is no longer framed as deferred speculative work.
- `HONEST-ROADMAP.md` is explicitly linked as the risk register.
- The roadmap contract stays consistent across all top-level docs.

---

## Implementation Track 2: Daemon-Truth EGUI Pass

### Daemon Responsibilities

Maintain an in-memory telemetry store keyed by:

- `project_id`
- `source_id`

Merge behavior:

1. Build the durable snapshot from sessions, history, genome, retrieval, and artifacts.
2. Overlay fresh terminal telemetry onto matching agents by `session_id` first, then by agent `id`.
3. Expose unmatched telemetry as ephemeral agents.
4. Merge telemetry context and intervention data into the snapshot.
5. Mark telemetry stale after 10 seconds without heartbeat.
6. Stop overlaying stale telemetry after 10 seconds.
7. Purge telemetry-only entries after 60 seconds.

### GUI Responsibilities

Terminal surfaces publish `TerminalOpsReport` on:

- tab spawn
- tab shutdown
- context tier change
- compaction event
- injection event
- intervention list change
- a 2-second heartbeat while the window is alive

Workbench read path rules:

- `Overview`, `Agents`, `Context`, `Artifacts`, sidebar, and status bar render from daemon snapshot only.
- Remove local shadow merges for agent/context/intervention/artifact result state.
- Artifact actions remain remote; visible post-action state must return through the daemon snapshot.

### Update Loop

- connect: `GetOpsSnapshot`
- steady state: `SubscribeOps`
- reconciliation: full snapshot every 15 seconds and on reconnect
- `Memory` view may continue polling history/genome/search independently in this phase

---

## Implementation Track 3: Parallel Validation

The following are still not validated and must be documented honestly:

- SessionStart stdout injection
- PreCompact survival
- real-world `GENOME.md` usefulness

Rules for this track:

- record actual pass/fail evidence
- update [`../HONEST-ROADMAP.md`](../HONEST-ROADMAP.md) with the result
- if validation fails, correct roadmap docs immediately

Do not hide failed validation behind “future work.” It changes the product claim.

---

## Follow-On Order

### Next Lane: Agent Control

- blocked-work indicators
- focus affordances
- handoff affordances
- restart affordances
- conflict review entry points

### Then: Artifact Polish

- review/apply UX cleanup
- stronger risky-action confirmation
- clearer action result handling
- tighter operator intent around apply/re-run/handoff flows

### Still Deferred

- structural blocking
- broader retrieval changes
- new TUI capabilities
- non-Rust UI surfaces

---

## Verification Checklist

### Documentation

```bash
python3 docs/validate_docs.py --contract
```

### Rust

```bash
cd impulse-rs
cargo fmt --check
cargo check --all-features
cargo test
cargo clippy --all-features --all-targets -- -D warnings
```

### Manual Acceptance

- Start the GUI with live terminal tabs and verify status-bar/sidebar counts match daemon snapshot counts.
- Trigger context tier changes and verify `Context` and `Overview` update without local-only state.
- Disconnect and reconnect the daemon and verify snapshot recovery.
- Acknowledge or apply an artifact and verify the visible state change arrives via daemon refresh, not GUI-local mutation.

---

## Completion Criteria

This handoff is complete when:

- the docs no longer describe EGUI as future work
- the daemon is the authoritative workbench source for the targeted EGUI surfaces
- telemetry heartbeat, stale cutoff, and purge behavior are covered by tests
- artifact actions visibly round-trip through the daemon snapshot
- the honest roadmap remains accurate about what is still unverified
