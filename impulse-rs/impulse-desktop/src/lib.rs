//! Hybrid Dioxus desktop shell boundary for Impulse.
//!
//! Dioxus owns the product interface. Tauri owns native process and IPC edges.
//! Native macOS islands are narrow capability bridges and never own Impulse
//! session, memory, terminal, or artifact state.

pub mod bridge;
pub mod mcp;
pub mod native;
pub mod runtime;
pub mod tauri_commands;
pub mod theme;
pub mod ui;
pub mod workspace;

pub use bridge::{
    DesktopBridgeError, DesktopCommandRouter, InMemoryTerminalBridge, TerminalBridge,
    TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest, TerminalResizeRequest,
    TerminalSessionResponse, TerminalWriteRequest,
};
pub use mcp::{
    AgentSpawnTool, AgentWriteTool, ListAgentsTool, ListWorkspacesTool, McpContext, McpError,
    McpInvocation, McpTool, McpToolRegistry, PassthroughMcpTool, SearchMemoryTool,
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
pub use theme::{format_count, status_dot_class, status_label};
pub use ui::{DesktopShell, DesktopShellWithSnapshot, DesktopShellWithSnapshotProps};
pub use workspace::{WorkspaceEntry, WorkspaceRegistry, WorkspaceRegistryError};
