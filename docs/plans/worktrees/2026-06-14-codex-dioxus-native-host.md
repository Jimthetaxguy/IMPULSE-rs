---
title: Codex Dioxus Native Host Pivot
description: Work card for codex-dioxus-native-host
updated: 2026-06-14
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, lane, dioxus, desktop, xterm, host]
---

# Codex Dioxus Native Host Pivot

## Lane Facts
- Owner: Codex
- Role: Pivot Impulse desktop host direction away from new Tauri scaffold work and toward Dioxus-native desktop hosting.
- Branch: agent/codex-dioxus-host-goal-cleanup, based on origin/main after PR #9.
- Worktree: <repo>
- Owned paths:
  - impulse-rs/impulse-desktop/src/ui.rs
  - impulse-rs/impulse-desktop/scripts/
  - impulse-rs/impulse-desktop/package.json
  - impulse-rs/impulse-desktop/tests/desktop_contract.rs
  - impulse-rs/impulse-desktop/README.md
  - _working-files/20260613-impulse-interface-dioxus-roadmap-spec.html
  - _working-files/20260613-codex-phase-ab-live-bridge-workspaces.md
- Blocked/shared paths:
  - Cargo.lock unless Dioxus desktop dependency changes become unavoidable
  - Any egui/impulse-gui path
- Plan/spec: User decision on 2026-06-14: "I don't know that we still want any Tauri now that we are shifting to full dioxus" and "Let's work on that as our next goal."
- Verification:
  - cargo fmt --all -- --check
  - node --check scripts/vendor_xterm_assets.mjs scripts/visual_smoke.mjs scripts/host_readiness_smoke.mjs
  - CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run dioxus:host:smoke
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target npm run legacy:host:smoke
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop
  - CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --workspace
  - git diff --check
- Latest status: complete for the bounded Dioxus-native host adapter pivot; real Dioxus Desktop launch scaffold remains the next frontier

## Decisions
- 2026-06-14: Do not proceed to `src-tauri` packaging scaffold as the next goal.
- 2026-06-14: Treat Dioxus Desktop as the target host direction. Tauri remains a compatibility adapter while code migrates.
- 2026-06-14: Introduce `window.__IMPULSE_DESKTOP_HOST` as the Dioxus-native JS host adapter shape before deleting existing Tauri-oriented code.

## Changes
- `TERMINAL_INTEROP_SCRIPT` and `DESKTOP_EVENT_BRIDGE_SCRIPT` now prefer `window.__IMPULSE_DESKTOP_HOST` before falling back to `window.__TAURI__`.
- `scripts/host_readiness_smoke.mjs` accepts `dioxus` and `legacy-tauri` host modes and asserts `data-impulse-host-kind`.
- `package.json` now makes `npm run host:smoke` default to the Dioxus-native adapter and exposes `npm run legacy:host:smoke` for compatibility coverage.
- Roadmap and README now frame the next frontier as a Dioxus Desktop launch scaffold.

