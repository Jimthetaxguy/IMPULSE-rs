# Impulse Desktop UX Functional Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the Impulse Desktop app so workspace registration and terminal launch actually work, folder selection uses a native OS picker instead of a bare text field, and the bottom status bar / 3-column shell no longer silently clip content.

**Architecture:** Four independent, sequentially-committed fixes inside the `impulse-desktop` Dioxus crate: (1) close a startup race where the host-bridge adapter resolves before the live JS↔Rust transport installs, by making the shared JS resolver poll with a bounded retry instead of reading once; (2) wire the already-scaffolded-but-dead `NativeIslandKind::FileOpenPanel` path to a real `rfd`-backed native folder dialog, isolated from the single-consumer host-invoke FIFO via `spawn_blocking`; (3) fix two independent CSS clipping bugs (`.event-strip` status bar, `.workspace-grid` 3-column shell) so content wraps/scrolls instead of silently disappearing, with new Playwright visual-smoke coverage at a narrower viewport; (4) run the full verification gate plus a manual smoke of the packaged binary as ground truth.

**Tech Stack:** Rust, Dioxus 0.6.3 (desktop, `document::eval`-based JS↔Rust bridge), `rfd` 0.14 (native file dialogs), `tokio` (async runtime), plain CSS, Node.js-backed smoke tests (`tests/desktop_contract.rs`) plus a Playwright visual-smoke suite (`scripts/visual_smoke.mjs`).

## Global Constraints

