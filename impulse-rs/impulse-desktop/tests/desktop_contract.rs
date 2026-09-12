use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    path::Path,
    process::Command,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};

use dioxus::prelude::document::{Document, Eval, EvalError, Evaluator};
use dioxus::prelude::*;
use impulse_desktop::ui::{
    agent_focus_bridge_script, agent_launch_bridge_script, apply_desktop_bridge_message,
    apply_desktop_bridge_message_with_status, build_governed_agent_spawn_request,
    builder_role_assignment, desktop_event_bridge_script, governed_task_mutation_bridge_script,
    mcp_invoke_bridge_script, review_decision_bridge_script, terminal_asset_paths,
    workspace_registration_bridge_script, BridgeStatusUpdate, DaemonOpsStatusUpdate,
    DesktopBridgeMessage, DesktopBridgeStateMut, GovernedAgentSpawnInput, ReviewDecisionUiRequest,
    XTERM_CSS_PATH, XTERM_FIT_JS_PATH, XTERM_JS_PATH,
};
use impulse_desktop::{
    default_builtin_mcp_tools, format_count, status_dot_class, status_label, AgentPlatformId,
    AgentRuntimeSnapshot, AgentSpawnRequest, AgentWriteRequest, BuiltInMcpTool,
    DesktopCommandRouter, DesktopEvent, DesktopShell, DesktopShellWithSnapshot,
    DesktopShellWithSnapshotProps, DesktopView, InMemoryTerminalBridge, McpInvocation,
    NativeIslandKind, NativeIslandRequest, RegisterWorkspaceRequest, ReviewDecision,
    ReviewQueueItem, ReviewQueueStatus, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalWriteRequest, WorkspaceEntry,
    WorkspaceTarget,
};
use impulse_ops::{
    agent_registry::AgentPlatformInfo, role_assignment::EnforcementStrength, AgentRuntime,
    AgentStatus, ContextHealthSummary, MemorySummary, ProjectOpsSnapshot, RetrievalSummary,
};
use serde_json::json;

fn platform_id(value: &str) -> AgentPlatformId {
    AgentPlatformId::try_new(value).expect("valid test platform id")
}

#[test]
fn test_agent_platform_info_roundtrips_trusted_runtime_capabilities() {
    let platform: AgentPlatformInfo = serde_json::from_value(json!({
        "id": "codex",
        "label": "Codex",
        "command": "codex",
        "runtime_capabilities": [
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ]
    }))
    .expect("trusted platform capability DTO should deserialize");

    let roundtrip = serde_json::to_value(platform).expect("platform DTO should serialize");
    assert_eq!(
        roundtrip["runtime_capabilities"],
        json!([
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ])
    );
}

#[test]
fn test_builder_role_assignment_requires_mediated_launch_control_and_optional_filesystem_scope() {
    let assignment = builder_role_assignment().expect("static Builder role profile");

    assert_eq!(assignment.role.as_str(), "builder");
    assert_eq!(assignment.requirements.len(), 3);
    assert!(assignment.requirements.iter().any(|requirement| {
        requirement.capability.as_str() == "workspace.target"
            && requirement.minimum_enforcement == EnforcementStrength::Mediated
            && requirement.mandatory
    }));
    assert!(assignment.requirements.iter().any(|requirement| {
        requirement.capability.as_str() == "process.lifecycle"
            && requirement.minimum_enforcement == EnforcementStrength::Mediated
            && requirement.mandatory
    }));
    assert!(assignment.requirements.iter().any(|requirement| {
        requirement.capability.as_str() == "filesystem.scoped"
            && requirement.minimum_enforcement == EnforcementStrength::Structural
            && !requirement.mandatory
    }));
}

#[test]
fn test_workspace_launcher_renders_required_builder_compatibility_preflight() {
    let platform: AgentPlatformInfo = serde_json::from_value(json!({
        "id": "codex",
        "label": "Codex",
        "command": "codex",
        "runtime_capabilities": [
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ]
    }))
    .expect("platform DTO");
    let workspace = WorkspaceEntry::new(WorkspaceTarget::from_root("/tmp/impulse"));
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: Vec::new(),
            agent_platforms: vec![platform],
            workspaces: vec![workspace],
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);
    assert!(html.contains("data-field=\"launch-task\""));
    assert!(html.contains("data-field=\"launch-acceptance-criteria\""));
    assert!(html.contains("aria-required=\"true\""));
    assert!(html.contains(">Task<"));
    assert!(html.contains(">Builder<"));
    assert!(html.contains("data-verification-profile=\"rust_workspace_v1\""));
    assert!(html.contains("Rust-only · rust_workspace_v1"));
    assert!(!html.contains("launch any agent"));
    assert!(html.contains("degraded"));
    assert!(html.contains("workspace.target"));
    assert!(html.contains("process.lifecycle"));
    assert!(html.contains("filesystem.scoped"));
    assert!(html.contains("required mediated · available mediated"));
    assert!(html.contains("required structural · available unsupported"));
    assert!(html.contains("cwd mediation is not a filesystem sandbox"));
    assert!(html.contains("data-action=\"launch-governed-agent\""));
    assert!(html.contains("aria-disabled=\"true\""));
    assert!(html.contains("disabled=\"true\""));
}

fn governed_platform(id: &str, runtime_capabilities: serde_json::Value) -> AgentPlatformInfo {
    serde_json::from_value(json!({
        "id": id,
        "label": id,
        "command": id,
        "runtime_capabilities": runtime_capabilities,
    }))
    .expect("governed platform DTO")
}

fn governed_workspace() -> WorkspaceEntry {
    WorkspaceEntry::new(WorkspaceTarget::from_root("/tmp/impulse"))
}

fn governed_launch(
    platforms: &[AgentPlatformInfo],
    selected_platform: &str,
    task: &str,
) -> Result<AgentSpawnRequest, String> {
    build_governed_agent_spawn_request(
        &[governed_workspace()],
        platforms,
        GovernedAgentSpawnInput {
            launch_root: "/tmp/impulse",
            selected_platform_id: selected_platform,
            agent_id: "builder-1",
            command: "",
            task,
            acceptance_criteria: "workspace tests pass",
        },
    )
}

#[test]
fn test_governed_request_builder_rejects_blank_task() {
    let platform = governed_platform(
        "codex",
        json!([
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ]),
    );

    assert!(governed_launch(&[platform], "codex", "  ").is_err());
}

#[test]
fn test_governed_request_builder_blocks_untrusted_or_incompatible_profiles() {
    let blocked = governed_platform(
        "partial-agent",
        json!([
            { "capability": "workspace.target", "enforcement": "mediated" }
        ]),
    );
    let capability_free = governed_platform("custom-agent", json!([]));
    let evaluator_error = governed_platform(
        "duplicate-profile",
        json!([
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "workspace.target", "enforcement": "structural" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ]),
    );

    assert!(governed_launch(&[blocked], "partial-agent", "Build it").is_err());
    assert!(governed_launch(&[capability_free], "custom-agent", "Build it").is_err());
    assert!(governed_launch(&[evaluator_error], "duplicate-profile", "Build it").is_err());
    assert!(governed_launch(&[], "codex", "Build it").is_err());
}

#[test]
fn test_governed_request_builder_uses_current_selected_platform() {
    let capabilities = json!([
        { "capability": "workspace.target", "enforcement": "mediated" },
        { "capability": "process.lifecycle", "enforcement": "mediated" }
    ]);
    let platforms = vec![
        governed_platform("codex", capabilities.clone()),
        governed_platform("ion", capabilities),
    ];

    let codex_request =
        governed_launch(&platforms, "codex", "Build it").expect("Codex is compatible");
    let ion_request = governed_launch(&platforms, "ion", "Build it").expect("Ion is compatible");

    assert_eq!(codex_request.platform, platform_id("codex"));
    assert_eq!(ion_request.platform, platform_id("ion"));
}

#[test]
fn test_governed_request_builder_emits_trimmed_task_builder_assignment_and_legacy_role_none() {
    let platform = governed_platform(
        "codex",
        json!([
            { "capability": "workspace.target", "enforcement": "mediated" },
            { "capability": "process.lifecycle", "enforcement": "mediated" }
        ]),
    );

    let request = governed_launch(&[platform], "codex", "  Implement the governed launcher  ")
        .expect("mandatory controls satisfy the Builder role");

    assert_eq!(
        request.task.as_deref(),
        Some("Implement the governed launcher")
    );
    assert_eq!(
        request.role_assignment,
        Some(builder_role_assignment().expect("static Builder role profile"))
    );
    assert_eq!(request.role, None);
    assert_eq!(request.acceptance_criteria, ["workspace tests pass"]);
    assert_eq!(
        request.verification_profile,
        Some(impulse_ops::governed_task::GovernedVerificationProfile::RustWorkspaceV1)
    );
}

#[derive(Clone, Default)]
struct FakeDocument {
    state: Rc<RefCell<FakeDocumentState>>,
}

#[derive(Default)]
struct FakeDocumentState {
    scripts: Vec<String>,
    bridge_messages: VecDeque<serde_json::Value>,
    eval_owners: Vec<generational_box::Owner>,
}

impl FakeDocument {
    fn with_bridge_messages(messages: Vec<DesktopBridgeMessage>) -> Self {
        Self {
            state: Rc::new(RefCell::new(FakeDocumentState {
                scripts: Vec::new(),
                bridge_messages: messages
                    .into_iter()
                    .map(|message| serde_json::to_value(message).expect("serialize bridge message"))
                    .collect(),
                eval_owners: Vec::new(),
            })),
        }
    }

    fn scripts(&self) -> Vec<String> {
        self.state.borrow().scripts.clone()
    }
}

impl Document for FakeDocument {
    fn eval(&self, js: String) -> Eval {
        self.state.borrow_mut().scripts.push(js.clone());
        let messages = if js.contains("__impulseOpsBridge") {
            std::mem::take(&mut self.state.borrow_mut().bridge_messages)
        } else {
            VecDeque::new()
        };
        let owner = generational_box::Owner::default();
        let evaluator = owner.insert(Box::new(FakeEvaluator { messages }) as Box<dyn Evaluator>);
        self.state.borrow_mut().eval_owners.push(owner);
        Eval::new(evaluator)
    }

    fn create_head_component(&self) -> bool {
        false
    }
}

struct FakeEvaluator {
    messages: VecDeque<serde_json::Value>,
}

impl Evaluator for FakeEvaluator {
    fn send(&self, _data: serde_json::Value) -> Result<(), EvalError> {
        Ok(())
    }

    fn poll_recv(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<Result<serde_json::Value, EvalError>> {
        match self.messages.pop_front() {
            Some(message) => Poll::Ready(Ok(message)),
            None => Poll::Ready(Err(EvalError::Finished)),
        }
    }

    fn poll_join(
        &mut self,
        _context: &mut Context<'_>,
    ) -> Poll<Result<serde_json::Value, EvalError>> {
        Poll::Ready(Err(EvalError::Finished))
    }
}

fn runtime_snapshot(agent_id: &str) -> AgentRuntimeSnapshot {
    AgentRuntimeSnapshot {
        agent_id: agent_id.to_string(),
        label: "Codex Live".to_string(),
        platform: platform_id("codex"),
        command: "codex".to_string(),
        args: Vec::new(),
        cwd: Some("<repo>".to_string()),
        workspace: Some(WorkspaceTarget {
            root: "<repo>".to_string(),
            label: Some("IMPULSE-rs".to_string()),
            purpose: Some("terminal harness".to_string()),
            project_notes: Some("watch Dioxus bridge".to_string()),
        }),
        session_id: Some(format!("{agent_id}-session")),
        governed_task_id: None,
        governed_task_revision: None,
        rows: 32,
        cols: 100,
        alive: true,
        focused: true,
        status: AgentStatus::Working {
            task: "wire live bridge".to_string(),
        },
        current_task: Some("wire live bridge".to_string()),
        role: None,
        role_assignment: None,
        role_compatibility: None,
        target: None,
        mcp_tools: vec![BuiltInMcpTool::new(
            "impulse.agent_spawn",
            "spawn a coding agent",
            vec!["terminal".to_string()],
            true,
        )],
        output_bytes: 512,
        output_lines: 12,
        context: ContextHealthSummary::default(),
    }
}

#[test]
fn test_dioxus_shell_renders_five_panel_layout_without_egui() {
    let mut vdom = VirtualDom::new(DesktopShell);
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("top-bar"));
    assert!(html.contains("left-rail"));
    assert!(html.contains("terminal-stage"));
    assert!(html.contains("view-rail"));
    assert!(html.contains("data-view=\"terminal\""));
    assert!(html.contains("data-view=\"memory\""));
    assert!(html.contains("data-view=\"review\""));
    assert!(html.contains("data-view=\"artifacts\""));
    assert!(html.contains("data-view=\"supervisor\""));
    assert!(html.contains("stage-view view-terminal active"));
    assert!(html.contains("right-inspector"));
    assert!(html.contains("event-strip"));
    assert!(html.contains("crt-hero"));
    assert!(html.contains("brand-wordmark"));
    assert!(html.contains("stat-row"));
    assert!(html.contains("your ai remembers"));
    assert!(html.contains("agent-pool"));
    assert!(html.contains("workspace-picker"));
    assert!(html.contains("data-source=\"workspace_target\""));
    assert!(html.contains("data-source=\"builtin_mcp_tools\""));
    assert!(html.contains("Rust MCP Tools"));
    assert!(html.contains("agent_spawn and agent_write require confirmation"));
    assert!(html.contains("Workspace Launcher"));
    assert!(html.contains("Register folder"));
    assert!(html.contains("Launch agent"));
    assert!(html.contains("MCP audited"));
    assert!(html.contains("class=\"terminal-empty-state\""));
    assert!(html.contains("data-terminal-state=\"empty\""));
    assert!(html.contains("Launch an agent from the workspace panel"));
    assert!(!html.contains("data-xterm-mount=\"true\""));
    assert!(!html.contains("terminal-pane-codex"));
    // Footer stream health is derived, not hardcoded: with no agents and a
    // healthy transport the runtime/terminal streams read `idle`.
    assert!(!html.contains("stream pending"));
    assert!(html.contains("data-stream=\"agent_runtime_update\""));
    assert!(html.contains("agent_runtime_update · idle"));
    assert!(html.contains("supervisor_local_action · ready"));
    assert!(!html.contains("data-pty-owner=\"rust-backend\""));
    assert!(!html.contains("<section class=\"review-console\""));
    assert!(!html.contains("<section class=\"operator-board\""));
    assert!(!html.contains("egui"));
}

#[test]
fn test_dioxus_shell_is_offline_packaged_for_fonts() {
    let mut vdom = VirtualDom::new(DesktopShell);
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("impulse-shell"));
    assert!(html.contains("ui-monospace"));
    assert!(html.contains("ui-rounded"));
    for forbidden in [
        "fonts.googleapis",
        "fonts.gstatic",
        "https://",
        "http://",
        "//fonts.",
    ] {
        assert!(
            !html.contains(forbidden),
            "shell SSR must not depend on remote font asset `{forbidden}`"
        );
    }
}

