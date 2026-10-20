//! OpenAI Chat Completions (`{base_url}/chat/completions`) and the servers
//! that speak it: OpenAI, Ollama, OpenRouter, vLLM, LiteLLM.
//!
//! Differences from Anthropic that this module absorbs: the system prompt is
//! a message; tool results are separate `tool`-role messages; tool calls sit
//! on the assistant message with `arguments` as a JSON *string* (Ollama
//! sometimes sends an object, which is accepted); and a tool result has no
//! error flag, so an error result is prefixed with `Error: `.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use serde_json::{json, Map, Value};

use super::sse::{self, SseEvent};
use super::types::{parse_tool_arguments, InvalidToolCall};
use super::{
    ContentBlock, MessageRole, ModelCapabilities, ModelProvider, ModelRequest, ModelResponse,
    ProviderError, StopReason, StreamEvent, ToolCall, Usage,
};
use crate::model_endpoint::ModelEndpoint;

/// A provider for one OpenAI-compatible Chat Completions endpoint.
pub struct OpenAiChatProvider {
    name: String,
    endpoint: ModelEndpoint,
    capabilities: ModelCapabilities,
    client: reqwest::Client,
}

impl OpenAiChatProvider {
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

    fn send(
        &self,
        request: &ModelRequest,
        stream: bool,
    ) -> Result<reqwest::RequestBuilder, ProviderError> {
        self.capabilities.check(request)?;
        let url = format!(
            "{}/chat/completions",
            self.endpoint.base_url.trim_end_matches('/')
        );
        let builder = self
            .client
            .post(url)
            .json(&request_body(&self.endpoint, request, stream));
        let builder = if stream {
            builder
        } else {
            builder.timeout(sse::REQUEST_TIMEOUT)
        };
        sse::authorize(builder, &self.endpoint.auth)
    }
}

#[async_trait]
impl ModelProvider for OpenAiChatProvider {
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
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        messages.extend(message_json(message.role, &message.content));
    }
    let mut body = Map::new();
    body.insert("model".into(), json!(endpoint.model));
    body.insert("messages".into(), Value::Array(messages));
    if let Some(limit) = request.max_output_tokens.or(endpoint.max_output_tokens) {
        body.insert("max_tokens".into(), json!(limit));
    }
    if let Some(temperature) = request.temperature {
        body.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                            },
                        })
                    })
                    .collect(),
            ),
        );
    }
    if stream {
        body.insert("stream".into(), json!(true));
        body.insert("stream_options".into(), json!({"include_usage": true}));
    }
    sse::merge_provider_params(&mut body, &request.provider_params);
    Value::Object(body)
}

/// One neutral message becomes one or more wire messages: tool results are
/// split out into `tool`-role messages, in order.
fn message_json(role: MessageRole, content: &[ContentBlock]) -> Vec<Value> {
    let mut out = Vec::new();
    match role {
        MessageRole::Assistant => {
            let text: String = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let tool_calls: Vec<Value> = content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolCall(call) => Some(json!({
                        "id": call.id,
                        "type": "function",
                        "function": {"name": call.name, "arguments": call.arguments.to_string()},
                    })),
                    _ => None,
                })
                .collect();
            let mut message = Map::new();
            message.insert("role".into(), json!("assistant"));
            message.insert(
                "content".into(),
                if text.is_empty() && !tool_calls.is_empty() {
                    Value::Null
                } else {
                    json!(text)
                },
            );
            if !tool_calls.is_empty() {
                message.insert("tool_calls".into(), Value::Array(tool_calls));
            }
            out.push(Value::Object(message));
        }
        MessageRole::User => {
            let mut parts = Vec::new();
            let mut has_image = false;
            for block in content {
                match block {
                    ContentBlock::Text { text } => parts.push(json!({"type": "text", "text": text})),
                    ContentBlock::Image {
                        media_type,
                        data_base64,
                    } => {
                        has_image = true;
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{media_type};base64,{data_base64}")},
                        }));
                    }
                    ContentBlock::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } => out.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": if *is_error { format!("Error: {content}") } else { content.clone() },
                    })),
                    // A tool call in a user message has no wire form; it is
                    // dropped rather than sent as text the model might obey.
                    ContentBlock::ToolCall(_) => {}
                }
            }
            if !parts.is_empty() {
                let content = if has_image {
                    Value::Array(parts)
                } else {
                    json!(parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<String>())
                };
                out.push(json!({"role": "user", "content": content}));
            }
        }
    }
    out
}

