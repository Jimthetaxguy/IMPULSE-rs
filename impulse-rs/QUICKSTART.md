# Quick Start Guide

Get up and running with Impulse in 5 minutes.

## Prerequisites

- **Rust** — Install via [rustup.rs](https://rustup.rs)
- **AI Agent CLI** — At least one of:
  - Claude Code: `npm install -g @anthropic-ai/claude-code`
  - Codex: `npm install -g @openai/codex`

## Step 1: Install Impulse

```bash
git clone https://github.com/Jimthetaxguy/IMPULSE-rs.git
cd impulse-rs
cargo install --path .
```

## Step 2: Initialize a Project

```bash
cd your-project
impulse-rs init
```

Creates `.impulse/` with `config.json`, `GENOME.md`, and `HISTORY.jsonl`.

## Step 3: Launch the TUI Workbench

```bash
cargo run -- run
```

The terminal-native workbench is the current operator path. The Dioxus Desktop host lives in `impulse-desktop`; Tauri-shaped code is legacy compatibility only while host parity moves over. The old egui `impulse-gui` workbench is legacy/frozen and should be used only for compile-maintenance or historical comparison.

### Use the Supervisor

The TUI supervisor coordinates agent sessions and context. Type questions or use slash commands:

```
/help      — Show available commands
/status    — Connection and backend info
/search    — Search terminal output
/clear     — Clear chat
```

## Step 4: Set Up Agent Hooks

Auto-track sessions with Claude Code. Codex is the other active platform; legacy OpenCode compatibility is preserved where already implemented, but new setup should prefer Claude Code or Codex.

```bash
impulse-rs validate-hooks --platform claude-code
```

## CLI Mode

For headless use without the GUI:

```bash
impulse-rs daemon                    # Start background daemon
impulse-rs session-start -n myproject -p claude-code
impulse-rs history                   # View session history
impulse-rs search-history --query "auth"
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Required for supervisor chat |
| `IMPULSE_MODEL` | Override chat model |

## Troubleshooting

**"Command not found" for agent** — Install the CLI first (`npm install -g @anthropic-ai/claude-code`)

**"No agent backend configured"** — Set `ANTHROPIC_API_KEY` or install Claude Code

**Desktop shell status** — Use the ratatui TUI for current work. The Dioxus Desktop host is the active native-shell target, Tauri-shaped code is legacy compatibility only, and `impulse-gui` is legacy/frozen.