## Cleanup Continuation
- 2026-06-14: Active docs and metadata now route the desktop goal to Dioxus Desktop; stale Tauri packaging and migration material is marked historical or compatibility-only.
- 2026-06-14: `host_commands.rs` no longer carries the misleading `let _ = state` keepalive in `mcp_invoke`; the command builds an owned `McpContext` and invokes the shared command body directly.
- 2026-06-14: `DESKTOP_EVENT_BRIDGE_SCRIPT` and `TERMINAL_INTEROP_SCRIPT` now share the same `resolveImpulseHostAdapter` JS helper through a Rust macro, so the Dioxus-native and legacy compatibility resolution rules cannot drift independently.
- 2026-06-14: Archived root Ralph plans 1-6 under `docs/archive/ralph-plans/`, added an archive index, updated docs navigation, and sanitized current-tree public-repo hygiene issues found by subagent scans.
- 2026-06-14: Replaced personal email/frontmatter values with GitHub noreply contact, replaced local absolute paths with synthetic placeholders, and replaced scanner-noisy `sk-*` test strings with impossible test placeholders.
- 2026-06-14: Tightened the canonical desktop host contract wording so `RUST-CANONICAL-CONTRACT.md` no longer describes the active terminal bridge as a Tauri IPC bridge.
- 2026-06-14: Added a feature-gated `desktop-app` binary target that launches the existing Dioxus `DesktopShell` through `dioxus::LaunchBuilder::desktop()`.
- 2026-06-14: Added a custom-head Dioxus host bootstrap that installs `window.__IMPULSE_DESKTOP_HOST` with a fail-visible manifest-only status until real command/event parity lands.
- 2026-06-14: Moved the Dioxus Desktop launch config and pending host bootstrap into a feature-gated `desktop_host` library module, leaving the binary as a thin launch composition.
- 2026-06-14: Centralized the Dioxus host kind/status as Rust constants and made the pending bootstrap mark `data-impulse-host-status` for runtime inspection.
- 2026-06-14: Added a typed Dioxus host manifest for invoke commands and event names so the pending bootstrap publishes the command/event surface the real adapter must implement.
- 2026-06-14: Added an `emit_dioxus_host_bootstrap` example and made the Dioxus host smoke execute the real Rust bootstrap, assert its manifest in Playwright, and reserve `legacy-tauri` as the only explicit compatibility mode.
- 2026-06-14: Converted the neutral terminal interop rerun smoke to `window.__IMPULSE_DESKTOP_HOST`; legacy `window.__TAURI__` coverage remains only in explicitly legacy-named tests.
- 2026-06-14: Cleaned prominent handbook, quickstart, benchmark, `impulse-gui`, Plan 6 lane, and reconciliation docs so they no longer describe Tauri/Tauri+Dioxus as the active native desktop path.
- 2026-06-14: Cleaned the historical code-review/DMG report so it no longer reads as active Tauri packaging guidance.
- 2026-06-14: Checked current Dioxus 0.6.3 docs for desktop bridging. `document::eval` supports Rust-to-JS execution plus `Eval::send`/`dioxus.recv()` message exchange inside component scripts; it does not provide a drop-in global Tauri-style `invoke` registry. The real Dioxus adapter should therefore be designed around a Dioxus-owned bridge task/eval lifecycle instead of copying Tauri IPC shape into `custom_head`.
- 2026-06-14: Renamed the pending Dioxus host bootstrap status to `manifest-only-pending-dioxus-eval-bridge` so tests and browser smoke clearly distinguish metadata publication from a real command/event adapter.
- 2026-06-14: Made the Dioxus host event manifest derive from `DesktopEvent::HOST_EVENT_NAMES`, reducing drift between runtime-emitted event names and browser-advertised listener names.
- 2026-06-14: Moved the Dioxus host invoke manifest into `host_commands.rs`, added named command constants, and made `desktop_host.rs` consume that command-surface contract instead of owning a separate string list.
- 2026-06-14: Renamed the host command-surface integration test from the legacy-oriented filename to `tests/host_surface.rs`, keeping compatibility-specific wording only for actual legacy adapter paths.
- 2026-06-14: Extracted shared inner bodies for runtime-only host commands so the legacy Tauri wrappers and host-neutral callers route through the same behavior functions.
- 2026-06-14: Cleaned active Rust protocol comments, the supervisor system prompt, the TUI module docs, and `AGENTS.md` ADR pointers so they describe operator/Dioxus surfaces instead of implying the retired egui or superseded Tauri stack is current.
- 2026-06-14: Marked `docs/DOC-PLAN.md` as historical and replaced old delete/remove guidance with archive-first public-doc hygiene guidance.
- 2026-06-14: Named the event-bridge degraded path with `markEventBridgeDegraded`, surfaced `data-impulse-ops-bridge-reason`, and added a Node-backed contract test proving the Dioxus host can mount in a degraded no-listen state without silently pretending live events are wired.
- 2026-06-14: Replaced the no-agent terminal fallback panes with an explicit empty state, so the shell no longer renders fake xterm mounts or fake agent IDs before a runtime-backed agent exists.
- 2026-06-14: Reconciled visual smoke coverage with the empty-default contract: SSR tests prove the default shell has no fake xterm mount, while seeded visual fixtures still require a live-agent xterm mount.
- 2026-06-14: Strengthened host readiness smoke so the Dioxus manifest-only bootstrap must fail closed for `invoke` and `listen` before the smoke overlays its test host adapter.
- 2026-06-14: Cleaned active docs found by subagent audits so egui/Ralph-loop guidance is clearly historical or legacy, removed scanner-shaped example strings, and neutralized personal-preference examples in research docs.
- 2026-06-14: Tightened the docs index archive row so Ralph plans 1-6 are labeled provenance-only rather than current roadmap material.
- 2026-06-14: Closed the token tracker compaction API cleanup TODO by replacing the long positional argument list with a typed `CompactionRecord`.
- 2026-06-14: Closed the terminal pane launch cleanup TODO by replacing long positional `TerminalPane::spawn` and `PaneManager::create_pane` calls with typed request structs.
- 2026-06-14: Closed retrieval store upsert cleanup TODOs by replacing long positional `upsert_history`/`upsert_genome` calls with typed `HistoryUpsert` and `GenomeUpsert` records.
- 2026-06-14: Closed memory search handler cleanup TODOs by replacing long positional `handle_search_history`/`handle_search_genome` calls with a shared `SearchMemoryOptions` record.
- 2026-06-14: Closed hook evidence capture cleanup by replacing the long positional `capture_hook_evidence` argument list with a named `HookEvidenceInput` request record.
- 2026-06-14: Closed supervisor artifact persistence cleanup by replacing the long positional `save_supervisor_artifact` helper with a typed `SupervisorArtifactInput`.
- 2026-06-14: Closed daemon dispatcher/connection cleanup by replacing remaining long-argument `process_request` and `handle_connection` plumbing with `ProcessRequestContext` and `ConnectionContext`.
- 2026-06-14: Cleaned imported interface design guidance and older active work cards so desktop implementation examples point to the Dioxus host adapter instead of active egui/Tauri wiring.
- 2026-06-14: Hardened the Dioxus desktop workspace registry so mutex poisoning no longer panics all later workspace commands; registry operations recover the inner map through one `lock_inner` helper.
- 2026-06-14: Hardened `DesktopRuntime` state access so terminal spawn/write/resize/focus/close/snapshot paths recover from mutex poisoning through one `lock_state` helper and return typed missing-session errors instead of internal panics.
- 2026-06-14: Hardened the in-process desktop MCP audit registry so tool invocation receipts, audit filtering, and audit clearing recover from mutex poisoning through one `lock_audit` helper.
- 2026-06-14: Hardened the exported in-memory terminal bridge support backend so open/write/resize/focus/close and test inspection helpers recover from mutex poisoning through one `lock_sessions` helper.
- 2026-06-14: Cleaned active public-repo metadata and docs hygiene issues found by a read-only subagent scan: neutralized personal author/decider/package metadata, removed private local research/archive paths, clarified Dioxus Desktop status, and reframed stale Ralph-era roadmap/process wording as historical.
- 2026-06-14: Made the retired `impulse-gui` boundary explicit by excluding it from the active workspace and giving the frozen crate its own standalone workspace, so active workspace checks and direct frozen-crate checks are both well-defined.
- 2026-06-14: Hardened the Dioxus host bootstrap manifest emitter to serialize invoke/event name arrays through `serde_json::json!` instead of hand-built JavaScript strings, with an escaping regression test and no new fallible source helper.
- Remaining cleanup queue:
  - Design and implement a real Dioxus-owned command/event bridge task using Dioxus eval/message semantics, then retire the pending custom-head adapter.
  - Reduce repeated `legacy-tauri-runtime` wrapper boilerplate only after the Dioxus Desktop launch scaffold defines the final host command shape.
  - Decide whether to rewrite public Git history for the history-only real-looking OpenAI placeholder prefix found in removed `docs/AI build_complete_guide.md`; current-tree gitleaks is clean.

