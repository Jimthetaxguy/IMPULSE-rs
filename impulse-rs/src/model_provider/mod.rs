//! Model providers (ADR-0022 stage 2): one small trait over every wire
//! protocol, capability metadata as data, one endpoint policy for retry,
//! fallback, and routing, and role-based selection.
//!
//! - [`ModelProvider`]: `generate` plus `stream`. `stream` has a default that
//!   replays `generate`, so a provider that cannot stream still streams.
//! - [`ModelCapabilities`]: declared per endpoint, checked before a request
//!   is sent ([`ModelCapabilities::check`]). Nothing is inferred from a model
//!   name.
//! - [`types`]: provider-neutral messages, tool calls, and stream events,
//!   normalizing Anthropic `tool_use` blocks and OpenAI `tool_calls`.
//! - [`anthropic`], [`openai_chat`]: the two HTTP wire implementations.
//!   `openai_chat` covers OpenAI, Ollama, OpenRouter, vLLM, and LiteLLM.
//! - [`policy`]: [`policy::EndpointPolicy`] and
//!   [`policy::PolicyProvider`], one mechanism for retry, fallback, and
//!   local-first routing. The policy is itself a `ModelProvider`.
//! - [`router`]: maps the ion, photon, and orchestrator roles to a policy
//!   over configured endpoints.
//! - [`service`]: a `tower::Service` adapter, for composing middleware
//!   (timeouts, concurrency limits) instead of a bespoke chain layer.
//! - [`ion_bridge`]: lets Ion's existing `LlmProvider` loop run on any
//!   `ModelProvider`.

pub mod anthropic;
#[cfg(test)]
mod http_tests;
pub mod ion_bridge;
pub mod openai_chat;
pub mod policy;
pub mod router;
pub mod service;
mod sse;
pub mod types;

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

pub use types::{
    ContentBlock, InvalidToolCall, Message, MessageRole, ModelRequest, ModelResponse, StopReason,
    StreamAccumulator, StreamEvent, ToolCall, ToolSpec, Usage,
};

/// What a provider can do, declared as data.
///
/// Defaults come from the wire protocol ([`ModelCapabilities::for_protocol`]);
/// an endpoint profile may override them in `config.json`. Model names are
/// never parsed for capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub tool_calling: bool,
    pub vision: bool,
    pub streaming: bool,
    /// The endpoint can be asked for JSON output. Impulse validates
    /// structured output in the harness; this only says the endpoint accepts
    /// the request.
    pub structured_output: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
}

impl ModelCapabilities {
    /// What the wire protocol supports. Real endpoints narrow this through
    /// their profile; a small local model without tool calling, for example,
    /// declares `tool_calling: false`.
    pub fn for_protocol(protocol: crate::model_endpoint::WireProtocol) -> Self {
        // All three protocols carry tools, images, streaming, and JSON
        // output; the match is exhaustive so a new protocol must state its
        // own defaults.
        match protocol {
            crate::model_endpoint::WireProtocol::AnthropicMessages
            | crate::model_endpoint::WireProtocol::OpenaiChat
            | crate::model_endpoint::WireProtocol::OpenaiResponses => Self {
                tool_calling: true,
                vision: true,
                streaming: true,
                structured_output: true,
                context_window: None,
            },
        }
    }

    /// Refuses a request this provider cannot serve, before any network call.
    pub fn check(&self, request: &ModelRequest) -> Result<(), ProviderError> {
        if !request.tools.is_empty() && !self.tool_calling {
            return Err(ProviderError::Unsupported {
                capability: "tool_calling",
            });
        }
        if request.has_images() && !self.vision {
            return Err(ProviderError::Unsupported {
                capability: "vision",
            });
        }
        Ok(())
    }
}

/// How the policy should treat an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Try the same endpoint again after a delay.
    Retry,
    /// This endpoint cannot serve the request; try the next one.
    NextEndpoint,
    /// Stop: the walk itself has already ended.
    Fatal,
}

