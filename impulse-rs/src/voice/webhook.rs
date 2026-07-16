//! Webhook-tool request parsing for ElevenLabs → local Impulse HTTP.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::envelope::{
    ElevenLabsClientToolRequest, VoiceToolCallSource,
};

/// Raw webhook body shape (subset of ElevenLabs webhook tool POST).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebhookToolRequest {
    #[serde(default, alias = "tool_call_id", alias = "id")]
    pub tool_call_id: Option<String>,
    #[serde(alias = "tool_name", alias = "name")]
    pub tool: String,
    #[serde(default, alias = "parameters", alias = "arguments", alias = "body")]
    pub params: Value,
    #[serde(default)]
    pub confirmed: bool,
}

/// Parse webhook JSON into the shared client-tool request envelope.
pub fn parse_webhook_tool_request(
    body: &[u8],
) -> Result<ElevenLabsClientToolRequest, String> {
    let hook: WebhookToolRequest =
        serde_json::from_slice(body).map_err(|e| format!("invalid webhook JSON: {e}"))?;
    if hook.tool.trim().is_empty() {
        return Err("webhook missing case-sensitive tool name".into());
    }
    Ok(ElevenLabsClientToolRequest {
        tool_call_id: hook.tool_call_id,
        tool: hook.tool,
        params: if hook.params.is_null() {
            Value::Object(Default::default())
        } else {
            hook.params
        },
        confirmed: hook.confirmed,
        wait_for_response: true,
        source: VoiceToolCallSource::Webhook,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elevenlabs_style_webhook_body() {
        let body = br#"{"tool_call_id":"wh-1","tool_name":"system_info","parameters":{}}"#;
        let req = parse_webhook_tool_request(body).unwrap();
        assert_eq!(req.tool, "system_info");
        assert_eq!(req.tool_call_id.as_deref(), Some("wh-1"));
        assert_eq!(req.source, VoiceToolCallSource::Webhook);
    }
}
