//! `impulse-contracts` — typed vocabulary shared by every Impulse-RS crate.
//!
//! This crate has **no** I/O, no async, and no platform deps. It defines
//! the wire-format types that the runtime, workspace registry, MCP server,
//! and Dioxus host all exchange.
//!
//! # Modules
//!
//! - [`harness`] — the agents Impulse-RS can drive (Claude Code, Codex, Gemini CLI, OpenCode, generic CLI).
//! - [`session`] — long-lived session identifiers, state, and lifecycle events.
//! - [`event`] — discrete facts the orchestrator emits (PTY bytes, tool invocations, approvals).
//! - [`tool`] — typed tool call descriptors with concurrency classification.
//! - [`workspace`] — pointer at a project root a session is anchored to.
//! - [`error`] — the canonical error enums (one per layer).
//! - [`id`] — strongly-typed newtypes for session, delegation, pane, and workspace ids.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod error;
pub mod event;
pub mod harness;
pub mod id;
pub mod session;
pub mod tool;
pub mod workspace;

pub use error::{ContractsError, ContractsResult};
pub use event::{OrchestratorEvent, PtyChunk, PtyStream, ToolEvent, ToolOutcome};
pub use harness::{
    AgentPlatformKind, BackendCapabilities, BackendDescriptor, BackendRegistry, CliSubprocessSpec,
    HarnessError, HarnessResult,
};
pub use id::{DelegationId, PaneId, SessionId, ToolCallId, WorkspaceId, WorkspacePath};
pub use session::{SessionPhase, SessionState};
pub use tool::{ConcurrencyClass, RiskClass, ToolDescriptor, ToolSpec};
pub use workspace::{WorkspaceHandle, WorkspaceSummary};
