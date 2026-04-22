# impulse-supervisor

Pure-Rust Dioxus desktop shell for the Impulse privileged supervisor terminal.

## Status: SCAFFOLD (Phase 8, Loop 151)

This crate contains the **architectural contract** for the future Dioxus shell
but is **not yet a workspace member**. It does not build as part of `cargo build`
from the workspace root.

### Why scaffold first

The shell architecture needed a concrete home before:
- The wire-format work in Loops 117–125 (defining `ImpulseCmd` types)
- The `PaneRole` split in Loops 115–116 (privileged-spawn contract)
- The eventual `impulse-gui` retirement (decision deferred to Loop 151)

By scaffolding the crate early, the first-principles contracts (ownership at
birth, supervisor-privileged, view-vs-state separation) become checkable in
code even while the runtime stays unwired.

## What's here

| File | Purpose |
|------|---------|
| `src/lib.rs` | Crate root — `PaneRoleRef`, `PaneIdentity`, scaffold stubs |
| `src/layout.rs` | `LayoutMode`, `WorkerGrid` — window split modes |
| `src/panes.rs` | `PaneRegistry` with one-supervisor invariant enforcement |
| `src/state.rs` | `ShellState` = `SessionState + TerminalState + OpsState` |

All four modules ship with unit tests that assert observable behavior.
Total: 29 tests covering role serialization, registry invariants, layout
constants, and shell-state round-trips.

## Not yet

- No `main.rs` binary — the Dioxus `launch()` entry point lives behind the
  `experimental-runtime` feature flag, which is off by default.
- No Dioxus rsx! components — deferred until the wire-format lands and the
  supervisor can actually receive compaction events.
- Not a workspace member — add to `impulse-rs/Cargo.toml` `members` when
  Phase 8 promotes this from scaffold to runtime.

## Contract enforced

See [`.opencode/ralph-loop-state/cleanup/DECISIONS.md`](../../.opencode/ralph-loop-state/cleanup/DECISIONS.md).
