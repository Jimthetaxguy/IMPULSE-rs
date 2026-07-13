use std::collections::HashMap;

use dioxus::prelude::*;
use impulse_ops::{
    agent_registry::AgentPlatformInfo,
    role_assignment::{
        evaluate_role_compatibility, AgentRoleAssignment, AgentRoleId, EnforcementStrength,
        RoleAssignmentError, RoleCapabilityRequirement, RuntimeCapabilityId,
    },
    ProjectOpsSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::host_commands::{McpInvokeRequest, RegisterWorkspaceRequest};
use crate::mcp::{McpInvocation, ReviewDecision, ReviewQueueItem, ReviewQueueStatus};
use crate::runtime::{
    default_builtin_mcp_tools, AgentPlatformId, AgentRuntimeSnapshot, AgentSpawnRequest,
    BuiltInMcpTool, WorkspaceTarget,
};
use crate::theme::{
    format_count, format_relative_age, status_dot_class, status_label, usage_meter_pct,
};
use crate::views::{ArtifactsView, DesktopView, MemoryView, ShellIntent};
use crate::workspace::WorkspaceEntry;

const CRT_CSS: &str = include_str!("../assets/impulse_crt.css");
pub const XTERM_CSS_PATH: &str = "assets/vendor/xterm/xterm.css";
pub const XTERM_JS_PATH: &str = "assets/vendor/xterm/xterm.js";
pub const XTERM_FIT_JS_PATH: &str = "assets/vendor/xterm/addon-fit.js";

pub fn terminal_asset_paths() -> &'static [&'static str] {
    &[XTERM_CSS_PATH, XTERM_JS_PATH, XTERM_FIT_JS_PATH]
}

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
  const resolveImpulseHostAdapter = () => {
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
  const { invoke, listen, hostKind } = resolveImpulseHostAdapter();
"#
    };
}

const DESKTOP_EVENT_BRIDGE_SCRIPT: &str = concat!(
    r#"
(async () => {
  const existing = window.__impulseOpsBridge;
  if (existing?.unlisten?.length) {
    for (const unlisten of existing.unlisten) {
      try { await unlisten(); } catch (_) {}
    }
  }

"#,
    impulse_host_adapter_resolution_script!(),
    r#"
  window.__impulseOpsBridge = {
    mounted: true,
    degraded: !listen,
    hostKind,
    unlisten: [],
  };
  document.documentElement?.setAttribute("data-impulse-host-kind", hostKind);
  document.documentElement?.setAttribute(
    "data-impulse-ops-bridge",
    listen ? "mounted" : "degraded"
  );

  const forward = (kind, payload) => {
    try {
      dioxus.send({ kind, payload });
    } catch (error) {
      console.warn("impulse ops bridge send failed", error);
    }
  };
  const markEventBridgeDegraded = (reason) => {
    window.__impulseOpsBridge.degraded = true;
    document.documentElement?.setAttribute("data-impulse-ops-bridge", "degraded");
    document.documentElement?.setAttribute("data-impulse-ops-bridge-reason", reason);
    forward("bridge_status", { status: "degraded", reason });
  };

  const refreshReviewQueue = async () => {
    if (!invoke) {
      forward("bridge_status", { status: "review_queue_failed", reason: "host invoke API unavailable" });
      return [];
    }
    try {
      const items = await invoke("review_queue");
      forward("review_queue", { items });
      return items;
    } catch (error) {
      forward("bridge_status", { status: "review_queue_failed", reason: String(error) });
      return [];
    }
  };
  const refreshAgents = async () => {
    if (!invoke) {
      forward("bridge_status", { status: "agent_snapshot_failed", reason: "host invoke API unavailable" });
      return [];
    }
    try {
      const agents = await invoke("agent_snapshot");
      forward("agent_snapshot", { agents });
      return agents;
    } catch (error) {
      forward("bridge_status", { status: "agent_snapshot_failed", reason: String(error) });
      return [];
    }
  };
  const refreshWorkspaces = async () => {
    if (!invoke) {
      forward("bridge_status", { status: "workspaces_failed", reason: "host invoke API unavailable" });
      return [];
    }
    try {
      const workspaces = await invoke("list_workspaces");
      forward("workspaces", { workspaces });
      return workspaces;
    } catch (error) {
      forward("bridge_status", { status: "workspaces_failed", reason: String(error) });
      return [];
    }
  };
  const refreshMcpDescriptors = async () => {
    if (!invoke) {
      forward("bridge_status", { status: "mcp_descriptors_failed", reason: "host invoke API unavailable" });
      return [];
    }
    try {
      const tools = await invoke("mcp_descriptors");
      forward("mcp_descriptors", { tools });
      return tools;
    } catch (error) {
      forward("bridge_status", { status: "mcp_descriptors_failed", reason: String(error) });
      return [];
    }
  };
  const refreshAgentPlatforms = async () => {
    if (!invoke) {
      forward("bridge_status", { status: "agent_platforms_failed", reason: "host invoke API unavailable" });
      return [];
    }
    try {
      const platforms = await invoke("agent_platforms");
      forward("agent_platforms", { platforms });
      return platforms;
    } catch (error) {
      forward("bridge_status", { status: "agent_platforms_failed", reason: String(error) });
      return [];
    }
  };

  window.__impulseOpsBridge.refreshAgents = refreshAgents;
  window.__impulseOpsBridge.refreshWorkspaces = refreshWorkspaces;
  window.__impulseOpsBridge.refreshMcpDescriptors = refreshMcpDescriptors;
  window.__impulseOpsBridge.refreshAgentPlatforms = refreshAgentPlatforms;
  window.__impulseOpsBridge.refreshReviewQueue = refreshReviewQueue;
  window.__impulseOpsBridge.registerWorkspace = async (request) => {
    if (!invoke) {
      forward("bridge_status", { status: "register_workspace_failed", reason: "host invoke API unavailable" });
      return null;
    }
    try {
      const entry = await invoke("register_workspace", { request });
      forward("workspace_registered", { entry });
      await refreshWorkspaces();
      return entry;
    } catch (error) {
      forward("bridge_status", {
        status: "register_workspace_failed",
        reason: String(error),
        request,
      });
      throw error;
    }
  };
  window.__impulseOpsBridge.invokeMcp = async (request) => {
    if (!invoke) {
      forward("bridge_status", { status: "mcp_invoke_failed", reason: "host invoke API unavailable" });
      return null;
    }
    try {
      const invocation = await invoke("mcp_invoke", { request });
      forward("mcp_invocation", { invocation });
      await refreshAgents();
      await refreshWorkspaces();
      return invocation;
    } catch (error) {
      forward("bridge_status", {
        status: "mcp_invoke_failed",
        reason: String(error),
        request,
      });
      throw error;
    }
  };
  window.__impulseOpsBridge.focusAgent = async (agentId) => {
    if (!invoke) {
      forward("bridge_status", { status: "agent_focus_failed", reason: "host invoke API unavailable" });
      return null;
    }
    try {
      const snapshot = await invoke("agent_focus", { request: { session_id: agentId } });
      forward("agent_runtime_update", snapshot);
      await refreshAgents();
      return snapshot;
    } catch (error) {
      forward("bridge_status", {
        status: "agent_focus_failed",
        reason: String(error),
        agent_id: agentId,
      });
      throw error;
    }
  };
  window.__impulseOpsBridge.reviewDecision = async (request) => {
    if (!invoke) {
      forward("bridge_status", { status: "review_decision_failed", reason: "host invoke API unavailable" });
      return null;
    }
    const commandRequest = { ...request, confirmed: true };
    try {
      const invocation = await invoke("review_decision", { request: commandRequest });
      forward("mcp_invocation", { invocation });
      await refreshReviewQueue();
      return invocation;
    } catch (error) {
      forward("bridge_status", {
        status: "review_decision_failed",
        reason: String(error),
        request: commandRequest,
      });
      throw error;
    }
  };

  if (!listen) {
    markEventBridgeDegraded("host event API unavailable");
    await new Promise(() => {});
  }

  const opsUnlisten = await listen("ops_update", (event) => {
    forward("ops_update", event?.payload ?? event);
  });
  const runtimeUnlisten = await listen("agent_runtime_update", (event) => {
    forward("agent_runtime_update", event?.payload ?? event);
  });
  const exitUnlisten = await listen("terminal_exit", (event) => {
    forward("terminal_exit", event?.payload ?? event);
  });
  const opsConnectionUnlisten = await listen("ops_connection_update", (event) => {
    forward("ops_connection_update", event?.payload ?? event);
  });
  window.__impulseOpsBridge.unlisten = [opsUnlisten, runtimeUnlisten, exitUnlisten, opsConnectionUnlisten];

  if (invoke) {
    await refreshAgents();
    await refreshWorkspaces();
    await refreshMcpDescriptors();
    await refreshAgentPlatforms();
    await refreshReviewQueue();
  }

  await new Promise(() => {});
})();
"#
);

