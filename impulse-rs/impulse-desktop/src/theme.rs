//! Design tokens and view helpers for the Dioxus retro broadcast shell.

use impulse_ops::{AgentStatus, ArtifactStatus};

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
    pub const MONO: &str =
        "ui-monospace, \"SF Mono\", \"SFMono-Regular\", Menlo, Monaco, Consolas, monospace";
    pub const BROADCAST: &str =
        "ui-rounded, \"Avenir Next Rounded\", \"Arial Rounded MT Bold\", system-ui, sans-serif";
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

/// Render a human-readable relative age from a past unix-millis timestamp `ms`
/// against a `now_ms` reference (e.g. "just now", "5m ago", "2h ago", "3d ago").
///
/// Pure and deterministic so callers pass the current time in for testability.
/// Future timestamps (`ms > now_ms`, e.g. minor clock skew) and the sub-minute
/// bucket both collapse to "just now"; coarser buckets truncate toward the
/// nearest whole unit below.
pub fn format_relative_age(ms: i64, now_ms: i64) -> String {
    let delta_ms = now_ms.saturating_sub(ms);
    if delta_ms < 0 {
        // Timestamp is in the future (clock skew); treat as the present.
        return "just now".to_string();
    }
    let seconds = delta_ms / 1_000;
    if seconds < 60 {
        return "just now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    format!("{days}d ago")
}

/// Clamp a `0.0..=1.0` usage fraction to an integer percentage in `0..=100`.
///
/// Out-of-range inputs (negative, NaN, or `> 1.0`) are clamped so the meter
/// bar width never escapes the track.
pub fn usage_meter_pct(fraction: f32) -> i32 {
    if fraction.is_nan() {
        return 0;
    }
    (fraction.clamp(0.0, 1.0) * 100.0).round() as i32
}

/// CSS dot/badge class for an artifact lifecycle status.
pub fn artifact_status_class(status: &ArtifactStatus) -> &'static str {
    match status {
        ArtifactStatus::Ready => "status-ready",
        ArtifactStatus::Staged => "status-staged",
        ArtifactStatus::Pending => "status-pending",
        ArtifactStatus::Applied => "status-applied",
        ArtifactStatus::Acknowledged => "status-acknowledged",
    }
}

/// Short lowercase label for an artifact lifecycle status.
pub fn artifact_status_label(status: &ArtifactStatus) -> &'static str {
    match status {
        ArtifactStatus::Ready => "ready",
        ArtifactStatus::Staged => "staged",
        ArtifactStatus::Pending => "pending",
        ArtifactStatus::Applied => "applied",
        ArtifactStatus::Acknowledged => "ack",
    }
}

