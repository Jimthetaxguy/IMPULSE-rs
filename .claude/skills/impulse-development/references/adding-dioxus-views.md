# Adding Dioxus Desktop Views to Impulse

Use this guide for every new desktop surface. Dioxus owns the product UI, xterm.js owns terminal
rendering, `impulse-term` owns PTYs, and daemon/`impulse-ops` contracts own durable state.

## 1. Start from a product contract

Before adding a route, read:

- `VISION.md` for the product hierarchy and live-versus-target boundary.
- `docs/spec/DESKTOP-SHELL-ARCHITECTURE.md` for cockpit information hierarchy.
- `docs/spec/RUST-CANONICAL-CONTRACT.md` for behavior that may be claimed as implemented.

Do not create a screen solely because data exists. Name the operator decision or action it helps
complete, and identify the authoritative read model first.

## 2. Choose the narrowest existing surface

- Extend `impulse-desktop/src/ui.rs` for cockpit composition, launch, selection, and transient UI
  signals.
- Extend `impulse-desktop/src/views.rs` for a typed `DesktopView` route.
- Extend `host_commands.rs` only when a new backend action is required.
- Extend `host_bridge.rs` only when the command/event transport contract changes.
- Keep PTY/process behavior in `impulse-term` or `runtime.rs`, never in an `rsx!` component.

Do not add product behavior to `impulse-gui` or enable its EGUI feature.

## 3. Preserve the cockpit hierarchy

- Left: truthful oversight, projects, and workers.
- Center: active project, assignment, evidence, and terminal execution.
- Right: runtime selection, governed role launch, or focused inspection.
- Low-level telemetry: disclosure or diagnostic route, not the idle-screen hero.

Role and runtime are different fields. A review service must say `Oversight`; it must not imply a
launched Supervisor model exists. Empty state teaches the real launch loop instead of filling the
screen with zero-value statistics.

## 4. Render authoritative data

Pass typed data into components:

```rust
#[component]
fn EvidenceSummary(tasks: Vec<GovernedTaskRun>, on_open: EventHandler<String>) -> Element {
    rsx! {
        section { class: "evidence-summary", "aria-labelledby": "evidence-title",
            h2 { id: "evidence-title", "Evidence" }
            for task in tasks {
                button {
                    key: "{task.id}",
                    onclick: {
                        let id = task.id.clone();
                        move |_| on_open.call(id.clone())
                    },
                    "{task.task}"
                }
            }
        }
    }
}
```

Frontend signals may hold focus, selected route, open/closed disclosure, or draft form input. They
must not become a second task, review, memory, session, or artifact store. When the daemon is
disconnected, mark cached state stale and disable mutations that cannot be acknowledged.

## 5. Add commands and events contract-first

For a new mutation:

1. Define and validate a typed request in `host_commands.rs` or `impulse-ops`.
2. Route it in the Rust host dispatcher.
3. Require a daemon acknowledgment before presenting success.
4. Add the command/event name to the declared host manifest.
5. Handle degraded and reconnect states explicitly.
6. Add serialization, FIFO ordering, and malformed-payload tests.

Never compose security-sensitive actors, evidence, subjects, or Supervisor verdicts in JavaScript.

## 6. Keep terminal ownership clean

xterm.js mounts into Dioxus-owned container elements and sends `terminal_write`, `terminal_resize`,
`terminal_focus`, and `terminal_close` through the host adapter. Rust publishes terminal output and
exit events. Do not build another terminal renderer in Dioxus and do not put process ownership in
the webview.

## 7. Style for work, not spectacle

Extend `impulse-desktop/assets/impulse_crt.css` using the existing restrained industrial tokens.
The filename is legacy; the active surface must not reintroduce full-screen scanlines, flicker,
giant glowing logos, or memory-stat-first hierarchy. Use semantic HTML, visible focus, `aria-*`
labels, reduced-motion behavior, and responsive layouts.

## 8. Prove the route

Add or update:

- SSR assertions in `impulse-desktop/tests/views_ssr.rs`.
- Product/host contracts in `impulse-desktop/tests/desktop_contract.rs`.
- Unit tests beside pure reducers and validators.
- Packaged smoke coverage when the change crosses the native host, PTY, or daemon lifecycle.

Run before commit:

```bash
cargo fmt --all -- --check
cargo test -p impulse-desktop --locked
cargo check -p impulse-desktop --locked --features desktop-app
```

Then run the workspace verification contract required by `AGENTS.md`.
