use dioxus::prelude::*;

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
                    section { class: "agent-pool", "data-source": "agent_snapshot",
                        h2 { "Agents" }
                        button { class: "rail-item", "Claude Code" }
                        button { class: "rail-item active", "Codex" }
                        button { class: "rail-item", "OpenCode" }
                        button { class: "rail-item", "Shell" }
                    }
                    section { class: "workspace-picker", "data-source": "workspace_target",
                        h2 { "Workspaces" }
                        button { class: "rail-item active", "Current repo" }
                        button { class: "rail-item", "/code" }
                        button { class: "rail-item", "Desktop clone" }
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
                    section { class: "inspector-section", "data-source": "builtin_mcp_tools",
                        h2 { "Rust MCP Tools" }
                        p { "agent_spawn and agent_write require confirmation" }
                        p { "search_memory is read-only" }
                    }
                    section { class: "inspector-section",
                        h2 { "Supervisor" }
                        p { "Actions require backend confirmation" }
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
