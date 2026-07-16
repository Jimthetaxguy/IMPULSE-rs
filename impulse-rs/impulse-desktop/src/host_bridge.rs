//! Live Dioxus host bridge.
//!
//! The manifest-only bootstrap ([`crate::desktop_host`]) installs
//! `window.__IMPULSE_DESKTOP_HOST` with `invoke`/`listen` stubs that always
//! reject, so the UI fails closed until a real transport lands. This module is
//! that real transport: it swaps the stubs for an `invoke`/`listen` pair backed
//! by the Dioxus `document::eval` channel and dispatches each call against a
//! [`DesktopShellState`] (the same command bodies the legacy Tauri host used).
//!
//! Wire protocol over the eval channel (all JSON):
//! - JS → Rust: `{ kind: "host_invoke", id, command, payload }`
//! - Rust → JS: `{ kind: "host_invoke_result", id, ok, result, error }`
//! - Rust → JS: `{ kind: "host_event", event, payload }` (drives `listen`)
//!
//! The dispatch table, request/response types, event marshalling, and the JS
//! transport string are all plain data and live here so they are unit- and
//! node-testable without spinning up a webview. Only [`use_live_host_bridge`]
//! touches the Dioxus runtime.

#![cfg(not(feature = "legacy-tauri-runtime"))]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{
    channel, error::TrySendError, unbounded_channel, Receiver, UnboundedReceiver, UnboundedSender,
};

use crate::desktop_shutdown::DesktopShutdownCoordinator;
use crate::host_commands::{
    self, DesktopShellState, AGENT_CLOSE_COMMAND, AGENT_FOCUS_COMMAND, AGENT_PLATFORMS_COMMAND,
    AGENT_RESIZE_COMMAND, AGENT_SNAPSHOT_COMMAND, AGENT_SPAWN_COMMAND, AGENT_WRITE_COMMAND,
    GOVERNED_TASK_MUTATE_COMMAND, LIST_WORKSPACES_COMMAND, MCP_DESCRIPTORS_COMMAND,
    MCP_INVOKE_COMMAND, NATIVE_ISLAND_REQUEST_COMMAND, REGISTER_WORKSPACE_COMMAND,
    REVIEW_DECISION_COMMAND, REVIEW_QUEUE_COMMAND, SUPERVISOR_LOCAL_ACTION_COMMAND,
    TERMINAL_CLOSE_COMMAND, TERMINAL_FOCUS_COMMAND, TERMINAL_OPEN_COMMAND, TERMINAL_RESIZE_COMMAND,
    TERMINAL_WRITE_COMMAND,
};
use crate::runtime::{DesktopEvent, DesktopEventSink};

/// Status the live bridge publishes once it has replaced the manifest-only
/// stubs. Anything other than [`crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS`]
/// reads as "ready" to the host-adapter resolver in `ui.rs`.
pub const LIVE_HOST_BRIDGE_STATUS: &str = "dioxus-eval-bridge-ready";
pub const LIVE_HOST_READINESS_COMMAND: &str = "impulse_host_ready";
pub const PACKAGE_SMOKE_COMPLETE_COMMAND: &str = "impulse_package_smoke_complete";
pub const PACKAGE_SMOKE_ENV: &str = "IMPULSE_DESKTOP_SMOKE";
pub const PACKAGE_SMOKE_RECEIPT_PREFIX: &str = "IMPULSE_DESKTOP_SMOKE_RECEIPT ";
pub const HOST_INVOKE_RESULT_KIND: &str = "host_invoke_result";
const HOST_INVOKE_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct HostReadinessProbe {
    status: String,
    assets_ready: bool,
    terminal_constructor_ready: bool,
    fit_addon_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct HostReadinessAck {
    status: String,
    smoke_mode: bool,
    smoke_cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PackageSmokeReceipt {
    status: String,
    session_id: String,
    bridge_ready: bool,
    assets_ready: bool,
    terminal_opened: bool,
    terminal_output_seen: bool,
    terminal_resized: bool,
    terminal_focused: bool,
    terminal_closed: bool,
    terminal_exit_seen: bool,
    ops_update_seen: bool,
}

/// A single `invoke()` call crossing from JS into Rust.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostInvokeRequest {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub payload: Value,
}

/// The reply to a [`HostInvokeRequest`]. `ok` mirrors the underlying
/// `Result`; `result` carries the serialized success value and `error` the
/// failure message (never both).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostInvokeResponse {
    pub kind: String,
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub close_desktop: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl HostInvokeResponse {
    fn ok(id: String, result: Value) -> Self {
        Self {
            kind: HOST_INVOKE_RESULT_KIND.to_string(),
            id,
            ok: true,
            result,
            error: None,
            close_desktop: false,
        }
    }

    fn err(id: String, error: String) -> Self {
        Self {
            kind: HOST_INVOKE_RESULT_KIND.to_string(),
            id,
            ok: false,
            result: Value::Null,
            error: Some(error),
            close_desktop: false,
        }
    }
}

/// Extract a typed command body from the `invoke` payload. The UI sends
/// bodies as `{ request: <T> }` (matching the Tauri `State`/`request`
/// convention); a bare object is also accepted so callers may pass the body
/// directly.
fn body<T: DeserializeOwned>(payload: Value) -> Result<T, String> {
    let inner = match payload {
        Value::Object(mut map) => map.remove("request").unwrap_or(Value::Object(map)),
        other => other,
    };
    serde_json::from_value(inner).map_err(|error| format!("invalid request payload: {error}"))
}

fn json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("failed to serialize result: {error}"))
}

fn package_smoke_enabled() -> bool {
    std::env::var(PACKAGE_SMOKE_ENV).as_deref() == Ok("1")
}

fn package_smoke_trace(message: impl AsRef<str>) {
    if package_smoke_enabled() {
        eprintln!("desktop package smoke: {}", message.as_ref());
    }
}

