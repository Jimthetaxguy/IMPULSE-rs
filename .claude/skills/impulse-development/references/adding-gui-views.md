---
title: Adding Dioxus desktop views
updated: 2026-08-30
kind: living reference
---
# Adding GUI Views to Impulse

Step-by-step guide for adding a center-stage view to the Dioxus Desktop host in `impulse-rs/impulse-desktop` (ADR-0008). Do not add views to frozen `impulse-gui`.

Dioxus owns layout and the left-rail. Rust owns PTY, daemon snapshots, and host commands. xterm.js owns terminal glyphs. The Terminal column lives in `ui.rs` and stays mounted across switches. Do not keep a parallel GUI state store.

Exceptions already in the tree: Review dispatches to `ReviewConsole` in `ui.rs`, and Supervisor dispatches to `OperatorBoard` in `ui.rs`. New snapshot-backed panels that follow Memory and Artifacts belong in `views.rs`.

---

## Steps

### 1. Add a `DesktopView` variant

In `impulse-rs/impulse-desktop/src/views.rs`, add the variant to `DesktopView`, extend `ALL`, and add `label` and `slug` arms. `ViewRail` in `ui.rs` iterates `DesktopView::ALL`, so that is the sidebar. Do not add an egui `ViewId`, `ImpulseApp` field, or `SelectableLabel`.

Current rail order is Terminal, Memory, Review, Artifacts, Supervisor. A new view goes on that enum, not in frozen `impulse-gui`. When you add a sixth rail entry, change `ALL` from `[DesktopView; 5]` to the new length and rename the unit test that asserts five.

Example shape (replace `MyView` with the real name):

    enum DesktopView { ..., MyView }
    DesktopView::ALL includes MyView
    label() returns "My View"
    slug() returns "my-view"

### 2. Write a Dioxus component that reads snapshot DTOs

Keep new Memory/Artifacts-style panels next to `MemoryView` / `ArtifactsView` in `views.rs`. Render one hero band, then snapshot-backed sections. Mount only while selected. Hardcode the `active` CSS class, matching the Memory/Artifacts contract (`stage-view view-<slug> active` plus `data-view`).

Bind the `ProjectOpsSnapshot` fields the view needs. Do not lock a GUI `SharedState`. `data-view` matches `DesktopView::slug()`. Do not tear down xterm mounts from this view. Mutating actions go through `host_commands.rs` and the Dioxus host adapter, not an egui signal bus.

Component sketch:

    #[component]
    pub fn MyView(context: ContextHealthSummary, memory: MemorySummary) -> Element {
        rsx! {
            div { class: "stage-view view-my-view active", "data-view": "my-view",
                header { class: "view-hero",
                    div { class: "view-eyebrow", "My View" }
                    div { class: "view-hero-value", "{memory.genome_decisions}" }
                }
            }
        }
    }

### 3. Dispatch from ui.rs

In the `match active_view_value` next to Memory, Review, Artifacts, and Supervisor, add a `MyView` arm that clones only the snapshot (or shell) fields that component needs. Import the component from `crate::views`. Leave the Terminal arm empty so the always-mounted terminal column is not duplicated.

Do not invent a second Review or Supervisor path in `views.rs` unless you are deliberately retiring `ReviewConsole` / `OperatorBoard`.

### 4. Cover the new variant in tests and visual smoke

Extend the `DesktopView::ALL` unit test in `views.rs`. Add an SSR assertion in `impulse-rs/impulse-desktop/tests/views_ssr.rs` for the new `data-view` slug when the component lives in `views.rs`. Add a route in `impulse-rs/impulse-desktop/scripts/visual_smoke.mjs` with `slug`, `active` selector, and `visibleText` from the hero band (Review uses `.review-console`; Supervisor uses `[data-source="operator_board"]`; Memory/Artifacts use `.view-<slug>.active`).

Gate is the named local `impulse-desktop` crate test plus the visual smoke script in that crate. Do not run tests inside frozen `impulse-gui`.

---

## Checklist

- DesktopView variant, label, slug, and ALL updated
- Dioxus component in views.rs bound to snapshot DTOs (or an explicit ui.rs shell console, matching Review/Supervisor)
- Hero band plus data-view slug for views.rs panels
- ui.rs match arm; Terminal left in ui.rs
- No impulse-gui, egui, ViewId, ImpulseApp, or SharedState
- Mutating action, if any, goes through host commands
- views.rs unit test and views_ssr.rs assertion when applicable
- visual_smoke.mjs route with the real active selector
- named local impulse-desktop crate test clean
