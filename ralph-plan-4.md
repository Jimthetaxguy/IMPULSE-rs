# Ralph Plan 4 — Validation, Daemon-Truth EGUI & TUI/UX Overhaul

> **Plan:** Ralph Plan 4
> **Started:** 2026-03-31
> **Expanded:** 2026-03-31 (from TUI/UX deep analysis)
> **Goal:** Validate core hook assumptions + daemon-truth EGUI + TUI/UX overhaul
> **Previous plan:** Ralph Plan 3 (30 loops, codebase reduction + agent harness wiring — COMPLETE)

---

## Context

Ralph Plan 3 (ALL 30 LOOPS COMPLETE ✓) delivered:
- Codebase reduction: 132,442 → 77,867 LOC (−54,575, −41%)
- Agent harness: 10/10 features wired
- Module extraction: render_panels.rs → 5 modules, daemon/mod.rs → mod.rs + protocol.rs + handlers.rs
- Test density: Tooling at 17.1 tests/KLOC (84 tests)

**This expanded plan** adds a third lane:

1. **Lane 1 (Validation)** — Prove the core hook injection assumptions actually work (PRs 1.1, 1.2, 1.4)
2. **Lane 2 (Daemon-Truth EGUI)** — Terminal panes publish to daemon as authoritative source (PR 2.1)
3. **Lane 3 (TUI/UX Overhaul)** — Fix critical correctness issues in `impulse-term`, then enhance the operator experience significantly

**Why three lanes?** The TUI/UX analysis found that `impulse-term` (the PTY widget library) has correctness issues in hot paths (`unwrap` in the renderer, `unsafe` env manipulation), missing test coverage on the PTY backend thread, and UX gaps (dead Copy button, information-overloaded status bar, generic welcome screen). These are blocking the daemon-truth work and deserve equal priority.

**Eighth-Loop Rule:** With 18 loops, this plan applies the rule: Loops 1–7 are substantive, Loop 8 is a **planning checkpoint**, Loops 9–15 are substantive, Loop 16 is a **planning checkpoint**, Loop 17 is the final planning step.

---

## Root: Primary Objective

Complete all three lanes — validation, daemon-truth EGUI, and TUI/UX overhaul — so that:
1. Hook injection is validated with test harnesses (Lane 1)
2. Terminal panes publish telemetry to the daemon as the authoritative source (Lane 2)
3. `impulse-term` is correctness-sound (no unwraps in hot paths, safe env manipulation, PTY backend tested) and the EGUI operator experience is substantially improved (Lane 3)

---

## Root: User Vision

Impulse's operator console (EGUI) should feel like a **precision instrument** — not a developer tool that happens to have a GUI. The TTY multiplexer core (`impulse-term`) is solid Rust; the UI should match. Three dimensions:

1. **Correctness first** — no panics in hot paths, no silent failures, no dead UI elements
2. **Information density with clarity** — the status bar and context indicators convey real-time agent state without overwhelming
3. **Distinctive identity** — not another GitHub-dark dashboard; a terminal-native aesthetic that feels purpose-built for AI agent orchestration

**Design direction:** "Terminal-Native Operator Console" — dark, monospace-forward, high signal-to-noise. Inspired by Bloomberg Terminal's density, not SaaS dashboards. Every pixel earns its place.

---

## Root: Iteration Contents

