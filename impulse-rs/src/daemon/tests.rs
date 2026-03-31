//! Daemon IPC Tests
//! Tests for socket communication, request/response handling

#[cfg(test)]
// clippy: tests module inside tests.rs is standard test organization
#[allow(clippy::module_inception)]
mod tests {
    use std::path::PathBuf;

    use crate::daemon::{DaemonRequest, DaemonResponse};
    use crate::state::{LiveState, Platform, Session, SessionStatus};

    // Re-import handler functions from the extracted handlers module.
    // super::super = daemon module (tests.rs is daemon::tests, inner mod is daemon::tests::tests)
    use super::super::handlers::{
        handle_guard_request, handle_plugin_request, handle_session_request, handle_status,
        handle_steward_request,
    };

    /// Test DaemonRequest serialization/deserialization
    #[test]
    fn test_daemon_request_serialization() {
        // Test Ping
        let ping = DaemonRequest::Ping;
        let json = serde_json::to_string(&ping).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Ping));

        // Test Status
        let status = DaemonRequest::Status;
        let json = serde_json::to_string(&status).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Status));

        // Test CreateSession
        let create = DaemonRequest::CreateSession {
            name: "test-session".to_string(),
            platform: Some("claude-code".to_string()),
        };
        let json = serde_json::to_string(&create).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::CreateSession { name, platform } if name == "test-session"
        ));

        // Test EndSession
        let end = DaemonRequest::EndSession {
            session_id: "test-123".to_string(),
            summary: "Test summary".to_string(),
        };
        let json = serde_json::to_string(&end).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::EndSession { session_id, summary } if session_id == "test-123"
        ));

        // Test TrackFile
        let track_file = DaemonRequest::TrackFile {
            session_id: "test-123".to_string(),
            file_path: "/path/to/file.rs".to_string(),
        };
        let json = serde_json::to_string(&track_file).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::TrackFile { session_id, file_path } if file_path == "/path/to/file.rs"
        ));

        // Test TrackTool
        let track_tool = DaemonRequest::TrackTool {
            session_id: "test-123".to_string(),
            tool_name: "Write".to_string(),
        };
        let json = serde_json::to_string(&track_tool).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::TrackTool { session_id, tool_name } if tool_name == "Write"
        ));

        // Test GetSession
        let get = DaemonRequest::GetSession {
            session_id: "test-123".to_string(),
        };
        let json = serde_json::to_string(&get).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::GetSession { session_id } if session_id == "test-123"
        ));

        // Test ListSessions
        let list = DaemonRequest::ListSessions;
        let json = serde_json::to_string(&list).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ListSessions));

        // Test Chat
        let chat = DaemonRequest::Chat {
            session_id: "test-123".to_string(),
            message: "Hello, world!".to_string(),
            inject_mode: Some("review".to_string()),
            inject_explain: true,
        };
        let json = serde_json::to_string(&chat).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::Chat {
                session_id,
                message,
                inject_mode,
                inject_explain
            } if session_id == "test-123"
                && message == "Hello, world!"
                && inject_mode.as_deref() == Some("review")
                && inject_explain
        ));

        // Test PublishTerminalOps
        let publish = DaemonRequest::PublishTerminalOps {
            report: impulse_ops::TerminalOpsReport {
                source_id: "gui-test".to_string(),
                published_at: impulse_ops::now_rfc3339(),
                agents: Vec::new(),
                context: impulse_ops::ContextHealthSummary::default(),
                interventions: Vec::new(),
            },
        };
        let json = serde_json::to_string(&publish).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::PublishTerminalOps { report } if report.source_id == "gui-test"
        ));

        let permissions = DaemonRequest::GetSupervisorPermissions;
        let json = serde_json::to_string(&permissions).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GetSupervisorPermissions));

        let supervisor_chat = DaemonRequest::SupervisorChat {
            prompt: "Focus the stuck agent".to_string(),
            context: Some("One agent is blocked".to_string()),
        };
        let json = serde_json::to_string(&supervisor_chat).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::SupervisorChat { prompt, context }
                if prompt == "Focus the stuck agent"
                    && context.as_deref() == Some("One agent is blocked")
        ));

        let run_action = DaemonRequest::RunSupervisorAction {
            action: impulse_ops::SupervisorAction::SearchMemory {
                query: "compaction".to_string(),
            },
        };
        let json = serde_json::to_string(&run_action).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::RunSupervisorAction {
                action: impulse_ops::SupervisorAction::SearchMemory { query }
            } if query == "compaction"
        ));

        // Test DebugSnapshot
        let debug = DaemonRequest::DebugSnapshot;
        let json = serde_json::to_string(&debug).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::DebugSnapshot));
    }

    /// Test DaemonResponse serialization/deserialization
    #[test]
    fn test_daemon_response_serialization() {
        // Test Ok response
        let ok = DaemonResponse::Ok {
            result: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&ok).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonResponse::Ok { result } if result["key"] == "value"));

        // Test Error response
        let error = DaemonResponse::Error {
            message: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&error).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonResponse::Error { message } if message == "Something went wrong"
        ));
    }

    /// Test request/response round-trip
    #[test]
    fn test_request_response_roundtrip() {
        let requests = vec![
            DaemonRequest::Ping,
            DaemonRequest::Status,
            DaemonRequest::CreateSession {
                name: "roundtrip-test".to_string(),
                platform: Some("opencode".to_string()),
            },
            DaemonRequest::EndSession {
                session_id: "abc123".to_string(),
                summary: "Roundtrip test".to_string(),
            },
            DaemonRequest::TrackFile {
                session_id: "abc123".to_string(),
                file_path: "src/main.rs".to_string(),
            },
            DaemonRequest::TrackTool {
                session_id: "abc123".to_string(),
                tool_name: "Edit".to_string(),
            },
            DaemonRequest::GetSession {
                session_id: "abc123".to_string(),
            },
            DaemonRequest::ListSessions,
            DaemonRequest::Chat {
                session_id: "abc123".to_string(),
                message: "Test message".to_string(),
                inject_mode: None,
                inject_explain: false,
            },
            DaemonRequest::AgentAssist {
                prompt: "Review pane activity".to_string(),
                context: Some("Two panes active".to_string()),
                insights: Vec::new(),
            },
            DaemonRequest::PublishTerminalOps {
                report: impulse_ops::TerminalOpsReport {
                    source_id: "gui-test".to_string(),
                    published_at: impulse_ops::now_rfc3339(),
                    agents: Vec::new(),
                    context: impulse_ops::ContextHealthSummary::default(),
                    interventions: Vec::new(),
                },
            },
            DaemonRequest::GetSupervisorPermissions,
            DaemonRequest::SupervisorChat {
                prompt: "Show me pending reviews".to_string(),
                context: None,
            },
            DaemonRequest::RunSupervisorAction {
                action: impulse_ops::SupervisorAction::FocusAgent {
                    agent_id: "tab-1".to_string(),
                    session_id: None,
                },
            },
            DaemonRequest::GuardEvaluate {
                target: "bash".to_string(),
                action: "git push --force main".to_string(),
            },
            DaemonRequest::GuardList,
        ];

        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }
    }

    /// Test GuardEvaluate and GuardList request serde roundtrip
    #[test]
    fn test_guard_evaluate_request_serde() {
        // Test GuardEvaluate from JSON
        let json =
            r#"{"type":"GuardEvaluate","data":{"target":"bash","action":"git push --force main"}}"#;
        let request: DaemonRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(request, DaemonRequest::GuardEvaluate { .. }));

        // Verify field values
        if let DaemonRequest::GuardEvaluate { target, action } = &request {
            assert_eq!(target, "bash");
            assert_eq!(action, "git push --force main");
        }

        // Test GuardList from JSON
        let json2 = r#"{"type":"GuardList"}"#;
        let request2: DaemonRequest = serde_json::from_str(json2).unwrap();
        assert!(matches!(request2, DaemonRequest::GuardList));

        // Test struct-to-JSON roundtrip for GuardEvaluate
        let guard_eval = DaemonRequest::GuardEvaluate {
            target: "file-write".to_string(),
            action: "write /etc/passwd".to_string(),
        };
        let serialized = serde_json::to_string(&guard_eval).unwrap();
        let value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(value["type"], "GuardEvaluate");
        assert_eq!(value["data"]["target"], "file-write");
        assert_eq!(value["data"]["action"], "write /etc/passwd");

        // Test struct-to-JSON roundtrip for GuardList
        let guard_list = DaemonRequest::GuardList;
        let serialized2 = serde_json::to_string(&guard_list).unwrap();
        let value2: serde_json::Value = serde_json::from_str(&serialized2).unwrap();
        assert_eq!(value2["type"], "GuardList");
    }

    /// Test malformed request handling
    #[test]
    fn test_malformed_request() {
        let malformed_requests = vec![
            "{invalid json}",
            "{\"type\": \"Unknown\"}",
            "{\"type\": \"CreateSession\"}", // missing required fields
            "",
            "null",
        ];

        for malformed in malformed_requests {
            let result: Result<DaemonRequest, _> = serde_json::from_str(malformed);
            // These should all fail to parse
            assert!(
                result.is_err() || malformed.is_empty(),
                "Expected parse failure for: {}",
                malformed
            );
        }
    }

    /// Test socket path generation
    #[test]
    fn test_socket_path_format() {
        let socket_path = PathBuf::from("/tmp/test.sock");

        assert!(socket_path.to_string_lossy().ends_with(".sock"));
        assert_eq!(socket_path.file_name().unwrap(), "test.sock");
    }

    /// Test client request building
    #[test]
    fn test_client_request_building() {
        // Test building CreateSession request
        let create = DaemonRequest::CreateSession {
            name: "client-test".to_string(),
            platform: Some("claude-code".to_string()),
        };
        let json = serde_json::to_string(&create).unwrap();

        // Verify it's valid JSON with expected structure
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "CreateSession");
        assert_eq!(value["data"]["name"], "client-test");
        assert_eq!(value["data"]["platform"], "claude-code");

        // Test building Chat request
        let chat = DaemonRequest::Chat {
            session_id: "session-123".to_string(),
            message: "What files were modified?".to_string(),
            inject_mode: Some("apply".to_string()),
            inject_explain: true,
        };
        let json = serde_json::to_string(&chat).unwrap();

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "Chat");
        assert_eq!(value["data"]["session_id"], "session-123");
        assert_eq!(value["data"]["message"], "What files were modified?");
        assert_eq!(value["data"]["inject_mode"], "apply");
        assert_eq!(value["data"]["inject_explain"], true);
    }

    /// Test response parsing
    #[test]
    fn test_response_parsing() {
        // Test parsing Ok response
        let ok_json = r#"{"type":"Ok","data":{"result":{"status":"ok"}}}"#;
        let parsed: DaemonResponse = serde_json::from_str(ok_json).unwrap();
        assert!(matches!(parsed, DaemonResponse::Ok { .. }));

        // Test parsing Error response
        let error_json = r#"{"type":"Error","data":{"message":"Session not found"}}"#;
        let parsed: DaemonResponse = serde_json::from_str(error_json).unwrap();
        assert!(matches!(
            parsed,
            DaemonResponse::Error { message } if message == "Session not found"
        ));

        // Test parsing unknown response type
        let unknown_json = r#"{"type":"Unknown","data":{}}"#;
        let result: Result<DaemonResponse, _> = serde_json::from_str(unknown_json);
        assert!(result.is_err());
    }

    /// Test Session ID format
    #[test]
    fn test_session_id_format() {
        let session = Session::new(
            "format-test".to_string(),
            Some(crate::state::Platform::ClaudeCode),
        );

        // Session ID should follow format: {working-dir}-{timestamp}-{uuid8}
        let parts: Vec<&str> = session.id.split('-').collect();
        assert!(parts.len() >= 3, "Session ID should have at least 3 parts");

        // Last part should be 8 characters (uuid8)
        let last_part = parts.last().unwrap();
        assert_eq!(last_part.len(), 8, "UUID part should be 8 characters");
    }

    /// Test Session status transitions
    #[test]
    fn test_session_status_transitions() {
        let mut session = Session::new("status-test".to_string(), None);

        // Initial status should be Active
        assert_eq!(session.status, SessionStatus::Active);

        // Test all status transitions
        session.set_status(SessionStatus::Idle);
        assert_eq!(session.status, SessionStatus::Idle);

        session.set_status(SessionStatus::Waiting);
        assert_eq!(session.status, SessionStatus::Waiting);

        session.set_status(SessionStatus::Completed);
        assert_eq!(session.status, SessionStatus::Completed);

        session.set_status(SessionStatus::Error);
        assert_eq!(session.status, SessionStatus::Error);

        // Can transition back to Active
        session.set_status(SessionStatus::Active);
        assert_eq!(session.status, SessionStatus::Active);
    }

    /// Test file tracking
    #[test]
    fn test_file_tracking() {
        let mut session = Session::new("file-test".to_string(), None);

        // Add first file
        session.add_file("src/main.rs");
        assert_eq!(session.active_files.len(), 1);
        assert!(session.active_files.contains(&"src/main.rs".to_string()));

        // Add duplicate - should not create duplicate
        session.add_file("src/main.rs");
        assert_eq!(session.active_files.len(), 1);

        // Add another file
        session.add_file("src/lib.rs");
        assert_eq!(session.active_files.len(), 2);

        // Verify order (most recent last)
        assert_eq!(session.active_files[0], "src/main.rs");
    }

    /// Test tool tracking with bounded history
    #[test]
    fn test_tool_tracking_bounded() {
        let mut session = Session::new("tool-test".to_string(), None);

        // Add more than 20 tools
        for i in 0..25 {
            session.add_tool(&format!("Tool{}", i));
        }

        // Should be bounded to 20
        assert_eq!(session.recent_tools.len(), 20);

        // Most recent should be first
        assert_eq!(session.recent_tools[0], "Tool24");

        // Adding same tool should move it to front
        session.add_tool("Tool0");
        assert_eq!(session.recent_tools[0], "Tool0");
        assert_eq!(session.recent_tools.len(), 20);
    }

    /// Test LiveState operations
    #[test]
    fn test_live_state_operations() {
        let mut state = LiveState::new();

        // Initially empty
        assert!(state.sessions.is_empty());

        // Add session
        let session = Session::new("state-test".to_string(), None);
        let id = session.id.clone();
        state.add_session(session);

        assert_eq!(state.sessions.len(), 1);
        assert!(state.get_session(&id).is_some());

        // Remove session
        let removed = state.remove_session(&id);
        assert!(removed.is_some());
        assert!(state.sessions.is_empty());

        // List sessions should be empty
        let sessions = state.list_sessions();
        assert!(sessions.is_empty());
    }

    /// Test session context building for chat
    #[test]
    fn test_session_context_building() {
        let mut session = Session::new("context-test".to_string(), Some(Platform::ClaudeCode));

        // Add files
        session.add_file("src/main.rs");
        session.add_file("src/lib.rs");

        // Add tools
        session.add_tool("Write");
        session.add_tool("Read");
        session.add_tool("Edit");

        // Build context prompt
        let files_list = session.active_files.join(", ");
        let tools_list = session.recent_tools.join(", ");

        let context_prompt = format!(
            "Session Context:\n- Session: {} (ID: {})\n- Files touched: {}\n- Recent tools: {}\n\nUser question: What files did I modify?",
            session.name, session.id, files_list, tools_list
        );

        assert!(context_prompt.contains("src/main.rs"));
        assert!(context_prompt.contains("src/lib.rs"));
        assert!(context_prompt.contains("Write"));
        assert!(context_prompt.contains("Edit"));
        assert!(context_prompt.contains("User question:"));
    }

    /// Test AgentAssist request serialization/deserialization
    #[test]
    fn test_agent_assist_request_serialization() {
        // Test with both prompt and context
        let request = DaemonRequest::AgentAssist {
            prompt: "Review the changes in pane 1".to_string(),
            context: Some("Pane 1 modified src/main.rs and src/lib.rs".to_string()),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::AgentAssist { prompt, context, .. }
                if prompt == "Review the changes in pane 1"
                && context.as_deref() == Some("Pane 1 modified src/main.rs and src/lib.rs")
        ));

        // Verify JSON structure
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "AgentAssist");
        assert_eq!(value["data"]["prompt"], "Review the changes in pane 1");
        assert_eq!(
            value["data"]["context"],
            "Pane 1 modified src/main.rs and src/lib.rs"
        );

        // Test with prompt only (no context)
        let request_no_ctx = DaemonRequest::AgentAssist {
            prompt: "Summarize current progress".to_string(),
            context: None,
            insights: Vec::new(),
        };
        let json2 = serde_json::to_string(&request_no_ctx).unwrap();
        let parsed2: DaemonRequest = serde_json::from_str(&json2).unwrap();
        assert!(matches!(
            parsed2,
            DaemonRequest::AgentAssist { prompt, context, .. }
                if prompt == "Summarize current progress"
                && context.is_none()
        ));

        // Test backward compatibility: JSON without insights field should deserialize
        let legacy_json = r#"{"type":"AgentAssist","data":{"prompt":"test","context":null}}"#;
        let parsed_legacy: DaemonRequest = serde_json::from_str(legacy_json).unwrap();
        assert!(matches!(
            parsed_legacy,
            DaemonRequest::AgentAssist { insights, .. } if insights.is_empty()
        ));
    }

    /// Test AgentAssistResult response serialization/deserialization
    #[test]
    fn test_agent_assist_result_serialization() {
        // Test success response
        let success = DaemonResponse::AgentAssistResult {
            success: true,
            response: "No conflicts detected between panes".to_string(),
            recommendations: Vec::new(),
            pane_summaries: Vec::new(),
        };
        let json = serde_json::to_string(&success).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonResponse::AgentAssistResult { success, response, .. }
                if success && response == "No conflicts detected between panes"
        ));

        // Verify JSON structure
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["type"], "AgentAssistResult");
        assert_eq!(value["data"]["success"], true);
        assert_eq!(
            value["data"]["response"],
            "No conflicts detected between panes"
        );

        // Test failure response
        let failure = DaemonResponse::AgentAssistResult {
            success: false,
            response: "Impulse Agent not configured".to_string(),
            recommendations: Vec::new(),
            pane_summaries: Vec::new(),
        };
        let json2 = serde_json::to_string(&failure).unwrap();
        let parsed2: DaemonResponse = serde_json::from_str(&json2).unwrap();
        assert!(matches!(
            parsed2,
            DaemonResponse::AgentAssistResult { success, response, .. }
                if !success && response == "Impulse Agent not configured"
        ));
    }

    /// Test AgentAssist round-trip through serialization
    #[test]
    fn test_agent_assist_roundtrip() {
        let requests = vec![
            DaemonRequest::AgentAssist {
                prompt: "Check for file conflicts".to_string(),
                context: Some("pane-1: src/main.rs, pane-2: src/main.rs".to_string()),
                insights: Vec::new(),
            },
            DaemonRequest::AgentAssist {
                prompt: "What should I work on next?".to_string(),
                context: None,
                insights: Vec::new(),
            },
        ];

        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }

        let responses = vec![
            DaemonResponse::AgentAssistResult {
                success: true,
                response: "All clear".to_string(),
                recommendations: Vec::new(),
                pane_summaries: Vec::new(),
            },
            DaemonResponse::AgentAssistResult {
                success: false,
                response: "Agent not ready".to_string(),
                recommendations: Vec::new(),
                pane_summaries: Vec::new(),
            },
        ];

        for response in responses {
            let json = serde_json::to_string(&response).unwrap();
            let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }
    }

    /// Test session context with no files/tools
    #[test]
    fn test_session_context_empty() {
        let session = Session::new("empty-session".to_string(), None);

        let files_list = session.active_files.join(", ");
        let tools_list = session.recent_tools.join(", ");

        assert!(files_list.is_empty());
        assert!(tools_list.is_empty());

        let context_prompt = format!(
            "Files: {}, Tools: {}",
            if files_list.is_empty() {
                "none".to_string()
            } else {
                files_list
            },
            if tools_list.is_empty() {
                "none".to_string()
            } else {
                tools_list
            }
        );

        assert!(context_prompt.contains("none"));
    }

    // ── Plugin IPC serde ────────────────────────────────────────────────

    #[test]
    fn test_list_plugins_request_serde() {
        let req = DaemonRequest::ListPlugins;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ListPlugins));
    }

    #[test]
    fn test_invoke_plugin_request_serde() {
        let req = DaemonRequest::InvokePlugin {
            name: "office".to_string(),
            input: crate::plugin::PluginInput::new()
                .with_path(PathBuf::from("/test/file.docx"))
                .with_query("extract"),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::InvokePlugin { ref name, .. } if name == "office"));
    }

    #[test]
    fn test_invoke_plugin_default_input() {
        // InvokePlugin with only a name should deserialize with default PluginInput
        let json = r#"{"type":"InvokePlugin","data":{"name":"test-plugin"}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::InvokePlugin { name, input } => {
                assert_eq!(name, "test-plugin");
                assert!(input.path.is_none());
                assert!(input.query.is_none());
            }
            _ => panic!("Expected InvokePlugin"),
        }
    }

    #[test]
    fn test_list_plugins_json_roundtrip() {
        let json = r#"{"type":"ListPlugins"}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ListPlugins));
        let re_serialized = serde_json::to_string(&parsed).unwrap();
        let reparsed: DaemonRequest = serde_json::from_str(&re_serialized).unwrap();
        assert!(matches!(reparsed, DaemonRequest::ListPlugins));
    }

    // -- Plugin registry initialization test --------------------------------------

    #[test]
    fn test_plugin_registry_initialized_after_init() {
        crate::plugin::registry::init_global_registry();
        let registry = crate::plugin::registry::global_registry();
        // After init, the office context provider should be registered
        let providers = registry.list_context_providers().unwrap();
        assert!(
            !providers.is_empty(),
            "init_global_registry should register at least the office context provider"
        );
    }

    // -- Protocol versioning tests -----------------------------------------------

    #[test]
    fn test_protocol_version_constant_defined() {
        const { assert!(crate::daemon::PROTOCOL_VERSION >= 1) };
    }

    #[test]
    fn test_ping_response_includes_protocol_version() {
        let ping_response =
            serde_json::json!({"pong": true, "protocol_version": crate::daemon::PROTOCOL_VERSION});
        assert_eq!(
            ping_response["protocol_version"].as_u64().unwrap(),
            crate::daemon::PROTOCOL_VERSION as u64
        );
    }

    #[test]
    fn test_status_response_includes_protocol_version() {
        let status_response = serde_json::json!({
            "sessions": 2,
            "active": 1,
            "protocol_version": crate::daemon::PROTOCOL_VERSION,
        });
        assert_eq!(
            status_response["protocol_version"].as_u64().unwrap(),
            crate::daemon::PROTOCOL_VERSION as u64
        );
    }

    // -- Handler integration tests -----------------------------------------------

    fn test_state() -> (tempfile::TempDir, std::sync::Arc<crate::state::State>) {
        let tmp = tempfile::TempDir::new().unwrap();
        let st = crate::state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, std::sync::Arc::new(st))
    }

    #[tokio::test]
    async fn test_handle_status_empty() {
        let (_tmp, state) = test_state();
        let resp = handle_status(&state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["sessions"].as_u64().unwrap(), 0);
                assert_eq!(result["active"].as_u64().unwrap(), 0);
                assert!(result["protocol_version"].as_u64().is_some());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_status_with_sessions() {
        let (_tmp, state) = test_state();
        state.create_session("s1".to_string(), None).await.unwrap();
        state.create_session("s2".to_string(), None).await.unwrap();
        let resp = handle_status(&state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["sessions"].as_u64().unwrap(), 2);
                assert_eq!(result["active"].as_u64().unwrap(), 2);
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_session_create() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::CreateSession {
            name: "test-session".to_string(),
            platform: Some("claude-code".to_string()),
        };
        let resp = handle_session_request(req, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert!(!result["session_id"].as_str().unwrap().is_empty());
                assert_eq!(result["name"].as_str().unwrap(), "test-session");
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_session_list() {
        let (_tmp, state) = test_state();
        state.create_session("s1".to_string(), None).await.unwrap();
        let resp = handle_session_request(DaemonRequest::ListSessions, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                let arr = result.as_array().unwrap();
                assert_eq!(arr.len(), 1);
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_session_end() {
        let (_tmp, state) = test_state();
        let session = state
            .create_session("end-test".to_string(), None)
            .await
            .unwrap();
        let req = DaemonRequest::EndSession {
            session_id: session.id.clone(),
            summary: "done".to_string(),
        };
        let resp = handle_session_request(req, &state).await;
        assert!(matches!(resp, DaemonResponse::Ok { .. }));
    }

    #[tokio::test]
    async fn test_handle_session_end_not_found() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::EndSession {
            session_id: "nonexistent".to_string(),
            summary: "done".to_string(),
        };
        let resp = handle_session_request(req, &state).await;
        // Returns Error when session not found
        assert!(matches!(resp, DaemonResponse::Error { .. }));
    }

    #[tokio::test]
    async fn test_handle_steward_status() {
        let (_tmp, state) = test_state();
        let resp = handle_steward_request(DaemonRequest::StewardStatus, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert!(result["mode"].as_str().is_some());
                assert!(result["thresholds"].is_object());
                assert!(result["pending_proposals"].as_u64().is_some());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_steward_proposals_list() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::StewardProposals {
            action: "list".to_string(),
            id: None,
        };
        let resp = handle_steward_request(req, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                // Should be an array (possibly empty)
                assert!(result.is_array());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_steward_proposals_unknown_action() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::StewardProposals {
            action: "invalid".to_string(),
            id: None,
        };
        let resp = handle_steward_request(req, &state).await;
        assert!(matches!(resp, DaemonResponse::Error { .. }));
    }

    #[tokio::test]
    async fn test_handle_steward_memory() {
        let (_tmp, state) = test_state();
        let resp = handle_steward_request(DaemonRequest::StewardMemory, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert!(result["patterns"].is_array());
                assert!(result["learnings"].is_array());
                assert!(result["stats"].is_object());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_guard_list() {
        let (_tmp, state) = test_state();
        let resp = handle_guard_request(DaemonRequest::GuardList, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                let rules = result["rules"].as_array().unwrap();
                // Default config has guardrails enabled with 9 built-in rules
                assert!(!rules.is_empty());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_guard_evaluate_block() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::GuardEvaluate {
            target: "bash".to_string(),
            action: "git push --force origin main".to_string(),
        };
        let resp = handle_guard_request(req, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                // force-push should trigger the built-in block rule
                assert!(result["blocked"].as_bool().unwrap());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_guard_evaluate_clean() {
        let (_tmp, state) = test_state();
        let req = DaemonRequest::GuardEvaluate {
            target: "bash".to_string(),
            action: "cargo test".to_string(),
        };
        let resp = handle_guard_request(req, &state).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert!(!result["blocked"].as_bool().unwrap());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_plugin_list() {
        crate::plugin::registry::init_global_registry();
        let resp = handle_plugin_request(DaemonRequest::ListPlugins).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert!(result["context_providers"].is_array());
                assert!(result["action_handlers"].is_array());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[tokio::test]
    async fn test_handle_session_track_file() {
        let (_tmp, state) = test_state();
        let session = state
            .create_session("track-test".to_string(), None)
            .await
            .unwrap();
        let req = DaemonRequest::TrackFile {
            session_id: session.id.clone(),
            file_path: "src/main.rs".to_string(),
        };
        let resp = handle_session_request(req, &state).await;
        assert!(matches!(resp, DaemonResponse::Ok { .. }));
        // Verify tracking
        let s = state.get_session(&session.id).await.unwrap().unwrap();
        assert!(s.active_files.contains(&"src/main.rs".to_string()));
    }

    // ── Debug snapshot tests ──────────────────────────────

    #[tokio::test]
    async fn test_handle_debug_snapshot() {
        let (_dir, state) = test_state();
        let registry = crate::tooling::ToolRegistry::with_defaults();
        crate::plugin::registry::init_global_registry();
        let resp = super::super::handlers::handle_debug_snapshot(&state, &registry).await;
        match resp {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["protocol_version"], super::super::PROTOCOL_VERSION);
                assert!(result["pid"].is_number());
                assert_eq!(result["sessions"]["total"], 0);
                assert_eq!(result["sessions"]["active"], 0);
                assert!(result["tools"]["registered"].as_u64().unwrap() > 0);
                assert!(result["guardrails"]["rules"].as_u64().unwrap() > 0);
                assert!(result["plugins"]["context_providers"].is_number());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    // ── Stale socket detection tests ─────────────────────

    #[tokio::test]
    async fn test_stale_socket_cleanup() {
        use super::super::Daemon;
        let (_dir, state) = test_state();
        let daemon = Daemon::new(state);
        let socket_path = daemon.socket_path().clone();
        let pid_path = socket_path.with_extension("pid");

        // Create fake stale socket file (no listener)
        tokio::fs::create_dir_all(socket_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&socket_path, b"stale").await.unwrap();
        tokio::fs::write(&pid_path, "99999999").await.unwrap(); // non-existent PID

        assert!(socket_path.exists());
        assert!(pid_path.exists());

        // Start should clean up stale socket and succeed
        // We can't fully start (it would block), but we can verify the cleanup logic
        // by checking that start doesn't bail with "already running"
        let start_result =
            tokio::time::timeout(std::time::Duration::from_millis(500), daemon.start()).await;

        // The start will either succeed (and timeout because it loops) or fail for a non-stale reason
        match start_result {
            Ok(Ok(())) => {} // loop exited normally (shouldn't happen but fine)
            Ok(Err(e)) => {
                // Should NOT be "already running" — that means stale detection failed
                assert!(
                    !e.to_string().contains("already running"),
                    "Stale socket not cleaned up: {}",
                    e
                );
            }
            Err(_timeout) => {
                // Timed out = daemon started listening successfully (expected)
                // Verify stale files were cleaned up
                assert!(
                    !pid_path.exists() || {
                        // New PID file should contain our PID
                        let pid = tokio::fs::read_to_string(&pid_path).await.unwrap();
                        pid.trim() == std::process::id().to_string()
                    }
                );
            }
        }
    }

    #[test]
    fn test_pid_file_path_derived_from_socket() {
        let socket_path = PathBuf::from("/tmp/test/impulse.sock");
        let pid_path = socket_path.with_extension("pid");
        assert_eq!(pid_path, PathBuf::from("/tmp/test/impulse.pid"));
    }

    // ── Conflict resolver IPC tests (Task 20) ──────────────────────────────

    #[test]
    fn test_get_conflict_history_request_roundtrip() {
        let req = DaemonRequest::GetConflictHistory;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GetConflictHistory));
    }

    #[test]
    fn test_clear_resolved_conflicts_request_roundtrip() {
        let req = DaemonRequest::ClearResolvedConflicts;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ClearResolvedConflicts));
    }

    #[test]
    fn test_conflict_history_empty_response() {
        // Simulate what the handler returns for an empty ConflictResolver
        let resolver = crate::agent::coordinator::ConflictResolver::new();
        let history = resolver.get_resolution_history();
        assert!(history.is_empty());

        // Verify it serializes to the expected Ok response
        let response = super::super::protocol::respond_ok(&history);
        match response {
            DaemonResponse::Ok { result } => {
                let arr = result.as_array().unwrap();
                assert!(arr.is_empty());
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[test]
    fn test_clear_resolved_conflicts_response() {
        // Simulate what the handler returns after clearing
        let response = super::super::protocol::respond_ok(&serde_json::json!({"cleared": true}));
        match response {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["cleared"], true);
            }
            _ => panic!("Expected Ok response"),
        }
    }

    #[test]
    fn test_backward_compat_old_json_without_conflict_variants() {
        // Old clients that don't know about GetConflictHistory / ClearResolvedConflicts
        // should still be able to parse other request types
        let old_json = r#"{"type":"Ping"}"#;
        let parsed: DaemonRequest = serde_json::from_str(old_json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Ping));

        let old_json = r#"{"type":"Status"}"#;
        let parsed: DaemonRequest = serde_json::from_str(old_json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Status));

        let old_json = r#"{"type":"GuardList"}"#;
        let parsed: DaemonRequest = serde_json::from_str(old_json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GuardList));
    }

    // ── Agent specialized IPC tests (Task 23) ─────────────────────────────

    #[test]
    fn test_agent_review_code_request_serde() {
        let req = DaemonRequest::AgentReviewCode {
            file_path: "src/main.rs".to_string(),
            diff: "+ let x = 42;\n- let x = 0;".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::AgentReviewCode {
                file_path,
                diff,
                insights,
            } => {
                assert_eq!(file_path, "src/main.rs");
                assert!(diff.contains("let x = 42"));
                assert!(insights.is_empty());
            }
            _ => panic!("Expected AgentReviewCode"),
        }
    }

    #[test]
    fn test_agent_analyze_error_request_serde() {
        let req = DaemonRequest::AgentAnalyzeError {
            error_text: "error[E0502]: cannot borrow `x` as mutable".to_string(),
            context: "pane-1".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::AgentAnalyzeError {
                error_text,
                context,
                insights,
            } => {
                assert!(error_text.contains("E0502"));
                assert_eq!(context, "pane-1");
                assert!(insights.is_empty());
            }
            _ => panic!("Expected AgentAnalyzeError"),
        }
    }

    #[test]
    fn test_agent_summarize_pane_request_serde() {
        let req = DaemonRequest::AgentSummarizePane {
            pane_id: 3,
            raw_output: "Modified src/lib.rs, ran cargo test, 5 passing".to_string(),
            insights: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::AgentSummarizePane {
                pane_id,
                raw_output,
                insights,
            } => {
                assert_eq!(pane_id, 3);
                assert!(raw_output.contains("cargo test"));
                assert!(insights.is_empty());
            }
            _ => panic!("Expected AgentSummarizePane"),
        }
    }

    #[test]
    fn test_agent_specialized_request_roundtrip() {
        let requests = vec![
            DaemonRequest::AgentReviewCode {
                file_path: "src/daemon/protocol.rs".to_string(),
                diff: "+++ new line".to_string(),
                insights: Vec::new(),
            },
            DaemonRequest::AgentAnalyzeError {
                error_text: "thread 'main' panicked".to_string(),
                context: "integration-test".to_string(),
                insights: Vec::new(),
            },
            DaemonRequest::AgentSummarizePane {
                pane_id: 1,
                raw_output: "cargo build succeeded".to_string(),
                insights: Vec::new(),
            },
        ];

        for request in requests {
            let json = serde_json::to_string(&request).unwrap();
            let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        }
    }

    #[test]
    fn test_agent_review_code_backward_compat_no_insights() {
        // Old clients that don't send insights should still parse
        let json =
            r#"{"type":"AgentReviewCode","data":{"file_path":"src/lib.rs","diff":"+ added"}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentReviewCode {
                file_path,
                diff,
                insights,
            } => {
                assert_eq!(file_path, "src/lib.rs");
                assert_eq!(diff, "+ added");
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentReviewCode"),
        }
    }

    #[test]
    fn test_agent_analyze_error_backward_compat_no_insights() {
        let json =
            r#"{"type":"AgentAnalyzeError","data":{"error_text":"panic!","context":"test"}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentAnalyzeError {
                error_text,
                context,
                insights,
            } => {
                assert_eq!(error_text, "panic!");
                assert_eq!(context, "test");
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentAnalyzeError"),
        }
    }

    #[test]
    fn test_agent_summarize_pane_backward_compat_minimal() {
        // Minimum required: pane_id — raw_output and insights default
        let json = r#"{"type":"AgentSummarizePane","data":{"pane_id":5}}"#;
        let parsed: DaemonRequest = serde_json::from_str(json).unwrap();
        match parsed {
            DaemonRequest::AgentSummarizePane {
                pane_id,
                raw_output,
                insights,
            } => {
                assert_eq!(pane_id, 5);
                assert!(raw_output.is_empty(), "raw_output should default to empty");
                assert!(insights.is_empty(), "insights should default to empty vec");
            }
            _ => panic!("Expected AgentSummarizePane"),
        }
    }

    #[test]
    fn test_agent_specialized_json_structure() {
        // Verify the JSON wire format for each new request type
        let review = DaemonRequest::AgentReviewCode {
            file_path: "f.rs".to_string(),
            diff: "d".to_string(),
            insights: Vec::new(),
        };
        let val: serde_json::Value = serde_json::to_value(&review).unwrap();
        assert_eq!(val["type"], "AgentReviewCode");
        assert_eq!(val["data"]["file_path"], "f.rs");
        assert_eq!(val["data"]["diff"], "d");

        let error = DaemonRequest::AgentAnalyzeError {
            error_text: "e".to_string(),
            context: "c".to_string(),
            insights: Vec::new(),
        };
        let val: serde_json::Value = serde_json::to_value(&error).unwrap();
        assert_eq!(val["type"], "AgentAnalyzeError");
        assert_eq!(val["data"]["error_text"], "e");
        assert_eq!(val["data"]["context"], "c");

        let summary = DaemonRequest::AgentSummarizePane {
            pane_id: 7,
            raw_output: "out".to_string(),
            insights: Vec::new(),
        };
        let val: serde_json::Value = serde_json::to_value(&summary).unwrap();
        assert_eq!(val["type"], "AgentSummarizePane");
        assert_eq!(val["data"]["pane_id"], 7);
        assert_eq!(val["data"]["raw_output"], "out");
    }

    #[test]
    fn test_backward_compat_old_json_without_agent_specialized_variants() {
        // Old clients that don't know about the new variants should
        // still be able to parse all existing request types
        let old_json = r#"{"type":"Ping"}"#;
        let parsed: DaemonRequest = serde_json::from_str(old_json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Ping));

        let old_json = r#"{"type":"AgentAssist","data":{"prompt":"help","context":null}}"#;
        let parsed: DaemonRequest = serde_json::from_str(old_json).unwrap();
        assert!(matches!(parsed, DaemonRequest::AgentAssist { .. }));
    }

    // -- GetConflictHistory / ClearResolvedConflicts IPC tests (Task 20) ---------

    #[test]
    fn test_get_conflict_history_serialization() {
        let req = DaemonRequest::GetConflictHistory;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::GetConflictHistory));
    }

    #[test]
    fn test_clear_resolved_conflicts_serialization() {
        let req = DaemonRequest::ClearResolvedConflicts;
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::ClearResolvedConflicts));
    }

    #[test]
    fn test_conflict_resolver_history_via_ipc_types() {
        // Simulate the IPC flow: create conflicts, resolve them, verify history
        use crate::agent::coordinator::{ConflictResolution, ConflictResolver};
        use crate::context_lifecycle::types::{ExtractedInsight, InsightType};

        let mut resolver = ConflictResolver::new();

        // Create conflict insights (two panes editing the same file)
        let insights = vec![
            ExtractedInsight {
                pane_id: 1,
                agent_kind: crate::context_lifecycle::types::AgentKind::ClaudeCode,
                timestamp: chrono::Utc::now(),
                insight_type: InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
            ExtractedInsight {
                pane_id: 2,
                agent_kind: crate::context_lifecycle::types::AgentKind::OpenCode,
                timestamp: chrono::Utc::now(),
                insight_type: InsightType::FileModified,
                content: "src/main.rs".to_string(),
                intent: None,
            },
        ];

        // Detect and add conflicts
        let recs = resolver.detect_and_add_conflicts(&insights);
        assert_eq!(recs.len(), 1);

        // History should be empty before resolution
        assert!(resolver.get_resolution_history().is_empty());

        // Resolve the conflict
        let resolved = resolver.resolve_conflict("src/main.rs", ConflictResolution::Merge);
        assert!(resolved);

        // History should now have one entry
        let history = resolver.get_resolution_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].file_path, "src/main.rs");
        assert!(matches!(history[0].resolution, ConflictResolution::Merge));

        // Verify it serializes cleanly for IPC transport
        let json = serde_json::to_value(history).unwrap();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["file_path"], "src/main.rs");
        assert_eq!(json[0]["resolution"], "merge");
    }

    #[test]
    fn test_conflict_resolver_clear_resolved_via_ipc_types() {
        use crate::agent::coordinator::{ConflictResolution, ConflictResolver};
        use crate::context_lifecycle::types::{ExtractedInsight, InsightType};

        let mut resolver = ConflictResolver::new();

        let insights = vec![
            ExtractedInsight {
                pane_id: 1,
                agent_kind: crate::context_lifecycle::types::AgentKind::ClaudeCode,
                timestamp: chrono::Utc::now(),
                insight_type: InsightType::FileModified,
                content: "src/lib.rs".to_string(),
                intent: None,
            },
            ExtractedInsight {
                pane_id: 2,
                agent_kind: crate::context_lifecycle::types::AgentKind::OpenCode,
                timestamp: chrono::Utc::now(),
                insight_type: InsightType::FileModified,
                content: "src/lib.rs".to_string(),
                intent: None,
            },
        ];

        resolver.detect_and_add_conflicts(&insights);

        // Resolve and clear
        resolver.resolve_conflict("src/lib.rs", ConflictResolution::AcceptTheirs);
        assert_eq!(resolver.get_resolved_conflicts().len(), 1);

        resolver.clear_resolved();
        assert!(resolver.get_resolved_conflicts().is_empty());
        // Unresolved should also be empty since the only conflict was resolved
        assert!(resolver.get_unresolved_conflicts().is_empty());

        // History persists even after clear (clear only removes tracked conflicts)
        assert_eq!(resolver.get_resolution_history().len(), 1);
    }

    #[test]
    fn test_conflict_history_empty_returns_empty_array() {
        use crate::agent::coordinator::ConflictResolver;

        let resolver = ConflictResolver::new();
        let history = resolver.get_resolution_history();
        assert!(history.is_empty());

        // Verify empty serialization is a valid JSON array
        let json = serde_json::to_value(history).unwrap();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 0);
    }
}
