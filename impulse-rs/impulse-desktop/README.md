# impulse-desktop

Active Tauri + Dioxus desktop shell contract for Impulse.

This crate is the active desktop shell path for the Tauri + Dioxus migration.
It is not the retired `impulse-gui`/egui workbench path. Dioxus owns the product
interface, Tauri owns the native shell/IPC boundary, Rust owns
PTY/session/daemon state, xterm.js owns terminal rendering, and macOS-native
islands stay behind typed request/result DTOs.

The product model is a terminal-agent harness: each coding agent is a PTY-backed
actor with an explicit workspace target, a runtime snapshot, and a visible set of
Rust MCP tools that can help the agent act inside that workspace. Dioxus renders
and requests those capabilities; Rust validates and executes them.

## Ownership

| Layer | Owner |
| --- | --- |
| Layout, rails, inspectors, command palette, review/apply surfaces | Dioxus |
| Native window/process/IPC boundary | Tauri |
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

The active path is:

```text
Dioxus controls -> Tauri command -> DesktopRuntime -> impulse-term TerminalBackend
        ^                                                    |
        |                                                    v
        +----- terminal_output / agent_runtime_update events +
```

## Features

- `native-macos` enables the Objective-C/AppKit compatibility bridge via
  `objc2`.
- `tauri-runtime` enables Tauri command annotations without making Tauri a hard
  dependency of the default workspace check.
