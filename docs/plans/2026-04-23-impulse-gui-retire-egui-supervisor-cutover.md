# Plan 2 — Retire egui & cut over to the Dioxus supervisor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `impulse-supervisor` (Dioxus, retained-mode) the only desktop UI for Impulse. Permanently delete `impulse-gui` (egui workbench) and `impulse-term` (egui adapter). The supervisor must reach feature parity for the workflows the user actually uses *before* the egui crates are archived; the parts of `impulse-gui` that are aspirational/unused are deleted, not ported.

**Architecture:** `impulse-supervisor` already has the foundational pieces from Phase 7 (L155-L179): Dioxus 0.6 launch, daemon bridge, panes layout, the `impulse-term-dioxus` renderer, BlockModel, BlockListView, OSC 133 wiring. What's missing is (a) supervisor-privileged spawn, (b) `BlockListView` actually connected to worker panes, (c) the workbench views the user relies on (sessions, memory, settings, etc.) ported from egui to Dioxus.

**Tech stack:** Rust 2021, Dioxus 0.6 (system webview via wry — WKWebView/WebView2/WebKitGTK, no Electron), portable-pty, vt100 (NOT alacritty_terminal — see L178 audit), parking_lot::FairMutex, thiserror, anyhow, tokio.

---

## Reconciliation against the user's pasted plan

The user pasted a detailed Plan 2 (Loops 180-210, ~30 loops). Significant chunks of Phase 1 and Phase 2 of that plan **already shipped in Loops 155-179**:

| User's plan | Status | Source / commit |
|---|---|---|
| L180 ungate `experimental-runtime` | Already wired; just needs to become *default* | `impulse-supervisor/Cargo.toml` (L167) |
| L181 vt100 → alacritty_terminal | **Conflicts with L178 audit** — defer per recommendation | `docs/research/2026-04-23-parser-swap-audit.md` |
| L182 Dioxus PtyTerminalView | **Done** (run-based rendering, one `<span>` per `CellRun`) | `impulse-term-dioxus/src/pty_view.rs` (L162-L165) |
| L186-187 BlockModel | **Done** | `impulse-term-core/src/blocks.rs` + `osc133.rs` (L168-L170) |
| L191-192 BlockListView + toolbar | **Done** | `impulse-term-dioxus/src/blocks_view.rs` + `styles.rs` (L171-L174) |

So this reconciled plan **drops the redo work**, **keeps the vt100 decision** from the L178 audit, and **front-loads the three open questions** the user's plan deferred to "before loop 186."

## Architectural choices (decided up-front, not deferred)

1. **ratatui TUI:** keep as a separate, supported binary. It's the no-GUI / SSH / CI path. The Dioxus supervisor is the desktop binary. Two distinct binaries; one shared core.
2. **Supervisor-spawned workers:** *gated* allowlist for now. The supervisor *can* spawn workers programmatically but only from a per-project allowlist file (`.impulse/spawn-allowlist.toml`). User-initiated spawns from the UI are unrestricted. Phase 2 (future) adds budget + audit ledger; out of scope here.
3. **`@impulse` wire format:** literal text protocol on the worker's PTY input. The hook detects `^@impulse [a-z-]+( .*)?$` at the start of a freshly-typed line, intercepts before the shell sees it, sends a JSON-line request `{"verb":"<verb>","arg":"<arg>","pane":"<uuid>"}` over `IMPULSE_CMD_SOCKET`. Verbs: `compact`, `watch`, `summarize`, `inject` (extensible). Worker pane shows `@impulse compact` typed back as confirmation; supervisor shows the resulting action in its log.

## Vt100 decision (reaffirmed)

Per `docs/research/2026-04-23-parser-swap-audit.md`: stay on vt100. Build scrollback by virtualizing the `BlockStore` in the renderer (each finished block is already an addressable unit — better UX than raw line scrollback). Re-evaluate only when scrollback / OSC 8 / sixel becomes a *hard* requirement.

---

## Tranche map (15 loops, branch `cleanup/loop-103-onward` continues)

