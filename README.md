# Impulse

**Your AI remembers. Silently.**

Impulse is a terminal-native sidecar for AI coding agents that preserves session continuity across tools and conversations. It tracks what you do, remembers what you decided, and injects relevant context into future sessions — automatically.

## Why

AI coding agents (Claude Code, OpenCode, Codex) forget everything between sessions. You re-explain your architecture, re-discover your preferences, and re-learn your codebase patterns every time. Impulse fixes that.

## What It Does

- **Session tracking** — Automatically records files touched, tools used, and decisions made
- **Persistent memory** — Project genome (decisions/preferences) and session history survive across sessions
- **Context injection** — Relevant past context is surfaced in new sessions via review-first injection
- **Multi-agent awareness** — Tracks which tools are active and what they're working on
- **Retrieval search** — FTS5 keyword search + semantic search across session history and genome
- **Context stewardship** — Monitors context window usage and proposes cleanup strategies
- **Daemon + TUI** — Long-running daemon with 9-tab interactive terminal UI
- **Platform hooks** — Auto-generated hooks for Claude Code and OpenCode

## Quick Start

```bash
cd impulse-rs
cargo build --release

# Initialize impulse in your project
cargo run -- init

# Start a session
cargo run -- session-start -n "my-project" -p claude-code

# Track work (normally done by hooks)
cargo run -- track-write --file src/main.rs --session-id <id>

# End session with verification gate
cargo run -- session-end --session-id <id> --summary "Implemented auth" --verify

# Launch TUI
cargo run -- run
```

## Architecture

```
Direct Mode (per-action, stateless)     Daemon Mode (long-running)
  Claude Code/OpenCode hooks              TUI, interactive chat
  read -> process -> write -> exit        Unix socket IPC
                    |
                    v
            .impulse/ directory
  HISTORY.jsonl  GENOME.md  config.json  LIVE_STATE.json
  retrieval.db   context/*  injections/*
```

- **Direct mode:** Each hook invocation reads state, processes, writes, and exits. Sub-millisecond.
- **Daemon mode:** Long-running process with Unix socket IPC for TUI and chat workflows.
- **File-based persistence:** All state lives in `.impulse/` — human-readable, git-trackable.

## Key Commands

| Command | Purpose |
|---------|---------|
| `init` | Initialize `.impulse/` in current directory |
| `session-start` / `session-end` | Lifecycle management |
| `status` / `summary` / `health` | Project overview |
| `search-history` / `search-genome` | Search past sessions and decisions |
| `steward` | Context window stewardship |
| `orchestrate` / `handoff` | Cross-tool context sharing |
| `run` | Launch 9-tab TUI |
| `daemon` | Start background daemon |

See `cargo run -- --help` for the full command list.

## Stack

- **Language:** Rust
- **TUI:** ratatui + crossterm
- **GUI:** egui (impulse-gui crate)
- **Storage:** SQLite (FTS5) + JSONL + Markdown
- **IPC:** Unix domain sockets
- **LLM:** Anthropic, OpenAI, Minimax (for daemon chat)

## Project Structure

```
impulse-rs/          # Rust implementation (canonical)
  src/
    main.rs          # CLI entry + command routing
    storage/         # Atomic file operations
    state/           # In-memory state + dirty flag sync
    daemon/          # Unix socket server
    agent/           # LLM provider abstraction
    retrieval/       # FTS5 + semantic search
    injection/       # Context injection engine
    stewardship/     # Context window management
    token_tracker/   # Token tracking algorithm
    credentials/     # Keychain + socket proxy
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

- **Start here:** [`docs/spec/RUST-CANONICAL-CONTRACT.md`](docs/spec/RUST-CANONICAL-CONTRACT.md)
- **Agent guidelines:** [`AGENTS.md`](AGENTS.md)
- **Meta-Harness synthesis:** [`docs/research/META-HARNESS-RUST-MULTI-AGENT.md`](docs/research/META-HARNESS-RUST-MULTI-AGENT.md)
- **Rust multi-agent guide:** [`docs/guides/RUST-MULTI-AGENT-PATTERNS.md`](docs/guides/RUST-MULTI-AGENT-PATTERNS.md)
- **Full index:** [`docs/INDEX.md`](docs/INDEX.md)
- **Full reference:** [`HANDBOOK.md`](HANDBOOK.md)

## Tests

```bash
cd impulse-rs
cargo test    # 486 passing
```

## License

Private — not yet open-sourced.