/// Why a provider call failed.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("endpoint does not support {capability}")]
    Unsupported { capability: &'static str },
    #[error("credential environment variable {env} is not set")]
    MissingCredential { env: String },
    #[error("cannot reach {endpoint}: {message}")]
    Transport { endpoint: String, message: String },
    #[error("{endpoint} timed out after {seconds}s")]
    Timeout { endpoint: String, seconds: u64 },
    #[error("{endpoint} rate-limited the request")]
    RateLimited {
        endpoint: String,
        retry_after: Option<Duration>,
    },
    #[error("{endpoint} returned HTTP {status}: {body}")]
    Http {
        endpoint: String,
        status: u16,
        body: String,
    },
    #[error("{endpoint} returned an unreadable response: {message}")]
    InvalidResponse { endpoint: String, message: String },
    #[error("the {protocol} wire protocol is not implemented by a provider yet")]
    UnsupportedProtocol {
        protocol: crate::model_endpoint::WireProtocol,
    },
    #[error("no endpoint could serve the request: {}", summarize(.attempts))]
    Exhausted { attempts: Vec<AttemptFailure> },
}

/// One failed attempt, recorded by the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptFailure {
    pub endpoint: String,
    pub error: String,
}

fn summarize(attempts: &[AttemptFailure]) -> String {
    if attempts.is_empty() {
        return "no endpoints were configured".to_string();
    }
    attempts
        .iter()
        .map(|attempt| format!("{}: {}", attempt.endpoint, attempt.error))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Longest error body kept in a [`ProviderError::Http`].
pub const MAX_ERROR_BODY_BYTES: usize = 2048;

impl ProviderError {
    /// How the policy should react.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Transport { .. } | Self::Timeout { .. } | Self::RateLimited { .. } => {
                ErrorClass::Retry
            }
            Self::Http { status, .. } if *status >= 500 || *status == 408 => ErrorClass::Retry,
            // Every other status, 400 and 422 included, moves on. A 400 often
            // describes what this endpoint's model cannot do ("does not
            // support tools", "context too long"), and the next candidate
            // may be a different model behind a different protocol.
            Self::Http { .. }
            | Self::Unsupported { .. }
            | Self::MissingCredential { .. }
            | Self::InvalidResponse { .. }
            | Self::UnsupportedProtocol { .. } => ErrorClass::NextEndpoint,
            Self::Exhausted { .. } => ErrorClass::Fatal,
        }
    }

    /// The server's requested delay, when it sent one.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

