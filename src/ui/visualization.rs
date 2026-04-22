//! Visualization helpers for TUI - charts, sparklines, gauges, and enhanced displays

use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Generate a sparkline from a series of data points
/// Returns ASCII representation suitable for terminal display
pub fn sparkline(data: &[f64], width: usize) -> String {
    if data.is_empty() || width == 0 {
        return String::new();
    }

    let min = data.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max = data.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let range = max - min;

    let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let step = if range == 0.0 {
        0.0
    } else {
        range / (chars.len() - 1) as f64
    };

    let points_per_char = (data.len() as f64 / width as f64).max(1.0);

    let mut result = String::new();
    for i in 0..width {
        let start = (i as f64 * points_per_char) as usize;
        let end = ((i + 1) as f64 * points_per_char) as usize;
        let slice = &data[start..end.min(data.len())];

        let avg = if slice.is_empty() {
            0.0
        } else {
            slice.iter().sum::<f64>() / slice.len() as f64
        };

        let idx = if range == 0.0 {
            chars.len() / 2
        } else {
            ((avg - min) / step).round() as usize
        }
        .min(chars.len() - 1);

        result.push(chars[idx]);
    }

    result
}

/// Generate a horizontal bar chart
pub fn horizontal_bar(value: f64, max: f64, width: usize) -> String {
    if width == 0 || max == 0.0 {
        return "░".repeat(width);
    }

    let filled = ((value / max) * width as f64).round() as usize;
    let filled = filled.min(width);

    let filled_str = "█".repeat(filled);
    let empty_str = "░".repeat(width - filled);

    format!("{}{}", filled_str, empty_str)
}

/// Generate a vertical bar for use in column charts
pub fn vertical_bar(height: usize, max_height: usize) -> String {
    if max_height == 0 || height == 0 {
        return " ".to_string();
    }

    let ratio = height as f64 / max_height as f64;
    let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let idx = ((ratio * (chars.len() - 1) as f64) as usize).min(chars.len() - 1);

    chars[idx].to_string()
}

/// Generate a gauge (0-100%)
pub fn gauge(percentage: f64, width: usize) -> String {
    let pct = percentage.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);

    let filled_str = "█".repeat(filled);
    let empty_str = "░".repeat(width - filled);

    format!("[{}{}] {}%", filled_str, empty_str, pct as i32)
}

/// Format duration in human-readable form
pub fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    }
}

/// Format bytes to human-readable form
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

/// Truncate text with ellipsis. UTF-8 safe — never slices mid-character.
pub fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else if max_len > 3 {
        let mut end = max_len - 3;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &text[..end])
    } else {
        let mut end = max_len;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_string()
    }
}

/// Pad text to specified width
pub fn pad(text: &str, width: usize, align: Align) -> String {
    let len = text.len();
    if len >= width {
        return text.to_string();
    }

    let padding = width - len;
    match align {
        Align::Left => format!("{}{}", text, " ".repeat(padding)),
        Align::Right => format!("{}{}", " ".repeat(padding), text),
        Align::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), text, " ".repeat(right))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// Session activity data for visualization
#[derive(Debug, Clone)]
pub struct SessionActivity {
    pub session_id: String,
    pub session_name: String,
    pub platform: Option<String>,
    pub duration_secs: i64,
    pub file_count: usize,
    pub tool_count: usize,
    pub timestamp: DateTime<Utc>,
}

impl SessionActivity {
    pub fn from_session(
        id: String,
        name: String,
        platform: Option<String>,
        created: DateTime<Utc>,
        last_activity: DateTime<Utc>,
        files: &[String],
        tools: &[String],
    ) -> Self {
        let duration_secs = (last_activity - created).num_seconds();
        Self {
            session_id: id,
            session_name: name,
            platform,
            duration_secs,
            file_count: files.len(),
            tool_count: tools.len(),
            timestamp: created,
        }
    }
}

/// Analytics summary for display
#[derive(Debug, Clone, Default)]
pub struct AnalyticsSummary {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_duration_secs: i64,
    pub total_files: usize,
    pub total_tools: usize,
    pub avg_duration_secs: i64,
    pub platform_breakdown: HashMap<String, usize>,
    pub daily_activity: Vec<DailyActivity>,
}

#[derive(Debug, Clone, Default)]
pub struct DailyActivity {
    pub date: String,
    pub session_count: usize,
    pub total_duration_secs: i64,
    pub file_count: usize,
}