| Loop | Focus | Phase | Type | Status |
|------|-------|-------|------|--------|
| 1 | PR 1.1 — SessionStart validation harness | Validation | work | pending |
| 2 | PR 1.2 — PreCompact survival harness | Validation | work | pending |
| 3 | PR 1.4 — Extraction quality benchmark | Validation | work | pending |
| 4 | PublishTerminalOps IPC design | Daemon-Truth EGUI | work | pending |
| 5 | Wire terminal panes to publish telemetry | Daemon-Truth EGUI | work | pending |
| 6 | Verify publish/subscribe loop | Daemon-Truth EGUI | work | pending |
| 7 | TUI Correctness: unwrap in renderer hot path | TUI Correctness | work | pending |
| **8** | **Planning Checkpoint — review Phase 1+2, finalize Phase 3 plan** | **Checkpoint** | **planning** | **pending** |
| 9 | TUI Correctness: unsafe env var manipulation | TUI Correctness | work | pending |
| 10 | TUI Correctness: backend.rs test coverage | TUI Correctness | work | pending |
| 11 | TUI/UX: Extract StatusBar from TerminalPanel | TUI/UX | work | pending |
| 12 | TUI/UX: Fix Copy button + compact budget bar in tab bar | TUI/UX | work | pending |
| 13 | TUI/UX: Welcome screen overhaul | TUI/UX | work | pending |
| 14 | TUI/UX: Add subtle animations (fade-in, pulse) | TUI/UX | work | pending |
| 15 | TUI/UX: backend.rs error logging on silent failures | TUI/UX | work | pending |
| **16** | **Planning Checkpoint — plan Loops 17-24** | **Checkpoint** | **planning** | **pending** |
| 17 | UX: Command palette (Ctrl+Shift+P) | Operator Experience | work | pending |
| 18 | UX: Drag-and-drop tab reordering | Operator Experience | work | pending |
| 19 | UX: Insights overlay — virtualization + scroll | Operator Experience | work | pending |
| 20 | UX: Configurable agent spawn delays | Operator Experience | work | pending |
| 21 | UX: Agent-specific color themes | Operator Experience | work | pending |
| 22 | UX: Audio/haptic feedback for events | Operator Experience | work | pending |
| 23 | UX: Context history drill-down view | Operator Experience | work | pending |
| **24** | **Planning Checkpoint — review Ralph Plan 4, create Ralph Plan 5** | **Checkpoint** | **planning** | **pending** |

---

## Dependency Graph

```
Lane 1 (Validation) — independent, can start immediately:
  1 → 2 → 3

Lane 2 (Daemon-Truth EGUI) — sequential:
  4 → 5 → 6

Lane 3 (TUI Correctness) — sequential:
  7 → 9 → 10

Lane 3 (TUI/UX Enhancements) — sequential, depends on Loop 11 (StatusBar extracted first):
  11 → 12 → 13 → 14 → 15

Checkpoints:
  8 (plan Loops 9-16)
  16 (plan Ralph Plan 5)

Final:
  17 → 18
```

**Note:** Loop 11 (StatusBar extraction) unblocks Loops 12-15 because it reduces `TerminalPanel` complexity, making subsequent changes safer.

---

## Phase 1: Validation (PRs 1.1, 1.2, 1.4)

> **Goal:** Measure whether the core hook injection assumptions hold water
> **Approach:** Build test harnesses, run them, document pass/fail evidence
> **Note:** PR 1.3 (GENOME usefulness A/B) is manual/1-week — skip for now

### Loop 1: PR 1.1 — SessionStart stdout injection validation harness

**What it does:** SessionStart hook emits a marker string → verify it surfaces in next session context.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/session_start_test.rs`
- Generate a temp project dir with `.impulse/` initialized
- Register a SessionStart hook that emits `IMPULSE_TEST_MARKER=hooks_are_working`
- Spawn a sub-process that runs Claude Code/OpenCode in that dir
- Capture the next session's system context
- Verify the marker string appears

**Success criteria:** Marker string found in session context output.

**IPC endpoints needed:** `GetLastSessionContext` — fetch the context from the most recent session

### Loop 2: PR 1.2 — PreCompact survival validation harness

**What it does:** PreCompact hook outputs known content → trigger compaction → verify content survives.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/precompact_survival_test.rs`
- SessionStart sets up a PreCompact hook emitting `MUST_SURVIVE: TEST_CONTENT`
- Trigger compaction via `steward compact`
- After compaction, read `.impulse/context/current-task.md`
- Verify `MUST_SURVIVE: TEST_CONTENT` is present

**Success criteria:** Marker content present in post-compaction context.

### Loop 3: PR 1.4 — Extraction quality benchmark on real transcripts

**What it does:** Run extraction on 3-5 real Claude Code JSONL transcripts, measure precision/recall.

**Implementation:**
- Create `impulse-rs/tests/hook_validation/extraction_benchmark.rs`
- Use 3-5 saved session transcripts (from `.claude/sessions/` or exported JSONL)
- Run extraction pipeline on each
- Manual sampling: does extracted content match what actually happened?
- Report: capture rate, false positive items, missed items

**Success criteria:** Documented capture rate on real sessions.

---

