//! Daemon IPC protocol types — request/response enums and serialization helpers.
//!
//! These types define the wire format for JSON-line communication over the Unix socket.

use serde::{Deserialize, Serialize};

use crate::context_lifecycle::types::ExtractedInsight;

/// Protocol version — increment when adding new request/response variants
/// or making breaking changes to existing ones. GUI checks this on connect
/// and warns if it doesn't match its expected version.
pub const PROTOCOL_VERSION: u32 = impulse_ops::DAEMON_PROTOCOL_VERSION;

pub(crate) const SOCKET_NAME: &str = "impulse.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    Ping,
    Status,
    /// Present this daemon run's operator capability (ADR-0018).
    ///
    /// Connection-scoped: a successful presentation raises *this* connection to
    /// operator class and has no effect on any other connection. Requests that
    /// mint acceptance (`MutateGovernedTask` with `RecordOperatorDecision`, and
    /// the lifecycle marks of a profiled task) are rejected without it.
    PresentOperatorCapability(impulse_ops::operator_capability::OperatorCapabilityPresentation),
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
    /// Fetch the workbench snapshot used by operator surfaces.
    GetOpsSnapshot,
    /// Poll for workbench events and a reconciled snapshot
    SubscribeOps {
        #[serde(default)]
        since_seq: Option<u64>,
    },
    /// Publish live terminal telemetry from a workbench surface.
    PublishTerminalOps {
        report: impulse_ops::TerminalOpsReport,
    },
    /// Read the effective supervisor permissions for the operator control plane.
    GetSupervisorPermissions,
    /// Structured supervisor chat for the operator control plane.
    SupervisorChat {
        prompt: String,
        context: Option<String>,
    },
    /// Run a structured supervisor action with daemon-side policy enforcement
    RunSupervisorAction {
        action: impulse_ops::SupervisorAction,
    },
    /// List project-scoped artifacts for workbench surfaces.
    ListArtifacts {
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Get a single project artifact by ID
    GetArtifact {
        artifact_id: String,
    },
    /// Run an artifact action for a workbench surface.
    RunArtifactAction {
        artifact_id: String,
        action_id: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    RegisterGovernedTask {
        registration: impulse_ops::governed_task::GovernedTaskRegistration,
    },
    GetGovernedTask {
        project_id: String,
        task_id: impulse_ops::governed_task::GovernedTaskId,
    },
    ListGovernedTasks {
        project_id: String,
    },
    MutateGovernedTask {
        request: impulse_ops::governed_task::GovernedTaskMutationRequest,
    },
    SubmitGovernedClaim {
        request: impulse_ops::governed_task::GovernedClaimRequest,
    },
    RunGovernedVerification {
        request: impulse_ops::governed_task::GovernedVerificationRequest,
    },
    RunGovernedSupervisorReview {
        request: impulse_ops::governed_task::GovernedSupervisorReviewRequest,
    },
    /// Fast-forward the canonical branch onto an accepted staged outcome
    /// (ADR-0019). Operator-class only: this is the step that makes a
    /// Builder's work canonical. A blocked promotion is a successful response
    /// carrying the typed outcome, not an error.
    PromoteGovernedOutcome {
        request: impulse_ops::governed_wiring::GovernedPromotionRequest,
    },
    /// Reclaim a finished staged worktree (ADR-0019). Operator-class only: it
    /// destroys work.
    DiscardGovernedStagedWorktree {
        request: impulse_ops::governed_wiring::GovernedStagedWorktreeDiscardRequest,
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
    Busy {
        resource: impulse_ops::DaemonBusyResource,
        retry_after_ms: u64,
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
        DaemonRequest::PresentOperatorCapability(_) => "PresentOperatorCapability",
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
        DaemonRequest::RegisterGovernedTask { .. } => "RegisterGovernedTask",
        DaemonRequest::GetGovernedTask { .. } => "GetGovernedTask",
        DaemonRequest::ListGovernedTasks { .. } => "ListGovernedTasks",
        DaemonRequest::MutateGovernedTask { .. } => "MutateGovernedTask",
        DaemonRequest::SubmitGovernedClaim { .. } => "SubmitGovernedClaim",
        DaemonRequest::RunGovernedVerification { .. } => "RunGovernedVerification",
        DaemonRequest::RunGovernedSupervisorReview { .. } => "RunGovernedSupervisorReview",
        DaemonRequest::PromoteGovernedOutcome { .. } => "PromoteGovernedOutcome",
        DaemonRequest::DiscardGovernedStagedWorktree { .. } => "DiscardGovernedStagedWorktree",
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── PROTOCOL_VERSION ───────────────────────────────────────────────

    #[test]
    fn test_protocol_version_is_nine() {
        // v7 (ADR-0018) adds PresentOperatorCapability and the operator-class
        // requirement on RecordOperatorDecision. v8 (ADR-0019) adds the
        // staged-worktree world scope: the MaterializeStagedWorktree /
        // DiscardStagedWorktree / RecordPromotion mutations and the promotion
        // outcome they carry. v9 makes those reachable over the socket:
        // PromoteGovernedOutcome, DiscardGovernedStagedWorktree, staged
        // materialization inside RegisterGovernedTask, and the flattened
        // producer acknowledgements carrying `replayed` plus a reservation's
        // `pending_rerun_reason`.
        assert_eq!(PROTOCOL_VERSION, 9);
    }

    #[test]
    fn test_present_operator_capability_round_trips_and_never_leaks_into_the_type_name() {
        let request = DaemonRequest::PresentOperatorCapability(
            impulse_ops::operator_capability::OperatorCapabilityPresentation {
                token: "a".repeat(64),
            },
        );
        let json = serde_json::to_string(&request).unwrap();
        // The newtype variant keeps the struct-variant wire shape exactly.
        assert!(json.contains(r#""data":{"token":"#));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::PresentOperatorCapability(ref presentation)
                if presentation.token == "a".repeat(64)
        ));
        assert_eq!(
            request_type_name(&request),
            "PresentOperatorCapability",
            "the traced request name must never carry the token"
        );
    }

    // ── request_type_name ───────────────────────────────────────────────

    #[test]
    fn test_request_type_name_unit_variants() {
        assert_eq!(request_type_name(&DaemonRequest::Ping), "Ping");
        assert_eq!(request_type_name(&DaemonRequest::Status), "Status");
        assert_eq!(
            request_type_name(&DaemonRequest::ListSessions),
            "ListSessions"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::StewardStatus),
            "StewardStatus"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::StewardMemory),
            "StewardMemory"
        );
        assert_eq!(request_type_name(&DaemonRequest::ToolSchema), "ToolSchema");
        assert_eq!(
            request_type_name(&DaemonRequest::GetOpsSnapshot),
            "GetOpsSnapshot"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::GetSupervisorPermissions),
            "GetSupervisorPermissions"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::DebugSnapshot),
            "DebugSnapshot"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::ListPlugins),
            "ListPlugins"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::ListDelegations),
            "ListDelegations"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::GetAgentPool),
            "GetAgentPool"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::GetConflictHistory),
            "GetConflictHistory"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::ClearResolvedConflicts),
            "ClearResolvedConflicts"
        );
        assert_eq!(request_type_name(&DaemonRequest::GuardList), "GuardList");
    }

    #[test]
    fn test_request_type_name_struct_variants() {
        assert_eq!(
            request_type_name(&DaemonRequest::CreateSession {
                name: "test".into(),
                platform: None
            }),
            "CreateSession"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::Chat {
                session_id: "s1".into(),
                message: "hi".into(),
                inject_mode: None,
                inject_explain: false
            }),
            "Chat"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::ListTools { category: None }),
            "ListTools"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::DescribeTool { name: "foo".into() }),
            "DescribeTool"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::InvokeTool {
                name: "bar".into(),
                params: serde_json::json!({})
            }),
            "InvokeTool"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::CheckConflict {
                session_id: "s1".into(),
                file_path: "src/lib.rs".into()
            }),
            "CheckConflict"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::AgentReviewCode {
                file_path: "x.rs".into(),
                diff: "".into(),
                insights: vec![]
            }),
            "AgentReviewCode"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::AgentAnalyzeError {
                error_text: "E0425".into(),
                context: "".into(),
                insights: vec![]
            }),
            "AgentAnalyzeError"
        );
        assert_eq!(
            request_type_name(&DaemonRequest::AgentSummarizePane {
                pane_id: 1,
                raw_output: "".into(),
                insights: vec![]
            }),
            "AgentSummarizePane"
        );
    }

    // ── respond_ok / respond_err ──────────────────────────────────────

    #[test]
    fn test_respond_ok_serializes_value() {
        let resp = respond_ok(&"hello");
        match resp {
            DaemonResponse::Ok { result } => assert_eq!(result, serde_json::json!("hello")),
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn test_respond_ok_with_nested_value() {
        let data = serde_json::json!({"count": 42, "active": true});
        let resp = respond_ok(&data);
        match resp {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["count"], 42);
                assert_eq!(result["active"], true);
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn test_respond_err_contains_message() {
        let resp = respond_err("something went wrong");
        match resp {
            DaemonResponse::Error { message } => {
                assert_eq!(message, "something went wrong");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_respond_err_with_display_format() {
        use std::fmt;
        struct CustomError;
        impl fmt::Display for CustomError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "custom error: {}", 42)
            }
        }
        let resp = respond_err(CustomError);
        match resp {
            DaemonResponse::Error { message } => {
                assert_eq!(message, "custom error: 42");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    // ── DaemonRequest serde roundtrip ─────────────────────────────────

    #[test]
    fn test_daemon_request_ping_roundtrip() {
        let req = DaemonRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"Ping"}"#);
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Ping));
    }

    #[test]
    fn test_daemon_request_create_session_roundtrip() {
        let req = DaemonRequest::CreateSession {
            name: "my-session".into(),
            platform: Some("claude".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::CreateSession { name, platform } => {
                assert_eq!(name, "my-session");
                assert_eq!(platform.as_deref(), Some("claude"));
            }
            other => panic!("expected CreateSession, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_request_chat_roundtrip() {
        let req = DaemonRequest::Chat {
            session_id: "sess-1".into(),
            message: "what files changed?".into(),
            inject_mode: Some("auto".into()),
            inject_explain: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::Chat {
                session_id,
                message,
                inject_mode,
                inject_explain,
            } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(message, "what files changed?");
                assert_eq!(inject_mode.as_deref(), Some("auto"));
                assert!(inject_explain);
            }
            other => panic!("expected Chat, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_request_list_sessions_roundtrip() {
        let req = DaemonRequest::ListSessions;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"ListSessions"}"#);
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ListSessions));
    }

    #[test]
    fn test_daemon_request_get_conflict_history_roundtrip() {
        let req = DaemonRequest::GetConflictHistory;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"GetConflictHistory"}"#);
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GetConflictHistory));
    }

    #[test]
    fn test_daemon_request_clear_resolved_conflicts_roundtrip() {
        let req = DaemonRequest::ClearResolvedConflicts;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"ClearResolvedConflicts"}"#);
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ClearResolvedConflicts));
    }

    #[test]
    fn test_daemon_request_check_conflict_roundtrip() {
        let req = DaemonRequest::CheckConflict {
            session_id: "sess-1".into(),
            file_path: "src/lib.rs".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::CheckConflict {
                session_id,
                file_path,
            } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(file_path, "src/lib.rs");
            }
            other => panic!("expected CheckConflict, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_request_list_tools_with_category() {
        let req = DaemonRequest::ListTools {
            category: Some("document".into()),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""category":"document""#));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::ListTools { category } => {
                assert_eq!(category.as_deref(), Some("document"));
            }
            other => panic!("expected ListTools, got {:?}", other),
        }
    }

    // ── DaemonResponse serde roundtrip ─────────────────────────────────

    #[test]
    fn test_daemon_response_ok_roundtrip() {
        let resp = DaemonResponse::Ok {
            result: serde_json::json!({"count": 3}),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::Ok { result } => assert_eq!(result["count"], 3),
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_error_roundtrip() {
        let resp = DaemonResponse::Error {
            message: "connection refused".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::Error { message } => assert_eq!(message, "connection refused"),
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_busy_roundtrip() {
        let resp = DaemonResponse::Busy {
            resource: impulse_ops::DaemonBusyResource::AgentTurn,
            retry_after_ms: 250,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""type":"Busy""#));
        assert!(json.contains(r#""resource":"agent_turn""#));
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonResponse::Busy {
                resource: impulse_ops::DaemonBusyResource::AgentTurn,
                retry_after_ms: 250,
            }
        ));
    }

    #[test]
    fn test_daemon_response_agent_assist_result_roundtrip() {
        let resp = DaemonResponse::AgentAssistResult {
            success: true,
            response: "Consider merging these changes".into(),
            recommendations: vec![],
            pane_summaries: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::AgentAssistResult {
                success, response, ..
            } => {
                assert!(success);
                assert_eq!(response, "Consider merging these changes");
            }
            other => panic!("expected AgentAssistResult, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_conflict_check_roundtrip() {
        let resp = DaemonResponse::ConflictCheck {
            has_conflict: true,
            conflicting_sessions: vec!["sess-1".into(), "sess-2".into()],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::ConflictCheck {
                has_conflict,
                conflicting_sessions,
            } => {
                assert!(has_conflict);
                assert_eq!(conflicting_sessions.len(), 2);
            }
            other => panic!("expected ConflictCheck, got {:?}", other),
        }
    }

    // ── serde tag format verification ─────────────────────────────────

    #[test]
    fn test_daemon_request_uses_type_tag_format() {
        let req = DaemonRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.starts_with(r#"{"type":""#));
        assert!(!json.contains(r#""data""#));
    }

    #[test]
    fn test_daemon_request_unit_variant_no_data_field() {
        let req = DaemonRequest::ListSessions;
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"type":"ListSessions"}"#);
    }

    // ── AgentAssist with empty insights ────────────────────────────────

    #[test]
    fn test_daemon_request_agent_assist_empty_insights_serializes() {
        // #[serde(default)] does NOT skip empty vectors — empty vec serializes as []
        let req = DaemonRequest::AgentAssist {
            prompt: "review my code".into(),
            context: None,
            insights: vec![],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""type":"AgentAssist""#));
        assert!(json.contains(r#""prompt":"review my code""#));
        assert!(json.contains(r#""insights":[]"#));
    }

    #[test]
    fn test_daemon_request_invoke_tool_default_params() {
        let req = DaemonRequest::InvokeTool {
            name: "calculator".into(),
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::InvokeTool { name, params } => {
                assert_eq!(name, "calculator");
                assert_eq!(params, serde_json::json!({}));
            }
            other => panic!("expected InvokeTool, got {:?}", other),
        }
    }

    // ── AgentSpecializedResult roundtrip ────────────────────────────────

    #[test]
    fn test_daemon_response_agent_specialized_result_roundtrip() {
        let resp = DaemonResponse::AgentSpecializedResult {
            success: false,
            response: "Could not analyze: file not found".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::AgentSpecializedResult { success, response } => {
                assert!(!success);
                assert!(response.contains("file not found"));
            }
            other => panic!("expected AgentSpecializedResult, got {:?}", other),
        }
    }

    fn assert_shared_request_compatible(request: impulse_ops::WorkbenchDaemonRequest) {
        let json = serde_json::to_string(&request).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request_type_name(&parsed), request_variant_name(&request));
    }

    fn request_variant_name(request: &impulse_ops::WorkbenchDaemonRequest) -> &'static str {
        match request {
            impulse_ops::WorkbenchDaemonRequest::Ping => "Ping",
            impulse_ops::WorkbenchDaemonRequest::Status => "Status",
            impulse_ops::WorkbenchDaemonRequest::PresentOperatorCapability(_) => {
                "PresentOperatorCapability"
            }
            impulse_ops::WorkbenchDaemonRequest::ListSessions => "ListSessions",
            impulse_ops::WorkbenchDaemonRequest::CreateSession { .. } => "CreateSession",
            impulse_ops::WorkbenchDaemonRequest::EndSession { .. } => "EndSession",
            impulse_ops::WorkbenchDaemonRequest::TrackFile { .. } => "TrackFile",
            impulse_ops::WorkbenchDaemonRequest::InvokeTool { .. } => "InvokeTool",
            impulse_ops::WorkbenchDaemonRequest::ToolSchema => "ToolSchema",
            impulse_ops::WorkbenchDaemonRequest::GetOpsSnapshot => "GetOpsSnapshot",
            impulse_ops::WorkbenchDaemonRequest::SubscribeOps { .. } => "SubscribeOps",
            impulse_ops::WorkbenchDaemonRequest::PublishTerminalOps { .. } => "PublishTerminalOps",
            impulse_ops::WorkbenchDaemonRequest::GetSupervisorPermissions => {
                "GetSupervisorPermissions"
            }
            impulse_ops::WorkbenchDaemonRequest::SupervisorChat { .. } => "SupervisorChat",
            impulse_ops::WorkbenchDaemonRequest::RunSupervisorAction { .. } => {
                "RunSupervisorAction"
            }
            impulse_ops::WorkbenchDaemonRequest::RunArtifactAction { .. } => "RunArtifactAction",
            impulse_ops::WorkbenchDaemonRequest::RegisterGovernedTask { .. } => {
                "RegisterGovernedTask"
            }
            impulse_ops::WorkbenchDaemonRequest::GetGovernedTask { .. } => "GetGovernedTask",
            impulse_ops::WorkbenchDaemonRequest::ListGovernedTasks { .. } => "ListGovernedTasks",
            impulse_ops::WorkbenchDaemonRequest::MutateGovernedTask { .. } => "MutateGovernedTask",
            impulse_ops::WorkbenchDaemonRequest::SubmitGovernedClaim { .. } => {
                "SubmitGovernedClaim"
            }
            impulse_ops::WorkbenchDaemonRequest::RunGovernedVerification { .. } => {
                "RunGovernedVerification"
            }
            impulse_ops::WorkbenchDaemonRequest::RunGovernedSupervisorReview { .. } => {
                "RunGovernedSupervisorReview"
            }
            impulse_ops::WorkbenchDaemonRequest::PromoteGovernedOutcome { .. } => {
                "PromoteGovernedOutcome"
            }
            impulse_ops::WorkbenchDaemonRequest::DiscardGovernedStagedWorktree { .. } => {
                "DiscardGovernedStagedWorktree"
            }
            impulse_ops::WorkbenchDaemonRequest::GuardList => "GuardList",
            impulse_ops::WorkbenchDaemonRequest::GetConflictHistory => "GetConflictHistory",
            impulse_ops::WorkbenchDaemonRequest::ClearResolvedConflicts => "ClearResolvedConflicts",
        }
    }

    #[test]
    fn test_shared_workbench_requests_deserialize_into_daemon_protocol() {
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::Ping);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::Status);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::ListSessions);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::CreateSession {
            name: "demo".into(),
            platform: Some("claude".into()),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::EndSession {
            session_id: "sess-1".into(),
            summary: "done".into(),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::TrackFile {
            session_id: "sess-1".into(),
            file_path: "src/main.rs".into(),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::InvokeTool {
            name: "memory_search".into(),
            params: serde_json::json!({"query": "daemon"}),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::ToolSchema);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::GetOpsSnapshot);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::SubscribeOps {
            since_seq: Some(7),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::PublishTerminalOps {
            report: impulse_ops::TerminalOpsReport {
                source_id: "gui".into(),
                published_at: "2026-04-01T00:00:00Z".into(),
                agents: Vec::new(),
                context: impulse_ops::ContextHealthSummary::default(),
                interventions: Vec::new(),
            },
        });
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::GetSupervisorPermissions,
        );
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::SupervisorChat {
            prompt: "status?".into(),
            context: Some("operator".into()),
        });
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::RunSupervisorAction {
                action: impulse_ops::SupervisorAction::FocusAgent {
                    agent_id: "agent-1".into(),
                    session_id: None,
                },
            },
        );
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::RunArtifactAction {
            artifact_id: "artifact-1".into(),
            action_id: "review".into(),
            params: serde_json::json!({}),
        });
        let registration = impulse_ops::governed_task::GovernedTaskRegistration::builder(
            "request-1",
            "task-1",
            "demo",
            "/tmp/demo",
            "prove governed lifecycle",
            "agent-1",
            "codex",
        )
        .build()
        .unwrap();
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::RegisterGovernedTask { registration },
        );
        let task_id = impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap();
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::GetGovernedTask {
            project_id: "demo".into(),
            task_id: task_id.clone(),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::ListGovernedTasks {
            project_id: "demo".into(),
        });
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::MutateGovernedTask {
            request: impulse_ops::governed_task::GovernedTaskMutationRequest {
                request_id: impulse_ops::governed_task::GovernedRequestId::try_new("request-2")
                    .unwrap(),
                project_id: "demo".into(),
                task_id,
                expected_revision: 0,
                mutation: impulse_ops::governed_task::GovernedTaskMutation::MarkRunning {
                    actor: impulse_ops::governed_task::GovernedActor {
                        kind: impulse_ops::governed_task::GovernedActorKind::System,
                        id: "desktop".into(),
                    },
                },
                fencing_generation: None,
            },
        });
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::SubmitGovernedClaim {
                request: impulse_ops::governed_task::GovernedClaimRequest {
                    request_id: impulse_ops::governed_task::GovernedRequestId::try_new(
                        "request-claim",
                    )
                    .unwrap(),
                    project_id: "demo".into(),
                    task_id: impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap(),
                    expected_revision: 1,
                    summary: "done".into(),
                    artifact_ids: Vec::new(),
                    generation: None,
                },
            },
        );
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::RunGovernedVerification {
                request: impulse_ops::governed_task::GovernedVerificationRequest {
                    request_id: impulse_ops::governed_task::GovernedRequestId::try_new(
                        "request-verify",
                    )
                    .unwrap(),
                    project_id: "demo".into(),
                    task_id: impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap(),
                    expected_revision: 2,
                },
            },
        );
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::RunGovernedSupervisorReview {
                request: impulse_ops::governed_task::GovernedSupervisorReviewRequest {
                    request_id: impulse_ops::governed_task::GovernedRequestId::try_new(
                        "request-review",
                    )
                    .unwrap(),
                    project_id: "demo".into(),
                    task_id: impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap(),
                    expected_revision: 3,
                },
            },
        );
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::PromoteGovernedOutcome {
                request: impulse_ops::governed_wiring::GovernedPromotionRequest {
                    request_id: impulse_ops::governed_task::GovernedRequestId::try_new(
                        "request-promote",
                    )
                    .unwrap(),
                    project_id: "demo".into(),
                    task_id: impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap(),
                    expected_revision: 4,
                },
            },
        );
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::DiscardGovernedStagedWorktree {
                request: impulse_ops::governed_wiring::GovernedStagedWorktreeDiscardRequest {
                    request_id: impulse_ops::governed_task::GovernedRequestId::try_new(
                        "request-discard",
                    )
                    .unwrap(),
                    project_id: "demo".into(),
                    task_id: impulse_ops::governed_task::GovernedTaskId::try_new("task-1").unwrap(),
                    expected_revision: 5,
                    reason: "the run was rejected".into(),
                },
            },
        );
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::GuardList);
        assert_shared_request_compatible(impulse_ops::WorkbenchDaemonRequest::GetConflictHistory);
        assert_shared_request_compatible(
            impulse_ops::WorkbenchDaemonRequest::ClearResolvedConflicts,
        );
    }

    #[test]
    fn test_shared_workbench_responses_roundtrip_from_daemon_protocol() {
        for response in [
            DaemonResponse::Ok {
                result: serde_json::json!({"pong": true}),
            },
            DaemonResponse::Error {
                message: "boom".into(),
            },
            DaemonResponse::Busy {
                resource: impulse_ops::DaemonBusyResource::AgentTurn,
                retry_after_ms: 250,
            },
            DaemonResponse::ConflictCheck {
                has_conflict: true,
                conflicting_sessions: vec!["a".into(), "b".into()],
            },
        ] {
            let json = serde_json::to_string(&response).unwrap();
            let parsed: impulse_ops::WorkbenchDaemonResponse = serde_json::from_str(&json).unwrap();
            let reparsed: DaemonResponse =
                serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(
                std::mem::discriminant(&response),
                std::mem::discriminant(&reparsed)
            );
        }
    }

    // ── Wire contract: every variant's tag, value round-trips, older clients ──
    //
    // Clients (the TUI, the Dioxus cockpit, the CLI, `impulse_ops`) match on the
    // adjacently tagged `"type"` field, so each tag below is part of protocol v9.
    // `DaemonRequest` and `DaemonResponse` do not derive `PartialEq`, so these
    // tests compare the re-serialized `serde_json::Value` instead.

    /// Number of `DaemonRequest` variants. `request_type_name` is an exhaustive
    /// match, so adding a variant fails to compile there; bump this and add the
    /// variant to `every_daemon_request` in the same change.
    const DAEMON_REQUEST_VARIANT_COUNT: usize = 51;

    fn wire_timestamp() -> chrono::DateTime<chrono::Utc> {
        "2026-10-23T12:34:56Z".parse().unwrap()
    }

    fn wire_insight() -> ExtractedInsight {
        ExtractedInsight {
            pane_id: 4,
            agent_kind: crate::context_lifecycle::types::AgentKind::Codex,
            timestamp: wire_timestamp(),
            insight_type: crate::context_lifecycle::types::InsightType::ErrorEncountered,
            content: "error[E0382]: borrow of moved value".into(),
            intent: Some(crate::context_lifecycle::intent::IntentCategory::Debugging),
        }
    }

    fn wire_recommendation() -> crate::agent::coordinator::Recommendation {
        crate::agent::coordinator::Recommendation {
            recommendation_type: crate::agent::coordinator::RecommendationType::FileConflict,
            panes_involved: vec!["pane-1".into(), "pane-2".into()],
            description: "Multiple agents modifying: src/lib.rs".into(),
            action: "Pause pane-2".into(),
            priority: 90,
        }
    }

    fn governed_request_id(id: &str) -> impulse_ops::governed_task::GovernedRequestId {
        impulse_ops::governed_task::GovernedRequestId::try_new(id).unwrap()
    }

    fn governed_task_id() -> impulse_ops::governed_task::GovernedTaskId {
        impulse_ops::governed_task::GovernedTaskId::try_new("task-7").unwrap()
    }

    /// One non-default value of every `DaemonRequest` variant, paired with the
    /// wire tag clients send for it.
    fn every_daemon_request() -> Vec<(&'static str, DaemonRequest)> {
        use impulse_ops::governed_task as gt;
        let registration = gt::GovernedTaskRegistration::builder(
            "request-register",
            "task-7",
            "demo",
            "/tmp/demo",
            "pin the wire contract",
            "agent-1",
            "codex",
        )
        .build()
        .unwrap();
        vec![
            ("Ping", DaemonRequest::Ping),
            ("Status", DaemonRequest::Status),
            (
                "PresentOperatorCapability",
                DaemonRequest::PresentOperatorCapability(
                    impulse_ops::operator_capability::OperatorCapabilityPresentation {
                        token: "f".repeat(64),
                    },
                ),
            ),
            (
                "CreateSession",
                DaemonRequest::CreateSession {
                    name: "wire".into(),
                    platform: Some("codex".into()),
                },
            ),
            (
                "EndSession",
                DaemonRequest::EndSession {
                    session_id: "s-1".into(),
                    summary: "done".into(),
                },
            ),
            (
                "TrackFile",
                DaemonRequest::TrackFile {
                    session_id: "s-1".into(),
                    file_path: "src/lib.rs".into(),
                },
            ),
            (
                "TrackTool",
                DaemonRequest::TrackTool {
                    session_id: "s-1".into(),
                    tool_name: "Edit".into(),
                },
            ),
            (
                "GetSession",
                DaemonRequest::GetSession {
                    session_id: "s-1".into(),
                },
            ),
            ("ListSessions", DaemonRequest::ListSessions),
            (
                "Chat",
                DaemonRequest::Chat {
                    session_id: "s-1".into(),
                    message: "hello".into(),
                    inject_mode: Some("review".into()),
                    inject_explain: true,
                },
            ),
            ("StewardStatus", DaemonRequest::StewardStatus),
            (
                "StewardProposals",
                DaemonRequest::StewardProposals {
                    action: "approve".into(),
                    id: Some("p-1".into()),
                },
            ),
            ("StewardMemory", DaemonRequest::StewardMemory),
            (
                "ListTools",
                DaemonRequest::ListTools {
                    category: Some("analysis".into()),
                },
            ),
            (
                "DescribeTool",
                DaemonRequest::DescribeTool {
                    name: "grep".into(),
                },
            ),
            (
                "InvokeTool",
                DaemonRequest::InvokeTool {
                    name: "grep".into(),
                    params: serde_json::json!({"pattern": "fn main", "limit": 3}),
                },
            ),
            ("ToolSchema", DaemonRequest::ToolSchema),
            ("GetOpsSnapshot", DaemonRequest::GetOpsSnapshot),
            (
                "SubscribeOps",
                DaemonRequest::SubscribeOps {
                    since_seq: Some(42),
                },
            ),
            (
                "PublishTerminalOps",
                DaemonRequest::PublishTerminalOps {
                    report: impulse_ops::TerminalOpsReport {
                        source_id: "gui-1".into(),
                        published_at: "2026-10-23T12:34:56Z".into(),
                        agents: Vec::new(),
                        context: impulse_ops::ContextHealthSummary {
                            tier: "essential".into(),
                            usage_fraction: 0.5,
                            estimated_tokens: 1000,
                            window_tokens: 2000,
                            compaction_count: 1,
                            ..Default::default()
                        },
                        interventions: vec![impulse_ops::InterventionRecommendation {
                            id: "i-1".into(),
                            title: "Compact".into(),
                            severity: "warn".into(),
                            target_agent_id: Some("tab-1".into()),
                            ..Default::default()
                        }],
                    },
                },
            ),
            (
                "GetSupervisorPermissions",
                DaemonRequest::GetSupervisorPermissions,
            ),
            (
                "SupervisorChat",
                DaemonRequest::SupervisorChat {
                    prompt: "status?".into(),
                    context: Some("two panes".into()),
                },
            ),
            (
                "RunSupervisorAction",
                DaemonRequest::RunSupervisorAction {
                    action: impulse_ops::SupervisorAction::SendInput {
                        agent_id: "tab-1".into(),
                        session_id: Some("s-1".into()),
                        content: "continue".into(),
                        confirmed: true,
                    },
                },
            ),
            (
                "ListArtifacts",
                DaemonRequest::ListArtifacts { limit: Some(5) },
            ),
            (
                "GetArtifact",
                DaemonRequest::GetArtifact {
                    artifact_id: "artifact-1".into(),
                },
            ),
            (
                "RunArtifactAction",
                DaemonRequest::RunArtifactAction {
                    artifact_id: "artifact-1".into(),
                    action_id: "review".into(),
                    params: serde_json::json!({"note": "ok"}),
                },
            ),
            (
                "RegisterGovernedTask",
                DaemonRequest::RegisterGovernedTask { registration },
            ),
            (
                "GetGovernedTask",
                DaemonRequest::GetGovernedTask {
                    project_id: "demo".into(),
                    task_id: governed_task_id(),
                },
            ),
            (
                "ListGovernedTasks",
                DaemonRequest::ListGovernedTasks {
                    project_id: "demo".into(),
                },
            ),
            (
                "MutateGovernedTask",
                DaemonRequest::MutateGovernedTask {
                    request: gt::GovernedTaskMutationRequest {
                        request_id: governed_request_id("request-mutate"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 1,
                        mutation: gt::GovernedTaskMutation::MarkRunning {
                            actor: gt::GovernedActor {
                                kind: gt::GovernedActorKind::System,
                                id: "desktop".into(),
                            },
                        },
                    },
                },
            ),
            (
                "SubmitGovernedClaim",
                DaemonRequest::SubmitGovernedClaim {
                    request: gt::GovernedClaimRequest {
                        request_id: governed_request_id("request-claim"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 2,
                        summary: "implemented".into(),
                        artifact_ids: vec!["artifact-1".into()],
                    },
                },
            ),
            (
                "RunGovernedVerification",
                DaemonRequest::RunGovernedVerification {
                    request: gt::GovernedVerificationRequest {
                        request_id: governed_request_id("request-verify"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 3,
                    },
                },
            ),
            (
                "RunGovernedSupervisorReview",
                DaemonRequest::RunGovernedSupervisorReview {
                    request: gt::GovernedSupervisorReviewRequest {
                        request_id: governed_request_id("request-review"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 4,
                    },
                },
            ),
            (
                "PromoteGovernedOutcome",
                DaemonRequest::PromoteGovernedOutcome {
                    request: impulse_ops::governed_wiring::GovernedPromotionRequest {
                        request_id: governed_request_id("request-promote"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 5,
                    },
                },
            ),
            (
                "DiscardGovernedStagedWorktree",
                DaemonRequest::DiscardGovernedStagedWorktree {
                    request: impulse_ops::governed_wiring::GovernedStagedWorktreeDiscardRequest {
                        request_id: governed_request_id("request-discard"),
                        project_id: "demo".into(),
                        task_id: governed_task_id(),
                        expected_revision: 6,
                        reason: "rejected".into(),
                    },
                },
            ),
            (
                "AgentAssist",
                DaemonRequest::AgentAssist {
                    prompt: "coordinate".into(),
                    context: Some("two panes".into()),
                    insights: vec![wire_insight()],
                },
            ),
            (
                "GuardEvaluate",
                DaemonRequest::GuardEvaluate {
                    target: "bash".into(),
                    action: "git push --force main".into(),
                },
            ),
            ("GuardList", DaemonRequest::GuardList),
            (
                "CheckConflict",
                DaemonRequest::CheckConflict {
                    session_id: "s-1".into(),
                    file_path: "src/lib.rs".into(),
                },
            ),
            ("DebugSnapshot", DaemonRequest::DebugSnapshot),
            ("ListPlugins", DaemonRequest::ListPlugins),
            (
                "InvokePlugin",
                DaemonRequest::InvokePlugin {
                    name: "office".into(),
                    input: crate::plugin::PluginInput::new()
                        .with_path(std::path::PathBuf::from("/tmp/report.docx"))
                        .with_query("tables")
                        .with_options(serde_json::json!({"max_pages": 2})),
                },
            ),
            (
                "RegisterDelegation",
                DaemonRequest::RegisterDelegation {
                    spec: crate::delegation::types::DelegationSpec {
                        task: "refactor auth".into(),
                        target_files: vec!["src/auth.rs".into()],
                        constraints: Some("no new deps".into()),
                        max_depth: 1,
                        restricted_tools: vec!["bash_exec".into()],
                    },
                    coordinator_pane_id: 3,
                    context_snapshot: "coordinator context".into(),
                },
            ),
            (
                "CompleteDelegation",
                DaemonRequest::CompleteDelegation {
                    delegation_id: "del-1".into(),
                    summary: "refactored".into(),
                    tool_trace: vec![impulse_ops::ToolInvocationRecord {
                        kind: "edit".into(),
                        target: "src/auth.rs".into(),
                        timestamp: Some("2026-10-23T12:34:56Z".into()),
                    }],
                    diff_summary: Some(impulse_ops::DiffSummary {
                        files_changed: 1,
                        lines_added: 10,
                        lines_removed: 4,
                    }),
                },
            ),
            ("ListDelegations", DaemonRequest::ListDelegations),
            ("GetAgentPool", DaemonRequest::GetAgentPool),
            (
                "AgentReviewCode",
                DaemonRequest::AgentReviewCode {
                    file_path: "src/lib.rs".into(),
                    diff: "+fn main() {}".into(),
                    insights: vec![wire_insight()],
                },
            ),
            (
                "AgentAnalyzeError",
                DaemonRequest::AgentAnalyzeError {
                    error_text: "panicked".into(),
                    context: "cargo test".into(),
                    insights: vec![wire_insight()],
                },
            ),
            (
                "AgentSummarizePane",
                DaemonRequest::AgentSummarizePane {
                    pane_id: 2,
                    raw_output: "Compiling impulse-rs".into(),
                    insights: vec![wire_insight()],
                },
            ),
            ("GetConflictHistory", DaemonRequest::GetConflictHistory),
            (
                "ClearResolvedConflicts",
                DaemonRequest::ClearResolvedConflicts,
            ),
        ]
    }

    /// Serialize to a JSON line, parse it back, and require the re-serialized
    /// value to be identical. Returns the wire value for further assertions.
    fn request_wire_round_trip(request: &DaemonRequest) -> serde_json::Value {
        let line = serde_json::to_string(request).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&line).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            wire,
            "{} changed across a round trip",
            request_type_name(request)
        );
        wire
    }

    fn response_wire_round_trip(response: &DaemonResponse) -> serde_json::Value {
        let line = serde_json::to_string(response).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&line).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&line).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
        wire
    }

    #[test]
    fn test_daemon_request_every_variant_pins_wire_tag_and_round_trips() {
        let requests = every_daemon_request();
        let tags: std::collections::BTreeSet<&str> = requests.iter().map(|(t, _)| *t).collect();
        assert_eq!(tags.len(), requests.len(), "duplicate tag in the table");
        assert_eq!(requests.len(), DAEMON_REQUEST_VARIANT_COUNT);

        for (tag, request) in &requests {
            let wire = request_wire_round_trip(request);
            assert_eq!(wire["type"], *tag, "wire tag for {tag}");
            assert_eq!(request_type_name(request), *tag);
            let object = wire.as_object().unwrap();
            assert!(
                object.keys().all(|k| k == "type" || k == "data"),
                "{tag} wire carries unexpected top-level keys: {object:?}"
            );
        }
    }

    #[test]
    fn test_daemon_request_unit_variants_serialize_without_data() {
        for (tag, request) in every_daemon_request() {
            let wire = serde_json::to_value(&request).unwrap();
            let is_unit = matches!(
                request,
                DaemonRequest::Ping
                    | DaemonRequest::Status
                    | DaemonRequest::ListSessions
                    | DaemonRequest::StewardStatus
                    | DaemonRequest::StewardMemory
                    | DaemonRequest::ToolSchema
                    | DaemonRequest::GetOpsSnapshot
                    | DaemonRequest::GetSupervisorPermissions
                    | DaemonRequest::GuardList
                    | DaemonRequest::DebugSnapshot
                    | DaemonRequest::ListPlugins
                    | DaemonRequest::ListDelegations
                    | DaemonRequest::GetAgentPool
                    | DaemonRequest::GetConflictHistory
                    | DaemonRequest::ClearResolvedConflicts
            );
            assert_eq!(
                wire.get("data").is_none(),
                is_unit,
                "{tag}: unit variants carry no data, struct variants always do"
            );
            if is_unit {
                let parsed: DaemonRequest =
                    serde_json::from_str(&format!(r#"{{"type":"{tag}"}}"#)).unwrap();
                assert_eq!(request_type_name(&parsed), tag);
            }
        }
    }

    #[test]
    fn test_daemon_request_payload_fields_survive_round_trip() {
        let requests = every_daemon_request();
        let wire_of = |tag: &str| {
            let (_, request) = requests.iter().find(|(t, _)| *t == tag).unwrap();
            request_wire_round_trip(request)
        };

        let assist = wire_of("AgentAssist");
        let insight = &assist["data"]["insights"][0];
        assert_eq!(insight["pane_id"], 4);
        assert_eq!(insight["agent_kind"], "codex");
        assert_eq!(insight["intent"], "debugging");
        assert_eq!(insight["timestamp"], "2026-10-23T12:34:56Z");

        let plugin = wire_of("InvokePlugin");
        assert_eq!(plugin["data"]["input"]["path"], "/tmp/report.docx");
        assert_eq!(plugin["data"]["input"]["options"]["max_pages"], 2);

        let delegation = wire_of("RegisterDelegation");
        assert_eq!(delegation["data"]["spec"]["max_depth"], 1);
        assert_eq!(
            delegation["data"]["spec"]["restricted_tools"][0],
            "bash_exec"
        );

        let completion = wire_of("CompleteDelegation");
        assert_eq!(completion["data"]["tool_trace"][0]["kind"], "edit");
        assert_eq!(completion["data"]["diff_summary"]["lines_removed"], 4);

        let action = wire_of("RunSupervisorAction");
        assert_eq!(action["data"]["action"]["kind"], "send_input");
        assert_eq!(action["data"]["action"]["confirmed"], true);
    }

    #[test]
    fn test_daemon_request_insight_without_intent_is_omitted_from_wire() {
        let mut insight = wire_insight();
        insight.intent = None;
        let request = DaemonRequest::AgentSummarizePane {
            pane_id: 1,
            raw_output: String::new(),
            insights: vec![insight],
        };
        let wire = request_wire_round_trip(&request);
        assert!(
            wire["data"]["insights"][0].get("intent").is_none(),
            "skip_serializing_if must keep `intent` off the wire when None"
        );
    }

    #[test]
    fn test_daemon_request_older_clients_may_omit_defaulted_fields() {
        let cases = [
            r#"{"type":"Chat","data":{"session_id":"s","message":"m"}}"#,
            r#"{"type":"ListTools","data":{}}"#,
            r#"{"type":"InvokeTool","data":{"name":"grep"}}"#,
            r#"{"type":"SubscribeOps","data":{}}"#,
            r#"{"type":"ListArtifacts","data":{}}"#,
            r#"{"type":"RunArtifactAction","data":{"artifact_id":"a","action_id":"b"}}"#,
            r#"{"type":"AgentAssist","data":{"prompt":"p"}}"#,
            r#"{"type":"RegisterDelegation","data":{"spec":{"task":"t"},"coordinator_pane_id":1}}"#,
            r#"{"type":"CompleteDelegation","data":{"delegation_id":"d","summary":"s"}}"#,
            r#"{"type":"AgentReviewCode","data":{"file_path":"f","diff":"d"}}"#,
            r#"{"type":"AgentAnalyzeError","data":{"error_text":"e","context":"c"}}"#,
            r#"{"type":"AgentSummarizePane","data":{"pane_id":2}}"#,
            r#"{"type":"PublishTerminalOps","data":{"report":{"source_id":"g","published_at":"t"}}}"#,
            r#"{"type":"AgentAssist","data":{"prompt":"p","insights":[{"pane_id":1,"agent_kind":"codex","timestamp":"2026-10-23T12:34:56Z","insight_type":"error_encountered","content":"x"}]}}"#,
        ];
        for line in cases {
            let parsed: DaemonRequest = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("older-client line rejected: {line}: {e}"));
            request_wire_round_trip(&parsed);
        }

        match serde_json::from_str(cases[0]).unwrap() {
            DaemonRequest::Chat {
                inject_mode,
                inject_explain,
                ..
            } => {
                assert!(inject_mode.is_none());
                assert!(!inject_explain);
            }
            other => panic!("expected Chat, got {}", request_type_name(&other)),
        }
        match serde_json::from_str(cases[2]).unwrap() {
            DaemonRequest::InvokeTool { params, .. } => assert!(params.is_null()),
            other => panic!("expected InvokeTool, got {}", request_type_name(&other)),
        }
        match serde_json::from_str(cases[7]).unwrap() {
            DaemonRequest::RegisterDelegation {
                spec,
                context_snapshot,
                ..
            } => {
                assert_eq!(
                    spec.max_depth,
                    crate::delegation::types::MAX_DELEGATION_DEPTH
                );
                assert!(context_snapshot.is_empty());
            }
            other => panic!(
                "expected RegisterDelegation, got {}",
                request_type_name(&other)
            ),
        }
        match serde_json::from_str(cases[8]).unwrap() {
            DaemonRequest::CompleteDelegation {
                tool_trace,
                diff_summary,
                ..
            } => {
                assert!(tool_trace.is_empty());
                assert!(diff_summary.is_none());
            }
            other => panic!(
                "expected CompleteDelegation, got {}",
                request_type_name(&other)
            ),
        }
        match serde_json::from_str(cases[11]).unwrap() {
            DaemonRequest::AgentSummarizePane {
                raw_output,
                insights,
                ..
            } => {
                assert!(raw_output.is_empty());
                assert!(insights.is_empty());
            }
            other => panic!(
                "expected AgentSummarizePane, got {}",
                request_type_name(&other)
            ),
        }
        match serde_json::from_str(cases[13]).unwrap() {
            DaemonRequest::AgentAssist { insights, .. } => assert!(insights[0].intent.is_none()),
            other => panic!("expected AgentAssist, got {}", request_type_name(&other)),
        }
    }

    #[test]
    fn test_daemon_request_rejects_unknown_tag() {
        let err = serde_json::from_str::<DaemonRequest>(r#"{"type":"NoSuchRequest"}"#)
            .expect_err("an unknown tag must not parse");
        assert!(err.to_string().contains("NoSuchRequest"), "{err}");
    }

    #[test]
    fn test_supervisor_action_every_variant_pins_kind_and_round_trips() {
        use impulse_ops::{
            PermissionChangeScope, SupervisorAction, SupervisorActionPermission, ToolCapabilityId,
        };
        let actions = [
            (
                "focus_agent",
                SupervisorAction::FocusAgent {
                    agent_id: "tab-1".into(),
                    session_id: Some("s-1".into()),
                },
            ),
            (
                "send_input",
                SupervisorAction::SendInput {
                    agent_id: "tab-1".into(),
                    session_id: None,
                    content: "continue".into(),
                    confirmed: true,
                },
            ),
            (
                "inject_context",
                SupervisorAction::InjectContext {
                    agent_id: Some("tab-1".into()),
                    session_id: Some("s-1".into()),
                    query: "auth design".into(),
                    confirmed: true,
                },
            ),
            (
                "cleanup_context",
                SupervisorAction::CleanupContext {
                    agent_id: Some("tab-1".into()),
                    session_id: None,
                    goal: Some("trim logs".into()),
                    confirmed: true,
                },
            ),
            (
                "handoff_context",
                SupervisorAction::HandoffContext {
                    session_id: Some("s-1".into()),
                    target_tool: "codex".into(),
                    task: "finish tests".into(),
                    notes: Some("see PR".into()),
                    confirmed: true,
                },
            ),
            (
                "open_artifact_review",
                SupervisorAction::OpenArtifactReview {
                    artifact_id: "artifact-1".into(),
                },
            ),
            (
                "search_memory",
                SupervisorAction::SearchMemory {
                    query: "compaction".into(),
                },
            ),
            (
                "modify_permissions",
                SupervisorAction::ModifyPermissions {
                    scope: PermissionChangeScope::SessionOverride,
                    grant_actions: vec![SupervisorActionPermission::SendInput],
                    grant_tool_capabilities: vec![ToolCapabilityId::Network],
                    confirmed: true,
                },
            ),
            (
                "clear_session_override",
                SupervisorAction::ClearSessionOverride { confirmed: true },
            ),
            (
                "reset_baseline_permissions",
                SupervisorAction::ResetBaselinePermissions { confirmed: true },
            ),
        ];
        for (kind, action) in actions {
            let request = DaemonRequest::RunSupervisorAction {
                action: action.clone(),
            };
            let wire = request_wire_round_trip(&request);
            assert_eq!(wire["data"]["action"]["kind"], kind);
            let parsed: SupervisorAction =
                serde_json::from_value(wire["data"]["action"].clone()).unwrap();
            assert_eq!(parsed, action);
        }

        let modify = serde_json::to_value(SupervisorAction::ModifyPermissions {
            scope: PermissionChangeScope::PersistentDefault,
            grant_actions: vec![SupervisorActionPermission::HandoffContext],
            grant_tool_capabilities: vec![ToolCapabilityId::FileSystemWrite],
            confirmed: false,
        })
        .unwrap();
        assert_eq!(modify["scope"], "persistent_default");
        assert_eq!(modify["grant_actions"][0], "handoff_context");
        assert_eq!(modify["grant_tool_capabilities"][0], "file_system_write");
    }

    #[test]
    fn test_supervisor_action_older_clients_may_omit_defaulted_fields() {
        use impulse_ops::{PermissionChangeScope, SupervisorAction};
        let cases = [
            (
                r#"{"kind":"focus_agent","agent_id":"a"}"#,
                SupervisorAction::FocusAgent {
                    agent_id: "a".into(),
                    session_id: None,
                },
            ),
            (
                r#"{"kind":"send_input","agent_id":"a","content":"c"}"#,
                SupervisorAction::SendInput {
                    agent_id: "a".into(),
                    session_id: None,
                    content: "c".into(),
                    confirmed: false,
                },
            ),
            (
                r#"{"kind":"inject_context","query":"q"}"#,
                SupervisorAction::InjectContext {
                    agent_id: None,
                    session_id: None,
                    query: "q".into(),
                    confirmed: false,
                },
            ),
            (
                r#"{"kind":"cleanup_context"}"#,
                SupervisorAction::CleanupContext {
                    agent_id: None,
                    session_id: None,
                    goal: None,
                    confirmed: false,
                },
            ),
            (
                r#"{"kind":"handoff_context","target_tool":"t","task":"k"}"#,
                SupervisorAction::HandoffContext {
                    session_id: None,
                    target_tool: "t".into(),
                    task: "k".into(),
                    notes: None,
                    confirmed: false,
                },
            ),
            (
                r#"{"kind":"modify_permissions","scope":"session_override"}"#,
                SupervisorAction::ModifyPermissions {
                    scope: PermissionChangeScope::SessionOverride,
                    grant_actions: Vec::new(),
                    grant_tool_capabilities: Vec::new(),
                    confirmed: false,
                },
            ),
            (
                r#"{"kind":"clear_session_override"}"#,
                SupervisorAction::ClearSessionOverride { confirmed: false },
            ),
            (
                r#"{"kind":"reset_baseline_permissions"}"#,
                SupervisorAction::ResetBaselinePermissions { confirmed: false },
            ),
        ];
        for (line, expected) in cases {
            let parsed: SupervisorAction = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("older-client action rejected: {line}: {e}"));
            assert_eq!(parsed, expected, "{line}");
        }
    }

    #[test]
    fn test_daemon_response_every_variant_pins_wire_tag_and_round_trips() {
        let responses = [
            (
                "Ok",
                DaemonResponse::Ok {
                    result: serde_json::json!({"sessions": [1, 2], "ok": true}),
                },
            ),
            (
                "Error",
                DaemonResponse::Error {
                    message: "socket closed".into(),
                },
            ),
            (
                "Busy",
                DaemonResponse::Busy {
                    resource: impulse_ops::DaemonBusyResource::AgentTurn,
                    retry_after_ms: 250,
                },
            ),
            (
                "AgentAssistResult",
                DaemonResponse::AgentAssistResult {
                    success: true,
                    response: "pause pane-2".into(),
                    recommendations: vec![wire_recommendation()],
                    pane_summaries: vec![(
                        "pane-1".into(),
                        vec!["edited src/lib.rs".into(), "ran tests".into()],
                    )],
                },
            ),
            (
                "AgentSpecializedResult",
                DaemonResponse::AgentSpecializedResult {
                    success: false,
                    response: "no provider".into(),
                },
            ),
            (
                "ConflictCheck",
                DaemonResponse::ConflictCheck {
                    has_conflict: true,
                    conflicting_sessions: vec!["s-2".into()],
                },
            ),
        ];
        for (tag, response) in &responses {
            let wire = response_wire_round_trip(response);
            assert_eq!(wire["type"], *tag);
            assert!(wire.get("data").is_some(), "{tag} always carries data");
        }

        let busy = serde_json::to_value(&responses[2].1).unwrap();
        assert_eq!(busy["data"]["resource"], "agent_turn");
        let assist = serde_json::to_value(&responses[3].1).unwrap();
        let recommendation = &assist["data"]["recommendations"][0];
        assert_eq!(recommendation["recommendation_type"], "file_conflict");
        assert_eq!(recommendation["priority"], 90);
        // Tuples travel as two-element arrays.
        assert_eq!(
            assist["data"]["pane_summaries"][0],
            serde_json::json!(["pane-1", ["edited src/lib.rs", "ran tests"]])
        );
    }

    #[test]
    fn test_daemon_response_older_daemons_may_omit_defaulted_fields() {
        let line = r#"{"type":"AgentAssistResult","data":{"success":true,"response":"r"}}"#;
        match serde_json::from_str::<DaemonResponse>(line).unwrap() {
            DaemonResponse::AgentAssistResult {
                recommendations,
                pane_summaries,
                ..
            } => {
                assert!(recommendations.is_empty());
                assert!(pane_summaries.is_empty());
            }
            other => panic!("expected AgentAssistResult, got {other:?}"),
        }

        let line = r#"{"type":"AgentAssistResult","data":{"success":true,"response":"r","recommendations":[{"recommendation_type":"error_assist","panes_involved":[],"description":"d","action":"a"}]}}"#;
        match serde_json::from_str::<DaemonResponse>(line).unwrap() {
            DaemonResponse::AgentAssistResult {
                recommendations, ..
            } => assert_eq!(recommendations[0].priority, 50, "priority defaults to 50"),
            other => panic!("expected AgentAssistResult, got {other:?}"),
        }
    }

    #[test]
    fn test_daemon_response_rejects_unknown_busy_resource() {
        let line = r#"{"type":"Busy","data":{"resource":"gpu","retry_after_ms":1}}"#;
        assert!(serde_json::from_str::<DaemonResponse>(line).is_err());
    }
}
