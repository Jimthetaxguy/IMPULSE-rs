# impulse-gui view audit (Plan 2 / Loop 180)

**Date:** 2026-04-23
**Branch:** `cleanup/loop-103-onward`
**Author:** Impulse maintainers (this audit run)

## Why this audit exists

Plan 2 retires `impulse-gui` (egui workbench, ~20,000 LOC across 31 files) by porting it to the Dioxus `impulse-supervisor`. Before any porting, this audit determines: which of the 15 view files are *actually wired into the running app*, which are *embedded in another wired view*, and which are *orphans* (built and tested but never rendered).

Naive estimate: "port 20kLOC of egui to Dioxus." Real estimate after audit: **port ~4-6kLOC, delete ~2.7kLOC outright.**

## Method

For each of the 15 files in `impulse-gui/src/views/`:
1. Find every `use crate::views::<file>::<Type>` and `super::<file>` reference outside the file itself.
2. Trace each reference to determine if it's actually rendered (called from a `View::ui` chain that starts at one of the 4 top-level `ViewId`s in `app.rs`).
3. Classify as **WIRED**, **EMBEDDED**, **DEAD**, or **ORPHAN**.

The 4 top-level `ViewId`s exposed via the sidebar (per `views/mod.rs`) are: `Overview`, `Agents` ("Terminals"), `Memory`, `Settings`. Anything not reachable from these is dead.

## Results

| File | LOC | Status | Reachable from | Action |
|---|---|---|---|---|
| `overview.rs` | 294 | **WIRED** | `app.rs` → `OverviewView` field → ViewId::Overview | **PORT** |
| `terminals.rs` | 1,767 | **WIRED** | `app.rs` → `TerminalsView` field → ViewId::Agents | **PORT** (mostly already covered by `impulse-term-dioxus` + `impulse-supervisor::panes`) |
| `memory.rs` | 101 | **WIRED** | `app.rs` → `MemoryView` field → ViewId::Memory | **PORT** (thin wrapper around 3 embedded sub-views) |
| `settings.rs` | 823 | **WIRED** | `app.rs` → `SettingsView` field → ViewId::Settings | **PORT** |
| `genome.rs` | 341 | **EMBEDDED** | `memory.rs` → `GenomeView` field | **PORT** (under Memory) |
| `search.rs` | 213 | **EMBEDDED** | `memory.rs` → `SearchView` field | **PORT** (under Memory) |
| `sessions.rs` | 638 | **EMBEDDED** | `memory.rs` → `SessionsView` field | **PORT** (under Memory) |
| `terminal_search.rs` | 425 | **EMBEDDED** | `terminals.rs` → `TerminalSearch` | **PORT** (under Terminals) |
| `guardrails.rs` | 361 | **DEAD** | `app.rs:34` field, `app.rs:81` constructor — never `.ui()` called | **DELETED** in this audit |
| `artifacts.rs` | 467 | **ORPHAN** | not in `mod.rs`, not constructed anywhere | **ALREADY GONE** (pre-existing archival) |
| `context.rs` | 320 | **ORPHAN** | not in `mod.rs` at HEAD, references nonexistent `ViewId::Context` (would not even compile if added) | **DELETED** in this audit |
| `terminal_context.rs` | 537 | **EXTENSION FILE** | initially classified as orphan; reclassified after build failure: contains `impl TerminalsView { fn workbench_context, fn context_tick, fn check_threshold_injections, fn process_pending_injections, ... }` extension methods called from `terminals.rs` | **KEEP** (refactor-merge into terminals.rs is future work) |
| `terminal_insights.rs` | 77 | **EXTENSION FILE** | same — contains `impl TerminalsView { fn search_live_insights, fn merge_tab_insights_to_history, fn collected_insights, ... }` | **KEEP** |
| `memory_persistence.rs` | 646 | **HELPER (live)** | initially flagged as chain-orphan via `context.rs`; with `context.rs` archived, it's used by `terminal_context.rs` + `terminal_insights.rs` (both live extension files) | **KEEP** |

### Reclassification note (2026-04-23 in-session)

The naive "find references to the public type" search in this audit produced a false-positive orphan classification for `terminal_context.rs`, `terminal_insights.rs`, and `memory_persistence.rs`. The build immediately surfaced the truth: those files do not export their named view structs as wired views, but they DO contain `impl TerminalsView { ... }` extension blocks that `terminals.rs` calls via `self.workbench_context(...)`, `self.collect_signals(...)`, etc. Rust permits split-impl across files in the same crate, and a search for the file's "primary" public type misses these.

**Lesson encoded in `feedback_investigate_test_count_discrepancies.md` precedent:** when an audit predicts deleting > N files, attempt the deletion in a working tree and watch the build before declaring victory. The build is the source of truth.

