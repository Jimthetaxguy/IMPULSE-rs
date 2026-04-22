use anyhow::{Context, Result};
use impulse_supervisor::PaneRoleRef;
use impulse_term::{PaneRole, TerminalPanel, TerminalTheme};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const DEFAULT_SOCKET_PATH: &str = ".impulse/sockets/impulse.sock";
const DEFAULT_SUPERVISOR_COMMAND: &str = "impulse-rs";
#[allow(dead_code)] // dead_code: runtime-driven supervisor PTY launch consumes this constant in loops 154-155; tests exercise it now.
const DEFAULT_SUPERVISOR_AGENT_NAME: &str = "supervisor";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonStatusSnapshot {
    pub connection_state: BridgeConnectionState,
    pub protocol_version: Option<u64>,
    pub raw_status: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // dead_code: runtime PTY launch and compaction handoff use this spec in loops 154-160; unit tests exercise it now.
pub struct SupervisorPtyLaunchSpec {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub agent_name: &'static str,
    pub pane_id: usize,
    pub socket_path: Option<PathBuf>,
    pub role_ref: PaneRoleRef,
}

#[allow(dead_code)] // dead_code: runtime PTY launch uses these helpers in loops 154-160; tests exercise them now.
impl SupervisorPtyLaunchSpec {
    pub fn pane_role(&self) -> PaneRole {
        PaneRole::Supervisor
    }

    pub fn launch_summary(&self) -> String {
        format!(
            "role={} pane={} command={} socket={}",
            self.role_ref.as_str(),
            self.pane_id,
            self.command,
            self.socket_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string())
        )
    }
}

#[allow(dead_code)] // dead_code: runtime swaps recording spawners for TerminalPanelSpawner in loops 154-155; tests exercise the trait now.
pub trait SupervisorPtySpawner {
    type Output;

    fn spawn_supervisor(&self, spec: &SupervisorPtyLaunchSpec) -> Result<Self::Output>;
}

pub struct DaemonBridge {
    config: BridgeConfig,
}

#[allow(dead_code)] // dead_code: spawn_supervisor_pty constructs this in the experimental runtime path, which lands later in the phase.
struct TerminalPanelSpawner;

impl Default for DaemonBridge {
    fn default() -> Self {
        Self::new(BridgeConfig::default())
    }
}

impl DaemonBridge {
    pub fn new(config: BridgeConfig) -> Self {
        Self { config }
    }

    pub fn preview_line(&self) -> String {
        let snapshot = self.disconnected_snapshot();
        format!(
            "{} | {}",
            preview_bridge_line(&self.config, &snapshot.connection_state),
            snapshot.status_line()
        )
    }

    pub fn disconnected_snapshot(&self) -> DaemonStatusSnapshot {
        DaemonStatusSnapshot {
            connection_state: BridgeConnectionState::Disconnected,
            protocol_version: None,
            raw_status: None,
        }
    }

    #[allow(dead_code)] // dead_code: runtime PTY orchestration consumes this launch spec in loops 154-160; tests exercise it now.
    pub fn supervisor_launch_spec(&self, pane_id: usize) -> SupervisorPtyLaunchSpec {
        SupervisorPtyLaunchSpec {
            command: self.config.supervisor_command.clone(),
            args: self.config.supervisor_args.clone(),
            working_dir: self.config.working_dir.clone(),
            agent_name: DEFAULT_SUPERVISOR_AGENT_NAME,
            pane_id,
            socket_path: Some(self.config.socket_path.clone()),
            role_ref: PaneRoleRef::Supervisor,
        }
    }

    #[allow(dead_code)] // dead_code: runtime event handlers invoke this once the Dioxus shell binds the supervisor pane.
    pub fn spawn_supervisor_pty(&self, pane_id: usize) -> Result<TerminalPanel> {
        self.spawn_supervisor_pty_with(pane_id, &TerminalPanelSpawner)
    }

