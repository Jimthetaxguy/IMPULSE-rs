# impulse-gui ⇢ Decouple from impulse-term (egui adapter) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sever every dependency from `impulse-gui` to `impulse-term` *except* the two that genuinely need egui (`TerminalPanel` widget, `TerminalTheme` egui-color struct). After this plan, `impulse-gui`'s only remaining tie to the egui adapter is the live terminal rendering surface — which Plan 2 (a separate, future plan) will replace with a Dioxus equivalent.

**Architecture:** All non-rendering shared types already live in `impulse-term-core` (re-exported by `impulse-term` for back-compat). This plan switches `impulse-gui`'s import paths from `impulse_term::context::*` to `impulse_term_core::context::*`, factors `agent_color` into a toolkit-neutral RGB triple in core (with a thin `egui::Color32` wrapper kept in the egui adapter for `impulse-term`'s own callers), and adds `impulse-term-core` as a direct dep of `impulse-gui`. No behavior changes; pure refactor verified by existing tests.

**Tech Stack:** Rust 2021, cargo workspace, vt100 (parser), egui/eframe (impulse-gui runtime — unchanged this plan), thiserror.

---

## Why this is the right next step

After Phase 7 (L155–L179) the workspace looks like:

```
impulse-term-core   (toolkit-neutral)  ← impulse-term-dioxus, impulse-term
impulse-term        (egui adapter)     ← impulse-gui                       ← impulse-supervisor (via dioxus crate)
impulse-gui         (egui workbench)   ← (binary)
impulse-supervisor  (dioxus shell)     ← (binary)
```

`impulse-gui` currently consumes `impulse-term` for **three reasons**, but only one of them actually needs egui:

| Import | Used in | Needs egui? |
|---|---|---|
| `impulse_term::context::*` (ContextTier, AgentKind, InsightType, ExtractedInsight, truncate_insight) | memory_persistence.rs, terminal_insights.rs, context.rs, terminal_context.rs, terminals.rs | ❌ no — already lives in core, just re-exported |
| `impulse_term::theme::agent_color` (returns `egui::Color32`) | impulse-gui/src/theme.rs:217 | ⚠️ partial — the *lookup* is RGB; only the return type needs egui |
| `impulse_term::TerminalPanel` + `impulse_term::theme::TerminalTheme` | views/terminals.rs | ✅ yes — this is the egui rendering widget |

This plan removes the first two reasons and leaves the third in place. After it lands, the path to retiring egui is unambiguous: replace `TerminalPanel` with the Dioxus equivalent, then delete `impulse-term`.

## File structure

| File | Status | Responsibility |
|---|---|---|
| `impulse-rs/impulse-term-core/src/theme.rs` | **NEW** | Toolkit-neutral agent color lookup returning `(u8, u8, u8)` |
| `impulse-rs/impulse-term-core/src/lib.rs` | MODIFY | Add `pub mod theme;` and re-export `agent_color_rgb` |
| `impulse-rs/impulse-term/src/theme.rs` | MODIFY | `agent_color()` becomes a 3-line wrapper that calls `impulse_term_core::theme::agent_color_rgb()` and converts to `egui::Color32` |
| `impulse-rs/impulse-gui/Cargo.toml` | MODIFY | Add `impulse-term-core = { path = "../impulse-term-core" }` |
| `impulse-rs/impulse-gui/src/theme.rs` | MODIFY | Line 217: replace `impulse_term::theme::agent_color(name)` with conversion from `impulse_term_core::theme::agent_color_rgb(name)` |
| `impulse-rs/impulse-gui/src/views/memory_persistence.rs` | MODIFY | Replace `impulse_term::context::*` with `impulse_term_core::context::*` (12 occurrences) |
| `impulse-rs/impulse-gui/src/views/terminal_insights.rs` | MODIFY | Same swap (1 occurrence) |
| `impulse-rs/impulse-gui/src/views/context.rs` | MODIFY | Same swap (3 occurrences) |
| `impulse-rs/impulse-gui/src/views/terminal_context.rs` | MODIFY | Same swap (5 occurrences) |
| `impulse-rs/impulse-gui/src/views/terminals.rs` | MODIFY | Replace `impulse_term::context::*` (line 13). Keep `impulse_term::TerminalPanel` and `impulse_term::theme::TerminalTheme` (egui-bound, future plan) |

**Out of scope for this plan:** anything that touches `TerminalPanel` or the egui-rendering path. Those move in Plan 2.

---

## Task 1: Toolkit-neutral agent color lookup in core

**Files:**
- Create: `impulse-rs/impulse-term-core/src/theme.rs`
- Modify: `impulse-rs/impulse-term-core/src/lib.rs`
- Test: same file (`mod tests`)

- [ ] **Step 1: Write the failing test**

