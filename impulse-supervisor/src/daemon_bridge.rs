use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const DEFAULT_SOCKET_PATH: &str = ".impulse/sockets/impulse.sock";
const DEFAULT_SUPERVISOR_COMMAND: &str = "impulse-rs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeConfig {
    pub socket_path: PathBuf,
    pub supervisor_command: String,
    pub supervisor_args: Vec<String>,
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // dead_code: Pending/Connected/Error are used by upcoming bridge lifecycle loops and current unit tests.
pub enum BridgeConnectionState {
    Disconnected,
    Pending,
    Connected,
    Error(String),
}

impl BridgeConfig {
    pub fn socket_display(&self) -> String {
        self.socket_path.display().to_string()
    }
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(DEFAULT_SOCKET_PATH),
            supervisor_command: DEFAULT_SUPERVISOR_COMMAND.to_string(),
            supervisor_args: Vec::new(),
            working_dir: None,
        }
    }
}

impl BridgeConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Pending => "pending",
            Self::Connected => "connected",
            Self::Error(_) => "error",
        }
    }
}

pub fn preview_bridge_line(config: &BridgeConfig, state: &BridgeConnectionState) -> String {
    format!(
        "daemon={} @ {} via {}",
        state.label(),
        config.socket_display(),
        config.supervisor_command
    )
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
enum MirrorDaemonRequest {
    Ping,
    Status,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
enum MirrorDaemonResponse {
    Ok { result: serde_json::Value },
    Error { message: String },
}

#[allow(dead_code)] // dead_code: bridge lifecycle facade lands in loops 153-155; raw client is exercised now by unit tests.
struct RawDaemonClient {
    socket_path: PathBuf,
}

#[allow(dead_code)] // dead_code: async bridge methods are consumed by the higher-level DaemonBridge in upcoming loops.
impl RawDaemonClient {
    fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .context(format!(
                "failed to connect to socket {}",
                self.socket_path.display()
            ))
    }

    async fn send(&self, request: MirrorDaemonRequest) -> Result<MirrorDaemonResponse> {
        let stream = self.connect().await?;
        Self::send_over_stream(stream, request).await
    }

    async fn send_over_stream(
        mut stream: UnixStream,
        request: MirrorDaemonRequest,
    ) -> Result<MirrorDaemonResponse> {
        let request_json = serde_json::to_string(&request).context("serialize daemon request")?;
        stream
            .write_all(request_json.as_bytes())
            .await
            .context("write request to daemon socket")?;
        stream
            .write_all(b"\n")
            .await
            .context("write newline delimiter")?;
        stream.flush().await.context("flush daemon socket")?;

        let (reader, _) = stream.split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .context("read daemon response")?;

        serde_json::from_str(&line).context("parse daemon response")
    }

    async fn ping(&self) -> Result<bool> {
        match self.send(MirrorDaemonRequest::Ping).await? {
            MirrorDaemonResponse::Ok { result } => Ok(result
                .get("pong")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)),
            MirrorDaemonResponse::Error { message } => anyhow::bail!(message),
        }
    }

    async fn status_value(&self) -> Result<serde_json::Value> {
        match self.send(MirrorDaemonRequest::Status).await? {
            MirrorDaemonResponse::Ok { result } => Ok(result),
            MirrorDaemonResponse::Error { message } => anyhow::bail!(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    #[test]
    fn test_default_bridge_config_uses_impulse_socket() {
        let config = BridgeConfig::default();
        assert_eq!(
            config.socket_path,
            PathBuf::from(".impulse/sockets/impulse.sock")
        );
    }

    #[test]
    fn test_default_bridge_config_uses_impulse_command() {
        let config = BridgeConfig::default();
        assert_eq!(config.supervisor_command, "impulse-rs");
    }

    #[test]
    fn test_bridge_connection_state_labels() {
        assert_eq!(BridgeConnectionState::Disconnected.label(), "disconnected");
        assert_eq!(BridgeConnectionState::Pending.label(), "pending");
        assert_eq!(BridgeConnectionState::Connected.label(), "connected");
        assert_eq!(
            BridgeConnectionState::Error("socket".into()).label(),
            "error"
        );
    }

    #[test]
    fn test_preview_bridge_line_includes_socket_path() {
        let preview = preview_bridge_line(
            &BridgeConfig::default(),
            &BridgeConnectionState::Disconnected,
        );
        assert!(preview.contains(".impulse/sockets/impulse.sock"));
    }

    #[test]
    fn test_preview_bridge_line_includes_connection_label() {
        let preview =
            preview_bridge_line(&BridgeConfig::default(), &BridgeConnectionState::Pending);
        assert!(preview.contains("daemon=pending"));
    }

    async fn serve_single_response(
        stream: UnixStream,
        expected_request: &'static str,
        response_line: &'static str,
    ) {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).await.unwrap();
        assert!(request_line.contains(expected_request));
        writer.write_all(response_line.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
    }

    fn unix_stream_pair() -> (UnixStream, UnixStream) {
        let (left, right) = std::os::unix::net::UnixStream::pair().unwrap();
        left.set_nonblocking(true).unwrap();
        right.set_nonblocking(true).unwrap();
        (
            UnixStream::from_std(left).unwrap(),
            UnixStream::from_std(right).unwrap(),
        )
    }

    #[tokio::test]
    async fn test_raw_daemon_client_ping_round_trip() {
        let (client_stream, server_stream) = unix_stream_pair();
        let server = tokio::spawn(async move {
            serve_single_response(
                server_stream,
                "\"Ping\"",
                r#"{"type":"Ok","result":{"pong":true}}"#,
            )
            .await;
        });

        let result =
            match RawDaemonClient::send_over_stream(client_stream, MirrorDaemonRequest::Ping)
                .await
                .unwrap()
            {
                MirrorDaemonResponse::Ok { result } => result["pong"].as_bool().unwrap(),
                MirrorDaemonResponse::Error { message } => panic!("{message}"),
            };
        server.await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_raw_daemon_client_status_round_trip() {
        let (client_stream, server_stream) = unix_stream_pair();
        let server = tokio::spawn(async move {
            serve_single_response(
                server_stream,
                "\"Status\"",
                r#"{"type":"Ok","result":{"status":"ready","protocol_version":1}}"#,
            )
            .await;
        });

        let result =
            match RawDaemonClient::send_over_stream(client_stream, MirrorDaemonRequest::Status)
                .await
                .unwrap()
            {
                MirrorDaemonResponse::Ok { result } => result,
                MirrorDaemonResponse::Error { message } => panic!("{message}"),
            };
        server.await.unwrap();
        assert_eq!(result["status"], "ready");
        assert_eq!(result["protocol_version"], 1);
    }
}
