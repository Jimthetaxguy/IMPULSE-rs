# impulse-rs

Rust implementation of Impulse — a sidecar that runs alongside AI coding agents (Claude Code, Codex, OpenCode) and remembers what they did across sessions.

## Desktop Shell (in progress)

The egui-based `impulse-gui` crate was retired 2026-04-17. Its replacement is the Dioxus Desktop host path in `impulse-desktop`: Dioxus owns the shell, xterm.js owns terminal rendering, and Rust owns daemon/runtime state. Tauri-shaped code is retained only as a temporary compatibility adapter.

For now, use the ratatui TUI:

```bash
cargo run -- run                 # ratatui terminal-native workbench
```

Planned desktop bring-up:

```bash
# Future: Dioxus Desktop launch scaffold with host adapter parity
```

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
cargo test                        # Workspace (~1,360 tests)
cargo test -p impulse-term        # Terminal crate (110 tests)
cargo test -p impulse-ops         # Ops crate (4 tests)
# impulse-rs main crate: 1,344 tests
```

## Architecture

**4 crates (post-Dioxus shell scaffold):**

| Crate | Purpose |
|-------|---------|
| `impulse-rs` | CLI + daemon + ratatui TUI (64K LOC, 1,344 tests) |
| `impulse-desktop` | Dioxus-owned desktop shell, host command DTOs, native island bridge contracts |
| `impulse-ops` | Shared types (SupervisorAction, OpsSnapshot, IPC protocol) |
| `impulse-term` | Terminal core (PTY, vt100, WriteQueue, context bridge, 110 tests) |

`impulse-gui` (egui native workbench) retired 2026-04-17. Replacement: Dioxus Desktop shell in `impulse-desktop` (static shell + typed host bridge scaffold).

**Dual mode:**
- **Direct mode** — stateless CLI, per-action (for hooks)
- **Daemon mode** — long-running Unix socket IPC (for TUI and future desktop shell)

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
