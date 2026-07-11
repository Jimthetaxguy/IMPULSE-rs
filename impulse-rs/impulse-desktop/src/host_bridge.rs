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

use std::sync::{Arc, Mutex, OnceLock};

use dioxus::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::host_commands::{
    self, DesktopShellState, AGENT_CLOSE_COMMAND, AGENT_FOCUS_COMMAND, AGENT_PLATFORMS_COMMAND,
    AGENT_RESIZE_COMMAND, AGENT_SNAPSHOT_COMMAND, AGENT_SPAWN_COMMAND, AGENT_WRITE_COMMAND,
    LIST_WORKSPACES_COMMAND, MCP_DESCRIPTORS_COMMAND, MCP_INVOKE_COMMAND,
    NATIVE_ISLAND_REQUEST_COMMAND, REGISTER_WORKSPACE_COMMAND, REVIEW_DECISION_COMMAND,
    REVIEW_QUEUE_COMMAND, SUPERVISOR_LOCAL_ACTION_COMMAND, TERMINAL_CLOSE_COMMAND,
    TERMINAL_FOCUS_COMMAND, TERMINAL_OPEN_COMMAND, TERMINAL_RESIZE_COMMAND, TERMINAL_WRITE_COMMAND,
};
use crate::runtime::{DesktopEvent, DesktopEventSink};

/// Status the live bridge publishes once it has replaced the manifest-only
/// stubs. Anything other than [`crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS`]
/// reads as "ready" to the host-adapter resolver in `ui.rs`.
pub const LIVE_HOST_BRIDGE_STATUS: &str = "dioxus-eval-bridge-ready";

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
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HostInvokeResponse {
    fn ok(id: String, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result,
            error: None,
        }
    }

    fn err(id: String, error: String) -> Self {
        Self {
            id,
            ok: false,
            result: Value::Null,
            error: Some(error),
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

/// Route one command against the shell state, returning the serialized success
/// value or an error message. Mirrors the non-legacy `host_commands` surface.
async fn dispatch_command(
    state: &DesktopShellState,
    command: &str,
    payload: Value,
) -> Result<Value, String> {
    let runtime = state.runtime.as_ref();
    match command {
        AGENT_SNAPSHOT_COMMAND => json(host_commands::agent_snapshot(runtime).await?),
        AGENT_PLATFORMS_COMMAND => json(host_commands::agent_platforms().await?),
        AGENT_SPAWN_COMMAND => json(host_commands::agent_spawn(runtime, body(payload)?).await?),
        AGENT_WRITE_COMMAND => json(host_commands::agent_write(runtime, body(payload)?).await?),
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
    match dispatch_command(state, &request.command, request.payload).await {
        Ok(result) => HostInvokeResponse::ok(id, result),
        Err(error) => HostInvokeResponse::err(id, error),
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
}

impl LiveHostContext {
    pub fn new(state: DesktopShellState, event_rx: UnboundedReceiver<DesktopEvent>) -> Self {
        Self {
            state,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
        }
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
  const host = {
    invoke(command, payload) {
      return new Promise((resolve, reject) => {
        const id = `host-${++nextId}`;
        pendingInvokes.set(id, { resolve, reject });
        try {
          dioxus.send({ kind: "host_invoke", id, command, payload: payload ?? null });
        } catch (error) {
          pendingInvokes.delete(id);
          reject(error);
        }
      });
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
/// the webview. Runs until either channel closes. This is the only function
/// that touches the Dioxus runtime; everything it calls is plain data.
pub fn use_live_host_bridge() {
    use_future(move || async move {
        let Some(context) = live_host_context() else {
            return;
        };
        let Some(mut event_rx) = context
            .event_rx
            .lock()
            .ok()
            .and_then(|mut guard| guard.take())
        else {
            // Another mount already claimed the receiver; nothing to do.
            return;
        };
        let state = context.state.clone();
        let mut eval = document::eval(LIVE_HOST_BRIDGE_SCRIPT);

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
                request = eval.recv::<HostInvokeRequest>() => {
                    match request {
                        Ok(request) => {
                            let response = dispatch_host_invoke(&state, request).await;
                            if let Ok(value) = serde_json::to_value(&response) {
                                let _ = eval.send(value);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

/// Root component for the live Dioxus desktop app: mounts the host bridge (so
/// `window.__IMPULSE_DESKTOP_HOST` becomes a real transport) and renders the
/// shell. The launcher installs the [`LiveHostContext`] via
/// [`install_live_host_context`] before launching this component.
#[component]
pub fn LiveDesktopApp() -> Element {
    use_live_host_bridge();
    rsx! {
        crate::ui::DesktopShell {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{AgentPlatformId, DesktopRuntime, LocalSupervisorAction};
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

    #[test]
    fn host_invoke_response_round_trips_both_arms() {
        let ok = HostInvokeResponse::ok("a".to_string(), json!([1, 2, 3]));
        let err = HostInvokeResponse::err("b".to_string(), "boom".to_string());
        for response in [ok, err] {
            let text = serde_json::to_string(&response).unwrap();
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
        assert!(script.contains(LIVE_HOST_BRIDGE_STATUS));
        // Must NOT advertise the pending sentinel — it is the live transport.
        assert_ne!(
            LIVE_HOST_BRIDGE_STATUS,
            crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS
        );
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
