---
title: Tauri + Dioxus Migration Handoff
status: active
version: 1.0.0
created: 2026-04-15
updated: 2026-04-15
---

# Tauri + Dioxus Migration Handoff

This document is the decision-complete build sequence for the desktop shell migration. Documentation cleanup (Phase 0) must be complete before any implementation phase begins.

See `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` for layer boundaries and `docs/decisions/0007-desktop-shell-stack.md` for the ADR.

---

## Phase 0 - Documentation Contract Reset (CURRENT PHASE)

**Exit criteria:** All docs describe Tauri+Dioxus as the desktop contract. egui is explicitly legacy. `validate_docs.py --contract` passes.

**Checklist:**
- [x] `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md`
- [x] `docs/spec/DESKTOP-STACK-TRADEOFFS.md`
- [x] `docs/decisions/0007-desktop-shell-stack.md`
- [x] `docs/plans/TAURI-DIOXUS-MIGRATION-HANDOFF.md` (this document)
- [x] `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`
- [ ] Update `docs/spec/RUST-CANONICAL-CONTRACT.md` - mark egui as legacy, Tauri+Dioxus as desktop contract
- [ ] Update `docs/ROADMAP-PLAN.md` - replace EGUI workbench phases with desktop shell phases
- [ ] Update `docs/plans/IMPLEMENTATION-HANDOFF.md` - add desktop migration reference
- [ ] Update `docs/INDEX.md` - add new spec files, mark impulse-gui as legacy
- [ ] Update `docs/SUMMARY.md` / `docs/SUMMARY.yaml`
- [ ] Update `AGENTS.md` - remove egui as active target, add desktop shell context
- [ ] Update `CLAUDE.md` - update product description and active stack
- [ ] Update `impulse-rs/docs/IMPULSE_TERM_STATUS.md` - egui deprecation status
- [ ] Update `impulse-rs/README.md` - workspace crate descriptions
- [ ] Update `impulse-rs/impulse-gui/README.md` - mark as legacy/freeze
- [ ] `python3 docs/validate_docs.py --contract`

---

## Phase 1 - Backend / UI Boundary Cleanup

**Entry criteria:** Phase 0 complete.

**Goal:** Confirm `impulse-term` core is fully framework-neutral before any Tauri wiring begins.

**Steps:**
1. Remove `eframe = "0.31"` from `impulse-rs/impulse-term/Cargo.toml`
2. Run `cargo check --manifest-path impulse-rs/Cargo.toml --workspace`
   - Clean: backend is already framework-free. Proceed.
   - Failures: the failing files have egui imports. Move them to a `legacy` module or delete.
3. Verify `backend.rs`, `context.rs`, and `WriteQueue` compile without egui
4. Move or mark egui-specific files (`renderer.rs`, `panel.rs`, `status_bar.rs`) as deprecated
5. Confirm ratatui path in `impulse-rs/src/` still compiles and runs

**Files that must NOT change in this phase:**
- `backend.rs`, `context.rs` - already clean
- Any `impulse-ops` files
- Daemon IPC contracts

**Exit criteria:**
- `cargo check`, `cargo test`, `cargo clippy` all pass
- Standalone ratatui operator surface still launches

---

## Phase 2 - Static Desktop Shell Skeleton

**Entry criteria:** Phase 1 complete.

**Goal:** Stand up a real Tauri + Dioxus app with static chrome. No live PTY. No daemon.

**Steps:**
1. Create `impulse-rs/impulse-desktop/` as a new workspace member
   - `impulse-rs/impulse-desktop/src-tauri/` - Tauri backend
   - `impulse-rs/impulse-desktop/src/` - Dioxus frontend
2. Add `impulse-desktop` to workspace `Cargo.toml`
3. Implement static five-panel layout in Dioxus `rsx!`:
   - Left rail: session list placeholders
   - Top bar: daemon status placeholder
   - Center: two placeholder terminal pane divs with `id="terminal-{n}"`
   - Right inspector: context/artifact/supervisor placeholder panels
   - Bottom strip: event log placeholder
