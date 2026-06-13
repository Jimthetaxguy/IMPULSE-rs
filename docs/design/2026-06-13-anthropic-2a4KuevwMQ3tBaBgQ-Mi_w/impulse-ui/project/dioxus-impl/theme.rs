//! theme.rs — Impulse Retro Broadcast design tokens, as Rust constants.
//!
//! Mirror of the values in `impulse_crt.css` so backend-driven inline styles,
//! native-island affordances (macOS menu tint, etc.), and any canvas/2D
//! rendering reference one source of truth. Keep in sync with the CSS `:root`.

/// Phosphor palette — hot, saturated, on pure black.
pub mod phosphor {
    pub const BLACK: &str = "#000000";
    pub const AMBER: &str = "#ffb01a"; // core wordmark
    pub const AMBER_HOT: &str = "#ffe39a"; // bright center
    pub const ORANGE: &str = "#ff6a00"; // edge bleed
    pub const RED: &str = "#ff3b1f"; // deep edge
    pub const BLUE: &str = "#5b63ff"; // periwinkle / structure
    pub const BLUE_HOT: &str = "#aeb4ff";
    pub const CYAN: &str = "#2fd0ff"; // live data
    pub const TEAL: &str = "#2fd6a8"; // aperture green
    pub const LIME: &str = "#b6f03c"; // healthy / OK
    pub const MAGENTA: &str = "#ff3d81"; // notable signal
    pub const YELLOW: &str = "#ffd23f"; // secondary hot
}

/// Neutral foreground ramp.
pub mod fg {
    pub const PRIMARY: &str = "#d6f3ff";
    pub const SECONDARY: &str = "#8fb8c8";
    pub const LABEL: &str = "#5d8090";
    pub const FAINT: &str = "#3a5562";
}

/// Type families.
pub mod font {
    pub const MONO: &str = "\"JetBrains Mono\", \"SFMono-Regular\", Menlo, Monaco, monospace";
    pub const BROADCAST: &str = "\"Baloo 2\", system-ui, sans-serif";
}

/// Map a structured agent status to the CSS status-dot class in impulse_crt.css.
/// Pair with `impulse_ops::AgentStatus`.
pub fn status_dot_class(status: &impulse_ops::AgentStatus) -> &'static str {
    use impulse_ops::AgentStatus::*;
    match status {
        Starting => "status-starting",
        Idle => "status-idle",
        Working { .. } => "status-working",
        Blocked { .. } => "status-blocked",
        Interrupted => "status-blocked",
        Completed => "status-completed",
    }
}

/// Short human label for an agent status (rail / chip text).
pub fn status_label(status: &impulse_ops::AgentStatus) -> &'static str {
    use impulse_ops::AgentStatus::*;
    match status {
        Starting => "starting",
        Idle => "idle",
        Working { .. } => "working",
        Blocked { .. } => "blocked",
        Interrupted => "interrupted",
        Completed => "done",
    }
}

/// Format a token count the way the hero stats render it: 47238 -> "47.2k".
pub fn format_count(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}
