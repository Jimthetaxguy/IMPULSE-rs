---
name: impulse-development
description: "Use when adding features to the Impulse codebase — CLI commands, daemon IPC messages, GUI views, dynamic tools, context lifecycle, or guardrails. Routes to detailed step-by-step guides."
version: 1.0.0
updated: 2026-08-29
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
  - impulse-rs/impulse-desktop/src/lib.rs
  - impulse-rs/impulse-desktop/src/ui.rs
  - impulse-rs/impulse-desktop/src/views.rs
  - impulse-rs/src/tooling/mod.rs
  - impulse-rs/src/guardrail/engine.rs
  - impulse-rs/impulse-term/src/backend.rs
---

# Impulse Development Guide

Project-level skill for adding features to the Impulse codebase. Routes to the
right reference file and global skills based on what you're building.

The active GUI is the Dioxus Desktop host in `impulse-desktop` (ADR-0008).
`impulse-gui` is a frozen egui/eframe workbench excluded from the workspace.
Do not add GUI views or product shell work there.

---

## Task Routing Table

| What You're Doing | Reference File | Key Source Files | Global Skill |
|---|---|---|---|
| Add a CLI command | `references/adding-cli-commands.md` | `src/main.rs`, handler module | `rust-programming` |
| Add a daemon IPC message | `references/adding-daemon-ipc.md` | `src/daemon/mod.rs` | `rust-daemon-ipc` |
| Add a GUI view | `references/adding-gui-views.md` | `impulse-desktop/src/ui.rs`, `views.rs` | `rust-programming` |
| Add a dynamic tool | `references/adding-dynamic-tools.md` | `src/tooling/mod.rs`, `traits.rs` | `rust-trait-design` |
| Add a guardrail rule | CLAUDE.md guardrail section | `src/guardrail/` | `plugin-hooks-guardrails` |
| Add a context lifecycle stage | Context lifecycle docs | `src/context_lifecycle/` | `context-engineering` |
| Add a terminal feature | impulse-term docs | `impulse-term/src/` (PTY); xterm.js in `impulse-desktop` | `pty-terminal-engineering` |
| Add a host command or desktop event | `impulse-desktop/README.md` | `impulse-desktop/src/host_commands.rs`, `host_bridge.rs`, `ui.rs` | `rust-programming` |

### Module-to-Skill Map (Context-Adaptive Loading)

When working on files in these paths, auto-surface the corresponding skills:

| Active Module / Path Pattern | Auto-surface Skills |
|---|---|
| `src/daemon/**` | `rust-daemon-ipc` + `rust-async-concurrency` |
| `impulse-desktop/src/**` | `rust-programming` (Dioxus Desktop host; ADR-0008) |
| `impulse-gui/**` | Frozen egui workbench. Do not add views. Use `impulse-desktop`. |
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

### Terminal Crate (`impulse-term/`) — 7 files, ~2.4K lines

PTY and vt100 authority. Product terminal glyphs render in xterm.js inside
`impulse-desktop`. The egui `renderer.rs` / `panel.rs` widgets exist only for
the frozen `impulse-gui` compile path.

| Module | Purpose |
|---|---|
| `backend.rs` | PTY spawn + vt100 parser + reader thread |
| `renderer.rs` | Frozen egui run-based renderer (impulse-gui only) |
| `input.rs` | Key to escape sequence mapping |
| `theme.rs` | ANSI color resolution + palette |
| `context.rs` | Context bridge (token estimation, extraction) |
| `panel.rs` | Frozen egui terminal widget (impulse-gui only) |

### Desktop Crate (`impulse-desktop/`) — active Dioxus cockpit

Workspace member. ADR-0008. New GUI work belongs here.

| Module | Purpose |
|---|---|
| `lib.rs` | Crate boundary and public host types |
| `ui.rs` | Dioxus shell, left rail, terminal stage kept alive |
| `views.rs` | Center-stage Memory / Review / Artifacts / Supervisor |
| `runtime.rs` | `DesktopRuntime` PTY and agent snapshots |
| `host_commands.rs` | Typed host invoke commands |
| `host_bridge.rs` | Dioxus host adapter seam |
| `desktop_host.rs` | `desktop-app` native host |
| `daemon_ops.rs` | Daemon publish/subscribe; workbench reads `ProjectOpsSnapshot` |
| `bridge.rs` | Terminal open/write/resize/close contracts |
| `native.rs` | macOS island DTOs |
| `mcp.rs` | Built-in MCP tool descriptors |
| `theme.rs` | Status classes and labels |
| `workspace.rs` | Workspace targeting |

### Frozen GUI Crate (`impulse-gui/`) — excluded from workspace

Legacy egui/eframe workbench. Compile maintenance only. Do not add views,
signal-bus kinds, or product shell features here.

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
cargo test -p impulse-term              # Terminal crate
cargo test -p impulse-desktop           # Dioxus desktop crate
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
- `impulse-desktop` depends on `impulse-term` as a workspace member
- Dioxus owns layout and controls; `DesktopRuntime` plus `impulse-term` own PTY bytes
- xterm.js renders terminal glyphs; Dioxus does not own PTY lifecycle
- Frozen `impulse-gui` still links `impulse-term` for compile maintenance only

### impulse-desktop → daemon
- Desktop publishes terminal ops and reads `ProjectOpsSnapshot` over daemon IPC
- Local runtime events never rewrite daemon-owned truth
- Shares ops/request types through workspace crates, not a parallel GUI state store

### Feature Flags
| Flag | Default | What It Enables |
|---|---|---|
| `office-support` | Yes | Excel/Word generation (calamine, docx-rs) |
| `monty-support` | No | Embedded Python scripts |
| `datafusion-support` | No | SQL queries over data |
| `desktop-app` (`impulse-desktop`) | No | Real Dioxus Desktop binary and host adapter |

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

6. **GUI view registration**: New Dioxus views belong in `impulse-desktop/src/views.rs`
   with a `DesktopView` variant and a left-rail entry in `ui.rs`. Do not add
   `ViewId` / `show()` work in frozen `impulse-gui`.

7. **Host commands, not an egui signal bus**: New desktop actions go through
   `host_commands.rs` / the Dioxus host adapter. Do not add `SignalKind` debounce
   windows in `impulse-gui`.

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
