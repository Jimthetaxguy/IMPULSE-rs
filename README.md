# IMPULSE — Feed the impulse to build.

**One governed cockpit for many coding agents.**

Impulse names both the creative urge to make something and the force that sets work in motion. The product exists to protect that first spark, then help it compound across agents, tools, projects, and sessions instead of dissolving into terminal sprawl and repeated context setup.

Everyone has felt it: the itch to open Claude Code or Codex and disappear into a build. Impulse is built for that moment.

Impulse is a terminal-native **local control plane and harness manager** for AI software-engineering agents. It launches and manages heterogeneous coding runtimes, places them in explicit workspaces, supervises their processes, and augments them with shared memory, tools, telemetry, artifacts, policy, and verification.

Claude Code, Codex, and similar CLIs keep their own internal coding loops. Ion is the Impulse-native coding runtime. Impulse governs the operating conditions around those loops; it does not claim to replace or fully control proprietary runtime internals.

> **Live foundation:** the Rust workspace provides PTY lifecycle, daemon workbench contracts and
> telemetry, registry-backed desktop platform identity, supervisor-specific permissions,
> capability-checked tools, memory/retrieval, artifacts, credentials, verification, and Ion's
> native REPL/tool loop.
>
> **Target:** runtime-independent role contracts, adapter capability negotiation, typed agent
> messaging, and stronger structural enforcement across supported runtimes. See
> [`VISION.md`](VISION.md) for the north star and explicit live-versus-target boundary.

## Why

Using several coding agents today usually means several unrelated terminals, permission models, context stores, and completion claims. Impulse brings those runtimes into one observable environment while preserving their terminal-native workflows. Persistent memory preserves continuity; the wider control plane handles managed launch, project scoping, coordination, intervention, and evidence-backed completion. Structural filesystem isolation depends on the selected runtime or sandbox rather than on the cockpit alone.

## What It Does

- **Managed agent terminals** — Spawns, monitors, writes to, resizes, focuses, and closes PTY-backed agent processes inside explicit workspace roots
- **Daemon workbench truth** — Serves the authoritative agent, context, artifact, and intervention snapshot over versioned IPC
- **Role and policy foundations** — Enforces a concrete supervisor permission policy today; generalized runtime-independent role contracts are the next control-plane boundary
- **Typed platform tools** — Exposes capability-checked Rust tools through native registries, MCP, and runtime-specific bridges
- **Session tracking** — Records files touched, tools used, and decisions made
- **Persistent memory** — Project genome (decisions/preferences) and session history survive across sessions
- **Context injection** — Relevant past context is surfaced in new sessions via review-first injection
- **Multi-agent observability** — Tracks active agents, tasks, terminal telemetry, delegations, and intervention recommendations
- **Retrieval search** — FTS5 keyword search plus feature-gated, fallback-safe semantic search across session history and genome
- **Context stewardship** — Monitors context window usage and proposes cleanup strategies
- **Artifacts and verification** — Separates worker claims from observed build/test evidence and reviewable outputs
- **Credential services** — Selects configured credential providers without treating secret values as agent memory
- **External + native runtimes** — Wraps terminal CLIs and provides Ion's direct model/tool loop in the same product

## Quick Start

```bash
cd impulse-rs
cargo build --workspace

# Terminal 1: start the control-plane daemon
cargo run -- daemon

# Terminal 2: launch the feature-gated Dioxus cockpit
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop

# Or stay entirely terminal-native
cargo run -- run

# Launch the Impulse-native coding runtime
cargo run --bin ion
```

The live foundation can launch registered PTY runtimes and project their daemon-backed state into the
cockpit. The complete role-assigned supervisor + builder workflow is the next vertical slice, not
an undocumented command-line flag. Session, memory, hook, and verification commands remain
available through `cargo run -- --help`.

## Architecture

```text
 Dioxus cockpit / ratatui / CLI
              |
              v
 Impulse daemon + control-plane contracts
              |
    +---------+----------+----------------+
    |                    |                |
 PTY/processes      platform registry  shared services
    |                                  memory, tools,
    |                                  telemetry, policy,
    |                                  artifacts, verification
    +----------+-------------------------+
               |
      +--------+---------+
      |                  |
 external CLI runtimes   Ion native runtime
 Claude, Codex, ...      direct model + tool loop
```

- **Direct mode:** A short-lived hook path reads state, processes one action, persists its result, and exits.
- **Daemon mode:** The long-running coordination point for workbench state, agent requests, telemetry, artifacts, and supervisor actions.
- **Desktop mode:** Dioxus + xterm.js is the cockpit. It renders state and sends commands; backend contracts remain authoritative.
- **Persistence:** Durable records include human-readable JSONL/Markdown/config artifacts, while SQLite indexes and ephemeral daemon/runtime state are intentionally not all human-readable or git-tracked.

## Key Commands

