# impulse-rs

Rust implementation of Impulse — the canonical codebase.

## Building

```bash
cargo build              # Debug
cargo build --release    # Release (optimized)
cargo install --path .   # Install globally
```

## Running

```bash
cargo run -- --help      # Show all commands
cargo run -- init        # Initialize .impulse/
cargo run -- run         # Launch TUI
cargo run -- daemon      # Start background daemon

# ImpulseAgent (LLM coordination)
cargo run -- agent-configure --provider anthropic --api-key $KEY
cargo run -- agent-status
cargo run -- agent-query "Review cross-pane activity"
```

## Testing

```bash
cargo test               # Run all tests (486 passing)
cargo test -- --nocapture  # With stdout
cargo test storage       # Run storage tests only
cargo test daemon        # Run daemon tests only
```

## Module Map

| Module | Files | Purpose |
|--------|-------|---------|
| `main.rs` | 1 | CLI entry, clap command routing (~30 commands) |
| `storage/` | 1 | Atomic file I/O (temp+rename), JSONL append, config |
| `state/` | 1 | In-memory state with dirty flag, Drop-based sync |
| `daemon/` | 2 | Unix socket IPC server, chat, command dispatch |
| `client/` | 1 | Socket client for daemon communication |
| `agent/` | 2 | LLM provider trait + Anthropic/OpenAI/Minimax impl |
| `memory/` | 1 | GENOME persistence, decision tracking |
| `session/` | 1 | Session lifecycle (start, end, track) |
| `retrieval/` | 7 | SQLite FTS5, embeddings, indexer, query, pageindex |
| `injection/` | 4 | Context injection engine, staging, review-first |
| `stewardship/` | 7 | Context window management, token monitoring, cleanup |
| `token_tracker/` | 5 | Token tracking algorithm, compaction metrics |
| `credentials/` | 4 | Keychain storage, socket proxy, CLI proxy |
| `docs/` | 4 | Documentation fetcher, provider models, cache |
| `tools/` | 7 | Tool init/list/update + benchmark, health, python, system |
| `orchestration/` | 1 | Cross-tool handoff, context routing |
| `verify/` | 1 | Verification gates (lint, test, build) |
| `tooling/` | 12 | DynamicTool registry, 19 tools (builtin + document) |
| `build_hygiene/` | 6 | Native cargo-sweep/wipe replacement, sccache |
| `monty/` | 4 | Computed routing via embedded Python scripts |
| `office/` | 3 | DOCX/XLSX parsing (feature-flagged: `office-support`) |
| `impulse_agent/` | 3 | Dual-mode LLM agent: API + CLI harness coordination (26 tests) |
| `context_lifecycle/` | 7 | Context window monitor, injector, extractor, detector (36 tests) |
| `intent/` | 4 | Intent detection from agent PTY output (11 tests) |
| `agent_discovery/` | 1 | Capabilities manifest export for external agents |
| `notification/` | 1 | Notification bus for cross-module event delivery |
| `ui/` | 4 | TUI rendering, pane manager, terminal pane |
| `branding.rs` | 1 | CLI branding and output formatting |
| `error.rs` | 1 | Error types (thiserror) |
| `integration_tests.rs` | 1 | Integration test suite |

## Code Conventions

- **Error handling:** `thiserror` for enums, `anyhow` for application errors
- **File I/O:** Always atomic (temp file + rename)
- **State:** Dirty flag pattern, sync on Drop
- **Async:** `tokio` runtime for daemon mode
- **Concurrency:** `tokio::sync::RwLock` (async), `std::sync::RwLock` (sync)
- **No panics:** Always return `Result<T>`

## Native Desktop App (impulse-gui)

```bash
cargo run -p impulse-gui
```

A standalone, pure-Rust native GUI using `egui` and embedded terminals. Remembers agent environments and protects against nested session loops.

## Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `ANTHROPIC_API_KEY` | LLM chat in daemon | Required for chat |
| `IMPULSE_MODEL` | Chat model selection | `claude-sonnet-4-20250514` |
| `IMPULSE_SESSION_ID` | Override session ID | Auto-generated |
| `IMPULSE_CAPABILITIES_PATH` | Path to capabilities manifest | `.impulse/impulse-capabilities.json` |
