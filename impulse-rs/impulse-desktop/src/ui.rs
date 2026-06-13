use dioxus::prelude::*;

use crate::mcp::McpInvocation;
use crate::runtime::{AgentRuntimeSnapshot, BuiltInMcpTool};
use crate::tauri_commands::McpInvokeRequest;
use crate::workspace::WorkspaceEntry;

const TERMINAL_INTEROP_SCRIPT: &str = r#"
(() => {
  if (window.__impulseTerminalInterop?.mounted) {
    return "already-mounted";
  }

  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke;
  const listen = tauri?.event?.listen;
  const Terminal = window.Terminal || window.XTerm?.Terminal;
  const FitAddonCtor = window.FitAddon?.FitAddon || window.FitAddon;
  const mounts = Array.from(document.querySelectorAll("[data-xterm-mount='true']"));

  window.__impulseTerminalInterop = {
    mounted: true,
    terminals: {},
    degraded: !invoke || !listen || !Terminal,
  };

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

  for (const mount of mounts) {
    const agentId = mount.dataset.agentId;
    if (!agentId || window.__impulseTerminalInterop.terminals[agentId]) {
      continue;
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

    window.__impulseTerminalInterop.terminals[agentId] = terminal;
    mount.setAttribute("data-xterm-state", "mounted");
  }

  listen("terminal_output", (event) => {
    const payload = resolvePayload(event);
    const agentId = resolveAgentId(payload);
    const terminal = window.__impulseTerminalInterop.terminals[agentId];
    if (terminal) terminal.write(resolveBytes(payload));
  });

  listen("terminal_exit", (event) => {
    const payload = resolvePayload(event);
    const agentId = resolveAgentId(payload);
    const terminal = window.__impulseTerminalInterop.terminals[agentId];
    if (terminal) terminal.write("\r\n[process exited]\r\n");
  });

  return "mounted";
})();
"#;

pub fn terminal_interop_script() -> &'static str {
    TERMINAL_INTEROP_SCRIPT
}

// ──────────────────────────── New components ────────────────────────────

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
                                "{snapshot.label}" }
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

/// New inspector subsection. Scrollable, capped to the first 100 invocations
/// the parent passes in. Optional agent filter narrows the visible rows.
#[component]
fn AuditTrail(invocations: Vec<McpInvocation>, agent_filter: Option<String>) -> Element {
    rsx! {
        section { class: "inspector-section audit-trail", "data-source": "mcp_audit",
            header { class: "section-header",
                h2 { "Audit Trail" }
                span { class: "audit-count", "{invocations.len()} invocations" }
            }
            div { class: "audit-list",
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
                                div { class: "audit-row state-{state}",
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
                ul { class: "workspace-rows",
                    for entry in workspaces.iter() {
                        {
                            let is_active = selected_root.as_deref() == Some(entry.target.root.as_str());
                            let last_used = match entry.last_used_unix_ms {
                                Some(ms) => format!("last used {} ms epoch", ms),
                                None => "never used".to_string(),
                            };
                            let class_name = if is_active { "workspace-row active" } else { "workspace-row" };
                            rsx! {
                                li { class: "{class_name}", "data-workspace-root": "{entry.target.root}",
                                    span { class: "workspace-label", "{entry.label()}" }
                                    span { class: "workspace-root", title: "{entry.target.root}",
                                        "{entry.target.root}" }
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

#[component]
pub fn DesktopShell() -> Element {
    use_effect(move || {
        spawn(async move {
            let _ = document::eval(TERMINAL_INTEROP_SCRIPT).await;
        });
    });

    rsx! {
        main { class: "impulse-shell",
            header { class: "top-bar",
                div { class: "brand",
                    h1 { "Impulse" }
                    span { class: "daemon-state", "Daemon offline" }
                }
                nav { class: "command-surface",
                    button { class: "icon-button", title: "Command palette", "Cmd-K" }
                    button { class: "icon-button", title: "Review context", "Review" }
                    button { class: "icon-button", title: "Settings", "Settings" }
                }
            }
            div { class: "workspace-grid",
                aside { class: "left-rail", "data-owner": "dioxus",
                    h2 { "Sessions" }
                    button { class: "rail-item active", "Terminal" }
                    button { class: "rail-item", "Memory" }
                    button { class: "rail-item", "Artifacts" }
                    button { class: "rail-item", "Supervisor" }
                    AgentPool {
                        agents: Vec::<AgentRuntimeSnapshot>::new(),
                        focused_agent_id: None,
                        on_focus: move |_id| {},
                    }
                    WorkspaceSwitcher {
                        workspaces: Vec::<WorkspaceEntry>::new(),
                        selected_root: String::new(),
                        on_select: move |_root| {},
                    }
                }
                section { class: "terminal-stage", "data-terminal-renderer": "xterm.js",
                    div { class: "terminal-tabs", "data-owner": "dioxus",
                        button { class: "terminal-tab active", "codex" }
                        button { class: "terminal-tab", "claude" }
                        button { class: "terminal-tab", "shell" }
                    }
                    div {
                        id: "terminal-pane-primary",
                        class: "xterm-mount",
                        "data-xterm-mount": "true",
                        "data-agent-id": "shell",
                        "data-pty-owner": "rust-backend",
                        "data-command-bus": "agent_write",
                        "xterm.js terminal mount"
                    }
                    div {
                        id: "terminal-pane-codex",
                        class: "xterm-mount pending",
                        "data-xterm-mount": "true",
                        "data-agent-id": "codex",
                        "data-platform": "codex",
                        "data-pty-owner": "rust-backend",
                        "data-xterm-on-data": "agent_write",
                        "data-xterm-on-resize": "agent_resize",
                    }
                }
                aside { class: "right-inspector", "data-owner": "dioxus",
                    section { class: "inspector-section",
                        h2 { "Context" }
                        p { "Review-first injection queue" }
                    }
                    section { class: "inspector-section",
                        h2 { "Native Islands" }
                        p { "macOS affordances report through serializable DTOs" }
                    }
                    McpToolPalette {
                        tools: Vec::<BuiltInMcpTool>::new(),
                        last_invocations: Vec::<McpInvocation>::new(),
                        on_invoke: move |_request| {},
                    }
                    WorkspaceList {
                        workspaces: Vec::<WorkspaceEntry>::new(),
                        selected_root: None,
                    }
                    section { class: "inspector-section",
                        h2 { "Supervisor" }
                        p { "Actions require backend confirmation" }
                    }
                    AuditTrail {
                        invocations: Vec::<McpInvocation>::new(),
                        agent_filter: None,
                    }
                    section { class: "inspector-section",
                        h2 { "Interop" }
                        p { "Agent runtime updates drive status, files, tools, diffs, and handoffs" }
                    }
                }
            }
            footer { class: "event-strip", "data-owner": "dioxus",
                span { "ops_update stream pending" }
                span { "terminal_output stream pending" }
                span { "agent_runtime_update stream pending" }
                span { "supervisor_local_action stream pending" }
            }
        }
    }
}
