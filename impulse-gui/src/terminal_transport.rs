use std::env;
use std::path::Path;

use impulse_ops::{MachineTarget, TerminalOwnership, TerminalTransportKind};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub command: String,
    pub args: Vec<String>,
    pub ownership: TerminalOwnership,
    pub warning: Option<LaunchFallbackWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LaunchFallbackWarning {
    #[error("IMPULSE_TERMINAL_TRANSPORT={value} is unsupported; using direct PTY ownership.")]
    UnsupportedTransport { value: String },
    #[error(
        "IMPULSE_TERMINAL_TRANSPORT=tmux requested, but tmux is not installed; using direct PTY ownership."
    )]
    TmuxUnavailable,
    #[error(
        "IMPULSE_TERMINAL_TRANSPORT=smux requested, but IMPULSE_SMUX_LAUNCHER is unset; using direct PTY ownership."
    )]
    MissingSmuxLauncher,
    #[error(
        "IMPULSE_SMUX_LAUNCHER must include both {{session}} and {{command}} placeholders; using direct PTY ownership."
    )]
    InvalidSmuxTemplate,
}

pub fn build_launch_plan(
    agent_name: &str,
    command: &str,
    args: &[&str],
    target_dir: &Path,
) -> LaunchPlan {
    let requested = requested_transport();
    let workspace_key = workspace_key(target_dir);
    let session_name = format!("{}-{}", workspace_key, sanitize_transport_token(agent_name));

    match requested.kind {
        TerminalTransportKind::Direct => {
            direct_launch_plan(command, args, &workspace_key).with_warning(requested.warning)
        }
        TerminalTransportKind::Tmux => {
            build_tmux_launch_plan(command, args, &workspace_key, &session_name)
                .with_warning(requested.warning)
        }
        TerminalTransportKind::Smux => {
            build_smux_launch_plan(command, args, &workspace_key, &session_name)
                .with_warning(requested.warning)
        }
    }
}

pub fn infer_ownership<'a, I>(base: &TerminalOwnership, remote_connections: I) -> TerminalOwnership
where
    I: IntoIterator<Item = &'a str>,
{
    let mut ownership = base.clone();
    for line in remote_connections {
        let lower = line.to_ascii_lowercase();
        if ownership.transport == TerminalTransportKind::Direct {
            if let Some(session_name) = parse_tmux_session_name(line) {
                ownership.transport = TerminalTransportKind::Tmux;
                ownership.owner_key = format!("tmux:{}", session_name);
                ownership.session_name = Some(session_name);
                ownership.note = Some("detected tmux session from terminal output".to_string());
            } else if lower.contains("smux") {
                ownership.transport = TerminalTransportKind::Smux;
                ownership.owner_key = "smux:detected".to_string();
                ownership.note = Some("detected smux launcher from terminal output".to_string());
            }
        }
    }
    ownership
}

pub fn infer_machine_target<'a, I>(
    target_dir: &Path,
    ownership: &TerminalOwnership,
    remote_connections: I,
) -> MachineTarget
where
    I: IntoIterator<Item = &'a str>,
{
    for line in remote_connections {
        if let Some((user, host)) = parse_ssh_target(line) {
            return MachineTarget::Remote {
                user,
                host,
                workdir: target_dir.display().to_string(),
                session_name: ownership.session_name.clone(),
            };
        }
    }

    MachineTarget::Local {
        workdir: target_dir.display().to_string(),
    }
}

pub fn parse_ssh_target(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let target = trimmed
        .strip_prefix("ssh ")?
        .split_whitespace()
        .find(|token| !token.starts_with('-') && token.contains('@'))?
        .trim_matches(&['"', '\''][..]);
    let (user, host) = target.split_once('@')?;
    if user.is_empty() || host.is_empty() {
        return None;
    }
    Some((user.to_string(), host.to_string()))
}

pub fn parse_tmux_session_name(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for window in tokens.windows(2) {
        if matches!(window[0], "-s" | "-t") && !window[1].starts_with('-') {
            return Some(window[1].trim_matches(&['"', '\''][..]).to_string());
        }
    }
    None
}

