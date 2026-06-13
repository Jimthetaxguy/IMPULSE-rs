//! Design tokens and view helpers for the Dioxus retro broadcast shell.

use impulse_ops::AgentStatus;

/// Phosphor palette mirrored from `assets/impulse_crt.css`.
pub mod phosphor {
    pub const BLACK: &str = "#000000";
    pub const AMBER: &str = "#ffb01a";
    pub const AMBER_HOT: &str = "#ffe39a";
    pub const ORANGE: &str = "#ff6a00";
    pub const RED: &str = "#ff3b1f";
    pub const BLUE: &str = "#5b63ff";
    pub const BLUE_HOT: &str = "#aeb4ff";
    pub const CYAN: &str = "#2fd0ff";
    pub const TEAL: &str = "#2fd6a8";
    pub const LIME: &str = "#b6f03c";
    pub const MAGENTA: &str = "#ff3d81";
    pub const YELLOW: &str = "#ffd23f";
}

/// Neutral foreground ramp mirrored from the CSS skin.
pub mod fg {
    pub const PRIMARY: &str = "#d6f3ff";
    pub const SECONDARY: &str = "#8fb8c8";
    pub const LABEL: &str = "#5d8090";
    pub const FAINT: &str = "#3a5562";
}

/// Type families used by Dioxus and native islands.
pub mod font {
    pub const MONO: &str = "\"JetBrains Mono\", \"SFMono-Regular\", Menlo, Monaco, monospace";
    pub const BROADCAST: &str = "\"Baloo 2\", system-ui, sans-serif";
}

pub fn status_dot_class(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Starting => "status-starting",
        AgentStatus::Idle => "status-idle",
        AgentStatus::Working { .. } => "status-working",
        AgentStatus::Blocked { .. } | AgentStatus::Interrupted => "status-blocked",
        AgentStatus::Completed => "status-completed",
    }
}

pub fn status_label(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Starting => "starting",
        AgentStatus::Idle => "idle",
        AgentStatus::Working { .. } => "working",
        AgentStatus::Blocked { .. } => "blocked",
        AgentStatus::Interrupted => "interrupted",
        AgentStatus::Completed => "done",
    }
}

pub fn format_count(n: usize) -> String {
    if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