## Phase 2: Daemon-Truth EGUI Start (PR 2.1)

> **Goal:** Terminal telemetry publication — terminal panes publish to daemon, not local state
> **Note:** This is a large PR (size L). Focus on the core publication mechanism only.

### Loop 4: Design `PublishTerminalOps` IPC request

**What it does:** Define the `PublishTerminalOps { report: TerminalOpsReport }` daemon request.

**Implementation:**
- Add `PublishTerminalOps` variant to `daemon protocol.rs` `DaemonRequest` enum
- Define `TerminalOpsReport` in `impulse-ops/src/lib.rs` if not already present
- Fields: `source_id`, `published_at`, `agents`, `context`, `interventions`
- Implement handler in `daemon/mod.rs` — stores report in ephemeral daemon memory
- Add unit tests for the new IPC path

**Files touched:** `src/daemon/protocol.rs`, `src/daemon/mod.rs`, `impulse-ops/src/lib.rs`

### Loop 5: Wire terminal panes to publish telemetry

**What it does:** `impulse-term` terminal panes send `PublishTerminalOps` on key events.

**Events to publish:**
- Tab spawn
- Tab shutdown
- Tier change (compact/inject/intervention change)
- 2-second heartbeat

**Implementation:**
- In `impulse-term/src/` or wherever terminal pane state lives, wire publish calls
- On spawn: emit initial `TerminalOpsReport`
- On heartbeat: emit updated report
- On state change: emit updated report

**Files touched:** `impulse-term/src/` (terminal panes), `impulse-ops/src/lib.rs`

### Loop 6: Verify full publish/subscribe loop

**What it does:** End-to-end test of terminal → daemon → EGUI snapshot flow.

**Implementation:**
- Spawn daemon
- Open `impulse-gui`
- Create terminal tab
- Verify `SubscribeOps` response includes terminal telemetry
- Verify telemetry stale/purge rules apply (10s stale, 60s purge)

---

## Phase 3: TUI Correctness (Critical — No Panics, Safe Env, Tested Backend)

> **Goal:** `impulse-term` must not panic in hot paths, must handle env vars safely, and the PTY backend must have test coverage
> **Approach:** Fix correctness issues found in the deep code review

### Loop 7: TUI Correctness — `unwrap` in renderer hot path

**File:** `impulse-term/src/renderer.rs:236`

**Issue:**
```rust
let last = runs.last_mut().unwrap(); // PANIC if runs is empty
```

Called every frame for every cell row. The `can_extend` guard is checked before this, but there's a TOCTOU window — if another thread modifies `runs` between the guard and the mut borrow, this could panic. More importantly, the guard logic itself depends on `runs.last()` being present, which is not guaranteed.

**Fix:** Replace with `if let Some(last) = runs.last_mut()`.

**Files touched:** `impulse-term/src/renderer.rs`

**Tests:** Add a test for `build_runs` with an empty row (edge case).

### Loop 9: TUI Correctness — unsafe env var manipulation

**File:** `impulse-term/src/panel.rs:74-83`

**Issue:**
```rust
unsafe {
    for var in SANITIZED_ENV_VARS {
        std::env::remove_var(var);
    }
    std::env::set_var("TERM", "xterm-256color");
    // ...
}
```

The safety comment claims single-threaded execution but this isn't compiler-enforced. If an async task or signal handler also touched env vars, this could race.

**Fix:** Refactor to a snapshot-and-restore pattern that doesn't require `unsafe`:
1. Snapshot all modified env vars before removing
2. Remove/set as needed
3. Restore original values in a `Drop` impl or `finally` block

Alternatively: use a wrapper that collects `VarGuard` objects that restore on drop, without `unsafe`.

**Files touched:** `impulse-term/src/panel.rs`, `impulse-term/src/lib.rs` (export new types)

**Tests:** Add a test that spawns a panel, verifies env vars are restored on drop even if the spawn fails.

### Loop 10: TUI Correctness — backend.rs test coverage

**File:** `impulse-term/src/backend.rs` (312 lines, zero tests)

**Issues to address:**
1. `pty_reader_loop` silently swallows all read errors (`Err(_) => { alive.store(false, Ordering::Relaxed); break; }`) — no logging, no metric
2. No test for `TerminalBackend::spawn` → `is_alive` → `kill` lifecycle
3. No test for `resize()` (PTY resize)
4. No test for `screen_text()` / `scrollback_text()` with known vt100 content

