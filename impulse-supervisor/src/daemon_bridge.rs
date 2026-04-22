use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