Append to (new) `impulse-rs/impulse-term-core/src/theme.rs`:

```rust
//! Toolkit-neutral theming primitives.
//!
//! Color values are returned as `(u8, u8, u8)` RGB triples so callers in
//! either an egui or a Dioxus renderer can convert without depending on each
//! other's color types.

/// Look up the canonical accent color for a known agent name.
///
/// Returns an `(r, g, b)` triple. Unknown names return the default text
/// color (`#c9d1d9`) so callers always get something to render.
pub fn agent_color_rgb(name: &str) -> (u8, u8, u8) {
    match name {
        "Claude Code" => (0x8b, 0x5c, 0xf6), // purple
        "OpenCode" => (0x3f, 0xb9, 0x50),    // green
        "Codex" => (0xd2, 0x99, 0x22),       // yellow
        "Shell" => (0x58, 0xa6, 0xff),       // blue
        _ => (0xc9, 0xd1, 0xd9),             // default text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_color_rgb_known_agents_match_legacy_palette() {
        // These RGB triples MUST match impulse-term/src/theme.rs:agent_color()
        // — that function will become a thin wrapper around this one.
        assert_eq!(agent_color_rgb("Claude Code"), (0x8b, 0x5c, 0xf6));
        assert_eq!(agent_color_rgb("OpenCode"), (0x3f, 0xb9, 0x50));
        assert_eq!(agent_color_rgb("Codex"), (0xd2, 0x99, 0x22));
        assert_eq!(agent_color_rgb("Shell"), (0x58, 0xa6, 0xff));
    }

    #[test]
    fn test_agent_color_rgb_unknown_agent_returns_default_text() {
        assert_eq!(agent_color_rgb("MysteryAgent"), (0xc9, 0xd1, 0xd9));
        assert_eq!(agent_color_rgb(""), (0xc9, 0xd1, 0xd9));
    }
}
```

- [ ] **Step 2: Run test to verify it fails (module not yet wired in lib.rs)**

Run: `cargo test -p impulse-term-core theme:: 2>&1 | tail -10`
Expected: compile error — `module not found` or `theme.rs is not a module of crate impulse_term_core`.

- [ ] **Step 3: Wire the module into lib.rs**

In `impulse-rs/impulse-term-core/src/lib.rs`, find the existing module list (around the top of the file) and add `theme`:

```rust
pub mod backend;
pub mod blocks;
pub mod context;
pub mod escape;
pub mod grid;
pub mod input;
pub mod osc133;
pub mod role;
pub mod theme; // NEW
```

Also add a re-export near the existing `pub use` block:

```rust
pub use theme::agent_color_rgb;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p impulse-term-core theme:: 2>&1 | tail -10`
Expected: `2 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
git add impulse-rs/impulse-term-core/src/theme.rs impulse-rs/impulse-term-core/src/lib.rs
git commit -m "feat(impulse-term-core): add toolkit-neutral agent_color_rgb"
```

---

## Task 2: Reduce impulse-term::theme::agent_color to a thin wrapper

**Files:**
- Modify: `impulse-rs/impulse-term/src/theme.rs:330-342`
- Test: same file (existing `test_agent_colors` already covers this)

- [ ] **Step 1: Verify the existing test exists and passes (baseline)**

Run: `cargo test -p impulse-term theme::tests::test_agent_colors 2>&1 | tail -5`
Expected: `1 passed`.

- [ ] **Step 2: Replace agent_color body with a wrapper call**

In `impulse-rs/impulse-term/src/theme.rs`, replace lines 330-342 with:

```rust
/// Return a color associated with an agent name for UI elements.
///
/// Thin egui-typed wrapper around `impulse_term_core::theme::agent_color_rgb`.
/// Kept so existing egui callers continue to receive `egui::Color32` directly.
pub fn agent_color(name: &str) -> egui::Color32 {
    let (r, g, b) = impulse_term_core::theme::agent_color_rgb(name);
    egui::Color32::from_rgb(r, g, b)
}
```

- [ ] **Step 3: Run the same test to verify behavior unchanged**

Run: `cargo test -p impulse-term theme::tests::test_agent_colors 2>&1 | tail -5`
Expected: `1 passed`. The test compares against `egui::Color32::from_rgb(0x8b, 0x5c, 0xf6)` etc., which must still match.

- [ ] **Step 4: Run the full impulse-term test suite as a regression check**

Run: `cargo test -p impulse-term 2>&1 | grep "test result:"`
Expected: same pass count as before (no failures, no skips).

- [ ] **Step 5: Commit**

```bash
git add impulse-rs/impulse-term/src/theme.rs
git commit -m "refactor(impulse-term): agent_color delegates to impulse-term-core"
```

---

## Task 3: Add impulse-term-core as a direct dep of impulse-gui

**Files:**
- Modify: `impulse-rs/impulse-gui/Cargo.toml`

- [ ] **Step 1: Add the dep line**

In `impulse-rs/impulse-gui/Cargo.toml`, locate the `[dependencies]` section. Find the line `impulse-term = { path = "../impulse-term" }` and add directly above it:

```toml
impulse-term-core = { path = "../impulse-term-core" }
impulse-term = { path = "../impulse-term" }
```

(Both deps coexist for now. Plan 2 will drop `impulse-term`.)

- [ ] **Step 2: Verify the workspace still builds**

Run: `cargo build -p impulse-gui 2>&1 | tail -5`
Expected: `Finished ... target(s)` with no errors. (No imports use `impulse_term_core` yet — Tasks 4–8 add them.)

- [ ] **Step 3: Commit**

```bash
git add impulse-rs/impulse-gui/Cargo.toml impulse-rs/Cargo.lock
git commit -m "chore(impulse-gui): add impulse-term-core dep"
```

---

## Task 4: Switch impulse-gui/src/theme.rs to call core directly

**Files:**
- Modify: `impulse-rs/impulse-gui/src/theme.rs:217`

- [ ] **Step 1: Inspect the current line for context**

Run: `sed -n '210,225p' impulse-rs/impulse-gui/src/theme.rs`
Expected output includes the line `impulse_term::theme::agent_color(name)`.

- [ ] **Step 2: Replace the call**

Find the line:

```rust
    impulse_term::theme::agent_color(name)