4. Verify layout renders on macOS with `cargo tauri dev`

**What is NOT in this phase:** No live PTY, no xterm.js, no daemon connection, no Tauri commands.

**Exit criteria:**
- `cargo tauri dev` launches on macOS
- Five-panel layout renders correctly
- No egui or ratatui imports in `impulse-desktop`
- Benchmark B3 (static shell) captured per `docs/guides/DESKTOP-BENCHMARK-METHODOLOGY.md`

---

## Phase 3 - Live Terminal Bridge

**Entry criteria:** Phase 2 complete.

**Goal:** Wire real PTY sessions through Tauri IPC to xterm.js panes.

### Backend (src-tauri)
1. Add `impulse-term` as a dependency
2. Implement Tauri command handlers:
   - `terminal_open` -> `TerminalBackend::spawn()` -> store in session registry
   - `terminal_write` -> look up session -> `WriteQueue::write_user_input(data)`
   - `terminal_resize` -> look up session -> `TerminalBackend::resize(cols, rows)`
   - `terminal_close` -> look up session -> `TerminalBackend::kill()` -> remove
   - `terminal_focus` -> update focus state
3. Spawn reader task per session; emit `terminal_output` Tauri events with raw bytes
4. Emit `terminal_exit` when `is_alive()` returns false

### Frontend (Dioxus)
5. In each terminal pane component, call `eval()` on mount:
   - `new Terminal({...})` -> `term.open(document.getElementById('terminal-{session_id}'))`
   - Subscribe to `terminal_output` events, filter by session_id, call `term.write(data)`
   - Wire `term.onData` to send `terminal_write` command
   - Wire `ResizeObserver` on pane container to send `terminal_resize` command
6. Wire session open/close buttons to commands

**Exit criteria:**
- PTY sessions open, receive input, stream output to xterm.js
- Resize propagates through the full chain
- Benchmarks B4a (2 panes) and B4b (4 panes) captured and thresholds met

---

## Phase 4 - Daemon Integration and Parity

**Entry criteria:** Phase 3 complete.

**Goal:** Connect to live daemon. Reach feature parity. Deprecate impulse-gui.

**Steps:**
1. Connect `src-tauri` backend to daemon via `GetOpsSnapshot`, `SubscribeOps`
2. Forward `ProjectOpsSnapshot` updates as `ops_update` Tauri events
3. Wire Dioxus side panels to render from `ops_update` snapshots
4. Implement session switching in left rail
5. Implement daemon reconnect - shell state must restore cleanly without restart
6. Once parity confirmed: freeze `impulse-gui`, remove from active roadmap

**Parity checklist:**
- [ ] Terminal tabs/panes open and close
- [ ] Session switching works
- [ ] Daemon connection state visible in top bar
- [ ] Context panel updates from daemon snapshots
- [ ] Artifact panel updates from daemon snapshots
- [ ] Supervisor/session control flows work
- [ ] Daemon reconnect restores shell state
- [ ] Copy/paste works in terminal panes
- [ ] Keyboard shortcuts work (Ctrl+C, Ctrl+D, arrow keys, tmux sequences)
- [ ] Pane resize works end-to-end

---

## Integration Test Scenarios

| Scenario | How to Verify |
|---|---|
| Open, close, resize, focus multiple PTY panes | Manual + automated PTY smoke test |
| Terminal output streams live across tab/split changes | Observe output continuity during tab switch |
| Context/artifact/supervisor panels update from daemon only | Disconnect daemon; confirm panels freeze, not invent data |
| Daemon reconnect restores shell state | Kill daemon, restart; confirm reconnect and panel update |
| Standalone ratatui surface still works | Run `impulse-rs` CLI; verify TUI launches and is functional |

---

## Rust Verification Commands

```bash
cargo check --manifest-path impulse-rs/Cargo.toml --workspace
cargo test --manifest-path impulse-rs/Cargo.toml --workspace
cargo clippy --manifest-path impulse-rs/Cargo.toml --workspace --all-targets -- -D warnings
```

Doc validation after Phase 0:

```bash
python3 docs/validate_docs.py --contract
```
