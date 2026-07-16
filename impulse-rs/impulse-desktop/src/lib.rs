//! Dioxus desktop shell boundary for Impulse.
//!
//! Dioxus owns the product interface and host direction. Legacy Tauri-shaped
//! adapters are temporary compatibility edges while Dioxus Desktop launch
//! plumbing lands.
//! Native macOS islands are narrow capability bridges and never own Impulse
//! session, memory, terminal, or artifact state.

pub mod bridge;
pub mod daemon_ops;
pub mod daemon_sidecar;
#[cfg(feature = "desktop-app")]
pub mod desktop_host;
pub mod desktop_shutdown;
pub mod host_bridge;
pub mod host_commands;
pub mod mcp;
pub mod native;
pub mod project_boundary;
pub mod runtime;
pub mod theme;
pub mod ui;
pub mod views;
pub mod workspace;

pub use bridge::{
    DesktopBridgeError, DesktopCommandRouter, InMemoryTerminalBridge, TerminalBridge,
    TerminalCloseRequest, TerminalFocusRequest, TerminalOpenRequest, TerminalResizeRequest,
    TerminalSessionResponse, TerminalWriteRequest,
};
pub use daemon_ops::{
    agent_runtime_from_snapshot, attach_desktop_daemon_ops, DesktopDaemonOpsAttachment,
    DesktopDaemonOpsConfig, DesktopDaemonOpsStartError, DEFAULT_HEARTBEAT_INTERVAL,
};
pub use daemon_sidecar::{
    DesktopDaemonProjectIdentity, DesktopDaemonSidecar, DesktopDaemonSidecarError,
    DesktopDaemonSidecarHandle, DesktopDaemonSidecarMode, DesktopInstanceLease,
    DEFAULT_DAEMON_STARTUP_TIMEOUT,
};
pub use desktop_shutdown::{DesktopShutdownCoordinator, DesktopShutdownReport};
pub use host_commands::{RegisterWorkspaceRequest, ReviewDecisionRequest};
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
pub use project_boundary::{
    DesktopProjectBoundary, DesktopProjectBoundaryConnector, DesktopProjectBoundaryController,
    ProjectMemoryScope, SwitchableDesktopEventSink, SwitchableGovernedTaskGateway,
};
pub use runtime::{
    default_builtin_mcp_tools, AgentPlatformId, AgentRuntimeSnapshot, AgentSpawnRequest,
    AgentWriteRequest, BuiltInMcpTool, DesktopEvent, DesktopEventSink, DesktopRuntime,
    DesktopRuntimeBuilder, GovernedRoutingMetadata, GovernedTaskGateway, LocalSupervisorAction,
    SupervisorLocalActionRequest, WeakDesktopRuntime, WorkspaceTarget,
};
pub use theme::{
    artifact_status_class, artifact_status_label, format_count, severity_class, status_dot_class,
    status_label, usage_meter_pct,
};
pub use ui::{DesktopShell, DesktopShellWithSnapshot, DesktopShellWithSnapshotProps};
pub use views::{ArtifactsView, ArtifactsViewProps, DesktopView, MemoryView, MemoryViewProps};
pub use workspace::{WorkspaceEntry, WorkspaceRegistry, WorkspaceRegistryError};