#[test]
fn test_dioxus_shell_declares_local_xterm_assets() {
    let mut vdom = VirtualDom::new(DesktopShell);
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("data-impulse-terminal-asset=\"xterm-css\""));
    assert!(html.contains("data-impulse-terminal-asset=\"xterm-js\""));
    assert!(html.contains("data-impulse-terminal-asset=\"xterm-fit-addon\""));
    assert!(html.contains(&format!("href=\"{XTERM_CSS_PATH}\"")));
    assert!(html.contains(&format!("src=\"{XTERM_JS_PATH}\"")));
    assert!(html.contains(&format!("src=\"{XTERM_FIT_JS_PATH}\"")));
    for path in terminal_asset_paths() {
        assert!(path.starts_with("assets/vendor/xterm/"));
        assert!(!path.contains("://"));
    }
}

#[test]
fn test_xterm_vendor_assets_are_present_and_manifested() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/vendor/xterm");
    let manifest_path = manifest_dir.join("manifest.json");
    let manifest_text =
        std::fs::read_to_string(&manifest_path).expect("xterm asset manifest must exist");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("xterm asset manifest must be json");

    assert_eq!(manifest["packages"]["@xterm/xterm"], "6.0.0");
    assert_eq!(manifest["packages"]["@xterm/addon-fit"], "0.11.0");
    assert_eq!(manifest["globals"]["terminal"], "window.Terminal");
    assert_eq!(manifest["globals"]["fitAddon"], "window.FitAddon.FitAddon");

    for path in terminal_asset_paths() {
        let asset_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let content = std::fs::read_to_string(&asset_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", asset_path.display()));
        for forbidden in ["url(http://", "url(https://", "src=\"http", "href=\"http"] {
            assert!(
                !content.contains(forbidden),
                "vendored xterm asset {} must not require network-loaded URL pattern {forbidden}",
                asset_path.display()
            );
        }
    }

    let xterm_js = std::fs::read_to_string(manifest_dir.join("xterm.js")).expect("xterm.js");
    let fit_js = std::fs::read_to_string(manifest_dir.join("addon-fit.js")).expect("addon-fit.js");
    assert!(xterm_js.contains("Terminal"));
    assert!(fit_js.contains("FitAddon"));
}

#[test]
fn test_host_readiness_smoke_script_is_declared() {
    let package_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("package.json");
    let package_text =
        std::fs::read_to_string(&package_path).expect("impulse-desktop package.json exists");
    let package: serde_json::Value =
        serde_json::from_str(&package_text).expect("package.json is valid json");

    assert_eq!(
        package["scripts"]["host:smoke"],
        "npm run dioxus:host:smoke"
    );
    assert_eq!(
        package["scripts"]["dioxus:host:smoke"],
        "npm run vendor:xterm && node scripts/host_readiness_smoke.mjs ../../output/playwright/impulse-desktop-dioxus-host-smoke dioxus"
    );
    assert_eq!(
        package["scripts"]["legacy:host:smoke"],
        "npm run vendor:xterm && node scripts/host_readiness_smoke.mjs ../../output/playwright/impulse-desktop-legacy-host-smoke legacy-tauri"
    );
    assert!(package_text.contains("@xterm/xterm"));
    assert!(package_text.contains("@xterm/addon-fit"));
}

#[test]
fn test_dioxus_desktop_launch_binary_is_feature_gated() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest_text =
        std::fs::read_to_string(&manifest_path).expect("impulse-desktop Cargo.toml exists");
    let launcher_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/impulse_desktop.rs");
    let launcher_text =
        std::fs::read_to_string(&launcher_path).expect("Dioxus desktop launcher exists");

    assert!(manifest_text.contains("name = \"impulse-desktop\""));
    assert!(manifest_text.contains("required-features = [\"desktop-app\"]"));
    assert!(manifest_text.contains("desktop-app = [\"dep:dioxus-desktop\", \"dioxus/desktop\"]"));
    assert!(manifest_text.contains("dioxus-desktop = { version = \"0.6.3\", optional = true }"));
    assert!(launcher_text.contains("use impulse_desktop::desktop_host::desktop_config;"));
    assert!(launcher_text.contains("dioxus::LaunchBuilder::desktop()"));
    assert!(launcher_text.contains(".with_cfg(desktop_config())"));
    // The launcher now assembles the live host context and launches the
    // bridge-mounting root component instead of the bare shell.
    assert!(launcher_text.contains("install_live_host_context(LiveHostContext::new("));
    assert!(launcher_text.contains(".launch(LiveDesktopApp);"));
}

