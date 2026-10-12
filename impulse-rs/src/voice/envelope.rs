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
