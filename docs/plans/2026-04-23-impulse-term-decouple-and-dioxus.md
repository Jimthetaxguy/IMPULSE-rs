# Plan: Decouple `impulse-term` from egui, then ship Dioxus renderer

> **Created:** 2026-04-23
> **Scope:** Ralph Plan 7 loops L155–L179 (25-loop tranche)
> **Driver:** egui's immediate-mode memory growth under streaming-terminal workloads. Replace with Dioxus + system webview (retained-mode, native compositor). No Electron, no bundled Chromium, no TypeScript in our codebase.
> **Non-goals (this tranche):** parser swap (`vt100` → `alacritty_terminal`) — deferred to a later tranche; that work becomes safe and cheap once decoupling lands.

## Why now

`impulse-term/Cargo.toml:11` declares `eframe = "0.31"` and the crate's public API leaks `egui::Color32` (`theme.rs:103`), `egui::Ui` (`renderer.rs:52`), `egui::Key` (`input.rs`). The crate's own architecture comment (`lib.rs:19`) admits `TerminalRenderer reads vt100::Screen → egui paint calls` — meaning the supposedly-shareable terminal core is actually an egui-only widget. Until that boundary is fixed, the Dioxus shell (`impulse-supervisor`) cannot consume the terminal core; it would have to reimplement PTY + parser + grid.

## End state after L179

```text
impulse-ops/                  (unchanged: shared types, daemon protocol)
impulse-term-core/            (NEW: PTY + parser + grid + context bridge, no GUI dep)
  ├── backend.rs              (moved from impulse-term)
  ├── context.rs              (moved)
  ├── role.rs                 (moved)
  ├── grid.rs                 (NEW: GridSnapshot, CellRun, TermColor, CellAttrs)
  └── input.rs                (NEW: TermKey enum + key_to_pty_bytes — toolkit-neutral)

impulse-term-egui/            (NEW: egui adapter — keeps current behavior)
  ├── renderer.rs             (moved, mapped to GridSnapshot)
  ├── theme.rs                (moved, maps TermColor → egui::Color32)
  ├── status_bar.rs           (moved)
  ├── input_egui.rs           (NEW: egui::Key → TermKey shim)
  └── panel.rs                (moved)

impulse-term-dioxus/          (NEW: Dioxus renderer)
  ├── lib.rs                  (rsx! component reading GridSnapshot)
  ├── runs.rs                 (CellRun → <span style="..."> with damage tracking)
  ├── theme_css.rs            (TermColor → CSS color string)
  └── input_dx.rs             (Dioxus key event → TermKey)

impulse-gui/                  (unchanged consumers, repointed to impulse-term-egui)
impulse-supervisor/           (consumes impulse-term-core + impulse-term-dioxus)
impulse-gui-legacy-adapter/   (archived — already orphaned, not in workspace)
```

## Loop-by-loop plan (L155 → L179)

### Phase 1: Decouple (L155–L162)

| Loop | Deliverable | Verify |
|------|-------------|--------|
| **L155** (this) | This plan doc committed; survey of egui surface complete (5 files, 5 imports) | `cat docs/plans/2026-04-23-impulse-term-decouple-and-dioxus.md` |
| **L156** | New worktree `worktrees/decouple-term`. Scaffold `impulse-term-core/` crate (Cargo.toml, lib.rs, empty modules). Add to workspace. | `cargo build -p impulse-term-core` |
| **L157** | Move `backend.rs`, `context.rs`, `role.rs` from `impulse-term/` to `impulse-term-core/`. Re-export from old crate for back-compat. | `cargo test -p impulse-term-core` |
| **L158** | Extract `TermColor`, `CellAttrs`, `GridSnapshot`, `CellRun` into `impulse-term-core/src/grid.rs`. Pure types, no egui. Move `vt100::Color` → `TermColor` mapping out of `theme.rs`. | `cargo test -p impulse-term-core` (new round-trip tests for TermColor) |
| **L159** | Extract `TermKey` enum into `impulse-term-core/src/input.rs`. Move `key_to_pty_bytes` (already toolkit-neutral). | `cargo test -p impulse-term-core` |
| **L160** | Scaffold `impulse-term-egui/` crate. Move `renderer.rs`, `theme.rs`, `status_bar.rs`, `panel.rs` into it. Add `input_egui.rs` (egui::Key → TermKey shim, ~30 LOC). | `cargo build -p impulse-term-egui` |
| **L161** | Repoint `impulse-gui` from `impulse-term` → `impulse-term-egui`. Update use paths. Delete (archive) the now-empty `impulse-term/` shim crate after confirming no consumers. | `cargo build --workspace && cargo test --workspace` |
| **L162** | Fix any breakage. **Verification gate:** all 110 impulse-term tests + all 1,344 workspace tests green. Commit. | Full gate (`cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check`) |

**Decouple-phase invariant:** at every loop boundary, the workspace must build and all tests pass. No commits with red CI.

### Phase 2: Dioxus renderer (L163–L167)

