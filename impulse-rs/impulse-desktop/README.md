# impulse-desktop

Active Dioxus desktop shell contract for Impulse.

This crate is the active desktop shell path for the Dioxus desktop migration.
It is not the retired `impulse-gui`/egui workbench path. Dioxus owns the product
interface and native desktop host direction, Rust owns PTY/session/daemon state,
xterm.js owns terminal rendering, and macOS-native islands stay behind typed
request/result DTOs. Legacy Tauri-shaped code remains a compatibility adapter
only while the host boundary migrates toward Dioxus-native desktop APIs.

The product model is a terminal-agent harness: each coding agent is a PTY-backed
actor with an explicit workspace target, a runtime snapshot, and a visible set of
Rust MCP tools that can help the agent act inside that workspace. Dioxus renders
and requests those capabilities; Rust validates and executes them.

## Ownership

| Layer | Owner |
| --- | --- |
| Layout, rails, inspectors, command palette, review/apply surfaces | Dioxus |
| Native window/process/IPC boundary | Dioxus desktop host adapters |
| PTY lifecycle, daemon snapshots, persistence | Rust backend |
| Terminal glyph rendering | xterm.js |
| Menu bar, panels, notifications, accessibility hooks | Native island bridge |
| Built-in MCP tools and connector status | Rust runtime snapshots surfaced to Dioxus |

Native islands must publish serializable DTOs back to Dioxus. They must not keep
independent copies of sessions, memory, terminal state, or artifacts.

## Runtime Contracts

- `WorkspaceTarget` names the folder an agent is allowed to operate in. The
  runtime derives it from `cwd` when the UI does not provide richer metadata.
- `AgentSpawnRequest` carries platform, command, workspace, terminal dimensions,
  and optional MCP tool descriptors.
- `AgentRuntimeSnapshot` is the source of truth for what Dioxus displays:
  focused state, process status, workspace, context health, output metrics, and
  built-in Rust MCP tools.
- Built-in MCP tools default to safe descriptors for `impulse.agent_spawn`,
  `impulse.agent_write`, `impulse.search_memory`, and
  `impulse.review_injection`; mutating terminal actions require confirmation.

The target path is:

```text
Dioxus controls -> Dioxus host adapter -> DesktopRuntime -> impulse-term TerminalBackend
        ^                                                    |
        |                                                    v
        +----- terminal_output / agent_runtime_update events +
```

The compatibility path still covered by tests is:

```text
Dioxus controls -> legacy Tauri-shaped adapter -> DesktopRuntime -> impulse-term TerminalBackend
```

## Features

- `desktop-app` enables the real Dioxus Desktop binary target:
  `cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop`.
  This is the launch scaffold for the target host path; command/event parity
  still lands behind `window.__IMPULSE_DESKTOP_HOST`. The current binary
  installs a fail-visible pending adapter so missing native command/event
  plumbing is explicit during launch work.
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
`data-impulse-terminal-asset` tags; the Dioxus desktop host should load those
same relative paths instead of a CDN.

The visual smoke renders static Dioxus SSR fixtures for each `DesktopView`, then
opens them in headless Chromium to assert non-blank layout, no shell overlap, no
viewport overflow, route-specific visible content, local xterm globals, and no
remote font or terminal asset URLs.

The host-readiness smoke is one step closer to the eventual Dioxus desktop host
without claiming a packaged app exists. It opens a local browser fixture, loads
the same vendored xterm assets, stubs either the Dioxus-native
`window.__IMPULSE_DESKTOP_HOST` adapter or the legacy Tauri-shaped
`invoke`/`listen` adapter, evaluates the Rust-owned terminal interop script, and
asserts terminal input is serialized as `agent_write` bytes, resize emits
`agent_resize`, and terminal output and exit events reach the xterm buffer.

```bash
cd <repo>/impulse-rs/impulse-desktop
npm install
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
