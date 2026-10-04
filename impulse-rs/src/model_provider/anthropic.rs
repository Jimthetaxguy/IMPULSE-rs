//! Anthropic Messages (`{base_url}/messages`).

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use serde_json::{json, Map, Value};

use super::sse::{self, SseEvent};
use super::types::InvalidToolCall;
use super::{
    ContentBlock, MessageRole, ModelCapabilities, ModelProvider, ModelRequest, ModelResponse,
    ProviderError, StopReason, StreamEvent, ToolCall, Usage,
};
use crate::model_endpoint::ModelEndpoint;

/// API version header Anthropic requires.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A provider for one Anthropic Messages endpoint.
pub struct AnthropicProvider {
    name: String,
    endpoint: ModelEndpoint,
    capabilities: ModelCapabilities,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(
        name: impl Into<String>,
        endpoint: ModelEndpoint,
        capabilities: ModelCapabilities,
    ) -> Result<Self, ProviderError> {
        let name = name.into();
        let client = sse::client_builder()
            .build()
            .map_err(|err| sse::transport_error(&name, err))?;
        Ok(Self {
            name,
            endpoint,
            capabilities,
            client,
        })
    }

    fn url(&self) -> String {
        format!("{}/messages", self.endpoint.base_url.trim_end_matches('/'))
    }

    fn send(
        &self,
        request: &ModelRequest,
        stream: bool,
    ) -> Result<reqwest::RequestBuilder, ProviderError> {
        self.capabilities.check(request)?;
        let body = request_body(&self.endpoint, request, stream);
        let builder = self
            .client
            .post(self.url())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body);
        let builder = if stream {
            builder
        } else {
            builder.timeout(sse::REQUEST_TIMEOUT)
        };
        sse::authorize(builder, &self.endpoint.auth)
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> ModelCapabilities {
        self.capabilities
    }

    async fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        let response = self
            .send(request, false)?
            .send()
            .await
            .map_err(|err| sse::transport_error(&self.name, err))?;
        let response = sse::error_for_status(&self.name, response).await?;
        let body = sse::json_body(&self.name, response).await?;
        parse_response(&body).map_err(|message| ProviderError::InvalidResponse {
            endpoint: self.name.clone(),
            message,
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
        let name = self.name.clone();
        let opened = async move {
            let response = self
                .send(request, true)?
                .send()
                .await
                .map_err(|err| sse::transport_error(&name, err))?;
            let response = sse::error_for_status(&name, response).await?;
            Ok::<_, ProviderError>(sse::sse_events(name, response))
        };
        let endpoint = self.name.clone();
        futures::stream::once(opened)
            .flat_map(move |opened| match opened {
                Err(err) => futures::stream::iter(vec![Err(err)]).boxed(),
                Ok(events) => {
                    let endpoint = endpoint.clone();
                    events
                        .flat_map(move |event| {
                            let mapped = match event {
                                Ok(event) => stream_events(&event).map_err(|message| {
                                    ProviderError::InvalidResponse {
                                        endpoint: endpoint.clone(),
                                        message,
                                    }
                                }),
                                Err(err) => Err(err),
                            };
                            futures::stream::iter(match mapped {
                                Ok(events) => events.into_iter().map(Ok).collect::<Vec<_>>(),
                                Err(err) => vec![Err(err)],
                            })
                        })
                        .boxed()
                }
            })
            .boxed()
    }
}

/// The JSON body for `request`.
pub fn request_body(endpoint: &ModelEndpoint, request: &ModelRequest, stream: bool) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(endpoint.model));
    body.insert(
        "max_tokens".into(),
        json!(request
            .max_output_tokens
            .or(endpoint.max_output_tokens)
            .unwrap_or(4096)),
    );
    if let Some(system) = &request.system {
        body.insert("system".into(), json!(system));
    }
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| {
            json!({
                "role": match message.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                },
                "content": message.content.iter().map(block_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    body.insert("messages".into(), Value::Array(messages));
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema,
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if stream {
        body.insert("stream".into(), json!(true));
    }
    sse::merge_provider_params(&mut body, &request.provider_params);
    Value::Object(body)
}

fn block_json(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Image {
            media_type,
            data_base64,
        } => json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": data_base64},
        }),
        ContentBlock::ToolCall(call) => json!({
            "type": "tool_use", "id": call.id, "name": call.name, "input": call.arguments,
        }),
        ContentBlock::ToolResult {
            call_id,
            content,
            is_error,
        } => json!({
            "type": "tool_result", "tool_use_id": call_id, "content": content, "is_error": is_error,
        }),
    }
}

