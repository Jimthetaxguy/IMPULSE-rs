//! A `tower::Service` view of a [`ModelProvider`].
//!
//! Composition (timeouts, concurrency limits, rate limits, tracing) uses
//! tower's middleware instead of a bespoke chain layer such as LangChain's
//! Runnable. Retry and fallback stay in [`super::policy`], where they share
//! one error classification; tower supplies the generic layers.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::{ModelProvider, ModelRequest, ModelResponse, ProviderError};

/// Wraps a shared provider as a `tower::Service<ModelRequest>`.
#[derive(Clone)]
pub struct ProviderService {
    provider: Arc<dyn ModelProvider>,
}

impl ProviderService {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self { provider }
    }
}

impl tower::Service<ModelRequest> for ProviderService {
    type Response = ModelResponse;
    type Error = ProviderError;
    type Future = Pin<Box<dyn Future<Output = Result<ModelResponse, ProviderError>> + Send>>;

    /// A provider holds no per-call capacity of its own; limits come from
    /// layers such as `tower::limit::ConcurrencyLimit`.
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: ModelRequest) -> Self::Future {
        let provider = Arc::clone(&self.provider);
        Box::pin(async move { provider.generate(&request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::WireProtocol;
    use crate::model_provider::{ContentBlock, ModelCapabilities, StopReason, Usage};
    use async_trait::async_trait;
    use tower::{Service as _, ServiceBuilder, ServiceExt as _};

    struct Echo;

    #[async_trait]
    impl ModelProvider for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities::for_protocol(WireProtocol::OpenaiChat)
        }
        async fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
            let text = match request.messages.first().and_then(|m| m.content.first()) {
                Some(ContentBlock::Text { text }) => text.clone(),
                _ => String::new(),
            };
            Ok(ModelResponse {
                content: vec![ContentBlock::text(text)],
                invalid_tool_calls: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
                model: "echo".into(),
            })
        }
    }

    #[tokio::test]
    async fn test_service_runs_the_provider_under_tower_middleware() {
        let mut service = ServiceBuilder::new()
            .concurrency_limit(1)
            .service(ProviderService::new(Arc::new(Echo)));
        let response = service
            .ready()
            .await
            .expect("ready")
            .call(ModelRequest::from_user("ping"))
            .await
            .expect("response");
        assert_eq!(response.text(), "ping");
    }
}