fn requested_transport() -> RequestedTransport {
    match env::var("IMPULSE_TERMINAL_TRANSPORT") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "direct" | "" => RequestedTransport::new(TerminalTransportKind::Direct),
            "tmux" => RequestedTransport::new(TerminalTransportKind::Tmux),
            "smux" => RequestedTransport::new(TerminalTransportKind::Smux),
            other => RequestedTransport {
                kind: TerminalTransportKind::Direct,
                warning: Some(LaunchFallbackWarning::UnsupportedTransport {
                    value: other.to_string(),
                }),
            },
        },
        Err(_) => RequestedTransport::new(TerminalTransportKind::Direct),
    }
}

fn direct_launch_plan(command: &str, args: &[&str], workspace_key: &str) -> LaunchPlan {
    LaunchPlan {
        command: command.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        ownership: direct_ownership(workspace_key),
        warning: None,
    }
}

fn build_tmux_launch_plan(
    command: &str,
    args: &[&str],
    workspace_key: &str,
    session_name: &str,
) -> LaunchPlan {
    if which::which("tmux").is_err() {
        return LaunchPlan {
            warning: Some(LaunchFallbackWarning::TmuxUnavailable),
            ..direct_launch_plan(command, args, workspace_key)
        };
    }

    let quoted_command = shell_join(command, args);
    let tmux_script = format!(
        "tmux new-session -A -s {} /bin/sh -lc {}",
        shell_quote(session_name),
        shell_quote(&quoted_command)
    );

    LaunchPlan {
        command: "/bin/sh".to_string(),
        args: vec!["-lc".to_string(), tmux_script],
        ownership: TerminalOwnership {
            transport: TerminalTransportKind::Tmux,
            owner_key: format!("tmux:{}", session_name),
            workspace_key: workspace_key.to_string(),
            session_name: Some(session_name.to_string()),
            note: Some("launched via tmux session wrapper".to_string()),
        },
        warning: None,
    }
}

fn build_smux_launch_plan(
    command: &str,
    args: &[&str],
    workspace_key: &str,
    session_name: &str,
) -> LaunchPlan {
    let Some(template) = env::var("IMPULSE_SMUX_LAUNCHER").ok() else {
        return LaunchPlan {
            warning: Some(LaunchFallbackWarning::MissingSmuxLauncher),
            ..direct_launch_plan(command, args, workspace_key)
        };
    };

    if !template.contains("{session}") || !template.contains("{command}") {
        return LaunchPlan {
            warning: Some(LaunchFallbackWarning::InvalidSmuxTemplate),
            ..direct_launch_plan(command, args, workspace_key)
        };
    }

    let joined = shell_join(command, args);
    let launcher = template
        .replace("{session}", &shell_quote(session_name))
        .replace("{command}", &shell_quote(&joined));

    LaunchPlan {
        command: "/bin/sh".to_string(),
        args: vec!["-lc".to_string(), launcher],
        ownership: TerminalOwnership {
            transport: TerminalTransportKind::Smux,
            owner_key: format!("smux:{}", session_name),
            workspace_key: workspace_key.to_string(),
            session_name: Some(session_name.to_string()),
            note: Some("launched via configured smux launcher".to_string()),
        },
        warning: None,
    }
}

fn direct_ownership(workspace_key: &str) -> TerminalOwnership {
    TerminalOwnership {
        transport: TerminalTransportKind::Direct,
        owner_key: format!("direct:{}", workspace_key),
        workspace_key: workspace_key.to_string(),
        session_name: None,
        note: None,
    }
}

fn workspace_key(target_dir: &Path) -> String {
    sanitize_transport_token(
        target_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace"),
    )
}

fn sanitize_transport_token(input: &str) -> String {
    let normalized = input
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>();
    let collapsed = normalized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "workspace".to_string()
    } else {
        collapsed
    }
}

