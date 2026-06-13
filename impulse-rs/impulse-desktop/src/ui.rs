use dioxus::prelude::*;
use impulse_ops::ProjectOpsSnapshot;

use crate::mcp::McpInvocation;
use crate::runtime::{AgentRuntimeSnapshot, BuiltInMcpTool};
use crate::tauri_commands::McpInvokeRequest;
use crate::theme::{format_count, status_dot_class, status_label};
use crate::workspace::WorkspaceEntry;

const CRT_CSS: &str = include_str!("../assets/impulse_crt.css");

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
pub fn DesktopShellWithSnapshot(snapshot: ProjectOpsSnapshot) -> Element {
    use_effect(move || {
        spawn(async move {
            let _ = document::eval(TERMINAL_INTEROP_SCRIPT).await;
        });
    });

    let context = &snapshot.context;
    let tokens = format_count(context.estimated_tokens);
    let window = format_count(context.window_tokens);
    let usage_pct = (context.usage_fraction * 100.0).round() as i32;
    let agents_online = snapshot.agents.iter().filter(|agent| agent.active).count();
    let working_agents = snapshot
        .agents
        .iter()
        .filter(|agent| matches!(agent.agent_status, impulse_ops::AgentStatus::Working { .. }))
        .count();
    let pending_review_count = context.pending_review_count;
    let daemon_online = !snapshot.agents.is_empty();
    let daemon_label = if daemon_online {
        "online · watching"
    } else {
        "daemon offline"
    };
    let daemon_state = if daemon_online { "online" } else { "offline" };
    let tier = if context.tier.is_empty() {
        "idle"
    } else {
        context.tier.as_str()
    };
    let generated_at = if snapshot.generated_at.is_empty() {
        "ops_update stream pending"
    } else {
        snapshot.generated_at.as_str()
    };

    rsx! {
        document::Style { {CRT_CSS} }
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Baloo+2:wght@500;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap",
        }
        main { class: "impulse-shell",
            header { class: "top-bar",
                div { class: "brand",
                    h1 { "impulse" }
                    span { class: "daemon-state", "data-state": "{daemon_state}", "{daemon_label}" }
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
                            s: "online · {working_agents} working",
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
                        h2 { "Context · {tier}" }
                        p { "{tokens} / {window} tokens · {context.injection_count} injections · {context.compaction_count} compactions" }
                    }
                    section { class: "inspector-section",
                        h2 { "Pending review" }
                        p {
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
                span { "ops_update {generated_at}" }
                span { "{agents_online} agents" }
                span { "{snapshot.artifacts.len()} artifacts" }
                span { "{snapshot.interventions.len()} interventions" }
                span { "terminal_output stream pending" }
                span { "agent_runtime_update stream pending" }
                span { "supervisor_local_action stream pending" }
            }
        }
    }
}

#[component]
pub fn DesktopShell() -> Element {
    rsx! {
        DesktopShellWithSnapshot {
            snapshot: ProjectOpsSnapshot::default(),
        }
    }
}
