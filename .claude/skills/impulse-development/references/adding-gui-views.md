# Retired EGUI View Guide

> **Do not follow this guide for new work.** `impulse-gui` is default-inactive legacy code pending
> gated physical retirement. The active desktop guide is
> [`adding-dioxus-views.md`](adding-dioxus-views.md). The content below remains only as recovery
> context for the pre-Dioxus implementation.

---

## Steps

### 1. Create the view module in `impulse-gui/src/views/`

```rust
// src/views/my_view.rs

use eframe::egui;

pub struct MyView {
    // View-specific state
    search_query: String,
    items: Vec<MyItem>,
}

impl Default for MyView {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            items: Vec::new(),
        }
    }
}

impl MyView {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &crate::state::SharedState) {
        // Header — always visible regardless of connection state
        ui.heading("My View");
        ui.separator();

        // Check daemon connection
        let data = match state.lock().ok() {
            Some(s) if s.is_connected => s.my_data.clone(),
            _ => {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 120, 50),
                    "Not connected to daemon",
                );
                return;
            }
        };

        // View content
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.search_query);
            if ui.button("Search").clicked() {
                self.items = filter_items(&data, &self.search_query);
            }
        });

        egui::ScrollArea::vertical()
            .id_source("my_view_scroll")
            .show(ui, |ui| {
                for (i, item) in self.items.iter().enumerate() {
                    ui.push_id(i, |ui| {
                        render_item(ui, item);
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_view_default() {
        let view = MyView::default();
        assert!(view.items.is_empty());
    }
}
```

**Key patterns:**
- Header + separator BEFORE connection check (view is always identifiable)
- `.lock().ok()` for SharedState (graceful mutex degradation)
- `id_source()` on ScrollArea (prevent ID collisions)
- `push_id(i, ...)` inside loops

### 2. Register the module in `views/mod.rs`

```rust
pub mod my_view;
pub use my_view::MyView;
```

### 3. Add ViewId variant

In `app.rs` or wherever ViewId is defined:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewId {
    // ... existing views ...
    MyView,
}
```

### 4. Add to ImpulseApp

```rust
pub struct ImpulseApp {
    // ... existing fields ...
    my_view: views::MyView,
}

// In Default/new:
my_view: views::MyView::default(),
```

### 5. Add view dispatch in update()

In the central panel rendering:

```rust
match self.active_view {
    // ... existing views ...
    ViewId::MyView => self.my_view.show(ui, &self.shared_state),
}
```

### 6. Add sidebar entry

In `widgets/sidebar.rs`:

```rust
if ui.selectable_label(
    *active_view == ViewId::MyView,
    "My View",
).clicked() {
    *active_view = ViewId::MyView;
}
```

Use `SelectableLabel` (not `Button`) — this is egui's standard navigation pattern.

### 7. Add keyboard shortcut

```rust
// In input handling (typically in app.rs)
if ctx.input(|i| i.key_pressed(egui::Key::Num6) && i.modifiers.ctrl) {
    self.active_view = ViewId::MyView;
}
```

Follow the existing Ctrl+1 through Ctrl+N pattern.

---

## Checklist

- [ ] View struct with `show(&mut self, ui, state)` method
- [ ] Header + separator shown before connection check
- [ ] Module registered in `views/mod.rs`
- [ ] ViewId variant added
- [ ] Field added to ImpulseApp
- [ ] View dispatch in update()
- [ ] Sidebar entry with SelectableLabel
- [ ] Keyboard shortcut (Ctrl+N)
- [ ] ScrollArea has id_source()
- [ ] Loops use push_id()
- [ ] SharedState accessed via .lock().ok()
- [ ] Tests for default state
- [ ] `cargo test` and `cargo clippy -- -D warnings` clean
