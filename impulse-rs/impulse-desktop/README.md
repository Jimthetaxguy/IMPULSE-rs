# impulse-desktop

Active Dioxus cockpit and typed desktop-host contract for Impulse.

This crate is the active desktop product path. It is not the legacy/frozen
`impulse-gui`/egui workbench. Dioxus owns the product interface, Rust owns PTY/session/daemon state,
xterm.js owns terminal rendering, and macOS-native islands stay behind typed
request/result DTOs. Legacy Tauri-shaped code remains a compatibility adapter only;
it is not the product authority.

The product model is a terminal-agent harness: each coding agent is a PTY-backed
actor with an explicit workspace target, a runtime snapshot, and a visible set of
Rust MCP tools that can help the agent act inside that workspace. Dioxus renders
and requests those capabilities; Rust validates and executes them.

## Ownership

| Layer | Owner |
| --- | --- |
| Layout, rails, inspectors, command palette, review/apply surfaces | Dioxus |
| Native window/process/IPC boundary | Dioxus desktop host adapters |
| PTY lifecycle and live terminal bytes | `DesktopRuntime` / `impulse-term` |
| Reconciled agents, context, memory, artifacts, and interventions | Impulse daemon |
| Terminal glyph rendering | xterm.js |
| Menu bar, panels, notifications, accessibility hooks | Native island bridge |
| Built-in MCP tools and connector status | Rust runtime snapshots surfaced to Dioxus |

Native islands must publish serializable DTOs back to Dioxus. They must not keep
independent copies of sessions, memory, terminal state, or artifacts.

## Runtime Contracts

- `WorkspaceTarget` names the cwd/project root in which an agent process starts.
  The runtime derives it from `cwd` when the UI does not provide richer metadata.
  This targeting is not a filesystem sandbox or authorization boundary; structural
  enforcement depends on the selected runtime or sandbox.
- `AgentSpawnRequest` carries platform, command, workspace, terminal dimensions,
  and optional MCP tool descriptors.
- `AgentRuntimeSnapshot` is the desktop's local PTY fact model: focused state,
  process status, workspace, output metrics, and built-in Rust MCP tools.
- Runtime agent ids are one-use event-routing addresses for the lifetime of a
  desktop runtime. Natural exits reap their records, and lifecycle events drain
  through a reentrant FIFO so delayed callbacks cannot resurrect or retarget a
  different process.
- `daemon_ops` converts those facts to `TerminalOpsReport`, publishes lifecycle
  changes plus a two-second heartbeat, and subscribes with the daemon's opaque
  `next_seq` token. Dioxus workbench panels render only the returned
  `ProjectOpsSnapshot`; local runtime events never rewrite daemon-owned truth.
- Daemon read freshness and desktop publish health are distinct: a successful
  subscription remains current when telemetry publishing is retrying, while a
  subscription failure marks retained workbench data as cached/stale.
- The adapter binds one daemon/project and filters agents from other registered
  workspaces. Cross-project routing requires a future project identity on the
  publish/subscribe contract rather than mixing workspaces into one snapshot.
- Built-in MCP tools default to safe descriptors for `impulse.agent_spawn`,
  `impulse.agent_write`, `impulse.search_memory`, and
  `impulse.review_injection`; mutating terminal actions require confirmation.

The active Dioxus path is:

```text
Dioxus controls -> Dioxus host adapter -> DesktopRuntime -> impulse-term TerminalBackend
        ^                                |                   |
        |                                | TerminalOpsReport | terminal bytes
        |                                v                   v
        +-- daemon ProjectOpsSnapshot <- Impulse daemon   xterm.js
```

The legacy compatibility path still covered by tests is:

```text
Dioxus controls -> legacy Tauri-shaped adapter -> DesktopRuntime -> impulse-term TerminalBackend
```

## Features

- `desktop-app` enables the real Dioxus Desktop binary target:
  `cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop`.
  The binary installs the live Dioxus eval bridge behind
  `window.__IMPULSE_DESKTOP_HOST`, starts the daemon-ops publisher/subscriber,
  and reports subscription freshness separately from telemetry publish health.
- `native-macos` enables the Objective-C/AppKit compatibility bridge via
  `objc2`.
- `legacy-tauri-runtime` enables the compatibility Tauri command annotations
  without making Tauri a hard dependency of the default workspace check.
- `tauri-runtime` remains a deprecated Cargo feature alias for older commands;
  do not use it for new work.

## Visual And Host Smoke

Before running the visual smoke, `npm run vendor:xterm` copies the pinned
`@xterm/xterm` and `@xterm/addon-fit` browser assets into
`assets/vendor/xterm/`. The Dioxus shell declares those local files with
`data-impulse-terminal-asset` tags, and the host contract resolves the same
relative paths instead of a CDN.

The visual smoke renders static Dioxus SSR fixtures for each `DesktopView`, then
opens them in headless Chromium to assert non-blank layout, no shell overlap, no
viewport overflow, route-specific visible content, local xterm globals, and no
remote font or terminal asset URLs.

The host-readiness smoke exercises the live Dioxus host contract without claiming
that a packaged release artifact exists. It opens a local browser fixture, loads
the same vendored xterm assets, stubs either the Dioxus-native
`window.__IMPULSE_DESKTOP_HOST` adapter or the legacy Tauri-shaped
`invoke`/`listen` adapter, evaluates the Rust-owned terminal interop script, and
asserts terminal input is serialized as `agent_write` bytes, resize emits
`agent_resize`, and terminal output and exit events reach the xterm buffer.

```bash
cd <repo>/impulse-rs/impulse-desktop
npm ci
npm run vendor:xterm
npm run visual:install
CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run visual:smoke
CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run host:smoke
CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run dioxus:host:smoke
CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run legacy:host:smoke
```

Screenshots are generated under `../../output/playwright/impulse-desktop-visual/`
plus `../../output/playwright/impulse-desktop-dioxus-host-smoke/` and
`../../output/playwright/impulse-desktop-host-smoke/`, all ignored by the
repository root `.gitignore`.
