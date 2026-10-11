//! Provider-neutral request, response, and stream types.
//!
//! Anthropic sends tool calls as `tool_use` content blocks with an object
//! `input`; OpenAI-compatible servers send `tool_calls` on the assistant
//! message with a JSON-*string* `arguments`. Both normalize to
//! [`ToolCall`] with parsed `arguments`, and a call whose arguments are not
//! valid JSON becomes an [`InvalidToolCall`] instead of an error, because
//! models do emit malformed JSON and the caller should decide what to do.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who authored a message. A system prompt is carried on
/// [`ModelRequest::system`], not as a message, because the two wire formats
/// place it differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// One piece of message content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    /// A base64-encoded image. Requires a provider with `vision`.
    Image {
        media_type: String,
        data_base64: String,
    },
    /// The model asking the caller to run a tool (assistant messages only).
    ToolCall(ToolCall),
    /// The caller reporting a tool's outcome (user messages only).
    ToolResult {
        call_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

/// One message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentBlock::text(text)],
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::text(text)],
        }
    }
}

/// A tool the model may call. `input_schema` is a JSON Schema object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// A well-formed tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// A tool call whose arguments did not parse as a JSON object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidToolCall {
    pub id: String,
    pub name: String,
    pub raw_arguments: String,
    pub error: String,
}

/// What to ask a model for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    /// Per-request output ceiling; the endpoint's `max_output_tokens` applies
    /// when this is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Provider-specific fields merged into the wire body last, for features
    /// this crate does not model. An escape hatch, not a routing input.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub provider_params: Value,
}

impl ModelRequest {
    /// A request with one user message and nothing else set.
    pub fn from_user(text: impl Into<String>) -> Self {
        Self {
            system: None,
            messages: vec![Message::user(text)],
            tools: Vec::new(),
            max_output_tokens: None,
            temperature: None,
            provider_params: Value::Null,
        }
    }

    /// Whether any message carries an image.
    pub fn has_images(&self) -> bool {
        self.messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::Image { .. }))
        })
    }
}

/// Why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

/// Token counts for one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A complete model reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    /// Text and tool-call blocks in the order the model produced them.
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalid_tool_calls: Vec<InvalidToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
    /// The model id the server reports, which may differ from the one asked
    /// for (an alias resolved server-side).
    pub model: String,
}

impl ModelResponse {
    /// All text blocks joined.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The well-formed tool calls, in order.
    pub fn tool_calls(&self) -> Vec<&ToolCall> {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect()
    }
}

/// One incremental piece of a streamed reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Text appended to the reply.
    TextDelta { text: String },
    /// Part of tool call number `index`. `id` and `name` arrive on the first
    /// fragment for that index; `arguments` is a raw JSON fragment.
    ToolCallDelta {
        index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default)]
        arguments: String,
    },
    /// Token counts, possibly partial; later values replace earlier ones
    /// field by field when nonzero.
    Usage { usage: Usage },
    /// The model id the server reports.
    Model { model: String },
    /// End of the reply.
    Stop { reason: StopReason },
}

/// Builds a [`ModelResponse`] from [`StreamEvent`]s. One shared
/// implementation, so no provider writes its own merging.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    blocks: Vec<PendingBlock>,
    usage: Usage,
    model: String,
    stop_reason: Option<StopReason>,
}

