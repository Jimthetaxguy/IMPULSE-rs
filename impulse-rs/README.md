# impulse-rs

Rust implementation of Impulse — a sidecar that runs alongside AI coding agents (Claude Code, Codex, OpenCode) and remembers what they did across sessions.

## GUI Workbench

```bash
cargo run -p impulse-gui
```

A native desktop app (egui/eframe) with embedded terminals, agent coordination, and session memory.

**4 views:** Workbench | Terminals | Memory | Settings

**4 themes:** Launch (blue, default) | Nebula (purple) | Solar (amber) | Aurora (green)

**Agent panel:** Right-side supervisor chat with backend auto-detection (daemon > Claude Code > API > unavailable). Animated thinking indicator, message timestamps, activity feed.

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
cargo test                        # Workspace (1,344 tests)
cargo test -p impulse-gui         # GUI crate (246 tests)
cargo test -p impulse-term        # Terminal crate (110 tests)
cargo test -p impulse-ops         # Ops crate (4 tests)
# Total: ~1,700 tests
```

## Architecture

**4 crates:**

| Crate | Purpose |
|-------|---------|
| `impulse-rs` | CLI + daemon + TUI (64K LOC, 1,344 tests) |
| `impulse-ops` | Shared types (SupervisorAction, OpsSnapshot, IPC protocol) |
| `impulse-term` | Terminal widget (PTY, vt100, WriteQueue, context bridge, 110 tests) |
| `impulse-gui` | Native workbench (egui, 4 views, 4 themes, 246 tests) |

**Dual mode:**
- **Direct mode** — stateless CLI, per-action (for hooks)
- **Daemon mode** — long-running Unix socket IPC (for TUI/GUI)

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