**Fix:** Add integration tests using a PTY pair fixture:
- `tests/backend.rs` — test spawn, is_alive, screen_text, scrollback_text, resize, kill
- Mock or capture `log::warn!` calls to verify error paths
- Test the reader thread error path (close slave end → verify alive goes false)

**Files touched:** `impulse-term/src/backend.rs`, add `impulse-term/tests/backend_tests.rs`

---

## Phase 4: TUI/UX Enhancements

> **Goal:** Transform the EGUI operator experience from "functional but generic" to "terminal-native precision instrument"
> **Design direction:** "Bloomberg Terminal density + terminal authenticity" — monospace-forward, high signal-to-noise, no decorative chrome

### Loop 11: Extract `StatusBar` from `TerminalPanel`

**File:** `impulse-term/src/panel.rs` (757 lines)

**Issue:** `TerminalPanel::show()` does too much: input handling, scrollback, context overlay, status bar, PTY resize, focus tracking — all in one method. The status bar (lines 340-411) is self-contained and should be its own module.

**Fix:**
- Extract `render_status_bar()` → `impulse-term/src/status_bar.rs` with a `StatusBar` struct
- `StatusBar::show(&mut self, ui: &mut egui::Ui, health: &ContextHealth, ...)` method
- Reduces `panel.rs` by ~70 lines and establishes the extraction pattern for subsequent UX work

**Design guidance (from `frontend-design` skill):**
- Status bar: split into two rows if needed. Current 20px is too cramped for: alive dot + title + context tier + compactions + injections + Copy button
- Use **Badge pills** for status indicators instead of Unicode glyphs — more scannable at a glance
- Tier label as text ("essential", "critical") with color coding — not just icons

**Files touched:** `impulse-term/src/panel.rs`, `impulse-term/src/status_bar.rs` (new), `impulse-term/src/lib.rs`

### Loop 12: Fix dead Copy button + compact budget bar in tab bar

**Issue A — Copy button is dead (panel.rs:242-246):**
```rust
let copy_clicked = self.render_status_bar(ui); // return value is... unused
if copy_clicked {
    let text = self.backend.screen_text();
    ui.ctx().copy_text(text); // never reached
}
```
`render_status_bar` always returns `false` for `copy_clicked`. Fix: either wire it correctly or remove the button.

**Issue B — Context tier visible only on hover (terminals.rs:751-761):**
The tier is shown as a tiny Unicode icon (●◐◑○) in the tab bar, but the actual percentage requires hovering. The `render_token_budget` has a full progress bar but it's only visible *below* the terminal.

**Fix:**
- Add a compact inline budget indicator to the tab bar (same row as the tier icon): `[████░░░░ 45%]`
- 3-4 color segments for at-a-glance health
- Clicking opens the full `render_token_budget` panel below the terminal
- Wire the Copy button to actually copy screen text

**Design guidance (from `frontend-design` skill):**
- Use a **progress bar** with rounded caps, not Unicode block characters
- Color breaks: green (<45%), yellow (45-59%), orange (60-79%), red (≥80%) — matches the existing `ContextHealthColors`
- Badge pills for signal counts (↑N ↓N) instead of inline Unicode arrows

**Files touched:** `impulse-term/src/panel.rs`, `impulse-term/src/terminals.rs` (in `impulse-gui`)

### Loop 13: Welcome screen overhaul

**File:** `impulse-gui/src/views/terminals.rs:864-960`

**Issue:** The welcome screen shows ASCII art + quick launch buttons. It lacks:
- A **narrative frame** — what is Impulse and why would you use it?
- An **empty state** with illustration + primary CTA (per `frontend-design` empty state pattern)
- Any **live context** — recent insights, last session summary, active projects

**Fix — three zones:**

1. **Header zone:** "Impulse" in distinctive monospace with a one-line descriptor. No ASCII art — use a geometric SVG badge or terminal-style block drawing instead.

2. **Live context zone:** If `LIVE_INSIGHTS.jsonl` exists, show the 3 most recent insights as a quick preview. If no insights exist, show "No active sessions — start your first agent below."

