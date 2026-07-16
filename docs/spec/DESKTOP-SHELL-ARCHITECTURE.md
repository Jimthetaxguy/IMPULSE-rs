---
title: Desktop Shell Architecture
status: active
version: 1.2.0
created: 2026-04-15
updated: 2026-07-16
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
  Left Oversight Dock | Center Mission + Terminal | Right Launch / Inspector
  Top Bar (shared launch target + connected-daemon state + review entry)
  Bottom Strip (meaningful event signals)

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

## Cockpit Information Hierarchy

The desktop is an operator cockpit for coordinated coding work, not a memory dashboard and not a
marketing splash screen.

1. **Project and assignment first.** The active project, current task, worker state, and next
   operator action must dominate the center lane.
2. **Oversight stays visible.** The left dock exposes daemon-owned review/evidence attention and
   worker state. It must say `Oversight` until a real Supervisor runtime has been launched; UI
   location or a review service must never imply a model-backed supervisor exists.
3. **Role and runtime stay separate.** The right launch dock selects a runtime such as Claude Code
   or Codex and assigns the governed Builder role as a distinct contract.
4. **Terminal work remains primary.** xterm panes are the execution surface. Memory, artifacts,
   evidence, and low-level telemetry support that work through inspectable routes or disclosure.
5. **Empty state teaches the real loop.** Register project -> choose runtime -> define assignment
   and acceptance criteria -> launch Builder -> inspect evidence. Zero-value statistics do not earn
   primary space.
6. **No implicit home-wide governance.** A packaged launch without a standard project-local daemon
   socket renders oversight as disconnected. `~/.impulse` memory fallback is not a project scope,
   and the first governed launch must bind daemon, project memory, telemetry, and task commands to
   the exact registered target before MCP context lookup or PTY creation. Activation rejects
   external state/socket/lock symlinks before filesystem or process mutation, then requires daemon
   attestation of the canonical project id, repository root, and local `.impulse` root. The task
   gateway rejects cross-project registrations and mutations independently of the UI.

Visual styling is restrained, industrial, and terminal-native. Amber and cyan are status accents,
not full-screen decoration. Retro texture may appear as a quiet brand detail, but scanline overlays,
flicker, giant glowing logos, and decorative telemetry must not compete with project work. The
historical June design exploration is reference material, not a current screen contract.

---

## Runtime Responsibilities

### Dioxus Desktop Host

- Owns the native window and desktop lifecycle
- Hosts the Dioxus application and installs `window.__IMPULSE_DESKTOP_HOST`
- Exposes host command handlers for the terminal bridge API
- Subscribes to daemon/runtime events and forwards them to the frontend as host events
- Attaches or starts a daemon companion only when an exact project-local boundary is explicit, or
  when the first governed launch supplies the exact selected project; otherwise it
  keeps oversight and project-memory operations disconnected
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
- The governed Builder launcher collects exact acceptance criteria and selects the closed
  `rust_workspace_v1` profile. Profiled evidence cards show
  `"$IMPULSE_CONTROL_CLI" --daemon governed-verify` and
  `"$IMPULSE_CONTROL_CLI" --daemon governed-review`; producer buttons are not part of the current host surface.

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
- Protocol v5 owns profiled claim, detached verification, and strict API Supervisor-review
  producers. The desktop cannot compose actor, subject, evidence, or verdict payloads for them.
- Protocol v6 additively exposes deterministic accepted-run candidates through the serde-defaulted
  `ProjectOpsSnapshot.memory_candidates` read model. The Memory view has no candidate mutation or
  `GENOME` promotion authority.

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
Profiled Builder launch in Dioxus
  -> exact task + acceptance criteria + rust_workspace_v1
  -> desktop observes clean canonical Git HEAD
  -> daemon independently re-attests HEAD and registers before PTY spawn
  -> desktop injects project/task/socket/control-CLI/profile routing

Builder terminal or Ion tool
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-claim / governed_submit_claim
     (summary + artifact ids only)
  -> daemon derives Worker + clean subject
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-verify runs fixed Rust commands in a detached checkout
  -> "$IMPULSE_CONTROL_CLI" --daemon governed-review runs strict tool-free/stateless API Supervisor review
  -> ops_update renders daemon-owned evidence

Operator action in Dioxus
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

There are intentionally no `governed_verify` or `governed_review` host commands in this table.
Profiled cards guide the operator to the routed control CLI inside the governed terminal. The
packaged executable is `impulse-rs`; `$IMPULSE_CONTROL_CLI` retains the exact injected executable
path, and the global `--daemon` flag must precede the producer subcommand. Adding
producer buttons requires a separate acknowledged host-command contract; UI-authored automatic
producer records are forbidden.

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
7. **Profiled producer truth is daemon-owned.** Dioxus supplies launch criteria and renders results;
   it never supplies automatic claim actor/subject, command evidence, or Supervisor verdict.
8. **Verification is host-trusted, not sandboxed.** The detached `rust_workspace_v1` checkout still
   executes project-authored Rust build scripts, proc macros, and tests on the host.
