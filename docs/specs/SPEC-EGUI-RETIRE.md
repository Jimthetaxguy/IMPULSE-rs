---
id: SPEC-EGUI-RETIRE
title: Retire egui from the impulse-rs workspace
status: in-progress
domain: gui
priority: high
created: 2026-04-23
owner: maintainers
supersedes:
  - docs/plans/2026-04-23-impulse-gui-decouple-from-egui-adapter.md
  - docs/plans/2026-04-23-impulse-gui-retire-egui-supervisor-cutover.md
---

# Retire egui from the impulse-rs workspace

## Why

egui is an immediate-mode GUI. For the workbench's typical 80×40 grid it
allocates ~3,200 galleys per frame, producing memory churn that the user
hit in production. The fix is structural, not parameter-tuning: move the
workbench to a retained-mode renderer (Dioxus 0.6 over the system webview)
that diffs only changed cells.

## What "done" looks like

1. Default `cargo build -p impulse-supervisor --features experimental-runtime`
   produces a usable Dioxus desktop binary that replaces `impulse-gui` for
   end-users.
2. `impulse-gui` and `impulse-term` (egui adapter) are archived under
   `_archive-YYYY-MM-DD-LXX/`.
3. `default-members` in the workspace `Cargo.toml` no longer references
   either retired crate.
4. `cargo test --workspace` stays green throughout; ratatui binary stays
   as a separate target (out of scope for this spec).

## Boundary decisions (locked)

These were front-loaded so individual stories don't re-litigate them:

| Decision | Choice | Rationale |
|---|---|---|
| In-place vs parallel rewrite | **In-place** — grow `impulse-supervisor` to absorb each surviving view, then delete `impulse-gui` | Avoids a long-lived parallel codebase; each view ships behind a feature flag until parity |
| ratatui TUI | **Stays** — separate binary, separate audience | TUI users don't share the egui memory bug |
| `@impulse` wire format | **JSON line** over `IMPULSE_CMD_SOCKET`, intercepted by hook from PTY input | Toolkit-neutral; lives in `impulse-term-core`; works for both Dioxus and any future renderer |
| vt100 parser | **Stays** — alacritty_terminal swap deferred | Audit (L178) showed vt100 is not the bottleneck; swap is a separate spec |

## Scope: 9 surviving views to port

Per the L180 view audit (`docs/research/2026-04-23-impulse-gui-view-audit.md`):

- WIRED top-level: `overview`, `terminals`, `memory`, `settings`
- EMBEDDED: `genome`, `search`, `sessions` (under memory); `terminal_search` (under terminals)

Already deleted: `guardrails`, `context`, `artifacts` (3 truly dead, 1,148 LOC).
Kept as live extension files: `terminal_context`, `terminal_insights`,
`memory_persistence` (`impl TerminalsView { … }` blocks; merge later).

## Stories

Each story below is implemented independently. Story IDs match Ralph loop
numbers for traceability with the existing commit history.

### Tranche A — Connect & Privilege

- [STORY-180](../stories/STORY-180-view-audit.md) — view audit (✅ done)
- [STORY-180b](../stories/STORY-180b-archive-dead-views.md) — archive 3 dead views (✅ done)
- [STORY-181](../stories/STORY-181-blocklist-into-supervisor.md) — wire BlockListView into supervisor worker panes (✅ done)
- [STORY-182](../stories/STORY-182-supervisor-tagged-spawn.md) — privileged spawn env-vars (✅ done)
- [STORY-183](../stories/STORY-183-impulse-wire-format.md) — `@impulse` wire format detector (next)
- [STORY-184](../stories/STORY-184-summarize-verb.md) — first verb: `summarize`

### Tranche B — Workbench Views (port to Dioxus)

- [STORY-185-194](../stories/STORY-185-tranche-b-port-views.md) — port the 9 surviving views in the audit-set order (sidebar+status_bar+command_palette → memory+sessions → genome+search → settings → terminals → overview)

### Tranche C — Cutover & Archive

- [STORY-195-199](../stories/STORY-195-tranche-c-cutover.md) — `experimental-runtime` becomes default `desktop`; `default-members` swap; archive `impulse-gui` + `impulse-term`

## Verification gate

Every story must leave these green before marking `status: done`:

```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo build -p impulse-supervisor --features experimental-runtime
cargo clippy -p impulse-supervisor --features experimental-runtime -- -D warnings
```

## Traceability

Source-code annotations for this spec use:

```rust
// rnpm[impl SPEC-EGUI-RETIRE]    — implements
// rnpm[verify SPEC-EGUI-RETIRE]  — test verifies
```

Commit trailer: `Spec: SPEC-EGUI-RETIRE` plus `Story: STORY-NNN`.