## Tests
- Passed: `cargo fmt --all -- --check`.
- Passed: `node --check scripts/vendor_xterm_assets.mjs && node --check scripts/visual_smoke.mjs && node --check scripts/host_readiness_smoke.mjs`.
- Passed: HTML embedded script syntax check.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run dioxus:host:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run host:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo build --workspace`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo clippy --workspace -- -D warnings`.
- Passed: `python3 docs/validate_docs.py --contract`.
- Passed: `python3 docs/validate_docs.py --all`.
- Passed: active-doc contradiction scan for stale Tauri-primary wording; remaining hits are compatibility-only, historical archive, or research context.
- Passed: `gitleaks detect --no-git --redact=20 --source . --config /dev/null`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs test_impulse_agent_api_key_masking`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs test_impulse_agent_resolve_from_config_api_mode`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs set_api_key_stores_and_clears`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo clippy -p impulse-desktop --features desktop-app --bin impulse-desktop -- -D warnings`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop test_dioxus_desktop_launch_binary_is_feature_gated`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --features desktop-app desktop_host`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --features desktop-app desktop_host` after adding the Dioxus host command/event manifest.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop host_invoke_manifest`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --test host_surface`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop --features desktop-app --example emit_dioxus_host_bootstrap`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target npm run dioxus:host:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target npm run legacy:host:smoke`.
- Passed: read-only subagent public-doc hygiene scan; no P0 current-doc secret leak found, P1/P2 docs hygiene findings patched.
- Passed: targeted active-doc scan for personal author metadata, private NullClaw research paths, local cleanup archive paths, and stale desktop-status phrasing after public metadata cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-rs -p impulse-desktop -p impulse-term -p impulse-ops`.
- Passed with existing warnings: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check --manifest-path impulse-gui/Cargo.toml`.
- Passed: `cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name'` reports only the active workspace packages: `impulse-rs`, `impulse-ops`, `impulse-desktop`, and `impulse-term`.
- Passed: `cargo fmt --all -- --check`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --features desktop-app desktop_host`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop`.
- Passed: targeted scan showing `desktop_host.rs` has no source-level `expect`, `unwrap`, `panic!`, `todo!`, or `unimplemented!` after the manifest serialization hardening.
- Passed: `python3 docs/validate_docs.py --all && git diff --check` after the archive-index cleanup.
- Passed: targeted scan for active index wording that labels archived Ralph plans as current roadmap material.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs token_tracker::algorithm::tests::test_full_workflow`.
- Passed: targeted scan showing the token tracker and terminal pane long-parameter refactor markers are gone.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs` after the terminal pane request-struct cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs retrieval::store`.
- Passed: targeted scan showing retrieval store long-parameter refactor markers are gone.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-rs` after the memory search handler request-struct cleanup.
- Passed: targeted scan showing memory handler long-parameter refactor markers are gone.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-rs` after the hook evidence request-struct cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs test_capture_hook_evidence`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs --test hook_validation_session_start`.
- Passed: targeted scan showing hook evidence long-parameter suppression markers are gone from `handlers/common.rs`, `handlers/session.rs`, and `handlers/daemon_dispatch.rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-rs` after the supervisor artifact request-struct cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs daemon::tests`.
- Passed: targeted scan showing all `TODO(refactor): extract params into struct` and `#[allow(clippy::too_many_arguments)]` markers are gone from active Rust/Dioxus cleanup scope.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs` after daemon dispatcher/connection context cleanup.
- Passed: targeted active-doc scan for stale design/import phrases (`egui-native`, `GUI · egui`, `current Tauri desktop roadmap markers`, `Tauri emits ops_update`, `Fonts in Tauri`, and the old degraded xterm/Tauri message); only intentional legacy-path wording remains.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop` after workspace registry poison-recovery cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop workspace`.
- Passed: targeted scan showing `workspace registry poisoned` panic strings are gone from `impulse-desktop/src/workspace.rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop` after runtime state poison-recovery cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --test runtime`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop`.
- Passed: targeted scan showing `desktop runtime mutex poisoned` and `agent existence checked above` panic strings are gone from `impulse-desktop/src/runtime.rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop` after MCP audit poison-recovery cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop mcp`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop` after MCP audit poison-recovery cleanup.
- Passed: targeted scan showing `mcp audit mutex poisoned` panic strings are gone from `impulse-desktop/src/mcp.rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop` after in-memory terminal bridge poison-recovery cleanup.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop test_terminal_bridge_routes_open_write_resize_focus_close`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop` after in-memory terminal bridge poison-recovery cleanup.
- Passed: targeted scan showing source-level `expect("... mutex poisoned")` panic strings are gone from `impulse-desktop/src`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop test_terminal_interop_rerun_mounts_new_panes_without_duplicate_listeners`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop test_desktop_event_bridge_script_executes_against_mocked_legacy_host_webview`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo check -p impulse-desktop --features legacy-tauri-runtime --locked`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --test host_surface`.
- Passed: `git diff --check`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-rs`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop`.
- Passed: `cargo fmt --all -- --check`.
- Passed: `python3 docs/validate_docs.py --all`.
- Passed: `gitleaks detect --no-git --redact=20 --source . --config /dev/null`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --test desktop_contract test_desktop_event_bridge`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target cargo test -p impulse-desktop --test desktop_contract`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-visual-target npm run visual:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target npm run dioxus:host:smoke`.
- Passed: `CARGO_TARGET_DIR=/tmp/impulse-codex-target npm run legacy:host:smoke`.

