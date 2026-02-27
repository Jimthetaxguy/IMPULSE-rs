//! Daemon IPC Tests
//! Tests for socket communication, request/response handling

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use std::path::PathBuf;

    use crate::daemon::{DaemonRequest, DaemonResponse};
    use crate::state::{LiveState, Platform, Session, SessionStatus};

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

    /// Test LiveState active_sessions filter
    #[test]
    fn test_live_state_active_filter() {
        let mut state = LiveState::new();

        // Add multiple sessions with different statuses
        let mut s1 = Session::new("active1".to_string(), None);
        s1.set_status(SessionStatus::Active);

        let mut s2 = Session::new("active2".to_string(), None);
        s2.set_status(SessionStatus::Active);

        let mut s3 = Session::new("idle".to_string(), None);
        s3.set_status(SessionStatus::Idle);

        let mut s4 = Session::new("completed".to_string(), None);
        s4.set_status(SessionStatus::Completed);

        state.add_session(s1);
        state.add_session(s2);
        state.add_session(s3);
        state.add_session(s4);

        let active: Vec<_> = state.active_sessions();
        assert_eq!(active.len(), 2);
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
        };
        let json = serde_json::to_string(&request).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonRequest::AgentAssist { prompt, context }
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
        };
        let json2 = serde_json::to_string(&request_no_ctx).unwrap();
        let parsed2: DaemonRequest = serde_json::from_str(&json2).unwrap();
        assert!(matches!(
            parsed2,
            DaemonRequest::AgentAssist { prompt, context }
                if prompt == "Summarize current progress"
                && context.is_none()
        ));
    }

    /// Test AgentAssistResult response serialization/deserialization
    #[test]
    fn test_agent_assist_result_serialization() {
        // Test success response
        let success = DaemonResponse::AgentAssistResult {
            success: true,
            response: "No conflicts detected between panes".to_string(),
        };
        let json = serde_json::to_string(&success).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            DaemonResponse::AgentAssistResult { success, response }
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
        };
        let json2 = serde_json::to_string(&failure).unwrap();
        let parsed2: DaemonResponse = serde_json::from_str(&json2).unwrap();
        assert!(matches!(
            parsed2,
            DaemonResponse::AgentAssistResult { success, response }
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
            },
            DaemonRequest::AgentAssist {
                prompt: "What should I work on next?".to_string(),
                context: None,
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
            },
            DaemonResponse::AgentAssistResult {
                success: false,
                response: "Agent not ready".to_string(),
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
}
