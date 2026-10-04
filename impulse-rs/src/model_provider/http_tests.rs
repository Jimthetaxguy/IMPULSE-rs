//! End-to-end tests of both wire providers over real HTTP: a loopback
//! socket serves recorded response bytes, so `reqwest`, status handling,
//! and the SSE reader run exactly as they do against a server.
//!
//! The opt-in test at the bottom calls a real local Ollama.

use std::sync::Arc;

use futures::StreamExt as _;
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::anthropic::AnthropicProvider;
use super::openai_chat::OpenAiChatProvider;
use super::policy::{Candidate, EndpointPolicy, PolicyProvider};
use super::router::capabilities_of;
use super::{ModelProvider, ModelRequest, ProviderError, StopReason, StreamAccumulator, ToolSpec};
use crate::model_endpoint::{EndpointAuth, ModelEndpoint, WireProtocol};

/// Serves `responses` in order, one per connection, and returns each raw
/// request it received.
async fn serve(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let address = listener.local_addr().expect("address");
    let handle = tokio::spawn(async move {
        let mut seen = Vec::new();
        for response in responses {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0u8; 8192];
            // Read headers, then the declared body length.
            loop {
                let n = socket.read(&mut buffer).await.expect("read");
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_ascii_lowercase();
                            lower
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            seen.push(String::from_utf8_lossy(&request).to_string());
            socket.write_all(response.as_bytes()).await.expect("write");
            socket.shutdown().await.ok();
        }
        seen
    });
    (format!("http://{address}/v1"), handle)
}

fn http_response(status: &str, content_type: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n{extra_headers}connection: close\r\n\r\n{body}",
        body.len()
    )
}

fn endpoint(protocol: WireProtocol, base_url: &str) -> ModelEndpoint {
    ModelEndpoint {
        protocol,
        base_url: base_url.to_string(),
        auth: EndpointAuth::None,
        model: "test-model".into(),
        max_output_tokens: Some(256),
        capabilities: None,
    }
}

fn openai(base_url: &str) -> OpenAiChatProvider {
    let endpoint = endpoint(WireProtocol::OpenaiChat, base_url);
    let caps = capabilities_of(&endpoint);
    OpenAiChatProvider::new("local", endpoint, caps).expect("provider")
}

fn anthropic(base_url: &str) -> AnthropicProvider {
    let endpoint = endpoint(WireProtocol::AnthropicMessages, base_url);
    let caps = capabilities_of(&endpoint);
    AnthropicProvider::new("anthropic", endpoint, caps).expect("provider")
}

fn tool_request() -> ModelRequest {
    let mut request = ModelRequest::from_user("list files");
    request.tools.push(ToolSpec {
        name: "file_read".into(),
        description: "read a file".into(),
        input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    });
    request
}

#[tokio::test]
async fn test_openai_generate_over_http_posts_to_chat_completions() {
    let body = json!({
        "model": "test-model",
        "choices": [{"finish_reason": "tool_calls", "message": {"content": null, "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "file_read", "arguments": "{\"path\":\"a\"}"}}
        ]}}],
        "usage": {"prompt_tokens": 7, "completion_tokens": 2}
    })
    .to_string();
    let (base, server) = serve(vec![http_response("200 OK", "application/json", "", &body)]).await;
    let response = openai(&base)
        .generate(&tool_request())
        .await
        .expect("generate");
    assert_eq!(response.tool_calls()[0].arguments, json!({"path": "a"}));
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let seen = server.await.expect("server");
    assert!(seen[0].starts_with("POST /v1/chat/completions"));
    assert!(seen[0].contains("\"type\":\"function\""));
}

#[tokio::test]
async fn test_openai_stream_over_http_reads_sse_to_done() {
    let sse = [
        r#"data: {"model":"test-model","choices":[{"delta":{"content":"Hel"}}]}"#,
        r#"data: {"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}"#,
        r#"data: {"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2}}"#,
        "data: [DONE]",
    ]
    .join("\n\n")
        + "\n\n";
    let (base, _server) = serve(vec![http_response("200 OK", "text/event-stream", "", &sse)]).await;
    let provider = openai(&base);
    let request = ModelRequest::from_user("hi");
    let mut acc = StreamAccumulator::new();
    let mut events = provider.stream(&request);
    while let Some(event) = events.next().await {
        acc.push(event.expect("event"));
    }
    let response = acc.finish();
    assert_eq!(response.text(), "Hello");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(response.usage.output_tokens, 2);
}