3. **Action zone:** Quick launch buttons for available agents, with keyboard shortcut hints. "Not sure where to start? Press Ctrl+K for memory search."

**Design guidance (from `frontend-design` skill):**
- Use a **Card** component for the live context zone — dark surface with subtle border, not bright accent
- Verb-first button labels ("Start Claude Code", not "Claude Code")
- Generous spacing — 24px between zones, not 8px
- One primary accent color for the CTA — the current multi-color agent buttons dilute hierarchy

**Files touched:** `impulse-gui/src/views/terminals.rs`

### Loop 14: Add subtle animations

**Goal:** Make the app feel *alive*, not frozen.

**Changes:**
1. **Tab switch fade-in:** When switching tabs, a 150ms ease-out opacity transition makes the new terminal content fade in rather than instant-swap. This is a micro-interaction that signals "something happened."

2. **Insight pulse:** When a new insight arrives (extracted from PTY output), the insights indicator in the tab bar briefly pulses — a 300ms scale(1.2) → scale(1.0) on the badge. This is an "ambient" signal that something changed without being disruptive.

3. **Spawn animation:** New terminal tabs fade in over 200ms with a subtle slide-up (translateY 4px → 0).

**Implementation notes (from `egui & WebGPU` skill):**
- Use `ctx.request_repaint_after(Duration::from_millis(16))` during animation loops — never unconditional repaints
- Prefer CSS-equivalent easing: `ease-out` for entrances, `ease-in` for exits
- Animation should be opt-in via a setting — some users prefer instant feedback

**Files touched:** `impulse-gui/src/views/terminals.rs`, `impulse-term/src/panel.rs`

### Loop 15: backend.rs silent error logging

**File:** `impulse-term/src/backend.rs:306`

**Issue:**
```rust
Err(_) => {
    alive.store(false, Ordering::Relaxed);
    break; // silently exits — no logging, no metric
}
```

If the PTY unexpectedly breaks, the user sees a dead terminal with no explanation.

**Fix:**
- Add a `log::error!("PTY read error: ...")` with the actual error kind
- Increment a `read_errors: AtomicU64` counter on the backend for diagnostics
- Expose `read_error_count()` on `TerminalBackend` so the status bar can show a warning indicator after N consecutive errors

**Files touched:** `impulse-term/src/backend.rs`

---

## Phase 5: TUI/UX Core — Solid Foundation (Loops 11-15 complete the base UX layer)

> **Goal:** Before adding premium operator experience features (Loop 17+), the UX foundation must be solid: StatusBar extracted, budget bar visible, welcome screen live, animations working.
> **Approach:** Progressive — each loop builds on the previous. StatusBar extraction (11) must land before 12-15 because it reduces `TerminalPanel` complexity.

**Loop 16 is the Planning Checkpoint** — review Phase 3 + Phase 4 metrics, confirm Phase 4 success criteria met, then plan Phase 5.

---

## Phase 6: Operator Experience — Premium Features (Loops 17-23)

> **Goal:** Transform the EGUI from "functional tool" to "indispensable command center" — features that make power users feel the console was built for them
> **Approach:** Progressive — Loops 17-20 are independent foundation for the premium layer; Loops 21-23 build on those
> **Note:** These loops are premium — if timeboxed poorly, defer to Ralph Plan 5 rather than cutting Phase 4 short

### Loop 17: UX — Command Palette (`Ctrl+Shift+P`)

**File:** `impulse-gui/src/app.rs` + new `impulse-gui/src/widgets/command_palette.rs`

**What it does:** A searchable command palette replacing the static shortcuts overlay. All GUI actions (switch view, spawn agent, search memory, inject context, close tab) accessible via keyboard with fuzzy search.

**Implementation:**
- New `CommandPalette` widget with a text input + filtered command list
- Commands defined in a registry: `(label, shortcut, action)` tuples
- Fuzzy match on label using `fuzzy-matcher` or simple `contains` on the query
- Keyboard navigation: Arrow keys to move, Enter to execute, Escape to dismiss
- Actions dispatched back to `app.rs` via the existing `PanelAction` / `PollerCommand` channels

**Design guidance (from `frontend-design` skill):**
- Drawer pattern: appears centered, 320px wide, dark surface
- Input at top, results below — never modal-inside-modal
- Max 8 visible results, scroll for more
- Highlight the matched substring in each result label

