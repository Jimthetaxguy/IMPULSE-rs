# Impulse Redesign — PTY Fix + Full UI Overhaul

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix terminal text corruption caused by PTY write races, then redesign the entire GUI with a space-themed "Launch" design language, tightened layout, and switchable color themes.

**Architecture:** Two sequential workstreams. WS1 adds a `WriteQueue` to `TerminalBackend` that serializes all PTY writes and makes injection atomic. WS2 replaces the color palette with a theme system (`ColorPalette` struct), reduces sidebar views from 7 to 4, moves the agent panel to the right side, and polishes all widgets. Theme persistence via `config.json`.

**Tech Stack:** Rust, egui/eframe 0.31, parking_lot, vt100, portable_pty, serde

---

## File Map

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `impulse-term/src/backend.rs` | Add `WriteQueue` struct, expose `write_user_input()` / `write_injection()` |
| Modify | `impulse-term/src/context.rs` | Route injection through `WriteQueue` |
| Modify | `impulse-term/src/panel.rs` | Route keyboard/paste input through `WriteQueue`, pass `WriteQueue` ref |
| Modify | `impulse-term/src/theme.rs` | Add `ColorPalette` with 4 named themes, `from_name()` constructor |
| Modify | `impulse-gui/src/theme.rs` | Replace hardcoded `colors` module with `ColorPalette`-driven palette |
| Modify | `impulse-gui/src/views/mod.rs` | Reduce `ViewId` from 7 to 4 variants |
| Modify | `impulse-gui/src/widgets/sidebar.rs` | Update for 4 views, rocket logo, new palette |
| Modify | `impulse-gui/src/agent_panel/mod.rs` | Widget polish (spacing, corner radius, accent colors) |
| Modify | `impulse-gui/src/app.rs` | Move agent panel to right side, remove pruned views, wire theme switching |
| Modify | `impulse-gui/src/views/settings.rs` | Add theme selector dropdown |
| Modify | `impulse-gui/src/views/memory.rs` | Absorb genome + sessions content |
| Delete | `impulse-gui/src/views/context.rs` | Removed (experimental, unused) |
| Delete | `impulse-gui/src/views/artifacts.rs` | Removed (experimental, unused) |
| Delete | `impulse-gui/src/views/genome.rs` | Merged into memory view |
| Delete | `impulse-gui/src/views/sessions.rs` | Merged into memory view |
| Modify | `impulse-gui/src/views/terminals.rs` | Tab styling, status bar polish |

---

## WS1: PTY Write Serialization

### Task 1: WriteQueue — tests and struct

**Files:**
- Modify: `impulse-term/src/backend.rs`

- [ ] **Step 1: Add WriteQueue struct and tests at the bottom of backend.rs**

Add this after the `pty_reader_loop` function:

```rust
/// Serializes all writes to the PTY, preventing message-level interleaving
/// between user input, context injection, and lifecycle writes.
pub struct WriteQueue {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Timestamp of the last user input write (epoch millis).
    last_user_input: Arc<AtomicU64>,
}

/// Minimum quiet period (ms) after user input before injection is allowed.
const INJECTION_QUIET_MS: u64 = 500;

impl WriteQueue {
    pub fn new(writer: Arc<Mutex<Box<dyn Write + Send>>>) -> Self {
        Self {
            writer,
            last_user_input: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Write user keyboard input — always succeeds, updates last-input timestamp.
    pub fn write_user_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_user_input.store(now, Ordering::Relaxed);
        Ok(())
    }

    /// Write injected context — blocked if user input occurred within INJECTION_QUIET_MS.
    /// Writes start marker + content + end marker in a single lock acquisition.
    pub fn write_injection(&self, data: &[u8]) -> Result<bool, Box<dyn std::error::Error>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last = self.last_user_input.load(Ordering::Relaxed);
        if now.saturating_sub(last) < INJECTION_QUIET_MS {
            return Ok(false); // skipped — user is actively typing
        }
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(true)
    }
}
```

- [ ] **Step 2: Add tests for WriteQueue**

Add to the existing `#[cfg(test)] mod tests` (create one if none exists at the bottom of backend.rs):