const TERMINAL_INTEROP_SCRIPT: &str = concat!(
    r#"
(() => {
"#,
    impulse_host_adapter_resolution_script!(),
    r#"
  const Terminal = window.Terminal || window.XTerm?.Terminal;
  const FitAddonCtor = window.FitAddon?.FitAddon || window.FitAddon;
  const mounts = Array.from(document.querySelectorAll("[data-xterm-mount='true']"));

  const interop = window.__impulseTerminalInterop || {
    mounted: true,
    terminals: {},
    unlisten: [],
    listenersMounted: false,
  };
  interop.mounted = true;
  interop.degraded = !invoke || !listen || !Terminal;
  interop.hostKind = hostKind;
  window.__impulseTerminalInterop = interop;
  document.documentElement?.setAttribute("data-impulse-host-kind", hostKind);

  if (!invoke || !listen || !Terminal) {
    mounts.forEach((mount) => mount.setAttribute("data-xterm-state", "degraded"));
    return "degraded";
  }

  const decoder = new TextDecoder();
  const encoder = new TextEncoder();

  const resolvePayload = (event) => event?.payload ?? event;
  const resolveAgentId = (payload) => payload?.agent_id || payload?.agentId;
  const resolveBytes = (payload) => {
    const data = payload?.data;
    if (typeof data === "string") return data;
    if (Array.isArray(data)) return decoder.decode(new Uint8Array(data));
    if (data instanceof Uint8Array) return decoder.decode(data);
    return "";
  };
  const encodeInput = (data) => Array.from(encoder.encode(data));

  const mountAgentTerminal = (mount) => {
    const agentId = mount.dataset.agentId;
    if (!agentId || interop.terminals[agentId]) {
      return;
    }

    const terminal = new Terminal({
      convertEol: true,
      cursorBlink: true,
      fontFamily: "SFMono-Regular, Menlo, Monaco, monospace",
      fontSize: 13,
    });
    const fitAddon = FitAddonCtor ? new FitAddonCtor() : null;
    if (fitAddon) terminal.loadAddon(fitAddon);
    terminal.open(mount);
    fitAddon?.fit();

    terminal.onData((data) => {
      invoke("agent_write", { request: { agent_id: agentId, data: encodeInput(data) } });
    });

    if (terminal.onResize) {
      terminal.onResize(({ cols, rows }) => {
        invoke("agent_resize", { request: { session_id: agentId, cols, rows } });
      });
    }

    interop.terminals[agentId] = terminal;
    mount.setAttribute("data-xterm-state", "mounted");
  };

  mounts.forEach(mountAgentTerminal);

  if (interop.listenersMounted) {
    return "mounted";
  }
  interop.listenersMounted = true;

  Promise.resolve(listen("terminal_output", (event) => {
    const payload = resolvePayload(event);
    const agentId = resolveAgentId(payload);
    const terminal = interop.terminals[agentId];
    if (terminal) terminal.write(resolveBytes(payload));
  })).then((unlisten) => interop.unlisten.push(unlisten));

  Promise.resolve(listen("terminal_exit", (event) => {
    const payload = resolvePayload(event);
    const agentId = resolveAgentId(payload);
    const terminal = interop.terminals[agentId];
    if (terminal) terminal.write("\r\n[process exited]\r\n");
  })).then((unlisten) => interop.unlisten.push(unlisten));

  return "mounted";
})();
"#
);

pub fn terminal_interop_script() -> &'static str {
    TERMINAL_INTEROP_SCRIPT
}

pub fn desktop_event_bridge_script() -> &'static str {
    DESKTOP_EVENT_BRIDGE_SCRIPT
}

pub fn workspace_registration_bridge_script(request: &RegisterWorkspaceRequest) -> String {
    let payload = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"(async () => {{
  const bridge = window.__impulseOpsBridge;
  if (!bridge?.registerWorkspace) {{
    console.warn("impulse workspace registration bridge unavailable");
    return "degraded";
  }}
  return await bridge.registerWorkspace({payload});
}})();"#
    )
}

pub fn mcp_invoke_bridge_script(request: &McpInvokeRequest) -> String {
    let payload = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"(async () => {{
  const bridge = window.__impulseOpsBridge;
  if (!bridge?.invokeMcp) {{
    console.warn("impulse MCP bridge unavailable");
    return "degraded";
  }}
  return await bridge.invokeMcp({payload});
}})();"#
    )
}

pub fn agent_launch_bridge_script(request: &AgentSpawnRequest) -> String {
    let request = McpInvokeRequest {
        tool: "impulse.agent_spawn".to_string(),
        arguments: serde_json::to_value(request).unwrap_or(Value::Null),
        confirmed: true,
        caller_agent_id: Some("impulse-ui".to_string()),
    };
    mcp_invoke_bridge_script(&request)
}

