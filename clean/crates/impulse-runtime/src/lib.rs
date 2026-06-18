//! `impulse-runtime` — the PTY orchestrator.
//!
//! Owns the live subprocess for each harness session, captures PTY output,
//! dispatches tool calls, and exposes a tokio-friendly API to the MCP server
//! and Dioxus host.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────┐    spawn   ┌─────────────────┐
//! │ Orchestrator │ ─────────► │ Backend Adapter │
//! └──────┬───────┘            └────────┬────────┘
//!        │ events                      │ portable-pty
//!        ▼                             ▼
//! ┌──────────────┐            ┌─────────────────┐
//! │ Event bus    │ ◄────────  │  PTY child      │
//! │ (broadcast)  │            │  (claude, codex)│
//! └──────────────┘            └─────────────────┘
//! ```
//!
//! Sessions are cheap to create (a PTY is a few KB of kernel state) and
//! each session gets its own mpsc channel for inbound events.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod backend;
pub mod orchestrator;
pub mod pty;
pub mod session;
pub mod tool_dispatch;

pub use backend::{
    BackendAdapter, ClaudeCodeAdapter, CodexAdapter, GeminiCliAdapter, GenericCliAdapter,
    OpenCodeAdapter,
};
pub use orchestrator::{Orchestrator, OrchestratorBuilder, OrchestratorConfig, OrchestratorHandle};
pub use pty::{PtyHandle, PtyOutput, PtySpawnSpec};
pub use session::{Session, SessionHandle};
pub use tool_dispatch::{DispatchResult, ToolDispatcher, ToolExecutionError};

/// Re-exports of common types for downstream consumers.
pub mod prelude {
    pub use crate::backend::BackendAdapter;
    pub use crate::orchestrator::{Orchestrator, OrchestratorBuilder, OrchestratorHandle};
    pub use crate::pty::{PtyHandle, PtyOutput, PtySpawnSpec};
    pub use crate::session::{Session, SessionHandle};
    pub use crate::tool_dispatch::{DispatchResult, ToolDispatcher, ToolExecutionError};

    pub use impulse_contracts::{
        AgentPlatformKind, BackendDescriptor, BackendRegistry, CliSubprocessSpec, ConcurrencyClass,
        OrchestratorEvent, PaneId, PtyChunk, PtyStream, RiskClass, SessionId, SessionPhase,
        SessionState, ToolDescriptor, ToolEvent, ToolOutcome, ToolSpec, WorkspaceHandle,
        WorkspaceId, WorkspacePath, WorkspaceSummary,
    };
}
