---
title: Dynamic Roadmap Plan
description: Actionable next steps based on the current Rust and EGUI product state
version: '1.2'
updated: 2026-03-05
type: doc
category: roadmap
phase: all
status: active
audience: builder
tags: [roadmap, action, planning, egui, daemon]
last_updated: 2026-03-05
authors:
  - name: James Pustorino
    role: Creator
---

# Dynamic Roadmap Plan — Impulse

> **Updated:** 2026-03-05
> **Purpose:** Record the actual active roadmap after the Rust EGUI workbench landed.
> **Risk register:** [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md)
> **Execution handoff:** [`plans/IMPLEMENTATION-HANDOFF.md`](./plans/IMPLEMENTATION-HANDOFF.md)

---

## Roadmap Contract

| Stage | Focus | Status |
|------|-------|--------|
| **Now** | Rust memory core + hooks + retrieval/injection + EGUI operator workbench | Active |
| **Next** | Daemon-truth EGUI integration + hook/compaction validation | Active |
| **Later** | Agent control + artifact polish + deeper coordination UX | Planned |

This document is intentionally aligned with [`spec/RUST-CANONICAL-CONTRACT.md`](./spec/RUST-CANONICAL-CONTRACT.md). If another active doc conflicts, the contract wins.

---

## Current State Summary

| Area | Status | Reality |
|------|--------|---------|
| Rust memory core | Implemented | Session tracking, genome/history, retrieval, injection, stewardship, daemon, and tool/runtime infrastructure are live. |
| EGUI operator workbench | In progress | `Overview`, `Agents`, `Context`, `Memory`, `Artifacts`, and `Settings` exist in `impulse-gui`. |
| Artifact model | In progress | Provider-neutral artifact envelopes and actions exist, but operator ergonomics still need polish. |
| Daemon-truth workbench | In progress | Snapshot/artifact IPC exists; terminal telemetry publication and overlay are now the active integration track. |
| Hook validation | Not yet validated | SessionStart injection, PreCompact survival, and real-world `GENOME.md` usefulness remain open risks. |
| Structural blocking | Deferred | Remains gated behind validation evidence from the honest roadmap. |

The old `Dashboard/Advanced UX` framing is obsolete. The product already has an active Rust-native EGUI surface. The remaining work is to make that surface operationally authoritative and easier to supervise.

---

## Immediate Sequence

### 1. Documentation Reset

Update the active docs together so they describe the same product:

- `docs/spec/RUST-CANONICAL-CONTRACT.md`
- `AGENTS.md`
- `CLAUDE.md`
- `docs/INDEX.md`
- `docs/SUMMARY.yaml`
- `docs/SUMMARY.md`
- `docs/ROADMAP-PLAN.md`
- `docs/plans/IMPLEMENTATION-HANDOFF.md`
- `impulse-rs/docs/IMPULSE_TERM_STATUS.md`

Required outcome:

- EGUI/operator workbench is marked as active work, not future dashboard work.
- `HONEST-ROADMAP.md` remains the canonical risk register.
- The roadmap sequence becomes:
  1. documentation reset
  2. daemon-truth EGUI pass
  3. parallel hook/compaction validation
  4. agent-control and artifact-polish follow-ons

### 2. Daemon-Truth EGUI Pass

Make the daemon the authoritative source of workbench state for:

- `Overview`
- `Agents`
- `Context`
- `Artifacts`
- sidebar alerts
- status bar summaries

The daemon snapshot remains the read model:

- `ProjectOpsSnapshot`

New shared publication model:

- `TerminalOpsReport`

New daemon publication request:

- `PublishTerminalOps { report: TerminalOpsReport }`

Implementation rules:

- Terminal telemetry is ephemeral daemon memory, not a new persisted file.
- Durable snapshot data is built first from sessions, history, genome, retrieval, and artifacts.
- Fresh terminal telemetry overlays onto that durable snapshot.
- GUI workbench surfaces must stop relying on local shadow merges for agent/context/artifact state.

### 3. Parallel Validation Track

Run the unresolved proof work from [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md):

- SessionStart stdout injection
- PreCompact survival
- real-world `GENOME.md` usefulness

If any of these fail, update the roadmap docs immediately. Failed validation is a roadmap correction, not a future TODO.

---

## Daemon-Truth Workbench Contract

### Shared Types

`ProjectOpsSnapshot` stays canonical for EGUI reads and includes:

- project metadata
- active agent runtime view
- context health summary
- intervention recommendations
- retrieval and memory summaries
- recent artifacts

`TerminalOpsReport` carries ephemeral terminal-side telemetry:

- `source_id`
- `published_at`
- `agents`
- `context`
- `interventions`

### Overlay Rules

- Match telemetry onto durable agents by `session_id` first, then agent `id`.
- Expose unmatched telemetry as ephemeral agents in the snapshot.
- Mark telemetry stale after 10 seconds without heartbeat.
- Stop overlaying stale telemetry after 10 seconds.
- Purge telemetry-only entries after 60 seconds.

### GUI Update Loop

- Bootstrap with `GetOpsSnapshot`.
- Use `SubscribeOps` as the primary refresh path.
- Reconcile with a full snapshot every 15 seconds and on reconnect.
- Keep dedicated history/genome/search polling only for the `Memory` view in this phase.

---

## Follow-On Order After Daemon Truth

### Lane 1: Agent Control

Only start after the daemon-truth workbench is stable.

- blocked-work indicators
- focus affordances
- handoff entry points
- restart entry points
- conflict review entry points

### Lane 2: Artifact Polish

Only start after agent control has a clear execution path.

- clearer review/apply ergonomics
- stronger risky-action confirmation flows
- better post-action result presentation
- tighter artifact action intentionality

### Explicitly Deferred

- structural conflict blocking
- broad retrieval redesign
- TUI expansion
- web UI

---

## Validation Expectations

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

- Run the GUI with live terminal tabs and verify status-bar/sidebar counts match the daemon snapshot.
- Trigger context tier changes and verify `Context` and `Overview` update without local-only state.
- Disconnect and reconnect the daemon and verify snapshot recovery.
- Apply or acknowledge an artifact and verify the visible state changes only after the daemon snapshot updates.

---

## What This Roadmap Does Not Claim

This roadmap does **not** claim that the hook assumptions are validated yet. It also does **not** claim that structural blocking is safe to enable. The honest roadmap still governs those risks.
