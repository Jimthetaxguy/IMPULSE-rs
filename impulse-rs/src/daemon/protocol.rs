//! Daemon IPC protocol types — request/response enums and serialization helpers.
//!
//! These types define the wire format for JSON-line communication over the Unix socket.

use serde::{Deserialize, Serialize};

use crate::context_lifecycle::types::ExtractedInsight;

/// Protocol version — increment when adding new request/response variants
/// or making breaking changes to existing ones. GUI checks this on connect
/// and warns if it doesn't match its expected version.
pub const PROTOCOL_VERSION: u32 = 2;

pub(crate) const SOCKET_NAME: &str = "impulse.sock";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    Ping,
    Status,
    CreateSession {
        name: String,
        platform: Option<String>,
    },
    EndSession {
        session_id: String,
        summary: String,
    },
    TrackFile {
        session_id: String,
        file_path: String,
    },
    TrackTool {
        session_id: String,
        tool_name: String,
    },
    GetSession {
        session_id: String,
    },
    ListSessions,
    Chat {
        session_id: String,
        message: String,
        #[serde(default)]
        inject_mode: Option<String>,
        #[serde(default)]
        inject_explain: bool,
    },
    StewardStatus,
    StewardProposals {
        action: String,
        id: Option<String>,
    },
    StewardMemory,
    /// List all available tools (for agent discovery)
    ListTools {
        #[serde(default)]
        category: Option<String>,
    },
    /// Get a tool's descriptor (params, capabilities)
    DescribeTool {
        name: String,
    },
    /// Invoke a tool by name with JSON params
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Export tool schemas in Claude tool-calling format
    ToolSchema,
    /// Fetch the workbench snapshot used by the egui operator console
    GetOpsSnapshot,
    /// Poll for workbench events and a reconciled snapshot
    SubscribeOps {
        #[serde(default)]
        since_seq: Option<u64>,
    },
    /// Publish live terminal telemetry from the egui workbench
    PublishTerminalOps {
        report: impulse_ops::TerminalOpsReport,
    },
    /// Read the effective supervisor permissions for the egui control plane
    GetSupervisorPermissions,
    /// Structured supervisor chat for the egui control plane
    SupervisorChat {
        prompt: String,
        context: Option<String>,
    },
    /// Run a structured supervisor action with daemon-side policy enforcement
    RunSupervisorAction {
        action: impulse_ops::SupervisorAction,
    },
    /// List project-scoped artifacts for the egui workbench
    ListArtifacts {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Get a single project artifact by ID
    GetArtifact {
        artifact_id: String,
    },
    /// Run an artifact action for the egui workbench
    RunArtifactAction {
        artifact_id: String,
        action_id: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Request AI coordination assistance via the Impulse Agent.
    /// When `insights` is provided, they are formatted into a structured
    /// cross-pane context block and prepended to the user prompt.
    AgentAssist {
        prompt: String,
        context: Option<String>,
        /// Extracted insights from the context lifecycle for prompt enrichment.
        #[serde(default)]
        insights: Vec<ExtractedInsight>,
    },
    /// Evaluate an action against guardrail rules
    GuardEvaluate {
        target: String,
        action: String,
    },
    /// List active guardrail rules
    GuardList,
    /// Check if a file is being modified by another session
    CheckConflict {
        session_id: String,
        file_path: String,
    },
    /// Return a detailed internal state snapshot for debugging
    DebugSnapshot,
    /// List registered plugins (context providers + action handlers)
    ListPlugins,
    /// Invoke a named action handler plugin
    InvokePlugin {
        name: String,
        #[serde(default)]
        input: crate::plugin::PluginInput,
    },
    /// Register a delegation detected in agent output (Phase 1B)
    RegisterDelegation {
        spec: crate::delegation::types::DelegationSpec,
        coordinator_pane_id: usize,
        #[serde(default)]
        context_snapshot: String,
    },
    /// Mark a delegation as completed (Phase 1B)
    CompleteDelegation {
        delegation_id: String,
        summary: String,
        #[serde(default)]
        tool_trace: Vec<impulse_ops::ToolInvocationRecord>,
        #[serde(default)]
        diff_summary: Option<impulse_ops::DiffSummary>,
    },
    /// List all tracked delegations (Phase 1B)
    ListDelegations,
    /// Get the agent pool — all sessions grouped by role (Phase 2B)
    GetAgentPool,
    /// Request a code review via the Impulse Agent (Task 23)
    AgentReviewCode {
        file_path: String,
        diff: String,
        #[serde(default)]
        insights: Vec<ExtractedInsight>,
    },
    /// Request error analysis via the Impulse Agent (Task 23)
    AgentAnalyzeError {
        error_text: String,
        context: String,
        #[serde(default)]
        insights: Vec<ExtractedInsight>,
    },
    /// Request a pane activity summary via the Impulse Agent (Task 23)
    AgentSummarizePane {
        pane_id: usize,
        #[serde(default)]
        raw_output: String,
        #[serde(default)]
        insights: Vec<ExtractedInsight>,
    },
    /// Get the conflict resolution history from the ConflictResolver (Task 20)
    GetConflictHistory,
    /// Clear resolved conflicts from the ConflictResolver (Task 20)
    ClearResolvedConflicts,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonResponse {
    Ok {
        result: serde_json::Value,
    },
    Error {
        message: String,
    },
    AgentAssistResult {
        success: bool,
        response: String,
        /// Coordination recommendations (conflicts, errors, delegations) from
        /// `run_full_coordination`. Empty when no insights were provided.
        #[serde(default)]
        recommendations: Vec<crate::agent::coordinator::Recommendation>,
        /// Per-pane insight summaries from `aggregate_pane_summaries`.
        /// Each entry is (pane_label, list_of_summary_lines).
        #[serde(default)]
        pane_summaries: Vec<(String, Vec<String>)>,
    },
    /// Result from specialized agent methods (review_code, analyze_error, summarize_pane).
    AgentSpecializedResult {
        success: bool,
        response: String,
    },
    ConflictCheck {
        has_conflict: bool,
        conflicting_sessions: Vec<String>,
    },
}

// ── Daemon response helpers ───────��─────────────────────────────────────────

/// Serialize a value into `DaemonResponse::Ok`, or return an error response.
pub(crate) fn respond_ok<T: serde::Serialize>(value: &T) -> DaemonResponse {
    match serde_json::to_value(value) {
        Ok(result) => DaemonResponse::Ok { result },
        Err(e) => DaemonResponse::Error {
            message: format!("serialize: {}", e),
        },
    }
}

/// Shorthand for error response.
pub(crate) fn respond_err(msg: impl std::fmt::Display) -> DaemonResponse {
    DaemonResponse::Error {
        message: msg.to_string(),
    }
}

pub(crate) fn request_type_name(req: &DaemonRequest) -> &'static str {
    match req {
        DaemonRequest::Ping => "Ping",
        DaemonRequest::Status => "Status",
        DaemonRequest::CreateSession { .. } => "CreateSession",
        DaemonRequest::EndSession { .. } => "EndSession",
        DaemonRequest::TrackFile { .. } => "TrackFile",
        DaemonRequest::TrackTool { .. } => "TrackTool",
        DaemonRequest::GetSession { .. } => "GetSession",
        DaemonRequest::ListSessions => "ListSessions",
        DaemonRequest::Chat { .. } => "Chat",
        DaemonRequest::StewardStatus => "StewardStatus",
        DaemonRequest::StewardProposals { .. } => "StewardProposals",
        DaemonRequest::StewardMemory => "StewardMemory",
        DaemonRequest::ListTools { .. } => "ListTools",
        DaemonRequest::DescribeTool { .. } => "DescribeTool",
        DaemonRequest::InvokeTool { .. } => "InvokeTool",
        DaemonRequest::ToolSchema => "ToolSchema",
        DaemonRequest::GetOpsSnapshot => "GetOpsSnapshot",
        DaemonRequest::SubscribeOps { .. } => "SubscribeOps",
        DaemonRequest::PublishTerminalOps { .. } => "PublishTerminalOps",
        DaemonRequest::GetSupervisorPermissions => "GetSupervisorPermissions",
        DaemonRequest::SupervisorChat { .. } => "SupervisorChat",
        DaemonRequest::RunSupervisorAction { .. } => "RunSupervisorAction",
        DaemonRequest::ListArtifacts { .. } => "ListArtifacts",
        DaemonRequest::GetArtifact { .. } => "GetArtifact",
        DaemonRequest::RunArtifactAction { .. } => "RunArtifactAction",
        DaemonRequest::AgentAssist { .. } => "AgentAssist",
        DaemonRequest::GuardEvaluate { .. } => "GuardEvaluate",
        DaemonRequest::GuardList => "GuardList",
        DaemonRequest::CheckConflict { .. } => "CheckConflict",
        DaemonRequest::DebugSnapshot => "DebugSnapshot",
        DaemonRequest::ListPlugins => "ListPlugins",
        DaemonRequest::InvokePlugin { .. } => "InvokePlugin",
        DaemonRequest::RegisterDelegation { .. } => "RegisterDelegation",
        DaemonRequest::CompleteDelegation { .. } => "CompleteDelegation",
        DaemonRequest::ListDelegations => "ListDelegations",
        DaemonRequest::GetAgentPool => "GetAgentPool",
        DaemonRequest::AgentReviewCode { .. } => "AgentReviewCode",
        DaemonRequest::AgentAnalyzeError { .. } => "AgentAnalyzeError",
        DaemonRequest::AgentSummarizePane { .. } => "AgentSummarizePane",
        DaemonRequest::GetConflictHistory => "GetConflictHistory",
        DaemonRequest::ClearResolvedConflicts => "ClearResolvedConflicts",
    }
}