```rust
#[test]
fn test_write_queue_user_input_succeeds() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(Box::new(SharedBuf(Arc::clone(&buf)))));
    let wq = WriteQueue::new(writer);
    wq.write_user_input(b"hello").unwrap();
    assert_eq!(&*buf.lock(), b"hello");
}

#[test]
fn test_write_queue_injection_blocked_after_input() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(Box::new(SharedBuf(Arc::clone(&buf)))));
    let wq = WriteQueue::new(writer);
    // Simulate recent user input.
    wq.write_user_input(b"x").unwrap();
    // Injection should be blocked (within 500ms).
    let injected = wq.write_injection(b"context").unwrap();
    assert!(!injected, "injection should be blocked right after user input");
    // Only user input should be in the buffer.
    assert_eq!(&*buf.lock(), b"x");
}

/// Shared buffer helper for testing WriteQueue without a real PTY.
struct SharedBuf(Arc<Mutex<Vec<u8>>>);
impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd impulse-rs && cargo test -p impulse-term -- write_queue`
Expected: 2 tests pass

- [ ] **Step 4: Commit**

```bash
git add impulse-term/src/backend.rs
git commit -m "feat(term): add WriteQueue for serialized PTY writes"
```

---

### Task 2: Wire WriteQueue into TerminalBackend

**Files:**
- Modify: `impulse-term/src/backend.rs`

- [ ] **Step 1: Add WriteQueue field to TerminalBackend**

In the `TerminalBackend` struct, add:

```rust
write_queue: WriteQueue,
```

In `spawn()`, after creating the writer Arc, create the WriteQueue:

```rust
let writer_arc = Arc::new(Mutex::new(writer));
let write_queue = WriteQueue::new(Arc::clone(&writer_arc));
```

Add `write_queue` to the `Self { ... }` construction. Keep the existing `writer` field for now (the WriteQueue wraps the same Arc).

- [ ] **Step 2: Add public accessors**

```rust
/// Access the write queue for serialized PTY writes.
pub fn write_queue(&self) -> &WriteQueue {
    &self.write_queue
}
```

- [ ] **Step 3: Deprecate direct write_input**

Add a doc comment to the existing `write_input`:

```rust
/// Write raw bytes to the PTY.
///
/// **Prefer `write_queue().write_user_input()` or `write_queue().write_injection()`**
/// for proper serialization. This method is retained for backwards compatibility
/// during the migration.
pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
```

- [ ] **Step 4: Run full test suite**

Run: `cd impulse-rs && cargo test -p impulse-term`
Expected: All existing tests pass (no behavioral change yet)

- [ ] **Step 5: Commit**

```bash
git add impulse-term/src/backend.rs
git commit -m "feat(term): wire WriteQueue into TerminalBackend"
```

---

### Task 3: Route panel input through WriteQueue

**Files:**
- Modify: `impulse-term/src/panel.rs`

- [ ] **Step 1: Replace write_input calls in handle_input**

In `handle_input()`, replace all `self.backend.write_input(...)` calls with `self.backend.write_queue().write_user_input(...)`:

Line ~344 (key input):
```rust
if let Some(bytes) = input::key_to_pty_bytes(key, modifiers, app_cursor) {
    let _ = self.backend.write_queue().write_user_input(&bytes);
}
```

Line ~364 (text input):
```rust
for text in &text_events {
    let _ = self.backend.write_queue().write_user_input(text.as_bytes());
}
```

Line ~379 (paste):
```rust
if let Some(text) = paste_text {
    let pasted = input::bracketed_paste(&text);
    let _ = self.backend.write_queue().write_user_input(&pasted);
}
```

- [ ] **Step 2: Route context injection through WriteQueue**

In `context_bridge()` — the `inject_context` method in `context.rs` currently calls `self.backend.write_input()`. Change `ContextBridge` to accept a reference to the `WriteQueue` instead.

In `context.rs`, change the `inject_context` method:

```rust
pub fn inject_context(&mut self, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(last) = self.last_injection_at {
        if last.elapsed().as_secs() < INJECTION_DEBOUNCE_SECS {
            return Ok(());
        }
    }

    let wrapped = self.wrap_injection(content);
    let pasted = crate::input::bracketed_paste(&wrapped);
    // Use write_injection — skips if user typed recently, writes atomically.
    let injected = self.backend.write_queue().write_injection(&pasted)?;

    if injected {
        self.last_injection_at = Some(Instant::now());
        self.injection_count += 1;
    }
    Ok(())
}
```

- [ ] **Step 3: Update TerminalPanel::write_input to use WriteQueue**

In `panel.rs`, the public `write_input` method:

```rust
pub fn write_input(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    self.backend.write_queue().write_user_input(data)
}
```

- [ ] **Step 4: Run full workspace tests**

Run: `cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings`
Expected: All 1,344+ tests pass, zero warnings

