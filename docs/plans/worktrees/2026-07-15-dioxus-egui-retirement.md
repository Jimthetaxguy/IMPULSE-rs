---
title: Dioxus Parity and EGUI Retirement
description: Work card for the dioxus-egui-retirement implementation lane
updated: 2026-07-16
type: doc
category: planning
phase: all
status: review
audience: builders
tags: [worktree, lane, dioxus, egui, packaging, retirement]
---

# Dioxus Parity and EGUI Retirement

## Lane Facts
- Owner: Codex root
- Role: implementation and integration owner; audit subagents are read-only
- Branch: `codex/dioxus-egui-retirement`
- Worktree: `/Users/jamespustorino/code/IMPULSE-rs/.worktrees/dioxus-egui-retirement`
- Owned paths: Dioxus host/readiness and retained-shell tests; macOS packaging scripts/workflow;
  `impulse-desktop` resources; EGUI retirement plan; narrowly related Cargo/docs contracts
- Blocked/shared paths: concurrent root checkout and every existing worktree; the physical
  `impulse-gui` and EGUI-only module deletion remains blocked until recovery evidence is created
  and the required explicit mass-removal approval is confirmed
- Plan/spec: `docs/plans/EGUI-DECOMMISSION.md` after forward-port to current `main`
- Verification: focused desktop/host/package tests, macOS bundle smoke where available,
  no-placeholder SSR contract, Cargo feature/tree audit, workspace format/check/test/Clippy/build,
  docs validation, diff hygiene, and leak scan
- Latest status: the cockpit is realigned around one shared launch target, exact project-scoped
  oversight/evidence, desktop-wide workers, terminal work, and launch/review. The first confirmed
  governed launch atomically binds daemon, memory, telemetry, and task authority to one registered
  project; malformed or unconfirmed launches are audited before any scope mutation. Static and
  no-environment package verification pass, but the latest signed GUI-host launch aborts after
  daemon startup, so the historical complete lifecycle receipt is not current release proof.
  Default workspace resolution excludes EGUI. Physical legacy deletion is a later approved tranche,
  not a blocker on this draft PR.

## Goal and User-Visible Outcome
- Tagged macOS releases build and package the real Dioxus cockpit plus the `impulse-rs` companion.
- The packaged webview proves the live Dioxus eval bridge reached Rust before release acceptance.
- Local xterm assets are present in the application bundle.
- Retained product surfaces do not present placeholder or local-only action buttons.
- EGUI/eframe is subsequently removed from source and workspace resolution without regressing PTY,
  ratatui, CLI, daemon, Dioxus, or temporary Tauri compatibility behavior.

## Non-Goals
- Do not remove Tauri compatibility in the EGUI tranche.
- Do not rewrite historical ADRs or archived design research.
- Do not add speculative replacement controls.
- Do not mass-delete legacy source before the recovery/approval gate.

## Ordering and Gates
1. [x] Forward-port and refresh the decommission contract.
2. [x] Replace the stale EGUI macOS bundler with a Dioxus package and real readiness smoke.
3. [x] Remove known disabled and local-only Dioxus affordances; add regression contracts.
4. [ ] Create and verify the recovery artifact plus consumer/dependency evidence.
5. After explicit mass-removal approval, remove `impulse-gui` and EGUI-only `impulse-term`
   modules/features/dependencies in independently reviewable commits.
6. Align active docs/contracts and prove zero shipping EGUI/eframe references.

## Decisions
- 2026-07-15: The graphical product surface remains Dioxus Desktop + xterm.js; daemon/runtime
  contracts remain authoritative and ratatui remains first-class.
- 2026-07-15: A browser fixture smoke is insufficient release proof. The packaged app must emit
  readiness only after JavaScript installed the live eval bridge and Rust received its probe.
- 2026-07-15: Artifact envelopes remain readable, but action buttons are hidden until they dispatch
  an authoritative typed artifact operation and render its result.
- 2026-07-15: Release packaging and dead-affordance cleanup precede destructive legacy removal.
- 2026-07-16: The desktop home surface prioritizes one shared launch-target selection, explicit
  connected-daemon-project oversight, desktop-wide workers, terminal work, and launch/review
  actions; memory and telemetry remain inspectors rather than a decorative hero dashboard.
- 2026-07-16: `LoopDestroyed` remains the normal Dioxus event-loop shutdown authority. Ordered
  host-close uses the same idempotent coordinator, while a desktop-owned daemon also watches its
  exact direct parent so abrupt desktop loss triggers bounded drain, sync, and runtime-file cleanup.
  Existing operator-owned daemons remain untouched.
- 2026-07-16: Desktop startup accepts only an explicit standard project socket. Otherwise it starts
  disconnected and the first confirmed registered-workspace launch performs exact daemon identity
  attestation before committing the one-project boundary.
- 2026-07-16: Project activation and every filesystem use site reject symlinked state, runtime,
  lifecycle-outbox, PID, and lock leaves. Lifecycle-outbox flock waits are bounded and
  shutdown-aware so a retained cross-process lock cannot hang App Quit indefinitely.

