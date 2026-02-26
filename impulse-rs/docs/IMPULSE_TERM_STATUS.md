# impulse-term: Implementation Status & Next Steps

> **Date:** 2026-02-25
> **Plan:** `.claude/plans/imperative-noodling-lake.md`

---

## What Was Built

### impulse-term crate (Steps 1-6: Complete)

A custom terminal widget crate replacing `egui_term 0.1`. Located at `impulse-rs/impulse-term/`.

| Module | Lines | Tests | Purpose |
|--------|-------|-------|---------|
| `backend.rs` | ~200 | 0 | PTY spawn via `portable-pty`, vt100 parser, background reader thread |
| `renderer.rs` | ~244 | 0 | Run-based vt100 → egui rendering (~100-300 draw calls/frame) |
| `input.rs` | ~299 | 14 | egui Key → VT100 escape sequences |
| `theme.rs` | ~179 | 8 | ANSI color resolution (256 + truecolor) + Impulse dark palette |
| `context.rs` | ~663 | 15 | ContextBridge: token estimation, extraction, compaction, injection |
| `panel.rs` | ~309 | 0 | Assembled widget: terminal grid + input + status bar + context overlay |
| `lib.rs` | ~30 | 2 | Public API re-exports |
| **Total** | **~1,924** | **39** | |

**Dependencies:** `portable-pty 0.9`, `vt100 0.15`, `eframe 0.31`, `parking_lot 0.12`, `chrono 0.4`, `serde 1`, `log 0.4`

### GUI Integration (Steps 7-8: Complete)

- `impulse-gui/Cargo.toml` — swapped `egui_term 0.1` → `impulse-term = { path = "../impulse-term" }`
- `impulse-gui/src/views/terminals.rs` — rewritten to use `TerminalPanel` (no more `BackendSettings`, `PtyEvent`, `TerminalView`)
- `impulse-gui/src/theme.rs` — `agent_color()` delegates to `impulse_term::theme::agent_color()`
- `impulse-gui/src/app.rs` — context lifecycle tick every 3 seconds
- Main `Cargo.toml` — workspace now has 3 members

### Key Design Decisions

1. **parking_lot::FairMutex** for vt100 parser — prevents reader-thread starvation (same as Alacritty)
2. **Self-contained context bridge** — ports `context_lifecycle` patterns without depending on main crate
3. **Run-based rendering** — groups consecutive cells with same attributes to reduce draw calls by ~95%
4. **Agent-specific injection** — XML delimiters for Claude, comment blocks for others, always bracketed paste

---

## What's Pending

### Step 9: Context Monitoring View (Not Started)

Create `impulse-gui/src/views/context.rs` — a dedicated sidebar view for monitoring context lifecycle across all terminal tabs.

**Requirements:**

1. **Add `ViewId::Context` to `views/mod.rs`**
   - Add to enum, `all()`, `title()` ("Context"), `icon()` (brain emoji), `shortcut_label()` ("Ctrl+5")
   - Add `pub mod context;`

2. **Create `views/context.rs`** with three sub-panels:
   - **Active Terminals** — list each tab with: agent name, alive status, context tier (●/◐/◑/○), token usage %, compaction count, injection count
   - **Recent Insights** — timeline of `ExtractedInsight` entries across all panes, showing: timestamp (relative), agent name, insight type badge, content preview
   - **Injection Controls** — target selector (dropdown of alive tabs), content preview, "Inject Now" button

3. **Data flow:**
   - The view needs access to terminal tab state (panels, alive status, context health)
   - Option A: Pass `&TerminalsView` to the context view's `ui()` method
   - Option B: Populate `SharedState` with context health data from the lifecycle tick
   - **Recommendation:** Option B — add fields to `SharedState`, populate during `context_tick()` in `app.rs`

4. **Add to `SharedState` in `state.rs`:**
   ```rust
   pub context_health: Vec<(u64, String, impulse_term::ContextHealth)>,  // (tab_id, agent_name, health)
   pub recent_insights: Vec<impulse_term::ExtractedInsight>,             // cross-pane, bounded at 50
   ```

5. **Wire into `app.rs`:**
   - Add `context: ContextView` field to `ImpulseApp`
   - Add `ViewId::Context` to central panel match
   - Update `context_tick()` to write health/insights to `SharedState`
   - Add Ctrl+5 shortcut

### Step 10: Status Bar Enhancement (Not Started)

Update `impulse-gui/src/widgets/status_bar.rs` to show context health at a glance.

**Requirements:**

1. **Add context tier indicator** after terminal count:
   - `●` green (Comfortable, <45%)
   - `◐` yellow (Essential, 45-60%)
   - `◑` orange (Critical, 60-80%)
   - `○` red (Minimal, >80%)
   - Show worst tier across all active tabs

2. **Add compaction/injection counters:**
   - `↓N` = total compaction count across all tabs
   - `↑N` = total injection count across all tabs

3. **Data source:** Read from `SharedState.context_health`

### Estimated Scope

| File | Action | Lines |
|------|--------|-------|
| `views/mod.rs` | Modify — add ViewId::Context | ~15 |
| `views/context.rs` | New — context monitoring view | ~250 |
| `state.rs` | Modify — add context health fields | ~10 |
| `app.rs` | Modify — wire context view + populate SharedState | ~30 |
| `widgets/status_bar.rs` | Modify — add context indicators | ~30 |
| `widgets/sidebar.rs` | Modify — add Context entry (if not auto from ViewId::all()) | ~5 |
| **Total** | | **~340** |

---

## Verification Checklist

All verification currently passes:

```bash
# impulse-term
cd impulse-rs/impulse-term
cargo build                    # ✓ clean
cargo clippy -- -D warnings    # ✓ clean
cargo fmt --check              # ✓ clean
cargo test                     # ✓ 39 tests pass

# impulse-gui
cd impulse-rs/impulse-gui
cargo build                    # ✓ clean
cargo clippy -- -D warnings    # ✓ clean
cargo fmt --check              # ✓ clean

# workspace
cd impulse-rs
cargo build                    # ✓ clean (all 3 members)
```

---

## Known Limitations

1. **No tests for renderer/panel/backend** — these are GUI components; testing requires a display context or mocking egui. Manual testing required.
2. **Context bridge is not yet connected to SharedState** — extraction runs but results stay local to each `TerminalPanel`. Steps 9-10 fix this.
3. **No cross-pane insight sharing** — insights from one terminal aren't visible to others. The Context view (Step 9) addresses this.
4. **Auto-injection not triggered** — `inject_context()` is available but nothing calls it automatically yet. Could be wired in Step 9's injection controls.
5. **Scrollback navigation limited** — renderer supports scroll_offset but no scroll gesture handling yet.
6. **No text selection/copy** — renderer draws text but doesn't handle mouse selection for clipboard.