- [ ] **Step 5: Commit**

```bash
git add impulse-term/src/panel.rs impulse-term/src/context.rs
git commit -m "feat(term): route all PTY writes through WriteQueue"
```

---

## WS2: UI Redesign

### Task 4: ColorPalette theme system

**Files:**
- Modify: `impulse-gui/src/theme.rs`

- [ ] **Step 1: Write tests for ColorPalette**

Replace the entire contents of `impulse-gui/src/theme.rs`. Keep the module doc and imports, replace the `colors` module and `apply_dark_theme` with:

```rust
//! Theme system for Impulse GUI.
//!
//! Provides `ColorPalette` — a complete set of semantic color tokens.
//! Four named themes ship: Launch (default), Nebula, Solar, Aurora.
//! The active palette is stored in config.json and switchable at runtime.

use eframe::egui;
use serde::{Deserialize, Serialize};

/// Semantic color palette for the entire GUI.
#[derive(Clone, Debug)]
pub struct ColorPalette {
    // Backgrounds
    pub bg_deep: egui::Color32,
    pub bg_surface: egui::Color32,
    pub bg_hover: egui::Color32,
    pub border: egui::Color32,

    // Accent
    pub accent: egui::Color32,
    pub accent_bright: egui::Color32,
    pub accent_dim: egui::Color32,

    // Text
    pub text: egui::Color32,
    pub text_muted: egui::Color32,
    pub text_dim: egui::Color32,
    pub text_faint: egui::Color32,

    // Status
    pub green: egui::Color32,
    pub yellow: egui::Color32,
    pub red: egui::Color32,
    pub blue: egui::Color32,
}

/// Theme name — persisted to config.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Launch,
    Nebula,
    Solar,
    Aurora,
}

impl ThemeName {
    pub fn all() -> &'static [ThemeName] {
        &[ThemeName::Launch, ThemeName::Nebula, ThemeName::Solar, ThemeName::Aurora]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Launch => "Launch",
            Self::Nebula => "Nebula",
            Self::Solar => "Solar",
            Self::Aurora => "Aurora",
        }
    }

    pub fn palette(&self) -> ColorPalette {
        match self {
            Self::Launch => ColorPalette::launch(),
            Self::Nebula => ColorPalette::nebula(),
            Self::Solar => ColorPalette::solar(),
            Self::Aurora => ColorPalette::aurora(),
        }
    }
}

impl ColorPalette {
    pub fn launch() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x02, 0x06, 0x17),
            bg_surface: egui::Color32::from_rgb(0x0c, 0x1a, 0x3d),
            bg_hover: egui::Color32::from_rgb(0x1e, 0x3a, 0x5f),
            border: egui::Color32::from_rgba_premultiplied(0x3b, 0x82, 0xf6, 0x26),
            accent: egui::Color32::from_rgb(0x3b, 0x82, 0xf6),
            accent_bright: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
            accent_dim: egui::Color32::from_rgb(0x1d, 0x4e, 0xd8),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    pub fn nebula() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x0a, 0x00, 0x15),
            bg_surface: egui::Color32::from_rgb(0x1a, 0x0a, 0x2e),
            bg_hover: egui::Color32::from_rgb(0x2e, 0x1a, 0x47),
            border: egui::Color32::from_rgba_premultiplied(0x8b, 0x5c, 0xf6, 0x26),
            accent: egui::Color32::from_rgb(0x7c, 0x3a, 0xed),
            accent_bright: egui::Color32::from_rgb(0xa7, 0x8b, 0xfa),
            accent_dim: egui::Color32::from_rgb(0x5b, 0x21, 0xb6),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    pub fn solar() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x0c, 0x0a, 0x00),
            bg_surface: egui::Color32::from_rgb(0x1a, 0x10, 0x00),
            bg_hover: egui::Color32::from_rgb(0x2e, 0x1f, 0x00),
            border: egui::Color32::from_rgba_premultiplied(0xf5, 0x9e, 0x0b, 0x26),
            accent: egui::Color32::from_rgb(0xd9, 0x77, 0x06),
            accent_bright: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            accent_dim: egui::Color32::from_rgb(0x92, 0x40, 0x0e),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }

    pub fn aurora() -> Self {
        Self {
            bg_deep: egui::Color32::from_rgb(0x00, 0x0a, 0x0a),
            bg_surface: egui::Color32::from_rgb(0x00, 0x1a, 0x1a),
            bg_hover: egui::Color32::from_rgb(0x00, 0x2e, 0x2e),
            border: egui::Color32::from_rgba_premultiplied(0x10, 0xb9, 0x81, 0x26),
            accent: egui::Color32::from_rgb(0x05, 0x96, 0x69),
            accent_bright: egui::Color32::from_rgb(0x6e, 0xe7, 0xb7),
            accent_dim: egui::Color32::from_rgb(0x04, 0x73, 0x57),
            text: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
            text_muted: egui::Color32::from_rgb(0x94, 0xa3, 0xb8),
            text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
            text_faint: egui::Color32::from_rgb(0x33, 0x41, 0x55),
            green: egui::Color32::from_rgb(0x4a, 0xde, 0x80),
            yellow: egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
            red: egui::Color32::from_rgb(0xf8, 0x71, 0x71),
            blue: egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
        }
    }
}

/// Apply a ColorPalette to the egui context visuals.
pub fn apply_theme(ctx: &egui::Context, palette: &ColorPalette) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = palette.bg_deep;
    visuals.window_fill = palette.bg_surface;
    visuals.extreme_bg_color = palette.bg_deep;
    visuals.faint_bg_color = palette.bg_surface;

    visuals.widgets.noninteractive.bg_fill = palette.bg_surface;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette.text);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, palette.border);

    visuals.widgets.inactive.bg_fill = palette.bg_surface;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette.text);

    visuals.widgets.hovered.bg_fill = palette.bg_hover;
    visuals.widgets.active.bg_fill = palette.bg_hover;

    visuals.selection.bg_fill = egui::Color32::from_rgba_premultiplied(
        palette.accent.r(),
        palette.accent.g(),
        palette.accent.b(),
        0x40,
    );

    ctx.set_visuals(visuals);
}

/// Return a color associated with an agent name for tab/button rendering.
pub fn agent_color(name: &str) -> egui::Color32 {
    impulse_term::theme::agent_color(name)
}

// Backwards-compat shim — existing code references `colors::ACCENT` etc.
// These resolve from the Launch palette. Migrate callers to use palette directly.
pub mod colors {
    use super::*;
    pub const BG: egui::Color32 = egui::Color32::from_rgb(0x02, 0x06, 0x17);
    pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x0c, 0x1a, 0x3d);
    pub const HOVER: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x3a, 0x5f);
    pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x29, 0x3b);
    pub const ACTIVE_BG: egui::Color32 = egui::Color32::from_rgb(0x0f, 0x17, 0x2a);
    pub const ACTIVE_AGENT_BG: egui::Color32 = egui::Color32::from_rgb(0x0c, 0x1a, 0x2e);
    pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe2, 0xe8, 0xf0);
    pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(0x94, 0xa3, 0xb8);
    pub const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x64, 0x74, 0x8b);
    pub const TEXT_FAINT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x41, 0x55);
    pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
    pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x4a, 0xde, 0x80);
    pub const YELLOW: egui::Color32 = egui::Color32::from_rgb(0xfb, 0xbf, 0x24);
    pub const RED: egui::Color32 = egui::Color32::from_rgb(0xf8, 0x71, 0x71);
    pub const BLUE: egui::Color32 = egui::Color32::from_rgb(0x60, 0xa5, 0xfa);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_name_all_has_four() {
        assert_eq!(ThemeName::all().len(), 4);
    }

    #[test]
    fn test_theme_name_default_is_launch() {
        assert_eq!(ThemeName::default(), ThemeName::Launch);
    }

    #[test]
    fn test_each_theme_builds_valid_palette() {
        for name in ThemeName::all() {
            let p = name.palette();
            assert_ne!(p.bg_deep, p.accent, "bg and accent should differ for {:?}", name);
            assert_ne!(p.text, p.bg_deep, "text and bg should differ for {:?}", name);
        }
    }

    #[test]
    fn test_theme_name_serde_round_trip() {
        for name in ThemeName::all() {
            let json = serde_json::to_string(name).unwrap();
            let recovered: ThemeName = serde_json::from_str(&json).unwrap();
            assert_eq!(*name, recovered);
        }
    }

    #[test]
    fn test_launch_palette_is_blue() {
        let p = ColorPalette::launch();
        // Blue channel of accent should be > red and green
        assert!(p.accent.b() > p.accent.r());
        assert!(p.accent.b() > p.accent.g());
    }

    #[test]
    fn test_colors_compat_matches_launch() {
        let p = ColorPalette::launch();
        assert_eq!(colors::ACCENT, p.accent);
        assert_eq!(colors::BG, p.bg_deep);
        assert_eq!(colors::TEXT, p.text);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd impulse-rs && cargo test -p impulse-gui -- theme`
