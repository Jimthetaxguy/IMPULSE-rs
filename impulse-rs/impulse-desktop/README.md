# impulse-desktop

Active Tauri + Dioxus desktop shell contract for Impulse.

This crate is the active desktop shell path for the Tauri + Dioxus migration.
It is not the retired `impulse-gui`/egui workbench path. Dioxus owns the product
interface, Tauri owns the native shell/IPC boundary, Rust owns
PTY/session/daemon state, xterm.js owns terminal rendering, and macOS-native
islands stay behind typed request/result DTOs.

## Ownership

| Layer | Owner |
| --- | --- |
| Layout, rails, inspectors, command palette, review/apply surfaces | Dioxus |
| Native window/process/IPC boundary | Tauri |
| PTY lifecycle, daemon snapshots, persistence | Rust backend |
| Terminal glyph rendering | xterm.js |
| Menu bar, panels, notifications, accessibility hooks | Native island bridge |

Native islands must publish serializable DTOs back to Dioxus. They must not keep
independent copies of sessions, memory, terminal state, or artifacts.

## Features

- `native-macos` enables the Objective-C/AppKit compatibility bridge via
  `objc2`.
- `tauri-runtime` enables Tauri command annotations without making Tauri a hard
  dependency of the default workspace check.
