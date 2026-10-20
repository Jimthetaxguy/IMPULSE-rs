---
title: Dioxus Deep Dive — Framework State and What It Means for the Impulse Cockpit
description: Dioxus 0.7/0.8 state, Cognition continuity, how impulse-desktop uses Dioxus 0.6.3 today, a measured 0.7.10 upgrade spike, and ranked cockpit opportunities
version: '1.0'
updated: 2026-10-04
type: research
category: technology-assessment
phase: all
status: complete
audience: builders
tags: [research, dioxus, desktop, cockpit, webview, xterm, upgrade, cognition, blitz]
---

# Dioxus Deep Dive — Framework State and What It Means for the Impulse Cockpit

> **Point-in-time record (2026-10-04).** Covers the upstream Dioxus project as of that date and the
> Impulse repository at `origin/main` `7481457`. External claims cite a primary URL; repository
> claims cite `path:line` against that commit. Anything that could not be confirmed is marked
> **UNVERIFIED**. The upgrade numbers come from a throwaway spike in an isolated worktree. None of
> the spike's code changes were committed.

## Summary

Impulse's cockpit (`impulse-rs/impulse-desktop`) pins Dioxus **0.6.3** (Feb 2025). The current stable
release is **0.7.10** (2026-07-30), and **0.8.0-alpha.1** (2026-07-31) is the newest release of any kind.
Five findings matter most:

1. **0.7.10 is close to a drop-in replacement for Impulse.** With the four version strings bumped,
   `cargo check -p impulse-desktop --features desktop-app --all-targets` compiled with **zero source
   changes**, and clippy `-D warnings` was clean. The full desktop test suite passed **293 of 296**.
   Two of the three failures come from the SSR escaper switching from named HTML entities
   (`&quot;`) to numeric ones (`&#34;`). The third is a test that asserts the literal `0.6.3` pin.
2. **On 0.6.3, `impulse-desktop` does not compile in release mode.** `cargo check -p
   impulse-desktop --release` fails with two `E0382` "borrow of moved value" errors inside `rsx!`
   `key:` attributes (`impulse-desktop/src/ui.rs:1770`, `impulse-desktop/src/views.rs:380`). The
   debug-only workspace gate cannot see this. It blocks the R1 Dioxus packaging route in
   `docs/plans/EGUI-DECOMMISSION.md:176-184`. On 0.7.10 the same code builds in release.
3. **The xterm.js assets probably do not load in the real WebView when the app runs unbundled.**
   This is inferred from Dioxus source. I did not observe it at runtime. The shell links
   `assets/vendor/xterm/*.js` by bare relative path (`ui.rs:39-41`, `ui.rs:3175-3186`). Both
   dioxus-desktop 0.6.3 and 0.7.10 serve every `/assets/...` request from `<exe>/../Resources` on
   macOS. The host-readiness smoke runs headless Chromium against a `file://` fixture
   (`scripts/host_readiness_smoke.mjs:62,101`), so it never exercises WKWebView or this path.
4. **The cockpit still uses a Tauri-style transport inside a Rust-native framework.** A Rust
   `onclick` formats a JavaScript string. The WebView calls a JS `invoke`, which crosses back into
   Rust over a `document::eval` channel. The result then crosses back into Rust a second time
   through a different eval to update signals. PTY output travels as a JSON array of numbers.
   Dioxus lets components call `DesktopShellState` directly. Removing the round-trips is the
   largest architectural simplification available.
5. **Upstream continuity has weakened but is not broken.** On 2026-09-10 the Dioxus team announced
   it was joining Cognition. Upstream has not published a release since 2026-07-31. There were 35
   commits on `main` in the last 90 days, mostly from one full-time maintainer (Nico Burns). The
   stated priority is now Blitz and Dioxus-Native, not the WebView desktop renderer that Impulse uses.

**Recommendation in one line:** fix the release-mode `key:` bug now. Then upgrade to 0.7.10 in a
small dedicated lane. Do not track 0.8 alphas. Start moving cockpit data flow off JS round-trips so
that more of the cockpit is testable in Rust and less depends on WebView behaviour.

---

## Part A — Dioxus itself

### A1. Versions and release cadence

