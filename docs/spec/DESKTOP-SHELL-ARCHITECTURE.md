---
title: Desktop Shell Architecture
status: active
version: 1.0.0
created: 2026-04-15
updated: 2026-07-13
---

# Desktop Shell Architecture

This document is the canonical reference for the Impulse desktop shell. It describes layer boundaries, runtime responsibilities, and data flow. All implementation work on the desktop product must be consistent with this document.

## Decision

The chosen desktop stack is:

- **Shell host:** Dioxus Desktop
- **UI layer:** Dioxus (`rsx!` components and signals)
- **Terminal rendering:** xterm.js (mounted into Dioxus `rsx!` component divs via `eval()`)
- **PTY / session / daemon ownership:** existing Rust backend (`impulse-term`, `impulse-ops`, daemon)
- **Terminal-native operator surface:** `ratatui` (standalone, preserved)
- **Legacy desktop surface:** `egui` / `impulse-gui` (freeze and sunset)
- **Legacy host adapter:** Tauri-shaped command/event bridge (compatibility only)

See `docs/decisions/0008-dioxus-desktop-host.md` for the current ADR.

---

## Layer Boundaries

```
DESKTOP SHELL (Dioxus Desktop)
  Left Rail | Center Terminal Panes (xterm.js) | Right Inspector
  Top Bar (daemon status)
  Bottom Strip (event log)

          | Dioxus host adapter commands + events |

RUST BACKEND (impulse-desktop / impulse-term)
  Terminal Bridge: terminal_open/write/resize/close/focus
  Terminal Events: terminal_output/exit/status/ops_update
  impulse-term core: TerminalBackend, WriteQueue, PTY, vt100
  impulse-ops / daemon IPC: ProjectOpsSnapshot, TerminalOpsReport

          | Unix socket / local IPC |

DAEMON (impulse-rs binary)
  Session + governed-task lifecycle, context extraction, memory pipeline,
  agent coordination, snapshot publishing
```

---

## Runtime Responsibilities

### Dioxus Desktop Host

- Owns the native window and desktop lifecycle
- Hosts the Dioxus application and installs `window.__IMPULSE_DESKTOP_HOST`
- Exposes host command handlers for the terminal bridge API
- Subscribes to daemon/runtime events and forwards them to the frontend as host events
- Never becomes the PTY owner
- Never holds UI state - it relays state from daemon/backend to the frontend

### Legacy Tauri-Shaped Adapter

- Exists only to keep older command/event tests and compatibility paths green while Dioxus Desktop launch plumbing lands
- Must not be used as the next product scaffold
- Must be removable after Dioxus host command/event parity is covered by tests

### Dioxus Frontend

- Renders all non-terminal UI chrome using `rsx!` components
- Mounts xterm.js instances into `<div>` elements for each terminal pane
- Subscribes to host events (`terminal_output`, `ops_update`, etc.) and routes them to the correct xterm.js instance or UI component
- Sends user actions (keyboard input, resize, tab switch, session commands) as host commands to the backend
- **Does not hold authoritative state.** All panel data (sessions, context, artifacts, supervisor) is read from daemon snapshots, not from frontend-local shadow copies

### Terminal Panes (xterm.js)

- Render PTY byte streams received via `terminal_output` events
- Send keyboard input back as `terminal_write` commands
- Notify backend of resize via `terminal_resize` commands when the pane dimensions change
- All terminal rendering is handled by xterm.js - no custom Rust terminal widget is built for the desktop path

### Native Islands (macOS)

- Provide macOS-specific affordances such as menu bar, global shortcuts, file panels, notifications, accessibility hooks, and optional floating panels
- Are invoked through serializable request/result DTOs, not shared UI state
- May use Swift/AppKit behind an Objective-C-compatible ABI (`@objc`/`NSObject`) or Rust `objc2`
- Must not retain authoritative session, memory, terminal, or artifact state; Dioxus remains the interface owner and daemon snapshots remain authoritative

### impulse-term Core

- Owns PTY spawn, stdin/stdout, resize (SIGWINCH), env injection, and session lifecycle
- Exposes `TerminalBackend` and `WriteQueue` as the authoritative PTY interface
- Keeps new PTY/process behavior framework-neutral; the optional egui renderer is frozen legacy
  compatibility and must not receive new product behavior
- `backend.rs`, `context.rs`, and session logic are reusable by both the ratatui path and the desktop path

### impulse-ops / Daemon

- Source of truth for all session, governed-task, context, artifact, and supervisor state
- Accepts desktop-authored `TerminalOpsReport` telemetry through `PublishTerminalOps`, overlays it
  onto durable workbench truth, and returns `ProjectOpsSnapshot`/`OpsSubscription` read models
