//! Token Tracker Metrics
//!
//! Metrics calculation and cross-platform analysis utilities

use std::collections::HashMap;

use super::types::*;

/// Cross-platform metrics analyzer
pub struct MetricsAnalyzer {
    /// All token events
    events: Vec<TokenEvent>,
    /// All compaction events
    compactions: Vec<CompactionEvent>,
    /// Compaction distances
    distances: Vec<CompactionDistance>,
}

impl MetricsAnalyzer {
    /// Create a new analyzer with data
    pub fn new(events: Vec<TokenEvent>, compactions: Vec<CompactionEvent>) -> Self {
        let distances = Self::calculate_distances(&compactions);

        Self {
            events,
            compactions,
            distances,
        }
    }

    /// Calculate distances between compactions
    fn calculate_distances(compactions: &[CompactionEvent]) -> Vec<CompactionDistance> {
        let mut sorted: Vec<&CompactionEvent> = compactions.iter().collect();
        sorted.sort_by_key(|c| c.completed_at);

        let mut distances = Vec::new();

        for window in sorted.windows(2) {
            let (earlier, later) = (window[0], window[1]);

            let time_distance = (later.completed_at - earlier.completed_at).num_seconds();
            let token_distance = earlier.tokens_after.saturating_sub(later.tokens_before);

            distances.push(CompactionDistance {
                event_id_1: earlier.id.clone(),
                event_id_2: later.id.clone(),
                time_distance_seconds: time_distance,
                token_distance,
                message_distance: 0,  // Would need event data to calculate
                stability_score: 0.0, // Would need full tracker to calculate
            });
        }

        distances
    }

    /// Calculate average distance between compactions
    pub fn avg_time_between_compactions(&self) -> f64 {
        if self.distances.is_empty() {
            return 0.0;
        }

        self.distances
            .iter()
            .map(|d| d.time_distance_seconds as f64)
            .sum::<f64>()
            / self.distances.len() as f64
    }

    /// Calculate average token processing between compactions
    pub fn avg_tokens_per_compaction_interval(&self) -> f64 {
        if self.distances.is_empty() {
            return 0.0;
        }

        self.distances
            .iter()
            .map(|d| d.token_distance as f64)
            .sum::<f64>()
            / self.distances.len() as f64
    }

    /// Get platform comparison metrics
    pub fn platform_comparison(&self) -> HashMap<Platform, PlatformMetrics> {
        let mut result = HashMap::new();

        for platform in [
            Platform::ClaudeCode,
            Platform::Codex,
            Platform::OpenCode,
            Platform::ChatGPT,
            Platform::Gemini,
        ] {
            let platform_events: Vec<&TokenEvent> = self
                .events
                .iter()
                .filter(|e| e.platform == platform)
                .collect();

            let platform_compactions: Vec<&CompactionEvent> = self
                .compactions
                .iter()
                .filter(|c| c.platform == platform)
                .collect();

            let avg_tokens = if !platform_events.is_empty() {
                platform_events
                    .iter()
                    .map(|e| e.context_tokens as f64)
                    .sum::<f64>()
                    / platform_events.len() as f64
            } else {
                0.0
            };

            let avg_usage = if !platform_events.is_empty() {
                platform_events.iter().map(|e| e.usage_ratio).sum::<f64>()
                    / platform_events.len() as f64
            } else {
                0.0
            };

            let avg_compression = if !platform_compactions.is_empty() {
                platform_compactions
                    .iter()
                    .map(|c| c.compression_ratio)
                    .sum::<f64>()
                    / platform_compactions.len() as f64
            } else {
                1.0
            };

            let auto_compaction_rate = if !platform_compactions.is_empty() {
                platform_compactions
                    .iter()
                    .filter(|c| c.is_automatic)
                    .count() as f64
                    / platform_compactions.len() as f64
            } else {
                0.0
            };

            // Calculate average time between compactions for this platform
            let platform_distances: Vec<&CompactionDistance> = self
                .distances
                .iter()
                .filter(|_d| {
                    // This is simplified - in real impl would need to link to compaction events
                    true
                })
                .collect();

            let avg_interval = if !platform_distances.is_empty() {
                platform_distances
                    .iter()
                    .map(|d| d.time_distance_seconds as f64)
                    .sum::<f64>()
                    / platform_distances.len() as f64
            } else {
                0.0
            };

            result.insert(
                platform,
                PlatformMetrics {
                    platform,
                    event_count: platform_events.len() as u64,
                    compaction_count: platform_compactions.len() as u64,
                    avg_tokens_per_event: avg_tokens,
                    avg_context_usage: avg_usage,
                    avg_compression_ratio: avg_compression,
                    auto_compaction_rate,
                    avg_time_between_compactions: avg_interval,
                },
            );
        }

        result
    }

    /// Calculate efficiency score for a platform
    pub fn efficiency_score(&self, platform: Platform) -> f64 {
        let metrics = self.platform_comparison();
        let platform_metrics = match metrics.get(&platform) {
            Some(m) => m,
            None => return 0.0,
        };

        // Efficiency = (low usage + high stability) / 2
        // Lower context usage = more efficient
        let usage_score = 1.0 - platform_metrics.avg_context_usage.min(1.0);

        // Higher time between compactions = more stable/efficient
        let stability_score = (platform_metrics.avg_time_between_compactions / 3600.0).min(1.0);

        // Higher compression = more efficient
        let compression_score = (1.0 - platform_metrics.avg_compression_ratio).min(1.0);

        // Weighted average
        usage_score * 0.4 + stability_score * 0.4 + compression_score * 0.2
    }

