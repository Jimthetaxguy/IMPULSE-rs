# IMPULSE Rust workspace

**Feed the impulse to build.**

This directory contains the canonical Rust implementation of Impulse: a terminal-native local
control plane and harness manager for AI software-engineering agents. It launches and supervises
external coding CLIs, provides the Impulse-native Ion runtime, and supplies shared platform
services such as memory, tools, telemetry, artifacts, policy, credentials, and verification.

Memory is one platform service, not the product boundary. Claude Code, Codex, and similar tools
retain their own internal coding loops; Impulse governs the operating conditions around those
loops. See the repository-level [product vision](../VISION.md) for the full live-versus-target
contract.

## Operator surfaces

- **Dioxus cockpit:** `impulse-desktop` is the live, feature-gated desktop path. Dioxus and
  xterm.js render the system; typed Rust host/runtime contracts and the daemon remain authoritative.
- **ratatui workbench:** the root crate provides the terminal-native TUI.
- **CLI and hooks:** short-lived commands initialize projects, track sessions, validate hooks, and
  query or update daemon-owned state.
- **Ion:** the workspace includes the native runtime and its transport-agnostic harness contracts.

`WorkspaceTarget` selects the cwd/project root for an agent process. It does not by itself create
filesystem isolation; structural enforcement depends on the selected runtime or sandbox.

## Run

From this directory:

```bash
cargo run -- --help
cargo run -- init
cargo run -- daemon
cargo run -- run
cargo run -- session-start -n myproject -p claude-code
cargo run -- validate-hooks --platform claude-code
```

Launch the feature-gated Dioxus cockpit with:

```bash
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop
```

## Workspace packages

| Package | Responsibility |
| --- | --- |
| `impulse-rs` | CLI, daemon, ratatui workbench, shared services, and native Ion execution path |
| `impulse-desktop` | Dioxus cockpit, xterm.js integration, typed host bridge, and desktop runtime adapters |
| `impulse-ion` | Transport-agnostic Ion harness request/response and adapter contracts |
| `impulse-ops` | Shared control-plane protocol, workbench, policy, registry, artifact, and telemetry models |
| `impulse-term` | Framework-neutral PTY lifecycle, terminal parsing, write queue, and context bridge |

`impulse-gui` is the legacy/frozen egui workbench. It is excluded from the active Cargo workspace
and retained only for compile maintenance while Dioxus owns the current desktop product path.

## Authority boundaries

- The daemon owns durable project/workbench truth.
- PTY runtimes own process and terminal mechanics, then publish structured facts.
- Dioxus owns presentation and operator input, not policy or persistence.
- Roles describe behavioral obligations and permissions; runtimes are the engines that execute
  them. Generalized runtime-independent role contracts and capability negotiation remain target
  architecture rather than completed product claims.
- Worker completion claims remain distinct from observed verification evidence and supervisor or
  user approval.

## Build and verify

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Exact verified test totals change as branches are integrated. Use the repository-level
[`AGENTS.md`](../AGENTS.md) and
[`RUST-CANONICAL-CONTRACT.md`](../docs/spec/RUST-CANONICAL-CONTRACT.md) for the current canonical
evidence rather than copying counts into this child README.

## Durable project data

Project-local `.impulse/` state includes session history, durable decisions, live session state,
and configuration. SQLite indexes and daemon/runtime state complement those human-readable
artifacts; not every operational record is intended to be committed.
