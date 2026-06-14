# Retro Broadcast → Dioxus Integration

Drop the Retro Broadcast skin into the live **`impulse-desktop`** crate
(Dioxus 0.6.3 shell; originally written against the legacy host-shaped bridge). It themes the existing `ui.rs` DOM and binds to the
real `impulse-ops` DTOs — no backend contract changes required.

## Files

| File | Destination | Role |
|---|---|---|
| `impulse_crt.css` | `impulse-desktop/assets/impulse_crt.css` | The skin. Keyed on the exact class names in `ui.rs`. |
| `theme.rs` | `impulse-desktop/src/theme.rs` | Rust token constants + `AgentStatus` → CSS mappers. |
| `retro_shell.rs` | `impulse-desktop/src/retro_shell.rs` | RSX components bound to `ProjectOpsSnapshot`. |

```rust
// lib.rs
pub mod theme;
pub mod retro_shell;
pub use retro_shell::RetroShell;
```

## 1. Asset registration

`retro_shell.rs` uses the `asset!` macro (manganis), which needs the asset
feature on Dioxus. Two options:

**A — bundle as an asset (preferred):**
```toml
# Cargo.toml
dioxus = { version = "0.6.3", default-features = false, features = ["minimal", "document", "asset"] }
```
```rust
const CRT_CSS: Asset = asset!("/assets/impulse_crt.css");
// ... document::Link { rel: "stylesheet", href: CRT_CSS }
```

**B — no new feature, inline at compile time:**
```rust
// replace the asset!/Link pair with:
document::Style { {include_str!("../assets/impulse_crt.css")} }
```
Option B keeps your current `features = ["minimal", "document"]` untouched.

## 2. Feed the snapshot (backend → front)

`RetroShell` takes `ReadOnlySignal<ProjectOpsSnapshot>`. The backend already
emits `DesktopEvent::OpsUpdate { payload }` and `AgentRuntimeUpdate` through the
host event sink. In the current Dioxus Desktop direction, bridge those events
through the Dioxus-owned host adapter into a `Signal`:

```rust
use dioxus::prelude::*;
use impulse_ops::ProjectOpsSnapshot;

#[component]
fn App() -> Element {
    let mut snapshot = use_signal(ProjectOpsSnapshot::default);

    // Subscribe to the Dioxus host `ops_update` event and update the signal.
    use_effect(move || {
        spawn(async move {
            let mut rx = listen_ops_update().await; // your Dioxus host listener
            while let Some(evt) = rx.next().await {
                if let Ok(snap) = serde_json::from_value::<ProjectOpsSnapshot>(evt.payload) {
                    snapshot.set(snap);
                }
            }
        });
    });

    rsx! { RetroShell { snapshot: snapshot.into() } }
}
```

`DesktopEvent` names are already defined (`runtime.rs`):
`ops_update`, `agent_runtime_update`, `terminal_output`, `terminal_exit`,
`supervisor_local_action`. The compatibility Tauri-shaped sink is legacy-only;
new product wiring should target the Dioxus host adapter.

For agent-level updates without a full snapshot, merge
`AgentRuntimeUpdate { snapshot }` into `snapshot.write().agents` by `agent_id`.

## 3. Data binding map (no hardcoded values)

| UI element | Source field |
|---|---|
| Memory stat `47.2k` | `context.estimated_tokens` → `theme::format_count` |
| `% of window` | `context.usage_fraction`, `context.window_tokens` |
| Agents online / working | `agents[].active`, `agents[].agent_status == Working` |
| Retrieval stat | `retrieval.backend`, `retrieval.mode` |
| Genome decisions | `memory.genome_decisions` |
| Pending review bar | `context.pending_review_count` (hidden when 0) |
| Context tier / injections / compactions | `context.tier / injection_count / compaction_count` |
| Agent rail dots | `agents[].agent_status` → `theme::status_dot_class` |
| Daemon online badge | `!agents.is_empty()` (swap for real daemon ping) |
| Event strip | `generated_at`, `artifacts.len()`, `interventions.len()` |

## 4. Terminal stage

The `xterm-mount` div keeps the existing `data-xterm-mount` / `data-agent-id`
contract, so the `TERMINAL_INTEROP_SCRIPT` in `ui.rs` still wires xterm.js to
`agent_write` / `agent_resize` unchanged. The skin only restyles the mount;
glyph rendering stays owned by xterm.js.

## 5. Legibility contract (important)

The skin keeps **chrome calm**: low-bloom cyan/neutral on rails, tabs,
inspector, and terminal — so live data stays readable. Full phosphor bloom is
reserved for the **brand wordmark** and a **single pending signal**. Do not add
bloom to terminal text or dense tables (it destroys legibility — see
`IMPULSE-DESIGN-SPEC.md` §11). xterm.js text must stay un-bloomed.

## 6. Fonts in the Desktop Host

The CSS pulls Baloo 2 + JetBrains Mono from Google Fonts for dev. For a
packaged desktop app, vendor the woff2 files under `assets/fonts/` and replace the
`document::Link` to Google Fonts with local `@font-face` declarations so the app
works offline.

## 7. Accessibility

`crt-flicker` and any sweep animations are gated behind
`@media (prefers-reduced-motion: no-preference)`. Bloom is decoration only —
every value is legible with effects disabled. Verify with reduced-motion on.

## 8. Verify (SSR snapshot test)

`dioxus-ssr` is already a dev-dependency. Add a contract test:

```rust
#[test]
fn retro_shell_renders_snapshot() {
    let mut snap = ProjectOpsSnapshot::default();
    snap.context.estimated_tokens = 47238;
    snap.context.window_tokens = 200000;
    snap.context.pending_review_count = 1;
    let mut dom = VirtualDom::new_with_props(
        RetroShell,
        RetroShellProps { snapshot: Signal::new(snap).into() },
    );
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains("impulse"));      // wordmark
    assert!(html.contains("47.2k"));        // bound stat
    assert!(html.contains("pending-bar"));  // loud signal shown
}
```