fn acknowledge_live_host(
    state: &DesktopShellState,
    probe: HostReadinessProbe,
) -> Result<HostReadinessAck, String> {
    if probe.status != LIVE_HOST_BRIDGE_STATUS {
        return Err(format!(
            "live host readiness status must be `{LIVE_HOST_BRIDGE_STATUS}`, got `{}`",
            probe.status
        ));
    }
    if !probe.assets_ready {
        return Err("packaged xterm assets are not readable from the webview".to_string());
    }
    if !probe.terminal_constructor_ready {
        return Err("xterm Terminal constructor is unavailable in the webview".to_string());
    }
    if !probe.fit_addon_ready {
        return Err("xterm FitAddon constructor is unavailable in the webview".to_string());
    }
    let smoke_cwd = state
        .memory_root()
        .ok()
        .and_then(|memory_root| memory_root.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .to_string_lossy()
        .into_owned();
    Ok(HostReadinessAck {
        status: LIVE_HOST_BRIDGE_STATUS.to_string(),
        smoke_mode: package_smoke_enabled(),
        smoke_cwd,
    })
}

fn emit_package_smoke_receipt(receipt: PackageSmokeReceipt) -> Result<Value, String> {
    if !package_smoke_enabled() {
        return Err("package smoke completion is unavailable outside diagnostic mode".to_string());
    }
    validate_package_smoke_receipt(&receipt)?;

    let encoded = serde_json::to_string(&receipt)
        .map_err(|error| format!("serialize package smoke receipt: {error}"))?;
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{PACKAGE_SMOKE_RECEIPT_PREFIX}{encoded}")
        .and_then(|_| stdout.flush())
        .map_err(|error| format!("write package smoke receipt: {error}"))?;
    json(receipt)
}

fn validate_package_smoke_receipt(receipt: &PackageSmokeReceipt) -> Result<(), String> {
    if receipt.status != LIVE_HOST_BRIDGE_STATUS {
        return Err(format!(
            "package smoke status must be `{LIVE_HOST_BRIDGE_STATUS}`, got `{}`",
            receipt.status
        ));
    }
    if receipt.session_id.trim().is_empty() {
        return Err("package smoke session id must not be blank".to_string());
    }
    let checks = [
        ("bridge_ready", receipt.bridge_ready),
        ("assets_ready", receipt.assets_ready),
        ("terminal_opened", receipt.terminal_opened),
        ("terminal_output_seen", receipt.terminal_output_seen),
        ("terminal_resized", receipt.terminal_resized),
        ("terminal_focused", receipt.terminal_focused),
        ("terminal_closed", receipt.terminal_closed),
        ("terminal_exit_seen", receipt.terminal_exit_seen),
        ("ops_update_seen", receipt.ops_update_seen),
    ];
    if let Some((name, _)) = checks.into_iter().find(|(_, passed)| !passed) {
        return Err(format!("package smoke check `{name}` did not pass"));
    }
    Ok(())
}

/// Route one command against the shell state, returning the serialized success
/// value or an error message. Mirrors the non-legacy `host_commands` surface.
async fn dispatch_command(
    state: &DesktopShellState,
    command: &str,
    payload: Value,
) -> Result<Value, String> {
    let runtime = state.runtime.as_ref();
    match command {
        LIVE_HOST_READINESS_COMMAND => json(acknowledge_live_host(
            state,
            body::<HostReadinessProbe>(payload)?,
        )?),
        PACKAGE_SMOKE_COMPLETE_COMMAND => {
            emit_package_smoke_receipt(body::<PackageSmokeReceipt>(payload)?)
        }
        AGENT_SNAPSHOT_COMMAND => json(host_commands::agent_snapshot(runtime).await?),
        AGENT_PLATFORMS_COMMAND => json(host_commands::agent_platforms().await?),
        AGENT_SPAWN_COMMAND => {
            json(host_commands::agent_spawn_with_state(state, body(payload)?).await?)
        }
        AGENT_WRITE_COMMAND => json(host_commands::agent_write(runtime, body(payload)?).await?),
        GOVERNED_TASK_MUTATE_COMMAND => {
            json(host_commands::governed_task_mutate(state, body(payload)?).await?)
        }
        AGENT_RESIZE_COMMAND => json(host_commands::agent_resize(runtime, body(payload)?).await?),
        AGENT_FOCUS_COMMAND => json(host_commands::agent_focus(runtime, body(payload)?).await?),
        AGENT_CLOSE_COMMAND => json(host_commands::agent_close(runtime, body(payload)?).await?),
        SUPERVISOR_LOCAL_ACTION_COMMAND => {
            json(host_commands::supervisor_local_action(runtime, body(payload)?).await?)
        }
        TERMINAL_OPEN_COMMAND => json(host_commands::terminal_open(runtime, body(payload)?).await?),
        TERMINAL_WRITE_COMMAND => {
            json(host_commands::terminal_write(runtime, body(payload)?).await?)
        }
        TERMINAL_RESIZE_COMMAND => {
            json(host_commands::terminal_resize(runtime, body(payload)?).await?)
        }
        TERMINAL_FOCUS_COMMAND => {
            json(host_commands::terminal_focus(runtime, body(payload)?).await?)
        }
        TERMINAL_CLOSE_COMMAND => {
            json(host_commands::terminal_close(runtime, body(payload)?).await?)
        }
        NATIVE_ISLAND_REQUEST_COMMAND => {
            json(host_commands::native_island_request(body(payload)?).await?)
        }
        LIST_WORKSPACES_COMMAND => json(host_commands::list_workspaces(state).await?),
        REGISTER_WORKSPACE_COMMAND => {
            json(host_commands::register_workspace(state, body(payload)?).await?)
        }
        MCP_DESCRIPTORS_COMMAND => json(host_commands::mcp_descriptors(state).await?),
        MCP_INVOKE_COMMAND => json(host_commands::mcp_invoke(state, body(payload)?).await?),
        REVIEW_QUEUE_COMMAND => json(host_commands::review_queue(state).await?),
        REVIEW_DECISION_COMMAND => {
            json(host_commands::review_decision(state, body(payload)?).await?)
        }
        other => Err(format!("unknown host command `{other}`")),
    }
}

/// Dispatch a single `invoke` request, always producing a response addressed to
/// the same `id` (success or error). Never panics.
pub async fn dispatch_host_invoke(
    state: &DesktopShellState,
    request: HostInvokeRequest,
) -> HostInvokeResponse {
    let id = request.id;
    let command = request.command.clone();
    let mut response = match dispatch_command(state, &request.command, request.payload).await {
        Ok(result) => HostInvokeResponse::ok(id, result),
        Err(error) => {
            eprintln!("Impulse live host command `{command}` failed: {error}");
            HostInvokeResponse::err(id, error)
        }
    };
    if response.ok && command == PACKAGE_SMOKE_COMPLETE_COMMAND {
        response.close_desktop = true;
    }
    response
}

async fn dispatch_host_invokes_fifo(
    state: DesktopShellState,
    mut requests: Receiver<HostInvokeRequest>,
    responses: UnboundedSender<HostInvokeResponse>,
) {
    while let Some(request) = requests.recv().await {
        package_smoke_trace(format!("dispatching host invoke `{}`", request.command));
        let response = dispatch_host_invoke(&state, request).await;
        package_smoke_trace(format!("host invoke `{}` completed", response.id));
        if responses.send(response).is_err() {
            package_smoke_trace("host invoke response channel closed");
            break;
        }
    }
}

/// Build the `host_event` envelope delivered to JS `listen` handlers. The
/// payload is the event's `data` content so handlers receive the same shape the
/// legacy Tauri host produced (e.g. `{ agent_id, data }` for terminal output).
pub fn host_event_envelope(event: &DesktopEvent) -> Value {
    let payload = serde_json::to_value(event)
        .ok()
        .and_then(|mut value| value.get_mut("data").map(Value::take))
        .unwrap_or(Value::Null);
    serde_json::json!({
        "kind": "host_event",
        "event": event.name(),
        "payload": payload,
    })
}

/// A [`DesktopEventSink`] that forwards every runtime event onto an unbounded
/// channel. The live bridge loop drains the receiver and pushes each event to
/// the webview.
pub struct ChannelEventSink {
    sender: UnboundedSender<DesktopEvent>,
}

impl DesktopEventSink for ChannelEventSink {
    fn emit(&self, event: DesktopEvent) {
        // A closed receiver only means the webview is gone; dropping the event
        // is the correct, non-fatal behavior.
        let _ = self.sender.send(event);
    }
}

/// Create a [`ChannelEventSink`] paired with its receiver. Wire the sink into
/// [`crate::runtime::DesktopRuntimeBuilder::with_event_sink`] and hand the
/// receiver to [`LiveHostContext`].
pub fn channel_event_sink() -> (Arc<ChannelEventSink>, UnboundedReceiver<DesktopEvent>) {
    let (sender, receiver) = unbounded_channel();
    (Arc::new(ChannelEventSink { sender }), receiver)
}

/// Everything the live bridge loop needs: the command state plus the event
/// receiver (taken exactly once when the loop starts).
#[derive(Clone)]
pub struct LiveHostContext {
    state: DesktopShellState,
    event_rx: Arc<Mutex<Option<UnboundedReceiver<DesktopEvent>>>>,
    shutdown_coordinator: Option<DesktopShutdownCoordinator>,
}

impl LiveHostContext {
    pub fn new(state: DesktopShellState, event_rx: UnboundedReceiver<DesktopEvent>) -> Self {
        Self {
            state,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
            shutdown_coordinator: None,
        }
    }

    /// Share the ordered desktop lifecycle boundary with the native host
    /// bridge. Programmatic close must drain workers and telemetry before it
    /// stops an owned companion and asks Dioxus to close its final window.
    pub fn with_shutdown_coordinator(
        mut self,
        shutdown_coordinator: DesktopShutdownCoordinator,
    ) -> Self {
        self.shutdown_coordinator = Some(shutdown_coordinator);
        self
    }
}

static LIVE_HOST_CONTEXT: OnceLock<LiveHostContext> = OnceLock::new();

/// Install the process-wide live-host context. The launcher calls this once,
/// before `dioxus::LaunchBuilder::desktop().launch(...)`, so the in-webview
/// bridge hook can find the runtime state and event stream. Returns `false` if
/// a context was already installed.
pub fn install_live_host_context(context: LiveHostContext) -> bool {
    LIVE_HOST_CONTEXT.set(context).is_ok()
}

fn live_host_context() -> Option<LiveHostContext> {
    LIVE_HOST_CONTEXT.get().cloned()
}

/// The JS transport. It replaces the manifest-only stubs with a real
/// `invoke`/`listen` pair, then loops on `dioxus.recv()` routing invoke results
/// to pending promises and host events to registered listeners.
pub const LIVE_HOST_BRIDGE_SCRIPT: &str = concat!(
    r#"
(async () => {
  const pendingInvokes = new Map();
  const listeners = new Map();
  let nextId = 0;
  let invokeTail = Promise.resolve();
  const host = {
    invoke(command, payload) {
      const scheduled = invokeTail.then(() => new Promise((resolve, reject) => {
          const id = `host-${++nextId}`;
          pendingInvokes.set(id, { resolve, reject });
          try {
            dioxus.send({ kind: "host_invoke", id, command, payload: payload ?? null });
          } catch (error) {
            pendingInvokes.delete(id);
            reject(error);
          }
        })
      );
      invokeTail = scheduled.then(
        () => undefined,
        () => undefined,
      );
      return scheduled;
    },
    listen(event, handler) {
      let set = listeners.get(event);
      if (!set) { set = new Set(); listeners.set(event, set); }
      set.add(handler);
      return Promise.resolve(() => { set.delete(handler); });
    },
    hostKind: "dioxus",
    status: ""#,
    "dioxus-eval-bridge-ready",
    r#"",
  };
  window.__IMPULSE_DESKTOP_HOST = host;
  document.documentElement?.setAttribute("data-impulse-host-kind", "dioxus");
  document.documentElement?.setAttribute(
    "data-impulse-host-status",
    ""#,
    "dioxus-eval-bridge-ready",
    r#""
  );
  window.dispatchEvent?.(new CustomEvent("impulse-host-ready", {
    detail: { status: host.status },
  }));

  const requiredAssets = [
    "assets/vendor/xterm/xterm.css",
    "assets/vendor/xterm/xterm.js",
    "assets/vendor/xterm/addon-fit.js",
  ];
  const withTimeout = (promise, label, timeoutMs = 12000) =>
    Promise.race([
      promise,
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs)
      ),
    ]);
  const terminalAssetState = () => {
    const terminalReady =
      typeof (window.Terminal || window.XTerm?.Terminal) === "function";
    const fitReady =
      typeof (window.FitAddon?.FitAddon || window.FitAddon) === "function";
    const stylesheetReady = Array.from(document.styleSheets ?? []).some((sheet) =>
      String(sheet?.href ?? "").endsWith(requiredAssets[0])
    );
    return {
      terminalReady,
      fitReady,
      stylesheetReady,
      assetsReady: terminalReady && fitReady && stylesheetReady,
    };
  };
  const waitForTerminalAssets = async () => {
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline) {
      const state = terminalAssetState();
      if (state.assetsReady) {
        return state;
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    return terminalAssetState();
  };
  const runPackageSmoke = async (readiness) => {
    const expectedOutput = "IMPULSE_PTY_SMOKE_OK";
    let outputText = "";
    let resolveOutput;
    let resolveOps;
    let resolveExit;
    let resolveShutdownProbe;
    const outputSeen = new Promise((resolve) => { resolveOutput = resolve; });
    const opsSeen = new Promise((resolve) => { resolveOps = resolve; });
    const exitSeen = new Promise((resolve) => { resolveExit = resolve; });
    const shutdownProbeReady = new Promise((resolve) => {
      resolveShutdownProbe = resolve;
    });
    let smokeSessionId = "impulse-package-smoke";
    let outputUnlisten;
    let opsUnlisten;
    let exitUnlisten;
    let terminalOpened = false;
    let terminalClosed = false;
    let shutdownProbeOutput = "";
    let shutdownProbeOpened = false;
    let shutdownProbeSessionId = "impulse-shutdown-probe";
    let receiptAcknowledged = false;

    try {
      outputUnlisten = await host.listen("terminal_output", (event) => {
        const payload = event?.payload ?? {};
        const bytes = Array.isArray(payload.data) ? payload.data : [];
        const decoded = new TextDecoder().decode(new Uint8Array(bytes));
        if (payload.agent_id === smokeSessionId) {
          outputText += decoded;
          if (outputText.includes(expectedOutput)) { resolveOutput(true); }
        }
        if (payload.agent_id === shutdownProbeSessionId) {
          shutdownProbeOutput += decoded;
          if (shutdownProbeOutput.includes("IMPULSE_SHUTDOWN_PROBE_READY")) {
            resolveShutdownProbe(true);
          }
        }
      });
      opsUnlisten = await host.listen("ops_update", (event) => {
        const envelope = event?.payload ?? {};
        const snapshot = envelope.payload ?? envelope;
        const agents = Array.isArray(snapshot?.agents) ? snapshot.agents : [];
        if (agents.some((agent) =>
          agent.id === smokeSessionId || agent.session_id === smokeSessionId
        )) {
          resolveOps(true);
        }
      });
      exitUnlisten = await host.listen("terminal_exit", (event) => {
        const payload = event?.payload ?? {};
        if (payload.agent_id === smokeSessionId) { resolveExit(true); }
      });

      const opened = await host.invoke("terminal_open", {
        request: {
          session_id: smokeSessionId,
          command: "/bin/sh",
          args: [],
          cwd: readiness.smoke_cwd,
          env: {},
          workspace: null,
          mcp_tools: [],
          rows: 24,
          cols: 80,
        },
      });
      smokeSessionId = opened.session_id;
      terminalOpened = !!opened.session_id;
      await host.invoke("terminal_resize", {
        request: { session_id: smokeSessionId, rows: 30, cols: 100 },
      });
      await host.invoke("terminal_focus", {
        request: { session_id: smokeSessionId },
      });
      await host.invoke("terminal_write", {
        request: {
          session_id: smokeSessionId,
          data: Array.from(
            new TextEncoder().encode("printf 'IMPULSE_PTY_SMOKE_OK\\n'\\n")
          ),
        },
      });
      await withTimeout(outputSeen, "terminal output");
      await withTimeout(opsSeen, "daemon ops update");
      await host.invoke("terminal_close", {
        request: { session_id: smokeSessionId },
      });
      terminalClosed = true;
      await withTimeout(exitSeen, "terminal exit");

      // Leave one exact, long-running worker alive when the native desktop
      // closes. The package verifier reads its PID file and proves the ordered
      // shutdown coordinator reaps it; the earlier smoke worker still proves
      // the explicit terminal-close path independently.
      const shutdownProbe = await host.invoke("terminal_open", {
        request: {
          session_id: shutdownProbeSessionId,
          command: "/bin/sh",
          args: [
            "-lc",
            "printf '%s' $$ > .impulse/desktop-shutdown-worker.pid; printf 'IMPULSE_SHUTDOWN_PROBE_READY\\n'; exec sleep 300",
          ],
          cwd: readiness.smoke_cwd,
          env: {},
          workspace: null,
          mcp_tools: [],
          rows: 24,
          cols: 80,
        },
      });
      shutdownProbeSessionId = shutdownProbe.session_id;
      shutdownProbeOpened = shutdownProbe.alive === true;
      if (!shutdownProbeOpened) {
        throw new Error("shutdown probe worker did not remain alive");
      }
      await withTimeout(shutdownProbeReady, "shutdown probe readiness");

      const receipt = await host.invoke("impulse_package_smoke_complete", {
        request: {
          status: host.status,
          session_id: smokeSessionId,
          bridge_ready: true,
          assets_ready: true,
          terminal_opened: opened.alive === true,
          terminal_output_seen: true,
          terminal_resized: true,
          terminal_focused: true,
          terminal_closed: true,
          terminal_exit_seen: true,
          ops_update_seen: true,
        },
      });
      receiptAcknowledged = true;
      return receipt;
    } finally {
      if (terminalOpened && !terminalClosed) {
        try {
          await host.invoke("terminal_close", {
            request: { session_id: smokeSessionId },
          });
        } catch (_) {}
      }
      if (shutdownProbeOpened && !receiptAcknowledged) {
        try {
          await host.invoke("terminal_close", {
            request: { session_id: shutdownProbeSessionId },
          });
        } catch (_) {}
      }
      for (const unlisten of [outputUnlisten, opsUnlisten, exitUnlisten]) {
        if (typeof unlisten !== "function") { continue; }
        try { await unlisten(); } catch (_) {}
      }
      // The Rust host owns final close and runs the ordered shutdown
      // coordinator before asking Tao to destroy the window.
    }
  };
  const startReadinessProbe = async () => {
    // Dioxus serves packaged assets through a custom WKWebView scheme. Fetch
    // can report those responses as opaque/non-ok even after WebKit has loaded
    // them successfully, so readiness must be based on observable effects.
    const assets = await waitForTerminalAssets();
    const readiness = await host.invoke("impulse_host_ready", {
      request: {
        status: host.status,
        assets_ready: assets.assetsReady,
        terminal_constructor_ready: assets.terminalReady,
        fit_addon_ready: assets.fitReady,
      },
    });
    if (readiness.smoke_mode) {
      await runPackageSmoke(readiness);
    }
  };
  void startReadinessProbe().catch((error) => {
    console.error("Impulse live host readiness failed", error);
  });

  for (;;) {
    let message;
    try {
      message = await dioxus.recv();
    } catch (error) {
      break;
    }
    if (!message) { continue; }
    if (message.kind === "host_invoke_result") {
      const entry = pendingInvokes.get(message.id);
      if (entry) {
        pendingInvokes.delete(message.id);
        if (message.ok) { entry.resolve(message.result); }
        else { entry.reject(new Error(message.error || "host invoke failed")); }
      }
    } else if (message.kind === "host_event") {
      const set = listeners.get(message.event);
      if (set) {
        for (const handler of set) {
          try { handler({ payload: message.payload }); } catch (_) {}
        }
      }
    }
  }
})();
"#
);