fn shell_join(command: &str, args: &[&str]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_quote(command));
    parts.extend(args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

struct RequestedTransport {
    kind: TerminalTransportKind,
    warning: Option<LaunchFallbackWarning>,
}

impl RequestedTransport {
    fn new(kind: TerminalTransportKind) -> Self {
        Self {
            kind,
            warning: None,
        }
    }
}

impl LaunchPlan {
    fn with_warning(mut self, warning: Option<LaunchFallbackWarning>) -> Self {
        if self.warning.is_none() {
            self.warning = warning;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = env::var(key).ok();
            env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                env::set_var(self.key, value);
            } else {
                env::remove_var(self.key);
            }
        }
    }

    fn demo_dir() -> TempDir {
        TempDir::new().expect("temp dir")
    }

    #[test]
    fn test_parse_ssh_target_returns_remote_tuple() {
        assert_eq!(
            parse_ssh_target("ssh james@example.com"),
            Some(("james".to_string(), "example.com".to_string()))
        );
    }

    #[test]
    fn test_parse_ssh_target_skips_flags_and_keeps_destination() {
        assert_eq!(
            parse_ssh_target("ssh -A -t james@example.com tmux attach -t demo"),
            Some(("james".to_string(), "example.com".to_string()))
        );
    }

    #[test]
    fn test_parse_tmux_session_name_extracts_session() {
        assert_eq!(
            parse_tmux_session_name("tmux new-session -A -s demo-shell /bin/sh"),
            Some("demo-shell".to_string())
        );
    }

    #[test]
    fn test_infer_machine_target_prefers_remote_connection() {
        let ownership = TerminalOwnership {
            session_name: Some("demo".to_string()),
            ..direct_ownership("workspace")
        };
        let target = infer_machine_target(
            Path::new("/tmp/project"),
            &ownership,
            ["ssh james@example.com"],
        );
        assert_eq!(
            target,
            MachineTarget::Remote {
                user: "james".to_string(),
                host: "example.com".to_string(),
                workdir: "/tmp/project".to_string(),
                session_name: Some("demo".to_string()),
            }
        );
    }

    #[test]
    fn test_infer_ownership_promotes_tmux_from_remote_connection() {
        let base = direct_ownership("workspace");
        let ownership = infer_ownership(&base, ["tmux new-session -A -s demo-shell /bin/sh"]);
        assert_eq!(ownership.transport, TerminalTransportKind::Tmux);
        assert_eq!(ownership.session_name.as_deref(), Some("demo-shell"));
    }

    #[test]
    fn test_unknown_transport_falls_back_to_direct_with_warning() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _transport = EnvVarGuard::set("IMPULSE_TERMINAL_TRANSPORT", "weird");
        let dir = demo_dir();
        let plan = build_launch_plan("Shell", "bash", &["-lc", "echo hi"], dir.path());
        assert_eq!(plan.ownership.transport, TerminalTransportKind::Direct);
        assert_eq!(
            plan.warning,
            Some(LaunchFallbackWarning::UnsupportedTransport {
                value: "weird".to_string()
            })
        );
    }

    #[test]
    fn test_smux_template_requires_placeholders() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _transport = EnvVarGuard::set("IMPULSE_TERMINAL_TRANSPORT", "smux");
        let _launcher = EnvVarGuard::set("IMPULSE_SMUX_LAUNCHER", "smux attach demo");
        let dir = demo_dir();
        let plan = build_launch_plan("Shell", "bash", &["-lc", "echo hi"], dir.path());
        assert_eq!(plan.ownership.transport, TerminalTransportKind::Direct);
        assert_eq!(
            plan.warning,
            Some(LaunchFallbackWarning::InvalidSmuxTemplate)
        );
    }

    #[test]
    fn test_build_smux_launch_plan_uses_template() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _transport = EnvVarGuard::set("IMPULSE_TERMINAL_TRANSPORT", "smux");
        let _launcher = EnvVarGuard::set(
            "IMPULSE_SMUX_LAUNCHER",
            "smux attach {session} --command {command}",
        );
        let dir = demo_dir();
        let plan = build_launch_plan("Shell", "bash", &["-lc", "echo hi"], dir.path());
        assert_eq!(plan.command, "/bin/sh");
        assert_eq!(plan.ownership.transport, TerminalTransportKind::Smux);
        assert!(
            plan.args[1].contains("smux attach"),
            "expected smux command, got {}",
            plan.args[1]
        );
        assert!(plan.warning.is_none());
    }

    #[test]
    fn test_tmux_transport_falls_back_when_binary_missing_or_wraps_when_available() {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _transport = EnvVarGuard::set("IMPULSE_TERMINAL_TRANSPORT", "tmux");
        let _launcher = EnvVarGuard::remove("IMPULSE_SMUX_LAUNCHER");
        let dir = demo_dir();
        let plan = build_launch_plan("Shell", "bash", &["-lc", "echo hi"], dir.path());
        if which::which("tmux").is_ok() {
            assert_eq!(plan.ownership.transport, TerminalTransportKind::Tmux);
            assert!(plan.warning.is_none());
        } else {
            assert_eq!(plan.ownership.transport, TerminalTransportKind::Direct);
            assert_eq!(plan.warning, Some(LaunchFallbackWarning::TmuxUnavailable));
        }
    }
}
