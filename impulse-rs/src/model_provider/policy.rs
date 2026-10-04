//! One policy for retry, fallback, and routing.
//!
//! LangChain keeps these as three separate mechanisms (client retries,
//! `with_fallbacks`, and a router), and their interaction is a known source
//! of surprise. Here they are one ordered walk:
//!
//! 1. **Routing** orders the candidates. `local_first` (the default) moves
//!    endpoints on a numeric loopback address (Ollama, a local vLLM) ahead of
//!    remote ones, keeping configured order within each group; `in_order`
//!    keeps configured order.
//! 2. **Capability** skips a candidate that cannot serve the request (tools
//!    on a model without tool calling, images without vision), before any
//!    network call.
//! 3. **Retry** repeats a candidate on a retryable error (transport,
//!    timeout, 429, 5xx) with capped exponential backoff, honoring a server's
//!    `Retry-After` up to the cap.
//! 4. **Fallback** moves to the next candidate when retries run out, when a
//!    server asks for a wait longer than the backoff cap, or on any other
//!    error: 401, 403, 404, a missing credential, and also 400 and 422,
//!    because a local model's "does not support tools" or "context too long"
//!    is a 400 the next (different) model may not return.
//!
//! [`PolicyProvider`] is itself a [`ModelProvider`], so the policy composes
//! like any provider. A stream falls back only before its first event; once
//! output has reached the caller it cannot be replayed from another model.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt as _};
use serde::{Deserialize, Serialize};

use super::{
    AttemptFailure, ErrorClass, ModelCapabilities, ModelProvider, ModelRequest, ModelResponse,
    ProviderError, StreamEvent,
};

/// Candidate order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Routing {
    /// Loopback endpoints first, then remote; configured order within each.
    #[default]
    LocalFirst,
    /// Configured order.
    InOrder,
}

/// Retry settings for one candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetryPolicy {
    /// Attempts per candidate, including the first. At least 1.
    pub max_attempts: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            initial_backoff_ms: 500,
            max_backoff_ms: 8_000,
        }
    }
}

impl RetryPolicy {
    /// Whether a server-requested wait is longer than this policy will wait.
    /// Retrying early would only earn another 429, so the walk moves on.
    pub fn exceeds_cap(&self, server_hint: Option<Duration>) -> bool {
        server_hint.is_some_and(|hint| hint > Duration::from_millis(self.max_backoff_ms))
    }

    /// Delay before retry number `retry` (1 for the first retry).
    pub fn backoff(&self, retry: u32, server_hint: Option<Duration>) -> Duration {
        let cap = Duration::from_millis(self.max_backoff_ms);
        if let Some(hint) = server_hint {
            return hint.min(cap);
        }
        let exponent = retry.saturating_sub(1).min(16);
        Duration::from_millis(self.initial_backoff_ms.saturating_mul(1 << exponent)).min(cap)
    }
}

/// The one policy for an endpoint chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EndpointPolicy {
    pub routing: Routing,
    pub retry: RetryPolicy,
}

/// Longest backoff a config may set.
pub const MAX_BACKOFF_MS: u64 = 60_000;
/// Most attempts per candidate a config may set.
pub const MAX_ATTEMPTS: u32 = 10;

impl EndpointPolicy {
    /// Checks the retry bounds.
    pub fn validate(&self) -> Result<(), String> {
        let retry = &self.retry;
        if !(1..=MAX_ATTEMPTS).contains(&retry.max_attempts) {
            return Err(format!(
                "policy.retry.max_attempts must be between 1 and {MAX_ATTEMPTS}"
            ));
        }
        if retry.max_backoff_ms > MAX_BACKOFF_MS || retry.initial_backoff_ms > retry.max_backoff_ms
        {
            return Err(format!(
                "policy.retry backoff must satisfy initial_backoff_ms <= max_backoff_ms <= {MAX_BACKOFF_MS}"
            ));
        }
        Ok(())
    }

    /// Orders `candidates` by this policy's routing.
    pub fn order(&self, mut candidates: Vec<Candidate>) -> Vec<Candidate> {
        if self.routing == Routing::LocalFirst {
            // Stable: configured order survives within each group.
            candidates.sort_by_key(|candidate| !candidate.local);
        }
        candidates
    }
}

/// One endpoint the policy may use.
#[derive(Clone)]
pub struct Candidate {
    pub provider: Arc<dyn ModelProvider>,
    /// Whether the endpoint is on this machine (numeric loopback).
    pub local: bool,
}

/// A provider that walks an ordered candidate list under one policy.
pub struct PolicyProvider {
    name: String,
    candidates: Vec<Candidate>,
    retry: RetryPolicy,
}