/// Return the live host bridge transport script.
pub fn live_host_bridge_script() -> &'static str {
    LIVE_HOST_BRIDGE_SCRIPT
}

/// Drive the bridge: install the JS transport, then concurrently service
/// `invoke` requests (dispatched against `state`) and forward runtime events to
/// the webview. A bounded FIFO worker preserves wire order while returning
/// results through a correlation channel, so daemon retries or PTY setup never
/// stop the loop from draining terminal/ops events. Runs until either source
/// channel closes. This is the only function that touches the Dioxus runtime;
/// everything it calls is plain data.
pub fn use_live_host_bridge() {
    #[cfg(feature = "desktop-app")]
    let desktop_context = dioxus_desktop::window();
    use_future(move || {
        #[cfg(feature = "desktop-app")]
        let desktop_context = desktop_context.clone();
        async move {
            package_smoke_trace("live host bridge future started");
            let Some(context) = live_host_context() else {
                package_smoke_trace("live host context is unavailable");
                return;
            };
            let Some(mut event_rx) = context
                .event_rx
                .lock()
                .ok()
                .and_then(|mut guard| guard.take())
            else {
                // Another mount already claimed the receiver; nothing to do.
                package_smoke_trace("live host event receiver was already claimed");
                return;
            };
            let state = context.state.clone();
            let shutdown_coordinator = context.shutdown_coordinator.clone();
            package_smoke_trace("creating Dioxus eval transport");
            let mut eval = document::eval(LIVE_HOST_BRIDGE_SCRIPT);
            package_smoke_trace("Dioxus eval transport created");
            let (invoke_result_tx, mut invoke_result_rx) =
                unbounded_channel::<HostInvokeResponse>();
            let (invoke_tx, invoke_rx) = channel::<HostInvokeRequest>(HOST_INVOKE_QUEUE_CAPACITY);
            let invoke_state = state.clone();
            let invoke_worker = spawn(dispatch_host_invokes_fifo(
                invoke_state,
                invoke_rx,
                invoke_result_tx,
            ));

            loop {
                tokio::select! {
                    event = event_rx.recv() => {
                        match event {
                            Some(event) => {
                                let _ = eval.send(host_event_envelope(&event));
                            }
                            None => break,
                        }
                    }
                    response = invoke_result_rx.recv() => {
                        if let Some(response) = response {
                            package_smoke_trace(format!("sending host invoke response `{}`", response.id));
                            let close_desktop = response.close_desktop;
                            if let Ok(value) = serde_json::to_value(&response) {
                                if let Err(error) = eval.send(value) {
                                    package_smoke_trace(format!("Dioxus eval response send failed: {error}"));
                                }
                            }
                            if close_desktop {
                                package_smoke_trace("requesting graceful native desktop close");
                                if let Some(coordinator) = shutdown_coordinator.as_ref() {
                                    match coordinator.shutdown() {
                                        Some(report) => {
                                            let daemon_mode = report
                                                .daemon_sidecar
                                                .as_ref()
                                                .map(|outcome| outcome.mode);
                                            package_smoke_trace(format!(
                                                "desktop shutdown completed: agents_seen={} agents_closed={} agents_already_exited={} runtime_errors={} daemon={:?} daemon_ops={:?} daemon_sidecar={:?}",
                                                report.runtime.agents_seen,
                                                report.runtime.agents_closed,
                                                report.runtime.agents_already_exited,
                                                report.runtime.errors.len(),
                                                daemon_mode,
                                                report.daemon_ops,
                                                report.daemon_sidecar,
                                            ));
                                        }
                                        None => package_smoke_trace(
                                            "desktop shutdown was already completed",
                                        ),
                                    }
                                }
                                #[cfg(feature = "desktop-app")]
                                desktop_context.close();
                            }
                        }
                    }
                    request = eval.recv::<HostInvokeRequest>() => {
                        match request {
                            Ok(request) => {
                                package_smoke_trace(format!("received host invoke `{}`", request.command));
                                let rejected = match invoke_tx.try_send(request) {
                                    Ok(()) => None,
                                    Err(TrySendError::Full(request)) => Some(HostInvokeResponse::err(
                                        request.id,
                                        "host invoke queue is full".to_string(),
                                    )),
                                    Err(TrySendError::Closed(request)) => Some(HostInvokeResponse::err(
                                        request.id,
                                        "host invoke worker is unavailable".to_string(),
                                    )),
                                };
                                if let Some(response) = rejected {
                                    if let Ok(value) = serde_json::to_value(&response) {
                                        let _ = eval.send(value);
                                    }
                                }
                            }
                            Err(error) => {
                                package_smoke_trace(format!("Dioxus eval receive failed: {error}"));
                                break;
                            }
                        }
                    }
                }
            }
            invoke_worker.cancel();
        }
    });
}