## Changes
- Replaced the stale EGUI bundle input with a Dioxus `Impulse.app` containing `impulse-desktop`,
  the `impulse-rs` companion, local xterm assets, Dioxus-owned metadata/icon resources, signatures,
  verifier, developer-preview DMG path, and release workflow integration.
- Added a real packaged-app readiness/lifecycle receipt covering the eval bridge, xterm readiness,
  PTY open/resize/focus/write/output/exit/close, daemon ops, ordered host close, worker cleanup, and
  sidecar cleanup, plus real daemon tests for wrong-parent rejection and parent-death cleanup.
- Made `impulse-term` framework-neutral by default; only excluded `impulse-gui` explicitly requests
  the temporary `egui` feature.
- Removed known dead Dioxus controls and fake review shortcuts; added source/SSR contracts.
- Reworked the desktop visual hierarchy into a compact launch-target/oversight/worker/terminal
  cockpit with a separate launch dock, one lifted target authority, duplicate-launch gating,
  close confirmation, and project-scoped review-service status.
- Added a fail-closed dynamic project boundary controller, switchable memory/event/task adapters,
  daemon identity attestation, registered-workspace preflight, trusted child routing variables, and
  the same activation path for visible UI and audited MCP launches.
- Added descriptor-level no-follow and regular-file enforcement for project state leaves, daemon
  and cleanup locks, PID markers, and the governed lifecycle outbox; lock acquisition remains
  serialized but cannot block shutdown forever.
- Updated current product/architecture documentation while preserving the explicit live-versus-
  target boundary and the recovery/approval gate for physical EGUI removal.

## Tests
- `cargo test -p impulse-desktop --lib --locked` — 196 passed, including dynamic scope,
  confirmation-before-activation, daemon attestation, no-follow state handling, bounded outbox locks,
  late shutdown installation, exact-parent spawn arguments, attached-daemon safety, and idempotent
  owned-process cleanup.
- `cargo test -p impulse-desktop --locked --test desktop_contract` — 70 passed.
- `cargo test -p impulse-desktop --locked --test host_surface` — 8 passed.
- `cargo test -p impulse-desktop --locked --test runtime` — 22 passed, 1 ignored.
- `cargo test -p impulse-desktop --locked --test views_ssr` — 7 passed.
- `cargo test -p impulse-rs --test daemon_signal_shutdown --locked` — 5 passed, including real
  SIGTERM/SIGINT cleanup, wrong-parent pre-runtime rejection, and parent-death state sync plus
  socket/PID cleanup.
- `cargo test -p impulse-desktop --locked --test macos_packaging_contract` — 4 passed.
- `cargo check -p impulse-desktop --features desktop-app --bin impulse-desktop --locked` — passed.
- Historical `bash scripts/build-macos-app.sh --smoke --smoke-timeout 30` complete-lifecycle receipt:
  `impulse-rs/target/package-smoke/20260716T053025Z-54598/desktop.log`. This predates the settled
  project-boundary tree and is retained as historical evidence only.
- Fresh signed bundle structure/signature plus no-environment disconnected-scope proof passed from
  the settled tree at `impulse-rs/target/package-scope-probe/20260716T111630Z-96178`; the following
  live GUI-host launch exited with `Abort trap: 6` after companion startup at
  `impulse-rs/target/package-smoke/20260716T111631Z-96178/desktop.log`, so a fresh lifecycle receipt
  remains open.
- Developer-preview DMG signature/checksum/mount verification passed for
  `/tmp/impulse-dioxus-package-release/package/Impulse-0.1.0-macos-arm64-developer-preview.dmg`.
- `cargo test --workspace --locked` — passed, including 196 desktop library tests, 70 desktop
  contracts, 1596 root library tests (5 ignored), daemon signal/process integration, and docs tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `cargo build --workspace --locked` and `cargo fmt --all -- --check` — passed.
- `cargo build --workspace --no-default-features --locked`, Dioxus desktop feature check, and the
  legacy Tauri adapter library check passed. The CI rehearsal also corrected that compatibility job
  to check the mutually exclusive adapter library rather than compiling Dioxus-only test targets.
- `python3 docs/validate_docs.py --self-test` and `--all` — passed, 143/143 metadata-valid plus
  contract validation.
- Default `cargo tree --workspace --locked` excludes `egui`/`eframe`; isolated
  `cargo check --manifest-path impulse-gui/Cargo.toml --locked` passes with six legacy dead-code
  warnings.
- `git diff --check` and production bypass/stale-real-system scans passed; the only hard-coded user
  path hit is the explicit `/Users/example/...` workspace parser fixture.
- Independent Codex red-team verdict: SHIP with no remaining P0/P1. The full MiniMax tool-reading
  prompt timed out at its 240-second bound; a transport probe passed and a no-tool evidence gate
  returned `VERDICT: SHIP` for committing/pushing a draft PR.
- AppKit Quit with a live PTY, current signed-host lifecycle proof, and abrupt-desktop worker cleanup
  remain explicit follow-up package gates rather than hidden release claims.

## Handoff Notes
- PR #20 is merged as `b7a42bd`.
- The dirty root checkout and all older worktrees remain untouched.