impl PolicyProvider {
    /// Orders `candidates` by `policy` and wraps them.
    pub fn new(
        name: impl Into<String>,
        policy: EndpointPolicy,
        candidates: Vec<Candidate>,
    ) -> Self {
        Self {
            name: name.into(),
            candidates: policy.order(candidates),
            retry: policy.retry,
        }
    }

    /// Candidate names in the order they will be tried.
    pub fn order(&self) -> Vec<&str> {
        self.candidates
            .iter()
            .map(|candidate| candidate.provider.name())
            .collect()
    }

    /// Runs `call` against each candidate in order under the policy.
    async fn walk<'a, T, F, Fut>(
        &'a self,
        request: &'a ModelRequest,
        mut call: F,
    ) -> Result<T, ProviderError>
    where
        F: FnMut(&'a dyn ModelProvider) -> Fut,
        Fut: std::future::Future<Output = Result<T, ProviderError>> + 'a,
    {
        let mut attempts = Vec::new();
        for candidate in &self.candidates {
            let provider = candidate.provider.as_ref();
            if let Err(err) = provider.capabilities().check(request) {
                attempts.push(AttemptFailure {
                    endpoint: provider.name().to_string(),
                    error: err.to_string(),
                });
                continue;
            }
            let mut attempt = 1;
            loop {
                match call(provider).await {
                    Ok(value) => return Ok(value),
                    Err(err) => match err.class() {
                        ErrorClass::Fatal => return Err(err),
                        ErrorClass::Retry
                            if attempt < self.retry.max_attempts
                                && !self.retry.exceeds_cap(err.retry_after()) =>
                        {
                            tokio::time::sleep(self.retry.backoff(attempt, err.retry_after()))
                                .await;
                            attempt += 1;
                        }
                        ErrorClass::Retry | ErrorClass::NextEndpoint => {
                            tracing::warn!(
                                endpoint = provider.name(),
                                "model endpoint failed, trying the next one: {err}"
                            );
                            attempts.push(AttemptFailure {
                                endpoint: provider.name().to_string(),
                                error: err.to_string(),
                            });
                            break;
                        }
                    },
                }
            }
        }
        Err(ProviderError::Exhausted { attempts })
    }
}

type OpenedStream<'a> = (
    StreamEvent,
    BoxStream<'a, Result<StreamEvent, ProviderError>>,
);

#[async_trait]
impl ModelProvider for PolicyProvider {
    fn name(&self) -> &str {
        &self.name
    }

    /// What at least one candidate can do. Each request is still checked
    /// against each candidate before it is sent.
    fn capabilities(&self) -> ModelCapabilities {
        let mut union = ModelCapabilities {
            tool_calling: false,
            vision: false,
            streaming: false,
            structured_output: false,
            context_window: None,
        };
        for candidate in &self.candidates {
            let caps = candidate.provider.capabilities();
            union.tool_calling |= caps.tool_calling;
            union.vision |= caps.vision;
            union.streaming |= caps.streaming;
            union.structured_output |= caps.structured_output;
            union.context_window = union.context_window.max(caps.context_window);
        }
        union
    }