| Command | Purpose |
|---------|---------|
| `init` | Initialize `.impulse/` in current directory |
| `session-start` / `session-end` | Lifecycle management |
| `status` / `summary` / `health` | Project overview |
| `search-history` / `search-genome` | Search past sessions and decisions |
| `steward` | Context window stewardship |
| `orchestrate` / `handoff` | Cross-tool context sharing |
| `run` | Launch the 10-tab ratatui workbench |
| `daemon` | Run the foreground control-plane daemon listener |

See `cargo run -- --help` for the full command list.

## Stack

- **Language:** Rust
- **TUI:** ratatui + crossterm (terminal-native operator surface)
- **Desktop:** Dioxus Desktop + xterm.js via `impulse-desktop`; egui `impulse-gui` is legacy/frozen for compile-maintenance only; Tauri-shaped code is legacy compatibility only
- **Storage:** SQLite (FTS5) + JSONL + Markdown
- **IPC:** Unix domain sockets
- **LLM:** Anthropic, OpenAI, and Minimax provider paths for daemon/Ion agent loops

## Project Structure

```
impulse-rs/          # Rust implementation (canonical)
  impulse-ops/       # Shared control-plane protocol, workbench, policy, artifact models
  impulse-term/      # PTY lifecycle, parser, write queue, terminal context
  impulse-desktop/   # Dioxus cockpit, host bridge, workspace/runtime/MCP adapters
  impulse-ion/       # Ion harness contract + adapter crate
  src/
    main.rs          # CLI entry + command routing
    storage/         # Atomic file operations
    state/           # In-memory state + dirty flag sync
    daemon/          # Unix socket server
    agent/           # External harness coordination
    ion_repl/        # Impulse-native coding-agent runtime
    llm_backends/    # Direct model provider/tool-loop boundary
    retrieval/       # FTS5 + semantic search
    injection/       # Context injection engine
    stewardship/     # Context window management
    token_tracker/   # Token tracking algorithm
    credentials/     # Keychain + socket proxy
    tooling/         # Capability-checked dynamic tools
    tools/           # Tool management + utilities
    docs/            # Documentation fetcher
    ui/              # TUI rendering
docs/                # Documentation
  spec/              # Canonical contract
  research/          # Analysis and research
  guides/            # Developer guides
memory-pipeline/     # Python research tooling
```

## Documentation

- **Product north star:** [`VISION.md`](VISION.md)
- **Start here:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **User stories:** [`docs/spec/USER-STORY-MAP.md`](docs/spec/USER-STORY-MAP.md)
- **Test traceability:** [`docs/spec/TEST-TRACEABILITY.md`](docs/spec/TEST-TRACEABILITY.md)
- **Agent guidelines:** [`AGENTS.md`](AGENTS.md)
- **Meta-Harness synthesis:** [`docs/research/META-HARNESS-RUST-MULTI-AGENT.md`](docs/research/META-HARNESS-RUST-MULTI-AGENT.md)
- **Rust multi-agent guide:** [`docs/guides/RUST-MULTI-AGENT-PATTERNS.md`](docs/guides/RUST-MULTI-AGENT-PATTERNS.md)
- **Full index:** [`docs/INDEX.md`](docs/INDEX.md)
- **Detailed historical reference:** [`HANDBOOK.md`](HANDBOOK.md)

## Real systems

Production paths use real local processes, operating-system services, and model providers; there
is no production `MOCK_MODE` switch. Tests use fakes only inside test targets.

| System | Configuration / secret name | Current verification boundary |
| --- | --- | --- |
| Claude Code, Codex, shell, and Ion processes | Executable registry + `PATH`; Ion can resolve its built sibling | PTY/runtime tests plus an ignored real-Ion sibling launch proof |
| Anthropic | `ANTHROPIC_API_KEY` or `CLAUDE_API_KEY` | Real provider path; missing credentials fail explicitly |
| OpenAI | `OPENAI_API_KEY` | Real provider path; missing credentials fail explicitly |
| MiniMax | `MINIMAX_API_KEY` | Real provider/harness path; missing credentials fail explicitly |
| macOS Keychain | Native signed-in-user Keychain; live test additionally requires `IMPULSE_RUN_LIVE_KEYCHAIN_TEST=1` and explicit wire-gate approval | Deterministic native-API tests; ignored live round trip is never part of the default suite |
| Credential socket / CLI proxy | Provider configuration; the CLI proxy may invoke Infisical, Doppler, or Vault while retaining the provider key names above | Provider contract tests |

There is not yet one budget-capped, opt-in command that exercises every remote LLM provider in a
single integration suite. Treat that as a real-systems verification gap, not as permission to add
mock provider behavior to shipping code.

## Tests

```bash
cd impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

This repository is public, but release licensing is not yet coherent: Cargo manifests declare
MIT while [`impulse-rs/LICENSE`](impulse-rs/LICENSE) contains Apache-2.0. Treat licensing as an
explicit pre-release decision; this draft PR does not silently choose or rewrite the license.
