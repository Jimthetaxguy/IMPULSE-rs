//! Hybrid Dioxus desktop shell boundary for Impulse.
//!
//! Dioxus owns the product interface. Tauri owns native process and IPC edges.
//! Native macOS islands are narrow capability bridges and never own Impulse
//! session, memory, terminal, or artifact state.

pub mod bridge;
pub mod native;
pub mod runtime;
pub mod tauri_commands;
pub mod ui;

pub use bridge::{
    DesktopBridgeError, DesktopCommandRouter, InMemoryTerminalBridge, TerminalBridge,
    TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest, TerminalResizeRequest,
    TerminalSessionResponse, TerminalWriteRequest,
};
pub use native::{
    DefaultNativeIslandHost, NativeIslandHost, NativeIslandKind, NativeIslandRequest,
    NativeIslandResult,
};
pub use runtime::{
    default_builtin_mcp_tools, AgentPlatformKind, AgentRuntimeSnapshot, AgentSpawnRequest,
    AgentWriteRequest, BuiltInMcpTool, DesktopEvent, DesktopEventSink, DesktopRuntime,
    DesktopRuntimeBuilder, LocalSupervisorAction, SupervisorLocalActionRequest, WorkspaceTarget,
};
pub use ui::DesktopShell;
