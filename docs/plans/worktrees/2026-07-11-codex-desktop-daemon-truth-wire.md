---
title: Dioxus desktop to daemon truth wire
description: Publish PTY lifecycle telemetry and render daemon-reconciled ops snapshots
updated: 2026-07-11
type: doc
category: planning
phase: all
status: complete
audience: builders
tags: [worktree, dioxus, daemon, telemetry, agent-manager]
---

# Dioxus desktop to daemon truth wire

## Lane facts

- Owner: Codex (`codex/desktop-daemon-truth-wire`)
- Worktree: `.worktrees/desktop-daemon-truth-wire`
- Original base: `main` at `c35309e`, stacked with truth-preserving overlay `351b820`.
- Live forward-port: `a5184e2` with equivalent commits `dbac22f` then `ae8fcd0` on `codex/live-daemon-truth-integration`.
- Goal: turn desktop PTY state into daemon telemetry input and feed the daemon's
  reconciled snapshot back to Dioxus as `ops_update`.
- Owned paths:
  - a new focused desktop daemon-ops adapter and its tests
  - `impulse-desktop/src/{lib.rs,runtime.rs,ui.rs,bin/impulse_desktop.rs}` only where needed
  - a real daemon-handler reconciliation regression in `impulse-rs/src/daemon/tests.rs`
  - a timestamp helper in `impulse-ops` if needed without adding dependencies
  - this card, this lane's working note, and narrow stable-vocabulary docs
- Blocked/shared paths: Claude's Ion/guardrail files, `CLAUDE.md`, Cargo manifests,
  and the currently active daemon-agent timeout/cache lane under `src/{agent,daemon,error,mcp}`.

## Ownership invariant

- Desktop owns PTY mechanics and emits runtime facts.
- Daemon owns `ProjectOpsSnapshot`, telemetry reconciliation, staleness, and event sequence.
- Dioxus renders daemon snapshots. A local runtime event may keep xterm responsive, but
  it must not overwrite a newer daemon `ops_update` with a shadow project snapshot.

## Acceptance criteria

- A pure conversion maps `AgentRuntimeSnapshot` to complete `AgentRuntime` telemetry,
  preserving status, role, target, workspace grouping, tools, context, and identity.
- A stable per-process source id and daemon-compatible RFC3339 timestamp form each
  `TerminalOpsReport`.
- Spawn, resize, focus, and exit changes publish immediately; a two-second heartbeat
  republishes while the desktop host lives, below the daemon's ten-second stale boundary.
- The desktop subscribes at startup and after publish cycles, retains `next_seq`, and
  emits only daemon-returned snapshots as `DesktopEvent::OpsUpdate`.
- Daemon unavailability is retryable and does not stop PTY output or host invocation.
- Subscription freshness remains distinct from telemetry-publish health: a fresh
  daemon snapshot stays current while desktop writes retry.
- Runtime lifecycle delivery is FIFO and reentrant, natural exits reap records,
  stale output cannot cross an exit marker, and an agent id cannot be reused
  without a future explicit incarnation token.
- A Unix-socket client test proves publish then subscribe framing and the resulting
  `ops_update`; a separate real-handler regression proves daemon reconciliation,
  while unit tests prove conversion and lifecycle removal.
- Claude-owned dirty paths and Cargo manifests remain untouched.

## Verification contract

- Focused daemon-ops adapter and desktop runtime tests.
- `cargo test -p impulse-desktop` and desktop contract tests.
- `cargo check --workspace`.
- `cargo clippy --workspace -- -D warnings`.
- `cargo fmt --all -- --check`.
- Workspace tests, isolating only documented pre-existing/environmental failures.
- MiniMax plus independent generic-agent adversarial review before commit.

## Delivered

- Added a production `daemon_ops` adapter with one managed worker, bounded
  latest-state wakeups, startup/heartbeat/lifecycle publish-subscribe cycles,
  opaque daemon cursor carry-forward, two-second I/O timeouts, and same-source
  empty shutdown publication.
