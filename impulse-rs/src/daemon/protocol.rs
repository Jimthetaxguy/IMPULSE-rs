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

#[cfg(test)]
mod tests {
    use super::*;

    // ── PROTOCOL_VERSION ───────────────────────────────────────────────

    #[test]
    fn test_protocol_version_is_three() {
        assert_eq!(PROTOCOL_VERSION, 3);
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
}