```

Replace with:

```rust
    let (r, g, b) = impulse_term_core::theme::agent_color_rgb(name);
    egui::Color32::from_rgb(r, g, b)
```

(impulse-gui already has egui in scope — `egui::Color32` resolves without a new import.)

- [ ] **Step 3: Run the impulse-gui test suite**

Run: `cargo test -p impulse-gui 2>&1 | grep "test result:"`
Expected: same pass count as baseline (refactor, no behavior change).

- [ ] **Step 4: Commit**

```bash
git add impulse-rs/impulse-gui/src/theme.rs
git commit -m "refactor(impulse-gui): theme uses impulse-term-core agent palette"
```

---

## Task 5: Switch memory_persistence.rs to impulse-term-core

**Files:**
- Modify: `impulse-rs/impulse-gui/src/views/memory_persistence.rs` (12 `impulse_term::context::` occurrences)

- [ ] **Step 1: Confirm baseline test count**

Run: `cargo test -p impulse-gui --test '*' 2>&1 | grep "test result:" | head -1`
Note the pass count.

- [ ] **Step 2: Sed-replace `impulse_term::context::` with `impulse_term_core::context::` in this file only**

Run: `sed -i.bak 's/impulse_term::context::/impulse_term_core::context::/g' impulse-rs/impulse-gui/src/views/memory_persistence.rs && rm impulse-rs/impulse-gui/src/views/memory_persistence.rs.bak`

- [ ] **Step 3: Verify no occurrences remain**

Run: `grep -n 'impulse_term::context::' impulse-rs/impulse-gui/src/views/memory_persistence.rs || echo "clean"`
Expected: `clean`.

- [ ] **Step 4: Run the workspace tests**

Run: `cargo test -p impulse-gui 2>&1 | grep "test result:"`
Expected: same pass count as Step 1.

- [ ] **Step 5: Commit**

```bash
git add impulse-rs/impulse-gui/src/views/memory_persistence.rs
git commit -m "refactor(impulse-gui): memory_persistence imports core context"
```

---

## Task 6: Switch the remaining four view files in one batch

**Files:**
- Modify: `impulse-rs/impulse-gui/src/views/terminal_insights.rs`
- Modify: `impulse-rs/impulse-gui/src/views/context.rs`
- Modify: `impulse-rs/impulse-gui/src/views/terminal_context.rs`
- Modify: `impulse-rs/impulse-gui/src/views/terminals.rs` (only the `impulse_term::context::` line — leave `TerminalPanel` and `TerminalTheme` alone)

These four files all do the same swap; batched into one task because each is mechanical and a single test run validates them together.

- [ ] **Step 1: Apply the import swap to all four**

Run:

```bash
for f in \
    impulse-rs/impulse-gui/src/views/terminal_insights.rs \
    impulse-rs/impulse-gui/src/views/context.rs \
    impulse-rs/impulse-gui/src/views/terminal_context.rs \
    impulse-rs/impulse-gui/src/views/terminals.rs; do
    sed -i.bak 's/impulse_term::context::/impulse_term_core::context::/g' "$f"
    rm "$f.bak"