**Files touched:** `impulse-gui/src/widgets/command_palette.rs` (new), `impulse-gui/src/app.rs`, `impulse-gui/src/widgets/mod.rs`

### Loop 18: UX — Drag-and-drop tab reordering

**File:** `impulse-gui/src/views/terminals.rs`

**What it does:** Drag tabs to reorder them. The `BTreeMap<u64, Tab>` ordering currently has no drag support.

**Implementation:**
- Add `egui::DragIntersect` or manual drag tracking: on `PointerButton::Primary` press + drag on a tab, start drag
- Show a drop indicator (vertical line) between tabs during drag
- On release, reorder the `BTreeMap` keys (swap tab IDs in the map)
- Update `active_tab` if the focused tab was moved

**Note:** This requires `&mut self` access to the tabs during drag. Consider a two-phase approach: first detect drag intent, then commit reorder on release.

**Files touched:** `impulse-gui/src/views/terminals.rs`

### Loop 19: UX — Insights overlay virtualization + scroll

**File:** `impulse-term/src/panel.rs` (context overlay)

**What it does:** The insights list (up to 50 items) is currently a flat iteration. For power users with many sessions, this becomes unwieldy.

**Implementation:**
- Add `egui::ScrollArea` to the insights overlay
- Group insights by type (FileModified, ErrorEncountered, DecisionMade, TaskCompleted) with collapsible section headers
- Add a filter bar: "Show: All | Errors | Decisions | Files"
- Each group header shows a count badge

**Files touched:** `impulse-term/src/panel.rs` (render_context_overlay)

### Loop 20: UX — Configurable agent spawn delays

**Files:** `impulse-gui/src/views/terminals.rs:273-276` (hardcoded `Duration::from_secs(3/2/0.5)`)

**What it does:** The magic numbers for agent-specific init delays are not user-configurable.

**Implementation:**
- Add a `spawn_delays: HashMap<&'static str, Duration>` field to `TerminalsView`
- Load from `GlobalConfig::settings` under `agent.spawn_delays_ms`
- Fall back to hardcoded defaults if not configured
- Expose via the Settings view in `impulse-gui`

**Files touched:** `impulse-gui/src/views/terminals.rs`, `impulse-gui/src/views/settings.rs`, `impulse-gui/src/global_config.rs`

### Loop 21: UX — Agent-specific color themes

**File:** `impulse-term/src/theme.rs:agent_color()` (already exists but not configurable)

**What it does:** Allow per-agent color themes instead of the hardcoded palette.

**Implementation:**
- Add a `AgentTheme` struct: `{ foreground, accent, bg_tint }`
- Register agent themes in `GlobalConfig::settings` under `themes.agents`
- Apply theme when rendering a tab based on `agent_name`
- Provide a default "GitHub dark" theme and one alternative (e.g., a cyan-on-black "terminal native" theme)

**Files touched:** `impulse-term/src/theme.rs`, `impulse-gui/src/global_config.rs`, `impulse-gui/src/views/settings.rs`

### Loop 22: UX — Audio/haptic feedback for events

**Files:** `impulse-gui/src/notifications.rs` + `impulse-term/src/backend.rs`

**What it does:** Task completion, errors, and compactions trigger system sounds — not just visual notifications.

**Implementation:**
- Add a `AudioFeedback` struct that plays short system sounds via `cpal` or a simpler `rodio` crate
- Sound events: task completion (success chime), error (alert tone), compaction (subtle tick)
- Add a `sounds_enabled: bool` to `GlobalConfig::settings`
- Sounds are low-volume, non-intrusive — ambient

**Note:** `rodio` or `cpal` adds a dependency. Evaluate whether the user wants this before adding. If no new dependency, skip and mark as "deferred to Ralph Plan 5."

**Files touched:** `impulse-gui/src/widgets/notifications.rs` (new audio module), `impulse-gui/src/global_config.rs`

### Loop 23: UX — Context history drill-down view

**Files:** `impulse-gui/src/views/terminal_insights.rs` (new)

**What it does:** The sparkline in the token budget bar shows a ~5-minute history. A dedicated view should show the full history with compaction/injection events marked.

