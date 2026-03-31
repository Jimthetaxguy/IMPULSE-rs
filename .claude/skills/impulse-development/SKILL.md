---
name: impulse-development
description: "Use when adding features to the Impulse codebase — CLI commands, daemon IPC messages, GUI views, dynamic tools, context lifecycle, or guardrails. Routes to detailed step-by-step guides."
version: 1.0.0
domain: engineering
category: languages
maturity: silver
triggers:
  - impulse
  - add command
  - add view
  - add tool
  - daemon message
  - signal bus
  - context lifecycle
  - guardrail
competency_tags:
  - impulse-codebase
  - cli-commands
  - daemon-ipc
  - gui-views
  - dynamic-tools
related_skills:
  - rust-programming
  - rust-daemon-ipc
  - pty-terminal-engineering
  - egui-webgpu-visualization
  - plugin-hooks-guardrails
  - context-engineering
  - ai-sidecar-patterns
  - rust-async-concurrency
platforms: [claude]
source: "Impulse codebase conventions"
priority: HIGH
path: ".claude/skills/impulse-development/"
codebase_source:
  - impulse-rs/src/main.rs
  - impulse-rs/src/daemon/mod.rs
  - impulse-rs/impulse-gui/src/app.rs
  - impulse-rs/src/tooling/mod.rs
  - impulse-rs/src/guardrail/engine.rs
  - impulse-rs/impulse-term/src/backend.rs
---

# Impulse Development Guide

Project-level skill for adding features to the Impulse codebase. Routes to the
right reference file and global skills based on what you're building.

---

## Task Routing Table

| What You're Doing | Reference File | Key Source Files | Global Skill |
|---|---|---|---|
| Add a CLI command | `references/adding-cli-commands.md` | `src/main.rs`, handler module | `rust-programming` |
| Add a daemon IPC message | `references/adding-daemon-ipc.md` | `src/daemon/mod.rs` | `rust-daemon-ipc` |
| Add a GUI view | `references/adding-gui-views.md` | `impulse-gui/src/app.rs`, `views/` | `egui-webgpu-visualization` |
| Add a dynamic tool | `references/adding-dynamic-tools.md` | `src/tooling/mod.rs`, `traits.rs` | `rust-trait-design` |
| Add a guardrail rule | CLAUDE.md guardrail section | `src/guardrail/` | `plugin-hooks-guardrails` |
| Add a context lifecycle stage | Context lifecycle docs | `src/context_lifecycle/` | `context-engineering` |
| Add a terminal feature | impulse-term docs | `impulse-term/src/` | `pty-terminal-engineering` |
| Add a signal bus signal | Signal bus section in MEMORY.md | `impulse-gui/src/widgets/signal_bus.rs` | `egui-webgpu-visualization` |

### Module-to-Skill Map (Context-Adaptive Loading)

When working on files in these paths, auto-surface the corresponding skills:

| Active Module / Path Pattern | Auto-surface Skills |
|---|---|
| `src/daemon/**` | `rust-daemon-ipc` + `rust-async-concurrency` |
| `impulse-gui/src/views/**` | `egui-webgpu-visualization` |
| `impulse-gui/src/widgets/**` | `egui-webgpu-visualization` |
| `impulse-term/**` | `pty-terminal-engineering` + `egui-webgpu-visualization` |
| `src/context_lifecycle/**` | `context-engineering` + `ai-sidecar-patterns` |
| `src/guardrail/**` | `plugin-hooks-guardrails` |
| `src/tooling/**` | `rust-trait-design` |
| `src/main.rs` (CLI dispatch) | `rust-programming` (complex CLI section) |
| `src/state/**` | `rust-programming` (atomic I/O + dirty flag sections) |
| `src/injection/**` | `ai-sidecar-patterns` + `context-engineering` |

---

## Module Map

### Main Crate (`impulse-rs/`) — 31 modules, ~41K lines

| Module | Purpose | Primary File |
|---|---|---|
| `daemon` | Unix socket IPC server | `src/daemon/mod.rs` |
| `state` | In-memory state with dirty flag + atomic sync | `src/state/mod.rs` |
| `tooling` | DynamicTool trait + registry + execution | `src/tooling/mod.rs` |
| `injection` | Context injection staging + surfaces | `src/injection/staging.rs` |
| `guardrail` | Pre-execution gating engine | `src/guardrail/engine.rs` |
| `context_lifecycle` | Bidirectional context manager for PTY panes | `src/context_lifecycle/` |
| `impulse_agent` | LLM integration (Anthropic/OpenAI/harness) | `src/agent/` |
| `intent` | Agent intent detection from PTY output | `src/intent/` |
| `retrieval` | Search index (embeddings + text) | `src/retrieval/indexer.rs` |
| `llm_backends` | LLM provider abstraction | `src/llm_backends/` |
| `mcp` | MCP server (stub) | `src/mcp/` |
| `plugin` | Plugin registry (stub) | `src/plugin/` |
| `office` | Excel/Word generation (feature-gated) | `src/office/` |
| `agent_discovery` | Detect AI agents on PATH | `src/agent_discovery/` |
| `credentials` | API key management | `src/credentials/` |
| `memory` | Memory persistence layer | `src/memory/` |
| `notification` | Desktop notifications | `src/notification/` |
| `orchestration` | Multi-agent orchestration | `src/orchestration/` |
| `verify` | Verification commands | `src/verify/` |

