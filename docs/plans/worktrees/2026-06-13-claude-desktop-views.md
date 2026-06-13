# Lane: claude-desktop-views — desktop shell view-switcher + Memory/Artifacts views

- **Owner:** Claude (Opus 4.8)
- **Role:** complement Codex's shell pass + own the final merge
- **Branch / worktree:** `agent/claude-desktop-views` @ `.worktrees/claude-desktop-views` (off `2feb9ce`)
- **Companion effort:** Codex on `agent/codex-dioxus-terminal-harness` (main tree), looping Phases A–F
- **Verification:** `cd impulse-rs && CARGO_TARGET_DIR=/tmp/impulse-claude-target cargo test -p impulse-desktop && cargo clippy -p impulse-desktop -- -D warnings && cargo fmt --check`

## Ownership split (agreed 2026-06-13)

| Concern | Owner | Component |
|---|---|---|
| Live ops/agent bridge, workspaces, project notes (Phase A/B) | **Codex** | `apply_desktop_bridge_message`, `runtime.rs`, `workspace.rs` |
| Review-first MCP console (Phase C) | **Codex** | `ReviewConsole`, `ReviewQueueItem`, `ReviewDecisionTool` |
| Multi-agent operator board (Phase D) | **Codex** | `OperatorBoard`, `OperatorLane` |
| **Memory view** | **Claude** | `views::MemoryView` |
| **Artifacts view** | **Claude** | `views::ArtifactsView` |
| **View-switcher spine** (unifies all five) | **Claude** | `views::DesktopView`, `ui::ViewRail`, keep-alive stage |
| Final reconciliation | **Claude** | this lane |

Claude's `views::ReviewView` and `views::SupervisorView` exist as **stand-ins** so this
worktree's shell is fully navigable and testable in isolation. At merge they are
**replaced** by Codex's `ReviewConsole` / `OperatorBoard` (see mapping below).

## Target unified architecture

`DesktopShellWithSnapshot` keeps its three-column `workspace-grid`. The change is the
**center `terminal-stage`**: a left-rail `ViewRail` drives a `DesktopView` signal that
swaps the stage between five destinations. The Terminal view (brand hero + xterm mounts)
is **kept alive** (rendered always, CSS-hidden when inactive) so the xterm interop is
never torn down (`TERMINAL_INTEROP_SCRIPT` stays sacrosanct). Other views render on demand.

```
left-rail: ViewRail [Terminal·Memory·Review·Artifacts·Supervisor] + AgentPool + WorkspaceSwitcher
center:    match active_view {
             Terminal   => kept-alive (BrandHero + stat-row + tabs + xterm mounts)
             Memory     => views::MemoryView           (Claude)
             Review     => ui::ReviewConsole            (Codex)  ← stand-in: views::ReviewView
             Artifacts  => views::ArtifactsView         (Claude)
             Supervisor => ui::OperatorBoard            (Codex)  ← stand-in: views::SupervisorView
           }
right:     inspector (unchanged: McpToolPalette, WorkspaceList, AuditTrail …)
```

All views bind to `ProjectOpsSnapshot` (+ Codex's `review_queue` signal for Review).

## Final-merge plan (Claude owns)

Merge happens **after Codex's phase loop settles**. Steps:

1. Re-sync: rebase/merge this branch onto Codex's settled `agent/codex-dioxus-terminal-harness`.
2. Resolve the conflict surfaces. **Use Codex's main-tree files as the base** (its build
   carries the live bridge + review queue); graft Claude's additions onto them:
   - **`lib.rs`** — Codex base + add Claude's `views::*` exports (additive block; Codex's
     `mcp::{ReviewQueueItem, ReviewDecisionTool, …}` exports stay).
   - **`theme.rs`** — Claude's helpers (`usage_meter_pct`, `artifact_status_*`, `severity_class`)
     are additive; keep.
   - **`assets/impulse_crt.css`** — concatenate. Namespaces are disjoint EXCEPT Claude renamed
     its review action row to **`.review-bundle-actions`** to avoid Codex's `.review-actions`
     (verified: that was the only collision). Keep Codex's 4-column `.workspace-row` (it has the
     project-notes column); drop Claude's 3-column variant.