fn stop_reason(raw: Option<&str>) -> StopReason {
    match raw {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") | Some("function_call") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

/// Tool arguments arrive as a JSON string, or (from some Ollama versions) as
/// an object already.
fn arguments_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn count(usage: Option<&Value>, field: &str) -> u32 {
    usage
        .and_then(|usage| usage.get(field))
        .and_then(Value::as_u64)
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX))
}

/// Parses a complete Chat Completions response.
pub fn parse_response(body: &Value) -> Result<ModelResponse, String> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or("response has no choices")?;
    let message = choice.get("message").ok_or("choice has no message")?;
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(ContentBlock::text(text));
        }
    }
    let mut invalid_tool_calls = Vec::new();
    for (position, call) in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let function = call.get("function");
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("call_{position}"));
        let name = function
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let raw = arguments_text(function.and_then(|f| f.get("arguments")));
        match parse_tool_arguments(&raw) {
            Ok(arguments) => content.push(ContentBlock::ToolCall(ToolCall {
                id,
                name,
                arguments,
            })),
            Err(error) => invalid_tool_calls.push(InvalidToolCall {
                id,
                name,
                raw_arguments: raw,
                error,
            }),
        }
    }
    let usage = body.get("usage");
    Ok(ModelResponse {
        content,
        invalid_tool_calls,
        stop_reason: stop_reason(choice.get("finish_reason").and_then(Value::as_str)),
        usage: Usage {
            input_tokens: count(usage, "prompt_tokens"),
            output_tokens: count(usage, "completion_tokens"),
        },
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Maps one streamed chunk to neutral events. `[DONE]` carries nothing.
pub(crate) fn stream_events(event: &SseEvent) -> Result<Vec<StreamEvent>, String> {
    if event.data.trim() == "[DONE]" {
        return Ok(Vec::new());
    }
    let chunk: Value = serde_json::from_str(&event.data)
        .map_err(|err| format!("stream chunk is not JSON: {err}"))?;
    if let Some(error) = chunk.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("stream error")
            .to_string());
    }
    let mut events = Vec::new();
    if let Some(model) = chunk.get("model").and_then(Value::as_str) {
        events.push(StreamEvent::Model {
            model: model.to_string(),
        });
    }
    if let Some(choice) = chunk
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    {
        let delta = choice.get("delta");
        if let Some(text) = delta.and_then(|d| d.get("content")).and_then(Value::as_str) {
            if !text.is_empty() {
                events.push(StreamEvent::TextDelta {
                    text: text.to_string(),
                });
            }
        }
        for (position, call) in delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let function = call.get("function");
            events.push(StreamEvent::ToolCallDelta {
                index: call
                    .get("index")
                    .and_then(Value::as_u64)
                    .map_or(position, |n| usize::try_from(n).unwrap_or(usize::MAX)),
                id: call.get("id").and_then(Value::as_str).map(str::to_string),
                name: function
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                arguments: arguments_text(function.and_then(|f| f.get("arguments"))),
            });
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            events.push(StreamEvent::Stop {
                reason: stop_reason(Some(reason)),
            });
        }
    }
    if let Some(usage) = chunk.get("usage").filter(|usage| !usage.is_null()) {
        events.push(StreamEvent::Usage {
            usage: Usage {
                input_tokens: count(Some(usage), "prompt_tokens"),
                output_tokens: count(Some(usage), "completion_tokens"),
            },
        });
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::{EndpointAuth, WireProtocol};
    use crate::model_provider::{Message, StreamAccumulator, ToolSpec};

    fn endpoint() -> ModelEndpoint {
        ModelEndpoint {
            protocol: WireProtocol::OpenaiChat,
            base_url: "http://127.0.0.1:11434/v1".into(),
            auth: EndpointAuth::None,
            model: "qwen3".into(),
            max_output_tokens: None,
            capabilities: None,
        }
    }

    #[test]
    fn test_request_body_splits_tool_results_and_stringifies_arguments() {
        let request = ModelRequest {
            system: Some("sys".into()),
            messages: vec![
                Message::user("hi"),
                Message {
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::ToolCall(ToolCall {
                        id: "call_1".into(),
                        name: "file_read".into(),
                        arguments: json!({"path": "a"}),
                    })],
                },
                Message {
                    role: MessageRole::User,
                    content: vec![
                        ContentBlock::ToolResult {
                            call_id: "call_1".into(),
                            content: "nope".into(),
                            is_error: true,
                        },
                        ContentBlock::text("and then?"),
                    ],
                },
            ],
            tools: vec![ToolSpec {
                name: "file_read".into(),
                description: "read".into(),
                input_schema: json!({"type": "object"}),
            }],
            max_output_tokens: Some(256),
            temperature: None,
            provider_params: Value::Null,
        };
        let body = request_body(&endpoint(), &request, true);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1], json!({"role": "user", "content": "hi"}));
        assert_eq!(messages[2]["content"], Value::Null);
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a\"}"
        );
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["content"], "Error: nope");
        assert_eq!(messages[4]["content"], "and then?");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn test_request_body_sends_images_as_data_url_parts() {
        let mut request = ModelRequest::from_user("what is this");
        request.messages[0].content.push(ContentBlock::Image {
            media_type: "image/png".into(),
            data_base64: "AA".into(),
        });
        let body = request_body(&endpoint(), &request, false);
        let content = &body["messages"][0]["content"];
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,AA");
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn test_parse_response_accepts_string_and_object_arguments() {
        let body = json!({
            "model": "qwen3",
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "content": "",
                    "tool_calls": [
                        {"id": "c1", "function": {"name": "a", "arguments": "{\"x\":1}"}},
                        {"function": {"name": "b", "arguments": {"y": 2}}},
                        {"id": "c3", "function": {"name": "c", "arguments": "{bad"}}
                    ]
                }
            }],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4}
        });
        let response = parse_response(&body).unwrap();
        let calls = response.tool_calls();
        assert_eq!(calls[0].arguments, json!({"x": 1}));
        assert_eq!(calls[1].id, "call_1", "a missing id is synthesized");
        assert_eq!(calls[1].arguments, json!({"y": 2}));
        assert_eq!(response.invalid_tool_calls[0].id, "c3");
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.output_tokens, 4);
        assert!(parse_response(&json!({"choices": []})).is_err());
    }

    #[test]
    fn test_stream_chunks_assemble_into_the_same_reply() {
        let raw = [
            r#"{"model":"qwen3","choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#,
            r#"{"model":"qwen3","choices":[{"delta":{"content":"lo"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"search_tools","arguments":"{\"qu"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ery\":\"x\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":6}}"#,
            "[DONE]",
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
        assert_eq!(response.text(), "Hello");
        assert_eq!(response.tool_calls()[0].arguments, json!({"query": "x"}));
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.usage.input_tokens, 5);
        assert_eq!(response.model, "qwen3");
    }

    #[test]
    fn test_stream_error_chunk_is_an_error() {
        let err = stream_events(&SseEvent {
            event: None,
            data: r#"{"error":{"message":"model not found"}}"#.into(),
        })
        .unwrap_err();
        assert_eq!(err, "model not found");
    }
}
