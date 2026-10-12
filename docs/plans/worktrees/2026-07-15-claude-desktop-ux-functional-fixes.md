# Lane: claude-desktop-ux-functional-fixes — fix dead host bridge, add folder picker, fix layout clipping

- **Owner:** Claude (Sonnet 5)
- **Role:** sole owner, no companion lane
- **Branch / worktree:** `claude/desktop-ux-functional-fixes` @ `.worktrees/desktop-ux-functional-fixes` (off `origin/main` @ `b7a42bd`)
- **Verification:** `cd impulse-rs && cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`, plus a manual run of the packaged `impulse-desktop` binary exercising register → launch → terminal end to end.

## Goal & user-visible outcome

James reported the Impulse Desktop app (Dioxus, `impulse-desktop` crate) "doesn't work well": folder registration and terminal launch don't function, on top of visual layout bugs. After this lane:

- Registering a workspace folder actually populates the workspace list and unblocks agent launch, even if the live JS↔Rust bridge finishes installing after the UI's first render.
- A native OS folder-picker button lets the user browse to a folder instead of typing an absolute path from memory.
- The bottom status bar (`event-strip`) no longer silently truncates its rightmost items — it wraps or ellipsizes instead.
- The 3-column shell (`workspace-grid`) no longer clips content below ~972px window width — it reflows/scrolls instead of losing content with no scrollbar.

## Root cause (diagnosed via source trace; not yet reproduced by running the packaged binary)

Adapter resolution in `ui.rs` (`resolveImpulseHostAdapter()`, ~L42-72, invoked from a `use_effect` around L2691) reads `window.__IMPULSE_DESKTOP_HOST` once, with no retry, to decide whether to trust it or fall back to a dead Tauri stub (`window.__TAURI__`, never set in the Dioxus binary). Separately, `use_live_host_bridge()` (`host_bridge.rs` L343-412, mounted from `LiveDesktopApp` L418-424) asynchronously installs the real host on that same global via its own independent `document::eval`. Dioxus gives no ordering guarantee between two independently-dispatched `eval` effects. If resolution wins the race, `window.__impulseOpsBridge`'s `invoke`/`listen` are permanently bound to `undefined`, and every host command (`register_workspace`, `list_workspaces`, `agent_platforms`, `agent_spawn`, `terminal_open`, …) fails closed with no recovery for the lifetime of that window. This single root cause plausibly explains the "everything reads as zero and nothing works" state in the reported screenshot (0 agents online, 0 workspaces registered, no terminal session, register-folder appears to do nothing).

**This is the load-bearing assumption of the lane.** Acceptance criterion 5 below is the ground-truth check; if it turns out the race does not reproduce on a clean `origin/main` checkout, stop and re-diagnose (see Rollback notes) before shipping speculative resilience code.

Two secondary, source-confirmed issues (independent of the race):

- No native OS folder-picker — "Folder path" (`ui.rs` ~L1028-1036) is a plain `<input type="text">`; the user must type an absolute path from memory.
- `event-strip` (`assets/impulse_crt.css` L913-921) has no `flex-wrap`/`overflow-x`/`text-overflow`, and `.workspace-grid` (`assets/impulse_crt.css` L165-169) is a fixed `232px / minmax(420px,1fr) / 320px` grid with no width-based media query anywhere in the stylesheet (the only `@media` rule is `prefers-reduced-motion`) and `overflow: hidden` on `.impulse-shell` — so both silently clip rather than wrap/scroll.

The Launch button being disabled in the empty state is *not* a bug — `can_launch` (`ui.rs` L1010) correctly gates on required fields and `role_compatibility`, and the disabled state is visually distinct (`opacity: 0.45` + a "blocked" status readout, `ui.rs` L1154-1185). It's included here only because it's easy to mistake for a bug when the host-bridge race prevents the user from ever getting past the empty state.

## Owned paths

- `impulse-rs/impulse-desktop/src/ui.rs`
- `impulse-rs/impulse-desktop/src/host_bridge.rs`
- `impulse-rs/impulse-desktop/src/bridge.rs`
- `impulse-rs/impulse-desktop/src/host_commands.rs`
- `impulse-rs/impulse-desktop/assets/impulse_crt.css`
- `impulse-rs/impulse-desktop/Cargo.toml` (adding `rfd` as a crate-local dependency)
- `impulse-rs/impulse-desktop/scripts/visual_smoke.mjs` and/or its fixtures, if the layout fix needs a new/extended viewport assertion
- New/updated tests colocated in the above modules' `mod tests`