Dates are crates.io publish dates from
[`crates.io/api/v1/crates/dioxus/versions`](https://crates.io/api/v1/crates/dioxus/versions). MSRV
is the crate's `rust_version` field.

| Line | Key versions (date) | MSRV |
|---|---|---|
| 0.6 | 0.6.0 (2024-12-07), 0.6.1 (2024-12-18), 0.6.2 (2025-01-22), **0.6.3 (2025-02-08)** | 1.79 |
| 0.7 pre | alpha.0 (2025-05-14) through rc.4 (2025-10-31) | 1.80 |
| 0.7 | 0.7.0 (2025-10-31), 0.7.1 (2025-11-06), 0.7.2 (2025-12-05), 0.7.3 (2026-01-17), 0.7.4 (2026-03-27), 0.7.5 (2026-04-07), 0.7.6 (2026-04-22), 0.7.7 (2026-05-01), 0.7.8 (2026-05-07), 0.7.9 (2026-05-08), **0.7.10 (2026-07-30)** | 1.80, then 1.83 from 0.7.3 |
| 0.8 pre | 0.8.0-alpha.0 (2026-05-19), **0.8.0-alpha.1 (2026-07-31)** | 1.85 |

- **Cadence.** Dioxus has shipped about one minor line a year (0.6 in Dec 2024, 0.7 in Oct 2025).
  Patches came frequently through 0.7.9. 0.7.10 is a single-PR backport
  ([v0.7.10 release](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.10)). Nothing has been
  published since 2026-07-31.
- **The 0.7 blog post predates the 0.7.0 crate.** The post is dated 2025-09-08
  ([release-070](https://dioxuslabs.com/blog/release-070/)), but 0.7.0 reached crates.io on
  2025-10-31.
- **0.7.x added features without breaking changes.** 0.7.3 added scoped CSS / CSS modules
  ([v0.7.3](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.3)). 0.7.4 added Swift, Kotlin and
  Java FFI and iOS/Android config in `Dioxus.toml`
  ([v0.7.4](https://github.com/DioxusLabs/dioxus/releases/tag/v0.7.4)).
- **WebView stack per release** (from the
  [dioxus-desktop dependency list](https://crates.io/api/v1/crates/dioxus-desktop/0.7.10/dependencies)):

  | dioxus-desktop | wry | tao |
  |---|---|---|
  | 0.6.3 | ^0.45 | ^0.30.8 |
  | 0.7.x | ^0.53.5 | ^0.34 |
  | 0.8.0-alpha.1 | ^0.55.1 | ^0.35.2 |

  The migration guide says wry 0.52, but the published crate requires 0.53.5.

### A2. What changed from 0.6 to 0.7 to 0.8

**0.6 → 0.7 breaking changes.** These are the items in the official guide,
[How to Upgrade to Dioxus 0.7](https://dioxuslabs.com/learn/0.7/migration/to_07/):

- **`dioxus-lib` removed.** Depend on `dioxus` with `default-features = false, features = ["lib"]`
  instead.
- **Forms submit by default.** `onsubmit` handlers must call `e.prevent_default()`. Desktop blocks
  page navigation separately.
- **Asset options unified.** `ImageAssetOptions::new()` becomes `AssetOptions::image()`.
- **Server functions.** `ServerFnError` is now a Dioxus type, and the default codec is JSON.
- **Removed from the prelude:** `use_drop`, `Runtime`, `queue_effect` and `provide_root_context`.
  The `VirtualDom::provide_root_context` method is unaffected.
- **Dependency bumps:** wry, axum 0.8, server_fn 0.7.
- **Custom renderers only:** the event-listener type changed (`ListenerCallback`).

The guide documents no break to `rsx!`, `use_resource`, `LaunchBuilder`, `dioxus_desktop::Config`,
`document::eval`, or the dioxus-ssr entry points. The spike in §C1 confirms that none of the APIs
Impulse uses broke at compile time.

One behaviour change is undocumented and was found by the spike: **dioxus-ssr 0.7 escapes text with
numeric entities.** `"` becomes `&#34;` instead of `&quot;`, and `<` becomes `&#60;` instead of `&lt;`.
The lockfile diff adds `askama_escape 0.13`, which fits this, but I did not confirm the
dependency's role.

**0.7 additions** ([release post](https://dioxuslabs.com/blog/release-070/)):

- **Subsecond** hot-patching of Rust code through `dx serve`.
- **Stores** (`#[derive(Store)]`, `use_store`, `GlobalStore`) for fine-grained nested state.
- **Fullstack rebuilt on axum 0.8.** Rocket-style `#[get]`/`#[post]`, `use_websocket`,
  `ServerEvents<T>`, `Streaming<T>`.
- **Other:** `wasm_split`, Dioxus-Native/Blitz preview, Radix-style primitives, automatic Tailwind,
  a `/public` directory, and `dx self-update`.

Hooks present in 0.7.10 ([docs.rs item list](https://docs.rs/dioxus/0.7.10/dioxus/all.html)) include
`use_signal`, `use_memo`, `use_resource`, `use_future`, `use_coroutine`, `use_effect`, `use_action`,
`use_loader` and `use_store`.

**0.7 → 0.8 (alpha only).** There is no migration guide yet: `/learn/0.8/migration/` returns 404.
The release notes
([alpha.0](https://github.com/DioxusLabs/dioxus/releases/tag/v0.8.0-alpha.0),
[alpha.1](https://github.com/DioxusLabs/dioxus/releases/tag/v0.8.0-alpha.1)) list:

- Edition 2024 and MSRV 1.85.
- `#[non_exhaustive]` on props derived by `#[component]`, so code that builds props structs directly
  breaks.
- macOS/iOS platform code moved to `objc2` (#5101). This removes the `cocoa`/`block 0.1.6`
  future-incompatibility warning that 0.7.10 still emits (seen in the spike).
- Hot-patching on by default.
- dioxus-ssr maps `initial_value`, `checked` and `selected` to their HTML equivalents (#5493).
- A fix for a store memory leak (#5711).
- Blitz synced to 0.3.0-beta.1.

### A3. Core model (0.7.10)

| Concept | API | Notes for Impulse |
|---|---|---|
| Components | `#[component] fn X(props…) -> Element`, `rsx!` | Impulse uses this throughout `ui.rs`/`views.rs` |
| Local state | `use_signal`, `use_memo`, `use_reactive` | Signals are RefCell-like and checked at runtime. Holding `.read()`/`.write()` across `.await` panics with "already borrowed" ([signals docs](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/essentials/basics/signals.md); [#4124](https://github.com/DioxusLabs/dioxus/issues/4124)) |
| Async | `spawn` (no `Send` bound; cancelled on unmount), `use_future`, `use_resource`, `use_coroutine` | "In Dioxus, all futures run on the main thread" ([async docs](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/essentials/basics/async.md)) |
| Desktop runtime | `tokio_runtime` is a default feature. `launch` builds a **multi-thread** tokio runtime with `enable_all()` and `block_on`s the event loop. A current-thread runtime is rejected by `assert_ne!` | dioxus-desktop 0.7.10 `src/launch.rs:108-135`. So `tokio::spawn` and tokio `UnixStream` work from components, which Impulse already relies on (`host_bridge.rs:390`) |
| Global / shared state | `Signal::global`, `GlobalSignal`, `GlobalStore`, context (`use_context_provider`/`use_context`) | Impulse uses none of these (see Part B) |
| Stores | `#[derive(Store)]`. Field and collection accessors mark only the changed paths dirty | New in 0.7. They fit the cockpit's snapshot fan-out (see C3) |
| Effects | `use_effect` runs again when the signals it read change | Used for the two bridge scripts (`ui.rs:3509-3574`) |

### A4. Renderers and platforms

| Renderer | Mechanism | Maturity |
|---|---|---|
| Desktop | wry (WKWebView on macOS), tao, muda menus, tray-icon, global-hotkey. rfd is used internally for file inputs | **Production.** Docs claim apps are "typically under 5MB" ([desktop guide](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/guides/platforms/desktop.md)); the Impulse binary measured larger (see §C1) |
| Web (WASM) | `dioxus/web` | Production |
| Mobile | Runs on `dioxus-desktop` (wry) on iOS/Android | Supported. Device tooling arrived in 0.7 |
| LiveView | `dioxus-liveview` 0.7.10 (axum 0.8) | Still shipping. Docs say it will fold into fullstack ([liveview guide](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/guides/platforms/liveview.md)) |
| SSR | `dioxus_ssr::{render, render_element, pre_render}` | Production. `VirtualDom` is `!Send` ([SSR guide](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/guides/utilities/ssr.md)) |
| Fullstack | axum 0.8 server functions, SSE, websockets | Production in 0.7 |
| Native (Blitz) | Stylo CSS, Taffy layout, Parley text, Vello/anyrender paint, winit, AccessKit | **Experimental.** The 0.7 post calls it a "work in progress" that has not focused on performance. The [Blitz README](https://github.com/DioxusLabs/blitz) says "beta… usable if you are an early adopter". **No binary-size claim was found** in the post or the README. It has no websockets or localStorage |

### A5. Tooling

- **`dx serve` hot reload.** RSX edits to elements, string attributes and literal props reload
  without a rebuild ([hot-reload docs](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/essentials/ui/hotreload.md)).
  Rust-code hot-patching (Subsecond) is opt-in through `--hot-patch`. The CLI help calls it "quite
  experimental and may lead to unexpected segfaults" (dioxus-cli 0.7.10 `src/cli/serve.rs:55-61`).
- **Subsecond only patches the tip crate.** Edits in workspace dependency crates are not patched.
  For Impulse this means `impulse-desktop` edits patch, while edits to `impulse-ops` or
  `impulse-term` need a full rebuild. Static initialisers and struct layout changes are not handled
  either. Open issues include [#4962](https://github.com/DioxusLabs/dioxus/issues/4962) (crash with
  `target-cpu=native`) and [#4713](https://github.com/DioxusLabs/dioxus/issues/4713) (Linux needs mold).
- **Bundling.** `dx bundle --desktop --package-types dmg` emits `.app` and `.dmg`, and only builds for
  the host platform ([bundle tutorial](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/tutorial/bundle.md)).
  By 0.7.5 the CLI had its own macOS bundler. It signs frameworks, then the binary, then the `.app`
  from `APPLE_CERTIFICATE`, and notarizes when the `APPLE_ID`/`APPLE_API_KEY` env sets are present
  (dioxus-cli 0.7.10 `src/bundler/macos.rs`). The tutorial still says Dioxus has no signing
  utilities, which is now out of date.
- **`Dioxus.toml`.** Sections include `[application]`, `[bundle]` (identifier, icon, resources,
  `external_bin`), and `[bundle.macos]` (`signing_identity`, `entitlements`, `hardened_runtime`, and
  others) ([configure docs](https://github.com/DioxusLabs/docsite/blob/main/docs-src/0.7/src/guides/tools/configure.md)).
  `external_bin` is the hook for shipping the companion `impulse-rs` binary that R1 requires.
- **Assets.** `asset!()` comes from manganis and is re-exported in the prelude. Under `dx`, an
  `asset!` path is copied into the bundle and rewritten to a resolvable URL.
- **Docs gap.** Several 0.7 doc pages are empty stubs in the docsite repo: `guides/deploy/macos.md`,
  `guides/tools/bundle.md`, `guides/testing/desktop.md`, and all of `guides/apis/*`
  ([docsite tree](https://github.com/DioxusLabs/docsite/tree/main/docs-src/0.7/src)). For desktop
  APIs, read the crate source.
- **The `dx` setup on this machine is wrong for Dioxus work.** `which -a dx` lists `~/bin/dx`
  first. That file is a shell script whose header reads "macOS Diagnostic Toolkit", so it shadows the
  Dioxus CLI. The real Dioxus CLI, `~/.cargo/bin/dx`, reports `dioxus 0.6.3`: too old for
  `--hot-patch` and the built-in notarizer.

### A6. Interop surface (dioxus-desktop 0.7.10)

| Need | API |
|---|---|
| Call JS and exchange values | `document::eval(js) -> Eval` with `.join::<T>()`, `.send(v)` and `.recv::<T>()`. On the JS side: `dioxus.send(x)` and `await dioxus.recv()`. Open footguns: [#3084](https://github.com/DioxusLabs/dioxus/issues/3084) (spurious `Err(Finished)` from recv) and [#3915](https://github.com/DioxusLabs/dioxus/issues/3915) (no `try_recv`) |
| Custom protocols | `Config::with_custom_protocol` and `with_asynchronous_custom_protocol`; `use_asset_handler` (dioxus-desktop `config.rs:226-266`, `hooks.rs:92`). Open panic: [#4414](https://github.com/DioxusLabs/dioxus/issues/4414) |
| HTML shell | `with_custom_head`, `with_custom_index`, `with_background_color`, `with_navigation_handler`, `with_disable_context_menu`, `with_data_directory` |
| Menus, tray, shortcuts | `Config::with_menu` (muda re-exported), `use_muda_event_handler`, `trayicon::*`, `use_global_shortcut`. Tray bug: [#4495](https://github.com/DioxusLabs/dioxus/issues/4495) |
| Multiwindow | `window().new_window(VirtualDom, Config)`. **Each window is a separate VirtualDom**, so signals and context do not cross windows ([#5160](https://github.com/DioxusLabs/dioxus/issues/5160), open) |
| Raw window events | `use_wry_event_handler`, `Config::with_custom_event_handler` |
| File dialogs | rfd is used internally but not re-exported. Add `rfd` directly (inference) |
| xterm.js | **No first-party example or issue exists.** Any embedding pattern is project-local. Impulse's pattern is in Part B |

### A7. Testing without a window

- **Render tests.** `dioxus_ssr::render_element(rsx!{…})` handles pure components.
  `VirtualDom::new_with_props` + `rebuild_in_place()` + `dioxus_ssr::render(&vdom)` handles stateful
  trees
  ([ssr tests @ v0.7.10](https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/ssr/tests/simple.rs)).
- **Hook tests.** Drive `wait_for_work()` and `render_immediate()` by hand. The docs say there is no
  full hook-testing library
  ([hook_test example](https://github.com/DioxusLabs/docsite/blob/main/packages/docs-router/src/doc_examples/hook_test.rs)).
  [#5324](https://github.com/DioxusLabs/dioxus/issues/5324) proposes a Blitz-based harness and is open.
- **Upstream's own desktop tests** run a real hidden window and inject events through `eval`, with a
  dead-man timer
  ([headless_tests/utils.rs](https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/desktop/headless_tests/utils.rs)).
  WebdriverIO's Dioxus service hits WKWebView throttling on headless macOS CI
  ([#5586](https://github.com/DioxusLabs/dioxus/issues/5586)).

### A8. Governance and continuity

- **What happened.** The Dioxus Labs post of 2026-09-10 by Jonathan Kelley is titled "Dioxus Labs is
  joining Cognition". Its key statements:
  - The team joins "to accelerate the development of Devin" and will "continue to work on Dioxus,
    Blitz, Taffy, and Subsecond".
  - The team "will naturally have less time to devote entirely to Dioxus".
  - Nico Burns, the Blitz lead, will work on Dioxus full time.
  - Investment shifts to Dioxus-Native and Blitz.
  - Devin CLI's terminal renderer is built with Dioxus, so Dioxus is in Cognition's own stack.
  - Dioxus Labs had raised $3.5M.

  Source: [dioxuslabs.com/blog/joining-cognition](https://dioxuslabs.com/blog/joining-cognition),
  fetched and read for this document. Cognition published a companion post the same day
  ([cognition.com/blog/welcoming-dioxus](https://cognition.com/blog/welcoming-dioxus)). Neither post
  says "acquired". **UNVERIFIED:** deal terms, ownership of the GitHub org and crates.io publishing
  rights, and whether the company entity or its IP was acquired.
- **License.** MIT OR Apache-2.0 (crates.io metadata; `LICENSE-MIT` and `LICENSE-APACHE` in the repo).
  A permissive dual license means a fork is always legally possible.
- **Activity** (`gh api` on `DioxusLabs/dioxus`, 2026-10-04):

  | Metric | Value |
  |---|---|
  | Commits to `main`, last 90 days | 35 |
  | Commits to `main`, last 12 months | 376 |
  | Monthly trend | 95 (Oct 2025) falling to about 7–16 (Jun–Sep 2026) |
  | Top committers, last 90 days | Nico Burns 14, Evan Almloff 4 |
  | Releases, last 12 months | 17, but none since 2026-07-31 |
  | Open issues / PRs | about 655 / 157 |
  | Stars | about 39k |

- **Roadmap.** The 0.8 roadmap ([Discussion #5024](https://github.com/DioxusLabs/dioxus/discussions/5024),
  2025-11-25) covers native mobile APIs, FFI, element handles across the WebView boundary, portals,
  and spinning `dx` out into its own repo. It says no drastic change is planned to state or fullstack,
  and gives **no date**. **UNVERIFIED:** a 0.8 stable date.

### A9. Known pain points

| Area | Evidence | Relevance to Impulse |
|---|---|---|
| Runtime borrow panics | Holding signal `.read()`/`.write()` across `.await`; [#5574](https://github.com/DioxusLabs/dioxus/issues/5574) | Medium. `DesktopShell` copies values out before setting signals (`ui.rs:3539-3568`), which is the safe pattern |
| Large lists | Keys are required. **No first-party virtualised list was found** (UNVERIFIED that none exists). About 2000 items took tens of seconds on Safari/WebKit ([#3076](https://github.com/DioxusLabs/dioxus/issues/3076)) | Relevant to a blackboard viewer or long audit trails, which will need paging or windowing |
| WKWebView | No `with_background_throttling` ([#5586](https://github.com/DioxusLabs/dioxus/issues/5586)). A killed WebContent process leaves a blank window with no reload ([#5885](https://github.com/DioxusLabs/dioxus/issues/5885), opened 2026-10-03) | High for a long-running cockpit hosting many terminals |
| Compile time | `rsx!` expansion dominates: 23.8 s vs 2.3 s with manual templates in one crate ([#5549](https://github.com/DioxusLabs/dioxus/issues/5549)) | `ui.rs` is 3,591 lines of mostly `rsx!`. Splitting it helps incremental builds |
| Debug/release macro divergence | Not filed upstream. Found here (§B4) | Directly hits Impulse on 0.6.3 |

---

## Part B — How Impulse uses Dioxus today

### B1. Dependency footprint

- **Dioxus deps.** `impulse-desktop/Cargo.toml` depends on
  `dioxus = { version = "0.6.3", default-features = false, features = ["minimal", "document"] }`,
  with optional `dioxus-desktop = "0.6.3"` behind feature `desktop-app` (`Cargo.toml:26,37-38`).
  Dev-deps are `dioxus-ssr = "0.6.0"` and `generational-box = "0.6.2"` (`Cargo.toml:57-58`).
- **Lockfile.** It resolves wry 0.45.0, tao 0.30.8 and three `muda` copies (`impulse-rs/Cargo.lock`).
  `cargo tree -i` shows dioxus-desktop 0.6.3 alone pulls in **two** of them: 0.11.5 directly and
  0.15.3 via tray-icon. Legacy `tauri` pulls in the third (0.19.2).
- **No dioxus-router, no `asset!`, no `Dioxus.toml`, no `dx`.** `Dioxus.toml` was searched for
  repo-wide and none was found. The binary is a plain Cargo `[[bin]]` with
  `required-features = ["desktop-app"]`.

### B2. Architecture map

| Layer | Where | What it does |
|---|---|---|
| Entrypoint | `src/bin/impulse_desktop.rs:14-48` | Builds `DesktopRuntime` (with the daemon-ops attachment), `WorkspaceRegistry` and `McpToolRegistry`. Installs a process-global `LiveHostContext` (`OnceLock`, `host_bridge.rs:268-280`), then `dioxus::LaunchBuilder::desktop().with_cfg(desktop_config()).launch(LiveDesktopApp)` (`:45-47`) |
| Window config | `desktop_host.rs:62-67` | Title, plus `with_custom_head(host_bootstrap_script())`. No menu, tray, resource directory, or custom protocol |
| Manifest-only bootstrap | `desktop_host.rs:8-56` | Installs `window.__IMPULSE_DESKTOP_HOST` stubs that always reject and publish the invoke/event manifest (ADR-0008 validation item 5, `docs/decisions/0008-dioxus-desktop-host.md:47`) |
| Live host bridge | `host_bridge.rs:283-357` (JS), `:371-436` (Rust) | `use_future` opens one long-lived `document::eval`. JS `invoke()` sends `{kind:"host_invoke"}`. A bounded FIFO worker (`tokio::spawn`, capacity 64) dispatches against `DesktopShellState` and replies `{kind:"host_invoke_result"}`. Runtime events are pushed as `{kind:"host_event"}`. This is the "Dioxus host invoke wire" (`CONTEXT.md:491-497`) |
| Root component | `host_bridge.rs:442-449` | `LiveDesktopApp` mounts the bridge, then renders `ui::DesktopShell` |
| State owner | `ui.rs:3494-3590` | `DesktopShell` holds ten `use_signal`s and opens a **second** eval (`DESKTOP_EVENT_BRIDGE_SCRIPT`, `ui.rs:113`) whose JS calls `invoke`/`listen` and `dioxus.send`s results back. Each message is reduced into cloned vectors, then every signal is `set` |
| Presentation | `ui.rs:3027` (`DesktopShellWithSnapshot`), components at `ui.rs:823-2950`, `views.rs:118-399` | All data passes down as owned props (`Vec<…>` clones). The view switch is a hand-rolled enum (`views.rs:31-38`, `ui.rs:3351`), not dioxus-router |
| Actions | e.g. `ui.rs:3255-3262`, `agent_focus_bridge_script` `ui.rs:603` | A Rust `onclick` formats a JS string and calls `document::eval`, which calls `window.__impulseOpsBridge.*`, which calls `invoke`, which crosses back into Rust |
| Terminal | `ui.rs:451-555` (`TERMINAL_INTEROP_SCRIPT`), mounts at `ui.rs:3329-3340` | xterm.js 6.0.0 + fit addon (`package.json` devDeps), vendored under `assets/vendor/xterm/`. JS `onData` calls `invoke("agent_write")` with input as a **number array**. `listen("terminal_output")` writes bytes that arrive as a **number array** (`DesktopEvent::TerminalOutput { data: Vec<u8> }`, `runtime.rs:326-331`) |
| Governed controls | `ui.rs:1681-1824` (`OperatorBoard`), `:1825` (`GovernedTaskCard`), scripts at `ui.rs:1544-1590` | Promote/Discard (ADR-0019) through the same eval → invoke path. Keys of the form `id:revision` remount cards so a stale rationale is discarded (`ui.rs:1764-1770`) |
| Native islands | `native.rs:8-16,66` | An enum lists seven kinds (MenuBar, GlobalShortcut, FileOpenPanel, …) but only `AppKitProbe` is implemented. The rest return `UnsupportedNativeIsland` |
| Dioxus-free core | `runtime.rs`, `daemon_ops.rs`, `mcp.rs`, `workspace.rs`, `host_commands.rs` | Grep finds Dioxus only in comments. The policy and state authority layer does not depend on the UI framework, which is the right shape (`CONTEXT.md:485-489`) |

### B3. Tests

- **Coverage.** 296 desktop tests pass on 0.6.3 (`cargo test -p impulse-desktop --features desktop-app`).
- **SSR tests.** These render components to HTML with `VirtualDom::new_with_props` +
  `rebuild_in_place()` + `dioxus_ssr::render` (`tests/views_ssr.rs:17-19`, and many in
  `tests/desktop_contract.rs`).
- **Fake Document for async eval flows.** The notable technique is in
  `tests/desktop_contract.rs:260-330`. A `FakeDocument` implements `dioxus::document::Document`, and a
  `FakeEvaluator` implements `Evaluator` (owned by `generational_box::Owner`). The fake is injected
  with `vdom.provide_root_context(Rc<dyn Document>)` (`:1555-1569`). This lets the bridge-message
  reducer run against the real `DesktopShell` without a window. It relies on semi-internal APIs
  (`Eval::new`, `Evaluator::poll_recv`, `generational-box`), and it still compiled unchanged on 0.7.10.
- **Smoke tests.** The Playwright smokes (`scripts/host_readiness_smoke.mjs`, `visual_smoke.mjs`)
  run in **headless Chromium** on a `file://` fixture (`host_readiness_smoke.mjs:62,101`). They
  validate the JS scripts but never WKWebView, the `dioxus://` protocol, or asset serving.

### B4. Defects and workarounds found while mapping

1. **Release build broken on 0.6.3 (bug).**
   - The failures: `cargo check -p impulse-desktop --release` fails with
     `E0382 borrow of moved value: task` (`ui.rs:1749` → `:1770`) and
     `E0382 borrow of moved value: action_id` (`views.rs:371` → `:380`).
   - Cause: in release builds the 0.6 `rsx!` macro evaluates the `key:` format string after the
     value has been moved into a prop or closure, while the debug expansion orders it differently.
   - Why it went unnoticed: the canonical gate (`CLAUDE.md` "Verification Gate") only builds the
     debug profile.
   - How the spike confirmed it: precomputing the key strings before the move fixes it on 0.6.3.
     0.7.10 compiles the unmodified code in release.
2. **xterm asset resolution (inferred risk).**
   - How the paths resolve: `ui.rs:39-41` uses plain relative paths. Both
     `dioxus-desktop-0.6.3/src/protocol.rs:104-108,218-229` and
     `dioxus-asset-resolver-0.7.10/src/native.rs:55-59,152-162` remap any `/assets/…` request to
     `current_exe()/../Resources/assets/…` on macOS. That is correct inside a `.app` bundle, but for
     `cargo run` it points to `target/Resources/…`, which does not exist.
   - A misleading upstream comment: the source comment promises a "cargo manifest dir" fallback that
     the code does not implement.
   - What a failure would look like: if this is right, the terminal interop reports `degraded`
     (`ui.rs:473-475`) in an unbundled run.
   - **UNVERIFIED at runtime.** I did not launch a GUI window unattended. The R1 checklist already
     anticipates this failure mode (`EGUI-DECOMMISSION.md:183-184`).
3. **The two eval scripts are a race workaround.** The resolver polls up to 25 × 10 ms for the
   live host to replace the stubs (`ui.rs:47-80`), because `use_live_host_bridge` and `DesktopShell`
   install independent evals with no ordering guarantee. One eval, or none, removes the race.
4. **The eval channel carries bulk data.** PTY bytes are serialized as JSON number arrays, roughly
   3–4× the raw size. They travel over an **unbounded** channel
   (`host_bridge.rs:246-247`) into a single eval. A noisy agent can therefore grow memory without
   bound and delay control messages that share the channel.

### B5. Feature usage scorecard

| Dioxus capability | Used? | Notes |
|---|---|---|
| `rsx!`, `#[component]`, `EventHandler`, keyed lists | Yes | Throughout |
| `use_signal`, `use_effect`, `use_future`, `spawn` | Yes | |
| `use_memo`, `use_resource`, `use_coroutine` | No | `use_coroutine` is the natural fit for the host bridge |
| Context / `GlobalSignal` / Stores | No | All state is owned by `DesktopShell` and prop-drilled |
| `document::eval` | Heavily | Both transport and action dispatch |
| Router | No | Enum switch, which is fine at five views |
| `asset!` / manganis / `dx` | No | Relative paths; plain Cargo |
| Menus, tray, global shortcuts, multiwindow | No | `native.rs` lists them as unsupported islands |
| Custom protocol / asset handler | No | Would fix B4.2 and B4.4 |
| SSR for tests | Yes | Plus a fake Document |
| Fullstack / LiveView / Blitz | No | |

**What the 0.6.3 pin costs:**

- the release-mode macro bug (B4.1);
- wry 0.45 and tao 0.30, eight wry minors behind, so WKWebView fixes since early 2025 are missing;
- duplicate muda copies;
- no Stores, no Subsecond, and no built-in signing or notarization in `dx bundle`;
- a widening gap that makes any later jump to 0.8 larger.

---

## Part C — Recommendations

### C1. Upgrade spike (0.6.3 → 0.7.10), measured

Setup: worktree `claude/dioxus-deep-dive-20261004`, isolated `CARGO_TARGET_DIR`, rustc 1.98.1. I
bumped `dioxus`, `dioxus-desktop`, `dioxus-ssr` and `generational-box` to `0.7.10` and ran
`cargo update -p` on those four packages. Everything was reverted afterwards.

| Check | 0.6.3 (main) | 0.7.10 (spike) |
|---|---|---|
| `cargo check -p impulse-desktop --features desktop-app --all-targets` | pass | **pass, zero source edits** |
| `cargo clippy … --all-targets -- -D warnings` | pass | pass; one future-incompat note (`block 0.1.6` via `cocoa`) |
| `cargo check -p impulse-desktop --release` | **fails (2 × E0382)** | pass |
| `cargo test -p impulse-desktop --features desktop-app --no-fail-fast` | 296 pass, 1 ignored | 293 pass, **3 fail**, 1 ignored |
| Release binary (`--bin impulse-desktop`, macOS arm64) | 6,696,352 B (with key fix applied) | 6,837,760 B (+2.1%) |
| Lockfile churn | — | +54 / −43 packages; wry 0.45→0.53.5, tao 0.30.8→0.34.8, muda 0.11.5+0.15.3 → 0.17.2 for the Dioxus stack; adds `subsecond`, `dioxus-stores`, `dioxus-asset-resolver`; drops `dioxus-fullstack`/`dioxus-lib` |

The three failures:

1. `test_dioxus_desktop_launch_binary_is_feature_gated` asserts the literal string
   `dioxus-desktop = { version = "0.6.3", optional = true }` (`tests/desktop_contract.rs:539`).
   Update the string.
2. `test_shell_supervisor_route_renders_authoritative_governed_evidence_and_controls` asserts
   `&lt;redacted&gt;` (`:2613`). 0.7 emits `&#60;redacted&#62;`.
3. `test_profiled_governed_tasks_label_rust_only_and_route_all_producers_via_daemon` asserts
   `&quot;$IMPULSE_CONTROL_CLI&quot;` (`:2687-2689`). 0.7 emits `&#34;`.

For failures 2 and 3, the better fix is to stop asserting on a particular escaping. Add a
`decode_entities(&html)` test helper, or assert on a `data-*` attribute, so the next escaper change
cannot break the tests.

**Not covered by the spike:**

- the full workspace gate after the lockfile change (only `-p impulse-desktop` was built and tested);
- a live WKWebView run;
- Linux;
- any check that 0.7's form behaviour change affects nothing. Grep finds no `form`/`onsubmit` in
  `ui.rs`, so I expect no impact.

### C2. Upgrade recommendation and ordering

Move to **0.7.10** and stay on the 0.7 line. Do not adopt 0.8 alphas. The 0.8 line has no
migration guide, adds `#[non_exhaustive]` component props, and requires edition 2024. Upstream also
has no release cadence right now. Order of work (each step blocks the next):

1. **Fix the release-mode `key:` bug on 0.6.3 first.** Precompute `card_key`/`action_key` before the
   move (`ui.rs:1749-1770`, `views.rs:371-380`) and add `cargo check -p impulse-desktop --release` to
   CI. This is independent of the upgrade and unblocks R1 either way.
2. **Make the SSR assertions escaping-agnostic.** This also runs on 0.6.3 and removes the only
   behavioural test failures before the bump.
3. **Bump in a dedicated lane.**
   - Change the four version strings and update the version-pin assertion.
   - Commit `cargo update -p dioxus -p dioxus-desktop -p dioxus-ssr -p generational-box`.
   - Run the full four-command gate.
   - Run the host smokes and one manual WKWebView launch.
   - Do not combine the bump with feature work. The lockfile diff is large enough to review alone.
4. **Install `dx` 0.7.10 and fix the `~/bin/dx` shadowing**, for example with `cargo binstall
   dioxus-cli@0.7.10` and a renamed diagnostic script. Packaging work (C3 item 1) needs it.
5. **Revisit 0.8** only after it reaches stable and a migration guide exists. Watch for the objc2
   move, which removes the `cocoa` future-incompat warning.

### C3. Opportunities ranked by value against cost

Cost is expressed as dependencies and blast radius, not elapsed time.

| # | Opportunity | Value | Dependencies / cost | Verdict |
|---|---|---|---|---|
| 1 | **A working `.app`/DMG** through `dx bundle` with `asset!`-declared xterm files and `[bundle] external_bin` for `impulse-rs`, plus a launch smoke that checks for `data-impulse-host-status="dioxus-eval-bridge-ready"` and `data-xterm-state="mounted"` | Very high. It is R1's preferred route and the gate for retiring the Tauri adapter (`EGUI-DECOMMISSION.md:176-184`) | Needs C2 steps 1, 3 and 4. Touches `ui.rs:39-41,3175-3186`, adds `Dioxus.toml`, CI macOS runner, signing secrets | **Do first** |
| 2 | **Replace JS round-trips with direct Rust calls.** Provide `DesktopShellState` through `use_context_provider` from `LiveDesktopApp`. Turn actions into `spawn(async move { host_commands::…(state).await })`. Feed runtime events into signals through a `use_coroutine` that drains `event_rx` directly. Keep exactly one eval, for xterm only | High. It removes the stub/poll race (B4.3) and two serialization hops per action. Most cockpit logic becomes testable with plain `VirtualDom` + `tokio::test`, and less of the product depends on WebView behaviour | Needs C2 step 3 (Stores optional). Large edit in `ui.rs` and `host_bridge.rs`. Must keep the "host invoke wire" contract for the legacy adapter until it is retired (ADR-0008 consequences, `0008-dioxus-desktop-host.md:33-37`). Can be staged one action family at a time | **Do second**, incrementally |
| 3 | **Binary terminal transport.** Serve PTY output through `with_asynchronous_custom_protocol` (a streaming `impulse-pty://agent/<id>` fetch) or base64 chunks. Add a bounded channel with coalescing in place of the unbounded `channel_event_sink` | High for agent-heavy sessions. It removes the 3–4× JSON inflation and protects control messages from output floods (B4.4) | Independent of 2. Touches `host_bridge.rs:246`, `runtime.rs:326-331`, and the terminal script `ui.rs:487-546`. Needs a throughput benchmark to prove the gain | **Do third**, with a measurement first |
| 4 | **Blackboard viewer** for ADR-0023 `.impulse/blackboard.db`: a read-only, paged list (task_id, content_type, size, TTL) with an 8 KiB-window detail pane | Medium-high. It makes off-context agent state inspectable. The ADR is on unmerged `claude/ion-blackboard-20261003` (`86ff306`) | Needs ADR-0023 merged, a daemon read endpoint (the cockpit must not open the DB itself: "must not become a second policy or persistence authority", `CONTEXT.md:487-488`), and paging because WebKit list cost is high (A9) | After ADR-0023 lands |
| 5 | **Governed-task timeline**: per-task revision history (claim, verify, review, promote/discard, receipts) | Medium-high. The operator board already shows current state (`ui.rs:1681`), and history is what reviewers actually ask for | Needs a daemon history projection over `GOVERNED_TASKS.json` / `PRODUCER_RESERVATIONS.json`. A Store-backed list fits | After 2 |
| 6 | **Native affordances** from Dioxus itself: a muda menu bar, `use_global_shortcut` (focus agent N), and rfd for "register workspace…" | Medium. It closes three of the six unimplemented `NativeIslandKind`s (`native.rs:8-16`) without objc2 | Needs C2 step 3. Small, isolated | Opportunistic |
| 7 | **Stores for the snapshot fan-out.** Replace the ten signals plus full-vector clones (`ui.rs:3539-3568`) with one `#[derive(Store)] CockpitState` | Medium. It avoids re-rendering every panel on every `ops_update` | Needs C2 step 3. Pairs naturally with 2 | With 2 |
| 8 | **Subsecond hot-patching for cockpit development** | Low-medium. Only tip-crate edits patch (`impulse-desktop` yes, `impulse-ops` no). The feature is labelled experimental | Needs `dx` 0.7.10 (C2 step 4). Zero code cost | Try once; do not rely on it |
| 9 | **Remote or web cockpit** (fullstack or LiveView) for viewing a session from another device | Low now. It would expose daemon control over the network and conflicts with the local, same-UID operator model (ADR-0018) | Needs an auth design. LiveView is due to be folded into fullstack upstream | Defer |
| 10 | **Multiwindow per workspace** | Low. Each window is a separate VirtualDom with no shared signals ([#5160](https://github.com/DioxusLabs/dioxus/issues/5160)), so state would need to sync through the daemon anyway | Needs 2 | Defer |
| 11 | **Blitz / Dioxus-Native for a smaller, WebView-free binary** | Speculative. xterm.js needs a real JS engine, and Blitz has none. The terminal would need a native renderer, such as the existing `impulse-term` vt100 grid painted directly. No size claim from upstream to rely on | Needs Blitz maturity plus a native terminal widget | Watch only |

### C4. Risks and watch list

| Risk | Signal to watch | Mitigation |
|---|---|---|
| **Upstream continuity** (Cognition) | Releases resuming on the 0.7/0.8 line; Nico Burns' commit share; whether WebView-desktop issues (for example [#5885](https://github.com/DioxusLabs/dioxus/issues/5885)) get triage | The dual MIT/Apache license allows a fork. Keep the Dioxus-free core boundary (`runtime.rs` and friends), which caps exposure to the presentation layer. Avoid 0.8-only APIs until stable |
| **Priority drift to Blitz** | 0.8 changelog weighted to native and mobile, with desktop WebView bugs left open | Impulse's renderer is the WebView path. If it stagnates, wry and tao remain maintained by the Tauri project. A thin wry host is a credible fallback because the cockpit's state authority already lives outside Dioxus |
| **WKWebView behaviour** | Throttling of hidden windows ([#5586](https://github.com/DioxusLabs/dioxus/issues/5586)); WebContent crash leaves a blank window ([#5885](https://github.com/DioxusLabs/dioxus/issues/5885)) | Add a liveness ping on the eval channel and a reload path. Test in the real WebView, not only Chromium (B3) |
| **Terminal performance** | Output-heavy agents (test logs, builds) | C3 item 3. Measure bytes per second through the bridge before and after |
| **Test coupling to internals** | `FakeDocument`/`Evaluator`/`generational-box` (B3) survived 0.7.10 but are semi-public | C3 item 2 shrinks this surface. Pin `generational-box` to the matching Dioxus minor |
| **Compile time** | `ui.rs` at 3,591 lines of `rsx!` ([#5549](https://github.com/DioxusLabs/dioxus/issues/5549)) | Split `ui.rs` by panel when it is next edited for other reasons |
| **Debug/release divergence** | Any `rsx!` that moves a value used in `key:` or an attribute format | Add a release `cargo check` to CI (C2 step 1) |

---

## Unverified items

- The xterm 404 under an unbundled `cargo run` on macOS (B4.2). It is inferred from dioxus-desktop
  source, not observed at runtime.
- The deal terms of the Cognition move, and ownership of the Dioxus GitHub org and crates.io
  publishing rights.
- Any 0.8 stable date.
- Any upstream binary-size claim for Blitz.
- Whether a first-party virtualised-list component exists anywhere in the Dioxus ecosystem.
- That `askama_escape 0.13` is the source of the SSR escaping change. The behaviour itself was
  observed.
- The full Impulse workspace gate on the 0.7.10 lockfile. Only `impulse-desktop` was built and
  tested.
