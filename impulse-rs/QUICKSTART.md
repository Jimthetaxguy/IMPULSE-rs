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

## Step 3: Launch the GUI Workbench

```bash
cargo run -p impulse-gui
```

The workbench opens with 4 views:
- **Workbench** (Ctrl+1) — Dashboard with agent fleet and session overview
- **Terminals** (Ctrl+2) — Spawn and manage AI agent terminals
- **Memory** (Ctrl+3) — Session history, genome decisions, search
- **Settings** (Ctrl+4) — Configuration and theme selection

### Spawn an Agent Terminal

1. Go to Terminals view (Ctrl+2)
2. Click an available agent (Claude Code, Codex, Shell)
3. Select a project directory
4. The terminal spawns with context lifecycle tracking

### Switch Themes

Go to Settings (Ctrl+4) and pick a theme:
- **Launch** (default) — Deep space blue
- **Nebula** — Purple violet
- **Solar** — Warm amber
- **Aurora** — Emerald green

### Use the Supervisor

The right-side panel (Ctrl+E) is the Impulse supervisor — an AI coordinator that monitors your agent terminals. Type questions or use slash commands:

```
/help      — Show available commands
/status    — Connection and backend info
/search    — Search terminal output
/clear     — Clear chat
```

## Step 4: Set Up Agent Hooks

Auto-track sessions with Claude Code:

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

**GUI shows "Waiting for daemon"** — The daemon is optional. Terminal multiplexing works without it. Memory/history features require `impulse-rs daemon`.
