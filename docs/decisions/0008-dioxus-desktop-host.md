---
title: "ADR-0008: Dioxus Desktop Host"
status: accepted
created: 2026-06-14
deciders: [Impulse Maintainers]
---

# ADR-0008: Dioxus Desktop Host

## Status

Accepted. Supersedes [`0007-desktop-shell-stack.md`](0007-desktop-shell-stack.md).

## Context

Impulse's desktop shell has moved from a Tauri+Dioxus migration target toward a full Dioxus Desktop host. The product goal is still a polished, terminal-centric agent harness: PTY-backed coding-agent panes, workspace-aware launch controls, memory/review/artifact panels, and Rust-native MCP-like tools.

The previous Tauri decision solved the native-shell problem but kept the architecture mentally centered on Tauri IPC. The current implementation now has a Dioxus-owned shell, view router, xterm.js asset path, host-readiness smoke, and a Dioxus-native `window.__IMPULSE_DESKTOP_HOST` adapter seam.

## Decision

Adopt **Dioxus Desktop + xterm.js terminal bridge** as the active desktop host target.

- **Dioxus Desktop** is the target native host and product shell.
- **Dioxus** owns layout, signals, panels, route state, and interactive controls.
- **xterm.js** remains the terminal renderer for PTY-backed agent panes.
- **Rust runtime crates** (`DesktopRuntime`, `impulse-term`, `impulse-ops`, built-in MCP tools) keep PTY, workspace, review, and persistence authority.
- **Tauri-shaped code** is legacy compatibility only. It must not be used as the next scaffold target.
- **The Dioxus host adapter seam** must publish its invoke-command and event-name manifest so command/event parity work is explicit and testable.

## Consequences

- The default host smoke must exercise the Dioxus-native adapter path.
- Compatibility coverage may keep a legacy Tauri-shaped bridge until Dioxus Desktop has command/event parity.
- New roadmap work must target a Dioxus Desktop launch scaffold, not `src-tauri`.
- Public modules and tests should use host-oriented names (`host_commands`, host smoke, host events) unless specifically testing the legacy adapter.
- Removing the legacy adapter is gated on Dioxus Desktop launch, terminal open/write/resize/output/exit coverage, workspace launch coverage, and review-action coverage.

## Validation

This decision is validated when:

1. `npm run host:smoke` defaults to the Dioxus-native host adapter.
2. `npm run legacy:host:smoke` keeps the legacy adapter explicit while it exists.
3. `cargo test -p impulse-desktop` passes with host-oriented module names.
4. Active roadmap/spec docs describe Dioxus Desktop as the target host and Tauri as compatibility only.
5. The Dioxus host bootstrap exposes host kind/status plus supported invoke/event metadata.
6. A follow-up Dioxus Desktop binary/launch scaffold loads the same shell and xterm assets.

## Related Documents

- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
- `docs/spec/RUST-CANONICAL-CONTRACT.md`
- `docs/plans/worktrees/2026-06-14-codex-dioxus-native-host.md`