    #[allow(dead_code)] // dead_code: runtime orchestration injects the real spawner later in the phase; tests exercise it now.
    pub fn spawn_supervisor_pty_with<S: SupervisorPtySpawner>(
        &self,
        pane_id: usize,
        spawner: &S,
    ) -> Result<S::Output> {
        let spec = self.supervisor_launch_spec(pane_id);
        spawner
            .spawn_supervisor(&spec)
            .context(format!("spawn supervisor pty: {}", spec.launch_summary()))
    }

    #[allow(dead_code)] // dead_code: runtime-driven daemon polling lands in loops 154-155.
    pub async fn probe_connection(&self) -> BridgeConnectionState {
        let client = RawDaemonClient::new(self.config.socket_path.clone());
        connection_state_from_ping_result(client.ping().await)
    }

    #[allow(dead_code)] // dead_code: runtime-driven daemon polling lands in loops 154-155.
    pub async fn status_snapshot(&self) -> DaemonStatusSnapshot {
        let client = RawDaemonClient::new(self.config.socket_path.clone());
        match client.status_value().await {
            Ok(raw) => status_snapshot_from_value(raw, BridgeConnectionState::Connected),
            Err(error) => DaemonStatusSnapshot {
                connection_state: BridgeConnectionState::Error(error.to_string()),
                protocol_version: None,
                raw_status: None,
            },
        }
    }
}

impl DaemonStatusSnapshot {
    pub fn status_line(&self) -> String {
        let protocol = self
            .protocol_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "?".to_string());
        format!(
            "connection={} protocol={protocol}",
            self.connection_state.label(),
        )
    }
}

impl SupervisorPtySpawner for TerminalPanelSpawner {
    type Output = TerminalPanel;

    fn spawn_supervisor(&self, spec: &SupervisorPtyLaunchSpec) -> Result<Self::Output> {
        TerminalPanel::spawn_supervisor(
            &spec.command,
            &spec.args,
            spec.working_dir.as_deref(),
            spec.agent_name,
            spec.pane_id,
            Option::<TerminalTheme>::None,
            spec.socket_path.as_deref(),
        )
        .map_err(|error| anyhow::anyhow!("failed to spawn terminal panel: {error}"))
    }
}

fn connection_state_from_ping_result(result: Result<bool>) -> BridgeConnectionState {
    match result {
        Ok(true) => BridgeConnectionState::Connected,
        Ok(false) => BridgeConnectionState::Pending,
        Err(error) => BridgeConnectionState::Error(error.to_string()),
    }
}