#[derive(Debug)]
enum PendingBlock {
    Text(String),
    Tool {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    },
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds one event in.
    pub fn push(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::TextDelta { text } => match self.blocks.last_mut() {
                Some(PendingBlock::Text(existing)) => existing.push_str(&text),
                _ => self.blocks.push(PendingBlock::Text(text)),
            },
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => {
                // The latest call at this index. Servers that omit `index`
                // report every call as 0, so a fragment carrying a different
                // id starts a new call rather than merging into the last one.
                let existing = self
                    .blocks
                    .iter_mut()
                    .rev()
                    .find_map(|block| match block {
                        PendingBlock::Tool {
                            index: i,
                            id,
                            name,
                            arguments,
                        } if *i == index => Some((id, name, arguments)),
                        _ => None,
                    })
                    .filter(|(existing_id, _, _)| match &id {
                        Some(new_id) => existing_id.is_empty() || *existing_id == new_id,
                        None => true,
                    });
                match existing {
                    Some((existing_id, existing_name, existing_args)) => {
                        if let Some(id) = id {
                            *existing_id = id;
                        }
                        if let Some(name) = name {
                            // Some servers repeat the whole name on every
                            // fragment; others split it. A fragment that
                            // restates what is already there replaces it.
                            if name.starts_with(existing_name.as_str()) {
                                *existing_name = name;
                            } else {
                                existing_name.push_str(&name);
                            }
                        }
                        existing_args.push_str(&arguments);
                    }
                    None => self.blocks.push(PendingBlock::Tool {
                        index,
                        id: id.unwrap_or_default(),
                        name: name.unwrap_or_default(),
                        arguments,
                    }),
                }
            }
            StreamEvent::Usage { usage } => {
                if usage.input_tokens > 0 {
                    self.usage.input_tokens = usage.input_tokens;
                }
                if usage.output_tokens > 0 {
                    self.usage.output_tokens = usage.output_tokens;
                }
            }
            StreamEvent::Model { model } => self.model = model,
            StreamEvent::Stop { reason } => self.stop_reason = Some(reason),
        }
    }

    /// Whether a `Stop` event has arrived.
    pub fn is_finished(&self) -> bool {
        self.stop_reason.is_some()
    }

    /// The assembled reply. A missing stop event reads as `Other`, so a
    /// truncated stream is visible rather than mistaken for a clean end.
    pub fn finish(self) -> ModelResponse {
        let mut content = Vec::new();
        let mut invalid_tool_calls = Vec::new();
        for block in self.blocks {
            match block {
                PendingBlock::Text(text) if text.is_empty() => {}
                PendingBlock::Text(text) => content.push(ContentBlock::Text { text }),
                PendingBlock::Tool {
                    id,
                    name,
                    arguments,
                    ..
                } => match parse_tool_arguments(&arguments) {
                    Ok(arguments) => content.push(ContentBlock::ToolCall(ToolCall {
                        id,
                        name,
                        arguments,
                    })),
                    Err(error) => invalid_tool_calls.push(InvalidToolCall {
                        id,
                        name,
                        raw_arguments: arguments,
                        error,
                    }),
                },
            }
        }
        ModelResponse {
            content,
            invalid_tool_calls,
            stop_reason: self.stop_reason.unwrap_or(StopReason::Other),
            usage: self.usage,
            model: self.model,
        }
    }
}