| Loop | Deliverable | Verify |
|------|-------------|--------|
| **L163** | Scaffold `impulse-term-dioxus/` crate with `dioxus = "0.6"`. Skeleton component `TerminalView { grid: Signal<GridSnapshot> }`. | `cargo build -p impulse-term-dioxus` |
| **L164** | Render: walk `GridSnapshot.runs()` (already run-based per `renderer.rs:31`), emit `<span style="color:..; background:..; font-weight:..">{text}</span>`. One `<div>` per row, monospace CSS. | Snapshot test: render fixture grid → assert HTML output contains expected runs |
| **L165** | **Damage tracking:** one `Signal<RowSnapshot>` per row. Backend reader thread updates only changed rows. Idle terminal = zero re-renders. Bound retained signals to the visible window + N rows of scrollback; older rows compact to `Signal<String>` (frozen blocks). | Bench: 60 fps for 30s of streaming output, RSS growth < 5MB |
| **L166** | Wire `impulse-supervisor`: replace placeholder pane content with `TerminalView`. Spawn one PTY per pane via `impulse-term-core::TerminalBackend`. | `cargo build -p impulse-supervisor --features experimental-runtime` |
| **L167** | End-to-end smoke: launch supervisor, see `bash` prompt in a worker pane, type `ls`, see output. Capture screenshot. | Manual smoke + ~3 integration tests under `experimental-runtime` feature |

### Phase 3: Block model + supervisor affordances (L168–L174)

| Loop | Deliverable | Verify |
|------|-------------|--------|
| **L168** | `impulse-term-core::blocks` — `BlockBoundary` enum (Prompt / Output / Exit), `BlockStore` collecting boundaries from grid stream. | Unit tests: parse fixture stream → assert block list |
| **L169** | Shell-integration parser — recognize OSC 133 sequences (`A` = prompt start, `B` = command start, `C` = output start, `D` = command exit). Same protocol VS Code uses. | Tests with fixture OSC streams |
| **L170** | Inject hooks for `bash`, `zsh`, `fish` `PROMPT_COMMAND` / `precmd` that emit OSC 133. Source from `impulse-term-core::shell_integration::script_for(shell)`. Document in README. | Integration test: spawn `bash --init-file <our-rc>`, run command, assert blocks parsed |
| **L171** | Dioxus `BlockView` component — renders one block (header bar with command + exit status, collapsible body). | Snapshot test |
| **L172** | Block affordances: copy button, rerun button (writes command back to PTY), "send to insight extractor" button (calls existing `ContextBridge::extract`). | Manual smoke + integration tests for handlers |
| **L173** | Sticky scroll: pin the running command's prompt at the top of the pane while output streams below. | Visual smoke test |
| **L174** | Gutter decorations: exit-code icon (✓ green / ✗ red), duration badge, agent-detected anomaly mark. | Visual + screenshot |

### Phase 4: Verify, archive, consolidate (L175–L179)

| Loop | Deliverable | Verify |
|------|-------------|--------|
| **L175** | **Workspace verification gate.** All workspace tests + clippy clean. Update test count in CLAUDE.md if changed. | `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check` |
| **L176** | Archive `impulse-gui-legacy-adapter/` → `_archive-2026-04-23-L176/`. (Already not in workspace.) Update README. | `find . -maxdepth 2 -name "impulse-gui-legacy-adapter" → only under _archive-*` |
| **L177** | Update `impulse-supervisor/src/lib.rs:17` contract comment: replace *"100% Rust; no webview"* with *"100% Rust authoring; system webview as renderer; no bundled Chromium; no TypeScript"*. Update `.opencode/ralph-loop-state/cleanup/DECISIONS.md` if it exists. | grep contract text |
| **L178** | Research spike: `alacritty_terminal` API audit. Output: `docs/research/2026-04-23-parser-swap-audit.md` with breakage assessment for next-tranche parser swap. No code changes. | Doc review |
| **L179** | Consolidation commit: update `CHANGES.md` with L155–L179 summary. Produce JSON rollup at `.opencode/ralph-loop-state/rollups/loops-155-179.json`. | `cat CHANGES.md` shows tranche entry |

## Verification gate (every loop)

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

A loop only counts as complete when all four pass. No exceptions.

## Rollback plan

The decoupling is mechanical — moving files between crates and adding shim types. If Phase 1 breaks the build in a way that can't be fixed in 1–2 loops:

1. Worktree `worktrees/decouple-term` is throw-away — discard it, no impact on `main`/`cleanup/loop-103-onward`.
2. The original `impulse-term/` crate is preserved untouched until L161, when consumers repoint. Until then, both old and new exist side-by-side.
3. If Phase 2 (Dioxus renderer) hits a wall (e.g., webview perf surprises), the egui shell is still live and unaffected — Phase 1 left it working through `impulse-term-egui`.

## Out of scope (deferred)

- **Parser swap (`vt100` → `alacritty_terminal`).** Researched in L178 only. Real swap deferred to next tranche; needs migration of ~110 tests and resize/reflow validation.
- **WebGL / Canvas Dioxus renderer.** DOM-only for v1. Revisit if RSS/CPU under streaming load exceeds budget.
- **OSC 8 hyperlinks, sixel/iTerm2 image protocols, BiDi text.** Phase 5+.
- **Retiring `impulse-gui`** (the egui shell). Not in this tranche. Once `impulse-supervisor` reaches functional parity (post-L179), evaluate as a separate decision.

## Design rules (carried into Dioxus to prevent egui memory bug recurrence)

1. **Run-based rendering.** Reuse the `CellRun` insight from `renderer.rs:31` — one `<span>` per run, not per cell. Reduces DOM nodes ~40×.
2. **Damage tracking via per-row signals.** Idle rows produce zero diffs. Streaming rows update only the active block's signals.
3. **Bounded retained state.** Scrollback above N rows compacts to immutable frozen blocks (`Signal<String>` per block, no per-cell signals).
4. **No frame loop on the GUI side.** The renderer is event-driven: backend reader thread sets a `Signal<u64>` row-version counter; Dioxus re-renders only on signal change. There is no `request_repaint()` equivalent firing 60×/sec like in egui.

These rules are the explicit insurance against repeating the immediate-mode memory issue in a new toolkit.
