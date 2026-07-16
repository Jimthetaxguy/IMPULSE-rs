//! Bridge: ElevenLabs tool-call → Impulse `ToolRegistry::execute`.

use std::sync::Arc;

use crate::tooling::{ExecutionOrigin, ToolContext, ToolRegistry};

use super::envelope::{ElevenLabsClientToolRequest, ElevenLabsToolResult};
use super::policy::{VoicePolicy, VoicePolicyDecision};
use super::provider::{default_voice_provider, VoiceProvider};

/// Shared bridge used by CLI, webhook handler, and future session loops.
pub struct VoiceToolBridge {
    registry: Arc<ToolRegistry>,
    context: ToolContext,
    policy: VoicePolicy,
    provider: VoiceProvider,
}

impl VoiceToolBridge {
    /// Build a bridge against the real default registry + voice-safe context.
    pub fn with_defaults() -> Self {
        let mut context = ToolContext::with_all_capabilities();
        context.execution_origin = ExecutionOrigin::Voice;
        Self {
            registry: Arc::new(ToolRegistry::with_defaults()),
            context,
            policy: VoicePolicy::default(),
            provider: default_voice_provider(),
        }
    }

    pub fn new(registry: Arc<ToolRegistry>, context: ToolContext, policy: VoicePolicy) -> Self {
        Self {
            registry,
            context,
            policy,
            provider: VoiceProvider::ElevenLabsAgent,
        }
    }

    pub fn provider(&self) -> VoiceProvider {
        self.provider
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn registry_arc(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.registry)
    }

    pub fn context(&self) -> &ToolContext {
        &self.context
    }

    /// Execute an ElevenLabs-shaped client tool call against Impulse tools.
    pub async fn handle_client_tool(
        &self,
        request: ElevenLabsClientToolRequest,
    ) -> ElevenLabsToolResult {
        // Refuse non-primary providers for this bridge path.
        if !self.provider.is_primary() {
            return ElevenLabsToolResult::error(
                request.tool,
                request.tool_call_id,
                format!(
                    "voice provider `{}` is not the primary ElevenLabs Agent path",
                    self.provider.as_str()
                ),
            );
        }

        let tool_name = request.tool.clone();
        if tool_name.trim().is_empty() {
            return ElevenLabsToolResult::error(
                tool_name,
                request.tool_call_id,
                "missing case-sensitive tool name",
            );
        }

        match self
            .policy
            .evaluate(&self.registry, &tool_name, request.confirmed)
        {
            VoicePolicyDecision::Deny { reason } => {
                return ElevenLabsToolResult::denied(tool_name, request.tool_call_id, reason);
            }
            VoicePolicyDecision::Allow => {}
        }

        let params = if request.params.is_null() {
            serde_json::json!({})
        } else {
            request.params
        };

        match self
            .registry
            .execute(&tool_name, params, &self.context)
            .await
        {
            Ok(tool_result) => {
                let mut result = ElevenLabsToolResult::ok(
                    tool_name,
                    request.tool_call_id,
                    serde_json::json!({
                        "output": tool_result.output,
                        "metadata": tool_result.metadata,
                        "artifacts": tool_result
                            .artifacts
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>(),
                    }),
                );
                result.wait_for_response = request.wait_for_response;
                result.provider = self.provider.as_str().to_string();
                result
            }
            Err(err) => {
                let mut result = ElevenLabsToolResult::error(
                    tool_name,
                    request.tool_call_id,
                    err.to_string(),
                );
                result.wait_for_response = request.wait_for_response;
                result.provider = self.provider.as_str().to_string();
                result
            }
        }
    }
}

/// Convenience entry: default bridge + request.
pub async fn invoke_elevenlabs_client_tool(
    request: ElevenLabsClientToolRequest,
) -> ElevenLabsToolResult {
    VoiceToolBridge::with_defaults()
        .handle_client_tool(request)
        .await
}

/// Entry with an injected bridge (tests / daemon wiring).
pub async fn invoke_elevenlabs_client_tool_with(
    bridge: &VoiceToolBridge,
    request: ElevenLabsClientToolRequest,
) -> ElevenLabsToolResult {
    bridge.handle_client_tool(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::envelope::ElevenLabsToolResultStatus;
    use serde_json::json;

    #[tokio::test]
    async fn system_info_readonly_returns_nonempty_shaped_result() {
        let bridge = VoiceToolBridge::with_defaults();
        let req = ElevenLabsClientToolRequest {
            tool_call_id: Some("call-sys-1".into()),
            tool: "system_info".into(),
            params: json!({ "include_env": false }),
            confirmed: false,
            wait_for_response: true,
            source: super::super::envelope::VoiceToolCallSource::ClientTool,
        };
        let out = bridge.handle_client_tool(req).await;
        assert_eq!(out.status, ElevenLabsToolResultStatus::Ok);
        assert_eq!(out.tool, "system_info");
        assert_eq!(out.tool_call_id.as_deref(), Some("call-sys-1"));
        assert!(out.wait_for_response);
        assert_eq!(out.provider, "elevenlabs_agent");
        assert!(out.error.is_none());
        // Real tool output — system_info always includes os/arch.
        let output = out.result.get("output").expect("output wrapper");
        assert!(output.get("os").is_some(), "expected real system_info os field");
        assert!(output.get("arch").is_some());
    }

    #[tokio::test]
    async fn bash_exec_mutate_blocked_without_confirmation() {
        let bridge = VoiceToolBridge::with_defaults();
        let req = ElevenLabsClientToolRequest {
            tool_call_id: Some("call-bash-1".into()),
            tool: "bash_exec".into(),
            params: json!({ "command": "echo should-not-run-$$" }),
            confirmed: false,
            wait_for_response: true,
            source: super::super::envelope::VoiceToolCallSource::ClientTool,
        };
        let out = bridge.handle_client_tool(req).await;
        assert_eq!(out.status, ElevenLabsToolResultStatus::Denied);
        assert!(
            out.error
                .as_deref()
                .unwrap_or("")
                .contains("requires confirmation"),
            "got {:?}",
            out.error
        );
        // No successful tool output body from bash_exec.
        assert_ne!(out.status, ElevenLabsToolResultStatus::Ok);
    }

    #[tokio::test]
    async fn unknown_tool_returns_error_envelope_not_panic() {
        let bridge = VoiceToolBridge::with_defaults();
        // Not on allowlist → denied; use allowlist-open policy for not-found path.
        let mut policy = VoicePolicy::default();
        policy.exposed_tools = None;
        let bridge = VoiceToolBridge::new(
            bridge.registry_arc(),
            bridge.context().clone(),
            policy,
        );
        let out = bridge
            .handle_client_tool(ElevenLabsClientToolRequest::new(
                "definitely_not_a_tool",
                json!({}),
            ))
            .await;
        assert_eq!(out.status, ElevenLabsToolResultStatus::Error);
        assert!(out.error.is_some());
    }

    #[tokio::test]
    async fn fixture_client_tool_json_maps_to_real_system_info() {
        let raw = r#"{
            "tool_call_id": "el-fixture-1",
            "tool_name": "system_info",
            "parameters": { "include_env": false },
            "wait_for_response": true
        }"#;
        let req: ElevenLabsClientToolRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.tool, "system_info");
        let out = invoke_elevenlabs_client_tool(req).await;
        assert_eq!(out.status, ElevenLabsToolResultStatus::Ok);
        let output = &out.result["output"];
        assert!(
            output.get("os").is_some() && output.get("arch").is_some(),
            "unexpected system_info output: {output}"
        );
    }
}
