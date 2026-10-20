//! HTTP plumbing shared by the wire providers: authentication from an
//! environment-variable name, status-to-error mapping, and a small
//! server-sent-events reader over a byte stream.

use std::time::Duration;

use futures::stream::{BoxStream, StreamExt as _};

use super::{ProviderError, MAX_ERROR_BODY_BYTES};
use crate::model_endpoint::EndpointAuth;

/// Connect timeout for every provider request.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-request timeout for a non-streaming call.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// Longest single SSE line accepted, so a server cannot grow one line
/// without bound.
const MAX_SSE_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Largest `data` payload one event may join from many lines.
const MAX_SSE_EVENT_BYTES: usize = 16 * 1024 * 1024;
/// Longest silence accepted between chunks of a streamed reply. A server
/// that sends headers and then nothing would otherwise hold the stream, and
/// the policy's fallback, forever.
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// The HTTP client settings every provider uses. Redirects are refused: a
/// model API has no reason to redirect a POST, and reqwest strips only the
/// standard credential headers on a cross-host redirect, so a custom header
/// such as Anthropic's `x-api-key` would otherwise be sent to the new host.
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
}

/// Applies `auth`, reading the credential from its environment variable.
/// The value is never stored or logged.
pub(crate) fn authorize(
    builder: reqwest::RequestBuilder,
    auth: &EndpointAuth,
) -> Result<reqwest::RequestBuilder, ProviderError> {
    let read = |env: &str| {
        std::env::var(env)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ProviderError::MissingCredential {
                env: env.to_string(),
            })
    };
    Ok(match auth {
        EndpointAuth::None => builder,
        EndpointAuth::BearerEnv { env } => builder.bearer_auth(read(env)?),
        EndpointAuth::HeaderEnv { header, env } => builder.header(header.as_str(), read(env)?),
    })
}

/// Reads a JSON body. A body that is not JSON is the server's fault, not
/// the network's, so it is `InvalidResponse` (move on), never `Transport`
/// (retry the same server).
pub(crate) async fn json_body(
    endpoint: &str,
    response: reqwest::Response,
) -> Result<serde_json::Value, ProviderError> {
    let text = response
        .text()
        .await
        .map_err(|err| transport_error(endpoint, err))?;
    serde_json::from_str(&text).map_err(|err| ProviderError::InvalidResponse {
        endpoint: endpoint.to_string(),
        message: format!("body is not JSON: {err}"),
    })
}

/// Body keys the harness owns. `provider_params` may add fields but never
/// replace these, so it cannot switch the model or the conversation behind
/// the host's back (ADR-0015, ADR-0022 rule 5).
pub(crate) const RESERVED_BODY_KEYS: &[&str] = &[
    "model",
    "messages",
    "system",
    "tools",
    "stream",
    "stream_options",
    "max_tokens",
];

/// Merges `provider_params` into `body`, skipping reserved keys.
pub(crate) fn merge_provider_params(
    body: &mut serde_json::Map<String, serde_json::Value>,
    params: &serde_json::Value,
) {
    if let serde_json::Value::Object(extra) = params {
        for (key, value) in extra {
            if RESERVED_BODY_KEYS.contains(&key.as_str()) {
                tracing::warn!("provider_params may not set '{key}'; ignored");
                continue;
            }
            body.insert(key.clone(), value.clone());
        }
    }
}

/// Maps a transport-level failure.
pub(crate) fn transport_error(endpoint: &str, err: reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        ProviderError::Timeout {
            endpoint: endpoint.to_string(),
            seconds: REQUEST_TIMEOUT.as_secs(),
        }
    } else {
        ProviderError::Transport {
            endpoint: endpoint.to_string(),
            message: err.to_string(),
        }
    }
}

/// Turns a non-success response into an error, keeping a bounded body.
pub(crate) async fn error_for_status(
    endpoint: &str,
    response: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    let body = response.text().await.unwrap_or_default();
    if status.as_u16() == 429 {
        return Err(ProviderError::RateLimited {
            endpoint: endpoint.to_string(),
            retry_after,
        });
    }
    Err(ProviderError::Http {
        endpoint: endpoint.to_string(),
        status: status.as_u16(),
        body: truncate(&body, MAX_ERROR_BODY_BYTES),
    })
}

