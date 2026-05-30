use std::sync::OnceLock;

use crate::bridge::{
    InMemoryTerminalBridge, TerminalBridge, TerminalCloseRequest, TerminalFocusRequest,
    TerminalOpenRequest, TerminalResizeRequest, TerminalSessionResponse, TerminalWriteRequest,
};
use crate::native::{DefaultNativeIslandHost, NativeIslandRequest, NativeIslandResult};
use crate::NativeIslandHost;

static TERMINAL_BRIDGE: OnceLock<InMemoryTerminalBridge> = OnceLock::new();

fn terminal_bridge() -> &'static InMemoryTerminalBridge {
    TERMINAL_BRIDGE.get_or_init(InMemoryTerminalBridge::default)
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn terminal_open(
    request: TerminalOpenRequest,
) -> Result<TerminalSessionResponse, String> {
    terminal_bridge()
        .open(request)
        .map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn terminal_write(request: TerminalWriteRequest) -> Result<(), String> {
    terminal_bridge()
        .write(request)
        .map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn terminal_resize(request: TerminalResizeRequest) -> Result<(), String> {
    terminal_bridge()
        .resize(request)
        .map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn terminal_close(request: TerminalCloseRequest) -> Result<(), String> {
    terminal_bridge()
        .close(request)
        .map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn terminal_focus(request: TerminalFocusRequest) -> Result<(), String> {
    terminal_bridge()
        .focus(request)
        .map_err(|error| error.to_string())
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn native_island_request(
    request: NativeIslandRequest,
) -> Result<NativeIslandResult, String> {
    DefaultNativeIslandHost
        .dispatch(request)
        .map_err(|error| error.to_string())
}
