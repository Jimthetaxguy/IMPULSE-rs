# Impulse Redesign — PTY Fix + Full UI Overhaul

## Summary

Two workstreams executed together: (1) fix the PTY write race conditions that cause terminal text corruption, and (2) full UI redesign with a "Launch" design language — space-themed, blue-bright, rocket branding, switchable color themes, tightened layout, and pruned navigation.

## Workstream 1: PTY Write Serialization

### Problem

Three independent code paths write to the same PTY without coordination:
- Keyboard input (`panel.rs` → `backend.write_input()`)
- Context injection (`context.rs` → `inject_context()` → `backend.write_input()`)
- Startup injection (`lifecycle.rs` → `pane.write_input()`)

The writer `Mutex` prevents byte-level corruption but not message-level interleaving. User typing "npm test" can have injection text spliced into the middle.

Additionally, bracketed paste (`\x1b[200~...\x1b[201~`) is not atomic — user keystrokes between start and end markers get included in the paste.

### Solution: WriteQueue

Add a `WriteQueue` to `TerminalBackend` that serializes all write operations:

```rust
pub struct WriteQueue {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    busy: AtomicBool, // true while a multi-part write (bracketed paste) is in progress
}

impl WriteQueue {
    /// Write user keyboard input — highest priority, never blocked.
    pub fn write_input(&self, data: &[u8]) -> Result<()>;

    /// Write injected context — blocked while user input is active.
    /// Uses bracketed paste atomically (start + content + end in one lock acquisition).
    pub fn write_injection(&self, data: &[u8]) -> Result<()>;
}
```

Key invariants:
- `write_injection` acquires the lock, writes start marker + content + end marker, then releases. No window for interleaving.
- Injection is debounced (existing 60s) AND skipped if user input occurred in the last 500ms.
- All three write paths go through `WriteQueue` — no direct `backend.write_input()` calls from outside.

### Files to Modify

| File | Change |
|------|--------|
| `impulse-term/src/backend.rs` | Add `WriteQueue`, expose `write_user_input()` and `write_injection()` |
| `impulse-term/src/context.rs` | Route `inject_context()` through `WriteQueue::write_injection()` |
| `impulse-term/src/panel.rs` | Route keyboard/paste input through `WriteQueue::write_user_input()` |
| `impulse-gui/src/views/terminals.rs` | Route `send_to_tab()` / `inject_to_tab()` through WriteQueue |
| `impulse-rs/src/ui/lifecycle.rs` | Route startup injection through WriteQueue |

### Tests

- Concurrent write ordering test (spawn threads, verify no interleaving)
- Injection blocked during active input test
- Bracketed paste atomicity test (start + content + end in single lock)

---

## Workstream 2: UI Redesign

### Design Language: "Launch"

**Identity:** Space-themed mission control. Rocket ship logo. Blue-bright on deep navy. The terminal is the cockpit window — everything else is instrumentation.

**Base Palette (Launch):**
| Token | Value | Usage |
|-------|-------|-------|
| `bg_deep` | `#020617` | App background, terminal bg |
| `bg_surface` | `#0c1a3d` | Panels, cards, elevated surfaces |
| `bg_hover` | `#1e3a5f` | Interactive hover state |
| `border` | `rgba(59,130,246,0.15)` | Subtle panel borders |
| `accent` | `#3b82f6` | Primary interactive elements |
| `accent_bright` | `#60a5fa` | Highlights, active states, cursor |
| `accent_glow` | `rgba(59,130,246,0.4)` | Logo glow, focus rings |
| `text` | `#e2e8f0` | Primary text |
| `text_muted` | `#94a3b8` | Secondary text |
| `text_dim` | `#64748b` | Disabled, placeholder |
| `green` | `#4ade80` | Success, connected, tests pass |
| `yellow` | `#fbbf24` | Warning, thinking |
| `red` | `#f87171` | Error, danger |

**Gradient accents:** Linear gradients on sidebar logo, active tab indicator, agent panel header bar. Gradient direction: 135deg (top-left to bottom-right).

**Alternative themes** (same layout, different `ColorPalette`):
- **Nebula:** Purple-violet (#7c3aed / #a78bfa) on deep plum (#0a0015)
- **Solar:** Amber-gold (#d97706 / #fbbf24) on dark brown (#0c0a00)
- **Aurora:** Emerald-cyan (#059669 / #6ee7b7) on deep ocean (#000a0a)

Theme stored in `config.json`, switchable in Settings view.

### Logo

Rocket ship sprite (🚀 emoji initially, custom pixel-art `.png` later). Displayed in sidebar at 28x28px with colored glow shadow matching the active theme accent. The glow pulses subtly when the daemon is connected.

### Layout Changes

**Reduce sidebar views from 7 to 4:**
- **Workbench** (was Overview) — dashboard with terminal tabs
- **Terminals** (unchanged) — multi-pane terminal view
- **Memory** (absorbs Genome + Sessions) — combined memory/history view
- **Settings** (absorbs Guardrails) — config + safety rules

**Remove:** Context (exp), Artifacts (exp). These are unfinished and add clutter. If needed later, re-add when they're real.

**Sidebar:** 44px collapsed (icon-only), 180px expanded. Rocket logo at top. View icons below. Connection status dot at bottom.

**Agent Panel:** Right side (not left — terminal is primary, agent is secondary). 200px default width, resizable 160-320px. Collapsible with `Ctrl+5`.

**Terminal area:** Full remaining width. Tab bar at top with colored dot per agent type. Status bar at bottom (compact: connection + dimensions + context % + version).

### Typography & Spacing

- System monospace for terminal: `"JetBrains Mono", "SF Mono", "Cascadia Code", monospace`
- UI text: egui default (system sans-serif)
- Consistent 8px grid for all margins and padding
- Panel inner margins: 12px
- Section gaps: 16px
- Compact status bars: 24px height

### Widget Refinements

- Tab pills: rounded corners (6px), gradient accent on active tab border-bottom
- Buttons: 4px corner radius, subtle border, accent fill on primary actions
- Input fields: 8px corner radius, inner glow on focus (`accent_glow`)
- Chat bubbles (agent panel): 8px radius, darker bg for user messages, surface bg for agent responses
- Status dots: 6px diameter, colored by state

---

## Verification

After each phase:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Visual verification: launch app, check all 4 views render correctly, test theme switching, verify terminal input doesn't corrupt with injection active.

---

## Execution Order

1. **PTY WriteQueue** — fix the race conditions first (terminal must work before it can look good)
2. **Theme system** — `ColorPalette` struct + 4 themes + Settings selector
3. **Layout restructure** — sidebar reduction (7→4), agent panel to right side
4. **Widget polish** — tabs, buttons, inputs, chat bubbles, status bars
5. **Logo + branding** — rocket sprite, glow effect, sidebar treatment
6. **Prune dead views** — remove Context (exp), Artifacts (exp), clean up ViewId enum