pub(crate) fn truncate(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// One server-sent event: its `event:` name (if any) and joined `data:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental SSE parser. Feed bytes, take complete events.
#[derive(Debug, Default)]
pub(crate) struct SseParser {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    /// Adds bytes and returns every event they complete.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > MAX_SSE_LINE_BYTES {
                return Err(format!("SSE line exceeds {MAX_SSE_LINE_BYTES} bytes"));
            }
            let line = String::from_utf8(line).map_err(|_| "SSE line is not UTF-8".to_string())?;
            if self.data.iter().map(String::len).sum::<usize>() + line.len() > MAX_SSE_EVENT_BYTES {
                return Err(format!("SSE event exceeds {MAX_SSE_EVENT_BYTES} bytes"));
            }
            if let Some(event) = self.line(&line) {
                events.push(event);
            }
        }
        if self.buffer.len() > MAX_SSE_LINE_BYTES {
            return Err(format!("SSE line exceeds {MAX_SSE_LINE_BYTES} bytes"));
        }
        Ok(events)
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if self.data.is_empty() {
                self.event = None;
                return None;
            }
            return Some(SseEvent {
                event: self.event.take(),
                data: std::mem::take(&mut self.data).join("\n"),
            });
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data.push(value.to_string()),
            _ => {}
        }
        None
    }

    /// Flushes a final event a server sent without a trailing blank line.
    pub(crate) fn finish(mut self) -> Option<SseEvent> {
        if !self.buffer.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.buffer)).into_owned();
            let _ = self.line(rest.trim_end_matches('\r'));
        }
        self.line("")
    }
}

/// Reads a response body as SSE events.
pub(crate) fn sse_events(
    endpoint: String,
    response: reqwest::Response,
) -> BoxStream<'static, Result<SseEvent, ProviderError>> {
    let body = response.bytes_stream();
    futures::stream::unfold(
        (
            body,
            Some(SseParser::default()),
            std::collections::VecDeque::new(),
            endpoint,
        ),
        |(mut body, mut parser, mut pending, endpoint)| async move {
            loop {
                if let Some(event) = pending.pop_front() {
                    return Some((Ok(event), (body, parser, pending, endpoint)));
                }
                let active = parser.as_mut()?;
                let next = match tokio::time::timeout(STREAM_IDLE_TIMEOUT, body.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        let err = ProviderError::Timeout {
                            endpoint: endpoint.clone(),
                            seconds: STREAM_IDLE_TIMEOUT.as_secs(),
                        };
                        return Some((Err(err), (body, None, pending, endpoint)));
                    }
                };
                match next {
                    Some(Ok(bytes)) => match active.feed(&bytes) {
                        Ok(events) => pending.extend(events),
                        Err(message) => {
                            let err = ProviderError::InvalidResponse {
                                endpoint: endpoint.clone(),
                                message,
                            };
                            return Some((Err(err), (body, None, pending, endpoint)));
                        }
                    },
                    Some(Err(err)) => {
                        let err = transport_error(&endpoint, err);
                        return Some((Err(err), (body, None, pending, endpoint)));
                    }
                    None => {
                        if let Some(event) = parser.take().and_then(SseParser::finish) {
                            pending.push_back(event);
                        }
                    }
                }
            }
        },
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_joins_data_lines_and_keeps_event_names() {
        let mut parser = SseParser::default();
        let events = parser
            .feed(b"event: message_start\ndata: {\"a\":\ndata: 1}\n\n: comment\ndata: x\r\n\r\n")
            .unwrap();
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: Some("message_start".into()),
                    data: "{\"a\":\n1}".into()
                },
                SseEvent {
                    event: None,
                    data: "x".into()
                }
            ]
        );
    }

    #[test]
    fn test_parser_handles_events_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"data: hel").unwrap().is_empty());
        assert!(parser.feed(b"lo\n").unwrap().is_empty());
        let events = parser.feed(b"\n").unwrap();
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_parser_flushes_an_unterminated_final_event() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"data: [DONE]").unwrap().is_empty());
        assert_eq!(parser.finish().unwrap().data, "[DONE]");
    }

    #[test]
    fn test_parser_caps_a_complete_oversized_line() {
        let mut parser = SseParser::default();
        let mut line = b"data: ".to_vec();
        line.extend(std::iter::repeat_n(b'x', MAX_SSE_LINE_BYTES + 1));
        line.push(b'\n');
        assert!(parser.feed(&line).unwrap_err().contains("exceeds"));
    }

    #[test]
    fn test_merge_provider_params_never_replaces_reserved_keys() {
        let mut body = serde_json::Map::new();
        body.insert("model".into(), serde_json::json!("chosen"));
        merge_provider_params(
            &mut body,
            &serde_json::json!({"model": "other", "stream": true, "top_k": 5}),
        );
        assert_eq!(body["model"], "chosen");
        assert!(body.get("stream").is_none());
        assert_eq!(body["top_k"], 5);
    }

    #[test]
    fn test_parser_rejects_non_utf8_lines() {
        let mut parser = SseParser::default();
        assert!(parser.feed(&[b'd', 0xff, b'\n']).is_err());
    }

    #[test]
    fn test_truncate_cuts_on_char_boundary() {
        assert_eq!(truncate("aé", 2), "a");
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn test_authorize_reports_a_missing_variable_by_name() {
        let client = reqwest::Client::new();
        let err = authorize(
            client.get("http://127.0.0.1:9"),
            &EndpointAuth::BearerEnv {
                env: "IMPULSE_TEST_PROVIDER_KEY_THAT_IS_NEVER_SET".into(),
            },
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("IMPULSE_TEST_PROVIDER_KEY_THAT_IS_NEVER_SET"));
        assert!(authorize(client.get("http://127.0.0.1:9"), &EndpointAuth::None).is_ok());
    }
}