Final audit deletions: **3 files / 1,148 LOC** (guardrails 361 + context 320 + already-gone artifacts 467) + 1 dead app.rs field. Not the originally-claimed 6 files / 2,408 LOC. Honest correction matters more than the bigger number.

### Summary

- **Port (wired or embedded under wired): 9 files, ~4,602 LOC**
  - overview, terminals, memory, settings, genome, search, sessions, terminal_search, plus the agent_panel/ + ipc/ subdirs that are out of scope for the view audit but in scope for the broader migration
- **Delete (dead or orphan): 6 files, 2,408 LOC**
  - guardrails (361), artifacts (467), context (320), terminal_context (537), terminal_insights (77), memory_persistence (646) — the chain-orphan
- **(plus dead `guardrails: GuardrailsView` field in `app.rs`)**

## Verification of "DEAD" / "ORPHAN" claims

Anyone challenging this audit can re-derive the result with these commands (run from `impulse-rs/`):

```bash
# guardrails: is the .ui() ever called? (only the field is set)
grep -n "guardrails\." impulse-gui/src/app.rs
# Expected: only the field decl + constructor, no method calls

# artifacts/context/terminal_context/terminal_insights: type usage outside own file
for v in ArtifactsView ContextView TerminalContextView TerminalInsightsView; do
    echo "=== $v ==="
    grep -rn "$v\b" impulse-gui/src/ | grep -v "fn ui\|impl View\|struct $v\|^impulse-gui/src/views/${v,,}.rs"
done
# Expected: only self-references in each file

# memory_persistence: who imports it?
grep -rn "use crate::views::memory_persistence\|super::memory_persistence" impulse-gui/src/
# Expected: only context.rs, terminal_context.rs, terminal_insights.rs (all orphans)
```

If any of those commands surface a *non-orphan* caller, this audit is wrong and the file should be re-classified before deletion.

## Surviving view dependency tree (for the porting plan)

```
ViewId::Overview      → OverviewView           [294 LOC]
ViewId::Agents        → TerminalsView          [1,767 LOC]
                          └─ TerminalSearch    [embedded; 425 LOC]
                          └─ uses: impulse-term TerminalPanel + TerminalTheme (egui — replace with impulse-term-dioxus)
                          └─ uses: widgets::conflict_banner [65 LOC]
ViewId::Memory        → MemoryView             [101 LOC, thin wrapper]
                          ├─ GenomeView        [embedded; 341 LOC]
                          ├─ SearchView        [embedded; 213 LOC, uses ipc::SearchResult]
                          └─ SessionsView      [embedded; 638 LOC, uses ipc::Session, HistoryEntry]
ViewId::Settings      → SettingsView           [823 LOC]
```

Plus widgets actually used by app.rs:
- `command_palette` (325 LOC) — Ctrl+P palette
- `notifications` (350 LOC) — toast manager
- `project_selector` (214 LOC)
- `signal_bus` (807 LOC) — cross-pane event bus
- `sidebar` (164 LOC)
- `status_bar` (163 LOC)
- `conflict_banner` (65 LOC) — used by terminals.rs only
- agent_panel/ subdir (5 files) + ipc/ subdir (3 files)

## Recommended porting order (for Tranche B of Plan 2)

Order chosen to ship demonstrable user value early (a usable supervisor) before tackling the large views.

1. **sidebar + status_bar + command_palette** — app shell. Once this lands, the supervisor *looks* like an app instead of just a terminal. (~650 LOC egui → similar Dioxus LOC)
2. **memory.rs (wrapper) + sessions.rs** — sessions is the highest-value view (browse past sessions, commit history). 739 LOC.
3. **genome + search** — read-only memory views. 554 LOC combined.
4. **settings.rs** — config UI. 823 LOC, mostly forms — straightforward Dioxus.
5. **terminals.rs migration** — replace TerminalPanel with PtyTerminalView + BlockListView from impulse-term-dioxus. The 1,767 LOC of egui logic mostly becomes wiring; the heavy rendering already exists in the Dioxus crate.
6. **overview.rs** — the workbench dashboard. 294 LOC. Often last because it depends on the other views being navigable.

## What this audit does NOT decide

- The architectural choice of in-place rewrite vs. parallel `impulse-gui-dx` crate (Plan 2 reconciliation answers: in-place — grow `impulse-supervisor` to absorb these views, then delete `impulse-gui`).
- Whether to keep ratatui TUI (Plan 2 reconciliation answers: yes, separate binary, separate audience).
- The `@impulse` wire format details (Plan 2 reconciliation answers: text protocol over PTY input, intercepted by hook, JSON-line over `IMPULSE_CMD_SOCKET`).
