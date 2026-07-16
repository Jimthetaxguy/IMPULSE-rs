---
name: impulse-development
description: "Use when adding features to the Impulse codebase — CLI commands, daemon IPC messages, GUI views, dynamic tools, context lifecycle, or guardrails. Routes to detailed step-by-step guides."
version: 2.0.0
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
  - frontend-design
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
  - impulse-rs/impulse-desktop/src/ui.rs
  - impulse-rs/impulse-desktop/src/host_bridge.rs
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
| Add a desktop view | `references/adding-dioxus-views.md` | `impulse-desktop/src/ui.rs`, `views.rs` | `frontend-design` |
| Add a dynamic tool | `references/adding-dynamic-tools.md` | `src/tooling/mod.rs`, `traits.rs` | `rust-trait-design` |
| Add a guardrail rule | CLAUDE.md guardrail section | `src/guardrail/` | `plugin-hooks-guardrails` |
| Add a context lifecycle stage | Context lifecycle docs | `src/context_lifecycle/` | `context-engineering` |
| Add a terminal feature | impulse-term docs | `impulse-term/src/` | `pty-terminal-engineering` |
| Add a desktop command/event | `references/adding-dioxus-views.md` + daemon IPC guide | `host_commands.rs`, `host_bridge.rs` | `rust-daemon-ipc` |

### Module-to-Skill Map (Context-Adaptive Loading)

When working on files in these paths, auto-surface the corresponding skills:

| Active Module / Path Pattern | Auto-surface Skills |
|---|---|
| `src/daemon/**` | `rust-daemon-ipc` + `rust-async-concurrency` |
| `impulse-desktop/src/ui.rs`, `views.rs`, `assets/**` | `frontend-design` |
| `impulse-desktop/src/host_bridge.rs`, `host_commands.rs` | `rust-daemon-ipc` + `pty-terminal-engineering` |
| `impulse-term/**` | `pty-terminal-engineering` |
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

### Terminal Crate (`impulse-term/`) — framework-neutral PTY core

| Module | Purpose |
|---|---|
| `backend.rs` | PTY spawn + vt100 parser + reader thread |
| `context.rs` | Context bridge (token estimation, extraction) |
| legacy optional modules | Frozen EGUI compatibility pending gated physical removal; never add product behavior |

### Desktop Crate (`impulse-desktop/`) — active Dioxus cockpit

| Module | Purpose |
|---|---|
| `ui.rs` | Supervisor-first cockpit composition, signals, launcher, terminal mounting |
| `views.rs` | Typed Terminal, Memory, Review, Artifacts, and Supervisor routes |
| `host_commands.rs` | Typed command surface over authoritative Rust state |
| `host_bridge.rs` | Dioxus eval transport and host event stream |
| `runtime.rs` | Role-aware PTY/session lifecycle |
| `daemon_ops.rs` | Daemon telemetry and governed-task gateway |
| `assets/impulse_crt.css` | Current restrained industrial styling; filename is legacy only |

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
cargo test -p impulse-desktop --locked  # Dioxus cockpit contracts
cargo check -p impulse-desktop --locked --features desktop-app
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

### impulse-term → impulse-desktop
- `impulse-desktop` depends on the default, framework-neutral `impulse-term` core
- Rust owns PTYs and lifecycle; xterm.js owns desktop glyph rendering
- New terminal behavior must remain independent of Dioxus, ratatui, and EGUI

### impulse-desktop → control plane
- Dioxus renders daemon/runtime read models and sends typed host commands
- `impulse-ops` and the daemon own task, evidence, review, and operations truth
- UI signals may hold transient selection/focus state only; never shadow authoritative project state
- The legacy `impulse-gui` crate is default-inactive and must receive no new work

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

6. **Desktop view registration**: New views need a `DesktopView` route, intentional navigation,
   SSR/contract coverage, and an authoritative backend read model. Do not create a local mock store.

7. **Host event order**: Command responses and PTY/ops events share the live bridge. Preserve FIFO
   invoke ordering, bounded queues, and fail-closed degraded status.

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