    async fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        self.walk(request, move |provider| provider.generate(request))
            .await
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
    ) -> BoxStream<'a, Result<StreamEvent, ProviderError>> {
        // Open each candidate's stream and wait for its first event. A
        // candidate that fails before producing output is retried or skipped
        // exactly like a failed `generate`; after the first event the stream
        // is committed to that candidate.
        let opened = self.walk(request, move |provider| async move {
            let mut events = provider.stream(request);
            match events.next().await {
                Some(Ok(first)) => Ok::<OpenedStream<'a>, ProviderError>((first, events)),
                Some(Err(err)) => Err(err),
                None => Err(ProviderError::InvalidResponse {
                    endpoint: provider.name().to_string(),
                    message: "stream ended before any event".to_string(),
                }),
            }
        });
        futures::stream::once(opened)
            .flat_map(|opened| match opened {
                Ok((first, rest)) => futures::stream::once(async move { Ok(first) })
                    .chain(rest)
                    .boxed(),
                Err(err) => futures::stream::once(async move { Err(err) }).boxed(),
            })
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::WireProtocol;
    use crate::model_provider::{ContentBlock, StopReason, StreamAccumulator, ToolSpec, Usage};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// Test double: replays a scripted sequence of results and counts calls.
    struct Scripted {
        name: &'static str,
        capabilities: ModelCapabilities,
        script: Mutex<Vec<Result<&'static str, ProviderError>>>,
        calls: AtomicUsize,
    }

    impl Scripted {
        fn new(name: &'static str, script: Vec<Result<&'static str, ProviderError>>) -> Arc<Self> {
            Arc::new(Self {
                name,
                capabilities: ModelCapabilities::for_protocol(WireProtocol::OpenaiChat),
                script: Mutex::new(script),
                calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ModelProvider for Scripted {
        fn name(&self) -> &str {
            self.name
        }
        fn capabilities(&self) -> ModelCapabilities {
            self.capabilities
        }
        async fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let next = {
                let mut script = self.script.lock().unwrap();
                if script.is_empty() {
                    Ok("default")
                } else {
                    script.remove(0)
                }
            };
            next.map(|text| ModelResponse {
                content: vec![ContentBlock::text(text)],
                invalid_tool_calls: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
                model: self.name.to_string(),
            })
        }
    }

    fn transport(endpoint: &str) -> ProviderError {
        ProviderError::Transport {
            endpoint: endpoint.into(),
            message: "connection refused".into(),
        }
    }

    fn http(status: u16) -> ProviderError {
        ProviderError::Http {
            endpoint: "e".into(),
            status,
            body: String::new(),
        }
    }

    fn candidate(provider: Arc<Scripted>, local: bool) -> Candidate {
        Candidate { provider, local }
    }

    #[test]
    fn test_local_first_moves_loopback_endpoints_ahead_stably() {
        let cloud_a = Scripted::new("cloud-a", vec![]);
        let local = Scripted::new("ollama", vec![]);
        let cloud_b = Scripted::new("cloud-b", vec![]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![
                candidate(cloud_a.clone(), false),
                candidate(local.clone(), true),
                candidate(cloud_b.clone(), false),
            ],
        );
        assert_eq!(policy.order(), ["ollama", "cloud-a", "cloud-b"]);
        let in_order = PolicyProvider::new(
            "ion",
            EndpointPolicy {
                routing: Routing::InOrder,
                ..EndpointPolicy::default()
            },
            vec![candidate(cloud_a, false), candidate(local, true)],
        );
        assert_eq!(in_order.order(), ["cloud-a", "ollama"]);
    }

    #[tokio::test(start_paused = true)]
    async fn test_local_failure_falls_back_to_cloud_after_retries() {
        let local = Scripted::new(
            "ollama",
            vec![Err(transport("ollama")), Err(transport("ollama"))],
        );
        let cloud = Scripted::new("cloud", vec![Ok("from cloud")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![
                candidate(cloud.clone(), false),
                candidate(local.clone(), true),
            ],
        );
        let response = policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .expect("cloud answers");
        assert_eq!(response.text(), "from cloud");
        assert_eq!(local.calls(), 2, "local was tried first, with one retry");
        assert_eq!(cloud.calls(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_retryable_error_then_success_stays_on_the_same_endpoint() {
        let local = Scripted::new("ollama", vec![Err(http(503)), Ok("recovered")]);
        let cloud = Scripted::new("cloud", vec![]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![
                candidate(local.clone(), true),
                candidate(cloud.clone(), false),
            ],
        );
        let response = policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap();
        assert_eq!(response.text(), "recovered");
        assert_eq!(cloud.calls(), 0);
    }

    #[tokio::test]
    async fn test_endpoint_error_moves_on_without_retrying() {
        let first = Scripted::new("first", vec![Err(http(401))]);
        let second = Scripted::new("second", vec![Ok("ok")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![candidate(first.clone(), false), candidate(second, false)],
        );
        policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap();
        assert_eq!(first.calls(), 1, "a 401 is not retried");
    }

    /// Review P1: Ollama answers "does not support tools" with a 400; that
    /// must not stop the walk before the cloud fallback.
    #[tokio::test]
    async fn test_a_400_from_the_local_model_falls_back_to_cloud() {
        let local = Scripted::new("ollama", vec![Err(http(400))]);
        let cloud = Scripted::new("cloud", vec![Ok("from cloud")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![
                candidate(local.clone(), true),
                candidate(cloud.clone(), false),
            ],
        );
        let response = policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap();
        assert_eq!(response.text(), "from cloud");
        assert_eq!(local.calls(), 1, "a 400 is not retried");
    }

    #[tokio::test(start_paused = true)]
    async fn test_a_retry_after_beyond_the_cap_moves_on_instead_of_retrying_early() {
        let limited = Scripted::new(
            "limited",
            vec![Err(ProviderError::RateLimited {
                endpoint: "limited".into(),
                retry_after: Some(Duration::from_secs(3_600)),
            })],
        );
        let other = Scripted::new("other", vec![Ok("ok")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![candidate(limited.clone(), false), candidate(other, false)],
        );
        policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap();
        assert_eq!(limited.calls(), 1);
    }

    #[tokio::test]
    async fn test_capability_routing_skips_a_local_model_without_tools() {
        let mut local_inner = Scripted::new("small-local", vec![]);
        Arc::get_mut(&mut local_inner)
            .unwrap()
            .capabilities
            .tool_calling = false;
        let cloud = Scripted::new("cloud", vec![Ok("with tools")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![
                candidate(local_inner.clone(), true),
                candidate(cloud, false),
            ],
        );
        let mut request = ModelRequest::from_user("x");
        request.tools.push(ToolSpec {
            name: "t".into(),
            description: "d".into(),
            input_schema: serde_json::json!({"type": "object"}),
        });
        let response = policy.generate(&request).await.unwrap();
        assert_eq!(response.text(), "with tools");
        assert_eq!(local_inner.calls(), 0, "skipped before any call");
        // Without tools the local model serves the request.
        policy
            .generate(&ModelRequest::from_user("y"))
            .await
            .unwrap();
        assert_eq!(local_inner.calls(), 1);
        assert!(policy.capabilities().tool_calling, "union of candidates");
    }

    #[tokio::test(start_paused = true)]
    async fn test_all_endpoints_failing_reports_every_attempt() {
        let a = Scripted::new("a", vec![Err(transport("a")), Err(transport("a"))]);
        let b = Scripted::new("b", vec![Err(http(404))]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![candidate(a, true), candidate(b, false)],
        );
        let err = policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap_err();
        let ProviderError::Exhausted { attempts } = &err else {
            panic!("expected Exhausted, got {err}");
        };
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].endpoint, "a");
        assert!(err.to_string().contains("HTTP 404"));
    }

    #[tokio::test]
    async fn test_empty_candidate_list_is_exhausted() {
        let policy = PolicyProvider::new("ion", EndpointPolicy::default(), vec![]);
        let err = policy
            .generate(&ModelRequest::from_user("x"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no endpoints"));
    }

    #[tokio::test(start_paused = true)]
    async fn test_stream_falls_back_before_the_first_event() {
        let local = Scripted::new("ollama", vec![Err(http(404))]);
        let cloud = Scripted::new("cloud", vec![Ok("streamed")]);
        let policy = PolicyProvider::new(
            "ion",
            EndpointPolicy::default(),
            vec![candidate(local, true), candidate(cloud, false)],
        );
        let request = ModelRequest::from_user("x");
        let mut acc = StreamAccumulator::new();
        let mut events = policy.stream(&request);
        while let Some(event) = events.next().await {
            acc.push(event.expect("event"));
        }
        let response = acc.finish();
        assert_eq!(response.text(), "streamed");
        assert_eq!(response.model, "cloud");
    }

    #[test]
    fn test_backoff_is_exponential_capped_and_honors_server_hint() {
        let retry = RetryPolicy {
            max_attempts: 5,
            initial_backoff_ms: 100,
            max_backoff_ms: 1_000,
        };
        assert_eq!(retry.backoff(1, None), Duration::from_millis(100));
        assert_eq!(retry.backoff(3, None), Duration::from_millis(400));
        assert_eq!(retry.backoff(9, None), Duration::from_millis(1_000));
        assert_eq!(
            retry.backoff(1, Some(Duration::from_secs(30))),
            Duration::from_millis(1_000),
            "a server hint is capped"
        );
    }

    #[test]
    fn test_policy_validate_and_round_trip() {
        let policy = EndpointPolicy::default();
        policy.validate().expect("defaults are valid");
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(
            serde_json::from_str::<EndpointPolicy>(&json).unwrap(),
            policy
        );
        let parsed: EndpointPolicy =
            serde_json::from_str(r#"{"routing":"in_order","retry":{"max_attempts":3}}"#).unwrap();
        assert_eq!(parsed.routing, Routing::InOrder);
        assert_eq!(parsed.retry.initial_backoff_ms, 500);
        let zero = EndpointPolicy {
            retry: RetryPolicy {
                max_attempts: 0,
                ..RetryPolicy::default()
            },
            ..EndpointPolicy::default()
        };
        assert!(zero.validate().unwrap_err().contains("max_attempts"));
        let inverted = EndpointPolicy {
            retry: RetryPolicy {
                max_attempts: 1,
                initial_backoff_ms: 9_000,
                max_backoff_ms: 1_000,
            },
            ..EndpointPolicy::default()
        };
        assert!(inverted.validate().is_err());
        assert!(serde_json::from_str::<EndpointPolicy>(r#"{"routng":"in_order"}"#).is_err());
    }
}
