# Task 3 Report: Governed runtime launch gate

Status: complete

## RED

Command:

```text
cargo test -p impulse-desktop runtime --lib
```

Observed exit `101` with 19 expected `E0560`/`E0609` errors. The failures showed
that `AgentSpawnRequest` had no `task` or `role_assignment` fields and
`AgentRuntimeSnapshot` had no `role_assignment` or `role_compatibility` fields.
No production implementation was changed before this RED run.

## Implementation

- Added backward-compatible optional task and typed product-role assignment to
  `AgentSpawnRequest`; legacy `AgentRole` remains the separate pane-topology role.
- Canonicalized platform identity, then evaluated the assignment through the
  trusted runtime registry before agent-ID reservation or PTY spawn.
- Mapped evaluator errors and mandatory incompatibility to
  `InvalidTerminalRequest`; optional gaps remain launchable and are recorded as
  degraded compatibility.
- Preserved task, assignment, and compatibility in runtime records, snapshots,
  daemon `AgentRuntime` conversion, and workbench overlays.
- Prevented legacy/defaulted overlays from erasing newer typed role facts.
- Added optional `IMPULSE_TASK` and `IMPULSE_ROLE_ID` child environment metadata.
- Updated existing snapshot/request constructors with `None` compatibility
  values, including the authorized two-field launcher constructor update.

## Verification

- `cargo test -p impulse-desktop runtime --lib` — 30 passed.
- `cargo test -p impulse-desktop daemon_ops --lib` — 16 passed.
- `cargo test -p impulse-rs ops_workbench --lib` — 16 passed.
- `cargo test -p impulse-desktop --test runtime` — 20 passed, 1 pre-existing
  ignored real-Ion test.
- `cargo check --workspace` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo check --frozen -p impulse-desktop --features desktop-app --bin impulse-desktop`
  — passed; Cargo emitted its existing future-incompatibility note for dependency
  `block v0.1.6`.
- `git diff --check` — passed.

The pre-existing `.gitignore` change and untracked implementation plan were not
edited or staged for Task 3.