- Never panic on production paths — every function returns `Result<T>`, no `unwrap()` outside tests/`Default`/`main()` (CLAUDE.md Principle 1).
- Every `Result`-returning function needs at least one test exercising its `Err` path (CLAUDE.md Testing Standards).
- Every new `Serialize + Deserialize` type needs a round-trip test (none introduced by this plan — all reused types already have coverage).
- Verification gate for every task: `cd impulse-rs && cargo build --workspace && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`.
- Stage explicit paths only — never `git add .`/`git add -A` (repo collaboration guide non-negotiable).
- No IA, copy, or overall visual-design changes — explicitly out of scope for this lane (see spec's Non-goals).
- No changes to `legacy-tauri-runtime`-gated code paths beyond what's already there.
- Work happens on branch `claude/desktop-ux-functional-fixes` in worktree `.worktrees/desktop-ux-functional-fixes`, already created off `origin/main` @ `b7a42bd` with a clean `cargo check --workspace` baseline (4m10s, 0 errors).

---

### Task 1: Fix the host-adapter resolution race with bounded retry

**Files:**
- Modify: `impulse-rs/impulse-desktop/src/ui.rs:42-72` (the `impulse_host_adapter_resolution_script!` macro)
- Modify: `impulse-rs/impulse-desktop/src/ui.rs:319-323` (`TERMINAL_INTEROP_SCRIPT`'s IIFE wrapper — sync → async)
- Modify: `impulse-rs/impulse-desktop/tests/desktop_contract.rs` (3 existing tests updated, 2 new regression tests, 1 new fast unit test)

**Interfaces:**
- Consumes: existing `desktop_event_bridge_script() -> &'static str` (`ui.rs:429`), `impulse_desktop::ui::terminal_interop_script() -> &'static str` (`ui.rs:425`), `impulse_desktop::host_commands::PENDING_HOST_BOOTSTRAP_STATUS` (already public), `skip_without_node() -> bool` (`tests/desktop_contract.rs:627`).
- Produces: no new public Rust API — only the expanded JS text inside both scripts changes (both scripts stay `&'static str`, callers unchanged). Later tasks do not depend on anything from this task.

- [ ] **Step 1: Write the two new regression tests proving the race is fixed**

Add to `impulse-rs/impulse-desktop/tests/desktop_contract.rs`, near the existing `run_pending_host_bridge_smoke` helper (search for that function to find the right neighborhood — these tests reuse its `skip_without_node()` guard pattern):

```rust
/// Regression test for the resolver race: `use_live_host_bridge()`
/// (host_bridge.rs) and this ops-bridge resolver both read/write
/// `window.__IMPULSE_DESKTOP_HOST` from independent, unordered
/// `document::eval` calls. This drives the real bridge script against a host
/// that starts as the manifest-only pending stub and only becomes the live
/// bridge ~30ms later (well inside the resolver's bounded poll budget),
/// simulating the live bridge installing *after* this script's first tick.
/// Before the bounded-retry fix, the one-shot resolver would have
/// permanently locked onto the pending stub and never recovered.
#[test]
fn test_desktop_event_bridge_resolver_recovers_from_late_installing_live_host_bridge() {
    if skip_without_node() {
        return;
    }

    let smoke_script = format!(
        r#"
const bridgeScript = {bridge_script};
const pendingStatus = {pending_status};
const liveStatus = {live_status};
const sent = [];
const invoked = [];
const listeners = {{}};
const attrs = {{}};

global.window = {{}};
global.document = {{
  documentElement: {{
    setAttribute: (key, value) => {{ attrs[key] = value; }}
  }}
}};
global.dioxus = {{ send: (message) => sent.push(message) }};

const pending = (operation) =>
  Promise.reject(new Error(`Dioxus Desktop host adapter pending: ${{operation}}`));
const pendingInvoke = (command) => pending(`invoke:${{command}}`);
const pendingListen = (event) => pending(`listen:${{event}}`);
pendingInvoke.__impulseHostPending = true;
pendingListen.__impulseHostPending = true;
window.__IMPULSE_DESKTOP_HOST = {{
  invoke: pendingInvoke,
  listen: pendingListen,
  hostKind: "dioxus",
  status: pendingStatus,
}};

// Simulate `use_live_host_bridge()`'s independent `document::eval` call
// installing the real transport a beat *after* this script starts, well
// inside the resolver's bounded poll window but strictly after its first
// (necessarily-not-ready) tick.
setTimeout(() => {{
  window.__IMPULSE_DESKTOP_HOST = {{
    invoke: async (command) => {{ invoked.push({{ command }}); return command === "agent_snapshot" ? [] : null; }},
    listen: async (event, handler) => {{ listeners[event] = handler; return async () => {{}}; }},
    hostKind: "dioxus",
    status: liveStatus,
  }};
}}, 30);

const bridgePromise = eval(bridgeScript);
if (bridgePromise && typeof bridgePromise.catch === "function") {{
  bridgePromise.catch((error) => {{
    console.error(error && error.stack ? error.stack : String(error));
    process.exit(1);
  }});
}}
setTimeout(() => {{
  console.log(JSON.stringify({{ attrs, sent, invoked, bridge: window.__impulseOpsBridge }}));
  process.exit(0);
}}, 400);
"#,
        bridge_script =
            serde_json::to_string(desktop_event_bridge_script()).expect("serialize bridge script"),
        pending_status =
            serde_json::to_string(impulse_desktop::host_commands::PENDING_HOST_BOOTSTRAP_STATUS)
                .expect("serialize pending status"),
        live_status = serde_json::to_string("dioxus-eval-bridge-ready")
            .expect("serialize live status"),
    );

    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir.path().join("late-installing-host-bridge-smoke.js");
    std::fs::write(&smoke_path, smoke_script).expect("write late-installing host smoke script");
    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run node late-installing host bridge smoke");
    assert!(
        output.status.success(),
        "late-installing host bridge smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let smoke: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("parse late-installing host bridge smoke output");

    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge"],
        serde_json::Value::String("mounted".to_string()),
        "resolver should have recovered once the live bridge installed: {smoke}"
    );
    assert_eq!(smoke["bridge"]["degraded"], serde_json::Value::Bool(false));
    let invoked = smoke["invoked"].as_array().expect("invoked array");
    assert!(
        invoked.iter().any(|call| call["command"] == "agent_snapshot"),
        "recovered live transport should receive bridge refresh invokes, got {invoked:?}"
    );
}

/// Same regression, isolated to the terminal-interop resolver: a *single*
/// `eval(interopScript)` invocation must recover mid-poll and mount, not
/// permanently resolve to "degraded" on the first (necessarily-pending)
/// check.
#[test]
fn test_terminal_interop_resolver_recovers_from_late_installing_live_host_bridge() {
    if skip_without_node() {
        return;
    }

    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir.path().join("terminal-interop-late-host-smoke.js");
    let smoke = format!(
        r#"
const assert = require("assert");
const interopScript = {interop_script};
const pendingStatus = {pending_status};

(async () => {{

const mounts = [{{
  dataset: {{ agentId: "codex" }},
  attrs: {{}},
  setAttribute(name, value) {{ this.attrs[name] = value; }},
}}];
global.document = {{
  querySelectorAll(selector) {{
    assert.strictEqual(selector, "[data-xterm-mount='true']");
    return mounts;
  }},
}};

const pending = (operation) =>
  Promise.reject(new Error(`Dioxus Desktop host adapter pending: ${{operation}}`));
const pendingInvoke = () => pending("invoke");
const pendingListen = () => pending("listen");
pendingInvoke.__impulseHostPending = true;
pendingListen.__impulseHostPending = true;
global.window = {{
  __IMPULSE_DESKTOP_HOST: {{
    invoke: pendingInvoke,
    listen: pendingListen,
    hostKind: "dioxus",
    status: pendingStatus,
  }},
}};

class Terminal {{
  constructor() {{ this.writes = []; }}
  loadAddon() {{}}
  open(mount) {{ mount.opened = true; }}
  onData() {{}}
  onResize() {{}}
  write(value) {{ this.writes.push(value); }}
}}
window.Terminal = Terminal;
window.FitAddon = class {{ fit() {{}} }};

// Simulate the live bridge (host_bridge.rs's `use_live_host_bridge()`)
// replacing the pending stub partway through this script's own resolution
// poll, well inside its bounded retry budget.
setTimeout(() => {{
  window.__IMPULSE_DESKTOP_HOST = {{
    invoke: async () => null,
    listen: async (event, handler) => () => {{}},
    hostKind: "dioxus",
    status: "dioxus-eval-bridge-ready",
  }};
}}, 30);

const result = await eval(interopScript);
assert.strictEqual(
  result,
  "mounted",
  `expected the resolver to recover once the live bridge installed, got ${{result}}`
);
assert.strictEqual(mounts[0].attrs["data-xterm-state"], "mounted");
}})().catch((error) => {{
  console.error(error);
  process.exit(1);
}});
"#,
        interop_script = serde_json::to_string(impulse_desktop::ui::terminal_interop_script())
            .expect("serialize interop script"),
        pending_status =
            serde_json::to_string(impulse_desktop::host_commands::PENDING_HOST_BOOTSTRAP_STATUS)
                .expect("serialize pending status"),
    );
    std::fs::write(&smoke_path, smoke).expect("write terminal interop late-host smoke");

    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run terminal interop late-host smoke");
    assert!(
        output.status.success(),
        "terminal interop late-host smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
```

- [ ] **Step 2: Run the two new tests to verify they fail (red)**

Run: `cd impulse-rs && cargo test -p impulse-desktop test_desktop_event_bridge_resolver_recovers_from_late_installing_live_host_bridge test_terminal_interop_resolver_recovers_from_late_installing_live_host_bridge -- --nocapture`
Expected: both FAIL (the current one-shot resolver locks onto the pending stub; `data-impulse-ops-bridge` stays `"degraded"` and the terminal-interop result stays `"degraded"` instead of `"mounted"`). If `node` isn't on `PATH`, both tests report skipped instead — install Node or run this step on a machine with it before continuing, since this is the load-bearing proof for the whole task.

- [ ] **Step 3: Replace the macro body with the bounded-retry resolver**

In `impulse-rs/impulse-desktop/src/ui.rs`, replace the entire `impulse_host_adapter_resolution_script!` macro (current lines 42-72) with:

```rust
macro_rules! impulse_host_adapter_resolution_script {
    () => {
        r#"
  // Keep in sync with host_commands::PENDING_HOST_BOOTSTRAP_STATUS — the
  // manifest-only Dioxus bootstrap installs invoke/listen stubs that always
  // reject until the live eval bridge replaces them. Treating those stubs as a
  // live API makes the bridges advertise themselves mounted and then
  // unhandled-reject on the first call, so we must detect and skip them.
  const PENDING_IMPULSE_HOST_STATUS = "manifest-only-pending-dioxus-eval-bridge";
  const impulseHostFnReady = (host, fn) =>
    !!host &&
    typeof host[fn] === "function" &&
    host.status !== PENDING_IMPULSE_HOST_STATUS &&
    !host[fn].__impulseHostPending;
  // `use_live_host_bridge()` (host_bridge.rs) installs the live host adapter
  // from an independent `document::eval` call mounted alongside (not before)
  // this resolver. Dioxus gives no ordering guarantee between the two async
  // tasks, and the webview gives none between the two separate eval calls
  // either, so a single unconditional read of `window.__IMPULSE_DESKTOP_HOST`
  // here is a race: if this script's first tick runs before the live bridge
  // replaces the manifest-only stub, it would decide the live host is
  // "missing" and that verdict gets captured for the rest of this script's
  // lifetime (both callers close over `invoke`/`listen` in long-lived
  // closures — one of them never re-executes for the life of the component).
  // Poll with a short bounded backoff until the live bridge (or a legacy
  // Tauri host) reports ready, or the attempt budget is exhausted, then fall
  // back to today's resolution exactly as before.
  const IMPULSE_HOST_ADAPTER_POLL_INTERVAL_MS = 10;
  const IMPULSE_HOST_ADAPTER_POLL_MAX_ATTEMPTS = 25;
  const impulseHostAdapterCandidateReady = () => {
    const dioxusHost = window.__IMPULSE_DESKTOP_HOST;
    const legacyTauri = window.__TAURI__;
    return (
      impulseHostFnReady(dioxusHost, "invoke") ||
      impulseHostFnReady(dioxusHost, "listen") ||
      !!legacyTauri?.core?.invoke ||
      !!legacyTauri?.event?.listen
    );
  };
  const resolveImpulseHostAdapter = async () => {
    for (
      let attempt = 0;
      attempt < IMPULSE_HOST_ADAPTER_POLL_MAX_ATTEMPTS && !impulseHostAdapterCandidateReady();
      attempt += 1
    ) {
      await new Promise((resolve) =>
        setTimeout(resolve, IMPULSE_HOST_ADAPTER_POLL_INTERVAL_MS)
      );
    }
    const dioxusHost = window.__IMPULSE_DESKTOP_HOST;
    const legacyTauri = window.__TAURI__;
    return {
      invoke: impulseHostFnReady(dioxusHost, "invoke")
        ? dioxusHost.invoke
        : legacyTauri?.core?.invoke,
      listen: impulseHostFnReady(dioxusHost, "listen")
        ? dioxusHost.listen
        : legacyTauri?.event?.listen,
      hostKind: dioxusHost ? "dioxus" : legacyTauri ? "legacy-tauri" : "missing",
    };
  };
  const { invoke, listen, hostKind } = await resolveImpulseHostAdapter();
"#
    };
}
```

(Worst-case added latency: `10ms × 25 = 250ms`, only on the genuine "host never arrives" path. In the normal case the first `impulseHostAdapterCandidateReady()` check typically succeeds immediately.)

- [ ] **Step 4: Make `TERMINAL_INTEROP_SCRIPT`'s wrapper async**

In `impulse-rs/impulse-desktop/src/ui.rs`, find the `TERMINAL_INTEROP_SCRIPT` constant definition (current lines 319-323, a `concat!(...)` whose first raw-string segment starts with `(() => {`). Change only the opening IIFE line:

```diff
 const TERMINAL_INTEROP_SCRIPT: &str = concat!(
     r#"
-(() => {
+(async () => {
 "#,
     impulse_host_adapter_resolution_script!(),
```

Everything after that (the rest of `TERMINAL_INTEROP_SCRIPT`'s body, through its trailing `})();`) is unchanged — a `})();` closer is valid for both sync and async arrow-function IIFEs, and `document::eval(TERMINAL_INTEROP_SCRIPT).await` at the call site (`ui.rs`, inside `DesktopShell`'s first `use_effect`) already discards the return value and already awaits the eval, so no Rust call-site changes are needed.

- [ ] **Step 5: Update the three existing tests that this change affects**

In `impulse-rs/impulse-desktop/tests/desktop_contract.rs`:

**5a.** `test_terminal_interop_prefers_dioxus_native_host_adapter` (around line 554) — the resolver call is now awaited:
```diff
-    assert!(script.contains("const { invoke, listen, hostKind } = resolveImpulseHostAdapter();"));
+    assert!(script.contains("const { invoke, listen, hostKind } = await resolveImpulseHostAdapter();"));
```

**5b.** `test_terminal_interop_rerun_mounts_new_panes_without_duplicate_listeners` (around lines 2491 and 2502) — `eval(interopScript)` now returns a `Promise<string>` instead of a bare string, since the script is an async IIFE. The test body is already wrapped in `(async () => { ... })()`, so `await` is legal at both call sites:
```diff
-const first = eval(interopScript);
+const first = await eval(interopScript);
 await Promise.resolve();
 await Promise.resolve();
 assert.strictEqual(first, "mounted");
 ...
-const second = eval(interopScript);
+const second = await eval(interopScript);
 await Promise.resolve();
 await Promise.resolve();
 assert.strictEqual(second, "mounted");
```

**5c.** The shared `run_pending_host_bridge_smoke` helper (around line 702) — its snapshot-capture delay must be long enough to outlast the resolver's new 250ms poll budget in the "host genuinely never arrives" case (used by `test_pending_dioxus_host_ops_bridge_fails_closed`; `test_pending_dioxus_host_falls_back_to_legacy_tauri` shares this harness but resolves on its first check since `window.__TAURI__` is already present, so it's unaffected in substance and just inherits the larger window):
```diff
 setTimeout(() => {{
   console.log(JSON.stringify({{ attrs, sent, invoked, bridge: window.__impulseOpsBridge }}));
   process.exit(0);
-}}, 40);
+}}, 400);
```

- [ ] **Step 6: Run the two new regression tests again to verify they pass (green)**

Run: `cd impulse-rs && cargo test -p impulse-desktop test_desktop_event_bridge_resolver_recovers_from_late_installing_live_host_bridge test_terminal_interop_resolver_recovers_from_late_installing_live_host_bridge -- --nocapture`
Expected: both PASS.

- [ ] **Step 7: Add the fast, no-Node unit test pinning the shared-macro contract**

Add to `impulse-rs/impulse-desktop/tests/desktop_contract.rs`:

```rust
/// Both host-adapter-resolution consumers share one macro; this pins the
/// bounded-retry contract in place so a future edit can't silently drop the
/// polling loop from one script while keeping it in the other.
#[test]
fn test_impulse_host_adapter_resolution_script_declares_bounded_retry_contract() {
    for script in [desktop_event_bridge_script(), impulse_desktop::ui::terminal_interop_script()] {
        assert!(script.contains("IMPULSE_HOST_ADAPTER_POLL_INTERVAL_MS"));
        assert!(script.contains("IMPULSE_HOST_ADAPTER_POLL_MAX_ATTEMPTS"));
        assert!(script.contains("impulseHostAdapterCandidateReady"));
        assert!(script.contains("const resolveImpulseHostAdapter = async ()"));
        assert!(script.contains("await resolveImpulseHostAdapter()"));
    }
    assert!(
        impulse_desktop::ui::terminal_interop_script()
            .trim_start()
            .starts_with("(async () => {"),
        "terminal interop script must be an async IIFE for the resolver's await to be legal"
    );
}
```

- [ ] **Step 8: Run the full `impulse-desktop` package test suite**

Run: `cd impulse-rs && cargo test -p impulse-desktop 2>&1 | tail -30`
Expected: PASS — all previously-passing tests still pass (including the three updated in Step 5), plus the two new regression tests and the new contract test.

- [ ] **Step 9: Commit**

```bash
cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes
git add impulse-rs/impulse-desktop/src/ui.rs impulse-rs/impulse-desktop/tests/desktop_contract.rs
git commit -m "fix(desktop): close host-adapter resolution race with bounded retry

The shared JS resolver macro used by DESKTOP_EVENT_BRIDGE_SCRIPT and
TERMINAL_INTEROP_SCRIPT read window.__IMPULSE_DESKTOP_HOST exactly
once. use_live_host_bridge() installs the live host from an
independent, unordered document::eval call, so if resolution ran
first it permanently locked onto the pending stub for the life of the
component - explaining why registering a workspace or launching an
agent could silently do nothing forever. The resolver now polls with
a bounded 10ms x 25 backoff before falling back to today's behavior.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 2: Add a native macOS folder picker to the Workspace Launcher

**Files:**
- Modify: `impulse-rs/impulse-desktop/Cargo.toml` (add `rfd` dependency, gated behind `desktop-app`)
- Modify: `impulse-rs/impulse-desktop/src/native.rs` (implement `NativeIslandKind::FileOpenPanel`)
- Modify: `impulse-rs/impulse-desktop/src/host_commands.rs` (isolate the blocking dialog via `spawn_blocking`)
- Modify: `impulse-rs/impulse-desktop/src/ui.rs` (JS bridge method, Rust glue functions, rsx "Browse…" button)
- Modify: `impulse-rs/impulse-desktop/assets/impulse_crt.css` (inline input+button layout)
- Modify: `impulse-rs/impulse-desktop/tests/desktop_contract.rs` and `tests/host_surface.rs` (new tests)

**Interfaces:**
- Consumes: `NativeIslandRequest { request_id: String, kind: NativeIslandKind, payload: Value }`, `NativeIslandResult { request_id: String, kind: NativeIslandKind, handled: bool, payload: Value }`, `NativeIslandKind::FileOpenPanel` (already declared, `#[serde(rename_all = "snake_case")]` → JSON tag `"file_open_panel"`), `DesktopBridgeError::NativeIslandFailed { message: String }` (already declared in `src/bridge.rs`), `err_to_string<E: std::fmt::Display>` (already at `host_commands.rs:76`).
- Produces: `pub trait FolderPicker` + `pub struct RfdFolderPicker` + `fn file_open_panel_with(...)` (all in `native.rs`, task-local — no other task depends on these); `pub fn workspace_folder_pick_bridge_script(&NativeIslandRequest) -> String`, `pub fn workspace_folder_pick_request(&str) -> NativeIslandRequest`, `pub fn parse_workspace_folder_pick_result(Result<Value, String>) -> Result<Option<String>, String>` (all in `ui.rs`, task-local).

- [ ] **Step 1: Add the `rfd` dependency**

In `impulse-rs/impulse-desktop/Cargo.toml`:

```diff
 [features]
 default = []
-desktop-app = ["dep:dioxus-desktop", "dioxus/desktop"]
+desktop-app = ["dep:dioxus-desktop", "dioxus/desktop", "dep:rfd"]
 native-macos = [
     "dep:objc2",
     "dep:objc2-app-kit",
     "dep:objc2-foundation",
 ]
```

```diff
 [dependencies]
 dioxus = { version = "0.6.3", default-features = false, features = ["minimal", "document"] }
 dioxus-desktop = { version = "0.6.3", optional = true }
 impulse-ops = { path = "../impulse-ops" }
 impulse-term = { path = "../impulse-term", default-features = false }
+rfd = { version = "0.14", optional = true, default-features = false, features = ["xdg-portal", "tokio"] }
 serde = { version = "1", features = ["derive"] }
 serde_json = "1"
 thiserror = "2"
 uuid = { version = "1", features = ["v4", "serde"] }
 tokio = { version = "1", default-features = false, features = ["sync", "macros", "rt"] }
 tauri = { version = "2.11.2", optional = true, default-features = false }
```

Pinned to `"0.14"` (not the latest `0.17.x`) because `dioxus-desktop 0.6.3` already depends on `rfd = "^0.14"` — pinning the same range lets Cargo unify onto the already-resolved `0.14.1` in `Cargo.lock` instead of resolving two incompatible `rfd` majors into one binary. `optional = true` (gated behind `desktop-app`, not unconditional) so the lib crate's default build (used by non-GUI tests) doesn't pick up `rfd`'s system dependencies.

Run: `cd impulse-rs && cargo check -p impulse-desktop --features desktop-app 2>&1 | tail -20`
Expected: succeeds (resolves `rfd` to `0.14.1` from the existing lockfile, no new major version pulled in). This step has no test of its own — it's a dependency addition; Step 2 below adds a `#[cfg(test)]` module for the code that will use it.

- [ ] **Step 2: Write failing tests for the pure dispatch logic (`file_open_panel_with`)**

Add a new test module to `impulse-rs/impulse-desktop/src/native.rs` (none exists in this file yet):

```rust
#[cfg(test)]
mod file_open_panel_tests {
    use super::*;

    enum FakeOutcome {
        Picked(PathBuf),
        Cancelled,
        Failed(String),
    }

    struct FakePicker {
        outcome: FakeOutcome,
        seen: std::cell::RefCell<Option<(String, Option<String>)>>,
    }

    impl FakePicker {
        fn new(outcome: FakeOutcome) -> Self {
            Self { outcome, seen: std::cell::RefCell::new(None) }
        }
    }

    impl FolderPicker for FakePicker {
        fn pick_folder(
            &self,
            title: &str,
            starting_directory: Option<&str>,
        ) -> Result<Option<PathBuf>, DesktopBridgeError> {
            *self.seen.borrow_mut() =
                Some((title.to_string(), starting_directory.map(str::to_string)));
            match &self.outcome {
                FakeOutcome::Picked(path) => Ok(Some(path.clone())),
                FakeOutcome::Cancelled => Ok(None),
                FakeOutcome::Failed(message) => {
                    Err(DesktopBridgeError::NativeIslandFailed { message: message.clone() })
                }
            }
        }
    }

    fn request(payload: Value) -> NativeIslandRequest {
        NativeIslandRequest {
            request_id: "req-1".to_string(),
            kind: NativeIslandKind::FileOpenPanel,
            payload,
        }
    }

    #[test]
    fn test_file_open_panel_with_returns_selected_path_on_pick() {
        let picker = FakePicker::new(FakeOutcome::Picked(PathBuf::from("/tmp/project")));
        let result = file_open_panel_with(request(empty_payload()), &picker).unwrap();

        assert!(result.handled);
        assert_eq!(result.kind, NativeIslandKind::FileOpenPanel);
        assert_eq!(result.payload["path"], "/tmp/project");
        assert_eq!(result.payload["cancelled"], false);
    }

    #[test]
    fn test_file_open_panel_with_returns_cancelled_when_user_cancels() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        let result = file_open_panel_with(request(empty_payload()), &picker).unwrap();

        assert!(result.handled);
        assert!(result.payload["path"].is_null());
        assert_eq!(result.payload["cancelled"], true);
    }

    #[test]
    fn test_file_open_panel_with_propagates_picker_error() {
        let picker = FakePicker::new(FakeOutcome::Failed("no display".to_string()));
        let error = file_open_panel_with(request(empty_payload()), &picker).unwrap_err();

        assert_eq!(
            error,
            DesktopBridgeError::NativeIslandFailed { message: "no display".to_string() }
        );
    }

    #[test]
    fn test_file_open_panel_with_uses_default_title_when_payload_empty() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        file_open_panel_with(request(empty_payload()), &picker).unwrap();

        let (title, starting_directory) = picker.seen.borrow().clone().unwrap();
        assert_eq!(title, "Select a folder");
        assert_eq!(starting_directory, None);
    }

    #[test]
    fn test_file_open_panel_with_forwards_custom_title_and_starting_directory() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        let payload = json!({ "title": "Select a project folder", "starting_directory": "/tmp" });
        file_open_panel_with(request(payload), &picker).unwrap();

        let (title, starting_directory) = picker.seen.borrow().clone().unwrap();
        assert_eq!(title, "Select a project folder");
        assert_eq!(starting_directory, Some("/tmp".to_string()));
    }

    #[test]
    fn test_dispatch_reports_unhandled_without_desktop_app_feature() {
        // Exercises the cfg(not(feature = "desktop-app")) branch, which is
        // what a bare `cargo test --workspace` compiles by default.
        let result = DefaultNativeIslandHost
            .dispatch(request(empty_payload()))
            .unwrap();
        #[cfg(not(feature = "desktop-app"))]
        {
            assert!(!result.handled);
            assert_eq!(result.kind, NativeIslandKind::FileOpenPanel);
        }
        #[cfg(feature = "desktop-app")]
        {
            let _ = result; // real picker path — see manual verification in Step 10
        }
    }
}
```

- [ ] **Step 3: Run the new tests to verify they fail (red)**

Run: `cd impulse-rs && cargo test -p impulse-desktop file_open_panel_tests 2>&1 | tail -30`
Expected: FAILS to compile — `FolderPicker`, `file_open_panel_with`, `PathBuf` (unimported), `Value`/`json!` usage against `NativeIslandKind::FileOpenPanel`'s still-unrouted dispatch arm don't exist yet.

- [ ] **Step 4: Implement `FolderPicker`, `RfdFolderPicker`, and `file_open_panel_with` in `native.rs`**

Add `use std::path::PathBuf;` near the top of `impulse-rs/impulse-desktop/src/native.rs` (alongside the existing `use serde::{Deserialize, Serialize};` and `use serde_json::{json, Value};`).

Update the `dispatch` match arm inside `impl NativeIslandHost for DefaultNativeIslandHost` to route `FileOpenPanel`:

```diff
     fn dispatch(
         &self,
         request: NativeIslandRequest,
     ) -> Result<NativeIslandResult, DesktopBridgeError> {
         match request.kind {
             NativeIslandKind::AppKitProbe => probe_appkit(request),
+            NativeIslandKind::FileOpenPanel => dispatch_file_open_panel(request),
             kind => Err(DesktopBridgeError::UnsupportedNativeIsland {
                 kind: kind.as_str().to_string(),
             }),
         }
     }
```

Then add these items (after `probe_appkit`'s disabled-feature fn, i.e. at the end of the file before the new test module from Step 2):

```rust
/// Abstraction over "ask the OS for a folder path." Exists so
/// [`file_open_panel_with`] can be exercised deterministically in tests
/// without a real display — [`RfdFolderPicker`] is the only implementation
/// that ever touches the OS.
pub trait FolderPicker {
    /// `Ok(Some(path))` — the user picked a folder.
    /// `Ok(None)` — the user cancelled the dialog. This is a normal outcome,
    /// never an error.
    /// `Err(_)` — the picker itself failed (e.g. no display available).
    fn pick_folder(
        &self,
        title: &str,
        starting_directory: Option<&str>,
    ) -> Result<Option<PathBuf>, DesktopBridgeError>;
}

/// Native folder picker backed by `rfd`. The synchronous `FileDialog`
/// (not `AsyncFileDialog`) is used deliberately: rfd's own docs confirm
/// dialogs may run "from any thread ... in a windowed app" (Dioxus Desktop
/// is one), so the blocking call is safe as long as the *caller* keeps it
/// off the single-consumer host-invoke FIFO thread — see
/// `host_commands::native_island_request`, which wraps this dispatch in
/// `tokio::task::spawn_blocking` for exactly that reason.
#[cfg(feature = "desktop-app")]
pub struct RfdFolderPicker;

#[cfg(feature = "desktop-app")]
impl FolderPicker for RfdFolderPicker {
    fn pick_folder(
        &self,
        title: &str,
        starting_directory: Option<&str>,
    ) -> Result<Option<PathBuf>, DesktopBridgeError> {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(directory) = starting_directory {
            dialog = dialog.set_directory(directory);
        }
        // `pick_folder` is infallible in rfd's own API (`Option<PathBuf>`,
        // no `Result`); `None` means "user cancelled," not an error.
        Ok(dialog.pick_folder())
    }
}

/// Pure dispatch logic for [`NativeIslandKind::FileOpenPanel`], independent
/// of which [`FolderPicker`] answers it. Never panics: a cancelled dialog is
/// a normal `Ok` result (`handled: true`, `payload.cancelled: true`,
/// `payload.path: null`), not an error — only a genuine picker failure
/// propagates as `Err`.
fn file_open_panel_with(
    request: NativeIslandRequest,
    picker: &dyn FolderPicker,
) -> Result<NativeIslandResult, DesktopBridgeError> {
    let title = request
        .payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Select a folder");
    let starting_directory = request.payload.get("starting_directory").and_then(Value::as_str);

    let selection = picker.pick_folder(title, starting_directory)?;

    let payload = match &selection {
        Some(path) => json!({
            "path": path.to_string_lossy(),
            "cancelled": false,
        }),
        None => json!({
            "path": Value::Null,
            "cancelled": true,
        }),
    };

    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::FileOpenPanel,
        handled: true,
        payload,
    })
}

#[cfg(feature = "desktop-app")]
fn dispatch_file_open_panel(request: NativeIslandRequest) -> Result<NativeIslandResult, DesktopBridgeError> {
    file_open_panel_with(request, &RfdFolderPicker)
}

#[cfg(not(feature = "desktop-app"))]
fn dispatch_file_open_panel(request: NativeIslandRequest) -> Result<NativeIslandResult, DesktopBridgeError> {
    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::FileOpenPanel,
        handled: false,
        payload: json!({
            "reason": "desktop-app feature not enabled",
            "path": Value::Null,
            "cancelled": false,
        }),
    })
}
```

Note `FolderPicker` and `file_open_panel_with` are **not** `cfg`-gated — only `RfdFolderPicker` and the two `dispatch_file_open_panel` variants touch `rfd`/the feature flag — so Step 3's tests compile and run under a bare `cargo test --workspace` with zero extra features.

- [ ] **Step 5: Run the tests again to verify they pass (green)**

Run: `cd impulse-rs && cargo test -p impulse-desktop file_open_panel_tests 2>&1 | tail -30`
Expected: PASS, all 6 tests green.

- [ ] **Step 6: Isolate the blocking dialog from the single-consumer host-invoke FIFO**

`dispatch_host_invokes_fifo` (`host_bridge.rs:172-183`) drains one `HostInvokeRequest` at a time on a single tokio task and awaits each dispatch before pulling the next. A synchronous, modal, user-controlled dialog called inline here would stall every other host command (terminal writes, agent snapshots, MCP invokes, …) for as long as the picker stays open. Fix by moving the blocking call onto tokio's blocking-thread pool.

In `impulse-rs/impulse-desktop/src/host_commands.rs`:

```diff
-use crate::native::{DefaultNativeIslandHost, NativeIslandRequest, NativeIslandResult};
+use crate::native::{DefaultNativeIslandHost, NativeIslandKind, NativeIslandRequest, NativeIslandResult};
```

```diff
 #[cfg_attr(feature = "legacy-tauri-runtime", tauri::command)]
 pub async fn native_island_request(
     request: NativeIslandRequest,
 ) -> Result<NativeIslandResult, String> {
+    // `FileOpenPanel` opens a real, potentially long-lived modal OS dialog.
+    // `dispatch_host_invokes_fifo` (host_bridge.rs) drains host invokes one
+    // at a time on a single tokio task, so calling a blocking dialog inline
+    // here would stall every other host command (terminal writes, agent
+    // snapshots, ...) for as long as the picker stays open. `spawn_blocking`
+    // moves the blocking call onto tokio's dedicated blocking-thread pool
+    // instead — it only needs the already-enabled `rt` Cargo feature and
+    // works even on a current-thread runtime, and it never blocks the FIFO
+    // worker.
+    if matches!(request.kind, NativeIslandKind::FileOpenPanel) {
+        return tokio::task::spawn_blocking(move || DefaultNativeIslandHost.dispatch(request))
+            .await
+            .map_err(err_to_string)?
+            .map_err(err_to_string);
+    }
     DefaultNativeIslandHost
         .dispatch(request)
         .map_err(err_to_string)
 }
```

(`err_to_string<E: std::fmt::Display>` already exists at `host_commands.rs:76`; `tokio::JoinError` implements `Display`, so it composes directly.)

Add to `impulse-rs/impulse-desktop/tests/host_surface.rs` (matching the existing `native_island_request` test convention around line 136):

```rust
#[tokio::test]
async fn test_host_native_island_file_open_panel_is_unhandled_without_desktop_app_feature() {
    let result = host_commands::native_island_request(NativeIslandRequest {
        request_id: "host-folder".to_string(),
        kind: NativeIslandKind::FileOpenPanel,
        payload: json!({}),
    })
    .await
    .expect("native island command should route");

    #[cfg(not(feature = "desktop-app"))]
    assert!(!result.handled);
}
```

Run: `cd impulse-rs && cargo test -p impulse-desktop test_host_native_island_file_open_panel_is_unhandled_without_desktop_app_feature 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Add the JS bridge method and Rust glue functions in `ui.rs`**

Import, inserted after `use crate::mcp::{...};` (current line 22) in `impulse-rs/impulse-desktop/src/ui.rs`:
```rust
use crate::native::{NativeIslandKind, NativeIslandRequest, NativeIslandResult};
```

New JS bridge method inside `DESKTOP_EVENT_BRIDGE_SCRIPT`'s `window.__impulseOpsBridge` object literal — insert after the `registerWorkspace` method block closes (current line 207), before `invokeMcp` (current line 208):
```js
  window.__impulseOpsBridge.pickWorkspaceFolder = async (request) => {
    if (!invoke) {
      forward("bridge_status", { status: "workspace_folder_pick_failed", reason: "host invoke API unavailable" });
      return null;
    }
    try {
      return await invoke("native_island_request", { request });
    } catch (error) {
      forward("bridge_status", {
        status: "workspace_folder_pick_failed",
        reason: String(error),
        request,
      });
      throw error;
    }
  };
```

New Rust helper functions — insert after `mcp_invoke_bridge_script` (current line 459), before `agent_launch_bridge_script`:
```rust
pub fn workspace_folder_pick_bridge_script(request: &NativeIslandRequest) -> String {
    let payload = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"(async () => {{
  const bridge = window.__impulseOpsBridge;
  if (!bridge?.pickWorkspaceFolder) {{
    console.warn("impulse workspace folder picker bridge unavailable");
    return null;
  }}
  return await bridge.pickWorkspaceFolder({payload});
}})();"#
    )
}