/// Calculate analytics from history entries
pub fn calculate_analytics(history: &[crate::state::HistoryEntry]) -> AnalyticsSummary {
    use crate::state::Platform;

    let mut summary = AnalyticsSummary {
        total_sessions: history.len(),
        ..Default::default()
    };

    let mut platform_counts: HashMap<String, usize> = HashMap::new();
    let mut daily_map: HashMap<String, DailyActivity> = HashMap::new();
    let mut total_duration = 0i64;

    for entry in history {
        // Platform breakdown
        let platform_str = match entry.platform {
            Some(Platform::ClaudeCode) => "ClaudeCode",
            Some(Platform::OpenCode) => "OpenCode",
            None => "Unknown",
        };
        *platform_counts.entry(platform_str.to_string()).or_insert(0) += 1;

        // Duration
        let duration = (entry.ended_at - entry.started_at).num_seconds();
        total_duration += duration;

        // Daily activity
        let date = entry.started_at.format("%Y-%m-%d").to_string();
        let daily = daily_map
            .entry(date.clone())
            .or_insert_with(|| DailyActivity {
                date,
                session_count: 0,
                total_duration_secs: 0,
                file_count: 0,
            });
        daily.session_count += 1;
        daily.total_duration_secs += duration;
        daily.file_count += entry.files_touched.len();
    }

    summary.platform_breakdown = platform_counts;
    summary.daily_activity = daily_map.into_values().collect();
    summary.daily_activity.sort_by(|a, b| a.date.cmp(&b.date));

    summary.total_duration_secs = total_duration;
    summary.avg_duration_secs = if history.is_empty() {
        0
    } else {
        total_duration / history.len() as i64
    };

    summary
}

/// Search result for sessions
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub session_id: String,
    pub session_name: String,
    pub match_type: MatchType,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchType {
    File,
    Tool,
    Summary,
    Name,
}

/// Search across sessions
pub fn search_sessions(
    query: &str,
    sessions: &[crate::state::Session],
    history: &[crate::state::HistoryEntry],
) -> Vec<SearchResult> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Search active sessions
    for session in sessions {
        let mut score = 0.0;
        let mut match_type = MatchType::Name;
        let mut snippet = String::new();

        // Name match
        if session.name.to_lowercase().contains(&query_lower) {
            score += 10.0;
            match_type = MatchType::Name;
            snippet = session.name.clone();
        }

        // File match
        for file in &session.active_files {
            if file.to_lowercase().contains(&query_lower) {
                score += 5.0;
                match_type = MatchType::File;
                snippet = file.clone();
                break;
            }
        }

        // Tool match
        for tool in &session.recent_tools {
            if tool.to_lowercase().contains(&query_lower) {
                score += 3.0;
                if match_type != MatchType::File {
                    match_type = MatchType::Tool;
                    snippet = tool.clone();
                }
            }
        }

        if score > 0.0 {
            results.push(SearchResult {
                session_id: session.id.clone(),
                session_name: session.name.clone(),
                match_type,
                snippet,
                score,
            });
        }
    }

    // Search history
    for entry in history {
        let mut score = 0.0;
        let mut match_type = MatchType::Summary;
        let mut snippet = String::new();

        // Name match
        if entry.session_name.to_lowercase().contains(&query_lower) {
            score += 10.0;
            match_type = MatchType::Name;
            snippet = entry.session_name.clone();
        }

        // Summary match
        if entry.summary.to_lowercase().contains(&query_lower) {
            score += 8.0;
            if match_type == MatchType::Summary {
                snippet = truncate(&entry.summary, 50);
            }
        }

        // File match
        for file in &entry.files_touched {
            if file.to_lowercase().contains(&query_lower) {
                score += 5.0;
                match_type = MatchType::File;
                snippet = file.clone();
                break;
            }
        }

        if score > 0.0 {
            results.push(SearchResult {
                session_id: entry.session_id.clone(),
                session_name: entry.session_name.clone(),
                match_type,
                snippet,
                score,
            });
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(20); // Limit results

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparkline() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let result = sparkline(&data, 8);
        // Sparkline should produce approximately width chars
        assert!(result.len() >= 8);
    }

    #[test]
    fn test_horizontal_bar() {
        let result = horizontal_bar(75.0, 100.0, 10);
        // Should be at least width chars
        assert!(result.len() >= 10);
        assert!(result.starts_with("██")); // Contains filled chars
    }

    #[test]
    fn test_gauge() {
        let result = gauge(50.0, 10);
        assert!(result.contains("50%"));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        // Duration formatting may vary slightly
        let result90 = format_duration(90);
        assert!(result90.contains("m"));
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_utf8_safe() {
        // 2-byte chars: "café" — 'é' is 2 bytes at offset 3-4.
        let result = truncate("café latte", 7);
        assert!(!result.is_empty());
        // Should not panic on multi-byte boundary.

        // 4-byte chars: emoji.
        let emoji = "hello 🌍 world";
        let result = truncate(emoji, 8);
        assert!(result.ends_with("..."));

        // Very short max_len (≤3 path).
        let result = truncate("café", 2);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_pad() {
        assert_eq!(pad("hi", 5, Align::Left), "hi   ");
        assert_eq!(pad("hi", 5, Align::Right), "   hi");
        assert_eq!(pad("hi", 5, Align::Center), " hi  ");
    }
}