3. Wire the switcher into Codex's `DesktopShellWithSnapshot` (it currently takes 6 props —
   `snapshot` + `runtime_agents`/`workspaces`/`mcp_tools`/`last_invocations`/`review_queue` — so
   the zero-arg `DesktopShell` wrapper must call that expanded constructor):
   - Add `let active_view = use_signal(|| DesktopView::Terminal);` + `ViewRail` to the left-rail
     (replacing the static `Sessions` buttons).
   - Wrap the existing hero/stat/tabs/xterm block in the kept-alive `stage-view view-terminal`
     wrapper (preserves the xterm interop across switches).
   - **Delete the UNCONDITIONAL `ReviewConsole { … }` and `OperatorBoard { … }` call sites** in
     Codex's `terminal-stage` (they currently render always) and move them behind `match` arms:
     Memory→`MemoryView`, Artifacts→`ArtifactsView`, Review→`ReviewConsole`, Supervisor→`OperatorBoard`.
   - `AgentPool`: settle on **one** agent type. Codex's `Vec<AgentRuntimeSnapshot>` (richer runtime
     type) wins; drop Claude's `Vec<AgentRuntime>` binding and re-point the call site.
   - Drop Claude's stand-in `ReviewView`/`SupervisorView` from the shell wiring (keep code+tests
     for reference, or delete).
   - Keep Codex's reactive `DesktopShell` (live `use_signal` + bridge eval); retire Claude's
     static `ProjectOpsSnapshot::default()` wrapper.
4. Reconcile tests: keep `desktop_contract.rs` green (Terminal default → `crt-hero`); merge both
   sides' SSR tests (Codex's review-console + Claude's `views_ssr.rs`).
5. Gate: `cargo test -p impulse-desktop && cargo clippy -- -D warnings && cargo fmt --check`, then
   full workspace.

### Pre-existing issues to flag to Codex (out of Claude's lane, NOT fixed here)

Surfaced by the adversarial review; they live in Codex-owned / sacrosanct code:
- **xterm interop effect** (`ui.rs` `use_effect` → `document::eval(TERMINAL_INTEROP_SCRIPT)`): the
  JS registers `listen('terminal_output'/'terminal_exit')` with no `unlisten()`. If the effect
  ever re-fires, listeners accumulate → duplicate terminal writes. Claude's keep-alive design does
  NOT re-fire it (Terminal never unmounts), but a one-time `use_hook` spawn would be safer.
- **`McpConfirmRow` checkbox** (`ui.rs`): `oninput` reads `evt.value() == "true"`; on WRY/WebKit a
  checkbox emits `"on"`/`""`. If confirmed, switch to `evt.checked()`. (Pre-existing; verify on the
  target webview before changing — Dioxus may normalize.)
- **`.stat .v`** (retro boot card): uses Baloo 2 + cyan bloom on numbers; the spec wants mono/calm
  for data. Left as Codex's retro-lane choice.

## Conflict-avoidance during the concurrent phase

- Claude does **not** edit the main tree's `DesktopShellWithSnapshot` (Codex owns it).
- Claude's main-tree `views.rs` is preserved by Codex (its green build depends on it) — left untouched.
- Disjoint CSS class namespaces (`view-*`/`stage-*` vs `review-console`/`operator-*`) → no style bleed.

## Status log (reverse-chronological)

- 2026-06-13 — Adversarial review (4 agents) → applied fixes: route `usage_pct` through guarded
  `usage_meter_pct`; removed P05-violating bloom from `.view-hero-value`/`.review-empty-mark`;
  switched hero/card numbers to mono (P06); demoted `.group-count` to label tone; renamed
  `.review-actions`→`.review-bundle-actions` (only real CSS collision); added `.view-rail` rule;
  +13 edge/variant tests. Gate green: **60 tests** (28 lib + 8 contract + 9 runtime + 3 tauri +
  12 views_ssr), clippy clean, fmt clean.
- 2026-06-13 — Lane opened. Worktree off `2feb9ce`. Built Memory/Review/Artifacts/Supervisor
  views + `ViewRail` switcher + keep-alive Terminal stage + 7 SSR tests.
