---
title: Desktop Shell Architecture
status: active
version: 1.0.0
created: 2026-04-15
updated: 2026-04-15
---

# Desktop Shell Architecture

This document is the canonical reference for the Impulse desktop shell. It describes layer boundaries, runtime responsibilities, and data flow. All implementation work on the desktop product must be consistent with this document.

## Decision

The chosen desktop stack is:

- **Shell container:** Tauri 2.x
- **UI layer:** Dioxus (inside the Tauri webview)
- **Terminal rendering:** xterm.js (mounted into Dioxus `rsx!` component divs via `eval()`)
- **PTY / session / daemon ownership:** existing Rust backend (`impulse-term`, `impulse-ops`, daemon)
- **Terminal-native operator surface:** `ratatui` (standalone, preserved)
- **Legacy desktop surface:** `egui` / `impulse-gui` (freeze and sunset)

See `docs/decisions/0007-desktop-shell-stack.md` for the full ADR.

---

## Layer Boundaries

```
DESKTOP SHELL (Tauri + Dioxus webview)
  Left Rail | Center Terminal Panes (xterm.js) | Right Inspector
  Top Bar (daemon status)
  Bottom Strip (event log)

          | Tauri IPC commands + events |

RUST BACKEND (src-tauri)
  Terminal Bridge: terminal_open/write/resize/close/focus
  Terminal Events: terminal_output/exit/status/ops_update
  impulse-term core: TerminalBackend, WriteQueue, PTY, vt100
  impulse-ops / daemon IPC: ProjectOpsSnapshot, TerminalOpsReport

          | Unix socket / local IPC |

DAEMON (impulse-rs binary)
  Session lifecycle, context extraction, memory pipeline,
  agent coordination, snapshot publishing
```

---

## Runtime Responsibilities

### Tauri Shell (src-tauri)

- Owns the native window, menu bar, and OS-level lifecycle
- Hosts the Dioxus webview
- Exposes command handlers for the terminal bridge API
- Subscribes to daemon events and forwards them to the frontend as Tauri events
- Never becomes the PTY owner
- Never holds UI state - it relays state from daemon/backend to the frontend

### Dioxus Frontend (inside webview)

- Renders all non-terminal UI chrome using `rsx!` components
- Mounts xterm.js instances into `<div>` elements for each terminal pane
- Subscribes to Tauri events (`terminal_output`, `ops_update`, etc.) and routes them to the correct xterm.js instance or UI component
- Sends user actions (keyboard input, resize, tab switch, session commands) as Tauri commands to the backend
- **Does not hold authoritative state.** All panel data (sessions, context, artifacts, supervisor) is read from daemon snapshots, not from frontend-local shadow copies

### Terminal Panes (xterm.js)

- Render PTY byte streams received via `terminal_output` events
- Send keyboard input back as `terminal_write` commands
- Notify backend of resize via `terminal_resize` commands when the pane dimensions change
- All terminal rendering is handled by xterm.js - no custom Rust terminal widget is built for the desktop path

### impulse-term Core

- Owns PTY spawn, stdin/stdout, resize (SIGWINCH), env injection, and session lifecycle
- Exposes `TerminalBackend` and `WriteQueue` as the authoritative PTY interface
- Has no rendering dependency - the `eframe` dependency must be removed before desktop wiring begins
- `backend.rs`, `context.rs`, and session logic are reusable by both the ratatui path and the desktop path

### impulse-ops / Daemon

- Source of truth for all session, context, artifact, and supervisor state
- Publishes `ProjectOpsSnapshot` and `TerminalOpsReport` snapshots
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
  -> Tauri backend emits terminal_output event {session_id, data: Vec<u8>}
  -> Dioxus event listener routes to correct xterm.js instance
  -> xterm.js.write(data)
  -> WebKit canvas/WebGL renders glyphs
```

### Terminal Input Path

```
User keypress in xterm.js
  -> xterm.js onData handler
  -> Dioxus eval() bridge sends terminal_write command
  -> Tauri backend command handler
  -> WriteQueue.write_user_input(bytes)
  -> PTY stdin -> Child process stdin
```

### Resize Path

```
Pane container resized (CSS layout change)
  -> xterm.js ResizeObserver / fit addon
  -> Dioxus eval() sends terminal_resize command {session_id, cols, rows}
  -> Tauri backend command handler
  -> TerminalBackend.resize(cols, rows)
  -> parser lock -> PTY master resize -> SIGWINCH -> parser set_size
```

### Daemon State Path

```
Daemon publishes ProjectOpsSnapshot / TerminalOpsReport
  -> Tauri backend receives via daemon IPC subscription
  -> Backend emits ops_update Tauri event {snapshot}
  -> Dioxus component subscribes to ops_update
  -> Component re-renders from new snapshot
  -> Side panels (context, artifacts, supervisor) update
```

---

## Terminal Bridge API

This is the full public interface between the Dioxus frontend and the Rust backend. It must stay thin.

### Commands (frontend -> backend)

| Command | Payload | Description |
|---|---|---|
| `terminal_open` | `{session_id, command, args, cwd, env, rows, cols}` | Spawn a PTY session |
| `terminal_write` | `{session_id, data: Vec<u8>}` | Write bytes to PTY stdin |
| `terminal_resize` | `{session_id, cols, rows}` | Resize PTY and parser |
| `terminal_close` | `{session_id}` | Kill PTY and clean up session |
| `terminal_focus` | `{session_id}` | Notify backend of focus change |

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
5. **The desktop shell is additive.** Daemon contracts are not changed to serve the desktop.