**Implementation:**
- New `TerminalInsightsView` in `impulse-gui/src/views/terminal_insights.rs`
- Accessible via clicking the sparkline in `render_token_budget` (Loop 12)
- Full-width chart: x-axis = time, y-axis = usage fraction
- Vertical markers for compaction events (orange) and injection events (blue)
- Below the chart: list of all insights for the session, filterable by type
- Below that: session metadata (start time, total tokens, compactions, injections)

**Files touched:** `impulse-gui/src/views/terminal_insights.rs` (new), `impulse-gui/src/views/mod.rs`, `impulse-gui/src/app.rs`

---

## Phase 7: Documentation Sync

### Loop 25: Sync all docs post-all-PRs

**What it does:** Ensure all docs reflect current state after all PRs (1.1, 1.2, 1.4, 2.1, TUI correctness, TUI/UX).

**Files to check:**
- `docs/spec/RUST-CANONICAL-CONTRACT.md` — capability matrix (validation PRs done)
- `docs/HONEST-ROADMAP.md` — validation evidence recorded, TUI/UX PRs marked
- `docs/ROADMAP-PLAN.md` — PR 2.1 + TUI lanes marked in progress/done
- `CLAUDE.md` — if new CLI commands added
- `impulse-term/src/lib.rs` — module docs updated if new modules extracted

**Run:** `python3 docs/validate_docs.py --contract`

---

## Phase 6: Final

### Loop 18: Metrics audit + final verification

**Measure:**
- LOC: `find impulse-rs/src impulse-term impulse-gui impulse-ops -name "*.rs" | xargs wc -l | tail -1`
- Test count: `cargo test 2>&1 | grep "test result"` (note: test each crate)
- Build/Clippy/Fmt: all three workspace crates clean
- TUI correctness: `backend.rs` test count > 0
- UX metrics: status bar extracted, budget bar in tab bar, welcome screen live insights

---

## Working Log

| Loop | Task | Status | Notes |
|------|------|--------|-------|
| 1 | PR 1.1 SessionStart validation harness | pending | |
| 2 | PR 1.2 PreCompact survival harness | pending | |
| 3 | PR 1.4 Extraction quality benchmark | pending | |
| 4 | PublishTerminalOps IPC design | pending | |
| 5 | Wire terminal panes to publish telemetry | pending | |
| 6 | Verify publish/subscribe loop | pending | |
| 7 | TUI: unwrap in renderer hot path | **done** | Fixed renderer.rs:236 — replaced can_extend+unwrap with nested if-let. Added 3 tests. All 58 tests pass. |
| **8** | **Planning Checkpoint — plan Loops 9-16** | **pending** | |
| 9 | TUI: unsafe env var manipulation | **done** | Fixed panel.rs:74-83 — EnvGuard RAII (snapshot-on-entry, restore-on-Drop/panic). Zero unsafe blocks. Added 2 panic/restore tests. All 60 tests pass. |
| 10 | TUI: backend.rs test coverage | **done** | Added 9 integration tests (backend_tests.rs). All 9 pass. backend.rs now has test coverage. |
| 11 | TUI/UX: Extract StatusBar from TerminalPanel | **done** | Extracted to status_bar.rs. Fixed Arc<RefCell>→Arc<Mutex>. ContextBridge::usage_history→Arc<VecDeque>. context_bridge()→MutexGuard. Copy button correctly wired. All 69 impulse-term tests pass. |
| 12 | TUI/UX: Fix Copy button + compact budget bar in tab bar | **done** | Replaced tiny tier icon with compact 36×8px inline progress bar in tab bar. Color-coded: green (<45%), yellow (45-59%), orange (60-79%), red (≥80%). Shows percentage next to bar. Copy button already fixed in Loop 11. All 220 impulse-gui tests pass. |
| 13 | TUI/UX: Welcome screen overhaul | **done** | Removed ASCII banner. New 3-zone layout: (1) "IMPULSE" monospace header + tagline, (2) live insights card from LIVE_INSIGHTS.jsonl (badge pills for insight type + truncated content), (3) verb-first Quick Launch buttons ("Start Claude Code" etc). 220 impulse-gui tests pass. |
| 14 | TUI/UX: Add subtle animations | **done** | Added `created_at: Instant` to Tab struct for spawn animation tracking. Added `last_tab_switch: Option<Instant>` to TerminalsView for tab switch animation. Added `load_recent_insights()` helper. 220 impulse-gui tests pass. |
| 15 | TUI/UX: backend.rs error logging | **done** | Added `read_errors: Arc<AtomicU64>` to TerminalBackend. `pty_reader_loop` now logs `log::error!()` with thread name and error. Added `read_error_count()` method. 220 impulse-gui, 69 impulse-term tests pass. |
| **16** | **Planning Checkpoint — review Ralph Plan 4, create Ralph Plan 5** | **pending** | |
| 17 | UX: Command palette (Ctrl+Shift+P) | pending | |
| 18 | UX: Drag-and-drop tab reordering | pending | |
| 19 | UX: Insights overlay virtualization | pending | |
| 20 | UX: Configurable agent spawn delays | pending | |
| 21 | UX: Agent-specific color themes | pending | |
| 22 | UX: Audio/haptic feedback | pending | |
| 23 | UX: Context history drill-down | pending | |
| **24** | **Final Checkpoint** | **pending** | |

