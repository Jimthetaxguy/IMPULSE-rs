//! DataFusion analytics types
//!
//! Provides type definitions for session analytics.
//! Future: `#[cfg(feature = "datafusion-support")]` enables Apache Arrow DataFusion queries.

use serde::{Deserialize, Serialize};

/// Session analytics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalytics {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_files_tracked: usize,
    pub total_tools_used: usize,
    pub average_session_duration_minutes: f64,
    pub most_active_platform: String,
}

/// Activity trend data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendPoint {
    pub timestamp: String,
    pub value: f64,
    pub label: Option<String>,
}

/// Aggregation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub group_by: String,
    pub count: usize,
    pub percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_analytics_serialization() {
        let analytics = SessionAnalytics {
            total_sessions: 10,
            active_sessions: 2,
            total_files_tracked: 50,
            total_tools_used: 30,
            average_session_duration_minutes: 45.0,
            most_active_platform: "claude-code".to_string(),
        };
        let json = serde_json::to_string(&analytics).unwrap();
        assert!(json.contains("claude-code"));
    }

    #[test]
    fn test_trend_point() {
        let point = TrendPoint {
            timestamp: "2026-02-24T12:00:00Z".to_string(),
            value: 5.0,
            label: Some("daily".to_string()),
        };
        assert_eq!(point.value, 5.0);
    }
}