/// Root component for the live Dioxus desktop app: mounts the host bridge (so
/// `window.__IMPULSE_DESKTOP_HOST` becomes a real transport) and renders the
/// shell. The launcher installs the [`LiveHostContext`] via
/// [`install_live_host_context`] before launching this component.
#[component]
pub fn LiveDesktopApp() -> Element {
    package_smoke_trace("LiveDesktopApp rendered");
    use_live_host_bridge();
    rsx! {
        crate::ui::DesktopShell {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        AgentPlatformId, DesktopRuntime, GovernedTaskGateway, LocalSupervisorAction,
    };
    use crate::workspace::WorkspaceRegistry;
    use crate::McpToolRegistry;
    use serde_json::json;

    fn test_state() -> DesktopShellState {
        let (sink, _rx) = channel_event_sink();
        let runtime = Arc::new(DesktopRuntime::builder().with_event_sink(sink).build());
        let workspaces = Arc::new(WorkspaceRegistry::empty());
        let mcp = Arc::new(McpToolRegistry::with_builtins());
        let memory_root = std::env::temp_dir().join(format!("impulse-host-bridge-{}", uuid_seed()));
        DesktopShellState::new(runtime, workspaces, mcp, memory_root)
    }

    struct AcknowledgingGovernedGateway {
        task: impulse_ops::governed_task::GovernedTaskRun,
        mutations: Mutex<Vec<impulse_ops::governed_task::GovernedTaskMutationRequest>>,
    }

    impl GovernedTaskGateway for AcknowledgingGovernedGateway {
        fn register(
            &self,
            _registration: impulse_ops::governed_task::GovernedTaskRegistration,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            Err("registration is not used by this host dispatch test".to_string())
        }

        fn mutate(
            &self,
            request: impulse_ops::governed_task::GovernedTaskMutationRequest,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            self.mutations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.clone());
            let mut updated = self.task.clone();
            updated.revision = request.expected_revision + 1;
            Ok(updated)
        }

        fn mutate_current(
            &self,
            _project_id: &str,
            _task_id: &impulse_ops::governed_task::GovernedTaskId,
            _request_id: impulse_ops::governed_task::GovernedRequestId,
            _mutation: impulse_ops::governed_task::GovernedTaskMutation,
        ) -> Result<impulse_ops::governed_task::GovernedTaskRun, String> {
            Err("current mutation is not used by this host dispatch test".to_string())
        }
    }

    fn test_state_with_governed_gateway(
        gateway: Arc<dyn GovernedTaskGateway>,
    ) -> DesktopShellState {
        let (sink, _rx) = channel_event_sink();
        let runtime = Arc::new(
            DesktopRuntime::builder()
                .with_event_sink(sink)
                .with_governed_task_gateway(gateway)
                .build(),
        );
        DesktopShellState::new(
            runtime,
            Arc::new(WorkspaceRegistry::empty()),
            Arc::new(McpToolRegistry::with_builtins()),
            std::env::temp_dir().join(format!("impulse-host-bridge-{}", uuid_seed())),
        )
    }

    // Deterministic-ish unique suffix without pulling Uuid into the test (the
    // crate already depends on it, but this keeps the helper self-contained).
    fn uuid_seed() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[test]
    fn host_invoke_request_round_trips() {
        let original = HostInvokeRequest {
            id: "host-1".to_string(),
            command: "agent_snapshot".to_string(),
            payload: json!({ "request": { "session_id": "codex" } }),
        };
        let text = serde_json::to_string(&original).unwrap();
        let back: HostInvokeRequest = serde_json::from_str(&text).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn host_invoke_request_ignores_envelope_kind_field() {
        // JS sends `{ kind: "host_invoke", id, command, payload }`; the extra
        // `kind` must not break deserialization.
        let value = json!({
            "kind": "host_invoke",
            "id": "host-2",
            "command": "list_workspaces",
            "payload": null
        });
        let request: HostInvokeRequest = serde_json::from_value(value).unwrap();
        assert_eq!(request.command, "list_workspaces");
    }

    fn passing_package_smoke_receipt() -> PackageSmokeReceipt {
        PackageSmokeReceipt {
            status: LIVE_HOST_BRIDGE_STATUS.to_string(),
            session_id: "package-smoke-session".to_string(),
            bridge_ready: true,
            assets_ready: true,
            terminal_opened: true,
            terminal_output_seen: true,
            terminal_resized: true,
            terminal_focused: true,
            terminal_closed: true,
            terminal_exit_seen: true,
            ops_update_seen: true,
        }
    }

    #[test]
    fn live_host_readiness_requires_assets_and_terminal_constructors() {
        let state = test_state();
        let ready = HostReadinessProbe {
            status: LIVE_HOST_BRIDGE_STATUS.to_string(),
            assets_ready: true,
            terminal_constructor_ready: true,
            fit_addon_ready: true,
        };
        let ack = acknowledge_live_host(&state, ready.clone()).expect("ready host acknowledged");
        assert_eq!(ack.status, LIVE_HOST_BRIDGE_STATUS);
        assert!(!ack.smoke_cwd.trim().is_empty());

        let mut missing_asset = ready.clone();
        missing_asset.assets_ready = false;
        assert!(acknowledge_live_host(&state, missing_asset)
            .unwrap_err()
            .contains("xterm assets"));

        let mut missing_terminal = ready.clone();
        missing_terminal.terminal_constructor_ready = false;
        assert!(acknowledge_live_host(&state, missing_terminal)
            .unwrap_err()
            .contains("Terminal constructor"));

        let mut stale_status = ready;
        stale_status.status = "manifest-only".to_string();
        assert!(acknowledge_live_host(&state, stale_status)
            .unwrap_err()
            .contains(LIVE_HOST_BRIDGE_STATUS));
    }

    #[test]
    fn package_smoke_receipt_requires_every_live_boundary() {
        let passing = passing_package_smoke_receipt();
        validate_package_smoke_receipt(&passing).expect("complete receipt");

        let mut missing_output = passing.clone();
        missing_output.terminal_output_seen = false;
        assert!(validate_package_smoke_receipt(&missing_output)
            .unwrap_err()
            .contains("terminal_output_seen"));

        let mut blank_session = passing;
        blank_session.session_id = "  ".to_string();
        assert!(validate_package_smoke_receipt(&blank_session)
            .unwrap_err()
            .contains("session id"));
    }

    #[test]
    fn host_invoke_response_round_trips_both_arms() {
        let ok = HostInvokeResponse::ok("a".to_string(), json!([1, 2, 3]));
        let err = HostInvokeResponse::err("b".to_string(), "boom".to_string());
        for response in [ok, err] {
            let text = serde_json::to_string(&response).unwrap();
            assert!(text.contains(&format!(r#""kind":"{HOST_INVOKE_RESULT_KIND}""#)));
            let back: HostInvokeResponse = serde_json::from_str(&text).unwrap();
            assert_eq!(response, back);
        }
    }

    #[test]
    fn body_unwraps_request_envelope_and_bare_object() {
        let wrapped: AgentWriteBody =
            body(json!({ "request": { "agent_id": "codex", "data": [1, 2] } })).unwrap();
        assert_eq!(wrapped.agent_id, "codex");
        let bare: AgentWriteBody = body(json!({ "agent_id": "claude", "data": [3] })).unwrap();
        assert_eq!(bare.agent_id, "claude");
    }

    #[test]
    fn body_errors_on_non_object_payload() {
        // Boundary: non-object (e.g. scalar) must error (exercises the serde path in body() helper).
        let res: Result<AgentWriteBody, _> = body(json!(42));
        assert!(res.is_err());
        let msg = res.unwrap_err();
        assert!(msg.contains("invalid request payload") || msg.contains("invalid"));
    }

    #[test]
    fn body_array_payload_errors() {
        let res: Result<AgentWriteBody, _> = body(json!([1, 2, 3]));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("invalid request payload"));
    }

    #[test]
    fn body_object_without_request_uses_bare_map() {
        // Exercises the unwrap_or(Value::Object(map)) arm for bare objects (no "request" key).
        let bare: AgentWriteBody = body(json!({"agent_id": "bare", "data": []})).unwrap();
        assert_eq!(bare.agent_id, "bare");
    }

    #[test]
    fn body_request_not_object_errors() {
        let res: Result<AgentWriteBody, _> = body(json!({"request": 123}));
        assert!(res.is_err());
    }

    #[derive(Deserialize, Debug)]
    struct AgentWriteBody {
        agent_id: String,
        #[allow(dead_code)]
        data: Vec<u8>,
    }

    #[tokio::test]
    async fn dispatch_agent_snapshot_returns_empty_array() {
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "host-1".to_string(),
                command: "agent_snapshot".to_string(),
                payload: Value::Null,
            },
        )
        .await;
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result, json!([]));
    }

    #[tokio::test]
    async fn dispatch_agent_platforms_returns_registry_catalog_with_ion() {
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "host-platforms".to_string(),
                command: "agent_platforms".to_string(),
                payload: Value::Null,
            },
        )
        .await;

        assert!(response.ok, "{:?}", response.error);
        let ids = response
            .result
            .as_array()
            .expect("platform catalog array")
            .iter()
            .filter_map(|value| value.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(ids.contains(&"ion"), "missing Ion platform: {ids:?}");
    }

    #[tokio::test]
    async fn dispatch_governed_task_mutation_returns_daemon_acknowledgment() {
        let task: impulse_ops::governed_task::GovernedTaskRun = serde_json::from_value(json!({
            "id": "task-host-dispatch",
            "revision": 0,
            "project_id": "impulse-rs",
            "workspace_root": "/tmp/impulse-rs",
            "task": "Exercise the live host governed-task route",
            "runtime_id": "codex",
            "agent_id": "codex-live",
            "execution_state": "running",
            "review_state": "awaiting_claim",
            "claims": [],
            "verifications": [],
            "supervisor_verdicts": [],
            "operator_decisions": [],
            "events": [],
            "created_at": "2026-07-13T20:00:00Z",
            "updated_at": "2026-07-13T20:00:00Z"
        }))
        .unwrap();
        let gateway = Arc::new(AcknowledgingGovernedGateway {
            task,
            mutations: Mutex::new(Vec::new()),
        });
        let state = test_state_with_governed_gateway(gateway.clone());
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "governed-host".to_string(),
                command: GOVERNED_TASK_MUTATE_COMMAND.to_string(),
                payload: json!({
                    "request": {
                        "request_id": "req-host-dispatch",
                        "project_id": "impulse-rs",
                        "task_id": "task-host-dispatch",
                        "expected_revision": 0,
                        "mutation": {
                            "kind": "mark_runtime_exited",
                            "data": {
                                "actor": { "kind": "system", "id": "desktop-runtime" },
                                "reason": "test exit"
                            }
                        }
                    }
                }),
            },
        )
        .await;

        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result["id"], "task-host-dispatch");
        assert_eq!(response.result["revision"], 1);
        assert_eq!(
            gateway
                .mutations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn dispatch_unknown_command_reports_error() {
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "host-9".to_string(),
                command: "not_a_command".to_string(),
                payload: Value::Null,
            },
        )
        .await;
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("unknown host command"));
        assert_eq!(response.id, "host-9");
    }

    #[tokio::test]
    async fn invoke_worker_preserves_fifo_wire_order() {
        let state = test_state();
        let (request_tx, request_rx) = channel(2);
        let (response_tx, mut response_rx) = unbounded_channel();
        let worker = tokio::spawn(dispatch_host_invokes_fifo(state, request_rx, response_tx));

        for id in ["host-first", "host-second"] {
            request_tx
                .send(HostInvokeRequest {
                    id: id.to_string(),
                    command: format!("unknown-{id}"),
                    payload: Value::Null,
                })
                .await
                .unwrap();
        }
        drop(request_tx);

        assert_eq!(response_rx.recv().await.unwrap().id, "host-first");
        assert_eq!(response_rx.recv().await.unwrap().id, "host-second");
        worker.await.unwrap();
    }

    #[tokio::test]
    async fn dispatch_register_then_list_workspace_round_trips_through_state() {
        let state = test_state();
        let register = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "reg".to_string(),
                command: "register_workspace".to_string(),
                payload: json!({ "request": { "root": "/tmp", "label": "scratch" } }),
            },
        )
        .await;
        assert!(register.ok, "{:?}", register.error);

        let list = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "list".to_string(),
                command: "list_workspaces".to_string(),
                payload: Value::Null,
            },
        )
        .await;
        assert!(list.ok, "{:?}", list.error);
        let roots: Vec<String> = list
            .result
            .as_array()
            .expect("workspaces serialize as an array")
            .iter()
            .filter_map(|entry| entry["target"]["root"].as_str().map(str::to_string))
            .collect();
        assert!(roots.iter().any(|root| root == "/tmp"), "got {roots:?}");
    }

    #[tokio::test]
    async fn dispatch_mcp_descriptors_lists_builtin_tools() {
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "desc".to_string(),
                command: "mcp_descriptors".to_string(),
                payload: Value::Null,
            },
        )
        .await;
        assert!(response.ok, "{:?}", response.error);
        let names: Vec<String> = response
            .result
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect();
        assert!(names.iter().any(|name| name == "impulse.agent_spawn"));
    }

    #[tokio::test]
    async fn dispatch_bad_payload_reports_error_not_panic() {
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "bad".to_string(),
                command: "agent_write".to_string(),
                payload: json!({ "request": { "agent_id": 5 } }),
            },
        )
        .await;
        assert!(!response.ok);
        assert!(response.error.unwrap().contains("invalid request payload"));
    }

    #[tokio::test]
    async fn dispatch_array_payload_hits_body_error_path() {
        // Ensures real dispatch_host_invoke + body() is exercised for malformed array (not just JS sim in smoke).
        // Use a command that actually calls body(payload) (snapshot ignores payload).
        let state = test_state();
        let response = dispatch_host_invoke(
            &state,
            HostInvokeRequest {
                id: "arrbad".to_string(),
                command: "agent_write".to_string(),
                payload: json!([1, 2]),
            },
        )
        .await;
        assert!(!response.ok);
        let err = response.error.unwrap_or_default();
        assert!(err.contains("invalid request payload") || err.contains("unknown"));
    }

    #[test]
    fn event_envelope_unwraps_terminal_output_to_handler_shape() {
        let envelope = host_event_envelope(&DesktopEvent::TerminalOutput {
            agent_id: "codex".to_string(),
            data: vec![111, 107],
        });
        assert_eq!(envelope["kind"], "host_event");
        assert_eq!(envelope["event"], "terminal_output");
        assert_eq!(envelope["payload"]["agent_id"], "codex");
        assert_eq!(envelope["payload"]["data"], json!([111, 107]));
    }

    #[test]
    fn event_envelope_uses_snake_case_event_names() {
        let supervisor = host_event_envelope(&DesktopEvent::SupervisorLocalAction {
            action: LocalSupervisorAction::FocusAgent {
                agent_id: "codex".to_string(),
            },
        });
        assert_eq!(supervisor["event"], "supervisor_local_action");
        assert_eq!(supervisor["payload"]["action"]["kind"], "focus_agent");
    }

    #[test]
    fn channel_event_sink_forwards_emitted_events() {
        let (sink, mut rx) = channel_event_sink();
        sink.emit(DesktopEvent::TerminalExit {
            agent_id: "codex".to_string(),
        });
        let event = rx.try_recv().expect("event forwarded");
        assert_eq!(event.name(), "terminal_exit");
    }

    #[test]
    fn live_host_context_yields_receiver_once() {
        let (_sink, rx) = channel_event_sink();
        let context = LiveHostContext::new(test_state(), rx);
        let first = context.event_rx.lock().unwrap().take();
        let second = context.event_rx.lock().unwrap().take();
        assert!(first.is_some());
        assert!(second.is_none(), "receiver must only be claimed once");
    }

    #[test]
    fn live_host_bridge_script_declares_transport_contract() {
        let script = live_host_bridge_script();
        assert!(script.contains("window.__IMPULSE_DESKTOP_HOST = host"));
        assert!(script.contains(r#"kind: "host_invoke""#));
        assert!(script.contains("host_invoke_result"));
        assert!(script.contains("host_event"));
        assert!(script.contains("await dioxus.recv()"));
        assert!(script.contains("dioxus.send("));
        assert!(script.contains("let invokeTail = Promise.resolve()"));
        assert!(script.contains("invokeTail = scheduled.then("));
        assert!(script.contains(LIVE_HOST_BRIDGE_STATUS));
        assert!(script.contains(LIVE_HOST_READINESS_COMMAND));
        assert!(script.contains(PACKAGE_SMOKE_COMPLETE_COMMAND));
        assert!(script.contains("IMPULSE_PTY_SMOKE_OK"));
        assert!(script.contains("assets/vendor/xterm/xterm.css"));
        assert!(script.contains("document.styleSheets"));
        assert!(script.contains("assetsReady: terminalReady && fitReady && stylesheetReady"));
        assert!(!script.contains("await fetch("));
        assert!(script.contains("terminal_opened"));
        assert!(script.contains("terminal_output_seen"));
        assert!(script.contains("ops_update_seen"));
        assert!(script.contains("finally"));
        assert!(script.contains("terminalOpened && !terminalClosed"));
        assert!(script.contains("receiptAcknowledged"));
        assert!(script.contains("impulse-shutdown-probe"));
        assert!(script.contains("IMPULSE_SHUTDOWN_PROBE_READY"));
        assert!(!script.contains("window.close()"));
        // Must NOT advertise the pending sentinel — it is the live transport.
        assert_ne!(
            LIVE_HOST_BRIDGE_STATUS,
            crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS
        );
    }

    #[test]
    fn live_host_smoke_uses_loaded_effects_when_custom_scheme_fetch_is_unavailable() {
        let Ok(node_version) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("node is unavailable; skipping live-host asset readiness smoke");
            return;
        };
        if !node_version.status.success() {
            eprintln!("node is unavailable; skipping live-host asset readiness smoke");
            return;
        }

        let smoke = format!(
            r#"
const bridgeScript = {bridge_script};
const liveStatus = {live_status};
const calls = [];
let fetchCalls = 0;
let openCount = 0;
let inbox = [];
let waiters = [];

const push = (message) => {{
  const waiter = waiters.shift();
  if (waiter) waiter(message);
  else inbox.push(message);
}};
const respond = (request, result) => push({{
  kind: "host_invoke_result",
  id: request.id,
  ok: true,
  result,
}});
const event = (name, payload) => push({{
  kind: "host_event",
  event: name,
  payload,
}});

class Terminal {{}}
class FitAddon {{}}
global.window = {{
  Terminal,
  FitAddon: {{ FitAddon }},
  dispatchEvent() {{}},
  close() {{
    console.log(JSON.stringify({{ calls, fetchCalls }}));
    process.exit(0);
  }},
}};
global.document = {{
  styleSheets: [{{ href: "dioxus://index/assets/vendor/xterm/xterm.css" }}],
  documentElement: {{ setAttribute() {{}} }},
}};
global.CustomEvent = class CustomEvent {{
  constructor(name, init) {{ this.name = name; this.detail = init?.detail; }}
}};
global.fetch = async () => {{
  fetchCalls += 1;
  throw new Error("custom scheme Fetch is opaque");
}};
global.dioxus = {{
  send(request) {{
    calls.push(request.command);
    queueMicrotask(() => {{
      if (request.command === "impulse_host_ready") {{
        respond(request, {{ status: liveStatus, smoke_mode: true, smoke_cwd: "/tmp" }});
      }} else if (request.command === "terminal_open") {{
        openCount += 1;
        const sessionId = openCount === 1
          ? "impulse-package-smoke"
          : "impulse-shutdown-probe";
        respond(request, {{ session_id: sessionId, alive: true }});
        if (openCount === 2) {{
          event("terminal_output", {{
            agent_id: sessionId,
            data: Array.from(new TextEncoder().encode("IMPULSE_SHUTDOWN_PROBE_READY\n")),
          }});
        }}
      }} else if (request.command === "terminal_write") {{
        respond(request, null);
        event("terminal_output", {{
          agent_id: "impulse-package-smoke",
          data: Array.from(new TextEncoder().encode("IMPULSE_PTY_SMOKE_OK\n")),
        }});
        event("ops_update", {{ agents: [{{ id: "impulse-package-smoke" }}] }});
      }} else if (request.command === "terminal_close") {{
        respond(request, null);
        event("terminal_exit", {{ agent_id: "impulse-package-smoke" }});
      }} else if (request.command === "impulse_package_smoke_complete") {{
        respond(request, request.payload.request);
        setTimeout(() => {{
          console.log(JSON.stringify({{ calls, fetchCalls }}));
          process.exit(0);
        }}, 0);
      }} else {{
        respond(request, null);
      }}
    }});
  }},
  recv() {{
    if (inbox.length) return Promise.resolve(inbox.shift());
    return new Promise((resolve) => waiters.push(resolve));
  }},
}};

eval(bridgeScript);
setTimeout(() => {{
  console.error(JSON.stringify({{ error: "smoke timed out", calls, fetchCalls }}));
  process.exit(1);
}}, 1000);
"#,
            bridge_script =
                serde_json::to_string(live_host_bridge_script()).expect("serialize bridge script"),
            live_status =
                serde_json::to_string(LIVE_HOST_BRIDGE_STATUS).expect("serialize live status"),
        );
        let tempdir = tempfile::tempdir().expect("create live-host smoke tempdir");
        let script_path = tempdir.path().join("live-host-loaded-effects-smoke.js");
        std::fs::write(&script_path, smoke).expect("write live-host smoke script");
        let output = std::process::Command::new("node")
            .arg(&script_path)
            .output()
            .expect("run live-host loaded-effects smoke");
        assert!(
            output.status.success(),
            "live-host loaded-effects smoke failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse live-host smoke output");
        assert_eq!(result["fetchCalls"], serde_json::json!(0));
        let calls = result["calls"].as_array().expect("host call list");
        for required in [
            "impulse_host_ready",
            "terminal_open",
            "terminal_write",
            "terminal_close",
            "impulse_package_smoke_complete",
        ] {
            assert!(
                calls.iter().any(|call| call == required),
                "missing live-host call {required}: {result}"
            );
        }
    }

    #[test]
    fn open_platform_id_round_trips_through_host_bridge_wire_shape() {
        // Open registry identities stay plain-string serializable across the
        // host boundary without reintroducing a closed desktop enum.
        for id in [
            "codex",
            "claude-code",
            "gemini",
            "cursor",
            "ion",
            "custom-agent",
        ] {
            let platform = AgentPlatformId::try_new(id).unwrap();
            let text = serde_json::to_string(&platform).unwrap();
            let back: AgentPlatformId = serde_json::from_str(&text).unwrap();
            assert_eq!(platform, back);
        }
    }
}