#[tokio::test]
async fn test_anthropic_generate_and_stream_over_http() {
    let body = json!({
        "model": "test-model",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 3, "output_tokens": 1},
        "content": [{"type": "text", "text": "Hi"}]
    })
    .to_string();
    let sse = [
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"test-model\",\"usage\":{\"input_tokens\":3}}}",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Yo\"}}",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}",
    ]
    .join("\n\n")
        + "\n\n";
    let (base, server) = serve(vec![
        http_response("200 OK", "application/json", "", &body),
        http_response("200 OK", "text/event-stream", "", &sse),
    ])
    .await;
    let provider = anthropic(&base);
    let request = ModelRequest::from_user("hi");
    assert_eq!(
        provider.generate(&request).await.expect("generate").text(),
        "Hi"
    );
    let mut acc = StreamAccumulator::new();
    let mut events = provider.stream(&request);
    while let Some(event) = events.next().await {
        acc.push(event.expect("event"));
    }
    assert_eq!(acc.finish().text(), "Yo");
    let seen = server.await.expect("server");
    assert!(seen[0].starts_with("POST /v1/messages"));
    assert!(seen[0]
        .to_ascii_lowercase()
        .contains("anthropic-version: 2023-06-01"));
    assert!(seen[1].contains("\"stream\":true"));
}

#[tokio::test]
async fn test_http_errors_map_to_policy_classes() {
    let (base, _server) = serve(vec![
        http_response(
            "429 Too Many Requests",
            "application/json",
            "retry-after: 7\r\n",
            "{}",
        ),
        http_response(
            "401 Unauthorized",
            "application/json",
            "",
            "{\"error\":\"bad key\"}",
        ),
    ])
    .await;
    let provider = openai(&base);
    let request = ModelRequest::from_user("x");
    let limited = provider.generate(&request).await.unwrap_err();
    assert_eq!(
        limited.retry_after(),
        Some(std::time::Duration::from_secs(7))
    );
    let unauthorized = provider.generate(&request).await.unwrap_err();
    assert!(matches!(
        unauthorized,
        ProviderError::Http { status: 401, ref body, .. } if body.contains("bad key")
    ));
}