pub fn agent_focus_bridge_script(agent_id: &str) -> String {
    let payload = serde_json::to_string(agent_id).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(async () => {{
  const bridge = window.__impulseOpsBridge;
  if (!bridge?.focusAgent) {{
    console.warn("impulse agent focus bridge unavailable");
    return "degraded";
  }}
  return await bridge.focusAgent({payload});
}})();"#
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesktopBridgeMessage {
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

/// Parsed `bridge_status` signal. The ops/terminal bridges forward these when
/// the host transport is unavailable (`degraded`) or an individual `invoke`
/// rejects (`*_failed`), so the shell can show the operator that something is
/// wrong instead of silently dropping the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeStatusUpdate {
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonOpsStatusUpdate {
    pub connected: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl DaemonOpsStatusUpdate {
    pub fn parse(message: &DesktopBridgeMessage) -> Option<Self> {
        if message.kind != "ops_connection_update" {
            return None;
        }
        let payload = message
            .payload
            .get("data")
            .unwrap_or(&message.payload)
            .clone();
        serde_json::from_value(payload).ok()
    }
}

impl BridgeStatusUpdate {
    /// Extract a status update from a `bridge_status` bridge message, or `None`
    /// for any other message kind (or a malformed payload missing `status`).
    pub fn parse(message: &DesktopBridgeMessage) -> Option<Self> {
        if message.kind != "bridge_status" {
            return None;
        }
        let status = message.payload.get("status").and_then(Value::as_str)?;
        if status.is_empty() {
            return None;
        }
        let reason = message
            .payload
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string);
        Some(Self {
            status: status.to_string(),
            reason,
        })
    }

    /// Every status the host emits is a problem signal; only explicit recovery
    /// markers clear the banner.
    pub fn is_degraded(&self) -> bool {
        !matches!(self.status.as_str(), "mounted" | "ok" | "ready")
    }

    /// Human-facing one-liner for the banner.
    pub fn headline(&self) -> String {
        match self.status.as_str() {
            "degraded" => "Host bridge degraded".to_string(),
            other => {
                let action = other.trim_end_matches("_failed").replace('_', " ");
                format!("Host call failed: {action}")
            }
        }
    }

    fn revokes_agent_platform_catalog(&self) -> bool {
        self.status == "agent_platforms_failed"
    }
}

/// Degraded-state banner. Rendered in the shell chrome whenever a
/// [`BridgeStatusUpdate::is_degraded`] status is active so the operator sees
/// that the Rust host transport is unavailable or a call failed.
#[component]
fn BridgeStatusBanner(status: BridgeStatusUpdate) -> Element {
    let reason = status
        .reason
        .clone()
        .unwrap_or_else(|| "no detail reported".to_string());
    rsx! {
        div {
            class: "bridge-status-banner",
            role: "alert",
            "data-bridge-status": "{status.status}",
            span { class: "bridge-status-mark", "!" }
            span { class: "bridge-status-headline", "{status.headline()}" }
            span { class: "bridge-status-reason", "{reason}" }
        }
    }
}

// ──────────────────────────── New components ────────────────────────────

#[component]
fn ViewRail(active: DesktopView, on_select: EventHandler<DesktopView>) -> Element {
    rsx! {
        nav { class: "view-rail", "aria-label": "Desktop views",
            for view in DesktopView::ALL {
                {
                    let class_name = if view == active { "rail-item active" } else { "rail-item" };
                    rsx! {
                        button {
                            key: "{view.slug()}",
                            class: "{class_name}",
                            "data-view": "{view.slug()}",
                            onclick: move |_| on_select.call(view),
                            "{view.label()}"
                        }
                    }
                }
            }
        }
    }
}

/// Replace the hard-coded workspace buttons in the left-rail. Renders one
/// rail-item per `WorkspaceEntry`; the selected entry gets the `active`
/// class. Click → `on_select` fires with the absolute `root` string so the
/// parent can decide whether to touch the registry or push the choice into
/// a focused-workspace signal.
#[component]
fn WorkspaceSwitcher(
    workspaces: Vec<WorkspaceEntry>,
    selected_root: String,
    on_select: EventHandler<String>,
) -> Element {
    rsx! {
        section { class: "workspace-picker", "data-source": "workspace_target",
            "aria-label": "Workspaces",
            h2 { "Workspaces" }
            if workspaces.is_empty() {
                p { class: "rail-empty", "No workspaces registered" }
            } else {
                for entry in workspaces.iter() {
                    {
                        let is_active = entry.target.root == selected_root;
                        let root = entry.target.root.clone();
                        let root_for_click = root.clone();
                        let class_name = if is_active { "rail-item active" } else { "rail-item" };
                        let label = entry.label().to_string();
                        rsx! {
                            button {
                                class: "{class_name}",
                                title: "{entry.target.root}",
                                onclick: move |_| on_select.call(root_for_click.clone()),
                                "{label}" }
                        }
                    }
                }
            }
        }
    }
}

/// Replace the hard-coded agent buttons in the left-rail. Driven by
/// `Vec<AgentRuntimeSnapshot>`. Empty state renders a hint.
#[component]
fn AgentPool(
    agents: Vec<AgentRuntimeSnapshot>,
    focused_agent_id: Option<String>,
    on_focus: EventHandler<String>,
) -> Element {
    rsx! {
        section { class: "agent-pool", "data-source": "agent_snapshot",
            "aria-label": "Agents",
            h2 { "Agents" }
            if agents.is_empty() {
                p { class: "rail-empty", "No agents running" }
            } else {
                for snapshot in agents.iter() {
                    {
                        let is_active = focused_agent_id.as_deref() == Some(snapshot.agent_id.as_str());
                        let class_name = if is_active { "rail-item active" } else { "rail-item" };
                        let id = snapshot.agent_id.clone();
                        let id_for_click = id.clone();
                        rsx! {
                            button {
                                class: "{class_name}",
                                "data-agent-id": "{id}",
                                onclick: move |_| on_focus.call(id_for_click.clone()),
                                span { class: "dot {status_dot_class(&snapshot.status)}" }
                                "{snapshot.label}"
                                span { class: "agent-status-label", "{status_label(&snapshot.status)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// List every MCP tool descriptor. Mutating tools (`requires_confirmation = true`)
/// get a per-row confirm checkbox that gates the Invoke button. Read-only tools
/// get a simple button. The last invocation per tool is shown inline as a small
/// status badge (ok/err/—).
#[component]
fn McpToolPalette(
    tools: Vec<BuiltInMcpTool>,
    last_invocations: Vec<McpInvocation>,
    on_invoke: EventHandler<McpInvokeRequest>,
) -> Element {
    rsx! {
        section { class: "inspector-section mcp-tools", "data-source": "builtin_mcp_tools",
            "aria-label": "Rust MCP tools",
            h2 { "Rust MCP Tools" }
            p { class: "section-hint", "agent_spawn and agent_write require confirmation" }
            for tool in tools.iter() {
                {
                    let last = last_invocations.iter().rev().find(|inv| inv.tool == tool.name);
                    let last_marker = match last {
                        Some(inv) if inv.ok => "ok",
                        Some(_) => "err",
                        None => "—",
                    };
                    let last_call_id = last.map(|inv| inv.call_id.clone()).unwrap_or_default();
                    let last_caller = last.and_then(|inv| inv.caller_agent_id.clone()).unwrap_or_default();
                    rsx! {
                        article { class: "mcp-tool", "data-tool": "{tool.name}",
                            header { class: "mcp-tool-header",
                                h3 { "{tool.name}" }
                                span {
                                    class: "mcp-tool-state state-{last_marker}",
                                    title: "last call {last_call_id} by {last_caller}",
                                    "{last_marker}" }
                            }
                            p { class: "mcp-tool-description", "{tool.description}" }
                            ul { class: "mcp-tool-capabilities",
                                for cap in tool.capabilities.iter() {
                                    li { "{cap}" }
                                }
                            }
                            if tool.requires_confirmation {
                                McpConfirmRow {
                                    tool_name: tool.name.clone(),
                                    on_invoke: on_invoke,
                                }
                            } else {
                                McpReadOnlyRow {
                                    tool_name: tool.name.clone(),
                                    on_invoke: on_invoke,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Private row used by `<McpToolPalette>` for tools that mutate terminal
/// state. A confirm checkbox gates the Invoke button. The user can paste a
/// JSON arguments blob in the text field; if it fails to parse we fall back
/// to an empty JSON object (which most tools accept as "no args").
#[component]
fn McpConfirmRow(tool_name: String, on_invoke: EventHandler<McpInvokeRequest>) -> Element {
    let mut confirmed = use_signal(|| false);
    let mut query = use_signal(String::new);
    rsx! {
        div { class: "mcp-tool-row mutating",
            input {
                r#type: "checkbox",
                id: "confirm-{tool_name}",
                checked: confirmed(),
                oninput: move |evt| confirmed.set(evt.value() == "true"),
            }
            label { r#for: "confirm-{tool_name}",
                "I understand this mutates terminal state" }
            input {
                r#type: "text",
                placeholder: "arguments JSON (optional)",
                value: "{query}",
                oninput: move |evt| query.set(evt.value()),
            }
            button {
                class: "invoke-button",
                disabled: !confirmed(),
                onclick: move |_| {
                    let arguments = serde_json::from_str::<serde_json::Value>(&query())
                        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                    on_invoke.call(McpInvokeRequest {
                        tool: tool_name.clone(),
                        arguments,
                        confirmed: confirmed(),
                        caller_agent_id: None,
                    });
                },
                "Invoke" }
        }
    }
}

/// Private row used by `<McpToolPalette>` for read-only tools.
#[component]
fn McpReadOnlyRow(tool_name: String, on_invoke: EventHandler<McpInvokeRequest>) -> Element {
    let mut query = use_signal(String::new);
    rsx! {
        div { class: "mcp-tool-row readonly",
            input {
                r#type: "text",
                placeholder: "arguments JSON (optional)",
                value: "{query}",
                oninput: move |evt| query.set(evt.value()),
            }
            button {
                class: "invoke-button",
                onclick: move |_| {
                    let arguments = serde_json::from_str::<serde_json::Value>(&query())
                        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
                    on_invoke.call(McpInvokeRequest {
                        tool: tool_name.clone(),
                        arguments,
                        confirmed: true,
                        caller_agent_id: None,
                    });
                },
                "Invoke" }
        }
    }
}

/// Fixed first product-role profile exposed by the Dioxus workspace launcher.
///
/// Workspace selection and process lifecycle must be mediated by the desktop
/// runtime. Structural filesystem scope is requested as an optional capability
/// so its current absence remains visible without overstating cwd mediation.
pub fn builder_role_assignment() -> Result<AgentRoleAssignment, RoleAssignmentError> {
    Ok(AgentRoleAssignment {
        role: AgentRoleId::try_new("builder")?,
        requirements: vec![
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new("workspace.target")?,
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            },
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new("process.lifecycle")?,
                minimum_enforcement: EnforcementStrength::Mediated,
                mandatory: true,
            },
            RoleCapabilityRequirement {
                capability: RuntimeCapabilityId::try_new("filesystem.scoped")?,
                minimum_enforcement: EnforcementStrength::Structural,
                mandatory: false,
            },
        ],
    })
}

/// Builds the exact governed launch request accepted by the workspace launcher.
///
/// This is also the launch preflight: missing or stale catalog entries,
/// evaluator errors, blocked role compatibility, blank tasks, and invalid
/// launch roots all return an error before a request can reach the host bridge.
pub fn build_governed_agent_spawn_request(
    workspaces: &[WorkspaceEntry],
    platforms: &[AgentPlatformInfo],
    launch_root: &str,
    selected_platform_id: &str,
    agent_id: &str,
    command: &str,
    task: &str,
) -> Result<AgentSpawnRequest, String> {
    let root = launch_root.trim();
    if root.is_empty() {
        return Err("a workspace root is required".to_string());
    }

    let task = task.trim();
    if task.is_empty() {
        return Err("a governed task is required".to_string());
    }

    let descriptor = platforms
        .iter()
        .find(|candidate| candidate.id.as_str() == selected_platform_id)
        .ok_or_else(|| "the selected platform is absent from the current catalog".to_string())?;
    let role_assignment = builder_role_assignment()
        .map_err(|error| format!("invalid Builder role assignment: {error}"))?;
    let compatibility = evaluate_role_compatibility(
        &descriptor.id,
        &descriptor.runtime_capabilities,
        &role_assignment,
    )
    .map_err(|error| format!("invalid platform capability profile: {error}"))?;
    if compatibility.is_blocked() {
        return Err("the selected platform is blocked for the Builder role".to_string());
    }

    let platform = AgentPlatformId::try_new(selected_platform_id.to_string())
        .map_err(|error| format!("invalid selected platform: {error}"))?;
    let agent_id = optional_text(agent_id.to_string());
    let session_id = agent_id.as_ref().map(|value| format!("{value}-session"));
    let workspace = workspaces
        .iter()
        .find(|entry| entry.target.root == root)
        .map(|entry| entry.target.clone())
        .unwrap_or_else(|| WorkspaceTarget::from_root(root));

    Ok(AgentSpawnRequest {
        agent_id,
        session_id,
        platform,
        command: optional_text(command.to_string()),
        args: Vec::new(),
        cwd: Some(root.to_string()),
        env: HashMap::new(),
        workspace: Some(workspace),
        mcp_tools: default_builtin_mcp_tools(),
        rows: 32,
        cols: 100,
        role: None,
        task: Some(task.to_string()),
        role_assignment: Some(role_assignment),
        target: None,
    })
}

fn enforcement_strength_label(strength: EnforcementStrength) -> &'static str {
    match strength {
        EnforcementStrength::Unsupported => "unsupported",
        EnforcementStrength::Advisory => "advisory",
        EnforcementStrength::Mediated => "mediated",
        EnforcementStrength::Structural => "structural",
    }
}

#[component]
fn WorkspaceLaunchPanel(
    workspaces: Vec<WorkspaceEntry>,
    platforms: Vec<AgentPlatformInfo>,
    selected_root: String,
    on_register: EventHandler<RegisterWorkspaceRequest>,
    on_launch: EventHandler<AgentSpawnRequest>,
) -> Element {
    let mut register_root = use_signal(String::new);
    let mut label = use_signal(String::new);
    let mut purpose = use_signal(String::new);
    let mut project_notes = use_signal(String::new);
    let mut selected_workspace_root = use_signal(String::new);
    let mut platform = use_signal(String::new);
    let mut agent_id = use_signal(String::new);
    let mut command = use_signal(String::new);
    let mut task = use_signal(String::new);

    let first_workspace_root = workspaces
        .first()
        .map(|entry| entry.target.root.clone())
        .unwrap_or_default();
    let selected_from_panel = selected_workspace_root();
    let launch_root = if !selected_from_panel.trim().is_empty() {
        selected_from_panel
    } else if !selected_root.trim().is_empty() {
        selected_root
    } else {
        first_workspace_root
    };
    let can_register = !register_root().trim().is_empty();
    let selected_platform = platform();
    let selected_platform_id = if selected_platform.is_empty() {
        platforms
            .first()
            .map(|candidate| candidate.id.to_string())
            .unwrap_or_default()
    } else {
        selected_platform
    };
    let selected_platform_descriptor = platforms
        .iter()
        .find(|candidate| candidate.id.as_str() == selected_platform_id);
    let role_assignment = builder_role_assignment();
    let role_compatibility = selected_platform_descriptor.and_then(|descriptor| {
        role_assignment.as_ref().ok().map(|assignment| {
            evaluate_role_compatibility(
                &descriptor.id,
                &descriptor.runtime_capabilities,
                assignment,
            )
        })
    });
    let compatibility_status = match role_compatibility.as_ref() {
        Some(Ok(compatibility)) if compatibility.is_blocked() => "blocked",
        Some(Ok(compatibility)) if compatibility.is_degraded() => "degraded",
        Some(Ok(_)) => "allowed",
        Some(Err(_)) | None => "blocked",
    };
    let compatibility_copy = match compatibility_status {
        "allowed" => "Allowed: every Builder launch requirement is satisfied.",
        "degraded" => {
            "Degraded: mandatory launch controls are satisfied; optional filesystem scope is unavailable."
        }
        _ => "Blocked: this platform does not satisfy every mandatory Builder launch control.",
    };
    let compatibility_checks = role_compatibility
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|compatibility| compatibility.checks.clone())
        .unwrap_or_default();
    let launch_request = build_governed_agent_spawn_request(
        &workspaces,
        &platforms,
        &launch_root,
        &selected_platform_id,
        &agent_id(),
        &command(),
        &task(),
    );
    let can_launch = launch_request.is_ok();
    let request_for_launch = launch_request.ok();

    rsx! {
        section { class: "inspector-section workspace-launch", "data-source": "workspace_launcher",
            "aria-label": "Workspace launcher",
            header { class: "section-header",
                div {
                    h2 { "Workspace Launcher" }
                    p { "Register a folder, then launch any agent from the runtime registry inside it." }
                }
                span { class: "workspace-launch-badge", "MCP audited" }
            }
            div { class: "workspace-launch-grid",
                label { class: "workspace-field wide",
                    span { "Folder path" }
                    input {
                        r#type: "text",
                        placeholder: "Absolute project folder",
                        value: "{register_root}",
                        oninput: move |evt| register_root.set(evt.value()),
                    }
                }
                label { class: "workspace-field",
                    span { "Label" }
                    input {
                        r#type: "text",
                        placeholder: "IMPULSE-rs",
                        value: "{label}",
                        oninput: move |evt| label.set(evt.value()),
                    }
                }
                label { class: "workspace-field",
                    span { "Purpose" }
                    input {
                        r#type: "text",
                        placeholder: "terminal harness",
                        value: "{purpose}",
                        oninput: move |evt| purpose.set(evt.value()),
                    }
                }
                label { class: "workspace-field wide",
                    span { "Project notes" }
                    input {
                        r#type: "text",
                        placeholder: "Context the agent should remember before acting",
                        value: "{project_notes}",
                        oninput: move |evt| project_notes.set(evt.value()),
                    }
                }
                button {
                    class: "invoke-button workspace-primary",
                    disabled: !can_register,
                    onclick: move |_| {
                        let root = register_root().trim().to_string();
                        if root.is_empty() {
                            return;
                        }
                        on_register.call(RegisterWorkspaceRequest {
                            root,
                            label: optional_text(label()),
                            purpose: optional_text(purpose()),
                            project_notes: optional_text(project_notes()),
                        });
                    },
                    "Register folder"
                }
            }
            div { class: "workspace-launch-grid launch",
                label { class: "workspace-field wide",
                    span { "Launch from" }
                    select {
                        value: "{launch_root}",
                        onchange: move |evt| selected_workspace_root.set(evt.value()),
                        if workspaces.is_empty() {
                            option { value: "", "Register a workspace first" }
                        } else {
                            for entry in workspaces.iter() {
                                option { value: "{entry.target.root}", "{entry.label()}" }
                            }
                        }
                    }
                }
                label { class: "workspace-field",
                    span { "Agent" }
                    select {
                        value: "{selected_platform_id}",
                        onchange: move |evt| platform.set(evt.value()),
                        if platforms.is_empty() {
                            option { value: "", disabled: true, "Agent registry unavailable" }
                        } else {
                            for candidate in platforms.iter() {
                                option { value: "{candidate.id}", "{candidate.label}" }
                            }
                        }
                    }
                }
                label { class: "workspace-field",
                    span { "Agent id" }
                    input {
                        r#type: "text",
                        placeholder: "optional",
                        value: "{agent_id}",
                        oninput: move |evt| agent_id.set(evt.value()),
                    }
                }
                label { class: "workspace-field wide",
                    span { "Command override" }
                    input {
                        r#type: "text",
                        placeholder: "blank uses platform default",
                        value: "{command}",
                        oninput: move |evt| command.set(evt.value()),
                    }
                }
                label {
                    class: "workspace-field wide",
                    "data-field": "launch-task",
                    span { "Task" }
                    input {
                        r#type: "text",
                        required: true,
                        "aria-required": "true",
                        placeholder: "Describe the explicit assignment",
                        value: "{task}",
                        oninput: move |evt| task.set(evt.value()),
                    }
                }
                div {
                    class: "workspace-field wide role-compatibility {compatibility_status}",
                    "data-role": "builder",
                    "data-compatibility": compatibility_status,
                    div { class: "role-compatibility-heading",
                        span { "Product role" }
                        strong { "Builder" }
                        span { class: "compatibility-status", "{compatibility_status}" }
                    }
                    p { "{compatibility_copy}" }
                    if compatibility_checks.is_empty() {
                        p { class: "compatibility-empty", "No trusted runtime capability evidence is available for this platform." }
                    } else {
                        ul { class: "compatibility-checks",
                            for check in compatibility_checks.iter() {
                                li {
                                    "data-capability": "{check.capability}",
                                    "data-required": enforcement_strength_label(check.required),
                                    "data-available": enforcement_strength_label(check.available),
                                    strong { "{check.capability}" }
                                    span { if check.mandatory { "mandatory" } else { "optional" } }
                                    span {
                                        "required {enforcement_strength_label(check.required)} · available {enforcement_strength_label(check.available)}"
                                    }
                                }
                            }
                        }
                    }
                    p { class: "compatibility-caveat",
                        "Workspace cwd mediation selects and validates the launch directory; cwd mediation is not a filesystem sandbox."
                    }
                }
                button {
                    class: "invoke-button workspace-primary",
                    "data-action": "launch-governed-agent",
                    "aria-disabled": if can_launch { "false" } else { "true" },
                    disabled: !can_launch,
                    onclick: move |_| {
                        let Some(request) = request_for_launch.clone() else {
                            return;
                        };
                        on_launch.call(request);
                    },
                    "Launch agent"
                }
            }
        }
    }
}

/// New inspector subsection. Scrollable, capped to the first 100 invocations
/// the parent passes in. Optional agent filter narrows the visible rows.
#[component]
fn AuditTrail(invocations: Vec<McpInvocation>, agent_filter: Option<String>) -> Element {
    rsx! {
        section { class: "inspector-section audit-trail", "data-source": "mcp_audit",
            "aria-label": "MCP audit trail",
            header { class: "section-header",
                h2 { "Audit Trail" }
                span { class: "audit-count", "{invocations.len()} invocations" }
            }
            div { class: "audit-list", role: "list", "aria-label": "MCP invocation log",
                for inv in invocations.iter().take(100) {
                    {
                        let passes_filter = match agent_filter.as_deref() {
                            Some(needle) => inv.caller_agent_id.as_deref() == Some(needle),
                            None => true,
                        };
                        if !passes_filter {
                            rsx! {}
                        } else {
                            let state = if inv.ok { "ok" } else { "err" };
                            let caller = inv.caller_agent_id.clone().unwrap_or_else(|| "<supervisor>".to_string());
                            let call_id_short: String = inv.call_id.chars().take(8).collect();
                            rsx! {
                                div { class: "audit-row state-{state}", role: "listitem",
                                    span { class: "audit-tool", "{inv.tool}" }
                                    span { class: "audit-state", "{state}" }
                                    span { class: "audit-caller", "{caller}" }
                                    span { class: "audit-call-id", title: "{inv.call_id}",
                                        "{call_id_short}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn optional_text(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Footer event-stream health label. `down` when the host transport is
/// degraded (no events can arrive), `live` when data has actually been
/// observed on the stream, otherwise `idle`. Replaces the previous hardcoded
/// "stream pending" so the strip reflects real state.
fn event_stream_state(active: bool, degraded: bool) -> &'static str {
    if degraded {
        "down"
    } else if active {
        "live"
    } else {
        "idle"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewDecisionUiRequest {
    pub id: String,
    pub decision: ReviewDecision,
    pub target_agent_id: Option<String>,
}

pub fn review_decision_bridge_script(request: &ReviewDecisionUiRequest) -> String {
    let payload = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_string());
    format!(
        r#"(async () => {{
  const bridge = window.__impulseOpsBridge;
  if (!bridge?.reviewDecision) {{
    console.warn("impulse review decision bridge unavailable");
    return "degraded";
  }}
  return await bridge.reviewDecision({payload});
}})();"#
    )
}

/// First-class review queue surface. It shows staged payloads and exposes
/// apply/skip events to the parent shell, which will route them through the
/// host MCP decision path in the live app.
#[component]
fn ReviewConsole(
    items: Vec<ReviewQueueItem>,
    on_decision: EventHandler<ReviewDecisionUiRequest>,
) -> Element {
    let pending = items
        .iter()
        .filter(|item| item.status == ReviewQueueStatus::Pending)
        .count();
    rsx! {
        section { class: "review-console", "data-source": "review_queue",
            "aria-label": "Review queue",
            header { class: "review-console-header",
                div {
                    h2 { "Review Queue" }
                    p { "aria-live": "polite", "{pending} pending · {items.len()} staged" }
                }
                span { class: "review-console-badge", "review-first" }
            }
            if items.is_empty() {
                p { class: "section-empty", "No staged context awaiting review" }
            } else {
                div { class: "review-items",
                    for item in items.iter().take(6) {
                        {
                            let status = review_status_label(&item.status);
                            let target = item.target_agent_id.clone().unwrap_or_else(|| "select target".to_string());
                            let is_pending = item.status == ReviewQueueStatus::Pending;
                            let can_apply = is_pending && item.target_agent_id.is_some();
                            let can_skip = is_pending;
                            let apply_id = item.id.clone();
                            let apply_target = item.target_agent_id.clone();
                            let skip_id = item.id.clone();
                            let skip_target = item.target_agent_id.clone();
                            let short_id: String = item.id.chars().take(8).collect();
                            rsx! {
                                article { class: "review-item status-{status}", "data-review-id": "{item.id}",
                                    header { class: "review-item-header",
                                        div {
                                            h3 { "staged {short_id}" }
                                            span { class: "review-target", "{target}" }
                                        }
                                        span { class: "review-status", "{status}" }
                                    }
                                    pre { class: "review-preview", "{item.preview}" }
                                    div { class: "review-actions",
                                        button {
                                            class: "invoke-button",
                                            disabled: !can_apply,
                                            onclick: move |_| {
                                                on_decision.call(ReviewDecisionUiRequest {
                                                    id: apply_id.clone(),
                                                    decision: ReviewDecision::Apply,
                                                    target_agent_id: apply_target.clone(),
                                                });
                                            },
                                            "Apply"
                                        }
                                        button {
                                            class: "invoke-button secondary",
                                            disabled: !can_skip,
                                            onclick: move |_| {
                                                on_decision.call(ReviewDecisionUiRequest {
                                                    id: skip_id.clone(),
                                                    decision: ReviewDecision::Skip,
                                                    target_agent_id: skip_target.clone(),
                                                });
                                            },
                                            "Skip"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn review_status_label(status: &ReviewQueueStatus) -> &'static str {
    match status {
        ReviewQueueStatus::Pending => "pending",
        ReviewQueueStatus::Applied => "applied",
        ReviewQueueStatus::Skipped => "skipped",
    }
}

#[component]
fn OperatorBoard(
    runtime_agents: Vec<AgentRuntimeSnapshot>,
    review_queue: Vec<ReviewQueueItem>,
    last_invocations: Vec<McpInvocation>,
) -> Element {
    let queued = runtime_agents
        .iter()
        .filter(|agent| matches!(agent.status, impulse_ops::AgentStatus::Starting))
        .count();
    let in_flight = runtime_agents
        .iter()
        .filter(|agent| matches!(agent.status, impulse_ops::AgentStatus::Working { .. }))
        .count();
    let review = review_queue
        .iter()
        .filter(|item| item.status == ReviewQueueStatus::Pending)
        .count();
    let done = runtime_agents
        .iter()
        .filter(|agent| matches!(agent.status, impulse_ops::AgentStatus::Completed))
        .count()
        + review_queue
            .iter()
            .filter(|item| item.status != ReviewQueueStatus::Pending)
            .count();

    rsx! {
        section { class: "operator-board", "data-source": "operator_board",
            div { class: "operator-lanes",
                OperatorLane { label: "Queued".to_string(), count: queued, hint: "starting agents".to_string() }
                OperatorLane { label: "In flight".to_string(), count: in_flight, hint: "active terminal work".to_string() }
                OperatorLane { label: "Review".to_string(), count: review, hint: "pending context gates".to_string() }
                OperatorLane { label: "Done".to_string(), count: done, hint: "completed or decided".to_string() }
            }
            if !runtime_agents.is_empty() {
                div { class: "operator-agent-strip",
                    for agent in runtime_agents.iter().take(4) {
                        {
                            let audit_count = last_invocations
                                .iter()
                                .filter(|invocation| invocation.caller_agent_id.as_deref() == Some(agent.agent_id.as_str()))
                                .count();
                            let workspace = agent.workspace.as_ref()
                                .and_then(|workspace| workspace.label.as_deref())
                                .or(agent.cwd.as_deref())
                                .unwrap_or("no workspace");
                            rsx! {
                                article { class: "operator-agent-card", "data-agent-id": "{agent.agent_id}",
                                    header {
                                        span { class: "dot {status_dot_class(&agent.status)}" }
                                        h3 { "{agent.label}" }
                                        span { class: "operator-agent-status", "{status_label(&agent.status)}" }
                                    }
                                    p { "{workspace}" }
                                    span { class: "operator-agent-audit", "{audit_count} audit rows" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn OperatorLane(label: String, count: usize, hint: String) -> Element {
    rsx! {
        div { class: "operator-lane",
            span { class: "operator-lane-label", "{label}" }
            strong { "{count}" }
            span { class: "operator-lane-hint", "{hint}" }
        }
    }
}

/// Read-only list of `WorkspaceEntry` rows for the right-inspector. Shows
/// root, label, and last-used timestamp. Empty state renders a hint.
#[component]
fn WorkspaceList(workspaces: Vec<WorkspaceEntry>, selected_root: Option<String>) -> Element {
    rsx! {
        section { class: "inspector-section workspace-list", "data-source": "list_workspaces",
            h2 { "Registered Workspaces" }
            if workspaces.is_empty() {
                p { class: "section-empty", "No workspaces registered" }
            } else {
                {
                    // Current wall-clock time in unix millis, captured once per
                    // render and passed into the pure `format_relative_age`
                    // helper so it stays deterministic/testable.
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    rsx! {
                        ul { class: "workspace-rows",
                            for entry in workspaces.iter() {
                                {
                                    let is_active = selected_root.as_deref() == Some(entry.target.root.as_str());
                                    let last_used = match entry.last_used_unix_ms {
                                        Some(ms) => format_relative_age(ms, now_ms),
                                        None => "never used".to_string(),
                                    };
                                    let class_name = if is_active { "workspace-row active" } else { "workspace-row" };
                                    rsx! {
                                        li { class: "{class_name}", "data-workspace-root": "{entry.target.root}",
                                            span { class: "workspace-label", "{entry.label()}" }
                                            span { class: "workspace-root", title: "{entry.target.root}",
                                                "{entry.target.root}" }
                                            if let Some(notes) = entry.target.project_notes.as_deref() {
                                                span { class: "workspace-notes", title: "{notes}", "notes" }
                                            }
                                            span { class: "workspace-last-used", "{last_used}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct DesktopBridgeStateMut<'a> {
    pub snapshot: &'a mut ProjectOpsSnapshot,
    pub runtime_agents: &'a mut Vec<AgentRuntimeSnapshot>,
    pub agent_platforms: &'a mut Vec<AgentPlatformInfo>,
    pub workspaces: &'a mut Vec<WorkspaceEntry>,
    pub mcp_tools: &'a mut Vec<BuiltInMcpTool>,
    pub review_queue: &'a mut Vec<ReviewQueueItem>,
    pub last_invocations: &'a mut Vec<McpInvocation>,
}

impl<'a> DesktopBridgeStateMut<'a> {
    pub fn new(
        snapshot: &'a mut ProjectOpsSnapshot,
        runtime_agents: &'a mut Vec<AgentRuntimeSnapshot>,
        agent_platforms: &'a mut Vec<AgentPlatformInfo>,
        workspaces: &'a mut Vec<WorkspaceEntry>,
        mcp_tools: &'a mut Vec<BuiltInMcpTool>,
        review_queue: &'a mut Vec<ReviewQueueItem>,
        last_invocations: &'a mut Vec<McpInvocation>,
    ) -> Self {
        Self {
            snapshot,
            runtime_agents,
            agent_platforms,
            workspaces,
            mcp_tools,
            review_queue,
            last_invocations,
        }
    }
}

pub fn apply_desktop_bridge_message(
    state: DesktopBridgeStateMut<'_>,
    message: DesktopBridgeMessage,
) -> Result<(), String> {
    let DesktopBridgeStateMut {
        snapshot,
        runtime_agents,
        agent_platforms,
        workspaces,
        mcp_tools,
        review_queue,
        last_invocations,
    } = state;
    match message.kind.as_str() {
        "ops_update" => {
            let payload = extract_ops_update_payload(&message.payload);
            *snapshot = serde_json::from_value(payload.clone())
                .map_err(|error| format!("invalid ops_update payload: {error}"))?;
            Ok(())
        }
        "agent_runtime_update" => {
            let payload = extract_agent_snapshot_payload(&message.payload);
            let runtime_snapshot = serde_json::from_value::<AgentRuntimeSnapshot>(payload.clone())
                .map_err(|error| format!("invalid agent_runtime_update payload: {error}"))?;
            upsert_agent_runtime(runtime_agents, runtime_snapshot);
            Ok(())
        }
        "agent_snapshot" => {
            let agents = message
                .payload
                .get("agents")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            let runtime_snapshots = serde_json::from_value::<Vec<AgentRuntimeSnapshot>>(agents)
                .map_err(|error| format!("invalid agent_snapshot payload: {error}"))?;
            *runtime_agents = runtime_snapshots
                .into_iter()
                .filter(|snapshot| snapshot.alive)
                .collect();
            Ok(())
        }
        "agent_platforms" => {
            let platforms = message
                .payload
                .get("platforms")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            *agent_platforms = serde_json::from_value::<Vec<AgentPlatformInfo>>(platforms)
                .map_err(|error| format!("invalid agent_platforms payload: {error}"))?;
            Ok(())
        }
        "bridge_status" => {
            let update = BridgeStatusUpdate::parse(&message)
                .ok_or_else(|| "invalid bridge_status payload: missing status".to_string())?;
            if update.revokes_agent_platform_catalog() {
                agent_platforms.clear();
            }
            Ok(())
        }
        "terminal_exit" => {
            let payload = message.payload.get("data").unwrap_or(&message.payload);
            let agent_id = payload
                .get("agent_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid terminal_exit payload: missing agent_id".to_string())?;
            runtime_agents.retain(|agent| agent.agent_id != agent_id);
            Ok(())
        }
        "ops_connection_update" => Ok(()),
        "workspaces" => {
            let items = message
                .payload
                .get("workspaces")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            *workspaces = serde_json::from_value::<Vec<WorkspaceEntry>>(items)
                .map_err(|error| format!("invalid workspaces payload: {error}"))?;
            Ok(())
        }
        "workspace_registered" => {
            let entry = message
                .payload
                .get("entry")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            let entry = serde_json::from_value::<WorkspaceEntry>(entry)
                .map_err(|error| format!("invalid workspace_registered payload: {error}"))?;
            if let Some(existing) = workspaces
                .iter_mut()
                .find(|workspace| workspace.target.root == entry.target.root)
            {
                *existing = entry;
            } else {
                workspaces.push(entry);
            }
            Ok(())
        }
        "mcp_descriptors" | "mcp_tools" => {
            let tools = message
                .payload
                .get("tools")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            *mcp_tools = serde_json::from_value::<Vec<BuiltInMcpTool>>(tools)
                .map_err(|error| format!("invalid mcp_descriptors payload: {error}"))?;
            Ok(())
        }
        "review_queue" => {
            let items = message
                .payload
                .get("items")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            *review_queue = serde_json::from_value::<Vec<ReviewQueueItem>>(items)
                .map_err(|error| format!("invalid review_queue payload: {error}"))?;
            Ok(())
        }
        "mcp_invocation" => {
            let invocation = message
                .payload
                .get("invocation")
                .cloned()
                .unwrap_or_else(|| message.payload.clone());
            let invocation = serde_json::from_value::<McpInvocation>(invocation)
                .map_err(|error| format!("invalid mcp_invocation payload: {error}"))?;
            last_invocations.push(invocation);
            if last_invocations.len() > 100 {
                let overflow = last_invocations.len() - 100;
                last_invocations.drain(0..overflow);
            }
            Ok(())
        }
        other => Err(format!("unknown desktop bridge message `{other}`")),
    }
}

fn upsert_agent_runtime(
    runtime_agents: &mut Vec<AgentRuntimeSnapshot>,
    runtime: AgentRuntimeSnapshot,
) {
    if !runtime.alive {
        runtime_agents.retain(|agent| agent.agent_id != runtime.agent_id);
        return;
    }
    if runtime.focused {
        for agent in runtime_agents.iter_mut() {
            if agent.agent_id != runtime.agent_id {
                agent.focused = false;
            }
        }
    }
    if let Some(existing) = runtime_agents
        .iter_mut()
        .find(|agent| agent.agent_id == runtime.agent_id)
    {
        *existing = runtime.clone();
    } else {
        runtime_agents.push(runtime.clone());
    }
}

fn extract_ops_update_payload(payload: &Value) -> &Value {
    payload
        .get("data")
        .and_then(|data| data.get("payload"))
        .or_else(|| payload.get("payload"))
        .unwrap_or(payload)
}

fn extract_agent_snapshot_payload(payload: &Value) -> &Value {
    payload
        .get("data")
        .and_then(|data| data.get("snapshot"))
        .or_else(|| payload.get("snapshot"))
        .unwrap_or(payload)
}

#[component]
fn Stat(k: String, v: String, s: String) -> Element {
    rsx! {
        div { class: "stat",
            div { class: "k", "{k}" }
            div { class: "v", "{v}" }
            div { class: "s", "{s}" }
        }
    }
}

#[component]
fn PendingReview(count: usize) -> Element {
    rsx! {
        div { class: "pending-bar",
            span { class: "label",
                span { class: "mark", ">" }
                "{count} injection(s) awaiting review"
            }
            span { class: "keys",
                b { "[a]" } " apply  " b { "[d]" } " diff  " b { "[s]" } " skip"
            }
        }
    }
}

#[component]
fn BrandHero() -> Element {
    let blade_colors = [
        "#ff8a1e", "#ff6a00", "#ffb01a", "#2fd6a8", "#2e7bff", "#5b63ff", "#2fd0ff", "#ff8a1e",
    ];
    let blades: Vec<(f64, f64, f64, &str)> = blade_colors
        .iter()
        .enumerate()
        .map(|(i, color)| {
            let angle = (i as f64 / 8.0) * std::f64::consts::TAU;
            let (cx, cy, radius) = (130.0, 130.0, 78.0);
            (
                cx + angle.cos() * radius,
                cy + angle.sin() * radius,
                angle.to_degrees() + 90.0,
                *color,
            )
        })
        .collect();

    rsx! {
        div { class: "crt-hero",
            div { style: "position:relative;width:200px;height:200px;",
                svg {
                    width: "200",
                    height: "200",
                    view_box: "0 0 260 260",
                    style: "position:absolute;inset:0;",
                    for (i, (x, y, rot, color)) in blades.iter().enumerate() {
                        g {
                            key: "{i}",
                            class: "glow-soft",
                            transform: "translate({x},{y}) rotate({rot})",
                            rect {
                                x: "-9",
                                y: "-30",
                                width: "18",
                                height: "52",
                                fill: "{color}",
                            }
                        }
                    }
                    circle {
                        cx: "130",
                        cy: "130",
                        r: "46",
                        fill: "none",
                        stroke: "#ffb01a",
                        stroke_width: "3",
                        style: "filter:drop-shadow(0 0 6px #ff6a00);",
                    }
                }
                div { style: "position:absolute;inset:0;display:grid;place-items:center;",
                    svg {
                        width: "64",
                        height: "99",
                        view_box: "0 0 60 93",
                        class: "glow-blue",
                        path { d: "M30 2 C40 14 44 30 44 48 L44 64 L16 64 L16 48 C16 30 20 14 30 2 Z", fill: "#5b63ff" }
                        circle { cx: "30", cy: "34", r: "8", fill: "#000" }
                        circle { cx: "30", cy: "34", r: "5", fill: "#2fd0ff" }
                        path { d: "M16 50 L4 70 L16 64 Z", fill: "#ff6a00" }
                        path { d: "M44 50 L56 70 L44 64 Z", fill: "#ff6a00" }
                        rect { x: "16", y: "64", width: "28", height: "6", fill: "#5b63ff" }
                        path { d: "M20 70 L30 92 L40 70 Z", fill: "#ffb01a" }
                        path { d: "M24 70 L30 84 L36 70 Z", fill: "#ff3b1f" }
                    }
                }
            }
            div { style: "text-align:left;",
                div { class: "brand-wordmark", "impulse" }
                div { class: "brand-tagline", "your ai remembers" }
            }
        }
    }
}

#[component]
pub fn DesktopShellWithSnapshot(
    snapshot: ProjectOpsSnapshot,
    #[props(default)] runtime_agents: Vec<AgentRuntimeSnapshot>,
    #[props(default)] agent_platforms: Vec<AgentPlatformInfo>,
    #[props(default)] workspaces: Vec<WorkspaceEntry>,
    #[props(default)] mcp_tools: Vec<BuiltInMcpTool>,
    #[props(default)] last_invocations: Vec<McpInvocation>,
    #[props(default)] review_queue: Vec<ReviewQueueItem>,
    #[props(default)] bridge_status: Option<BridgeStatusUpdate>,
    #[props(default)] daemon_ops_status: Option<DaemonOpsStatusUpdate>,
    #[props(default = DesktopView::Terminal)] initial_view: DesktopView,
) -> Element {
    let context = &snapshot.context;
    let tokens = format_count(context.estimated_tokens);
    let window = format_count(context.window_tokens);
    let usage_pct = usage_meter_pct(context.usage_fraction);
    let snapshot_agents_online = snapshot.agents.iter().filter(|agent| agent.active).count();
    let snapshot_working_agents = snapshot
        .agents
        .iter()
        .filter(|agent| matches!(agent.agent_status, impulse_ops::AgentStatus::Working { .. }))
        .count();
    let pending_queue_count = review_queue
        .iter()
        .filter(|item| item.status == ReviewQueueStatus::Pending)
        .count();
    let pending_review_count = context.pending_review_count.max(pending_queue_count);
    let daemon_online = daemon_ops_status
        .as_ref()
        .map(|status| status.connected)
        .unwrap_or_else(|| !snapshot.generated_at.is_empty());
    let daemon_publish_degraded = daemon_ops_status
        .as_ref()
        .is_some_and(|status| status.connected && status.error.is_some());
    let daemon_label = if daemon_publish_degraded {
        "online · publish degraded"
    } else if daemon_online {
        "online · watching"
    } else {
        "daemon offline"
    };
    let daemon_state = if daemon_publish_degraded {
        "degraded"
    } else if daemon_online {
        "online"
    } else {
        "offline"
    };
    let daemon_snapshot_stale = daemon_ops_status
        .as_ref()
        .is_some_and(|status| !status.connected);
    let agents_online = if daemon_online {
        snapshot_agents_online
    } else {
        0
    };
    let working_agents = if daemon_online {
        snapshot_working_agents
    } else {
        0
    };
    let agent_summary = if daemon_online {
        format!("online · {working_agents} working")
    } else {
        "daemon offline · cached snapshot hidden".to_string()
    };
    let artifact_summary = if daemon_online {
        format!("{} artifacts", snapshot.artifacts.len())
    } else {
        format!("{} cached artifacts", snapshot.artifacts.len())
    };
    let intervention_summary = if daemon_online {
        format!("{} interventions", snapshot.interventions.len())
    } else {
        format!("{} cached interventions", snapshot.interventions.len())
    };
    let daemon_detail = daemon_ops_status
        .as_ref()
        .and_then(|status| status.error.as_deref())
        .unwrap_or(daemon_label);
    let ops_stream = match daemon_ops_status.as_ref().map(|status| status.connected) {
        Some(true) => "live",
        Some(false) => "down",
        None => "idle",
    };
    let tier = if context.tier.is_empty() {
        "idle"
    } else {
        context.tier.as_str()
    };
    let generated_at = if snapshot.generated_at.is_empty() {
        "awaiting first ops_update"
    } else {
        snapshot.generated_at.as_str()
    };
    // Real footer stream health derived from observed state (replaces the old
    // permanent "stream pending"). A degraded host transport means no events
    // can arrive at all, so every stream reads `down`.
    let bridge_degraded = bridge_status
        .as_ref()
        .map(BridgeStatusUpdate::is_degraded)
        .unwrap_or(false);
    let terminal_stream = event_stream_state(
        runtime_agents.iter().any(|agent| agent.output_bytes > 0),
        bridge_degraded,
    );
    let runtime_stream = event_stream_state(!runtime_agents.is_empty(), bridge_degraded);
    // No UI consumer subscribes to supervisor_local_action yet, so reflect the
    // transport: `down` when degraded, otherwise `ready` (defined, awaiting a
    // consumer) — never the misleading permanent "pending".
    let supervisor_stream = if bridge_degraded { "down" } else { "ready" };
    let first_workspace_root = workspaces
        .first()
        .map(|entry| entry.target.root.clone())
        .unwrap_or_default();
    let mut focused_workspace_root = use_signal(String::new);
    let selected_root = if focused_workspace_root().trim().is_empty() {
        first_workspace_root
    } else {
        focused_workspace_root()
    };
    let has_runtime_agents = !runtime_agents.is_empty();
    let mut active_view = use_signal(|| initial_view);
    let mut latest_shell_intent = use_signal(|| None::<String>);
    // In-flight feedback for the focus-agent bridge call (a representative
    // high-traffic async action). Holds the agent id currently being focused so
    // the triggering terminal tab can render an aria-busy/disabled "…" state
    // while the host `focusAgent` invoke resolves, then clears on completion.
    let mut focusing_agent_id = use_signal(|| None::<String>);
    let active_view_value = active_view();
    let terminal_view_class = if active_view_value == DesktopView::Terminal {
        "stage-view view-terminal active"
    } else {
        "stage-view view-terminal"
    };

    rsx! {
        link {
            rel: "stylesheet",
            href: XTERM_CSS_PATH,
            "data-impulse-terminal-asset": "xterm-css",
        }
        script {
            src: XTERM_JS_PATH,
            "data-impulse-terminal-asset": "xterm-js",
        }
        script {
            src: XTERM_FIT_JS_PATH,
            "data-impulse-terminal-asset": "xterm-fit-addon",
        }
        style { {CRT_CSS} }
        main {
            class: "impulse-shell",
            "data-daemon-freshness": if daemon_snapshot_stale { "stale" } else { "current" },
            header { class: "top-bar",
                div { class: "brand",
                    h1 { "impulse" }
                    span {
                        class: "daemon-state",
                        "data-state": "{daemon_state}",
                        title: "{daemon_detail}",
                        "{daemon_label}"
                    }
                }
                nav { class: "command-surface",
                    button {
                        class: "icon-button is-disabled",
                        title: "Command palette (coming soon)",
                        disabled: true,
                        "aria-disabled": "true",
                        "Cmd-K"
                    }
                    button {
                        class: "icon-button",
                        title: "Review context",
                        onclick: move |_| active_view.set(DesktopView::Review),
                        "Review"
                    }
                    button {
                        class: "icon-button is-disabled",
                        title: "Settings (coming soon)",
                        disabled: true,
                        "aria-disabled": "true",
                        "Settings"
                    }
                }
            }
            if let Some(status) = bridge_status.as_ref() {
                if status.is_degraded() {
                    BridgeStatusBanner { status: status.clone() }
                }
            }
            if daemon_snapshot_stale {
                section {
                    class: "bridge-status-banner",
                    "data-daemon-status": "stale",
                    role: "status",
                    strong { "Daemon disconnected" }
                    span { "Workbench data is cached and may be stale." }
                }
            }
            if daemon_publish_degraded {
                section {
                    class: "bridge-status-banner",
                    "data-daemon-status": "publish-degraded",
                    role: "status",
                    strong { "Telemetry publish degraded" }
                    span { "Daemon snapshot reads remain live; desktop lifecycle writes will retry." }
                }
            }
            div { class: "workspace-grid",
                aside { class: "left-rail", "data-owner": "dioxus",
                    h2 { "Views" }
                    ViewRail {
                        active: active_view_value,
                        on_select: move |view: DesktopView| active_view.set(view),
                    }
                    AgentPool {
                        agents: runtime_agents.clone(),
                        focused_agent_id: None,
                        on_focus: move |id: String| {
                            let script = agent_focus_bridge_script(&id);
                            spawn(async move {
                                let _ = document::eval(&script).await;
                            });
                        },
                    }
                    WorkspaceSwitcher {
                        workspaces: workspaces.clone(),
                        selected_root: selected_root.clone(),
                        on_select: move |root: String| focused_workspace_root.set(root),
                    }
                }
                section { class: "terminal-stage", "data-terminal-renderer": "xterm.js",
                    div { class: "{terminal_view_class}", "data-view": "terminal",
                        BrandHero {}
                        div { class: "stat-row",
                            Stat {
                                k: "Memory".to_string(),
                                v: tokens.clone(),
                                s: "tokens · {usage_pct}% of {window}",
                            }
                            Stat {
                                k: "Agents".to_string(),
                                v: agents_online.to_string(),
                                s: agent_summary.clone(),
                            }
                            Stat {
                                k: "Retrieval".to_string(),
                                v: snapshot.retrieval.backend.clone(),
                                s: "{snapshot.memory.genome_decisions} genome decisions",
                            }
                        }
                        if pending_review_count > 0 {
                            PendingReview { count: pending_review_count }
                        }
                        div { class: "terminal-tabs", "data-owner": "dioxus",
                            if has_runtime_agents {
                                for agent in runtime_agents.iter() {
                                {
                                    let class_name = if agent.focused { "terminal-tab active" } else { "terminal-tab" };
                                    let agent_id = agent.agent_id.clone();
                                    let agent_id_for_click = agent_id.clone();
                                    let is_focusing = focusing_agent_id().as_deref() == Some(agent_id.as_str());
                                    let label = agent.label.clone();
                                    rsx! {
                                        button {
                                            class: "{class_name}",
                                                "data-agent-id": "{agent_id}",
                                                disabled: is_focusing,
                                                "aria-busy": if is_focusing { "true" } else { "false" },
                                                onclick: move |_| {
                                                    let agent_id = agent_id_for_click.clone();
                                                    let script = agent_focus_bridge_script(&agent_id);
                                                    focusing_agent_id.set(Some(agent_id));
                                                    spawn(async move {
                                                        let _ = document::eval(&script).await;
                                                        focusing_agent_id.set(None);
                                                    });
                                                },
                                                if is_focusing { "focusing…" } else { "{label}" } }
                                        }
                                    }
                                }
                            } else {
                                button { class: "terminal-tab active", "No agent" }
                            }
                        }
                        if has_runtime_agents {
                            for agent in runtime_agents.iter() {
                                {
                                    let pane_id = format!("terminal-pane-{}", impulse_ops::sanitize_id(&agent.agent_id));
                                    let class_name = if agent.focused { "xterm-mount" } else { "xterm-mount pending" };
                                    rsx! {
                                        div {
                                            id: "{pane_id}",
                                            class: "{class_name}",
                                            "data-xterm-mount": "true",
                                            "data-agent-id": "{agent.agent_id}",
                                            "data-platform": "{agent.platform.as_str()}",
                                            "data-pty-owner": "rust-backend",
                                            "data-xterm-on-data": "agent_write",
                                            "data-xterm-on-resize": "agent_resize",
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "terminal-empty-state", "data-terminal-state": "empty",
                                h3 { "No terminal session" }
                                p { "Launch an agent from the workspace panel to attach a Rust-backed xterm pane." }
                            }
                        }
                    }
                    match active_view_value {
                        DesktopView::Terminal => rsx! {},
                        DesktopView::Memory => rsx! {
                            MemoryView {
                                context: snapshot.context.clone(),
                                memory: snapshot.memory.clone(),
                                retrieval: snapshot.retrieval.clone(),
                            }
                        },
                        DesktopView::Review => rsx! {
                            ReviewConsole {
                                items: review_queue.clone(),
                                on_decision: move |request| {
                                    let script = review_decision_bridge_script(&request);
                                    spawn(async move {
                                        let _ = document::eval(&script).await;
                                    });
                                },
                            }
                        },
                        DesktopView::Artifacts => rsx! {
                            ArtifactsView {
                                artifacts: snapshot.artifacts.clone(),
                                on_intent: move |intent: ShellIntent| {
                                    latest_shell_intent.set(Some(intent.describe()));
                                },
                            }
                        },
                        DesktopView::Supervisor => rsx! {
                            OperatorBoard {
                                runtime_agents: runtime_agents.clone(),
                                review_queue: review_queue.clone(),
                                last_invocations: last_invocations.clone(),
                            }
                        },
                    }
                }
                aside { class: "right-inspector", "data-owner": "dioxus",
                    section { class: "inspector-section",
                        h2 { "Context · {tier}" }
                        p { "{tokens} / {window} tokens · {context.injection_count} injections · {context.compaction_count} compactions" }
                    }
                    section { class: "inspector-section", "aria-label": "Pending review",
                        h2 { "Pending review" }
                        p { "aria-live": "polite",
                            if pending_review_count > 0 {
                                "{pending_review_count} bundle(s) awaiting review-first apply"
                            } else {
                                "Nothing pending. Memory is quiet."
                            }
                        }
                    }
                    section { class: "inspector-section",
                        h2 { "Native Islands" }
                        p { "macOS affordances report through serializable DTOs" }
                    }
                    WorkspaceLaunchPanel {
                        workspaces: workspaces.clone(),
                        platforms: agent_platforms.clone(),
                        selected_root: selected_root.clone(),
                        on_register: move |request: RegisterWorkspaceRequest| {
                            let script = workspace_registration_bridge_script(&request);
                            spawn(async move {
                                let _ = document::eval(&script).await;
                            });
                        },
                        on_launch: move |request: AgentSpawnRequest| {
                            let script = agent_launch_bridge_script(&request);
                            spawn(async move {
                                let _ = document::eval(&script).await;
                            });
                        },
                    }
                    McpToolPalette {
                        tools: mcp_tools.clone(),
                        last_invocations: last_invocations.clone(),
                        on_invoke: move |request: McpInvokeRequest| {
                            let script = mcp_invoke_bridge_script(&request);
                            spawn(async move {
                                let _ = document::eval(&script).await;
                            });
                        },
                    }
                    WorkspaceList {
                        workspaces: workspaces.clone(),
                        selected_root: if selected_root.is_empty() { None } else { Some(selected_root.clone()) },
                    }
                    section { class: "inspector-section",
                        h2 { "Supervisor" }
                        p { "Actions require backend confirmation" }
                    }
                    AuditTrail {
                        invocations: last_invocations,
                        agent_filter: None,
                    }
                    section { class: "inspector-section",
                        h2 { "Interop" }
                        p { "Agent runtime updates drive status, files, tools, diffs, and handoffs" }
                    }
                }
            }
            footer { class: "event-strip", "data-owner": "dioxus",
                span { "data-stream": "ops_update", "ops_update {generated_at} · {ops_stream}" }
                span { "{agents_online} agents" }
                span { "{artifact_summary}" }
                span { "{intervention_summary}" }
                span { "data-stream": "terminal_output", "terminal_output · {terminal_stream}" }
                span { "data-stream": "agent_runtime_update", "agent_runtime_update · {runtime_stream}" }
                span { "data-stream": "supervisor_local_action", "supervisor_local_action · {supervisor_stream}" }
                if let Some(intent) = latest_shell_intent() {
                    span { class: "shell-notice", "{intent}" }
                }
            }
        }
    }
}

#[component]
pub fn DesktopShell() -> Element {
    let mut snapshot = use_signal(ProjectOpsSnapshot::default);
    let mut runtime_agents = use_signal(Vec::<AgentRuntimeSnapshot>::new);
    let mut agent_platforms = use_signal(Vec::<AgentPlatformInfo>::new);
    let mut workspaces = use_signal(Vec::<WorkspaceEntry>::new);
    let mut mcp_tools = use_signal(Vec::<BuiltInMcpTool>::new);
    let mut review_queue = use_signal(Vec::<ReviewQueueItem>::new);
    let mut last_invocations = use_signal(Vec::<McpInvocation>::new);
    let mut bridge_status = use_signal(|| None::<BridgeStatusUpdate>);
    let mut daemon_ops_status = use_signal(|| None::<DaemonOpsStatusUpdate>);

    use_effect(move || {
        let _agent_mount_count = runtime_agents().len();
        spawn(async move {
            let _ = document::eval(TERMINAL_INTEROP_SCRIPT).await;
        });
    });

    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(DESKTOP_EVENT_BRIDGE_SCRIPT);
            while let Ok(message) = eval.recv::<DesktopBridgeMessage>().await {
                if let Some(update) = DaemonOpsStatusUpdate::parse(&message) {
                    daemon_ops_status.set(Some(update));
                    continue;
                }
                // Status messages update the operator banner. A platform
                // refresh failure also revokes stale compatibility evidence.
                if let Some(update) = BridgeStatusUpdate::parse(&message) {
                    if update.revokes_agent_platform_catalog() {
                        agent_platforms.set(Vec::new());
                    }
                    bridge_status.set(Some(update));
                    continue;
                }
                let mut next_snapshot = snapshot();
                let mut next_agents = runtime_agents();
                let mut next_platforms = agent_platforms();
                let mut next_workspaces = workspaces();
                let mut next_mcp_tools = mcp_tools();
                let mut next_queue = review_queue();
                let mut next_invocations = last_invocations();
                if apply_desktop_bridge_message(
                    DesktopBridgeStateMut::new(
                        &mut next_snapshot,
                        &mut next_agents,
                        &mut next_platforms,
                        &mut next_workspaces,
                        &mut next_mcp_tools,
                        &mut next_queue,
                        &mut next_invocations,
                    ),
                    message,
                )
                .is_ok()
                {
                    snapshot.set(next_snapshot);
                    runtime_agents.set(next_agents);
                    agent_platforms.set(next_platforms);
                    workspaces.set(next_workspaces);
                    mcp_tools.set(next_mcp_tools);
                    review_queue.set(next_queue);
                    last_invocations.set(next_invocations);
                    // A successful refresh means the transport recovered.
                    if bridge_status.read().is_some() {
                        bridge_status.set(None);
                    }
                }
            }
        });
    });

    rsx! {
        DesktopShellWithSnapshot {
            snapshot: snapshot(),
            runtime_agents: runtime_agents(),
            agent_platforms: agent_platforms(),
            workspaces: workspaces(),
            mcp_tools: mcp_tools(),
            review_queue: review_queue(),
            last_invocations: last_invocations(),
            bridge_status: bridge_status(),
            daemon_ops_status: daemon_ops_status(),
        }
    }
}
