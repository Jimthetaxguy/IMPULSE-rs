use std::collections::HashMap;

use dioxus::prelude::*;
use impulse_desktop::{
    AgentWriteRequest, DesktopCommandRouter, DesktopShell, InMemoryTerminalBridge,
    NativeIslandKind, NativeIslandRequest, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalWriteRequest,
};
use serde_json::json;

#[test]
fn test_dioxus_shell_renders_five_panel_layout_without_egui() {
    let mut vdom = VirtualDom::new(DesktopShell);
    vdom.rebuild_in_place();

    let html = dioxus_ssr::render(&vdom);

    assert!(html.contains("top-bar"));
    assert!(html.contains("left-rail"));
    assert!(html.contains("terminal-stage"));
    assert!(html.contains("right-inspector"));
    assert!(html.contains("event-strip"));
    assert!(html.contains("agent-pool"));
    assert!(html.contains("xterm.js terminal mount"));
    assert!(html.contains("terminal-pane-codex"));
    assert!(html.contains("agent_runtime_update stream pending"));
    assert!(html.contains("data-pty-owner=\"rust-backend\""));
    assert!(!html.contains("egui"));
}

#[test]
fn test_terminal_interop_serializes_xterm_input_as_byte_array() {
    let script = impulse_desktop::ui::terminal_interop_script();

    assert!(script.contains("const encoder = new TextEncoder();"));
    assert!(script.contains("const encodeInput = (data) => Array.from(encoder.encode(data));"));
    assert!(script.contains(
        r#"invoke("agent_write", { request: { agent_id: agentId, data: encodeInput(data) } });"#
    ));
    assert!(!script.contains("data } });"));
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