#[test]
fn test_terminal_interop_prefers_dioxus_native_host_adapter() {
    let script = impulse_desktop::ui::terminal_interop_script();

    assert!(script.contains("resolveImpulseHostAdapter"));
    assert!(script.contains("window.__IMPULSE_DESKTOP_HOST"));
    assert!(script.contains("const legacyTauri = window.__TAURI__"));
    assert!(
        script.contains("const { invoke, listen, hostKind } = await resolveImpulseHostAdapter();")
    );
    assert!(script.contains(r#"hostKind: dioxusHost ? "dioxus""#));
    assert!(script.contains(r#"legacyTauri ? "legacy-tauri""#));
    assert!(script.contains("data-impulse-host-kind"));

    // The resolver must treat the manifest-only bootstrap stubs as unavailable
    // rather than advertising them as a live host. It keys off both the pending
    // status sentinel and the `__impulseHostPending` flag the bootstrap stamps
    // onto its rejecting stubs.
    assert!(script.contains("impulseHostFnReady"));
    assert!(script.contains("__impulseHostPending"));
    assert!(script.contains("legacyTauri?.core?.invoke"));
    assert!(script.contains("legacyTauri?.event?.listen"));
    assert!(script.contains(impulse_desktop::host_commands::PENDING_HOST_BOOTSTRAP_STATUS));
}

/// The manifest-only Dioxus bootstrap installs `invoke`/`listen` that always
/// reject. Without a real eval bridge or a legacy Tauri host, the ops bridge
/// must degrade — never advertise itself as mounted over the rejecting stubs.
#[test]
fn test_pending_dioxus_host_ops_bridge_fails_closed() {
    if skip_without_node() {
        return;
    }

    let smoke = run_pending_host_bridge_smoke(/* with_legacy = */ false);
    assert_eq!(
        smoke["attrs"]["data-impulse-host-kind"],
        serde_json::Value::String("dioxus".to_string()),
        "host-kind should still report the present dioxus host object"
    );
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge"],
        serde_json::Value::String("degraded".to_string()),
        "pending stub host must not advertise a mounted bridge"
    );
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge-reason"],
        serde_json::Value::String("host event API unavailable".to_string())
    );
    assert_eq!(smoke["bridge"]["degraded"], serde_json::Value::Bool(true));
    assert_eq!(
        smoke["invoked"].as_array().map(|calls| calls.len()),
        Some(0),
        "pending stub host must not be invoked"
    );
}

/// When a legacy Tauri host is present alongside the pending Dioxus stubs, the
/// resolver must fall back to the working legacy transport and mount the bridge
/// rather than degrade on the rejecting stubs.
#[test]
fn test_pending_dioxus_host_falls_back_to_legacy_tauri() {
    if skip_without_node() {
        return;
    }

    let smoke = run_pending_host_bridge_smoke(/* with_legacy = */ true);
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge"],
        serde_json::Value::String("mounted".to_string()),
        "legacy fallback should mount the bridge"
    );
    assert_eq!(smoke["bridge"]["degraded"], serde_json::Value::Bool(false));
    let invoked = smoke["invoked"].as_array().expect("invoked array");
    assert!(
        invoked
            .iter()
            .any(|call| call["command"] == "agent_snapshot"),
        "legacy transport should receive bridge refresh invokes, got {invoked:?}"
    );
}

fn skip_without_node() -> bool {
    match Command::new("node").arg("--version").output() {
        Ok(output) if output.status.success() => false,
        _ => {
            eprintln!("node is unavailable; skipping pending-host bridge smoke");
            true
        }
    }
}

/// Drive the ops bridge script against a mocked webview whose
/// `window.__IMPULSE_DESKTOP_HOST` mirrors the real manifest-only bootstrap:
/// `invoke`/`listen` are present but flagged `__impulseHostPending` and reject.
/// Optionally also install a working legacy Tauri host to exercise fallback.
fn run_pending_host_bridge_smoke(with_legacy: bool) -> serde_json::Value {
    let legacy_setup = if with_legacy {
        r#"
window.__TAURI__ = {
  core: {
    invoke: async (command) => {
      invoked.push({ command });
      if (command === "agent_snapshot") return [];
      if (command === "list_workspaces") return [];
      if (command === "mcp_descriptors") return [];
      if (command === "review_queue") return [];
      return null;
    }
  },
  event: {
    listen: async (name, handler) => { listeners[name] = handler; return async () => {}; }
  }
};
"#
    } else {
        ""
    };

    let smoke_script = format!(
        r#"
const bridgeScript = {bridge_script};
const pendingStatus = {pending_status};
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
const pendingInvoke = (command) => {{ invoked.push({{ command, pending: true }}); return pending(`invoke:${{command}}`); }};
const pendingListen = (event) => pending(`listen:${{event}}`);
pendingInvoke.__impulseHostPending = true;
pendingListen.__impulseHostPending = true;
window.__IMPULSE_DESKTOP_HOST = {{
  invoke: pendingInvoke,
  listen: pendingListen,
  hostKind: "dioxus",
  status: pendingStatus,
}};
{legacy_setup}

const bridgePromise = eval(bridgeScript);
if (bridgePromise && typeof bridgePromise.catch === "function") {{
  bridgePromise.catch(() => {{}});
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
        legacy_setup = legacy_setup,
    );

    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir.path().join("pending-host-bridge-smoke.js");
    std::fs::write(&smoke_path, smoke_script).expect("write pending-host smoke script");
    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run node pending-host bridge smoke");
    assert!(
        output.status.success(),
        "pending-host bridge smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse pending-host bridge smoke output")
}

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
        live_status =
            serde_json::to_string("dioxus-eval-bridge-ready").expect("serialize live status"),
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
        invoked
            .iter()
            .any(|call| call["command"] == "agent_snapshot"),
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

/// Both host-adapter-resolution consumers share one macro; this pins the
/// bounded-retry contract in place so a future edit can't silently drop the
/// polling loop from one script while keeping it in the other.
#[test]
fn test_impulse_host_adapter_resolution_script_declares_bounded_retry_contract() {
    for script in [
        desktop_event_bridge_script(),
        impulse_desktop::ui::terminal_interop_script(),
    ] {
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

#[test]
fn test_retro_shell_binds_project_ops_snapshot() {
    let mut snapshot = ProjectOpsSnapshot {
        generated_at: "2026-06-13T12:00:00Z".to_string(),
        context: ContextHealthSummary {
            tier: "operator".to_string(),
            usage_fraction: 0.236,
            estimated_tokens: 47_238,
            window_tokens: 200_000,
            compaction_count: 2,
            injection_count: 7,
            pending_review_count: 1,
            ..Default::default()
        },
        memory: MemorySummary {
            genome_decisions: 12,
            ..Default::default()
        },
        retrieval: RetrievalSummary {
            backend: "sqlite-vector".to_string(),
            mode: "hybrid".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    snapshot.agents.push(AgentRuntime {
        id: "codex".to_string(),
        label: "Codex".to_string(),
        active: true,
        agent_status: AgentStatus::Working {
            task: "integrate design".to_string(),
        },
        ..Default::default()
    });

    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("online · watching"));
    assert!(html.contains("Context · operator"));
    assert!(html.contains("47.2k"));
    assert!(html.contains("200.0k"));
    assert!(html.contains("tokens · 24% of 200.0k"));
    assert!(html.contains("1 injection(s) awaiting review"));
    assert!(html.contains("sqlite-vector"));
    assert!(html.contains("12 genome decisions"));
    assert!(html.contains("ops_update 2026-06-13T12:00:00Z"));
}

#[test]
fn test_workspace_launcher_renders_registry_platforms_including_ion_and_custom() {
    let agent_platforms: Vec<AgentPlatformInfo> = serde_json::from_value(json!([
        {"id": "ion", "label": "Ion", "command": "ion"},
        {"id": "custom-agent", "label": "Custom Agent", "command": "custom-agent"}
    ]))
    .expect("platform fixtures");
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: Vec::new(),
            agent_platforms,
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);
    assert!(html.contains("Ion"), "{html}");
    assert!(html.contains("Custom Agent"), "{html}");
    assert!(html.contains("value=\"ion\""), "{html}");
    assert!(html.contains("value=\"custom-agent\""), "{html}");
}

#[test]
fn test_desktop_event_bridge_script_subscribes_to_live_host_events() {
    let script = desktop_event_bridge_script();

    assert!(script.contains("resolveImpulseHostAdapter"));
    assert!(script.contains("dioxus.send"));
    assert!(script.contains(r#"listen("ops_update""#));
    assert!(script.contains(r#"listen("agent_runtime_update""#));
    assert!(script.contains(r#"invoke("agent_snapshot")"#));
    assert!(script.contains(r#"invoke("list_workspaces")"#));
    assert!(script.contains(r#"invoke("mcp_descriptors")"#));
    assert!(script.contains(r#"invoke("register_workspace", { request })"#));
    assert!(script.contains(r#"invoke("mcp_invoke", { request })"#));
    assert!(script.contains(r#"invoke("agent_focus", { request: { session_id: agentId } })"#));
    assert!(script.contains(r#"invoke("review_queue")"#));
    assert!(script.contains(r#"invoke("review_decision", { request: commandRequest })"#));
    assert!(script.contains(r#"forward("mcp_invocation", { invocation })"#));
    assert!(script.contains(r#"forward("workspaces", { workspaces })"#));
    assert!(script.contains(r#"forward("mcp_descriptors", { tools })"#));
    assert!(script.contains("confirmed: true"));
    assert!(script.contains("refreshReviewQueue"));
    assert!(script.contains("refreshWorkspaces"));
    assert!(script.contains("const markEventBridgeDegraded = (reason) =>"));
    assert!(script.contains(r#"data-impulse-ops-bridge-reason"#));
    assert!(script.contains(r#"markEventBridgeDegraded("host event API unavailable")"#));
    assert!(script.contains("unlisten"));
}

#[test]
fn test_desktop_event_bridge_degraded_state_is_explicit() {
    let Ok(node_version) = Command::new("node").arg("--version").output() else {
        eprintln!("node is unavailable; skipping degraded JS bridge smoke");
        return;
    };
    if !node_version.status.success() {
        eprintln!("node is unavailable; skipping degraded JS bridge smoke");
        return;
    }

    let smoke_script = format!(
        r#"
const bridgeScript = {bridge_script};
const sent = [];
const attrs = {{}};

global.window = {{
  __IMPULSE_DESKTOP_HOST: {{
    invoke: async () => []
  }}
}};
global.document = {{
  documentElement: {{
    setAttribute: (key, value) => {{ attrs[key] = value; }}
  }}
}};
global.dioxus = {{
  send: (message) => sent.push(message)
}};

eval(bridgeScript);
setTimeout(() => {{
  console.log(JSON.stringify({{ attrs, sent, bridge: window.__impulseOpsBridge }}));
  process.exit(0);
}}, 25);
"#,
        bridge_script =
            serde_json::to_string(desktop_event_bridge_script()).expect("serialize bridge script"),
    );
    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir
        .path()
        .join("desktop-event-bridge-degraded-smoke.js");
    std::fs::write(&smoke_path, smoke_script).expect("write degraded smoke script");
    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run node degraded bridge smoke");

    assert!(
        output.status.success(),
        "degraded bridge smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let smoke: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse degraded bridge smoke output");
    assert_eq!(
        smoke["attrs"]["data-impulse-host-kind"],
        serde_json::Value::String("dioxus".to_string())
    );
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge"],
        serde_json::Value::String("degraded".to_string())
    );
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge-reason"],
        serde_json::Value::String("host event API unavailable".to_string())
    );
    assert_eq!(smoke["bridge"]["mounted"], serde_json::Value::Bool(true));
    assert_eq!(smoke["bridge"]["degraded"], serde_json::Value::Bool(true));
    let sent = smoke["sent"].as_array().expect("sent messages");
    assert!(
        sent.iter().any(|message| {
            message["kind"] == "bridge_status"
                && message["payload"]["status"] == "degraded"
                && message["payload"]["reason"] == "host event API unavailable"
        }),
        "expected degraded bridge_status message, got {sent:?}"
    );
}

#[test]
fn test_desktop_event_bridge_script_executes_against_mocked_legacy_host_webview() {
    let Ok(node_version) = Command::new("node").arg("--version").output() else {
        eprintln!("node is unavailable; skipping JS bridge smoke");
        return;
    };
    if !node_version.status.success() {
        eprintln!("node is unavailable; skipping JS bridge smoke");
        return;
    }

    let ops_snapshot = ProjectOpsSnapshot {
        generated_at: "2026-06-13T23:59:00Z".to_string(),
        retrieval: RetrievalSummary {
            backend: "sqlite-vector".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let runtime = runtime_snapshot("codex-live");
    let agent_platforms = vec![AgentPlatformInfo {
        id: platform_id("ion"),
        label: "Ion".to_string(),
        command: "ion".to_string(),
        runtime_capabilities: Vec::new(),
    }];
    let workspace = WorkspaceEntry::new(WorkspaceTarget {
        root: "/tmp".to_string(),
        label: Some("scratch".to_string()),
        purpose: Some("bridge smoke".to_string()),
        project_notes: Some("mocked webview context".to_string()),
    });
    let mcp_tool = BuiltInMcpTool::new(
        "impulse.agent_spawn",
        "spawn a coding agent",
        vec!["terminal".to_string(), "workspace".to_string()],
        true,
    );
    let review_item = ReviewQueueItem {
        id: "review-1".to_string(),
        staged_at_unix_ms: 1,
        status: ReviewQueueStatus::Pending,
        decided_at_unix_ms: None,
        decision: None,
        target_agent_id: Some("codex-live".to_string()),
        arguments: json!({ "content": "cargo test\n" }),
        path: "/tmp/review-1.json".to_string(),
        preview: "cargo test\\n".to_string(),
    };
    let spawn_invocation = McpInvocation {
        call_id: "call-spawn".to_string(),
        tool: "impulse.agent_spawn".to_string(),
        caller_agent_id: Some("impulse-ui".to_string()),
        arguments: json!({ "agent_id": "codex-live" }),
        confirmed: true,
        result: serde_json::to_value(&runtime).expect("serialize runtime"),
        ok: true,
    };
    let review_invocation = McpInvocation {
        call_id: "call-review".to_string(),
        tool: "impulse.review_decision".to_string(),
        caller_agent_id: Some("impulse-ui".to_string()),
        arguments: json!({ "id": "review-1", "decision": "skip" }),
        confirmed: true,
        result: json!({ "ok": true }),
        ok: true,
    };

    let smoke_script = format!(
        r#"
const bridgeScript = {bridge_script};
const opsSnapshot = {ops_snapshot};
const runtimeSnapshot = {runtime_snapshot};
const agentPlatforms = {agent_platforms};
const workspace = {workspace};
const mcpTool = {mcp_tool};
const reviewItem = {review_item};
const spawnInvocation = {spawn_invocation};
const reviewInvocation = {review_invocation};
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
global.dioxus = {{
  send: (message) => sent.push(message)
}};
window.__TAURI__ = {{
  core: {{
    invoke: async (command, args) => {{
      invoked.push({{ command, args }});
      if (command === "agent_snapshot") return [runtimeSnapshot];
      if (command === "agent_platforms") return agentPlatforms;
      if (command === "list_workspaces") return [workspace];
      if (command === "mcp_descriptors") return [mcpTool];
      if (command === "review_queue") return [reviewItem];
      if (command === "mcp_invoke") return spawnInvocation;
      if (command === "review_decision") return reviewInvocation;
      throw new Error(`unexpected invoke ${{command}}`);
    }}
  }},
  event: {{
    listen: async (name, handler) => {{
      listeners[name] = handler;
      return async () => {{}};
    }}
  }}
}};

const bridgePromise = eval(bridgeScript);
if (bridgePromise && typeof bridgePromise.catch === "function") {{
  bridgePromise.catch((error) => {{
    console.error(error && error.stack ? error.stack : String(error));
    process.exit(1);
  }});
}}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
(async () => {{
  await delay(25);
  if (!listeners.ops_update || !listeners.agent_runtime_update) {{
    throw new Error("bridge did not subscribe to expected host events");
  }}
  listeners.ops_update({{ payload: opsSnapshot }});
  listeners.agent_runtime_update({{ payload: runtimeSnapshot }});
  await window.__impulseOpsBridge.invokeMcp({{
    tool: "impulse.agent_spawn",
    arguments: runtimeSnapshot,
    confirmed: true,
    caller_agent_id: "impulse-ui"
  }});
  await window.__impulseOpsBridge.reviewDecision({{
    id: "review-1",
    decision: "skip",
    target_agent_id: null
  }});
  console.log(JSON.stringify({{ attrs, invoked, sent }}));
  process.exit(0);
}})().catch((error) => {{
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
}});
"#,
        bridge_script =
            serde_json::to_string(desktop_event_bridge_script()).expect("serialize bridge script"),
        ops_snapshot = serde_json::to_string(&ops_snapshot).expect("serialize ops snapshot"),
        runtime_snapshot = serde_json::to_string(&runtime).expect("serialize runtime snapshot"),
        agent_platforms =
            serde_json::to_string(&agent_platforms).expect("serialize agent platforms"),
        workspace = serde_json::to_string(&workspace).expect("serialize workspace"),
        mcp_tool = serde_json::to_string(&mcp_tool).expect("serialize mcp tool"),
        review_item = serde_json::to_string(&review_item).expect("serialize review item"),
        spawn_invocation =
            serde_json::to_string(&spawn_invocation).expect("serialize spawn invocation"),
        review_invocation =
            serde_json::to_string(&review_invocation).expect("serialize review invocation"),
    );
    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir.path().join("desktop-event-bridge-smoke.js");
    std::fs::write(&smoke_path, smoke_script).expect("write smoke script");
    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run node bridge smoke");

    assert!(
        output.status.success(),
        "bridge smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let smoke: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse bridge smoke output");
    assert_eq!(
        smoke["attrs"]["data-impulse-ops-bridge"],
        serde_json::Value::String("mounted".to_string())
    );
    let invoked = smoke["invoked"]
        .as_array()
        .expect("invoked commands should be array");
    let invoked_commands = invoked
        .iter()
        .filter_map(|item| item["command"].as_str())
        .collect::<Vec<_>>();
    for expected in [
        "agent_snapshot",
        "agent_platforms",
        "list_workspaces",
        "mcp_descriptors",
        "review_queue",
        "mcp_invoke",
        "review_decision",
    ] {
        assert!(
            invoked_commands.contains(&expected),
            "expected mocked bridge to invoke {expected}; got {invoked_commands:?}"
        );
    }
    let review_call = invoked
        .iter()
        .find(|item| item["command"] == "review_decision")
        .expect("review_decision invocation");
    assert_eq!(review_call["args"]["request"]["confirmed"], true);

    let messages = smoke["sent"]
        .as_array()
        .expect("sent messages should be array");
    let sent_kinds = messages
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect::<Vec<_>>();
    for expected in [
        "agent_snapshot",
        "agent_platforms",
        "workspaces",
        "mcp_descriptors",
        "review_queue",
        "ops_update",
        "agent_runtime_update",
        "mcp_invocation",
    ] {
        assert!(
            sent_kinds.contains(&expected),
            "expected mocked bridge to send {expected}; got {sent_kinds:?}"
        );
    }

    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut agent_platforms = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();
    for message in messages {
        let message = serde_json::from_value::<DesktopBridgeMessage>(message.clone())
            .expect("smoke message should match DesktopBridgeMessage");
        apply_desktop_bridge_message(
            DesktopBridgeStateMut::new(
                &mut snapshot,
                &mut runtime_agents,
                &mut agent_platforms,
                &mut workspaces,
                &mut mcp_tools,
                &mut review_queue,
                &mut last_invocations,
            ),
            message,
        )
        .expect("smoke message should reduce into desktop state");
    }

    assert_eq!(snapshot.generated_at, "2026-06-13T23:59:00Z");
    assert_eq!(runtime_agents.len(), 1);
    assert_eq!(runtime_agents[0].agent_id, "codex-live");
    assert_eq!(agent_platforms.len(), 1);
    assert_eq!(agent_platforms[0].id, "ion");
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].target.root, "/tmp");
    assert_eq!(mcp_tools.len(), 1);
    assert_eq!(mcp_tools[0].name, "impulse.agent_spawn");
    assert_eq!(review_queue.len(), 1);
    assert_eq!(review_queue[0].id, "review-1");
    assert_eq!(last_invocations.len(), 2);
    assert_eq!(last_invocations[0].tool, "impulse.agent_spawn");
    assert_eq!(last_invocations[1].tool, "impulse.review_decision");
}

#[tokio::test]
async fn test_live_desktop_shell_consumes_eval_bridge_messages() {
    let runtime = runtime_snapshot("codex-live");
    let workspace = WorkspaceEntry::new(WorkspaceTarget {
        root: "<repo>".to_string(),
        label: Some("IMPULSE-rs".to_string()),
        purpose: Some("terminal harness".to_string()),
        project_notes: Some("review project notes before injection".to_string()),
    });
    let mcp_tool = BuiltInMcpTool::new(
        "impulse.project_context",
        "read workspace context",
        vec!["workspace".to_string(), "read_only".to_string()],
        false,
    );
    let review_item = ReviewQueueItem {
        id: "review-1".to_string(),
        staged_at_unix_ms: 1,
        status: ReviewQueueStatus::Pending,
        decided_at_unix_ms: None,
        decision: None,
        target_agent_id: Some("codex-live".to_string()),
        arguments: json!({ "content": "cargo test\n" }),
        path: "/tmp/review-1.json".to_string(),
        preview: "cargo test\\n".to_string(),
    };
    let invocation = McpInvocation {
        call_id: "call-1".to_string(),
        tool: "impulse.project_context".to_string(),
        caller_agent_id: Some("codex-live".to_string()),
        arguments: json!({ "root": "<repo>" }),
        confirmed: true,
        result: json!({ "ok": true }),
        ok: true,
    };
    let fake_document = FakeDocument::with_bridge_messages(vec![
        DesktopBridgeMessage {
            kind: "agent_snapshot".to_string(),
            payload: json!({ "agents": [runtime] }),
        },
        DesktopBridgeMessage {
            kind: "workspaces".to_string(),
            payload: json!({ "workspaces": [workspace] }),
        },
        DesktopBridgeMessage {
            kind: "mcp_descriptors".to_string(),
            payload: json!({ "tools": [mcp_tool] }),
        },
        DesktopBridgeMessage {
            kind: "review_queue".to_string(),
            payload: json!({ "items": [review_item] }),
        },
        DesktopBridgeMessage {
            kind: "mcp_invocation".to_string(),
            payload: json!({ "invocation": invocation }),
        },
    ]);
    let document_context: Rc<dyn Document> = Rc::new(fake_document.clone());
    let mut vdom = VirtualDom::new(DesktopShell);
    vdom.provide_root_context(document_context);
    vdom.rebuild_in_place();
    let _ = vdom.render_immediate_to_vec();

    for _ in 0..6 {
        if tokio::time::timeout(Duration::from_millis(100), vdom.wait_for_work())
            .await
            .is_err()
        {
            break;
        }
        let _ = vdom.render_immediate_to_vec();
    }
    let html = dioxus_ssr::render(&vdom);
    let scripts = fake_document.scripts();

    assert!(
        scripts
            .iter()
            .any(|script| script.contains("__impulseOpsBridge")),
        "DesktopShell should evaluate the host event bridge"
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("__impulseTerminalInterop")),
        "DesktopShell should evaluate the terminal interop bridge"
    );
    assert!(html.contains("Codex Live"));
    assert!(html.contains("data-agent-id=\"codex-live\""));
    assert!(html.contains("terminal-pane-codex-live"));
    assert!(html.contains("data-xterm-mount=\"true\""));
    assert!(!html.contains("data-terminal-state=\"empty\""));
    assert!(html.contains("IMPULSE-rs"));
    assert!(html.contains("workspace-notes"));
    assert!(html.contains("impulse.project_context"));
    assert!(html.contains("1 injection(s) awaiting review"));
    assert!(html.contains("1 bundle(s) awaiting review-first apply"));
    assert!(!html.contains("Review Queue"));
    assert!(!html.contains("<section class=\"review-console\""));
    assert!(!html.contains("data-source=\"operator_board\""));
    assert!(html.contains("1 invocations"));
}

#[test]
fn test_review_decision_bridge_script_serializes_apply_request() {
    let script = review_decision_bridge_script(&ReviewDecisionUiRequest {
        id: "review-1".to_string(),
        decision: ReviewDecision::Apply,
        target_agent_id: Some("codex-live".to_string()),
    });

    assert!(script.contains("window.__impulseOpsBridge"));
    assert!(script.contains("reviewDecision"));
    assert!(script.contains(r#""id":"review-1""#));
    assert!(script.contains(r#""decision":"apply""#));
    assert!(script.contains(r#""target_agent_id":"codex-live""#));
}

#[test]
fn test_review_decision_bridge_script_serializes_skip_request() {
    let script = review_decision_bridge_script(&ReviewDecisionUiRequest {
        id: "review-2".to_string(),
        decision: ReviewDecision::Skip,
        target_agent_id: None,
    });

    assert!(script.contains("reviewDecision"));
    assert!(script.contains(r#""id":"review-2""#));
    assert!(script.contains(r#""decision":"skip""#));
    assert!(script.contains(r#""target_agent_id":null"#));
}

#[test]
fn test_workspace_registration_bridge_script_serializes_project_notes() {
    let script = workspace_registration_bridge_script(&RegisterWorkspaceRequest {
        root: "/tmp".to_string(),
        label: Some("scratch".to_string()),
        purpose: Some("terminal harness".to_string()),
        project_notes: Some("watch Dioxus bridge".to_string()),
    });

    assert!(script.contains("registerWorkspace"));
    assert!(script.contains(r#""root":"/tmp""#));
    assert!(script.contains(r#""label":"scratch""#));
    assert!(script.contains(r#""purpose":"terminal harness""#));
    assert!(script.contains(r#""project_notes":"watch Dioxus bridge""#));
}

#[test]
fn test_agent_launch_bridge_script_routes_through_audited_mcp_spawn() {
    let role_assignment = builder_role_assignment().expect("static Builder role profile");
    let script = agent_launch_bridge_script(&AgentSpawnRequest {
        agent_id: Some("codex-live".to_string()),
        session_id: Some("codex-live-session".to_string()),
        platform: platform_id("codex"),
        command: None,
        args: Vec::new(),
        cwd: Some("/tmp".to_string()),
        env: HashMap::new(),
        workspace: Some(WorkspaceTarget {
            root: "/tmp".to_string(),
            label: Some("scratch".to_string()),
            purpose: Some("terminal harness".to_string()),
            project_notes: Some("registered workspace context".to_string()),
        }),
        mcp_tools: default_builtin_mcp_tools(),
        rows: 32,
        cols: 100,
        role: None,
        task: Some("Implement the governed launcher".to_string()),
        role_assignment: Some(role_assignment),
        acceptance_criteria: vec!["workspace tests pass".to_string()],
        verification_profile: Some(
            impulse_ops::governed_task::GovernedVerificationProfile::RustWorkspaceV1,
        ),
        target: None,
    });

    assert!(script.contains("invokeMcp"));
    assert!(script.contains(r#""tool":"impulse.agent_spawn""#));
    assert!(script.contains(r#""confirmed":true"#));
    assert!(script.contains(r#""caller_agent_id":"impulse-ui""#));
    assert!(script.contains(r#""platform":"codex""#));
    assert!(script.contains(r#""workspace""#));
    assert!(script.contains(r#""root":"/tmp""#));
    assert!(script.contains(r#""project_notes":"registered workspace context""#));
    assert!(script.contains(r#""task":"Implement the governed launcher""#));
    assert!(script.contains(r#""role":"builder""#));
    assert!(script.contains(r#""capability":"workspace.target""#));
    assert!(script.contains(r#""capability":"process.lifecycle""#));
    assert!(script.contains(r#""capability":"filesystem.scoped""#));
}

#[test]
fn test_host_manifest_exposes_registry_platform_catalog() {
    assert!(
        impulse_desktop::host_commands::HOST_INVOKE_COMMANDS.contains(&"agent_platforms"),
        "the launcher must load platform choices from the runtime registry"
    );
}

#[test]
fn test_host_manifest_and_bridge_expose_acknowledged_governed_task_mutation() {
    use impulse_ops::governed_task::{
        GovernedActor, GovernedActorKind, GovernedRequestId, GovernedTaskId, GovernedTaskMutation,
        GovernedTaskMutationRequest, SupervisorVerdictInput, SupervisorVerdictKind,
    };

    assert!(
        impulse_desktop::host_commands::HOST_INVOKE_COMMANDS
            .contains(&impulse_desktop::host_commands::GOVERNED_TASK_MUTATE_COMMAND),
        "the live host must expose the acknowledged governed-task command"
    );

    let request = GovernedTaskMutationRequest {
        request_id: GovernedRequestId::try_new("req-ui-supervisor").unwrap(),
        project_id: "impulse-rs".to_string(),
        task_id: GovernedTaskId::try_new("task-ui-supervisor").unwrap(),
        expected_revision: 7,
        mutation: GovernedTaskMutation::RecordSupervisorVerdict {
            verdict: SupervisorVerdictInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Supervisor,
                    id: "supervisor-ui".to_string(),
                },
                verification_id: impulse_ops::governed_task::GovernedRecordId::try_new(
                    "verification-ui",
                )
                .unwrap(),
                verdict: SupervisorVerdictKind::RecommendAccept,
                rationale: "Evidence satisfies the acceptance criteria".to_string(),
            },
        },
    };
    let script = governed_task_mutation_bridge_script(&request);

    assert!(script.contains("mutateGovernedTask"));
    assert!(script.contains(r#""expected_revision":7"#));
    assert!(script.contains(r#""recommend_accept""#));
    assert!(!script.contains("confirmed"));
}

#[test]
fn test_desktop_reducer_accepts_registry_platform_catalog_messages() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut agent_platforms = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    let result = apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut agent_platforms,
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "agent_platforms".to_string(),
            payload: json!({
                "platforms": [
                    {"id": "ion", "label": "Ion", "command": "ion"},
                    {"id": "custom-agent", "label": "Custom", "command": "custom"}
                ]
            }),
        },
    );

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        agent_platforms
            .iter()
            .map(|platform| platform.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ion", "custom-agent"]
    );
}

#[test]
fn test_agent_platform_failure_revokes_catalog_and_blocks_governed_preview() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut agent_platforms = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    for message in [
        DesktopBridgeMessage {
            kind: "agent_platforms".to_string(),
            payload: json!({
                "platforms": [{
                    "id": "codex",
                    "label": "Codex",
                    "command": "codex",
                    "runtime_capabilities": [
                        { "capability": "workspace.target", "enforcement": "mediated" },
                        { "capability": "process.lifecycle", "enforcement": "mediated" }
                    ]
                }]
            }),
        },
        DesktopBridgeMessage {
            kind: "bridge_status".to_string(),
            payload: json!({
                "status": "agent_platforms_failed",
                "reason": "registry unavailable"
            }),
        },
    ] {
        apply_desktop_bridge_message(
            DesktopBridgeStateMut::new(
                &mut snapshot,
                &mut runtime_agents,
                &mut agent_platforms,
                &mut workspaces,
                &mut mcp_tools,
                &mut review_queue,
                &mut last_invocations,
            ),
            message,
        )
        .expect("platform catalog messages should reduce");

        if !agent_platforms.is_empty() {
            assert!(governed_launch(&agent_platforms, "codex", "Build it").is_ok());
        }
    }

    assert!(agent_platforms.is_empty());
    assert!(governed_launch(&agent_platforms, "codex", "Build it").is_err());
}

#[test]
fn test_malformed_agent_platform_catalog_revokes_stale_compatibility_and_recovers() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut agent_platforms = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    let compatible = DesktopBridgeMessage {
        kind: "agent_platforms".to_string(),
        payload: json!({
            "platforms": [{
                "id": "codex",
                "label": "Codex",
                "command": "codex",
                "runtime_capabilities": [
                    { "capability": "workspace.target", "enforcement": "mediated" },
                    { "capability": "process.lifecycle", "enforcement": "mediated" }
                ]
            }]
        }),
    };
    let malformed = DesktopBridgeMessage {
        kind: "agent_platforms".to_string(),
        payload: json!({ "platforms": [{ "id": 42 }] }),
    };

    let initial_status = apply_desktop_bridge_message_with_status(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut agent_platforms,
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        compatible.clone(),
    )
    .expect("compatible catalog reduces");
    assert!(initial_status.is_none());
    assert!(governed_launch(&agent_platforms, "codex", "Build it").is_ok());

    let failure_status = apply_desktop_bridge_message_with_status(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut agent_platforms,
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        malformed,
    )
    .expect("malformed catalog becomes an explicit degraded status");
    assert_eq!(
        failure_status.as_ref().map(|status| status.status.as_str()),
        Some("agent_platforms_failed")
    );
    assert!(agent_platforms.is_empty());
    assert!(governed_launch(&agent_platforms, "codex", "Build it").is_err());

    let recovery_status = apply_desktop_bridge_message_with_status(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut agent_platforms,
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        compatible,
    )
    .expect("valid catalog recovers");
    assert!(recovery_status.is_none());
    assert!(governed_launch(&agent_platforms, "codex", "Build it").is_ok());
}

#[test]
fn test_mcp_invoke_and_focus_bridge_scripts_serialize_requests() {
    let mcp_script = mcp_invoke_bridge_script(&impulse_desktop::host_commands::McpInvokeRequest {
        tool: "impulse.project_context".to_string(),
        arguments: json!({ "root": "/tmp" }),
        confirmed: true,
        caller_agent_id: Some("impulse-ui".to_string()),
    });
    let focus_script = agent_focus_bridge_script("codex-live");

    assert!(mcp_script.contains("invokeMcp"));
    assert!(mcp_script.contains(r#""tool":"impulse.project_context""#));
    assert!(focus_script.contains("focusAgent"));
    assert!(focus_script.contains(r#""codex-live""#));
}

#[test]
fn test_apply_bridge_message_accepts_full_desktop_event_wrappers() {
    let runtime = runtime_snapshot("codex-live");
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();
    let event = DesktopEvent::AgentRuntimeUpdate {
        snapshot: Box::new(runtime),
    };

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "agent_runtime_update".to_string(),
            payload: serde_json::to_value(event).expect("serialize event"),
        },
    )
    .expect("apply runtime update");

    assert_eq!(runtime_agents.len(), 1);
    assert_eq!(runtime_agents[0].agent_id, "codex-live");
    assert!(
        snapshot.agents.is_empty(),
        "local runtime events are not daemon truth"
    );
}

#[test]
fn test_incremental_focused_runtime_update_clears_peer_focus() {
    let first = runtime_snapshot("focus-one");
    let mut second = runtime_snapshot("focus-two");
    second.focused = false;
    let mut runtime_agents = vec![first, second];
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "agent_runtime_update".to_string(),
            payload: serde_json::to_value(DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(runtime_snapshot("focus-two")),
            })
            .expect("focused runtime event"),
        },
    )
    .expect("apply focused runtime update");

    assert_eq!(
        runtime_agents.iter().filter(|agent| agent.focused).count(),
        1
    );
    assert!(runtime_agents
        .iter()
        .any(|agent| agent.agent_id == "focus-two" && agent.focused));
    assert!(runtime_agents
        .iter()
        .any(|agent| agent.agent_id == "focus-one" && !agent.focused));
}

#[test]
fn test_daemon_ops_snapshot_survives_local_runtime_refresh_and_exit() {
    let daemon_agent = AgentRuntime {
        id: "daemon-agent".to_string(),
        label: "Daemon Agent".to_string(),
        backend_kind: "codex".to_string(),
        active: true,
        ..Default::default()
    };
    let expected = ProjectOpsSnapshot {
        generated_at: "daemon-truth".to_string(),
        agents: vec![daemon_agent],
        memory: MemorySummary {
            active_sessions: 4,
            ..Default::default()
        },
        retrieval: RetrievalSummary {
            backend: "sqlite-vector".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let runtime = runtime_snapshot("codex-live");
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    for message in [
        DesktopBridgeMessage {
            kind: "ops_update".to_string(),
            payload: serde_json::to_value(&expected).expect("ops snapshot"),
        },
        DesktopBridgeMessage {
            kind: "agent_runtime_update".to_string(),
            payload: serde_json::to_value(DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(runtime.clone()),
            })
            .expect("runtime event"),
        },
        DesktopBridgeMessage {
            kind: "agent_snapshot".to_string(),
            payload: json!({"agents": [runtime]}),
        },
    ] {
        apply_desktop_bridge_message(
            DesktopBridgeStateMut::new(
                &mut snapshot,
                &mut runtime_agents,
                &mut Vec::new(),
                &mut workspaces,
                &mut mcp_tools,
                &mut review_queue,
                &mut last_invocations,
            ),
            message,
        )
        .expect("apply message");
    }
    assert_eq!(snapshot, expected);
    assert_eq!(runtime_agents.len(), 1);

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "terminal_exit".to_string(),
            payload: json!({"agent_id": "codex-live"}),
        },
    )
    .expect("apply terminal exit");
    assert!(runtime_agents.is_empty());
    assert_eq!(snapshot, expected);

    let mut dead_runtime = runtime_snapshot("codex-live");
    dead_runtime.alive = false;
    dead_runtime.status = AgentStatus::Completed;
    for message in [
        DesktopBridgeMessage {
            kind: "agent_snapshot".to_string(),
            payload: json!({"agents": [dead_runtime.clone()]}),
        },
        DesktopBridgeMessage {
            kind: "agent_runtime_update".to_string(),
            payload: serde_json::to_value(DesktopEvent::AgentRuntimeUpdate {
                snapshot: Box::new(dead_runtime),
            })
            .expect("dead runtime event"),
        },
    ] {
        apply_desktop_bridge_message(
            DesktopBridgeStateMut::new(
                &mut snapshot,
                &mut runtime_agents,
                &mut Vec::new(),
                &mut workspaces,
                &mut mcp_tools,
                &mut review_queue,
                &mut last_invocations,
            ),
            message,
        )
        .expect("apply dead runtime refresh");
    }
    assert!(
        runtime_agents.is_empty(),
        "dead runtime snapshots stay reaped"
    );
    assert_eq!(snapshot, expected);
}

#[test]
fn test_apply_bridge_message_accepts_ops_update_wrapper() {
    let expected = ProjectOpsSnapshot {
        generated_at: "2026-06-13T18:00:00Z".to_string(),
        retrieval: RetrievalSummary {
            backend: "sqlite-vector".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "ops_update".to_string(),
            payload: serde_json::to_value(DesktopEvent::OpsUpdate {
                payload: serde_json::to_value(expected).expect("serialize ops snapshot"),
            })
            .expect("serialize event"),
        },
    )
    .expect("apply ops update");

    assert_eq!(snapshot.generated_at, "2026-06-13T18:00:00Z");
    assert_eq!(snapshot.retrieval.backend, "sqlite-vector");
}

#[test]
fn test_apply_bridge_message_accepts_review_queue_items() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "review_queue".to_string(),
            payload: json!({
                "items": [{
                    "id": "review-1",
                    "staged_at_unix_ms": 1,
                    "status": "pending",
                    "target_agent_id": "codex-live",
                    "arguments": { "content": "cargo test\n" },
                    "path": "/tmp/review-1.json",
                    "preview": "cargo test\\n"
                }]
            }),
        },
    )
    .expect("apply review queue");

    assert_eq!(review_queue.len(), 1);
    assert_eq!(review_queue[0].id, "review-1");
    assert_eq!(review_queue[0].status, ReviewQueueStatus::Pending);
}

#[test]
fn test_apply_bridge_message_accepts_workspace_and_mcp_descriptor_payloads() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "workspaces".to_string(),
            payload: json!({
                "workspaces": [{
                    "target": {
                        "root": "/tmp",
                        "label": "scratch",
                        "purpose": "terminal harness",
                        "project_notes": "operator-authored context"
                    },
                    "last_used_unix_ms": null
                }]
            }),
        },
    )
    .expect("apply workspaces");

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "mcp_descriptors".to_string(),
            payload: json!({
                "tools": [{
                    "name": "impulse.agent_spawn",
                    "description": "spawn a coding agent",
                    "capabilities": ["terminal", "workspace"],
                    "requires_confirmation": true
                }]
            }),
        },
    )
    .expect("apply descriptors");

    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].target.root, "/tmp");
    assert_eq!(
        workspaces[0].target.project_notes.as_deref(),
        Some("operator-authored context")
    );
    assert_eq!(mcp_tools.len(), 1);
    assert_eq!(mcp_tools[0].name, "impulse.agent_spawn");
    assert!(mcp_tools[0].requires_confirmation);
}

#[test]
fn test_apply_bridge_message_upserts_workspace_registered_payload() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    for label in ["scratch", "scratch-renamed"] {
        apply_desktop_bridge_message(
            DesktopBridgeStateMut::new(
                &mut snapshot,
                &mut runtime_agents,
                &mut Vec::new(),
                &mut workspaces,
                &mut mcp_tools,
                &mut review_queue,
                &mut last_invocations,
            ),
            DesktopBridgeMessage {
                kind: "workspace_registered".to_string(),
                payload: json!({
                    "entry": {
                        "target": {
                            "root": "/tmp",
                            "label": label,
                            "purpose": "terminal harness",
                            "project_notes": "operator-authored context"
                        },
                        "last_used_unix_ms": null
                    }
                }),
            },
        )
        .expect("apply registered workspace");
    }

    assert_eq!(workspaces.len(), 1);
    assert_eq!(
        workspaces[0].target.label.as_deref(),
        Some("scratch-renamed")
    );
}

#[test]
fn test_apply_bridge_message_accepts_mcp_invocation_receipts() {
    let mut snapshot = ProjectOpsSnapshot::default();
    let mut runtime_agents = Vec::new();
    let mut workspaces = Vec::new();
    let mut mcp_tools = Vec::new();
    let mut review_queue = Vec::new();
    let mut last_invocations = Vec::new();

    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut runtime_agents,
            &mut Vec::new(),
            &mut workspaces,
            &mut mcp_tools,
            &mut review_queue,
            &mut last_invocations,
        ),
        DesktopBridgeMessage {
            kind: "mcp_invocation".to_string(),
            payload: json!({
                "invocation": {
                    "call_id": "call-review",
                    "tool": "impulse.review_decision",
                    "caller_agent_id": "supervisor",
                    "arguments": { "id": "review-1", "decision": "apply" },
                    "confirmed": true,
                    "result": { "ok": true },
                    "ok": true
                }
            }),
        },
    )
    .expect("apply MCP invocation");

    assert_eq!(last_invocations.len(), 1);
    assert_eq!(last_invocations[0].tool, "impulse.review_decision");
    assert!(last_invocations[0].confirmed);
    assert!(last_invocations[0].ok);
}

#[test]
fn test_shell_render_accepts_live_agents_workspaces_and_tools() {
    let snapshot = ProjectOpsSnapshot::default();
    let runtime = runtime_snapshot("codex-live");
    let workspace = WorkspaceEntry::new(WorkspaceTarget {
        root: "<repo>".to_string(),
        label: Some("IMPULSE-rs".to_string()),
        purpose: Some("terminal harness".to_string()),
        project_notes: Some("review project notes before injection".to_string()),
    });
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: vec![runtime],
            agent_platforms: Vec::new(),
            workspaces: vec![workspace],
            mcp_tools: vec![BuiltInMcpTool::new(
                "impulse.project_context",
                "read workspace context",
                vec!["workspace".to_string(), "read_only".to_string()],
                false,
            )],
            last_invocations: vec![McpInvocation {
                call_id: "call-1".to_string(),
                tool: "impulse.project_context".to_string(),
                caller_agent_id: Some("codex-live".to_string()),
                arguments: json!({}),
                confirmed: true,
                result: json!({ "ok": true }),
                ok: true,
            }],
            review_queue: vec![ReviewQueueItem {
                id: "review-1".to_string(),
                staged_at_unix_ms: 1,
                status: ReviewQueueStatus::Pending,
                decided_at_unix_ms: None,
                decision: None,
                target_agent_id: Some("codex-live".to_string()),
                arguments: json!({ "content": "cargo test\n" }),
                path: "/tmp/review-1.json".to_string(),
                preview: "cargo test\\n".to_string(),
            }],
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("Codex Live"));
    assert!(html.contains("data-agent-id=\"codex-live\""));
    assert!(html.contains("terminal-pane-codex-live"));
    assert!(html.contains("IMPULSE-rs"));
    assert!(html.contains("workspace-notes"));
    assert!(html.contains("impulse.project_context"));
    assert!(html.contains("Workspace Launcher"));
    assert!(html.contains("Register folder"));
    assert!(html.contains("Launch agent"));
    assert!(html.contains("MCP audited"));
    assert!(html.contains("1 injection(s) awaiting review"));
    assert!(!html.contains("Review Queue"));
    assert!(!html.contains("data-source=\"operator_board\""));
    assert!(html.contains("1 invocations"));
}

#[test]
fn test_shell_review_route_gates_review_console() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: vec![runtime_snapshot("codex-live")],
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: vec![ReviewQueueItem {
                id: "review-1".to_string(),
                staged_at_unix_ms: 1,
                status: ReviewQueueStatus::Pending,
                decided_at_unix_ms: None,
                decision: None,
                target_agent_id: Some("codex-live".to_string()),
                arguments: json!({ "content": "cargo test\n" }),
                path: "/tmp/review-1.json".to_string(),
                preview: "cargo test\\n".to_string(),
            }],
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Review,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("stage-view view-terminal"));
    assert!(!html.contains("stage-view view-terminal active"));
    assert!(html.contains("<section class=\"review-console\""));
    assert!(html.contains("Review Queue"));
    assert!(html.contains("1 pending"));
    assert!(html.contains("cargo test"));
    assert!(html.contains("Apply"));
    assert!(html.contains("Skip"));
    assert!(!html.contains("data-source=\"operator_board\""));
}

#[test]
fn test_shell_supervisor_route_gates_operator_board() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: vec![runtime_snapshot("codex-live")],
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: vec![McpInvocation {
                call_id: "call-1".to_string(),
                tool: "impulse.project_context".to_string(),
                caller_agent_id: Some("codex-live".to_string()),
                arguments: json!({}),
                confirmed: true,
                result: json!({ "ok": true }),
                ok: true,
            }],
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Supervisor,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("stage-view view-terminal"));
    assert!(!html.contains("stage-view view-terminal active"));
    assert!(html.contains("data-source=\"operator_board\""));
    assert!(html.contains("In flight"));
    assert!(html.contains("1 audit rows"));
    assert!(!html.contains("<section class=\"review-console\""));
}

#[test]
fn test_shell_supervisor_route_renders_authoritative_governed_evidence_and_controls() {
    let governed_task = serde_json::from_value(json!({
        "id": "task-visual-review",
        "revision": 3,
        "project_id": "impulse-rs",
        "workspace_root": "/tmp/impulse-rs",
        "task": "Prove the governed completion path with a deliberately long task title",
        "acceptance_criteria": ["Workspace tests pass", "No raw command output is retained"],
        "approval_policy": "operator_required",
        "runtime_id": "codex",
        "agent_id": "codex-live",
        "session_id": "codex-live-session",
        "initial_subject_revision": "abc123",
        "execution_state": "runtime_exited",
        "review_state": "awaiting_supervisor",
        "claims": [{
            "id": "claim-visual-review",
            "actor": { "kind": "worker", "id": "codex-live" },
            "summary": "Implemented the daemon-owned task lifecycle and recovery path.",
            "subject_revision": "def456",
            "artifact_ids": ["artifact-governed-diff"],
            "diff_ref": "artifacts/governed-task.diff",
            "submitted_at": "2026-07-13T20:00:00Z",
            "based_on_revision": 1
        }],
        "verifications": [{
            "id": "verification-visual-review",
            "actor": { "kind": "verifier", "id": "impulse-verifier" },
            "claim_id": "claim-visual-review",
            "subject_revision": "def456",
            "policy": "rust-workspace",
            "outcome": "passed",
            "commands": [{
                "name": "workspace tests",
                "executable": "cargo",
                "redacted_args": ["test", "--workspace", "<redacted>"],
                "command_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "exit_code": 0,
                "success": true,
                "output_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "output_ref": "artifacts/verification/workspace-tests.log",
                "output_bytes": 2048,
                "output_truncated": true
            }],
            "artifact_ids": ["artifact-workspace-tests"],
            "notes": "All mandatory checks passed against the claimed revision.",
            "recorded_at": "2026-07-13T20:01:00Z",
            "based_on_revision": 2
        }],
        "supervisor_verdicts": [],
        "operator_decisions": [],
        "events": [],
        "created_at": "2026-07-13T19:59:00Z",
        "updated_at": "2026-07-13T20:01:00Z"
    }))
    .expect("governed task fixture");
    let snapshot = ProjectOpsSnapshot {
        governed_tasks: vec![governed_task],
        ..Default::default()
    };

    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Supervisor,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("data-governed-task-id=\"task-visual-review\""));
    assert!(html.contains("data-review-state=\"awaiting supervisor\""));
    assert!(html.contains("Implemented the daemon-owned task lifecycle"));
    assert!(html.contains("redacted argv"));
    assert!(html.contains("&lt;redacted&gt;"));
    assert!(html.contains("output truncated"));
    assert!(html.contains("Recommend accept"));
    assert!(html.contains("Request changes"));
    assert!(html.contains("Escalate"));
    assert!(!html.contains(">Approve<"));
}

fn profiled_governed_task(
    id: &str,
    review_state: &str,
) -> impulse_ops::governed_task::GovernedTaskRun {
    serde_json::from_value(json!({
        "id": id,
        "revision": 1,
        "project_id": "impulse-rs",
        "workspace_root": "/tmp/impulse-rs",
        "task": format!("Exercise {review_state} producer guidance"),
        "acceptance_criteria": ["The daemon owns the producer record"],
        "approval_policy": "operator_required",
        "verification_profile": "rust_workspace_v1",
        "runtime_id": "codex",
        "agent_id": format!("worker-{id}"),
        "execution_state": "running",
        "review_state": review_state,
        "claims": [],
        "verifications": [],
        "supervisor_verdicts": [],
        "operator_decisions": [],
        "events": [],
        "created_at": "2026-07-13T20:00:00Z",
        "updated_at": "2026-07-13T20:00:00Z"
    }))
    .expect("profiled governed task fixture")
}

#[test]
fn test_profiled_governed_tasks_label_rust_only_and_route_all_producers_via_daemon() {
    let snapshot = ProjectOpsSnapshot {
        governed_tasks: vec![
            profiled_governed_task("task-claim-guidance", "awaiting_claim"),
            profiled_governed_task("task-verify-guidance", "awaiting_verification"),
            profiled_governed_task("task-review-guidance", "awaiting_supervisor"),
        ],
        ..Default::default()
    };
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Supervisor,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("Rust-only verification profile · rust_workspace_v1"));
    assert_eq!(
        html.matches("data-verification-profile=\"rust_workspace_v1\"")
            .count(),
        4,
        "the launcher and all three profiled task cards must identify the Rust-only profile"
    );
    assert_eq!(html.matches("class=\"governed-task-profile\"").count(), 3);
    assert!(html.contains("&quot;$IMPULSE_CONTROL_CLI&quot; --daemon governed-claim"));
    assert!(html.contains("&quot;$IMPULSE_CONTROL_CLI&quot; --daemon governed-verify"));
    assert!(html.contains("&quot;$IMPULSE_CONTROL_CLI&quot; --daemon governed-review"));
    assert!(!html.contains("$IMPULSE_CONTROL_CLI governed-"));
}

#[test]
fn test_retro_theme_helpers_map_backend_statuses() {
    assert_eq!(format_count(999), "999");
    assert_eq!(format_count(47_238), "47.2k");
    assert_eq!(status_dot_class(&AgentStatus::Idle), "status-idle");
    assert_eq!(
        status_dot_class(&AgentStatus::Working {
            task: "build".to_string(),
        }),
        "status-working"
    );
    assert_eq!(
        status_dot_class(&AgentStatus::Interrupted),
        "status-blocked"
    );
    assert_eq!(status_label(&AgentStatus::Completed), "done");
}

#[test]
fn test_terminal_interop_serializes_xterm_input_as_byte_array() {
    let script = impulse_desktop::ui::terminal_interop_script();

    assert!(script.contains("window.Terminal || window.XTerm?.Terminal"));
    assert!(script.contains("window.FitAddon?.FitAddon || window.FitAddon"));
    assert!(script.contains("const encoder = new TextEncoder();"));
    assert!(script.contains("const encodeInput = (data) => Array.from(encoder.encode(data));"));
    assert!(script.contains("mounts.forEach(mountAgentTerminal);"));
    assert!(script.contains("listenersMounted"));
    assert!(!script.contains("already-mounted"));
    assert!(script.contains(
        r#"invokeTerminal("agent_write", { request: { agent_id: agentId, data: encodeInput(data) } });"#
    ));
    assert!(script.contains(r#"data-xterm-state", "input-error"#));
    assert!(!script.contains("data } });"));
}

#[test]
fn test_terminal_interop_rerun_mounts_new_panes_without_duplicate_listeners() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node is unavailable; skipping terminal interop behavior smoke");
        return;
    }

    let tempdir = tempfile::tempdir().expect("tempdir");
    let smoke_path = tempdir.path().join("terminal-interop-smoke.js");
    let smoke = format!(
        r#"
const assert = require("assert");
const interopScript = {interop_script};

(async () => {{

const listeners = [];
const invokes = [];
const makeMount = (agentId) => ({{
  dataset: {{ agentId }},
  attrs: {{}},
  setAttribute(name, value) {{ this.attrs[name] = value; }},
}});

let mounts = [makeMount("codex")];
global.document = {{
  querySelectorAll(selector) {{
    assert.strictEqual(selector, "[data-xterm-mount='true']");
    return mounts;
  }},
}};
global.window = {{
  __IMPULSE_DESKTOP_HOST: {{
    invoke(command, payload) {{
      invokes.push({{ command, payload }});
      return Promise.resolve(null);
    }},
    listen(event, handler) {{
      listeners.push({{ event, handler }});
      return Promise.resolve(() => {{}});
    }},
  }},
}};

class Terminal {{
  constructor() {{
    this.writes = [];
    this.dataHandlers = [];
  }}
  loadAddon() {{}}
  open(mount) {{ mount.opened = true; }}
  onData(handler) {{ this.dataHandlers.push(handler); }}
  onResize() {{}}
  write(value) {{ this.writes.push(value); }}
}}
global.window.Terminal = Terminal;
global.window.FitAddon = class {{ fit() {{}} }};

const first = await eval(interopScript);
await Promise.resolve();
await Promise.resolve();
assert.strictEqual(first, "mounted");
assert.deepStrictEqual(Object.keys(window.__impulseTerminalInterop.terminals), ["codex"]);
assert.strictEqual(listeners.length, 2);
assert.deepStrictEqual(listeners.map((item) => item.event), ["terminal_output", "terminal_exit"]);
assert.strictEqual(mounts[0].attrs["data-xterm-state"], "mounted");

const codexMount = mounts[0];
mounts = [codexMount, makeMount("claude")];
const second = await eval(interopScript);
await Promise.resolve();
await Promise.resolve();
assert.strictEqual(second, "mounted");
assert.deepStrictEqual(Object.keys(window.__impulseTerminalInterop.terminals).sort(), ["claude", "codex"]);
assert.strictEqual(listeners.length, 2, "terminal listeners should not duplicate on rerun");
assert.strictEqual(mounts[1].attrs["data-xterm-state"], "mounted");

listeners.find((item) => item.event === "terminal_output").handler({{
  payload: {{ agent_id: "claude", data: [111, 107] }},
}});
assert.deepStrictEqual(window.__impulseTerminalInterop.terminals.claude.writes, ["ok"]);
}})().catch((error) => {{
  console.error(error);
  process.exit(1);
}});
"#,
        interop_script = serde_json::to_string(impulse_desktop::ui::terminal_interop_script())
            .expect("serialize interop script")
    );
    std::fs::write(&smoke_path, smoke).expect("write terminal interop smoke");

    let output = Command::new("node")
        .arg(&smoke_path)
        .output()
        .expect("run terminal interop smoke");
    assert!(
        output.status.success(),
        "terminal interop smoke failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_agent_write_request_accepts_bytes_and_rejects_js_string_data() {
    let decoded: AgentWriteRequest =
        serde_json::from_value(json!({ "agent_id": "codex", "data": [112, 119, 100, 10] }))
            .expect("byte array data should deserialize");
    assert_eq!(decoded.agent_id, "codex");
    assert_eq!(decoded.data, b"pwd\n");

    let error = serde_json::from_value::<AgentWriteRequest>(
        json!({ "agent_id": "codex", "data": "pwd\n" }),
    )
    .expect_err("string data should not deserialize as Vec<u8>");
    assert!(error.to_string().contains("invalid type"));
}

#[test]
fn test_terminal_bridge_routes_open_write_resize_focus_close() {
    let terminal_bridge = InMemoryTerminalBridge::default();
    let router =
        DesktopCommandRouter::new(terminal_bridge, impulse_desktop::DefaultNativeIslandHost);

    let opened = router
        .terminal_open(TerminalOpenRequest {
            session_id: Some("session-a".to_string()),
            command: "codex".to_string(),
            args: Vec::new(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            workspace: None,
            mcp_tools: Vec::new(),
            rows: 30,
            cols: 100,
        })
        .expect("open terminal session");

    assert_eq!(opened.session_id, "session-a");
    assert_eq!(opened.rows, 30);
    assert_eq!(opened.cols, 100);

    router
        .terminal_write(TerminalWriteRequest {
            session_id: "session-a".to_string(),
            data: b"hello".to_vec(),
        })
        .expect("write terminal input");

    router
        .terminal_resize(TerminalResizeRequest {
            session_id: "session-a".to_string(),
            rows: 40,
            cols: 120,
        })
        .expect("resize terminal");

    router
        .terminal_focus(TerminalFocusRequest {
            session_id: "session-a".to_string(),
        })
        .expect("focus terminal");

    router
        .terminal_close(TerminalCloseRequest {
            session_id: "session-a".to_string(),
        })
        .expect("close terminal");
}

#[test]
fn test_native_island_request_uses_serializable_dto_boundary() {
    let request = NativeIslandRequest {
        request_id: "native-1".to_string(),
        kind: NativeIslandKind::AppKitProbe,
        payload: json!({ "source": "dioxus-command-palette" }),
    };

    let json = serde_json::to_string(&request).expect("serialize request");
    let decoded: NativeIslandRequest = serde_json::from_str(&json).expect("deserialize request");

    assert_eq!(decoded.request_id, "native-1");
    assert_eq!(decoded.kind, NativeIslandKind::AppKitProbe);
    assert_eq!(decoded.payload["source"], "dioxus-command-palette");
}

#[test]
fn test_native_island_probe_reports_dioxus_as_state_owner() {
    let router = DesktopCommandRouter::new(
        InMemoryTerminalBridge::default(),
        impulse_desktop::DefaultNativeIslandHost,
    );

    let result = router
        .native_island_request(NativeIslandRequest {
            request_id: "probe-1".to_string(),
            kind: NativeIslandKind::AppKitProbe,
            payload: json!({}),
        })
        .expect("probe native island");

    assert_eq!(result.request_id, "probe-1");
    assert_eq!(result.kind, NativeIslandKind::AppKitProbe);
    assert_eq!(result.payload["state_owner"], "dioxus");
}

#[cfg(all(target_os = "macos", feature = "native-macos"))]
#[test]
fn test_appkit_probe_smoke_uses_objc_bridge() {
    let router = DesktopCommandRouter::new(
        InMemoryTerminalBridge::default(),
        impulse_desktop::DefaultNativeIslandHost,
    );

    let result = router
        .native_island_request(NativeIslandRequest {
            request_id: "appkit-smoke".to_string(),
            kind: NativeIslandKind::AppKitProbe,
            payload: json!({}),
        })
        .expect("probe AppKit through objc2");

    assert!(result.handled);
    assert_eq!(result.payload["bridge"], "objc2");
    assert_eq!(result.payload["framework"], "AppKit");
}

#[test]
fn test_bridge_status_update_parses_degraded_and_failed_messages() {
    let degraded = BridgeStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "bridge_status".to_string(),
        payload: json!({ "status": "degraded", "reason": "host event API unavailable" }),
    })
    .expect("degraded status parses");
    assert!(degraded.is_degraded());
    assert_eq!(degraded.headline(), "Host bridge degraded");
    assert_eq!(
        degraded.reason.as_deref(),
        Some("host event API unavailable")
    );

    let failed = BridgeStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "bridge_status".to_string(),
        payload: json!({ "status": "review_queue_failed", "reason": "boom" }),
    })
    .expect("failed status parses");
    assert!(failed.is_degraded());
    assert_eq!(failed.headline(), "Host call failed: review queue");
}

#[test]
fn test_bridge_status_update_ignores_other_messages_and_empty_status() {
    assert!(BridgeStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "ops_update".to_string(),
        payload: json!({ "status": "degraded" }),
    })
    .is_none());
    assert!(BridgeStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "bridge_status".to_string(),
        payload: json!({ "reason": "missing status" }),
    })
    .is_none());
    assert!(BridgeStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "bridge_status".to_string(),
        payload: json!({ "status": "" }),
    })
    .is_none());
}

#[test]
fn test_bridge_status_update_recovery_markers_are_not_degraded() {
    for status in ["mounted", "ok", "ready"] {
        let update = BridgeStatusUpdate {
            status: status.to_string(),
            reason: None,
        };
        assert!(!update.is_degraded(), "{status} should clear the banner");
    }
}

#[test]
fn test_daemon_ops_status_update_parses_connection_health() {
    let degraded = DaemonOpsStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "ops_connection_update".to_string(),
        payload: json!({"connected": false, "error": "daemon unavailable"}),
    })
    .expect("daemon ops status");
    assert!(!degraded.connected);
    assert_eq!(degraded.error.as_deref(), Some("daemon unavailable"));
    assert!(DaemonOpsStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "ops_update".to_string(),
        payload: json!({"connected": true}),
    })
    .is_none());

    let wrapped = DaemonOpsStatusUpdate::parse(&DesktopBridgeMessage {
        kind: "ops_connection_update".to_string(),
        payload: serde_json::to_value(DesktopEvent::OpsConnectionUpdate {
            connected: true,
            error: None,
        })
        .expect("wrapped connection event"),
    })
    .expect("legacy wrapped daemon status");
    assert!(wrapped.connected);
}

#[test]
fn test_shell_renders_bridge_status_banner_when_degraded() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: Some(BridgeStatusUpdate {
                status: "degraded".to_string(),
                reason: Some("host event API unavailable".to_string()),
            }),
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    // The class name also appears in the inlined CRT stylesheet, so assert on
    // the rendered element's marker attribute, which the CSS never emits.
    assert!(html.contains("data-bridge-status=\"degraded\""));
    assert!(html.contains("class=\"bridge-status-banner\""));
    assert!(html.contains("Host bridge degraded"));
    assert!(html.contains("host event API unavailable"));
}

#[test]
fn test_shell_hides_bridge_status_banner_when_healthy() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    // The class is present in the inlined stylesheet; the element's marker
    // attribute is what proves the banner did (not) render.
    assert!(!html.contains("data-bridge-status="));
}

#[test]
fn test_footer_stream_health_reflects_live_agent() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: vec![runtime_snapshot("codex-live")],
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: Some(DaemonOpsStatusUpdate {
                connected: true,
                error: None,
            }),
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    // runtime_snapshot has output_bytes > 0 and a present agent → both streams live.
    assert!(html.contains("terminal_output · live"));
    assert!(html.contains("agent_runtime_update · live"));
    assert!(html.contains("ops_update awaiting first ops_update · live"));
    assert!(html.contains("online · watching"));
    assert!(html.contains("data-daemon-freshness=\"current\""));
    assert!(html.contains("supervisor_local_action · ready"));
    assert!(!html.contains("stream pending"));
}

#[test]
fn test_publish_only_degradation_keeps_subscribed_snapshot_current() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot {
                generated_at: "fresh-daemon-snapshot".to_string(),
                agents: vec![AgentRuntime {
                    id: "daemon-agent".to_string(),
                    active: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            runtime_agents: vec![runtime_snapshot("codex-live")],
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: Some(DaemonOpsStatusUpdate {
                connected: true,
                error: Some("publish: temporary failure".to_string()),
            }),
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("online · publish degraded"));
    assert!(html.contains("data-daemon-freshness=\"current\""));
    assert!(html.contains("data-daemon-status=\"publish-degraded\""));
    assert!(html.contains("Daemon snapshot reads remain live"));
    assert!(html.contains("online · 0 working"));
    assert!(!html.contains("Daemon disconnected"));
    assert!(!html.contains("cached snapshot hidden"));
}

#[test]
fn test_footer_stream_health_reads_down_when_transport_degraded() {
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot: ProjectOpsSnapshot::default(),
            runtime_agents: vec![runtime_snapshot("codex-live")],
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: Some(BridgeStatusUpdate {
                status: "degraded".to_string(),
                reason: Some("host event API unavailable".to_string()),
            }),
            daemon_ops_status: Some(DaemonOpsStatusUpdate {
                connected: false,
                error: Some("daemon unavailable".to_string()),
            }),
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Terminal,
        },
    );
    vdom.rebuild_in_place();
    let html = dioxus_ssr::render(&vdom);

    // A degraded transport means no events can arrive — every stream reads down.
    assert!(html.contains("terminal_output · down"));
    assert!(html.contains("agent_runtime_update · down"));
    assert!(html.contains("ops_update awaiting first ops_update · down"));
    assert!(html.contains("daemon offline"));
    assert!(html.contains("data-daemon-freshness=\"stale\""));
    assert!(html.contains("data-daemon-status=\"stale\""));
    assert!(html.contains("Workbench data is cached and may be stale."));
    assert!(html.contains("0 cached artifacts"));
    assert!(html.contains("0 cached interventions"));
    assert!(html.contains("supervisor_local_action · down"));
}

// ─────────────── ADR-0019 staged-worktree controls (protocol v9) ───────────────

/// An accepted, staged governed task with an active checkout.
fn staged_governed_task() -> impulse_ops::governed_task::GovernedTaskRun {
    use impulse_ops::governed_task as gt;
    with_accepted_claim(gt::GovernedTaskRun {
        id: gt::GovernedTaskId::try_new("staged-task").expect("task id"),
        revision: 7,
        project_id: "impulse-rs".to_string(),
        workspace_root: "/tmp/impulse-rs".to_string(),
        task: "Wire the cockpit's staged controls".to_string(),
        acceptance_criteria: vec!["the workspace gate is green".to_string()],
        approval_policy: gt::ApprovalPolicy::OperatorRequired,
        verification_profile: Some(gt::GovernedVerificationProfile::RustWorkspaceV1),
        role_assignment: None,
        role_compatibility: None,
        runtime_id: "ion".to_string(),
        agent_id: "builder-01".to_string(),
        session_id: None,
        initial_subject_revision: Some("a".repeat(40)),
        world_scope: gt::WorldScope::StagedAuthoritative,
        staged_worktree: Some(gt::StagedWorktree {
            id: gt::GovernedRecordId::try_new("staged-1").expect("staged id"),
            actor: gt::GovernedActor {
                kind: gt::GovernedActorKind::System,
                id: "impulse-daemon:staged_worktree".to_string(),
            },
            root: "/tmp/impulse-rs/.impulse/worktrees/staged-task".to_string(),
            initial_subject_revision: "a".repeat(40),
            // Pinned on purpose: an *unpinned* worktree can never be promoted,
            // so ADR-0019 rule 7 makes it discardable from any state, which
            // would mask the accepted-run rule these tests exercise.
            // #52 added a pin scheme version; `current()` is its constructor,
            // so the fixture tracks the live scheme instead of pinning a
            // literal that will drift the next time the scheme moves.
            shared_config_digest: gt::SharedRepositoryConfigPin::Recorded(
                gt::SharedRepositoryConfigDigest::current(
                    format!("sha256:{}", "d".repeat(64)),
                    None,
                    None,
                ),
            ),
            status: gt::StagedWorktreeStatus::Active,
            materialized_at: "2026-09-12T00:00:00Z".to_string(),
            based_on_revision: 1,
        }),
        promotions: vec![],
        execution_state: gt::GovernedExecutionState::RuntimeExited,
        review_state: gt::GovernedReviewState::Accepted,
        claims: vec![],
        verifications: vec![],
        supervisor_verdicts: vec![],
        operator_decisions: vec![],
        events: vec![],
        created_at: "2026-09-12T00:00:00Z".to_string(),
        updated_at: "2026-09-12T00:00:00Z".to_string(),
    })
}

fn with_promotion(
    mut task: impulse_ops::governed_task::GovernedTaskRun,
    outcome: impulse_ops::governed_task::GovernedPromotionOutcome,
) -> impulse_ops::governed_task::GovernedTaskRun {
    use impulse_ops::governed_task as gt;
    task.promotions.push(gt::GovernedPromotion {
        id: gt::GovernedRecordId::try_new("promotion-1").expect("promotion id"),
        actor: gt::GovernedActor {
            kind: gt::GovernedActorKind::Operator,
            id: "local-operator-ui".to_string(),
        },
        accepted_revision: "b".repeat(40),
        initial_subject_revision: "a".repeat(40),
        outcome,
        recorded_at: "2026-09-12T00:01:00Z".to_string(),
        based_on_revision: task.revision,
    });
    task.revision += 1;
    task
}

/// Attach the accepted worker claim.
///
/// Folded into the base fixture rather than opt-in: an accepted run cannot
/// exist without the claim its acceptance was granted against, and #52's
/// `governed_outcome_is_promotable` requires one because the promotion producer
/// reads the accepted revision off it. A claimless fixture modelled a state the
/// ledger does not produce.
fn with_accepted_claim(
    mut task: impulse_ops::governed_task::GovernedTaskRun,
) -> impulse_ops::governed_task::GovernedTaskRun {
    use impulse_ops::governed_task as gt;
    task.claims.push(gt::WorkerCompletionClaim {
        id: gt::GovernedRecordId::try_new("claim-1").expect("claim id"),
        actor: gt::GovernedActor {
            kind: gt::GovernedActorKind::Worker,
            id: "builder-01".to_string(),
        },
        summary: "the gate is green".to_string(),
        subject_revision: "b".repeat(40),
        artifact_ids: Vec::new(),
        diff_ref: None,
        loop_report_digest: None,
        loop_report_version: None,
        submitted_at: "2026-09-12T00:00:30Z".to_string(),
        based_on_revision: task.revision,
    });
    task
}

fn operator_board_html(tasks: Vec<impulse_ops::governed_task::GovernedTaskRun>) -> String {
    let snapshot = ProjectOpsSnapshot {
        governed_tasks: tasks,
        ..ProjectOpsSnapshot::default()
    };
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: Default::default(),
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Supervisor,
        },
    );
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

/// The two controls exist, are labelled, and name the staged checkout they act
/// on. Promote is live on an accepted staged run; Discard is not, because
/// ADR-0019 rule 7 keeps an accepted run's checkout until a promotion has been
/// attempted — and the surface must say which rule stopped it.
#[test]
fn test_staged_controls_render_with_a_named_reason_when_a_control_is_unavailable() {
    let html = operator_board_html(vec![staged_governed_task()]);

    assert!(html.contains("data-world-scope=\"staged_authoritative\""));
    assert!(html.contains("/tmp/impulse-rs/.impulse/worktrees/staged-task"));
    assert!(html.contains("data-governed-control=\"promote\""));
    assert!(html.contains("data-governed-control=\"discard\""));
    assert!(html.contains("Promote onto the canonical branch"));
    assert!(html.contains("Discard staged worktree"));
    assert!(
        html.contains("data-governed-control-disabled=\"discard\""),
        "a disabled control must render its reason, not just grey out"
    );
    assert!(html.contains("promote it first, then discard"));
    assert!(
        !html.contains("data-governed-control-disabled=\"promote\""),
        "an accepted staged run with an active worktree is promotable"
    );
    assert!(
        !html.contains("data-governed-control=\"discard-confirmation\""),
        "the discard confirmation is armed by a click, never rendered up front"
    );
}

/// A non-staged run has nothing to promote or reclaim, so the section is absent
/// rather than a wall of disabled buttons.
#[test]
fn test_an_authoritative_run_renders_no_staged_controls() {
    let mut authoritative = staged_governed_task();
    authoritative.world_scope = impulse_ops::governed_task::WorldScope::Authoritative;
    authoritative.staged_worktree = None;
    let html = operator_board_html(vec![authoritative]);

    assert!(!html.contains("data-governed-control=\"promote\""));
    assert!(!html.contains("data-governed-control=\"discard\""));
    assert!(!html.contains("data-world-scope=\"staged_authoritative\""));
}

/// ADR-0019 rule 6. A blocked promotion is rendered as a typed execution fact
/// carrying the canonical head and a per-reason remedy, and the run stays
/// actionable — Promote is still live for the retry the remedy describes.
#[test]
fn test_a_blocked_promotion_renders_as_an_execution_fact_with_its_remedy() {
    use impulse_ops::governed_task as gt;

    for (reason, slug, expected_remedy_fragment) in [
        (
            gt::PromotionBlockedReason::CanonicalHeadMoved,
            "canonical_head_moved",
            "retry the promotion",
        ),
        (
            gt::PromotionBlockedReason::DetachedHead,
            "detached_head",
            "Check out the branch this work belongs on",
        ),
        (
            gt::PromotionBlockedReason::ConcurrentBranchUpdate,
            "concurrent_branch_update",
            "Nothing was written.",
        ),
        (
            gt::PromotionBlockedReason::RepositoryConfigChanged {
                component: gt::SharedConfigComponent::InfoAttributes,
            },
            "repository_config_changed",
            ".git/info/attributes",
        ),
        (
            gt::PromotionBlockedReason::RepositoryConfigUnpinned,
            "repository_config_unpinned",
            "re-materialize it",
        ),
    ] {
        let blocked = with_promotion(
            staged_governed_task(),
            gt::GovernedPromotionOutcome::PromotionBlocked {
                canonical_head: "c".repeat(40),
                reason,
            },
        );
        let html = operator_board_html(vec![blocked]);

        assert!(
            html.contains(&format!("data-promotion-blocked-reason=\"{slug}\"")),
            "{slug} must be rendered as a typed banner"
        );
        assert!(
            html.contains(&"c".repeat(40)),
            "{slug} must show the canonical head the operator has to reconcile"
        );
        assert!(
            html.contains(expected_remedy_fragment),
            "{slug} must carry its own remedy line"
        );
        assert!(
            html.contains("The run stays accepted and the staged worktree stays active."),
            "{slug} must not read as a failed run"
        );
        assert!(
            !html.contains("data-governed-control-disabled=\"promote\""),
            "{slug} leaves the run retryable"
        );
        assert!(
            html.contains("data-review-state=\"accepted\""),
            "{slug} must not change the review state the card reports"
        );
    }
}

/// A successful promotion is final: the run is promoted at most once, and the
/// checkout it used becomes reclaimable.
#[test]
fn test_a_promoted_run_offers_discard_and_no_longer_offers_promote() {
    let promoted = with_promotion(
        staged_governed_task(),
        impulse_ops::governed_task::GovernedPromotionOutcome::Promoted {
            promoted_revision: "b".repeat(40),
        },
    );
    let html = operator_board_html(vec![promoted.clone()]);

    assert!(html.contains("data-governed-control-disabled=\"promote\""));
    assert!(html.contains("promoted at most once"));
    assert!(!html.contains("data-governed-control-disabled=\"discard\""));
    assert!(!html.contains("data-promotion-blocked-reason"));
    assert_eq!(
        impulse_desktop::ui::blocked_promotion_notice(&promoted),
        None,
        "a successful promotion is not something to warn about"
    );
}

/// The enable decision is the shared `impulse_ops` predicate and nothing else,
/// so the cockpit cannot offer a control the daemon would refuse — or withhold
/// one it would accept.
#[test]
fn test_control_states_track_the_shared_impulse_ops_predicates() {
    use impulse_desktop::ui::{discard_control_state, promote_control_state};
    use impulse_ops::governed_task as gt;
    use impulse_ops::governed_wiring::{
        governed_outcome_is_promotable, staged_worktree_is_discardable,
    };

    let mut candidates = vec![staged_governed_task()];
    for review_state in [
        gt::GovernedReviewState::AwaitingClaim,
        gt::GovernedReviewState::AwaitingOperator,
        gt::GovernedReviewState::Rejected,
        gt::GovernedReviewState::Escalated,
        gt::GovernedReviewState::Accepted,
    ] {
        let mut task = staged_governed_task();
        task.review_state = review_state;
        candidates.push(task);
    }
    candidates.push(with_promotion(
        staged_governed_task(),
        gt::GovernedPromotionOutcome::Promoted {
            promoted_revision: "b".repeat(40),
        },
    ));
    candidates.push(with_promotion(
        staged_governed_task(),
        gt::GovernedPromotionOutcome::PromotionBlocked {
            canonical_head: "c".repeat(40),
            reason: gt::PromotionBlockedReason::DetachedHead,
        },
    ));
    let mut launch_failed = staged_governed_task();
    launch_failed.execution_state = gt::GovernedExecutionState::LaunchFailed;
    launch_failed.review_state = gt::GovernedReviewState::AwaitingClaim;
    candidates.push(launch_failed);

    for task in candidates {
        let promote = promote_control_state(&task);
        assert_eq!(
            promote.is_enabled(),
            governed_outcome_is_promotable(&task),
            "promote control disagreed with the daemon predicate for {:?}/{:?}",
            task.review_state,
            task.execution_state
        );
        if !promote.is_enabled() {
            assert!(
                promote.reason().is_some_and(|reason| !reason.is_empty()),
                "a disabled promote control must name its reason"
            );
        }

        let discard = discard_control_state(&task);
        let daemon_would_discard =
            staged_worktree_is_discardable(&task) && task.active_staged_worktree().is_some();
        assert_eq!(
            discard.is_enabled(),
            daemon_would_discard,
            "discard control disagreed with the daemon predicate for {:?}/{:?}",
            task.review_state,
            task.execution_state
        );
        if !discard.is_enabled() {
            assert!(
                discard.reason().is_some_and(|reason| !reason.is_empty()),
                "a disabled discard control must name its reason"
            );
        }
    }
}

/// ADR-0019's Consequences: the surface offering a discard must say what it
/// costs and show the OID, and it must do so *before* the request is sent. The
/// notice is computed from the task through the same predicate that fills the
/// acknowledgement's `unreferenced_accepted_commit`.
#[test]
fn test_the_discard_cost_notice_names_the_unreferenced_accepted_commit() {
    use impulse_desktop::ui::discard_cost_notice;
    use impulse_ops::governed_task as gt;

    let blocked = with_promotion(
        staged_governed_task(),
        gt::GovernedPromotionOutcome::PromotionBlocked {
            canonical_head: "c".repeat(40),
            reason: gt::PromotionBlockedReason::CanonicalHeadMoved,
        },
    );
    let notice = discard_cost_notice(&blocked).expect("a blocked run's commit has only one ref");
    assert!(notice.contains(&"b".repeat(40)), "the OID must be shown");
    assert!(
        notice.contains("drops its only ref"),
        "the cost must be stated, got: {notice}"
    );
    assert!(
        notice.contains("git cat-file -p"),
        "recovery must be actionable, not just described"
    );
    assert_eq!(
        impulse_ops::governed_wiring::unreferenced_accepted_commit_on_discard(&blocked),
        Some("b".repeat(40).as_str()),
        "the notice must be derived from the same predicate that fills the acknowledgement"
    );

    let promoted = with_promotion(
        staged_governed_task(),
        gt::GovernedPromotionOutcome::Promoted {
            promoted_revision: "b".repeat(40),
        },
    );
    assert_eq!(
        discard_cost_notice(&promoted),
        None,
        "a promoted commit is on the canonical branch and costs nothing to discard"
    );

    let mut rejected = staged_governed_task();
    rejected.review_state = gt::GovernedReviewState::Rejected;
    assert_eq!(discard_cost_notice(&rejected), None);
}

/// Both controls reach the host through the same named bridge entry points the
/// JS bootstrap installs; a rename on either side is a silent dead button.
#[test]
fn test_staged_control_bridge_scripts_target_the_installed_bridge_entry_points() {
    use impulse_desktop::ui::{governed_discard_bridge_script, governed_promotion_bridge_script};
    use impulse_ops::governed_task as gt;

    let promote =
        governed_promotion_bridge_script(&impulse_ops::governed_wiring::GovernedPromotionRequest {
            request_id: gt::GovernedRequestId::try_new("ui-promote-1").expect("request id"),
            project_id: "impulse-rs".to_string(),
            task_id: gt::GovernedTaskId::try_new("staged-task").expect("task id"),
            expected_revision: 7,
        });
    assert!(promote.contains("bridge.promoteGovernedOutcome"));
    assert!(promote.contains("\"expected_revision\":7"));
    assert!(promote.contains("degraded"));

    let discard = governed_discard_bridge_script(
        &impulse_ops::governed_wiring::GovernedStagedWorktreeDiscardRequest {
            request_id: gt::GovernedRequestId::try_new("ui-discard-1").expect("request id"),
            project_id: "impulse-rs".to_string(),
            task_id: gt::GovernedTaskId::try_new("staged-task").expect("task id"),
            expected_revision: 7,
            reason: "the checkout is finished with".to_string(),
        },
    );
    assert!(discard.contains("bridge.discardGovernedStagedWorktree"));
    assert!(discard.contains("the checkout is finished with"));

    let bootstrap = desktop_event_bridge_script();
    assert!(bootstrap.contains("promoteGovernedOutcome"));
    assert!(bootstrap.contains("discardGovernedStagedWorktree"));
    assert!(bootstrap.contains("governed_outcome_promote"));
    assert!(bootstrap.contains("governed_staged_worktree_discard"));
    assert!(bootstrap.contains("governed_promotion_failed"));
    assert!(bootstrap.contains("governed_discard_failed"));
}

// ───────────────────────── review round 1 regressions ─────────────────────────

/// **P1.** An accepted staged run whose pin is `Unknown` — the pre-pin
/// population `RepositoryConfigUnpinned` exists for — is discardable through the
/// unknown-pin short-circuit with *zero* promotion attempts. The confirmation
/// used to tell the operator "nothing here was accepted and left unpromoted"
/// right before an irreversible action that stranded a commit.
#[test]
fn test_an_unpinned_accepted_run_names_the_commit_a_discard_would_strand() {
    use impulse_desktop::ui::{
        discard_control_state, discard_cost_notice, discard_reassurance_notice,
    };
    use impulse_ops::governed_task as gt;

    let mut unpinned = staged_governed_task();
    if let Some(staged) = unpinned.staged_worktree.as_mut() {
        staged.shared_config_digest = gt::SharedRepositoryConfigPin::Unknown;
    }
    assert_eq!(unpinned.review_state, gt::GovernedReviewState::Accepted);
    assert!(
        unpinned.promotions.is_empty(),
        "this reaches discard with no promotion attempt at all"
    );
    assert!(
        discard_control_state(&unpinned).is_enabled(),
        "an unpinned worktree can never be promoted, so discard is the only way forward"
    );

    let cost = discard_cost_notice(&unpinned)
        .expect("an accepted, unpromoted run always costs its commit");
    assert!(
        cost.contains(&"b".repeat(40)),
        "the OID must be shown: {cost}"
    );
    assert!(cost.contains("git cat-file -p"));
    assert_eq!(
        discard_reassurance_notice(&unpinned),
        None,
        "a run with a real cost must never also carry a reassurance"
    );
}

/// The reassurance sentence must never claim "nothing was accepted" about an
/// accepted run, in any shape — including the claimless record the state layer
/// should not produce.
#[test]
fn test_no_accepted_run_is_ever_told_that_nothing_was_accepted() {
    use impulse_desktop::ui::discard_reassurance_notice;
    use impulse_ops::governed_task as gt;

    let accepted = staged_governed_task();

    let mut claimless = accepted.clone();
    claimless.claims.clear();
    let claimless_notice =
        discard_reassurance_notice(&claimless).expect("a claimless accepted run still needs words");
    assert!(
        !claimless_notice.contains("never accepted")
            && !claimless_notice.contains("nothing here was accepted"),
        "an accepted run must not be told nothing was accepted, got: {claimless_notice}"
    );
    assert!(
        claimless_notice.contains("Check the staged checkout's HEAD"),
        "with no OID to show, the surface must admit it rather than reassure: {claimless_notice}"
    );

    let promoted = with_promotion(
        accepted.clone(),
        gt::GovernedPromotionOutcome::Promoted {
            promoted_revision: "b".repeat(40),
        },
    );
    let promoted_notice =
        discard_reassurance_notice(&promoted).expect("a promoted run genuinely costs nothing");
    assert!(promoted_notice.contains("already on the canonical branch"));

    let mut rejected = accepted;
    rejected.review_state = gt::GovernedReviewState::Rejected;
    rejected.claims.clear();
    assert!(discard_reassurance_notice(&rejected)
        .expect("a rejected run costs nothing")
        .contains("never accepted"));
}

/// **P2.** `pending_rerun_reason` and `unreferenced_accepted_commit` exist only
/// on the acknowledgement — the `ops_update` the card waits for carries neither
/// — so the bridge forwards them into the banner channel or they are lost.
#[test]
fn test_ack_only_facts_reach_the_banner_channel() {
    use impulse_desktop::ui::{GOVERNED_RERUN_PENDING_STATUS, GOVERNED_UNREFERENCED_COMMIT_STATUS};

    let bootstrap = desktop_event_bridge_script();
    assert!(
        bootstrap.contains("ack?.pending_rerun_reason"),
        "the promotion bridge must read the ack-only rerun reason"
    );
    assert!(
        bootstrap.contains(GOVERNED_RERUN_PENDING_STATUS),
        "and forward it under the status the banner knows"
    );
    assert!(
        bootstrap.contains("ack?.unreferenced_accepted_commit"),
        "the discard bridge must read the ack-only stranded commit"
    );
    assert!(bootstrap.contains(GOVERNED_UNREFERENCED_COMMIT_STATUS));

    // Neither reads as a failed host call.
    let rerun = BridgeStatusUpdate {
        status: GOVERNED_RERUN_PENDING_STATUS.to_string(),
        reason: Some("a previous promotion producer was interrupted".to_string()),
    };
    assert!(rerun.headline().contains("redoing an interrupted producer"));
    assert!(!rerun.headline().contains("Host call failed"));

    let stranded = BridgeStatusUpdate {
        status: GOVERNED_UNREFERENCED_COMMIT_STATUS.to_string(),
        reason: Some(format!(
            "Accepted commit {} was never promoted",
            "b".repeat(40)
        )),
    };
    assert!(stranded
        .headline()
        .contains("only ref to an accepted commit"));
}

/// Nit: a stale revision means the board is looking at a task the daemon has
/// already moved past. "Host call failed: governed promotion" sends the operator
/// hunting for a transport problem that does not exist.
#[test]
fn test_a_revision_conflict_reads_as_a_stale_board_not_a_transport_failure() {
    let conflict = BridgeStatusUpdate {
        status: "governed_promotion_failed".to_string(),
        reason: Some("governed task revision conflict: expected 7, current 9".to_string()),
    };
    let headline = conflict.headline();
    assert!(headline.contains("Board is out of date"), "got: {headline}");
    assert!(headline.contains("refresh and retry"));
    assert!(!headline.contains("Host call failed"));
}

/// Nit: the desktop launch path never declares a world scope, so every
/// registration it builds is `Authoritative`. That is precisely why
/// `RegisterGovernedTask` is absent from the desktop's operator-class list —
/// the daemon gates registration only for a staged scope.
#[test]
fn test_every_desktop_registration_is_authoritative_scoped() {
    use impulse_ops::governed_task as gt;

    // Structural: the launch path never calls the builder's scope setter, so
    // the registration cannot be anything but the default. Checked against the
    // source so that adding a staged launch trips this test rather than
    // silently shipping an unauthenticated staged registration.
    let runtime_source = include_str!("../src/runtime.rs");
    assert!(
        !runtime_source.contains(".world_scope("),
        "the desktop declares a world scope now; RegisterGovernedTask must join \
         UnixDaemonOpsClient::requires_operator_class at the same time, because the daemon \
         gates staged-scope registration as operator-class"
    );

    // And the default that therefore applies is `Authoritative`.
    let registration = gt::GovernedTaskRegistration::builder(
        "request-1".to_string(),
        "task-1".to_string(),
        "impulse-rs".to_string(),
        "/tmp/impulse-rs".to_string(),
        "Launch a governed agent".to_string(),
        "builder-01".to_string(),
        "codex",
    )
    .build()
    .expect("a minimal registration builds");
    assert_eq!(registration.world_scope, gt::WorldScope::Authoritative);
}

// ───────────────────── review round 3: durable ack notices ─────────────────────

fn discard_ack_message(task_id: &str, commit: &str) -> DesktopBridgeMessage {
    DesktopBridgeMessage {
        kind: "governed_ack".to_string(),
        payload: json!({
            "task_id": task_id,
            "kind": "discard_stranded_commit",
            "commit": commit,
            "discarded_root": "/tmp/impulse-rs/.impulse/worktrees/staged-task",
            "detail": format!(
                "Accepted commit {commit} was never promoted, so removing the staged worktree \
                 dropped its only ref. `git cat-file -p {commit}` recovers it from the reflog \
                 until that expires."
            ),
        }),
    }
}

fn board_html_with_acks(
    tasks: Vec<impulse_ops::governed_task::GovernedTaskRun>,
    acks: std::collections::BTreeMap<String, impulse_desktop::ui::GovernedAckNotice>,
) -> String {
    let snapshot = ProjectOpsSnapshot {
        governed_tasks: tasks,
        ..ProjectOpsSnapshot::default()
    };
    let mut vdom = VirtualDom::new_with_props(
        DesktopShellWithSnapshot,
        DesktopShellWithSnapshotProps {
            snapshot,
            runtime_agents: Vec::new(),
            agent_platforms: Vec::new(),
            workspaces: Vec::new(),
            mcp_tools: Vec::new(),
            last_invocations: Vec::new(),
            review_queue: Vec::new(),
            bridge_status: None,
            daemon_ops_status: None,
            governed_acks: acks,
            on_dismiss_governed_ack: None,
            initial_view: DesktopView::Supervisor,
        },
    );
    vdom.rebuild_in_place();
    dioxus_ssr::render(&vdom)
}

/// **P2.** The reducer files an acknowledgement notice per task, and an
/// `ops_update` — which lands immediately after every discard — must not remove
/// it. The transient `bridge_status` slot is reset by every successfully
/// reduced message, which is exactly why the OID cannot live only there.
#[test]
fn test_an_ops_update_after_a_discard_does_not_remove_the_stranded_commit_notice() {
    use impulse_desktop::ui::GovernedAckNotice;

    let commit = "b".repeat(40);
    let notice = GovernedAckNotice::parse(&discard_ack_message("staged-task", &commit))
        .expect("a well-formed governed_ack parses");
    assert_eq!(notice.commit.as_deref(), Some(commit.as_str()));

    let mut acks = std::collections::BTreeMap::new();
    acks.insert(notice.task_id.clone(), notice);

    // The reducer that handles `ops_update` cannot touch this map: it takes a
    // `DesktopBridgeStateMut` that does not contain it, and the shell files
    // governed acks on a separate branch that `continue`s before reduction.
    let mut snapshot = ProjectOpsSnapshot::default();
    let (mut agents, mut platforms, mut spaces, mut tools, mut queue, mut invocations) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    apply_desktop_bridge_message(
        DesktopBridgeStateMut::new(
            &mut snapshot,
            &mut agents,
            &mut platforms,
            &mut spaces,
            &mut tools,
            &mut queue,
            &mut invocations,
        ),
        DesktopBridgeMessage {
            kind: "ops_update".to_string(),
            payload: serde_json::to_value(ProjectOpsSnapshot::default()).unwrap(),
        },
    )
    .expect("an ops_update reduces");
    assert_eq!(acks.len(), 1, "reducing an ops_update files no ack changes");

    // And it is still rendered on the card afterwards.
    let html = board_html_with_acks(vec![staged_governed_task()], acks);
    assert!(html.contains("data-governed-ack=\"discard_stranded_commit\""));
    assert!(
        html.contains(&commit),
        "the OID must still be on the card after an ops_update"
    );
    assert!(html.contains("git cat-file -p"));
    assert!(
        html.contains("data-governed-control=\"dismiss-ack\""),
        "the only way to clear it is the operator's own Dismiss"
    );
}

/// **P2.** A card remount — which every revision bump causes, because the card
/// is keyed `id:revision` — must not take the notice with it. The notice lives
/// on the board, above that key.
#[test]
fn test_the_stranded_commit_notice_survives_a_card_remount() {
    use impulse_desktop::ui::GovernedAckNotice;

    let commit = "b".repeat(40);
    let notice = GovernedAckNotice::parse(&discard_ack_message("staged-task", &commit)).unwrap();
    let mut acks = std::collections::BTreeMap::new();
    acks.insert(notice.task_id.clone(), notice);

    let before = board_html_with_acks(vec![staged_governed_task()], acks.clone());
    assert!(before.contains("data-review-state=\"accepted\""));
    assert!(before.contains(&commit));

    // Same task, new daemon revision: the card is a different keyed element.
    let mut bumped = staged_governed_task();
    bumped.revision += 1;
    let after = board_html_with_acks(vec![bumped], acks);
    assert!(
        after.contains(&commit),
        "a revision bump remounts the card; the notice is board-owned and must survive"
    );
    assert!(after.contains("data-governed-ack-task=\"staged-task\""));
}

/// **P2.** Dismissal is the one thing that clears it, and it clears only the
/// task it was filed against.
#[test]
fn test_dismissal_clears_only_the_dismissed_task_s_notice() {
    use impulse_desktop::ui::GovernedAckNotice;

    // Deliberately NOT the fixture's claim revision. That OID legitimately
    // appears in the discard *cost* notice for an accepted, unpromoted run, so
    // reusing it here would make "the OID is gone" unprovable -- the assertion
    // would fail for a reason that is the P1 fix working correctly.
    let commit = "e".repeat(40);
    let other = "c".repeat(40);
    let mut acks = std::collections::BTreeMap::new();
    for (task_id, oid) in [("staged-task", &commit), ("other-task", &other)] {
        let notice = GovernedAckNotice::parse(&discard_ack_message(task_id, oid)).unwrap();
        acks.insert(notice.task_id.clone(), notice);
    }

    let both = board_html_with_acks(vec![staged_governed_task()], acks.clone());
    assert!(both.contains(&commit));

    // The dismissal the shell performs is a removal keyed by task id.
    acks.remove("staged-task");
    let dismissed = board_html_with_acks(vec![staged_governed_task()], acks.clone());
    assert!(
        !dismissed.contains("data-governed-ack=\"discard_stranded_commit\""),
        "dismissing removes the notice"
    );
    assert!(
        !dismissed.contains(&commit),
        "and the OID with it: the operator said they were done with it"
    );
    assert_eq!(
        acks.len(),
        1,
        "the other task's notice is untouched by this dismissal"
    );
}

/// The notice crosses the eval channel as JSON, and a payload that cannot be
/// filed against a card is dropped rather than keyed under an empty string.
#[test]
fn test_governed_ack_notice_round_trips_and_rejects_unfilable_payloads() {
    use impulse_desktop::ui::{GovernedAckKind, GovernedAckNotice};

    let notice = GovernedAckNotice::parse(&discard_ack_message("staged-task", &"b".repeat(40)))
        .expect("parses");
    let json = serde_json::to_string(&notice).expect("serialize");
    let recovered: GovernedAckNotice = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(recovered, notice);
    assert_eq!(recovered.kind, GovernedAckKind::DiscardStrandedCommit);
    assert_eq!(recovered.kind.as_str(), "discard_stranded_commit");

    // A promotion rerun notice carries no commit.
    let rerun = GovernedAckNotice::parse(&DesktopBridgeMessage {
        kind: "governed_ack".to_string(),
        payload: json!({
            "task_id": "staged-task",
            "kind": "promotion_rerun_pending",
            "detail": "a previous promotion producer was interrupted",
        }),
    })
    .expect("a rerun notice parses without a commit");
    assert_eq!(rerun.commit, None);
    assert!(rerun.headline().contains("redoing an interrupted producer"));

    for unfilable in [
        json!({"task_id": "", "kind": "discard_stranded_commit", "detail": "x"}),
        json!({"task_id": "  ", "kind": "discard_stranded_commit", "detail": "x"}),
        json!({"task_id": "staged-task", "kind": "discard_stranded_commit", "detail": "   "}),
        json!({"task_id": "staged-task", "kind": "not_a_kind", "detail": "x"}),
    ] {
        assert_eq!(
            GovernedAckNotice::parse(&DesktopBridgeMessage {
                kind: "governed_ack".to_string(),
                payload: unfilable.clone(),
            }),
            None,
            "an unfilable payload must be dropped, not keyed under an empty task: {unfilable}"
        );
    }

    assert_eq!(
        GovernedAckNotice::parse(&DesktopBridgeMessage {
            kind: "ops_update".to_string(),
            payload: json!({"task_id": "staged-task", "kind": "discard_stranded_commit", "detail": "x"}),
        }),
        None,
        "only a governed_ack message files a notice"
    );
}

/// The bridge emits the durable record *and* keeps the transient echo.
#[test]
fn test_the_bridge_emits_a_durable_ack_alongside_the_transient_banner() {
    let bootstrap = desktop_event_bridge_script();
    assert!(bootstrap.contains("forward(\"governed_ack\""));
    assert!(bootstrap.contains("\"discard_stranded_commit\""));
    assert!(bootstrap.contains("\"promotion_rerun_pending\""));
    // The banner echo is still there; it is the transient half of the pair.
    assert!(bootstrap.contains("governed_discard_unreferenced_commit"));
    assert!(bootstrap.contains("governed_promotion_rerun_pending"));
    // The notice is filed against the task the request named.
    assert!(bootstrap.contains("request?.task_id"));
}

/// The staged-configuration refusal is typed now (#52), so the surface builds
/// its notice from `StagedConfigRefusalReason` and renders the daemon's own
/// `remedy` verbatim. That is the point of the ack carrying `reason.remedy()`:
/// there is exactly one copy of the mapping, and it is not this one.
#[test]
fn test_the_refusal_notice_is_built_from_the_typed_reason_and_quotes_the_daemon_remedy() {
    use impulse_desktop::ui::staged_config_refusal_notice;
    use impulse_ops::governed_task::SharedConfigComponent;
    use impulse_ops::governed_wiring::{GovernedStagedConfigRefusalAck, StagedConfigRefusalReason};

    for reason in [
        StagedConfigRefusalReason::Unpinned,
        StagedConfigRefusalReason::Changed {
            component: SharedConfigComponent::RepositoryConfig,
        },
        StagedConfigRefusalReason::UnsupportedSubmodules {
            path: "/tmp/impulse-rs/.gitmodules".to_string(),
        },
    ] {
        // Build the ack the way the daemon does, so the remedy under test is
        // the one that actually crosses the wire.
        let ack = GovernedStagedConfigRefusalAck::new(staged_governed_task(), reason.clone());
        assert!(ack.refused, "the ack discriminates on one field");
        let notice = staged_config_refusal_notice(&ack.reason, &ack.remedy);

        assert_eq!(
            notice.remedy, ack.remedy,
            "the remedy is rendered verbatim, never re-derived locally"
        );
        assert!(
            notice.headline.contains("Nothing was touched."),
            "a refusal never reads as a run failure: {}",
            notice.headline
        );
        assert!(
            !notice.remedy.contains("  "),
            "the wire remedy must not carry a run of interior spaces: {:?}",
            notice.remedy
        );
    }

    // Each reason names the specific thing the operator has to look at.
    let changed = StagedConfigRefusalReason::Changed {
        component: SharedConfigComponent::InfoAttributes,
    };
    let ack = GovernedStagedConfigRefusalAck::new(staged_governed_task(), changed);
    let notice = staged_config_refusal_notice(&ack.reason, &ack.remedy);
    assert!(
        notice.headline.contains(".git/info/attributes"),
        "a changed pin must name the component, got: {}",
        notice.headline
    );

    let submodules = StagedConfigRefusalReason::UnsupportedSubmodules {
        path: "/tmp/impulse-rs/.gitmodules".to_string(),
    };
    let ack = GovernedStagedConfigRefusalAck::new(staged_governed_task(), submodules);
    let notice = staged_config_refusal_notice(&ack.reason, &ack.remedy);
    assert!(
        notice.headline.contains("/tmp/impulse-rs/.gitmodules"),
        "an unsupported-submodule refusal must name the path, got: {}",
        notice.headline
    );
}
