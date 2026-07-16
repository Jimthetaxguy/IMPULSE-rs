# Quick Start Guide

Get from a source checkout to a tracked project and choose the Dioxus cockpit or the first-class
terminal-only workbench.

## Prerequisites

- **Rust** — Install via [rustup.rs](https://rustup.rs)
- **AI Agent CLI** — At least one of:
  - Claude Code: `npm install -g @anthropic-ai/claude-code`
  - Codex: `npm install -g @openai/codex`

## Step 1: Install Impulse

```bash
git clone https://github.com/Jimthetaxguy/IMPULSE-rs.git
cd IMPULSE-rs/impulse-rs
cargo install --path .
```

## Step 2: Initialize a Project

```bash
cd <path-to-your-project>
impulse-rs init
```

Creates `.impulse/` with `config.json`, `GENOME.md`, and `HISTORY.jsonl`.

## Step 3: Choose an Operator Surface

### Dioxus Cockpit (Active Desktop Product Path)

From the Rust workspace root in the source checkout:

```bash
cd <path-to-IMPULSE-rs>/impulse-rs
cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop
```

The cockpit keeps one selected launch target, the connected daemon project's oversight service,
desktop-wide workers, the focused terminal, and launch/review controls visible together. The rail
and launch dock share the same target authority. Register the project folder, choose a runtime
separately from the Builder role, and launch only after the compatibility preview passes. The
oversight service exposes project-scoped review state; it does not claim that a selectable
Supervisor runtime has already been launched.

On macOS, build and lifecycle-smoke the local signed application with:

```bash
bash scripts/build-macos-app.sh --smoke
open Impulse.app
```

The package includes the `impulse-rs` daemon companion. It attaches to an already-running daemon
when available and owns cleanup only for the companion it starts.

### ratatui Workbench (First-Class Terminal-Only Alternative)

```bash
impulse-rs run
```

The ratatui workbench remains a supported operator surface for terminal-native session, history,
context, and stewardship workflows. Tauri-shaped code is compatibility-only. The old egui
`impulse-gui` workbench is legacy/frozen and should be used only for explicit compile maintenance
or historical comparison.

### Monitor Work in the TUI

The ratatui workbench monitors sessions, history, context, and stewardship. Its input commands are:

```
/track <path>    — Track a file in the current session
/tool <name>     — Track a tool in the current session
/session <name>  — Create a session
/search <query>  — Search across sessions
/tag <name>      — Tag the selected session
```

To use the Impulse-native coding-agent REPL instead, launch `ion`. Its `/help`, `/verify`,
`/tools`, `/clear`, and `/quit` commands belong to the Ion surface, not the ratatui workbench.

## Step 4: Set Up Agent Hooks

Auto-track sessions with Claude Code. Codex is the other active platform; legacy OpenCode compatibility is preserved where already implemented, but new setup should prefer Claude Code or Codex.

```bash
impulse-rs validate-hooks --platform claude-code
```

## CLI Mode

For headless use without the GUI:

```bash
impulse-rs daemon                    # Start the daemon in the foreground
impulse-rs session-start -n myproject -p claude-code
impulse-rs history                   # View session history
impulse-rs search-history --query "auth"
```

Keep the foreground daemon in its own terminal when using `--daemon` commands:

```bash
impulse-rs --daemon status
```

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Required for supervisor chat |
| `IMPULSE_MODEL` | Override chat model |

## Troubleshooting

**"Command not found" for agent** — Install the CLI first (`npm install -g @anthropic-ai/claude-code`)

**"No agent backend configured"** — Set `ANTHROPIC_API_KEY` or install Claude Code

**Desktop shell status** — Dioxus is the active desktop cockpit. ratatui remains the first-class
terminal-only alternative, Tauri-shaped code is compatibility-only, and `impulse-gui` is
legacy/frozen.
