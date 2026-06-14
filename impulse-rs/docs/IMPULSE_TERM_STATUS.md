# impulse-term: Implementation Status & Daemon-Truth Gap

> **Updated:** 2026-03-05
> **Scope:** Historical/legacy EGUI-only status for the earlier Impulse operator workbench
> **Roadmap:** [`../../docs/ROADMAP-PLAN.md`](../../docs/ROADMAP-PLAN.md)
> **Risk register:** [`../../docs/HONEST-ROADMAP.md`](../../docs/HONEST-ROADMAP.md)

> **Status:** Historical. The active desktop direction is Dioxus Desktop; EGUI/`impulse-gui` is retained for compile-maintenance only.

---

## What Is Already Built

### impulse-term crate

The custom terminal widget stack in `impulse-rs/impulse-term/` is implemented and integrated into `impulse-gui`.

Delivered areas:

- PTY spawn and terminal parsing
- vt100 rendering
- keyboard/input handling
- ANSI theming
- context bridge with token estimation, extraction, compaction, and injection support
- terminal panel composition and GUI embedding

### EGUI workbench surfaces

The original “not started” status for context/status work is no longer accurate.

Implemented workbench surfaces now include:

- `Overview`
- `Agents`
- `Context`
- `Memory`
- `Artifacts`
- `Settings`

Operator-facing context/status concepts already exist in the GUI:

- cross-agent context summaries
- artifact review surfaces
- sidebar alerts
- status-bar workbench counts

---

## The Remaining Gap

The current remaining issue is not missing UI panes. It is authority and integration.

Some live terminal insight and context telemetry still originate in the GUI/runtime layer and must be published into the daemon so the daemon snapshot becomes the source of truth for:

- `Overview`
- `Agents`
- `Context`
- `Artifacts`
- sidebar alerts
- status bar

This was the daemon-truth EGUI pass before the Dioxus Desktop host direction.

---

## Daemon-Truth Requirements

### Telemetry publication

Terminal surfaces must publish `TerminalOpsReport` on:

- tab spawn
- tab shutdown
- context tier change
- compaction event
- injection event
- intervention list change
- 2-second heartbeat while the window is alive

### Daemon overlay behavior

The daemon should:

1. build the durable workbench snapshot from sessions, history, genome, retrieval, and artifacts
2. overlay fresh terminal telemetry by `session_id` first, then by agent `id`
3. expose unmatched telemetry as ephemeral agents
4. stop trusting telemetry after 10 seconds without heartbeat
5. purge telemetry-only entries after 60 seconds

### GUI rendering rule

The GUI workbench surfaces should render from daemon snapshot state, not from local shadow merges.

`Memory` may continue using its dedicated IPC path during this phase.

---

## What Changed Relative To The Original Plan

The earlier status doc said these items were not started:

- Context monitoring view
- Status bar enhancement

That is no longer true.

The accurate status is:

- context/status UI exists
- terminal telemetry publication is in progress
- daemon-truth consolidation is the active remaining work

---

## Validation Focus

Use this status doc with the main roadmap docs during implementation:

- [`../../docs/spec/RUST-CANONICAL-CONTRACT.md`](../../docs/spec/RUST-CANONICAL-CONTRACT.md)
- [`../../docs/ROADMAP-PLAN.md`](../../docs/ROADMAP-PLAN.md)
- [`../../docs/plans/IMPLEMENTATION-HANDOFF.md`](../../docs/plans/IMPLEMENTATION-HANDOFF.md)
- [`../../docs/HONEST-ROADMAP.md`](../../docs/HONEST-ROADMAP.md)

Required checks:

```bash
cd impulse-rs
cargo fmt --check
cargo check --all-features
cargo test
cargo clippy --all-features --all-targets -- -D warnings
```

Manual acceptance should confirm that context and artifact state visible in the GUI matches the daemon snapshot after reconnects, heartbeats, and artifact actions.