Expected: 6 tests pass

- [ ] **Step 3: Commit**

```bash
git add impulse-gui/src/theme.rs
git commit -m "feat(gui): add ColorPalette theme system with 4 named themes"
```

---

### Task 5: Prune ViewId from 7 to 4

**Files:**
- Modify: `impulse-gui/src/views/mod.rs`
- Modify: `impulse-gui/src/app.rs`

- [ ] **Step 1: Update ViewId enum**

In `views/mod.rs`, replace the `ViewId` enum and its impls:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewId {
    Overview,
    Agents,
    Memory,
    Settings,
}

impl ViewId {
    pub fn all() -> &'static [ViewId] {
        &[
            ViewId::Overview,
            ViewId::Agents,
            ViewId::Memory,
            ViewId::Settings,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ViewId::Overview => "Workbench",
            ViewId::Agents => "Terminals",
            ViewId::Memory => "Memory",
            ViewId::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ViewId::Overview => "\u{1F680}", // 🚀
            ViewId::Agents => "\u{2328}",    // ⌨
            ViewId::Memory => "\u{1F9E0}",   // 🧠
            ViewId::Settings => "\u{2699}",  // ⚙
        }
    }

    pub fn shortcut_label(&self) -> &'static str {
        match self {
            ViewId::Overview => "Ctrl+1",
            ViewId::Agents => "Ctrl+2",
            ViewId::Memory => "Ctrl+3",
            ViewId::Settings => "Ctrl+4",
        }
    }
}
```

Remove the module declarations for pruned views:
```rust
// Remove these lines:
// pub mod artifacts;
// pub mod context;
// pub mod genome;
// pub mod sessions;
```

Keep: `terminal_context`, `terminal_insights`, `terminal_search` (used by terminals view).

- [ ] **Step 2: Update app.rs — remove pruned view fields and match arms**

In `ImpulseApp` struct, remove:
- `context: ContextView`
- `artifacts: ArtifactsView`
- `guardrails: GuardrailsView`

Remove their `use` imports. Remove their construction in `new()`. Remove their match arms in the `update()` method's view dispatch.

Update keyboard shortcut handling: Ctrl+3 → Memory, Ctrl+4 → Settings. Remove Ctrl+5/6/7 view shortcuts.

- [ ] **Step 3: Build and fix compile errors**

Run: `cd impulse-rs && cargo build -p impulse-gui 2>&1 | head -40`

Fix any remaining references to removed variants. The compiler will guide you — each `ViewId::Context`, `ViewId::Artifacts`, `ViewId::Guardrails` reference needs removal.

- [ ] **Step 4: Run tests**

Run: `cd impulse-rs && cargo test`
Expected: Some tests referencing removed views will fail — delete those tests.

- [ ] **Step 5: Commit**

```bash
git add impulse-gui/src/views/mod.rs impulse-gui/src/app.rs
git commit -m "refactor(gui): prune views from 7 to 4 (Overview, Terminals, Memory, Settings)"
```

---

### Task 6: Move agent panel to right side

**Files:**
- Modify: `impulse-gui/src/app.rs`

- [ ] **Step 1: Change agent panel from left to right in the layout**

In `app.rs`, find where the agent panel is rendered. It currently uses a left-side panel. Change to:

```rust
if self.agent_visible {
    egui::SidePanel::right("agent_panel")
        .resizable(true)
        .default_width(220.0)
        .width_range(160.0..=320.0)
        .show(ctx, |ui| {
            self.agent_panel.ui(ui, ctx);
        });
}
```

- [ ] **Step 2: Build and verify**

Run: `cd impulse-rs && cargo build -p impulse-gui`
Expected: Compiles cleanly

- [ ] **Step 3: Commit**

```bash
git add impulse-gui/src/app.rs
git commit -m "refactor(gui): move agent panel from left to right side"
```

---

### Task 7: Sidebar polish — rocket logo + new palette

**Files:**
- Modify: `impulse-gui/src/widgets/sidebar.rs`

- [ ] **Step 1: Update sidebar with rocket branding and new colors**

Update the logo section to show a rocket:

```rust
// Logo / brand.
if expanded {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("\u{1F680}").size(18.0)); // 🚀
        ui.strong(egui::RichText::new("IMPULSE").color(colors::ACCENT));
    });
} else {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("\u{1F680}").size(20.0)); // 🚀
    });
}
```

Update width constants:

```rust
const COLLAPSED_WIDTH: f32 = 44.0;
const EXPANDED_WIDTH: f32 = 180.0;
```

Update the active state background to use the new palette:

```rust
let btn = egui::Button::new(egui::RichText::new(&text).color(color))
    .fill(if is_active {
        colors::SURFACE // was ACTIVE_BG
    } else {
        egui::Color32::TRANSPARENT
    })
    .corner_radius(egui::CornerRadius::same(6))
    .min_size(egui::vec2(if expanded { EXPANDED_WIDTH - 16.0 } else { 32.0 }, 32.0));