- Preserved the daemon as the only owner of `ProjectOpsSnapshot`; local runtime
  messages update only the terminal pool.
- Added explicit read freshness and publish-degradation UI states so retained
  snapshots are never silently presented as live after subscribe failure.
- Hardened PTY lifecycle ordering with a reentrant FIFO, weak callback ownership,
  runtime-drop cleanup, natural-exit reaping, one-use routing ids, focus
  exclusivity, and project-boundary filtering for absolute, relative, and
  inherited working directories.
- Kept Claude's active Ion implementation paths and Cargo manifests untouched.

## Adversarial review loop

- MiniMax M2.7/M3 sandboxed passes challenged shutdown ordering, coalesced
  wakeups, cursor handling, and lifecycle ownership. Their concrete concerns
  were converted into deterministic tests or rejected against the shared
  latest-state invariant; the provider exhausted its final-answer budget on
  the longest passes, so MiniMax was used as adversarial input rather than the
  sole approval signal.
- Independent code and test reviewers found project bleed, immediate-exit
  resurrection, stale focus, conflated read/write health, vacuous recovery
  assertions, id reuse, unreaped processes, reentrant deadlock, callback
  retention, and post-exit output races. Each was fixed and re-reviewed.
- Final independent code and test reviews both returned `NO_FINDINGS`.

## Verification evidence

- PASS: `cargo check --workspace`.
- PASS: `cargo clippy --workspace -- -D warnings`.
- PASS: `cargo fmt --all -- --check`.
- PASS: `cargo test --workspace -- --skip test_reconciled_clean_archive_has_contracts_snapshot`.
  All runnable workspace tests passed, including 106 desktop library tests, 46
  desktop contracts, 8 host-surface tests, 16 desktop-runtime tests, and 6
  view tests.
- PASS: focused real handler publish-then-subscribe reconciliation test.
- PASS: `cargo check -p impulse-desktop --features desktop-app` and desktop
  all-target clippy.
- PASS: `npm run dioxus:host:smoke`; the browser path exercised `agent_write`,
  `agent_resize`, `list_workspaces`, `agent_snapshot`, and `register_workspace`.
- PASS: `git diff --check`.
- KNOWN EXTERNAL: the unskipped archive proof requires a gitignored fixture that
  exists in the canonical checkout but not isolated worktrees.
- KNOWN PRE-EXISTING: `docs/validate_docs.py --contract` reports 17 documents
  older than the 120-day threshold; `--all` additionally reports the unrelated
  `research/2026-06-30-sites-map-phase1-spec.md` status. Both new lane cards
  pass metadata validation.

## Integration handoff

- Original branch: `codex/desktop-daemon-truth-wire`, stacked on `351b820`.
- Live forward-port branch: `codex/live-daemon-truth-integration`, preserving the
  dependency order as `dbac22f` then `ae8fcd0` on top of `a5184e2`.
- The forward-port is conflict-free and fully verified. Do not merge while Claude's
  daemon-agent cache lane is dirty: its current take/check-in design can initialize
  concurrent agents and overwrite one request's session history. Rebase onto the
  final concurrency-safe clean tip, rerun the same gates, then integrate both commits.
- ST-09 is now backed compositionally by Unix JSONL client framing, a separate real
  daemon-handler reconciliation test, lifecycle/reducer tests, desktop contracts, and
  browser-host readiness smoke. A packaged desktop-to-real-daemon E2E remains open.
  ST-13 remains planned because this adapter intentionally binds one daemon project;
  multi-workspace routing and daemon-approved control remain prerequisites for a
  first-class manager claim.
- The next platform-management slice is the draft
  `2026-07-11-codex-ion-managed-platform.md`: move platform identity to the
  runtime registry, use token-safe command detection, and only then expose Ion
  through generic launch/manager surfaces.