/// A redirect is an error, never followed, so a credential header cannot
/// be carried to another host.
#[tokio::test]
async fn test_redirects_are_not_followed() {
    let (base, server) = serve(vec![http_response(
        "307 Temporary Redirect",
        "text/plain",
        "location: http://127.0.0.1:9/elsewhere\r\n",
        "",
    )])
    .await;
    let err = openai(&base)
        .generate(&ModelRequest::from_user("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::Http { status: 307, .. }),
        "{err}"
    );
    assert_eq!(server.await.expect("server").len(), 1);
}

#[tokio::test]
async fn test_missing_credential_is_reported_before_any_request() {
    let mut endpoint = endpoint(WireProtocol::OpenaiChat, "http://127.0.0.1:9/v1");
    endpoint.auth = EndpointAuth::BearerEnv {
        env: "IMPULSE_TEST_MODEL_KEY_NEVER_SET".into(),
    };
    let caps = capabilities_of(&endpoint);
    let provider = OpenAiChatProvider::new("cloud", endpoint, caps).expect("provider");
    let err = provider
        .generate(&ModelRequest::from_user("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::MissingCredential { .. }));
}

/// A dead local endpoint (nothing listening) falls through to a working one.
#[tokio::test(start_paused = false)]
async fn test_local_first_falls_back_from_a_dead_local_server_over_http() {
    // Bind and drop a listener to get a port with nothing behind it.
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_url = format!("http://{}/v1", dead.local_addr().unwrap());
    drop(dead);
    let body = json!({
        "model": "cloud-model",
        "choices": [{"finish_reason": "stop", "message": {"content": "from fallback"}}]
    })
    .to_string();
    let (live_url, _server) =
        serve(vec![http_response("200 OK", "application/json", "", &body)]).await;
    let policy = PolicyProvider::new(
        "ion",
        EndpointPolicy {
            retry: super::policy::RetryPolicy {
                max_attempts: 1,
                ..Default::default()
            },
            ..EndpointPolicy::default()
        },
        vec![
            Candidate {
                provider: Arc::new(openai(&live_url)),
                local: false,
            },
            Candidate {
                provider: Arc::new(openai(&dead_url)),
                local: true,
            },
        ],
    );
    let response = policy
        .generate(&ModelRequest::from_user("x"))
        .await
        .expect("fallback answers");
    assert_eq!(response.text(), "from fallback");
}

/// Review P2: a server that sends 200 headers and then nothing must time
/// out, so the policy can fall back before the first event. Paused time
/// skips the idle timeout without waiting for it.
#[tokio::test(start_paused = true)]
async fn test_a_stalled_stream_times_out_and_falls_back() {
    let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stalled_url = format!("http://{}/v1", stalled.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut socket, _) = stalled.accept().await.unwrap();
        let mut buffer = [0u8; 8192];
        let _ = socket.read(&mut buffer).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
            .await
            .unwrap();
        // Hold the connection open and silent.
        tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
        drop(socket);
    });
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"alive\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let (live_url, _server) =
        serve(vec![http_response("200 OK", "text/event-stream", "", sse)]).await;
    let policy = PolicyProvider::new(
        "ion",
        EndpointPolicy {
            retry: super::policy::RetryPolicy {
                max_attempts: 1,
                ..Default::default()
            },
            ..EndpointPolicy::default()
        },
        vec![
            Candidate {
                provider: Arc::new(openai(&stalled_url)),
                local: true,
            },
            Candidate {
                provider: Arc::new(openai(&live_url)),
                local: false,
            },
        ],
    );
    let request = ModelRequest::from_user("x");
    let mut acc = StreamAccumulator::new();
    let mut events = policy.stream(&request);
    while let Some(event) = events.next().await {
        acc.push(event.expect("event"));
    }
    assert_eq!(acc.finish().text(), "alive");
}

/// A 200 whose body is not JSON is the server's fault: `InvalidResponse`,
/// not a transport error that would be retried.
#[tokio::test]
async fn test_a_non_json_success_body_is_an_invalid_response() {
    let (base, _server) = serve(vec![http_response(
        "200 OK",
        "text/html",
        "",
        "<html>proxy</html>",
    )])
    .await;
    let err = openai(&base)
        .generate(&ModelRequest::from_user("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::InvalidResponse { .. }),
        "{err}"
    );
}

/// Real local model. Opt in with `IMPULSE_OLLAMA_IT=1` and a running
/// `ollama serve`; the model defaults to `qwen3` (`IMPULSE_OLLAMA_MODEL`).
#[tokio::test]
#[ignore = "needs a running local Ollama; set IMPULSE_OLLAMA_IT=1 and run with --ignored"]
async fn test_real_ollama_generates_streams_and_calls_a_tool() {
    if std::env::var("IMPULSE_OLLAMA_IT").as_deref() != Ok("1") {
        return;
    }
    let model = std::env::var("IMPULSE_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3".into());
    let mut endpoint = endpoint(WireProtocol::OpenaiChat, "http://127.0.0.1:11434/v1");
    endpoint.model = model;
    endpoint.max_output_tokens = Some(512);
    let caps = capabilities_of(&endpoint);
    let provider = OpenAiChatProvider::new("ollama", endpoint, caps).expect("provider");

    let response = provider
        .generate(&ModelRequest::from_user(
            "Reply with the single word: ready",
        ))
        .await
        .expect("generate");
    assert!(!response.text().trim().is_empty(), "{response:?}");

    let mut acc = StreamAccumulator::new();
    let request = ModelRequest::from_user("Count from 1 to 3.");
    let mut events = provider.stream(&request);
    while let Some(event) = events.next().await {
        acc.push(event.expect("stream event"));
    }
    let streamed = acc.finish();
    assert!(!streamed.text().trim().is_empty(), "{streamed:?}");

    let mut request = tool_request();
    request.messages[0] = super::Message::user(
        "Use the file_read tool to read the file at path README.md. Do not answer in text.",
    );
    let response = provider.generate(&request).await.expect("tool call");
    assert!(
        response
            .tool_calls()
            .iter()
            .any(|call| call.name == "file_read"),
        "expected a file_read call, got {response:?}"
    );
}
