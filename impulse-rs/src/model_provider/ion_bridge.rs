//! Runs Ion's existing tool loop (`llm_backends::Agent`) on any
//! [`ModelProvider`], by implementing the older `LlmProvider` trait over it.
//!
//! This is how ADR-0022 stage 2 puts Ion on typed endpoints without
//! rewriting the loop: `Agent::chat_with_tools`, the confirmation gate, the
//! untrusted-output envelope, compaction, and the loop contract all stay as
//! they are, and only the provider underneath changes.
//!
//! The endpoint names the model. `ChatRequest::model` (which the step-model
//! hook may set per step) is ignored here, because a profile is bound to one
//! model id; per-step model choice across endpoints is a router concern, not
//! a provider one.

use std::sync::Arc;

use async_trait::async_trait;

use super::{
    ContentBlock, Message, MessageRole, ModelProvider, ModelRequest, ProviderError, StopReason,
    ToolCall, ToolSpec,
};
use crate::error::{AgentError, AgentResult};
use crate::llm_backends::{self, LlmProvider, Role};

/// An `LlmProvider` backed by a [`ModelProvider`].
pub struct ModelProviderLlm {
    provider: Arc<dyn ModelProvider>,
    model: String,
}

impl ModelProviderLlm {
    /// `model` is the label Ion shows; the endpoint decides what is called.
    pub fn new(provider: Arc<dyn ModelProvider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for ModelProviderLlm {
    fn name(&self) -> &str {
        self.provider.name()
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        request: llm_backends::ChatRequest,
    ) -> AgentResult<llm_backends::ChatResponse> {
        let neutral = to_model_request(&request);
        let response = self
            .provider
            .generate(&neutral)
            .await
            .map_err(agent_error)?;
        if let Some(invalid) = response.invalid_tool_calls.first() {
            // Running a tool with arguments the model never produced would be
            // worse than stopping; the loop reports this as a failed turn.
            return Err(AgentError::ApiResponse(format!(
                "model produced a malformed call to {}: {}",
                invalid.name, invalid.error
            )));
        }
        let tool_calls: Vec<llm_backends::ToolCall> = response
            .tool_calls()
            .into_iter()
            .map(|call| llm_backends::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.arguments.clone(),
            })
            .collect();
        Ok(llm_backends::ChatResponse {
            content: response.text(),
            model: if response.model.is_empty() {
                self.model.clone()
            } else {
                response.model.clone()
            },
            usage: llm_backends::Usage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
            },
            // Ion's loop runs tools only on `ToolUse`. Some OpenAI-compatible
            // servers report `stop` alongside tool calls, so the presence of
            // calls decides, except when output was cut off mid-call.
            stop_reason: match response.stop_reason {
                StopReason::MaxTokens => llm_backends::StopReason::MaxTokens,
                _ if !tool_calls.is_empty() => llm_backends::StopReason::ToolUse,
                StopReason::EndTurn => llm_backends::StopReason::EndTurn,
                StopReason::ToolUse | StopReason::Other => llm_backends::StopReason::Other,
            },
            tool_calls,
        })
    }

    fn supported_models(&self) -> Vec<&str> {
        vec![self.model.as_str()]
    }
}

/// Converts Ion's request into the neutral form. System messages join into
/// the system prompt; each message's text, tool calls, and tool results
/// become content blocks in that order.
pub fn to_model_request(request: &llm_backends::ChatRequest) -> ModelRequest {
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        let role = match message.role {
            Role::System => {
                if !message.content.is_empty() {
                    system.push(message.content.clone());
                }
                continue;
            }
            Role::User => MessageRole::User,
            Role::Assistant => MessageRole::Assistant,
        };
        let mut content = Vec::new();
        if !message.content.is_empty() {
            content.push(ContentBlock::text(message.content.clone()));
        }
        content.extend(message.tool_calls.iter().map(|call| {
            ContentBlock::ToolCall(ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.input.clone(),
            })
        }));
        content.extend(
            message
                .tool_results
                .iter()
                .map(|result| ContentBlock::ToolResult {
                    call_id: result.tool_use_id.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                }),
        );
        if !content.is_empty() {
            messages.push(Message { role, content });
        }
    }
    ModelRequest {
        system: (!system.is_empty()).then(|| system.join("\n\n")),
        messages,
        tools: request
            .tools
            .iter()
            .map(|tool| ToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input_schema: tool.input_schema.clone(),
            })
            .collect(),
        // The endpoint governs output size and sampling on this path. Ion's
        // Agent always sends its built-in defaults (4,096 tokens, 0.7), which
        // would override a profile's declared limit and break models that
        // reject a temperature.
        max_output_tokens: None,
        temperature: None,
        provider_params: serde_json::Value::Null,
    }
}