/// One model behind one wire protocol.
///
/// Implementations are cheap to share (`Arc<dyn ModelProvider>`) and hold no
/// conversation state; a request carries the whole conversation.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// A stable label for logs and errors (the profile name when known).
    fn name(&self) -> &str;

    /// What this provider can do.
    fn capabilities(&self) -> ModelCapabilities;

    /// One complete reply.
    async fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError>;

    /// The reply as incremental events. The default replays [`generate`]
    /// as one burst, so callers can always stream.
    ///
    /// [`generate`]: ModelProvider::generate
    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
        use futures::StreamExt as _;
        futures::stream::once(async move { self.generate(request).await })
            .flat_map(|result| match result {
                Ok(response) => futures::stream::iter(
                    types::events_from_response(&response)
                        .into_iter()
                        .map(Ok)
                        .collect::<Vec<_>>(),
                )
                .boxed(),
                Err(err) => futures::stream::iter(vec![Err(err)]).boxed(),
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::WireProtocol;
    use futures::StreamExt as _;

    struct Fixed;

    #[async_trait]
    impl ModelProvider for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities::for_protocol(WireProtocol::OpenaiChat)
        }
        async fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
            Ok(ModelResponse {
                content: vec![ContentBlock::text("hi")],
                invalid_tool_calls: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
                model: "fixed-model".into(),
            })
        }
    }

    #[tokio::test]
    async fn test_default_stream_replays_generate() {
        let provider = Fixed;
        let request = ModelRequest::from_user("x");
        let mut acc = StreamAccumulator::new();
        let mut events = provider.stream(&request);
        while let Some(event) = events.next().await {
            acc.push(event.expect("event"));
        }
        let response = acc.finish();
        assert_eq!(response.text(), "hi");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_capabilities_check_refuses_tools_and_images_when_unsupported() {
        let mut caps = ModelCapabilities::for_protocol(WireProtocol::OpenaiChat);
        caps.tool_calling = false;
        caps.vision = false;
        let mut request = ModelRequest::from_user("x");
        caps.check(&request).expect("plain text is fine");
        request.tools.push(ToolSpec {
            name: "t".into(),
            description: "d".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        let err = caps.check(&request).unwrap_err();
        assert!(matches!(
            err,
            ProviderError::Unsupported {
                capability: "tool_calling"
            }
        ));
        request.tools.clear();
        request.messages[0].content.push(ContentBlock::Image {
            media_type: "image/png".into(),
            data_base64: "AA".into(),
        });
        assert!(caps
            .check(&request)
            .unwrap_err()
            .to_string()
            .contains("vision"));
    }

    #[test]
    fn test_capabilities_round_trip_and_reject_unknown_fields() {
        let caps = ModelCapabilities {
            context_window: Some(128_000),
            ..ModelCapabilities::for_protocol(WireProtocol::AnthropicMessages)
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert_eq!(
            serde_json::from_str::<ModelCapabilities>(&json).unwrap(),
            caps
        );
        assert!(serde_json::from_str::<ModelCapabilities>(
            r#"{"tool_calling":true,"vision":true,"streaming":true,"structured_output":true,"thinking":true}"#
        )
        .is_err());
    }

    #[test]
    fn test_error_classes_drive_retry_next_and_fatal() {
        let http = |status| ProviderError::Http {
            endpoint: "e".into(),
            status,
            body: String::new(),
        };
        assert_eq!(http(503).class(), ErrorClass::Retry);
        assert_eq!(http(408).class(), ErrorClass::Retry);
        assert_eq!(
            http(400).class(),
            ErrorClass::NextEndpoint,
            "a 400 may be this model's limitation, not the request's"
        );
        assert_eq!(http(401).class(), ErrorClass::NextEndpoint);
        assert_eq!(http(404).class(), ErrorClass::NextEndpoint);
        assert_eq!(
            ProviderError::Transport {
                endpoint: "e".into(),
                message: "refused".into()
            }
            .class(),
            ErrorClass::Retry
        );
        assert_eq!(
            ProviderError::Unsupported {
                capability: "vision"
            }
            .class(),
            ErrorClass::NextEndpoint
        );
        let limited = ProviderError::RateLimited {
            endpoint: "e".into(),
            retry_after: Some(Duration::from_secs(3)),
        };
        assert_eq!(limited.class(), ErrorClass::Retry);
        assert_eq!(limited.retry_after(), Some(Duration::from_secs(3)));
    }

    #[test]
    fn test_error_display_names_endpoint_and_cause() {
        let err = ProviderError::Exhausted {
            attempts: vec![
                AttemptFailure {
                    endpoint: "local".into(),
                    error: "refused".into(),
                },
                AttemptFailure {
                    endpoint: "cloud".into(),
                    error: "HTTP 401".into(),
                },
            ],
        };
        let text = err.to_string();
        assert!(text.contains("local: refused") && text.contains("cloud: HTTP 401"));
        assert!(ProviderError::Exhausted { attempts: vec![] }
            .to_string()
            .contains("no endpoints"));
        assert!(ProviderError::MissingCredential {
            env: "OPENAI_API_KEY".into()
        }
        .to_string()
        .contains("OPENAI_API_KEY"));
        assert!(ProviderError::UnsupportedProtocol {
            protocol: WireProtocol::OpenaiResponses
        }
        .to_string()
        .contains("openai_responses"));
        assert!(ProviderError::Timeout {
            endpoint: "e".into(),
            seconds: 5
        }
        .to_string()
        .contains("5s"));
        assert!(ProviderError::InvalidResponse {
            endpoint: "e".into(),
            message: "bad".into()
        }
        .to_string()
        .contains("bad"));
    }
}
