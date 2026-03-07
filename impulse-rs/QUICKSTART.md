# Quick Start Guide

Get up and running with Impulse in 5 minutes.

## Prerequisites

- **Rust** — Install via [rustup.rs](https://rustup.rs)
- **AI Agent CLI** — At least one of:
  - Claude Code: `npm install -g @anthropic-ai/claude-code`
  - OpenCode: `pip install opencode`
  - Codex: `npm install -g @openai/codex`

## Step 1: Install Impulse

```bash
# Clone and install
git clone https://github.com/jamespustorino/impulse-rs.git
cd impulse-rs
cargo install --path .

# Verify installation
impulse-rs --version
```

## Step 2: Initialize a Project

```bash
# Go to your project
cd your-project

# Initialize Impulse
impulse-rs init
```

This creates `.impulse/` with:
- `config.json` — Runtime configuration
- `GENOME.md` — Project memory (decisions, preferences)
- `HISTORY.jsonl` — Session history

## Step 3: Launch TUI

```bash
impulse-rs run
```

You'll see the terminal multiplexer interface.

## Step 4: Start an AI Agent

Press keys to launch agents:

| Key | Agent | First-time Setup |
|-----|-------|------------------|
| `C` | Claude Code | Run `claude` in terminal first to authenticate |
| `O` | OpenCode | Works out of the box |
| `X` | Codex | Run `codex` in terminal first to authenticate |
| `c` | Shell | Just works |

## Step 5: Use the Agent

Now you're coding with an AI agent inside Impulse!

Try asking the agent:
- "What files are in this project?"
- "Add a login feature"
- "Refactor the auth module"

## What Happens Automatically

Impulse tracks everything:

1. **Session start** — Records when agent starts
2. **File writes** — Tracks files the agent modifies
3. **Tool usage** — Records which tools are used
4. **Session end** — Logs summary, verifies changes

View history:
```bash
impulse-rs history
impulse-rs search-history --query "login"
```

## Next Steps

### Enable Context Injection

Let the agent learn from past sessions:

```bash
# Review mode (recommended) — review before injecting
impulse-rs config set context_injection_mode review

# Apply mode — auto-inject
impulse-rs config set context_injection_mode apply

# Off — no injection
impulse-rs config set context_injection_mode off
```

### Set Up Agent Hooks

Auto-track sessions with Claude Code:

```bash
impulse-rs hooks --platform claude-code
```

This creates `.claude/hooks/hooks.json` that tracks:
- Session start/end
- File edits
- Tool usage

Validate the real Claude hook loop before trusting it:

```bash
impulse-rs validate-hooks --platform claude-code
```

This creates `.impulse/validation/claude-code/` with:
- Startup sentinel hooks for `SessionStart`
- Transcript capture for `SessionEnd`
- A local settings snippet
- An evidence template for pass/fail results

### Try Semantic Search

```bash
# Index your history
impulse-rs index-memory --scope history

# Search with AI understanding
impulse-rs search-history --query "how did we implement auth" --mode semantic
```

## Common Tasks

### Run Multiple Agents

Press `c` for shell, `C` for Claude, `O` for OpenCode — have 3+ agents working together.

### Respawn a Terminal

Press `R` to restart the current pane (useful if agent crashes).

### Scroll Back

Press `[` to enter scroll mode, `q` to exit.

### Get Help

Press `?` in TUI for key bindings.

## Troubleshooting

### "Command not found" for agent

Install the CLI first:
```bash
# Claude Code
npm install -g @anthropic-ai/claude-code

# OpenCode  
pip install opencode
```

### "API key not found"

Set your API key:
```bash
export ANTHROPIC_API_KEY=sk-ant-...
```

### "The GUI is offline"

The GUI is a thin workbench client. Memory/history/ops require the daemon:

```bash
cargo run -- daemon
```

### TUI looks broken

Ensure your terminal supports 256 colors:
```bash
export TERM=xterm-256color
```

## What's Next?

- Read [PLATFORMS.md](PLATFORMS.md) for platform-specific setup
- Explore [HANDBOOK.md](../HANDBOOK.md) for all commands
- Check out the 23 dynamic tools: `impulse-rs tooling-list`