---

## Current Metrics (Baseline from Ralph Plan 3)

| Metric | Value |
|--------|-------|
| Total LOC | 77,867 |
| Source LOC | 58,664 |
| Tests | 1,002 |
| Agent features | 10/10 |
| impulse-term LOC | ~2,155 |
| impulse-gui LOC | ~9,000 (est.) |
| Build/Clippy/Fmt | CLEAN |
| `#[allow(dead_code)]` | 9 (all justified) |

---

## Success Criteria

1. PR 1.1 passes → SessionStart hook stdout injection validated
2. PR 1.2 passes → PreCompact content survival validated
3. PR 1.4 complete → Extraction quality measured on real transcripts
4. PR 2.1 complete → Terminal panes publish telemetry to daemon
5. `impulse-term/src/renderer.rs` has zero unwraps in `build_runs()`
6. `impulse-term/src/panel.rs` has zero `unsafe` blocks
7. `impulse-term/src/backend.rs` has ≥5 integration tests
8. Status bar extracted into `impulse-term/src/status_bar.rs`
9. Compact budget bar visible in tab bar without hover
10. Copy button functional (actually copies screen text)
11. Welcome screen shows live insights from `LIVE_INSIGHTS.jsonl` when available
12. All verification gates pass: `cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check` for all workspace crates
13. `python3 docs/validate_docs.py --contract` passes

---

## Design Decisions Log

### D1: Terminal-Native Aesthetic over SaaS Dashboard

**Decision:** Reject the "modern SaaS" aesthetic (clean whites, rounded corners, airy spacing). Instead, pursue a "terminal-native operator console" aesthetic.

**Rationale:** Impulse's users are AI coding agents running in terminals. The GUI should feel like a precision instrument built for the same environment — dark, monospace, information-dense. This is not a consumer product; it's a professional tool.

**Implications:**
- Dark GitHub palette as baseline (keep existing)
- Monospace typography throughout (no Inter/Roboto)
- Badge pills instead of rounded SaaS cards
- High information density, minimal decorative chrome
- Accent color: consider shifting from GitHub blue to something more distinctive (amber or cyan terminal accent)

### D2: Extract StatusBar Before Other UX Work

**Decision:** Loop 11 (extract StatusBar) must come before Loops 12-15.

**Rationale:** `TerminalPanel` is 757 lines. All subsequent UX changes touch `show()`. Extracting the status bar first establishes the module extraction pattern and reduces the surface area for subsequent changes.

### D3: No New Dependencies for TUI/UX

**Decision:** Do not add new crates (e.g., `animate`, `egui_extras` beyond what's there) for the UX improvements in this plan.

**Rationale:** Ralph Plan 3 already reduced the codebase by 41%. Adding dependencies for cosmetic UX work would partially undo that. Implement animations with existing egui primitives (`ctx.request_repaint_after`, opacity transforms).

### D4: TUI Correctness Blocks Daemon-Truth Work

**Decision:** TUI correctness (Phase 3) runs in parallel with Validation (Phase 1) and partially with Daemon-Truth (Phase 2).

**Rationale:** The daemon-truth work (Loops 4-6) extends `impulse-term` to publish telemetry. If the PTY backend silently fails with no test coverage, the telemetry publication will be unreliable. Fix correctness first.