## Blocked / shared paths (do not touch)

- Workspace-root `Cargo.toml` / `Cargo.lock` — only the desktop crate's own `Cargo.toml` changes, and only to add `rfd`; check for lockfile conflicts with other active lanes before merging.
- `AGENTS.md`, `CLAUDE.md` — currently dirty on `codex/mode-taxonomy-cleanup` (another agent's in-progress work, observed via `git status --short` on the main checkout before opening this lane); this lane branched from `origin/main` and does not touch these files.
- Any file under other active `.worktrees/*` lanes (`governed-role-launch`, `governed-runtime-producers`, `governed-task-run`, `dioxus-egui-retirement`, `accepted-run-memory-candidates`, `agent-cache-serialization`, `agent-truth-parity`, `desktop-daemon-truth-wire`, `legacy-ui-retirement-plan`, `live-daemon-truth-integration`, `registry-platform-identity`) — none of them appear to touch `impulse-desktop/src/{ui,host_bridge,bridge,host_commands}.rs` or the CSS file based on their names; reconfirm with `git diff --stat origin/main...<their-branch>` before opening a PR if any land first.

## Non-goals (explicit — deferred to a future Phase 2 spec)

- No redesign of information architecture, copy/labels, or overall visual style (jargon-heavy labels like "Native Islands", the CRT theme, etc.)
- No fix for the redundant native-titlebar + custom in-app top-bar (cosmetic double chrome, not functionally broken — confirmed no pixel-level overlap with the agent tab strip)
- No changes to governed-agent-launch business logic (task/acceptance-criteria requirements, role-compatibility gating) beyond making the disabled state clearer, and only if trivial
- No new terminal features, no PTY/xterm.js protocol changes

## Public interfaces / docs that change

- New Cargo dependency: `rfd` (native file dialog) in `impulse-desktop/Cargo.toml`.
- New host-local affordance exposed from the Workspace Launcher folder-path field (exact mechanism — a new host command vs. a direct Dioxus `onclick` spawning a blocking native-dialog task — decided during implementation planning).
- CSS: new rules for `.event-strip` (wrap/overflow) and `.workspace-grid` (responsive floor) in `assets/impulse_crt.css`.
- No daemon IPC protocol changes anticipated (folder picker is host-local, no round trip through `IPC-PROTOCOL.md`'s endpoint groups); no persistence-format changes.

## Acceptance criteria

1. A test proves adapter resolution retries/re-subscribes rather than resolving exactly once, and that a late-installing live bridge (simulated) is still picked up. Exact test strategy (mock timing vs. structural refactor to a signal-driven effect) decided in the implementation plan.
2. Clicking a new "Browse…" control next to "Folder path" opens a native macOS folder-picker and populates the text field with the selected absolute path.
3. A test proves no `event-strip` child span causes horizontal overflow of `.impulse-shell` at a window width where combined content would previously have exceeded it (extend the existing Playwright visual-smoke suite, or add a targeted CSS/SSR test).
4. A test proves `.workspace-grid` content is reachable (scrolls) rather than clipped below the current ~972px floor — the existing visual-smoke suite's narrowest fixture (1024px) is above the floor and must be extended with a narrower one.
5. **Ground truth:** manual run of `cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop` — register a real folder, confirm it appears in the workspace list, launch a governed agent against it, confirm a terminal pane opens and accepts input.
6. Full verification gate green: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`.

## Rollback / handoff notes

- Single-owner lane, no handoff anticipated. If interrupted mid-lane, the status log below carries current state; the worktree persists at `.worktrees/desktop-ux-functional-fixes` until merged or abandoned.
- If acceptance criterion 5's manual run shows workspace registration already works cleanly on `origin/main` (i.e. the suspected race does not reproduce), stop before implementing the retry fix, update this doc's root-cause section with what was actually observed, and re-diagnose — do not ship speculative resilience code against an unconfirmed race.

## Status log (reverse-chronological)

- 2026-07-15 — Lane opened during brainstorming with James. Spec written after two research passes (repo/tech-stack identification, then root-cause trace of the folder-register and terminal-launch reports) confirmed `impulse-gui`/egui is legacy/frozen and the live app is the Dioxus `impulse-desktop` crate. Root cause for the functional bugs is a suspected (not yet reproduced) host-bridge resolution race; two independent CSS layout bugs (status-bar truncation, grid clipping below ~972px) were directly confirmed in source. Worktree created off `origin/main` @ `b7a42bd`. Implementation plan not yet written.