fn status_snapshot_from_value(
    raw: serde_json::Value,
    connection_state: BridgeConnectionState,
) -> DaemonStatusSnapshot {
    let protocol_version = raw.get("protocol_version").and_then(|value| value.as_u64());
    DaemonStatusSnapshot {
        connection_state,
        protocol_version,
        raw_status: Some(raw),
    }
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
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SpawnReceipt {
        summary: String,
        pane_role: PaneRole,
    }

    #[derive(Default)]
    struct RecordingSpawner {
        calls: Mutex<Vec<SupervisorPtyLaunchSpec>>,
    }

    impl SupervisorPtySpawner for RecordingSpawner {
        type Output = SpawnReceipt;

        fn spawn_supervisor(&self, spec: &SupervisorPtyLaunchSpec) -> Result<Self::Output> {
            self.calls
                .lock()
                .expect("recording spawner mutex should stay available")
                .push(spec.clone());
            Ok(SpawnReceipt {
                summary: spec.launch_summary(),
                pane_role: spec.pane_role(),
            })
        }
    }

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

    #[test]
    fn test_daemon_bridge_preview_line_uses_default_config() {
        let bridge = DaemonBridge::default();
        assert!(bridge
            .preview_line()
            .contains(".impulse/sockets/impulse.sock"));
    }

    #[test]
    fn test_daemon_bridge_preview_line_includes_snapshot_status() {
        let bridge = DaemonBridge::default();
        assert!(bridge.preview_line().contains("connection=disconnected"));
    }

    #[test]
    fn test_supervisor_launch_spec_uses_supervisor_role_ref() {
        let bridge = DaemonBridge::default();
        let spec = bridge.supervisor_launch_spec(7);
        assert_eq!(spec.role_ref, PaneRoleRef::Supervisor);
    }

    #[test]
    fn test_supervisor_launch_spec_maps_to_impulse_term_supervisor_role() {
        let bridge = DaemonBridge::default();
        let spec = bridge.supervisor_launch_spec(7);
        assert_eq!(spec.pane_role(), PaneRole::Supervisor);
    }

    #[test]
    fn test_supervisor_launch_spec_includes_socket_path() {
        let bridge = DaemonBridge::default();
        let spec = bridge.supervisor_launch_spec(7);
        assert_eq!(
            spec.socket_path,
            Some(PathBuf::from(".impulse/sockets/impulse.sock"))
        );
    }

    #[test]
    fn test_supervisor_launch_spec_defaults_agent_name() {
        let bridge = DaemonBridge::default();
        let spec = bridge.supervisor_launch_spec(7);
        assert_eq!(spec.agent_name, "supervisor");
    }

    #[test]
    fn test_supervisor_launch_summary_mentions_role_and_socket() {
        let bridge = DaemonBridge::default();
        let spec = bridge.supervisor_launch_spec(7);
        let summary = spec.launch_summary();
        assert!(summary.contains("role=supervisor"));
        assert!(summary.contains(".impulse/sockets/impulse.sock"));
    }

    #[test]
    fn test_spawn_supervisor_pty_with_passes_launch_spec_to_spawner() {
        let bridge = DaemonBridge::default();
        let spawner = RecordingSpawner::default();

        let receipt = bridge
            .spawn_supervisor_pty_with(42, &spawner)
            .expect("recording spawner should capture supervisor launch");

        let calls = spawner
            .calls
            .lock()
            .expect("recording spawner mutex should stay available");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].pane_id, 42);
        assert_eq!(calls[0].pane_role(), PaneRole::Supervisor);
        assert_eq!(receipt.pane_role, PaneRole::Supervisor);
    }

    #[test]
    fn test_spawn_supervisor_pty_with_returns_launch_summary() {
        let bridge = DaemonBridge::default();
        let spawner = RecordingSpawner::default();

        let receipt = bridge
            .spawn_supervisor_pty_with(7, &spawner)
            .expect("recording spawner should return a receipt");

        assert!(receipt.summary.contains("pane=7"));
        assert!(receipt.summary.contains("command=impulse-rs"));
    }

    #[test]
    fn test_connection_state_from_ping_result_connected() {
        let state = connection_state_from_ping_result(Ok(true));
        assert_eq!(state, BridgeConnectionState::Connected);
    }

    #[test]
    fn test_connection_state_from_ping_result_error() {
        let state = connection_state_from_ping_result(Err(anyhow::anyhow!("socket unavailable")));
        assert!(matches!(state, BridgeConnectionState::Error(_)));
    }

    #[test]
    fn test_status_snapshot_from_value_extracts_protocol_version() {
        let snapshot = status_snapshot_from_value(
            serde_json::json!({
                "status": "ready",
                "protocol_version": 7
            }),
            BridgeConnectionState::Connected,
        );
        assert_eq!(snapshot.protocol_version, Some(7));
        assert_eq!(snapshot.raw_status.unwrap()["status"], "ready");
    }

    #[test]
    fn test_status_snapshot_line_includes_protocol_version() {
        let snapshot = DaemonStatusSnapshot {
            connection_state: BridgeConnectionState::Connected,
            protocol_version: Some(7),
            raw_status: None,
        };
        assert_eq!(snapshot.status_line(), "connection=connected protocol=7");
    }

    #[test]
    fn test_status_snapshot_line_uses_unknown_protocol_placeholder() {
        let snapshot = DaemonStatusSnapshot {
            connection_state: BridgeConnectionState::Pending,
            protocol_version: None,
            raw_status: None,
        };
        assert_eq!(snapshot.status_line(), "connection=pending protocol=?");
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