```

- [ ] **Step 2: Build and verify**

Run: `cd impulse-rs && cargo build -p impulse-gui`

- [ ] **Step 3: Commit**

```bash
git add impulse-gui/src/widgets/sidebar.rs
git commit -m "style(gui): sidebar rocket logo, new palette colors, tighter layout"
```

---

### Task 8: Agent panel widget polish

**Files:**
- Modify: `impulse-gui/src/agent_panel/mod.rs`

- [ ] **Step 1: Polish the header**

Update the header accent bar color and supervisor label:

```rust
ui.horizontal(|ui| {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, colors::ACCENT);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("\u{1F680}").size(12.0)); // 🚀
    ui.strong(egui::RichText::new("Supervisor").size(13.0).color(colors::ACCENT));
```

- [ ] **Step 2: Polish the input bar**

Update corner radius and colors:

```rust
egui::Frame::new()
    .fill(colors::BG)
    .inner_margin(egui::Margin::symmetric(8, 6))
    .corner_radius(egui::CornerRadius::same(8))
    .stroke(egui::Stroke::new(1.0, colors::BORDER))
```

- [ ] **Step 3: Build and run tests**

Run: `cd impulse-rs && cargo build && cargo test -p impulse-gui`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add impulse-gui/src/agent_panel/mod.rs
git commit -m "style(gui): polish agent panel header, input bar, and spacing"
```