    /// Get historical trend for a platform
    pub fn platform_trend(&self, platform: Platform) -> TrendAnalysis {
        let platform_events: Vec<&TokenEvent> = self
            .events
            .iter()
            .filter(|e| e.platform == platform)
            .collect();

        if platform_events.len() < 2 {
            return TrendAnalysis {
                platform,
                trend_direction: TrendDirection::Stable,
                slope: 0.0,
                volatility: 0.0,
                sample_size: platform_events.len(),
            };
        }

        // Sort by timestamp
        let mut sorted = platform_events.clone();
        sorted.sort_by_key(|e| e.timestamp);

        // Calculate linear regression on usage ratio over time
        let n = sorted.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, event) in sorted.iter().enumerate() {
            let x = i as f64;
            let y = event.usage_ratio;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let slope = if (n * sum_x2 - sum_x * sum_x).abs() > f64::EPSILON {
            (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x * sum_x)
        } else {
            0.0
        };

        // Calculate volatility (standard deviation)
        let mean = sum_y / n;
        let variance = sorted
            .iter()
            .map(|e| (e.usage_ratio - mean).powi(2))
            .sum::<f64>()
            / n;
        let volatility = variance.sqrt();

        let trend_direction = if slope > 0.01 {
            TrendDirection::Increasing
        } else if slope < -0.01 {
            TrendDirection::Decreasing
        } else {
            TrendDirection::Stable
        };

        TrendAnalysis {
            platform,
            trend_direction,
            slope,
            volatility,
            sample_size: sorted.len(),
        }
    }
}

/// Platform-specific metrics
#[derive(Debug, Clone)]
pub struct PlatformMetrics {
    pub platform: Platform,
    pub event_count: u64,
    pub compaction_count: u64,
    pub avg_tokens_per_event: f64,
    pub avg_context_usage: f64,
    pub avg_compression_ratio: f64,
    pub auto_compaction_rate: f64,
    pub avg_time_between_compactions: f64,
}

/// Trend direction for platform usage
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrendDirection {
    Increasing,
    Stable,
    Decreasing,
}

/// Trend analysis result
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    pub platform: Platform,
    pub trend_direction: TrendDirection,
    /// Slope of usage ratio over time
    pub slope: f64,
    /// Volatility (standard deviation) of usage
    pub volatility: f64,
    pub sample_size: usize,
}

/// Utility function to calculate similarity between embeddings (for future semantic analysis)
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let magnitude_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}

/// Calculate token estimate from text (simple approximation)
pub fn estimate_tokens(text: &str) -> u32 {
    // Rough approximation: ~4 characters per token on average
    ((text.len() as f64) / 4.0).ceil() as u32
}

/// Calculate token estimate from messages (considering overhead)
pub fn estimate_message_tokens(message_count: u32, avg_content_tokens: u32) -> u32 {
    // Each message has ~4 token overhead (role, formatting)
    let overhead = message_count * 4;
    overhead + (message_count * avg_content_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_tracker::{CompactionEvent, CompactionType, Platform, TokenEvent};
    use chrono::Utc;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];

        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn test_token_estimation() {
        let text = "This is a test string with some words.";
        let tokens = estimate_tokens(text);
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn test_message_token_estimation() {
        let tokens = estimate_message_tokens(10, 50);
        // 10 messages * 4 overhead + 10 messages * 50 content = 540
        assert_eq!(tokens, 540);
    }

    #[test]
    fn test_platform_metrics_calculation() {
        use chrono::Duration;

        let events = vec![
            TokenEvent {
                id: "1".to_string(),
                platform: Platform::ClaudeCode,
                session_id: "s1".to_string(),
                timestamp: Utc::now(),
                context_tokens: 50000,
                max_context: 200000,
                usage_ratio: 0.25,
                message_count: 10,
                tool_call_count: 20,
            },
            TokenEvent {
                id: "2".to_string(),
                platform: Platform::ClaudeCode,
                session_id: "s1".to_string(),
                timestamp: Utc::now(),
                context_tokens: 100000,
                max_context: 200000,
                usage_ratio: 0.5,
                message_count: 20,
                tool_call_count: 40,
            },
        ];

        let compactions = vec![CompactionEvent {
            id: "c1".to_string(),
            platform: Platform::ClaudeCode,
            session_id: "s1".to_string(),
            started_at: Utc::now(),
            completed_at: Utc::now() + Duration::seconds(5),
            duration_ms: 5000,
            tokens_before: 150000,
            tokens_after: 80000,
            compression_ratio: 0.53,
            compaction_type: CompactionType::Summarize,
            is_automatic: true,
        }];

        let analyzer = MetricsAnalyzer::new(events, compactions);

        let comparison = analyzer.platform_comparison();
        let claude_metrics = comparison.get(&Platform::ClaudeCode).unwrap();

        assert_eq!(claude_metrics.event_count, 2);
        assert_eq!(claude_metrics.compaction_count, 1);
    }

    #[test]
    fn test_efficiency_score() {
        let events = vec![TokenEvent {
            id: "1".to_string(),
            platform: Platform::ClaudeCode,
            session_id: "s1".to_string(),
            timestamp: Utc::now(),
            context_tokens: 30000,
            max_context: 200000,
            usage_ratio: 0.15,
            message_count: 5,
            tool_call_count: 10,
        }];

        let compactions = vec![];

        let analyzer = MetricsAnalyzer::new(events, compactions);
        let efficiency = analyzer.efficiency_score(Platform::ClaudeCode);

        // Efficiency = (low usage + high stability) / 2
        // Usage score = 1 - 0.15 = 0.85
        // With no compactions, stability defaults to 0
        // Compression score = 0 (no compactions)
        // Efficiency = 0.85 * 0.4 + 0 * 0.4 + 0 * 0.2 = 0.34
        assert!(efficiency > 0.3);
    }
}
