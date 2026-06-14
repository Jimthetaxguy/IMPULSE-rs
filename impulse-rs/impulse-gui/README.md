# impulse-gui

> **LEGACY / FROZEN:** `impulse-gui` is the old egui/eframe desktop workbench. Do not add new features here. Active desktop work belongs in the Dioxus Desktop `impulse-desktop` host; this crate receives compile-maintenance and historical comparison only until removal after parity.

Native desktop workbench for Impulse -- an egui/eframe application that hosts embedded terminals, agent coordination, and session memory in a single window.

## Architecture

```
ImpulseApp (app.rs)
├── Sidebar         (widgets/sidebar.rs)
├── Views           (views/)
│   ├── Overview    — daemon status, agent activity, ops snapshot
│   ├── Terminals   — PTY multiplexer with context lifecycle
│   ├── Memory      — sessions, genome, search (3 sub-tabs)
│   └── Settings    — categorized config editor (5 sections)
├── Agent Panel     (agent_panel/) — supervisor chat, right side
├── Widgets         (widgets/)
│   ├── StatusBar        — connection + protocol + session info
│   ├── CommandPalette   — fuzzy-match command launcher
│   ├── ConflictBanner   — file conflict alerts
│   ├── Notifications    — toast-style alerts with severity
│   ├── ProjectSelector  — recent project switcher
│   └── SignalBus        — cross-component event system
└── IPC Client      (ipc/) — sync Unix socket client to daemon
```

**State flow:** A background poller thread connects to the Impulse daemon over a Unix socket, polls for sessions/history/genome/ops data, and writes results into `SharedState` behind `Arc<Mutex<>>`. Views read from `SharedState` each frame. Terminal telemetry flows back to the daemon via `PublishTerminalOps`.

## Views

| View | Module | Description |
|------|--------|-------------|
| **Workbench** | `overview.rs` | Dashboard with daemon connection status, active sessions, and ops snapshot summary |
| **Terminals** | `terminals.rs` | PTY multiplexer -- spawn Claude Code, Codex, legacy OpenCode, or shell tabs with full vt100 rendering |
| *Terminal Context* | `terminal_context.rs` | Context lifecycle: extraction ticks, threshold injection, signal collection |
| *Terminal Insights* | `terminal_insights.rs` | Insight persistence: append to `LIVE_INSIGHTS.jsonl`, merge, search across panes |
| *Terminal Search* | `terminal_search.rs` | Ctrl+F overlay searching all pane transcripts with match counts and F3 navigation |
| **Memory** | `memory.rs` | Container with 3 sub-tabs: Sessions, Genome, Search |
| *Sessions* | `sessions.rs` | Active sessions list + history timeline with detail cards |
| *Genome* | `genome.rs` | Decision timeline with filter, date headers, tags, rationale; raw text tab |
| *Search* | `search.rs` | Query the daemon's retrieval system with relevance-scored results |
| **Settings** | `settings.rs` | 20 config keys in 5 sections: Agent, Stewardship, Injection, Search, Performance |
| **Guardrails** | `guardrails.rs` | Active guardrail rules with action/target filtering (block/warn/log) |

## Agent Panel

The right-side panel (`agent_panel/`) provides supervisor chat for coordinating across terminal panes.

| Module | Purpose |
|--------|---------|
| `mod.rs` | Panel layout, input field, message scroll |
| `backend.rs` | Backend auto-detection: daemon > Claude Code subprocess > direct API > unavailable |
| `chat.rs` | Message threading, context enrichment with cross-pane insights |
| `actions.rs` | Supervisor actions and proposal execution |
| `persistence.rs` | Chat history save/load |

## Themes

Four named themes, switchable at runtime and persisted to `config.json`:

| Theme | Accent | Background |
|-------|--------|------------|
| **Launch** (default) | Electric blue | Deep space navy |
| **Nebula** | Purple-violet | Deep plum |
| **Solar** | Amber-gold | Dark brown |
| **Aurora** | Emerald-cyan | Deep ocean |

Each theme provides a full `ColorPalette` with semantic tokens: backgrounds (deep, surface, hover), accent (normal, bright, dim), text (normal, muted, dim, faint), and status colors (green, yellow, red, blue).

## Building

```bash
cargo build -p impulse-gui            # Debug build
cargo build -p impulse-gui --release  # Release build
cargo run -p impulse-gui              # Run (1200x800 default window)
cargo run -p impulse-gui -- --debug   # Run with debug logging
```

## Testing

```bash
cargo test -p impulse-gui             # 251 tests
```

Tests cover IPC protocol round-trips, theme validation, serde round-trips for wire types, memory persistence, signal bus routing, notification lifecycle, and widget state.

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+1-4` | Switch views (Workbench / Terminals / Memory / Settings) |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+T` | New terminal tab |
| `Ctrl+W` | Close current terminal tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous terminal tab |
| `Ctrl+K` | Open Memory |
| `Ctrl+R` | Refresh daemon data |
| `Ctrl+L` | Focus Agent panel |

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `eframe` 0.31 | egui native app framework |
| `impulse-ops` | Shared types: `WorkbenchDaemonRequest/Response`, `SupervisorAction`, `OpsSnapshot` |
| `impulse-term` | Terminal widget: PTY spawn, vt100 parsing, `WriteQueue`, context bridge |
| `serde` + `serde_json` | IPC serialization and config persistence |
| `chrono` | Timestamp formatting for sessions and history |
| `ureq` | HTTP client for direct API agent backend |
| `rfd` | Native file dialogs (project selector) |
| `which` | Agent binary auto-detection (claude, codex, opencode) |
| `thiserror` | Typed error enum (`GuiError`) |
| `dirs` | Platform-standard config paths (`~/.impulse/`) |

## IPC Integration

The GUI connects to the Impulse daemon via a synchronous Unix socket client (`ipc/client.rs`). No tokio dependency -- uses `std::os::unix::net::UnixStream` on a background `std::thread`.

**Socket discovery:** Walks up from `$CWD` looking for `.impulse/sockets/impulse.sock`, falls back to `IMPULSE_SOCKET_PATH` env var.

**Protocol:** JSON-line over Unix domain socket. Request/response types are shared via `impulse-ops` (`WorkbenchDaemonRequest` / `WorkbenchDaemonResponse`). Protocol version handshake ensures GUI and daemon are compatible.

**Endpoints used:**

| Category | Requests | Purpose |
|----------|----------|---------|
| Core | `Ping`, `Status`, `ListSessions` | Health checks, session listing |
| Session | `CreateSession`, `EndSession`, `TrackFile` | Session lifecycle management |
| Tools | `InvokeTool` (session_query, genome_read, memory_search) | Data retrieval |
| Ops | `GetOpsSnapshot`, `SubscribeOps`, `PublishTerminalOps` | Bidirectional ops telemetry |
| Supervisor | `SupervisorChat`, `RunSupervisorAction`, `GetSupervisorPermissions` | Agent coordination |
| Artifacts | `RunArtifactAction` | Artifact management |
| Guards | `GuardList` | Guardrail rule retrieval |