/// Maps a provider failure onto Ion's error type, keeping the cause text.
pub fn agent_error(err: ProviderError) -> AgentError {
    match &err {
        ProviderError::MissingCredential { env } => AgentError::MissingApiKey {
            provider: env.clone(),
        },
        ProviderError::RateLimited { .. } => AgentError::RateLimited,
        ProviderError::Http { status, .. } if *status == 401 || *status == 403 => {
            AgentError::Authentication(err.to_string())
        }
        ProviderError::Unsupported { .. } => AgentError::InvalidRequest(err.to_string()),
        ProviderError::InvalidResponse { .. } => AgentError::ApiResponse(err.to_string()),
        _ => AgentError::ApiRequest(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::WireProtocol;
    use crate::model_provider::{InvalidToolCall, ModelCapabilities, ModelResponse, Usage};
    use serde_json::json;
    use std::sync::Mutex;

    struct Recording {
        seen: Mutex<Option<ModelRequest>>,
        reply: ModelResponse,
    }

    #[async_trait]
    impl ModelProvider for Recording {
        fn name(&self) -> &str {
            "recording"
        }
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities::for_protocol(WireProtocol::OpenaiChat)
        }
        async fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
            *self.seen.lock().unwrap() = Some(request.clone());
            Ok(self.reply.clone())
        }
    }

    fn ion_request() -> llm_backends::ChatRequest {
        llm_backends::ChatRequest {
            model: "ignored-by-endpoints".into(),
            messages: vec![
                llm_backends::Message {
                    role: Role::System,
                    content: "You are Ion.".into(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                },
                llm_backends::Message {
                    role: Role::User,
                    content: "read a.rs".into(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                },
                llm_backends::Message::assistant_tool_use(
                    "",
                    vec![llm_backends::ToolCall {
                        id: "t1".into(),
                        name: "file_read".into(),
                        input: json!({"path": "a.rs"}),
                    }],
                ),
                llm_backends::Message::tool_results(vec![llm_backends::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "fn main() {}".into(),
                    is_error: false,
                }]),
            ],
            temperature: 0.2,
            max_tokens: Some(512),
            tools: vec![llm_backends::ToolDefinition {
                name: "file_read".into(),
                description: "read".into(),
                input_schema: json!({"type": "object"}),
            }],
        }
    }

    #[test]
    fn test_to_model_request_lifts_system_and_keeps_tool_round_trip() {
        let neutral = to_model_request(&ion_request());
        assert_eq!(neutral.system.as_deref(), Some("You are Ion."));
        assert_eq!(neutral.messages.len(), 3);
        assert!(matches!(
            &neutral.messages[1].content[0],
            ContentBlock::ToolCall(call) if call.arguments == json!({"path": "a.rs"})
        ));
        assert!(matches!(
            &neutral.messages[2].content[0],
            ContentBlock::ToolResult { call_id, .. } if call_id == "t1"
        ));
        assert_eq!(neutral.tools[0].name, "file_read");
        assert_eq!(
            neutral.max_output_tokens, None,
            "the endpoint's limit applies"
        );
        assert_eq!(neutral.temperature, None);
    }

    #[tokio::test]
    async fn test_chat_maps_tool_calls_and_usage_back_to_ion() {
        let provider = Arc::new(Recording {
            seen: Mutex::new(None),
            reply: ModelResponse {
                content: vec![
                    ContentBlock::text("Checking."),
                    ContentBlock::ToolCall(ToolCall {
                        id: "t2".into(),
                        name: "bash_exec".into(),
                        arguments: json!({"command": "ls"}),
                    }),
                ],
                invalid_tool_calls: Vec::new(),
                stop_reason: StopReason::ToolUse,
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 3,
                },
                model: "qwen3".into(),
            },
        });
        let llm = ModelProviderLlm::new(provider.clone(), "qwen3");
        let response = llm.chat(ion_request()).await.expect("chat");
        assert_eq!(response.content, "Checking.");
        assert_eq!(response.tool_calls[0].input, json!({"command": "ls"}));
        assert_eq!(response.stop_reason, llm_backends::StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 10);
        assert!(provider.seen.lock().unwrap().is_some());
        assert_eq!(llm.default_model(), "qwen3");
        assert_eq!(llm.supported_models(), ["qwen3"]);
    }

    /// Review P1: tool calls reported with `finish_reason: "stop"` must still
    /// reach Ion's loop as `ToolUse`.
    #[tokio::test]
    async fn test_tool_calls_with_an_end_turn_stop_reason_are_still_tool_use() {
        let provider = Arc::new(Recording {
            seen: Mutex::new(None),
            reply: ModelResponse {
                content: vec![ContentBlock::ToolCall(ToolCall {
                    id: "c".into(),
                    name: "file_read".into(),
                    arguments: json!({"path": "a"}),
                })],
                invalid_tool_calls: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
                model: "m".into(),
            },
        });
        let response = ModelProviderLlm::new(provider, "m")
            .chat(ion_request())
            .await
            .expect("chat");
        assert_eq!(response.stop_reason, llm_backends::StopReason::ToolUse);
    }

    #[tokio::test]
    async fn test_chat_refuses_a_malformed_tool_call() {
        let provider = Arc::new(Recording {
            seen: Mutex::new(None),
            reply: ModelResponse {
                content: Vec::new(),
                invalid_tool_calls: vec![InvalidToolCall {
                    id: "t".into(),
                    name: "file_write".into(),
                    raw_arguments: "{".into(),
                    error: "not JSON".into(),
                }],
                stop_reason: StopReason::ToolUse,
                usage: Usage::default(),
                model: "m".into(),
            },
        });
        let err = ModelProviderLlm::new(provider, "m")
            .chat(ion_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("malformed call to file_write"));
    }

    #[test]
    fn test_agent_error_mapping_keeps_the_cause() {
        assert!(matches!(
            agent_error(ProviderError::MissingCredential {
                env: "OPENAI_API_KEY".into()
            }),
            AgentError::MissingApiKey { .. }
        ));
        assert!(matches!(
            agent_error(ProviderError::Http {
                endpoint: "e".into(),
                status: 401,
                body: "bad key".into()
            }),
            AgentError::Authentication(message) if message.contains("bad key")
        ));
        assert!(matches!(
            agent_error(ProviderError::Exhausted { attempts: vec![] }),
            AgentError::ApiRequest(_)
        ));
    }
}