## Handoff Notes
- Next frontier after this lane is a real Dioxus Desktop binary/launch scaffold, not a Tauri scaffold.

## Next Cleanup Queue

| Priority | Item | Evidence | Next action |
| --- | --- | --- | --- |
| Shark | Dioxus Desktop command/event parity is not wired into the real desktop launcher yet | `desktop-app` launches `DesktopShell` and installs a manifest-only `window.__IMPULSE_DESKTOP_HOST`; host smoke still stubs working invoke/listen in Playwright | Replace the pending host bootstrap with a real Dioxus eval/message bridge that routes to `DesktopRuntime` |
| Shark | Host wrappers are repetitive and still depend on Tauri types under the compatibility feature | `src/host_commands.rs` has parallel legacy/non-legacy wrapper surfaces | After Dioxus launch scaffold lands, collapse command bodies behind one host-neutral adapter trait and keep legacy wrappers thin |
| Bear | Historical docs still contain Tauri-era references by design | Archive docs and research notes retain provenance | Leave archived references labeled historical; only patch if they appear in active navigation or validator-required contract files |
| Bear | Public Git history has a removed real-looking placeholder prefix | Current-tree gitleaks is clean; history-only scan found the removed file | Decide separately whether to rewrite history; do not mix that with feature cleanup |
| Bear | PDF metadata still contains personal author metadata | `docs/ai_native_dev_guide.pdf` metadata reports a personal author value; no metadata-safe PDF tool is installed in this environment | Strip or regenerate PDF metadata before public release with `qpdf`, `exiftool`, Ghostscript, or the original document pipeline |
| Bear | Root branch has multiple untracked work cards and generated review artifacts | `git status --short` shows untracked `_working-files/**` and new docs/scripts | Before PR, stage by filename and decide which work cards are public-facing versus local-only |