/// CSS class for an intervention severity. The backend stores severity as a
/// free-form string, so we fold common spellings into three calm/loud tiers
/// and default unknown values to the quietest tier (`sev-info`).
pub fn severity_class(severity: &str) -> &'static str {
    if severity.eq_ignore_ascii_case("critical")
        || severity.eq_ignore_ascii_case("block")
        || severity.eq_ignore_ascii_case("blocker")
        || severity.eq_ignore_ascii_case("error")
    {
        "sev-critical"
    } else if severity.eq_ignore_ascii_case("warn")
        || severity.eq_ignore_ascii_case("warning")
        || severity.eq_ignore_ascii_case("high")
    {
        "sev-warn"
    } else {
        "sev-info"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_meter_pct_rounds_and_clamps() {
        assert_eq!(usage_meter_pct(0.236), 24);
        assert_eq!(usage_meter_pct(0.0), 0);
        assert_eq!(usage_meter_pct(1.0), 100);
        // Out-of-range inputs are clamped, never escape the track.
        assert_eq!(usage_meter_pct(-0.5), 0);
        assert_eq!(usage_meter_pct(1.8), 100);
        assert_eq!(usage_meter_pct(f32::NAN), 0);
        // Infinities are not NaN; they fall through to clamp.
        assert_eq!(usage_meter_pct(f32::INFINITY), 100);
        assert_eq!(usage_meter_pct(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn test_format_count_boundary_at_thousand() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1.0k");
        assert_eq!(format_count(47_238), "47.2k");
    }

    #[test]
    fn test_format_relative_age_buckets() {
        const SEC: i64 = 1_000;
        const MIN: i64 = 60 * SEC;
        const HOUR: i64 = 60 * MIN;
        const DAY: i64 = 24 * HOUR;
        let now = 1_000 * DAY;
        // Sub-minute (and exactly now) collapse to "just now".
        assert_eq!(format_relative_age(now, now), "just now");
        assert_eq!(format_relative_age(now - 30 * SEC, now), "just now");
        assert_eq!(format_relative_age(now - 59 * SEC, now), "just now");
        // Minute bucket.
        assert_eq!(format_relative_age(now - MIN, now), "1m ago");
        assert_eq!(format_relative_age(now - 5 * MIN, now), "5m ago");
        assert_eq!(format_relative_age(now - 59 * MIN, now), "59m ago");
        // Hour bucket.
        assert_eq!(format_relative_age(now - HOUR, now), "1h ago");
        assert_eq!(format_relative_age(now - 2 * HOUR, now), "2h ago");
        assert_eq!(format_relative_age(now - 23 * HOUR, now), "23h ago");
        // Day bucket.
        assert_eq!(format_relative_age(now - DAY, now), "1d ago");
        assert_eq!(format_relative_age(now - 3 * DAY, now), "3d ago");
        // Truncation toward the lower whole unit.
        assert_eq!(format_relative_age(now - (90 * MIN), now), "1h ago");
        // Future timestamps (clock skew) collapse to "just now".
        assert_eq!(format_relative_age(now + 5 * MIN, now), "just now");
    }

    #[test]
    fn test_status_helpers_cover_every_agent_status() {
        let cases = [
            (AgentStatus::Starting, "status-starting", "starting"),
            (AgentStatus::Idle, "status-idle", "idle"),
            (
                AgentStatus::Working {
                    task: "build".to_string(),
                },
                "status-working",
                "working",
            ),
            (
                AgentStatus::Blocked {
                    reason: "lock".to_string(),
                },
                "status-blocked",
                "blocked",
            ),
            (AgentStatus::Interrupted, "status-blocked", "interrupted"),
            (AgentStatus::Completed, "status-completed", "done"),
        ];
        for (status, dot, label) in cases {
            assert_eq!(status_dot_class(&status), dot);
            assert_eq!(status_label(&status), label);
        }
    }

    #[test]
    fn test_artifact_status_class_covers_every_variant() {
        assert_eq!(
            artifact_status_class(&ArtifactStatus::Ready),
            "status-ready"
        );
        assert_eq!(
            artifact_status_class(&ArtifactStatus::Staged),
            "status-staged"
        );
        assert_eq!(
            artifact_status_class(&ArtifactStatus::Pending),
            "status-pending"
        );
        assert_eq!(
            artifact_status_class(&ArtifactStatus::Applied),
            "status-applied"
        );
        assert_eq!(
            artifact_status_class(&ArtifactStatus::Acknowledged),
            "status-acknowledged"
        );
    }

    #[test]
    fn test_artifact_status_label_is_compact() {
        assert_eq!(artifact_status_label(&ArtifactStatus::Acknowledged), "ack");
        assert_eq!(artifact_status_label(&ArtifactStatus::Pending), "pending");
    }

    #[test]
    fn test_severity_class_folds_spellings_and_defaults_quiet() {
        assert_eq!(severity_class("critical"), "sev-critical");
        assert_eq!(severity_class("BLOCK"), "sev-critical");
        assert_eq!(severity_class("blocker"), "sev-critical");
        assert_eq!(severity_class("error"), "sev-critical");
        assert_eq!(severity_class("warn"), "sev-warn");
        assert_eq!(severity_class("warning"), "sev-warn");
        assert_eq!(severity_class("High"), "sev-warn");
        assert_eq!(severity_class("info"), "sev-info");
        // Unknown / empty severities fall back to the quietest tier.
        assert_eq!(severity_class("whatever"), "sev-info");
        assert_eq!(severity_class(""), "sev-info");
    }
}