---

### Task 9: Wire theme switching in Settings + app.rs

**Files:**
- Modify: `impulse-gui/src/views/settings.rs`
- Modify: `impulse-gui/src/app.rs`

- [ ] **Step 1: Add theme selector to Settings view**

Add a `ThemeName` field to `SettingsView`. In the settings UI, add a combo box:

```rust
ui.horizontal(|ui| {
    ui.label("Theme:");
    egui::ComboBox::from_id_salt("theme_selector")
        .selected_text(self.active_theme.label())
        .show_ui(ui, |ui| {
            for theme in crate::theme::ThemeName::all() {
                ui.selectable_value(&mut self.active_theme, *theme, theme.label());
            }
        });
});
```

- [ ] **Step 2: Apply theme on change in app.rs**

In `ImpulseApp::update()`, check if the settings theme changed and call `apply_theme()`:

```rust
let current_theme = self.settings.active_theme();
if current_theme != self.last_applied_theme {
    crate::theme::apply_theme(ctx, &current_theme.palette());
    self.last_applied_theme = current_theme;
}
```

Add `last_applied_theme: ThemeName` field to `ImpulseApp`.

- [ ] **Step 3: Apply default theme on startup**

In `ImpulseApp::new()`:

```rust
crate::theme::apply_theme(&cc.egui_ctx, &crate::theme::ThemeName::default().palette());
```

- [ ] **Step 4: Build and test**

Run: `cd impulse-rs && cargo build && cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add impulse-gui/src/views/settings.rs impulse-gui/src/app.rs
git commit -m "feat(gui): theme switching in Settings view with 4 named themes"
```

---

### Task 10: Final verification + cleanup

**Files:**
- Modify: `impulse-rs/CLAUDE.md` (update metrics)

- [ ] **Step 1: Full verification gate**

Run:
```bash
cd impulse-rs && cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Expected: All pass, zero warnings.

- [ ] **Step 2: Update test counts in CLAUDE.md**

Update the test count line to reflect the new total (will be different from 1,025 after adding WriteQueue tests and removing pruned view tests).

- [ ] **Step 3: Visual verification**

```bash
cd impulse-rs && cargo run -p impulse-gui
```

Check:
- Launch theme renders (blue-bright, deep navy background)
- Rocket emoji in sidebar
- 4 views only (Workbench, Terminals, Memory, Settings)
- Agent panel on right side
- Terminal input doesn't corrupt when injection fires
- Theme switching works in Settings

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: update CLAUDE.md metrics after redesign"
```
