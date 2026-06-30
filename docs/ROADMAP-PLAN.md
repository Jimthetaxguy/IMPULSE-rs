---
title: Dynamic Roadmap Plan
description: Actionable next steps based on the current Rust and Dioxus desktop host product state
version: '1.2'
updated: 2026-06-14
type: doc
category: roadmap
phase: all
status: active
audience: builder
tags: [roadmap, action, planning, dioxus, desktop, daemon]
last_updated: 2026-06-14
authors:
  - name: Impulse Maintainers
    role: Maintainer
---

# Dynamic Roadmap Plan — Impulse

> **Updated:** 2026-06-14
> **Purpose:** Record the active roadmap after the desktop contract reset to Dioxus Desktop.
> **Risk register:** [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md)
> **Execution handoff:** [`plans/IMPLEMENTATION-HANDOFF.md`](./plans/IMPLEMENTATION-HANDOFF.md)

---

## Roadmap Contract

| Stage | Focus | Status |
|------|-------|--------|
| **Now** | Rust memory core + hooks + retrieval/injection + Dioxus desktop host | Active |
| **Next** | Dioxus Desktop launch scaffold + live terminal bridge parity | [x] Complete (scaffold + bridge parity via smoke + real dispatch_host_invoke/LiveHostContext + Mcp execute unit tests + registry centralization; autoresearch cleanup; capture evidence + full verif gate; pushed) |
| **Later** | Daemon parity in desktop shell + agent control + artifact polish | Planned |
| **Cleanup** | Full egui decommission (remove frozen `impulse-gui` + `impulse-term` egui layer) | Planned — gated on Dioxus host operationally authoritative; see [`plans/EGUI-DECOMMISSION.md`](./plans/EGUI-DECOMMISSION.md) |

This document is intentionally aligned with [`spec/RUST-CANONICAL-CONTRACT.md`](./spec/RUST-CANONICAL-CONTRACT.md). If another active doc conflicts, the contract wins.

> **Enhancement backlog:** [`LONG-RANGE-ENHANCEMENTS.md`](./LONG-RANGE-ENHANCEMENTS.md) organizes the full PR queue by theme (33 PRs across 8 lanes).

---

## Current State Summary

| Area | Status | Reality |
|------|--------|---------|
| Rust memory core | Implemented | Session tracking, genome/history, retrieval, injection, stewardship, daemon, and tool/runtime infrastructure are live. |
| Dioxus desktop host | Scaffold parity complete | `impulse-desktop` contains Dioxus shell, typed host bridge, smoke asserting live "dioxus-eval-bridge-ready", exercised commands, real dispatch_host_invoke + body tests; registry central + error prop; daemon parity later. |
| Agent harness wiring | **COMPLETE** | All 10 features wired: context→prompts, intent classification, full coordination, conflict history IPC, JSON harness protocol, session awareness, specialized IPC (2026-03-31). |
| Artifact model | In progress | Provider-neutral artifact envelopes and actions exist, but operator ergonomics still need polish. |
| Daemon-truth desktop panels | Planned | Snapshot/artifact IPC exists; desktop shell must render daemon snapshots rather than frontend-local truth. |
| Hook validation | Not yet validated | SessionStart injection, PreCompact survival, and real-world `GENOME.md` usefulness remain open risks. |
| Structural blocking | Deferred | Remains gated behind validation evidence from the honest roadmap. |

The old `Dashboard/Advanced UX`, active-EGUI framing, and Tauri-as-target framing are obsolete. The remaining desktop work is to make the Dioxus Desktop host operationally authoritative while preserving the ratatui path.

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

- Dioxus Desktop is marked as the active desktop host.
- egui / `impulse-gui` is marked legacy/frozen, not active feature work.
- `HONEST-ROADMAP.md` remains the canonical risk register.
- The roadmap sequence becomes:
  1. documentation reset
  2. Dioxus Desktop launch scaffold
  3. host command/event parity
  4. live terminal bridge
  5. daemon parity and artifact polish

### 2. Desktop Boundary And Daemon-Truth Pass

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
- Desktop shell surfaces must render daemon snapshots rather than maintaining local shadow truth for agent/context/artifact state.

### 3. Parallel Validation Track

Run the unresolved proof work from [`HONEST-ROADMAP.md`](./HONEST-ROADMAP.md):

- SessionStart stdout injection
- PreCompact survival
- real-world `GENOME.md` usefulness

If any of these fail, update the roadmap docs immediately. Failed validation is a roadmap correction, not a future TODO.

---

## Daemon-Truth Workbench Contract

### Shared Types

`ProjectOpsSnapshot` stays canonical for desktop shell reads and includes:

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

### Desktop Update Loop

- Bootstrap with `GetOpsSnapshot`.
- Use `SubscribeOps` as the primary refresh path.
- Reconcile with a full snapshot every 15 seconds and on reconnect.
- Keep dedicated history/genome/search polling only for the `Memory` view in this phase.

---

## Follow-On Order After Daemon Truth

### Lane 1: Agent Control

Only start after the daemon-truth workbench is stable. **Historical note:** the agent harness wiring landed in the 2026-03-31 Ralph Plan 3 cycle; the current roadmap should validate the live daemon and Dioxus host paths rather than treat the old plan as the active source of truth.
- Context→prompt pipeline (ExtractedInsight → prompts)
- Intent classification at 9 extraction sites
- Full coordination pipeline + pane summaries
- Conflict history IPC (GetConflictHistory, ClearResolvedConflicts)
- Structured JSON harness protocol with fallback
- Session history (5-turn bound, cached_agent)
- Specialized IPC endpoints (AgentReviewCode, AgentAnalyzeError, AgentSummarizePane)

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