/// Build the request for a "Browse…" click: a fresh correlation id, the
/// `FileOpenPanel` island kind, and a small payload the native picker reads
/// for the dialog title / starting directory. `current_root` only seeds
/// `starting_directory` when it is non-empty, so an in-progress typed value
/// isn't silently discarded by an empty default.
pub fn workspace_folder_pick_request(current_root: &str) -> NativeIslandRequest {
    let mut payload = serde_json::json!({ "title": "Select a project folder" });
    let trimmed = current_root.trim();
    if !trimmed.is_empty() {
        payload["starting_directory"] = Value::String(trimmed.to_string());
    }
    NativeIslandRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        kind: NativeIslandKind::FileOpenPanel,
        payload,
    }
}

/// Parse the JS bridge's return value for a folder-pick round trip. Never
/// panics:
/// - `Ok(Some(path))` — the user picked a folder.
/// - `Ok(None)` — the dialog was cancelled, or the host reported
///   `handled: false` (bridge/feature unavailable), or the bridge itself
///   returned `null` — all three are "nothing to apply," not failures the
///   caller must react to differently.
/// - `Err(reason)` — the eval bridge rejected, or the payload could not be
///   parsed as a `NativeIslandResult`.
///
/// Takes `Result<Value, String>` rather than Dioxus's own `EvalError` so this
/// function has zero Dioxus-runtime dependency and is trivially unit
/// testable; callers convert with `.map_err(|error| error.to_string())`.
pub fn parse_workspace_folder_pick_result(
    eval_result: Result<Value, String>,
) -> Result<Option<String>, String> {
    let value = eval_result?;
    if value.is_null() {
        return Ok(None);
    }
    let result: NativeIslandResult = serde_json::from_value(value)
        .map_err(|error| format!("invalid native_island_request result: {error}"))?;
    if !result.handled {
        return Ok(None);
    }
    let path = result
        .payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(path)
}
```

- [ ] **Step 8: Write failing tests for the new glue functions**

Add to `impulse-rs/impulse-desktop/tests/desktop_contract.rs`, alongside `test_workspace_registration_bridge_script_serializes_project_notes` (line 1344), matching the existing `assert!(script.contains(...))` convention used for every other `*_bridge_script`:

```rust
#[test]
fn test_workspace_folder_pick_bridge_script_calls_pick_workspace_folder() {
    let request = workspace_folder_pick_request("/tmp/project");
    let script = workspace_folder_pick_bridge_script(&request);

    assert!(script.contains("pickWorkspaceFolder"));
    assert!(script.contains(r#""kind":"file_open_panel""#));
    assert!(script.contains(r#""starting_directory":"/tmp/project""#));
}

#[test]
fn test_workspace_folder_pick_request_omits_starting_directory_when_root_blank() {
    let request = workspace_folder_pick_request("   ");
    assert!(!request.payload.as_object().unwrap().contains_key("starting_directory"));
    assert_eq!(request.payload["title"], "Select a project folder");
}

#[test]
fn test_parse_workspace_folder_pick_result_returns_path_on_success() {
    let value = serde_json::json!({
        "request_id": "r1",
        "kind": "file_open_panel",
        "handled": true,
        "payload": { "path": "/tmp/project", "cancelled": false }
    });
    assert_eq!(
        parse_workspace_folder_pick_result(Ok(value)),
        Ok(Some("/tmp/project".to_string()))
    );
}

#[test]
fn test_parse_workspace_folder_pick_result_returns_none_on_cancel() {
    let value = serde_json::json!({
        "request_id": "r1",
        "kind": "file_open_panel",
        "handled": true,
        "payload": { "path": null, "cancelled": true }
    });
    assert_eq!(parse_workspace_folder_pick_result(Ok(value)), Ok(None));
}

#[test]
fn test_parse_workspace_folder_pick_result_returns_none_when_bridge_unavailable() {
    assert_eq!(
        parse_workspace_folder_pick_result(Ok(serde_json::Value::Null)),
        Ok(None)
    );
}

#[test]
fn test_parse_workspace_folder_pick_result_propagates_eval_error() {
    let result = parse_workspace_folder_pick_result(Err("bridge rejected".to_string()));
    assert_eq!(result, Err("bridge rejected".to_string()));
}

#[test]
fn test_parse_workspace_folder_pick_result_rejects_malformed_payload() {
    let result = parse_workspace_folder_pick_result(Ok(serde_json::json!({ "unexpected": true })));
    assert!(result.is_err());
}
```

Run: `cd impulse-rs && cargo test -p impulse-desktop workspace_folder_pick 2>&1 | tail -30`
Expected: FAILS to compile at this point only if Step 7 hasn't landed yet in the same commit — since Steps 7 and 8 are applied together in this plan, run this after Step 7's code is in place and confirm all 6 tests PASS directly (the "red" step for this pair is implicit: these are pure-function tests against code that Step 7 just added, so the meaningful verification is the green run in the next step).

- [ ] **Step 9: Wire the "Browse…" button into the Workspace Launcher rsx**

In `impulse-rs/impulse-desktop/src/ui.rs`, replace the Folder path field (current lines 1028-1036):

```rust
label { class: "workspace-field wide",
    span { "Folder path" }
    input {
        r#type: "text",
        placeholder: "Absolute project folder",
        value: "{register_root}",
        oninput: move |evt| register_root.set(evt.value()),
    }
}
```

with:

```rust
label { class: "workspace-field wide",
    span { "Folder path" }
    div { class: "workspace-field-inline",
        input {
            r#type: "text",
            placeholder: "Absolute project folder",
            value: "{register_root}",
            oninput: move |evt| register_root.set(evt.value()),
        }
        button {
            class: "invoke-button secondary",
            "data-action": "browse-workspace-folder",
            "aria-label": "Browse for a folder",
            onclick: move |_| {
                let current_root = register_root();
                let request = workspace_folder_pick_request(&current_root);
                spawn(async move {
                    let script = workspace_folder_pick_bridge_script(&request);
                    let eval_result = document::eval(&script)
                        .await
                        .map_err(|error| error.to_string());
                    if let Ok(Some(path)) = parse_workspace_folder_pick_result(eval_result) {
                        register_root.set(path);
                    }
                });
            },
            "Browse…"
        }
    }
}
```

This matches every other onclick's shape in this file (`spawn(async move { let script = ...; document::eval(&script).await ... })`), the only difference being that this one's `Ok`/`Err` is consumed via `parse_workspace_folder_pick_result` instead of discarded with `let _ =`. No new `EventHandler` prop is added to `WorkspaceLaunchPanel`'s signature — the picker's answer is purely local to this component's own `register_root` signal.

Add the inline layout CSS to `impulse-rs/impulse-desktop/assets/impulse_crt.css`, inserted after `.workspace-field select { cursor: pointer; }` (current line 841), before `.workspace-primary` (current line 843):

```css
.workspace-field-inline {
  display: flex;
  gap: 6px;
  align-items: stretch;
}

.workspace-field-inline input {
  flex: 1;
  min-width: 0;
}

.workspace-field-inline .invoke-button {
  flex: 0 0 auto;
  white-space: nowrap;
}
```

(`.invoke-button.secondary` already exists at `assets/impulse_crt.css:448-451` and is reused as-is for the Browse button.)

- [ ] **Step 10: Run the full `impulse-desktop` package test suite**

Run: `cd impulse-rs && cargo test -p impulse-desktop 2>&1 | tail -40`
Expected: PASS — all tests from Steps 2, 6, and 8 pass, plus everything from Task 1.

Then run with the real feature to confirm it compiles with `rfd` active (do not run the `--ignored` manual dialog test in CI/headless — it opens a real modal):
Run: `cd impulse-rs && cargo build -p impulse-desktop --features desktop-app 2>&1 | tail -20`
Expected: builds successfully with zero errors.

Add the explicitly-non-executing manual smoke test to `impulse-rs/impulse-desktop/src/native.rs`, inside the `file_open_panel_tests` module (this documents the one path that genuinely cannot be automated):

```rust
#[test]
#[ignore = "opens a real native folder-picker dialog; run manually with a live display: `cargo test --features desktop-app -- --ignored`"]
#[cfg(feature = "desktop-app")]
fn manual_pick_folder_opens_native_dialog() {
    let result = RfdFolderPicker.pick_folder("Select a project folder", None);
    assert!(result.is_ok());
}
```

- [ ] **Step 11: Commit**

```bash
cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes
git add impulse-rs/impulse-desktop/Cargo.toml impulse-rs/impulse-desktop/src/native.rs \
        impulse-rs/impulse-desktop/src/host_commands.rs impulse-rs/impulse-desktop/src/ui.rs \
        impulse-rs/impulse-desktop/assets/impulse_crt.css \
        impulse-rs/impulse-desktop/tests/desktop_contract.rs impulse-rs/impulse-desktop/tests/host_surface.rs
git commit -m "feat(desktop): add native macOS folder picker to Workspace Launcher

Wires the previously-dead NativeIslandKind::FileOpenPanel through a
real rfd-backed dialog. The blocking dialog call is isolated via
tokio::task::spawn_blocking so it can't stall the single-consumer
host-invoke FIFO worker while the picker is open. A 'Browse...'
button next to Folder path populates the field on selection; Cancel
leaves any typed value untouched.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 3: Fix `.event-strip` and `.workspace-grid` CSS clipping, with visual-smoke coverage

**Files:**
- Modify: `impulse-rs/impulse-desktop/assets/impulse_crt.css` (two rule blocks)
- Modify: `impulse-rs/impulse-desktop/scripts/visual_smoke.mjs` (new viewport + new assertions)

**Interfaces:**
- Consumes: existing `.event-strip`/`.workspace-grid` markup in `src/ui.rs` (unchanged by this task — CSS-only fix), existing `assertLayout` function and `viewports` array in `scripts/visual_smoke.mjs`.
- Produces: no new Rust/JS-callable API — purely a stylesheet and test-script change. No other task depends on this one.

- [ ] **Step 1: Add the new viewport and new assertions to the visual-smoke script (will fail against current CSS)**

In `impulse-rs/impulse-desktop/scripts/visual_smoke.mjs`, replace the `viewports` array (current lines 13-16):

```diff
 const viewports = [
   { label: "desktop", width: 1440, height: 900 },
   { label: "compact", width: 1024, height: 768 },
+  { label: "narrow", width: 900, height: 700 },
 ];
```

(`900px` is guaranteed below the naive `232 + 420 + 320 = 972px` floor of `.workspace-grid`'s current columns — the real overflow threshold can only be equal to or higher than 972px depending on runtime content, so 900px reliably forces both the grid's scroll fallback and `.event-strip` wrapping regardless of what's rendered.)

Extend the `page.evaluate` callback inside `assertLayout` (current lines ~105-138) — add these fields to the returned object:

```diff
   const result = await page.evaluate((routeSlug) => {
     const rectOf = (selector) => {
       const element = document.querySelector(selector);
       if (!element) return null;
       const rect = element.getBoundingClientRect();
       return {
         x: rect.x,
         y: rect.y,
         width: rect.width,
         height: rect.height,
         top: rect.top,
         right: rect.right,
         bottom: rect.bottom,
         left: rect.left,
       };
     };
     const text = document.body.innerText || "";
+
+    const eventStripSpans = Array.from(document.querySelectorAll(".event-strip span")).map(
+      (span) => {
+        const spanRect = span.getBoundingClientRect();
+        return {
+          scrollWidth: span.scrollWidth,
+          clientWidth: span.clientWidth,
+          right: spanRect.right,
+        };
+      },
+    );
+
+    const gridEl = document.querySelector(".workspace-grid");
+    const gridScrollWidth = gridEl ? gridEl.scrollWidth : null;
+    const gridClientWidth = gridEl ? gridEl.clientWidth : null;
+    let inspectorReachable = null;
+    if (gridEl) {
+      const maxScrollLeft = gridEl.scrollWidth - gridEl.clientWidth;
+      gridEl.scrollLeft = maxScrollLeft;
+      const inspectorEl = document.querySelector(".right-inspector");
+      const shellEl = document.querySelector(".impulse-shell");
+      if (inspectorEl && shellEl) {
+        const inspectorRect = inspectorEl.getBoundingClientRect();
+        const shellRect = shellEl.getBoundingClientRect();
+        inspectorReachable =
+          inspectorRect.left >= shellRect.left - 1 && inspectorRect.right <= shellRect.right + 1;
+      }
+      gridEl.scrollLeft = 0;
+    }

     return {
       routeSlug,
       bodyTextLength: text.trim().length,
       shell: rectOf(".impulse-shell"),
       top: rectOf(".top-bar"),
       grid: rectOf(".workspace-grid"),
       left: rectOf(".left-rail"),
       stage: rectOf(".terminal-stage"),
       inspector: rectOf(".right-inspector"),
       footer: rectOf(".event-strip"),
       scrollWidth: document.documentElement.scrollWidth,
       scrollHeight: document.documentElement.scrollHeight,
       clientWidth: document.documentElement.clientWidth,
       clientHeight: document.documentElement.clientHeight,
       terminalVisible: rectOf(".view-terminal.active"),
+      eventStripSpans,
+      gridScrollWidth,
+      gridClientWidth,
+      inspectorReachable,
     };
   }, route.slug);
```

Add the new assertions inside `assertLayout`, immediately after the existing `if (route.slug !== "terminal") { ... }` block:

```js
  for (const span of result.eventStripSpans) {
    assert(
      span.scrollWidth <= span.clientWidth + 1,
      `${route.slug}: event-strip span text clipped (scrollWidth ${span.scrollWidth} > clientWidth ${span.clientWidth}) at ${viewport.width}x${viewport.height}`,
    );
    assert(
      span.right <= result.shell.right + 1,
      `${route.slug}: event-strip span extends past shell edge at ${viewport.width}x${viewport.height}`,
    );
  }

  if (result.gridScrollWidth !== null && result.gridClientWidth !== null) {
    assert(
      result.grid.width <= viewport.width + 1,
      `${route.slug}: workspace-grid outer box (${result.grid.width}) wider than viewport (${viewport.width}) - overflow is leaking to the shell instead of scrolling locally`,
    );
    if (result.gridScrollWidth > result.gridClientWidth + 1) {
      assert(
        result.inspectorReachable,
        `${route.slug}: workspace-grid overflows (${result.gridScrollWidth} > ${result.gridClientWidth}) but right-inspector is not reachable by scrolling`,
      );
    }
  }
```

These assertions are viewport-agnostic — at `desktop`/`compact` they're a no-op pass-through (nothing overflows with current fixture content), and at the new `narrow` (900px) viewport both engage. `cargo run --example render_visual_fixtures` needs **no changes** — it emits plain static HTML fixtures with a responsive `<meta viewport>` tag and no baked-in width; all viewport sizing happens in this script at `browser.newPage({ viewport })` time against the same fixture files.

- [ ] **Step 2: Run the visual-smoke suite to verify it fails (red) against the current CSS**

Run: `cd impulse-rs/impulse-desktop && npm run visual:fixtures && npm run visual:smoke 2>&1 | tail -60`
Expected: FAILS at the `narrow` (900×700) viewport — `.event-strip` span(s) report `scrollWidth > clientWidth` (clipped text) and/or `.workspace-grid`'s outer box exceeds the 900px viewport width. If `npm`/Playwright isn't installed in this environment, install per `impulse-desktop/package.json`'s existing dev dependencies before continuing — this is the load-bearing proof for this task.

- [ ] **Step 3: Fix `.event-strip` — wrap instead of silently clip**

In `impulse-rs/impulse-desktop/assets/impulse_crt.css`, replace the current rule (lines 913-925):

```diff
 .event-strip {
   display: flex;
+  flex-wrap: wrap;
-  gap: 22px;
+  gap: 6px 22px;
   padding: 6px 22px;
   border-top: 1px solid var(--c-line);
   color: var(--c-label);
   font-size: 11px;
 }
+
+.event-strip span {
+  min-width: 0;
+  max-width: 100%;
+  overflow-wrap: anywhere;
+}

 .event-strip span::before {
   content: "> ";
   color: var(--p-cyan);
 }
```

Wrapping (not per-span ellipsis) is the right call here because every item in this footer — including `shell-notice`'s arbitrary-length operator-facing `{intent}` message — is primary status information with no secondary place it's shown in full; truncating with `…` would hide information the footer exists to surface. This also matches existing precedent in the same file: `.governed-task-card h3` (lines 551-554) already overrides the ellipsis pattern for exactly this reason (primary content that must stay fully readable). The middle `.workspace-grid` row already has `min-height: 0` (line 168) inside `.impulse-shell`'s `grid-template-rows: auto 1fr auto`, so it can shrink to give the footer extra height when it wraps — no other layout change is required. `overflow-wrap: anywhere` on the spans is the load-bearing declaration: without it, a single very-long unbroken string (like a `shell-notice`) still forces its flex item wider than the row even with `flex-wrap: wrap` set, because a run of non-whitespace text can't break at word boundaries.

- [ ] **Step 4: Fix `.workspace-grid` — horizontal scroll fallback, unconditional (not a magic-number breakpoint)**

In `impulse-rs/impulse-desktop/assets/impulse_crt.css`, replace the current rule (lines 165-169):

```diff
 .workspace-grid {
   display: grid;
   grid-template-columns: 232px minmax(420px, 1fr) 320px;
   min-height: 0;
+  min-width: 0;
+  overflow-x: auto;
+  overflow-y: hidden;
 }
```

No `@media` breakpoint is used because the real overflow threshold is content-dependent (the middle `minmax(420px, 1fr)` track's actual minimum can exceed 420px depending on what renders inside it), so a static `@media (max-width: 972px)` guard would be both wrong and incomplete — it would leave a gap between 972px and the real, variable threshold where the bug still reproduces undetected. Applying `min-width: 0; overflow-x: auto;` unconditionally is strictly simpler and more correct: at any width where the columns fit, the browser renders no visible scrollbar and nothing changes; at any width where they don't, the browser's own (content-aware) overflow detection activates the scrollbar automatically. `overflow-y: hidden` is pinned explicitly because setting only `overflow-x` to a non-`visible` value forces the computed `overflow-y` to `auto` as a side effect per the CSS Overflow spec — pinning it keeps this fix scoped to the horizontal axis only; vertical overflow is already independently handled by `.left-rail`/`.right-inspector`'s own `overflow: auto` (lines 171-175, unchanged) and `.terminal-stage`'s `min-height: 0` flex chain. `.impulse-shell`'s global `overflow: hidden` (line 44) is untouched — the fix stays scoped entirely to `.workspace-grid`'s own box.

- [ ] **Step 5: Run the visual-smoke suite again to verify it passes (green)**

Run: `cd impulse-rs/impulse-desktop && npm run visual:smoke 2>&1 | tail -60`
Expected: PASS at all three viewports (`desktop`, `compact`, `narrow`). At `narrow`, the new assertions confirm: no `event-strip` span has clipped text, `.workspace-grid`'s outer box does not exceed the 900px viewport (overflow is contained, not leaking to the shell), and when the grid does overflow, `.right-inspector` is reachable by scrolling.

- [ ] **Step 6: Run the full Rust verification gate for the desktop package**

Run: `cd impulse-rs && cargo test -p impulse-desktop 2>&1 | tail -20`
Expected: PASS — this task is CSS/JS-only and touches no Rust code, so this confirms no accidental regression (there should be none).

- [ ] **Step 7: Commit**

```bash
cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes
git add impulse-rs/impulse-desktop/assets/impulse_crt.css impulse-rs/impulse-desktop/scripts/visual_smoke.mjs
git commit -m "fix(desktop): stop event-strip and workspace-grid from silently clipping

.event-strip had no flex-wrap/overflow handling, so its dynamic status
spans (including the arbitrary-length shell-notice intent message)
could be cut off with zero visual indication once combined content
exceeded window width. .workspace-grid's fixed 232/1fr/320 columns
had no width-based media query anywhere in the stylesheet and no
min-width:0, so content below the (content-dependent, often >972px)
overflow threshold was clipped rather than reachable.

Fixed event-strip to wrap (primary status info, not safe to truncate)
and workspace-grid to scroll horizontally when it doesn't fit -
applied unconditionally rather than behind a magic-number breakpoint,
since the real threshold varies with rendered content. Added a 900px
'narrow' viewport plus span-clipping and grid-reachability assertions
to the Playwright visual-smoke suite, which previously only tested
1024px and up - above the floor where either bug reproduces.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 4: Full verification gate and manual ground-truth smoke test

**Files:** none modified — this task only runs commands and records results in the lane work card.

**Interfaces:**
- Consumes: everything from Tasks 1-3.
- Produces: verified, working software; the lane's final status log entry.

- [ ] **Step 1: Run the complete verification gate**

Run:
```bash
cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes/impulse-rs
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all four commands exit 0. Record the exact `cargo test --workspace` pass/ignored/fail totals (per this repo's CLAUDE.md: don't rely on a stale checked-in aggregate — capture what this checkout actually reports) in the lane work card's status log.

- [ ] **Step 2: Manually run the packaged desktop binary and verify the full user flow end to end**

Run: `cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes/impulse-rs && cargo run -p impulse-desktop --features desktop-app --bin impulse-desktop`

In the running app, verify all of the following (this is the ground-truth check the automated tests can't fully replace — it's what the original bug report was about):
1. The app launches without the workspace list / agent pool getting permanently stuck at zero (Task 1's fix).
2. Clicking "Browse…" next to "Folder path" opens a real native macOS folder picker; selecting a folder populates the text field; Cancel leaves any typed value untouched (Task 2's fix).
3. Registering a real folder (via typed path or Browse) makes it appear in the workspace list.
4. Selecting a platform, typing a Task and Acceptance Criteria, and clicking "Launch" against that workspace opens a terminal pane that accepts input.
5. While the folder-picker dialog is open, other UI (typing in another field, terminal output) is not frozen — confirms the `spawn_blocking` isolation from Task 2 actually works, not just that it compiles.
6. Resize the window narrower than ~972px: the bottom status bar wraps to multiple lines instead of clipping text, and the 3-column shell either fits or scrolls horizontally instead of losing the right-hand inspector panel.

Record the outcome (pass/fail per item, with any deviations) in the lane work card's status log. If any item fails, do not mark the lane complete — return to the relevant task and fix before proceeding.

- [ ] **Step 3: Update the lane work card**

Edit `docs/plans/worktrees/2026-07-15-claude-desktop-ux-functional-fixes.md`: add a new entry at the top of the "Status log" section recording the verification-gate totals from Step 1 and the manual smoke-test results from Step 2, and update "Latest status" fields if the template calls for them.

- [ ] **Step 4: Commit the work-card update**

```bash
cd /Users/jamespustorino/code/IMPULSE-rs/.worktrees/desktop-ux-functional-fixes
git add docs/plans/worktrees/2026-07-15-claude-desktop-ux-functional-fixes.md
git commit -m "docs: record final verification results for desktop UX functional fixes lane

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

Lane implementation is complete once this task's Step 1 and Step 2 both pass. Merge/PR/cleanup decisions (per superpowers:finishing-a-development-branch) happen after this plan finishes executing, not as part of it.