- Desktop shell panels read exclusively from daemon state, never from frontend-local shadow state
- Daemon reconnect must restore desktop shell state cleanly without a full restart

### ratatui (standalone)

- Remains a first-class terminal-native operator surface
- Provides behavior reference for operator flows during migration
- Is not the desktop pixel renderer
- Must remain functional throughout the entire migration

---

## Data Flow

### Terminal Output Path

```
Child process stdout
  -> PTY master reader thread (pty_reader_loop)
  -> vt100::Parser (backend.rs)
  -> Dioxus host adapter emits terminal_output event {session_id, data: Vec<u8>}
  -> Dioxus event listener routes to correct xterm.js instance
  -> xterm.js.write(data)
  -> WebKit canvas/WebGL renders glyphs
```

### Terminal Input Path

```
User keypress in xterm.js
  -> xterm.js onData handler
  -> Dioxus eval() bridge sends terminal_write command
  -> host command handler
  -> WriteQueue.write_user_input(bytes)
  -> PTY stdin -> Child process stdin
```

### Resize Path

```
Pane container resized (CSS layout change)
  -> xterm.js ResizeObserver / fit addon
  -> Dioxus eval() sends terminal_resize command {session_id, cols, rows}
  -> host command handler
  -> TerminalBackend.resize(cols, rows)
  -> parser lock -> PTY master resize -> SIGWINCH -> parser set_size
```

### Daemon State Path

```
Desktop runtime publishes TerminalOpsReport
  -> daemon overlays fresh terminal telemetry onto durable workbench truth
  -> SubscribeOps returns ProjectOpsSnapshot / OpsSubscription
  -> Dioxus host receives the authoritative read model
  -> Backend emits ops_update host event {snapshot}
  -> Dioxus component subscribes to ops_update
  -> Component re-renders from new snapshot
  -> Side panels (context, artifacts, supervisor) update
```

### Governed Task Decision Path

```text
Supervisor/operator action in Dioxus
  -> governed_task_mutate host command
  -> acknowledged desktop daemon client
  -> daemon expected-revision/idempotency transition
  -> authoritative GovernedTaskRun response + next ops_update
  -> card re-renders; no optimistic task state
```

---

## Terminal and Governed Bridge Subset

This table documents the terminal, governed-task, and native-island subset used in the data flows
above. The complete current host-command manifest is `host_commands::HOST_INVOKE_COMMANDS`; it also
contains the `agent_*`, workspace, MCP, review, and supervisor command families. All host commands
must remain thin adapters over backend-owned state and policy.

### Commands (frontend -> backend)

| Command | Payload | Description |
|---|---|---|
| `terminal_open` | `{session_id, command, args, cwd, env, rows, cols}` | Spawn a PTY session |
| `terminal_write` | `{session_id, data: Vec<u8>}` | Write bytes to PTY stdin |
| `terminal_resize` | `{session_id, cols, rows}` | Resize PTY and parser |
| `terminal_close` | `{session_id}` | Kill PTY and clean up session |
| `terminal_focus` | `{session_id}` | Notify backend of focus change |
| `governed_task_mutate` | `{request: GovernedTaskMutationRequest}` | Apply an acknowledged revisioned task transition and return daemon-owned state |
| `native_island_request` | `{request_id, kind, payload}` | Invoke a narrow native macOS island and return a serialized result |

### Events (backend -> frontend)

| Event | Payload | Description |
|---|---|---|
| `terminal_output` | `{session_id, data: Vec<u8>}` | PTY stdout bytes |
| `terminal_exit` | `{session_id, exit_code}` | PTY child exited |
| `terminal_status` | `{session_id, alive, cols, rows}` | Status change |
| `ops_update` | `{snapshot: ProjectOpsSnapshot}` | Daemon state update |

---

## Invariants

1. **PTY ownership never moves to the frontend.** The Rust backend owns all PTY handles.
2. **Panel state comes from daemon, not from terminal scraping.** Context, artifacts, and supervisor panels are populated from `ops_update` snapshots.
3. **ratatui stays functional.** No migration step should break the standalone ratatui operator surface.
4. **egui is not extended.** The `impulse-gui` crate receives only compile-maintenance.
5. **The desktop shell does not fork authority.** Shared daemon contracts may evolve for product
   capabilities such as governed tasks, but desktop-only shadow state or policy is forbidden.
6. **Task decisions are acknowledged.** The UI waits for daemon-owned state and never treats a
   terminal exit or optimistic click result as task acceptance.
