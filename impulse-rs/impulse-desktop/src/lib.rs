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
pub mod views;
pub mod workspace;

pub use bridge::{
    DesktopBridgeError, DesktopCommandRouter, InMemoryTerminalBridge, TerminalBridge,
    TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest, TerminalResizeRequest,
    TerminalSessionResponse, TerminalWriteRequest,
};
pub use mcp::{
    AgentSpawnTool, AgentWriteTool, ListAgentsTool, ListWorkspacesTool, McpContext, McpError,
    McpInvocation, McpTool, McpToolRegistry, PassthroughMcpTool, ProjectContextTool,
    ReviewDecision, ReviewDecisionTool, ReviewInjectionTool, ReviewQueueItem, ReviewQueueStatus,
    SearchMemoryTool,
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
pub use tauri_commands::{RegisterWorkspaceRequest, ReviewDecisionRequest};
pub use theme::{
    artifact_status_class, artifact_status_label, format_count, severity_class, status_dot_class,
    status_label, usage_meter_pct,
};
pub use ui::{DesktopShell, DesktopShellWithSnapshot, DesktopShellWithSnapshotProps};
pub use views::{
    ArtifactsView, ArtifactsViewProps, DesktopView, MemoryView, MemoryViewProps, ReviewView,
    ReviewViewProps, ShellIntent, SupervisorView, SupervisorViewProps,
};
pub use workspace::{WorkspaceEntry, WorkspaceRegistry, WorkspaceRegistryError};