done
```

- [ ] **Step 2: Verify no `impulse_term::context::` remains anywhere in impulse-gui**

Run: `grep -rn 'impulse_term::context::' impulse-rs/impulse-gui/src/ || echo "clean"`
Expected: `clean`.

- [ ] **Step 3: Verify `impulse_term::TerminalPanel` and `impulse_term::theme::TerminalTheme` are still present in terminals.rs**

Run: `grep -n 'TerminalPanel\|TerminalTheme' impulse-rs/impulse-gui/src/views/terminals.rs | head -5`
Expected: at least 3 hits including `use impulse_term::TerminalPanel;`. These are intentional — Plan 2 will remove them.

- [ ] **Step 4: Run the impulse-gui test suite**

Run: `cargo test -p impulse-gui 2>&1 | grep "test result:"`
Expected: same pass count as before.

- [ ] **Step 5: Commit**

```bash
git add impulse-rs/impulse-gui/src/views/terminal_insights.rs \
        impulse-rs/impulse-gui/src/views/context.rs \
        impulse-rs/impulse-gui/src/views/terminal_context.rs \
        impulse-rs/impulse-gui/src/views/terminals.rs
git commit -m "refactor(impulse-gui): four views import core context"
```

---

## Task 7: Workspace verification gate

**Files:** none modified — pure verification.

- [ ] **Step 1: cargo build --workspace**

Run: `cargo build --workspace 2>&1 | tail -3`
Expected: `Finished ...` with zero errors.

- [ ] **Step 2: cargo test --workspace**

Run: `cargo test --workspace 2>&1 | grep "test result:" | awk '{sum+=$4} END {print "Total: " sum " passed"}'`
Expected: ≥ 2,005 passed (2,003 baseline from L175 + 2 new tests from Task 1).

- [ ] **Step 3: cargo clippy --workspace -- -D warnings**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -3`
Expected: `Finished ...` with no warnings.

- [ ] **Step 4: cargo fmt --check**

Run: `cargo fmt --check`
Expected: no output.

- [ ] **Step 5: Per CLAUDE.md pre-commit checklist item #8 — runtime feature gate**

Run: `cargo build -p impulse-supervisor --features experimental-runtime 2>&1 | tail -3`
Expected: `Finished ...` with no errors. (This task touched no runtime code, but the gate is non-negotiable.)

Run: `cargo clippy -p impulse-supervisor --features experimental-runtime -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 6: Audit residual coupling**

Run: `grep -rn 'impulse_term::' impulse-rs/impulse-gui/src/ | grep -v 'impulse_term_core' | grep -v 'impulse-term-core' | sort -u`

Expected: only references to `impulse_term::TerminalPanel` and `impulse_term::theme::TerminalTheme` (and any helpers they expose). No `impulse_term::context::*`. If anything else appears, fix it before declaring victory.

---

## Task 8: Document the residual coupling for Plan 2

**Files:**
- Modify: `impulse-rs/CLAUDE.md` (Architecture section, the bulleted workspace inventory)

- [ ] **Step 1: Add a note next to the `impulse-term` line**

Find the line in the workspace inventory describing `impulse-term` (currently: "egui adapter for `impulse-term-core` (renderer, theme, panel, status bar, key shim)") and append:

```
   - **Remaining impulse-gui coupling (post-Plan-1):** only `TerminalPanel` widget and `TerminalTheme` egui-color struct. Plan 2 (`docs/plans/2026-XX-XX-impulse-gui-dioxus-terminal-view.md`) replaces these with the Dioxus equivalent.
```

- [ ] **Step 2: Commit**

```bash
git add impulse-rs/CLAUDE.md
git commit -m "docs(claude.md): record post-Plan-1 residual egui coupling"
```

---

## Done criteria

- All 8 tasks committed
- `cargo test --workspace` passes with ≥ 2,005 tests
- `grep -rn 'impulse_term::' impulse-rs/impulse-gui/src/` returns only `TerminalPanel` and `TerminalTheme` references
- Both `impulse-term-core` and `impulse-term` listed as deps in `impulse-gui/Cargo.toml`
- `impulse-supervisor` runtime gate still builds clean

## What this plan does NOT do (and what comes next)

- **Does not** remove `impulse-term` — `impulse-gui` still depends on it for `TerminalPanel`.
- **Does not** migrate any UI code from egui to Dioxus.
- **Does not** archive any crates.

The next plan ("Plan 2: Migrate impulse-gui terminal view to Dioxus") needs an architectural choice first:

- **Option A (in-place rewrite):** rewrite `impulse-gui` as a Dioxus app, replacing `eframe::App` with `dioxus::launch` and porting widgets one at a time behind feature flags. Risk: long-lived broken state.
- **Option B (parallel crate):** create `impulse-gui-dx` (new Dioxus crate) and grow it view-by-view to parity with `impulse-gui`. Switch the binary target when feature-complete. Risk: temporary code duplication.

Plan 2 should start with brainstorming this choice before writing tasks.