/// Parses tool arguments. Empty means no arguments (`{}`); anything else
/// must be a JSON object.
pub fn parse_tool_arguments(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(other) => Err(format!(
            "tool arguments must be a JSON object, got {}",
            json_kind(&other)
        )),
        Err(err) => Err(format!("tool arguments are not valid JSON: {err}")),
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Replays a complete response as stream events, for providers or tests
/// that only produce whole replies.
pub fn events_from_response(response: &ModelResponse) -> Vec<StreamEvent> {
    let mut events = vec![StreamEvent::Model {
        model: response.model.clone(),
    }];
    let mut tool_index = 0;
    for block in &response.content {
        match block {
            ContentBlock::Text { text } => {
                events.push(StreamEvent::TextDelta { text: text.clone() })
            }
            ContentBlock::ToolCall(call) => {
                events.push(StreamEvent::ToolCallDelta {
                    index: tool_index,
                    id: Some(call.id.clone()),
                    name: Some(call.name.clone()),
                    arguments: call.arguments.to_string(),
                });
                tool_index += 1;
            }
            ContentBlock::Image { .. } | ContentBlock::ToolResult { .. } => {}
        }
    }
    for invalid in &response.invalid_tool_calls {
        events.push(StreamEvent::ToolCallDelta {
            index: tool_index,
            id: Some(invalid.id.clone()),
            name: Some(invalid.name.clone()),
            arguments: invalid.raw_arguments.clone(),
        });
        tool_index += 1;
    }
    events.push(StreamEvent::Usage {
        usage: response.usage,
    });
    events.push(StreamEvent::Stop {
        reason: response.stop_reason,
    });
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_accumulator_merges_text_and_indexed_tool_fragments() {
        let mut acc = StreamAccumulator::new();
        acc.push(StreamEvent::Model { model: "m".into() });
        acc.push(StreamEvent::TextDelta { text: "Hel".into() });
        acc.push(StreamEvent::TextDelta { text: "lo".into() });
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            name: Some("file_read".into()),
            arguments: "{\"pa".into(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 1,
            id: Some("call_2".into()),
            name: Some("bash_exec".into()),
            arguments: "{}".into(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            arguments: "th\":\"a.rs\"}".into(),
        });
        acc.push(StreamEvent::Usage {
            usage: Usage {
                input_tokens: 10,
                output_tokens: 0,
            },
        });
        acc.push(StreamEvent::Usage {
            usage: Usage {
                input_tokens: 0,
                output_tokens: 7,
            },
        });
        acc.push(StreamEvent::Stop {
            reason: StopReason::ToolUse,
        });
        assert!(acc.is_finished());
        let response = acc.finish();
        assert_eq!(response.text(), "Hello");
        let calls = response.tool_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments, json!({"path": "a.rs"}));
        assert_eq!(calls[1].name, "bash_exec");
        assert_eq!(
            response.usage,
            Usage {
                input_tokens: 10,
                output_tokens: 7
            }
        );
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.model, "m");
    }

    /// Review P2: servers that omit `index` send one complete call per chunk,
    /// all at index 0; distinct ids must stay distinct calls, and a repeated
    /// name must not be doubled.
    #[test]
    fn test_accumulator_separates_unindexed_calls_and_does_not_double_names() {
        let mut acc = StreamAccumulator::new();
        for (id, name, args) in [("c1", "f", "{\"x\":1}"), ("c2", "g", "{\"y\":2}")] {
            acc.push(StreamEvent::ToolCallDelta {
                index: 0,
                id: Some(id.into()),
                name: Some(name.into()),
                arguments: args.into(),
            });
        }
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("c3".into()),
            name: Some("get_weather".into()),
            arguments: String::new(),
        });
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: Some("get_weather".into()),
            arguments: "{}".into(),
        });
        let response = acc.finish();
        let calls = response.tool_calls();
        assert!(response.invalid_tool_calls.is_empty(), "{response:?}");
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].arguments, json!({"x": 1}));
        assert_eq!(calls[1].name, "g");
        assert_eq!(calls[2].name, "get_weather");
    }

    #[test]
    fn test_accumulator_joins_a_name_split_across_fragments() {
        let mut acc = StreamAccumulator::new();
        for part in ["get_", "weather"] {
            acc.push(StreamEvent::ToolCallDelta {
                index: 0,
                id: None,
                name: Some(part.into()),
                arguments: String::new(),
            });
        }
        assert_eq!(acc.finish().tool_calls()[0].name, "get_weather");
    }

    #[test]
    fn test_accumulator_captures_malformed_arguments_as_invalid_tool_call() {
        let mut acc = StreamAccumulator::new();
        acc.push(StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("c".into()),
            name: Some("t".into()),
            arguments: "{not json".into(),
        });
        let response = acc.finish();
        assert!(response.tool_calls().is_empty());
        assert_eq!(response.invalid_tool_calls.len(), 1);
        assert_eq!(response.invalid_tool_calls[0].raw_arguments, "{not json");
        assert_eq!(response.stop_reason, StopReason::Other, "no stop event");
    }

    #[test]
    fn test_parse_tool_arguments_accepts_empty_and_objects_only() {
        assert_eq!(parse_tool_arguments("  ").unwrap(), json!({}));
        assert_eq!(parse_tool_arguments("{\"a\":1}").unwrap(), json!({"a": 1}));
        assert!(parse_tool_arguments("[1]")
            .unwrap_err()
            .contains("an array"));
        assert!(parse_tool_arguments("{")
            .unwrap_err()
            .contains("not valid JSON"));
    }

    #[test]
    fn test_events_from_response_round_trips_through_the_accumulator() {
        let original = ModelResponse {
            content: vec![
                ContentBlock::text("thinking"),
                ContentBlock::ToolCall(ToolCall {
                    id: "c1".into(),
                    name: "search_tools".into(),
                    arguments: json!({"query": "x"}),
                }),
            ],
            invalid_tool_calls: Vec::new(),
            stop_reason: StopReason::ToolUse,
            usage: Usage {
                input_tokens: 3,
                output_tokens: 4,
            },
            model: "m".into(),
        };
        let mut acc = StreamAccumulator::new();
        for event in events_from_response(&original) {
            acc.push(event);
        }
        assert_eq!(acc.finish(), original);
    }

    #[test]
    fn test_request_round_trips_and_detects_images() {
        let mut request = ModelRequest::from_user("hi");
        assert!(!request.has_images());
        request.messages.push(Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Image {
                media_type: "image/png".into(),
                data_base64: "AAAA".into(),
            }],
        });
        assert!(request.has_images());
        let json = serde_json::to_string(&request).unwrap();
        let recovered: ModelRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, request);
    }

    #[test]
    fn test_response_and_stream_event_round_trip() {
        let response = ModelResponse {
            content: vec![ContentBlock::text("x")],
            invalid_tool_calls: vec![InvalidToolCall {
                id: "i".into(),
                name: "n".into(),
                raw_arguments: "{".into(),
                error: "e".into(),
            }],
            stop_reason: StopReason::MaxTokens,
            usage: Usage::default(),
            model: "m".into(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(
            serde_json::from_str::<ModelResponse>(&json).unwrap(),
            response
        );
        for event in events_from_response(&response) {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<StreamEvent>(&json).unwrap(), event);
        }
    }
}