### Terminal Crate (`impulse-term/`) — 7 files, ~2.4K lines

| Module | Purpose |
|---|---|
| `backend.rs` | PTY spawn + vt100 parser + reader thread |
| `renderer.rs` | Run-based rendering for egui |
| `input.rs` | Key → escape sequence mapping |
| `theme.rs` | ANSI color resolution + palette |
| `context.rs` | Context bridge (token estimation, extraction) |
| `panel.rs` | Assembled egui terminal widget |

### GUI Crate (`impulse-gui/`) — multi-view workbench

| Module | Purpose |
|---|---|
| `app.rs` | ImpulseApp coordinator, view dispatch |
| `views/terminals.rs` | Terminal tab multiplexer |
| `views/sessions.rs` | Daemon session viewer |
| `views/genome.rs` | Decision viewer/editor |
| `views/search.rs` | Memory search |
| `views/settings.rs` | Configuration panel |
| `views/overview.rs` | Project overview |
| `widgets/sidebar.rs` | Navigation sidebar |
| `widgets/signal_bus.rs` | Event routing + debounce |
| `widgets/status_bar.rs` | Bottom status bar |
| `agent_panel/` | Left-side agent chat panel |

---

## Testing Conventions

### Test Organization
- Unit tests: `mod tests` at bottom of each file
- Integration tests: `src/daemon/tests.rs` with `DaemonGuard` RAII
- Naming: `test_<module>_<behavior>` (e.g., `test_guard_engine_blocks_force_push`)

### Running Tests
```bash
cd impulse-rs
cargo test                              # All tests
cargo test -- --skip integration_tests  # Unit only
cargo test daemon::tests                # Specific module
cd impulse-term && cargo test           # Terminal crate
cd impulse-gui && cargo test            # GUI crate
```

### DaemonGuard RAII Pattern
Integration tests spawn a real daemon in a tempdir:
```rust
let guard = DaemonGuard::spawn(&tempdir).await?;
// Test via IPC...
// guard drops → kills daemon, removes socket, cleans tempdir
```
Each test gets a unique socket path to support parallel execution.

### Verification Before Commit
```bash
cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```
All four must pass. Never skip with `--no-verify`.

---

## Cross-Crate Conventions

### impulse-term → impulse-gui
- `impulse-gui` depends on `impulse-term` as a workspace member
- GUI creates `TerminalBackend` + `TerminalPanel` per tab
- Context bridge data flows: `impulse-term` extracts → `impulse-gui` displays via signal bus

### impulse-gui → main crate
- GUI connects to daemon via Unix socket IPC (background poller thread)
- Shares `DaemonRequest`/`DaemonResponse` types via `impulse-rs` dependency
- `SharedState` in GUI mirrors daemon state; IPC is the sync mechanism

### Feature Flags
| Flag | Default | What It Enables |
|---|---|---|
| `office-support` | Yes | Excel/Word generation (calamine, docx-rs) |
| `monty-support` | No | Embedded Python scripts |
| `datafusion-support` | No | SQL queries over data |

---

## Common Gotchas

1. **Doubled keystrokes in terminal**: If you handle printable characters in both
   `Event::Key` and `Event::Text`, keys appear twice. See `impulse-term/src/input.rs`.

2. **FairMutex required for parser**: Using `std::sync::Mutex` for the vt100 parser
   causes UI thread starvation. Always use `parking_lot::FairMutex`.

3. **Atomic writes are mandatory**: Never write directly to `.impulse/` files.
   Always temp + rename. See `src/state/mod.rs`.

4. **Dirty flag on every mutation**: If you add a new mutation to `State`, set
   `self.dirty = true`. Forgetting this causes data loss on shutdown.

5. **Daemon request/response pairs**: Every new `DaemonRequest` variant needs a
   corresponding `DaemonResponse` variant and a match arm in the handler.

6. **GUI view registration**: New views need: ViewId enum variant, sidebar entry,
   keyboard shortcut (Ctrl+N), and the view struct implementing show().

7. **Signal bus debounce**: New signal kinds need a debounce window in the
   `debounce_window()` method. Missing this causes notification spam.

8. **Test isolation**: Integration tests MUST use `tempfile::TempDir` for the
   `.impulse/` directory. Never use the real project directory.

---

## SessionStart Context Hook (Suggestion)

Add to `.claude/hooks/hooks.json` for automatic skill surfacing:

```json
{
  "hooks": {
    "SessionStart": [{
      "command": "git diff --name-only HEAD~3 2>/dev/null | head -20"
    }]
  }
}
```

This shows recently modified files at session start. Cross-reference with the
Module-to-Skill Map above to identify which skills are relevant.