fn stop_reason(raw: Option<&str>) -> StopReason {
    match raw {
        Some("end_turn") | Some("stop_sequence") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

fn tool_call(id: &str, name: &str, input: &Value) -> Result<ToolCall, InvalidToolCall> {
    match input {
        Value::Object(_) => Ok(ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: input.clone(),
        }),
        other => Err(InvalidToolCall {
            id: id.to_string(),
            name: name.to_string(),
            raw_arguments: other.to_string(),
            error: "tool input must be a JSON object".to_string(),
        }),
    }
}

/// Parses a complete Messages response.
pub fn parse_response(body: &Value) -> Result<ModelResponse, String> {
    let blocks = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or("response has no content array")?;
    let mut content = Vec::new();
    let mut invalid_tool_calls = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => content.push(ContentBlock::text(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )),
            Some("tool_use") => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                match tool_call(id, name, &input) {
                    Ok(call) => content.push(ContentBlock::ToolCall(call)),
                    Err(invalid) => invalid_tool_calls.push(invalid),
                }
            }
            // Thinking and other block types are not part of the neutral reply.
            _ => {}
        }
    }
    let usage = body.get("usage");
    let count = |field: &str| {
        usage
            .and_then(|usage| usage.get(field))
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
    };
    Ok(ModelResponse {
        content,
        invalid_tool_calls,
        stop_reason: stop_reason(body.get("stop_reason").and_then(Value::as_str)),
        usage: Usage {
            input_tokens: count("input_tokens"),
            output_tokens: count("output_tokens"),
        },
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Maps one Messages stream event to neutral events.
pub(crate) fn stream_events(event: &SseEvent) -> Result<Vec<StreamEvent>, String> {
    let data: Value = serde_json::from_str(&event.data)
        .map_err(|err| format!("stream event is not JSON: {err}"))?;
    let kind = data
        .get("type")
        .and_then(Value::as_str)
        .or(event.event.as_deref())
        .unwrap_or_default();
    let count = |value: Option<&Value>, field: &str| {
        value
            .and_then(|usage| usage.get(field))
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
    };
    let index = || {
        data.get("index")
            .and_then(Value::as_u64)
            .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
    };
    Ok(match kind {
        "message_start" => {
            let message = data.get("message");
            let usage = message.and_then(|m| m.get("usage"));
            vec![
                StreamEvent::Model {
                    model: message
                        .and_then(|m| m.get("model"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
                StreamEvent::Usage {
                    usage: Usage {
                        input_tokens: count(usage, "input_tokens"),
                        output_tokens: count(usage, "output_tokens"),
                    },
                },
            ]
        }
        "content_block_start" => {
            let block = data.get("content_block");
            match block.and_then(|b| b.get("type")).and_then(Value::as_str) {
                Some("tool_use") => vec![StreamEvent::ToolCallDelta {
                    index: index(),
                    id: block
                        .and_then(|b| b.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: block
                        .and_then(|b| b.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments: String::new(),
                }],
                Some("text") => {
                    let text = block
                        .and_then(|b| b.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![StreamEvent::TextDelta {
                            text: text.to_string(),
                        }]
                    }
                }
                _ => Vec::new(),
            }
        }
        "content_block_delta" => {
            let delta = data.get("delta");
            match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                Some("text_delta") => vec![StreamEvent::TextDelta {
                    text: delta
                        .and_then(|d| d.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }],
                Some("input_json_delta") => vec![StreamEvent::ToolCallDelta {
                    index: index(),
                    id: None,
                    name: None,
                    arguments: delta
                        .and_then(|d| d.get("partial_json"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }],
                _ => Vec::new(),
            }
        }
        "message_delta" => {
            let mut events = vec![StreamEvent::Usage {
                usage: Usage {
                    input_tokens: count(data.get("usage"), "input_tokens"),
                    output_tokens: count(data.get("usage"), "output_tokens"),
                },
            }];
            if let Some(reason) = data
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(Value::as_str)
            {
                events.push(StreamEvent::Stop {
                    reason: stop_reason(Some(reason)),
                });
            }
            events
        }
        "error" => {
            return Err(data
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("stream error")
                .to_string())
        }
        // ping, content_block_stop, message_stop, and unknown events carry
        // nothing the neutral stream needs.
        _ => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::{EndpointAuth, WireProtocol};
    use crate::model_provider::{Message, StreamAccumulator, ToolSpec};

    fn endpoint() -> ModelEndpoint {
        ModelEndpoint {
            protocol: WireProtocol::AnthropicMessages,
            base_url: "https://api.anthropic.com/v1".into(),
            auth: EndpointAuth::HeaderEnv {
                header: "x-api-key".into(),
                env: "ANTHROPIC_API_KEY".into(),
            },
            model: "claude-sonnet".into(),
            max_output_tokens: Some(1024),
            capabilities: None,
        }
    }

    #[test]
    fn test_request_body_maps_tools_results_images_and_params() {
        let request = ModelRequest {
            system: Some("be brief".into()),
            messages: vec![
                Message::user("read it"),
                Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCall {
                        id: "toolu_1".into(),
                        name: "file_read".into(),
                        arguments: json!({"path": "a"}),
                    })],
                },
                Message {
                    role: MessageRole::User,
                    content: vec![
                        ContentBlock::ToolResult {
                            call_id: "toolu_1".into(),
                            content: "text".into(),
                            is_error: false,
                        },
                        ContentBlock::Image {
                            media_type: "image/png".into(),
                            data_base64: "AA".into(),
                        },
                    ],
                },
            ],
            tools: vec![ToolSpec {
                name: "file_read".into(),
                description: "read".into(),
                input_schema: json!({"type": "object"}),
            }],
            max_output_tokens: None,
            temperature: Some(0.0),
            provider_params: json!({"metadata": {"user_id": "u"}}),
        };
        let body = request_body(&endpoint(), &request, true);
        assert_eq!(body["model"], "claude-sonnet");
        assert_eq!(body["max_tokens"], 1024);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][0]["input"]["path"], "a");
        assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(
            body["messages"][2]["content"][1]["source"]["type"],
            "base64"
        );
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["metadata"]["user_id"], "u");
    }

    #[test]
    fn test_parse_response_normalizes_tool_use_and_rejects_non_object_input() {
        let body = json!({
            "model": "claude-sonnet-x",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 12, "output_tokens": 5},
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "Reading."},
                {"type": "tool_use", "id": "toolu_1", "name": "file_read", "input": {"path": "a"}},
                {"type": "tool_use", "id": "toolu_2", "name": "bad", "input": "oops"}
            ]
        });
        let response = parse_response(&body).unwrap();
        assert_eq!(response.text(), "Reading.");
        assert_eq!(response.tool_calls()[0].arguments, json!({"path": "a"}));
        assert_eq!(response.invalid_tool_calls[0].name, "bad");
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 12);
        assert_eq!(response.model, "claude-sonnet-x");
        assert!(parse_response(&json!({})).is_err());
    }

    #[test]
    fn test_stream_events_assemble_into_the_same_reply() {
        let raw = [
            r#"{"type":"message_start","message":{"model":"m","usage":{"input_tokens":9,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"search_tools","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"x\"}"}}"#,
            r#"{"type":"ping"}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":20}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut acc = StreamAccumulator::new();
        for data in raw {
            for event in stream_events(&SseEvent {
                event: None,
                data: data.into(),
            })
            .unwrap()
            {
                acc.push(event);
            }
        }
        let response = acc.finish();
        assert_eq!(response.text(), "Hi");
        assert_eq!(response.tool_calls()[0].arguments, json!({"query": "x"}));
        assert_eq!(response.usage.input_tokens, 9);
        assert_eq!(response.usage.output_tokens, 20);
        assert_eq!(response.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn test_stream_error_event_is_an_error() {
        let err = stream_events(&SseEvent {
            event: Some("error".into()),
            data: r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
                .into(),
        })
        .unwrap_err();
        assert_eq!(err, "Overloaded");
        assert!(stream_events(&SseEvent {
            event: None,
            data: "not json".into()
        })
        .is_err());
    }
}