### Tranche A — Connect & Privilege (loops 180-184, 5 loops)

| Loop | Task | Verification |
|---|---|---|
| 180 | View audit: catalog all 15 `impulse-gui` views, mark each as "ported" (has Dioxus equivalent) / "needed" (must port) / "delete" (orphan or aspirational). Write `docs/research/2026-04-24-impulse-gui-view-audit.md`. | doc committed, every view has a verdict |
| 181 | Wire `BlockListView` into supervisor worker panes (currently render `PtyTerminalView` only). Toggle live-grid / block-history view per pane. | manual run shows blocks per pane |
| 182 | Supervisor-tagged spawn: when supervisor spawns a worker, inject `IMPULSE_SUPERVISOR=1` (NO — only on the supervisor pane), `IMPULSE_WORKER_PANE_ID=<uuid>`, `IMPULSE_CMD_SOCKET=<path>`. Worker panes get the socket; only the supervisor pane gets `IMPULSE_SUPERVISOR=1`. | env-var introspection test in `panes.rs` |
| 183 | `@impulse` wire format implementation: hook detector + JSON command, no verb impls yet. Detector lives in core (toolkit-neutral). | unit test: feed bytes, get parsed command |
| 184 | First verb: `@impulse summarize` — supervisor reads target pane's recent block history, returns summary into supervisor pane. | integration test through supervisor binary |

### Tranche B — Workbench Views (loops 185-194, 10 loops, scope set by Loop 180 audit)

This tranche's exact loop count depends on the audit. Place-holder structure assumes ~5-7 surviving views, ~2 loops per view (Dioxus component + integration with daemon bridge + tests).

| Loop | Task | Verification |
|---|---|---|
| 185-186 | View 1 (likely sessions browser) | parity test against egui screenshot |
| 187-188 | View 2 (likely memory persistence) | parity test |
| 189-190 | View 3 (likely settings) | parity test |
| 191-192 | View 4 (TBD by audit) | parity test |
| 193-194 | Polish: command palette + Ctrl+P quick switch + keyboard nav | manual exercise |

### Tranche C — Cutover & Archive (loops 195-199, 5 loops)

| Loop | Task | Verification |
|---|---|---|
| 195 | Make `experimental-runtime` the default feature on `impulse-supervisor`; rename to `desktop` | `cargo build -p impulse-supervisor` (no flags) ships the desktop binary |
| 196 | Replace `default-members` in root `Cargo.toml`: drop `impulse-gui`, add `impulse-supervisor` | `cargo build` from workspace root builds supervisor |
| 197 | Archive `impulse-gui` → `_archive-2026-04-XX-L197/`. Archive `impulse-term` (egui adapter) → same dir | `git ls-files impulse-rs/impulse-gui` returns nothing |
| 198 | Update CLAUDE.md (workspace inventory, retired crates, "no immediate mode" stated) + README + `impulse-rs/CLAUDE.md` | docs grep clean for active-egui references |
| 199 | Final verification gate + CHANGES.md tranche entry + json-reports rollup `loops-180-199.json` | all 6 gates green |

---

## Done criteria

- `cargo build --workspace` produces a desktop binary that uses zero egui code paths
- `git grep -l 'use eframe\|use egui' impulse-rs/` returns only files in `_archive-*/`
- The supervisor reaches whatever subset of "feature parity" the L180 audit defined as required
- `cargo test --workspace` ≥ 2,005 (non-regressing from Plan 1)
- The user can run `cargo run -p impulse-supervisor` (no flags) and get the workbench
- A run of the binary visibly demonstrates: privileged supervisor pane (left), worker panes (main), `@impulse summarize <pane>` working live

## Out of scope (deliberately)

- `alacritty_terminal` swap (deferred per L178 audit — revisit when scrollback or OSC 8 becomes a hard requirement)
- Autonomous worker spawning with budget guardrails (Phase 2 future work)
- Sixel / kitty-graphics inline images (no current user requirement)
- Migrating the ratatui TUI (it stays — different surface, different audience)
