# impulse-rs

Rust implementation of Impulse — a sidecar that runs alongside AI coding agents (Claude Code, Codex, OpenCode) and remembers what they did across sessions.

## Desktop Workbench

In this checkout, the live desktop workbench is the egui/eframe-based `impulse-gui` crate. It shares daemon contracts with the CLI/TUI and is part of the verified root workspace.

Desktop and terminal entry points:

```bash
cargo run -p impulse-gui         # desktop workbench
cargo run -- run                 # ratatui terminal-native workbench
```

Historical `src-tauri/` references still exist in some docs, but the checked-in `src-tauri/` sources are effectively absent in this checkout and should be treated as planning/history, not the live runtime target.

**Terminal multiplexer:** Spawn Claude Code, Codex, OpenCode, or shell terminals. Context lifecycle (extraction, injection, compaction detection). PTY writes serialized via WriteQueue to prevent text corruption.

## CLI

```bash
cargo run -- --help                    # Show all commands
cargo run -- init                      # Initialize .impulse/
cargo run -- daemon                    # Start background daemon
cargo run -- session-start -n myproject -p claude-code
cargo run -- validate-hooks --platform claude-code

# Agent coordination
cargo run -- agent-configure --provider anthropic --api-key $KEY
cargo run -- agent-query "Review cross-pane activity"
```

## Building

```bash
cargo build              # Debug
cargo build --release    # Release
cargo install --path .   # Install globally
```

## Testing

```bash
cargo test --workspace
cargo test -p impulse-rs
cargo test -p impulse-term
cargo test -p impulse-ops
cargo test -p impulse-gui
```

## Architecture

**Verified workspace members in this checkout:**

| Crate | Purpose |
|-------|---------|
| `impulse-rs` | CLI + daemon + ratatui TUI |
| `impulse-ops` | Shared types (SupervisorAction, OpsSnapshot, IPC protocol) |
| `impulse-term` | Terminal core (PTY, vt100, WriteQueue, context bridge) |
| `impulse-gui` | Live egui/eframe workbench backed by daemon snapshots and terminal telemetry |

Companion crates present in the repo but not part of the root workspace:

- `impulse-gui-legacy-adapter` — legacy adapter/reference surface
- `impulse-shell-ui` — extracted shell UI experiments/reference code

**Dual mode:**
- **Direct mode** — stateless CLI, per-action (for hooks)
- **Daemon mode** — long-running Unix socket IPC (for the TUI and `impulse-gui`)

**Data (`.impulse/`):**
- `HISTORY.jsonl` — append-only session log
- `GENOME.md` — permanent decisions
- `LIVE_STATE.json` — active session state
- `config.json` — runtime config + theme

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `ANTHROPIC_API_KEY` | LLM chat | Required for chat |
| `IMPULSE_MODEL` | Model override | `claude-sonnet-4-20250514` |
| `IMPULSE_SESSION_ID` | Session ID override | Auto-generated |
| `IMPULSE_HOME` | Custom `.impulse/` dir | `$CWD/.impulse/` |

## Code Conventions

- **Error handling:** `thiserror` enums + `anyhow` application errors, `.context()` on all I/O
- **File I/O:** Atomic writes (temp + rename), unique temp names
- **State:** Dirty flag pattern, sync on Drop
- **PTY writes:** Serialized via `WriteQueue` (user input > injection, 500ms quiet period)
- **Themes:** `ColorPalette` struct, `ThemeName` enum (serde, persisted to config.json)
- **No panics:** Always return `Result<T>`, no `unwrap()` on production paths
