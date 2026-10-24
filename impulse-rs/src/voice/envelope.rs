//! ElevenLabs-shaped tool-call request and result envelopes.
//!
//! Field names follow common ElevenLabs client-tool / webhook payloads so
//! dashboard tools can map parameter names case-sensitively onto Impulse tool
//! ids and argument keys.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where the tool call originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceToolCallSource {
    ClientTool,
    Webhook,
}

/// Normalized client-tool (or webhook) invocation from an ElevenLabs agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElevenLabsClientToolRequest {
    /// Correlation id from the agent turn (optional for local CLI).
    #[serde(default, alias = "tool_call_id", alias = "id")]
    pub tool_call_id: Option<String>,
    /// Case-sensitive Impulse tool id (e.g. `system_info`, `bash_exec`).
    #[serde(alias = "tool_name", alias = "name")]
    pub tool: String,
    /// Tool parameters object (JSON object preferred).
    #[serde(default, alias = "parameters", alias = "arguments")]
    pub params: Value,
    /// Explicit human / policy confirmation for mutating tools.
    #[serde(default)]
    pub confirmed: bool,
    /// Whether the agent expects a tool result in context (wait-for-response).
    #[serde(default = "default_wait_for_response")]
    pub wait_for_response: bool,
    #[serde(default = "default_client_source")]
    pub source: VoiceToolCallSource,
}

fn default_wait_for_response() -> bool {
    true
}

fn default_client_source() -> VoiceToolCallSource {
    VoiceToolCallSource::ClientTool
}

impl ElevenLabsClientToolRequest {
    pub fn new(tool: impl Into<String>, params: Value) -> Self {
        Self {
            tool_call_id: None,
            tool: tool.into(),
            params,
            confirmed: false,
            wait_for_response: true,
            source: VoiceToolCallSource::ClientTool,
        }
    }
}

/// Outcome status for a tool round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElevenLabsToolResultStatus {
    Ok,
    Error,
    Denied,
}

/// Structured result returned to the conversation path (wait-for-response).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElevenLabsToolResult {
    pub status: ElevenLabsToolResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub tool: String,
    /// Primary payload for the agent context (success body or error detail).
    pub result: Value,
    /// True when this should be appended as tool output for the agent turn.
    pub wait_for_response: bool,
    /// Provider that handled the call (always elevenlabs_agent on this path).
    pub provider: String,
    /// Human-readable error when status is Error/Denied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ElevenLabsToolResult {
    pub fn ok(tool: impl Into<String>, tool_call_id: Option<String>, result: Value) -> Self {
        Self {
            status: ElevenLabsToolResultStatus::Ok,
            tool_call_id,
            tool: tool.into(),
            result,
            wait_for_response: true,
            provider: "elevenlabs_agent".into(),
            error: None,
        }
    }

    pub fn denied(
        tool: impl Into<String>,
        tool_call_id: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        let reason = reason.into();
        Self {
            status: ElevenLabsToolResultStatus::Denied,
            tool_call_id,
            tool: tool.into(),
            result: serde_json::json!({ "denied": true, "reason": reason }),
            wait_for_response: true,
            provider: "elevenlabs_agent".into(),
            error: Some(reason),
        }
    }

    pub fn error(
        tool: impl Into<String>,
        tool_call_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        let message = message.into();
        Self {
            status: ElevenLabsToolResultStatus::Error,
            tool_call_id,
            tool: tool.into(),
            result: serde_json::json!({ "error": message }),
            wait_for_response: true,
            provider: "elevenlabs_agent".into(),
            error: Some(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_without_confirmed_is_not_confirmed() {
        // A payload that omits the flag must never approve a mutating tool.
        let request: ElevenLabsClientToolRequest =
            serde_json::from_value(json!({"tool": "bash_exec", "params": {"command": "ls"}}))
                .unwrap();
        assert!(!request.confirmed);
        assert!(request.wait_for_response);
        assert_eq!(request.source, VoiceToolCallSource::ClientTool);
        assert_eq!(request.tool_call_id, None);
    }

    #[test]
    fn test_request_accepts_the_elevenlabs_field_aliases() {
        for payload in [
            json!({"tool_call_id": "c1", "tool_name": "system_info", "parameters": {"a": 1}}),
            json!({"id": "c1", "name": "system_info", "arguments": {"a": 1}}),
            json!({"tool_call_id": "c1", "tool": "system_info", "params": {"a": 1}}),
        ] {
            let request: ElevenLabsClientToolRequest = serde_json::from_value(payload).unwrap();
            assert_eq!(request.tool_call_id.as_deref(), Some("c1"));
            assert_eq!(request.tool, "system_info");
            assert_eq!(request.params, json!({"a": 1}));
        }
    }

    #[test]
    fn test_request_without_a_tool_name_is_rejected() {
        let result = serde_json::from_value::<ElevenLabsClientToolRequest>(json!({"params": {}}));
        assert!(result.is_err());
    }

    #[test]
    fn round_trip_client_tool_request() {
        let mut original = ElevenLabsClientToolRequest::new("file_read", json!({"path": "a"}));
        original.tool_call_id = Some("c2".into());
        original.confirmed = true;
        original.wait_for_response = false;
        original.source = VoiceToolCallSource::Webhook;
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("\"source\":\"webhook\""), "{json}");
        let recovered: ElevenLabsClientToolRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn test_tool_result_constructors_set_status_error_and_payload() {
        let ok = ElevenLabsToolResult::ok("system_info", Some("c1".into()), json!({"os": "x"}));
        assert_eq!(ok.status, ElevenLabsToolResultStatus::Ok);
        assert_eq!(ok.error, None);
        assert_eq!(ok.provider, "elevenlabs_agent");

        let denied = ElevenLabsToolResult::denied("bash_exec", None, "needs confirmation");
        assert_eq!(denied.status, ElevenLabsToolResultStatus::Denied);
        assert_eq!(denied.error.as_deref(), Some("needs confirmation"));
        assert_eq!(denied.result["denied"], true);

        let error = ElevenLabsToolResult::error("file_read", None, "not found");
        assert_eq!(error.status, ElevenLabsToolResultStatus::Error);
        assert_eq!(error.result["error"], "not found");
    }

    #[test]
    fn round_trip_tool_result_omits_absent_optionals() {
        for original in [
            ElevenLabsToolResult::ok("system_info", None, json!({"os": "x"})),
            ElevenLabsToolResult::denied("bash_exec", Some("c3".into()), "no"),
            ElevenLabsToolResult::error("file_read", None, "boom"),
        ] {
            let json = serde_json::to_value(&original).unwrap();
            if original.tool_call_id.is_none() {
                assert!(json.get("tool_call_id").is_none(), "{json}");
            }
            if original.error.is_none() {
                assert!(json.get("error").is_none(), "{json}");
            }
            let recovered: ElevenLabsToolResult = serde_json::from_value(json).unwrap();
            assert_eq!(recovered, original);
        }
        assert_eq!(
            serde_json::to_string(&ElevenLabsToolResultStatus::Denied).unwrap(),
            "\"denied\""
        );
    }
}
