//! Token Tracking Algorithm
//!
//! Dynamic algorithm for tracking token usage and measuring autocompaction events

use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

use super::types::*;

/// Token Tracker - Core algorithm for tracking token usage and compaction events
pub struct TokenTracker {
    /// Current token events
    events: Vec<TokenEvent>,
    /// Compaction events
    compactions: Vec<CompactionEvent>,
    /// Token budget configuration
    budget: TokenBudget,
    /// Confidence decay configuration
    confidence_config: ConfidenceDecayConfig,
    /// Platform configurations
    platform_configs: HashMap<Platform, PlatformConfig>,
    /// Session to platform mapping
    session_platforms: HashMap<String, Platform>,
}

impl TokenTracker {
    /// Create a new TokenTracker with default configuration
    pub fn new() -> Self {
        let mut platform_configs = HashMap::new();
        for platform in [
            Platform::ClaudeCode,
            Platform::Codex,
            Platform::OpenCode,
            Platform::ChatGPT,
            Platform::Gemini,
        ] {
            platform_configs.insert(platform, PlatformConfig::for_platform(platform));
        }

        Self {
            events: Vec::new(),
            compactions: Vec::new(),
            budget: TokenBudget::default(),
            confidence_config: ConfidenceDecayConfig::default(),
            platform_configs,
            session_platforms: HashMap::new(),
        }
    }

    /// Create with custom token budget
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        self.budget = budget;
        self
    }

    /// Record a token event
    pub fn record_event(
        &mut self,
        platform: Platform,
        session_id: &str,
        context_tokens: u32,
        max_context: u32,
        message_count: u32,
        tool_call_count: u32,
    ) {
        let usage_ratio = if max_context > 0 {
            context_tokens as f64 / max_context as f64
        } else {
            0.0
        };

        let event = TokenEvent {
            id: Uuid::new_v4().to_string(),
            platform,
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
            context_tokens,
            max_context,
            usage_ratio,
            message_count,
            tool_call_count,
        };

        self.session_platforms
            .insert(session_id.to_string(), platform);
        self.events.push(event);
    }

    /// Record a compaction event.
    pub fn record_compaction(&mut self, record: CompactionRecord) {
        let duration_ms = (record.completed_at - record.started_at).num_milliseconds() as u64;
        let compression_ratio = if record.tokens_before > 0 {
            record.tokens_after as f64 / record.tokens_before as f64
        } else {
            1.0
        };

        let event = CompactionEvent {
            id: Uuid::new_v4().to_string(),
            platform: record.platform,
            session_id: record.session_id,
            started_at: record.started_at,
            completed_at: record.completed_at,
            duration_ms,
            tokens_before: record.tokens_before,
            tokens_after: record.tokens_after,
            compression_ratio,
            compaction_type: record.compaction_type,
            is_automatic: record.is_automatic,
        };

        self.compactions.push(event);
    }

    /// Get the appropriate token budget based on current usage
    pub fn get_token_budget(&self, usage_ratio: f64) -> u32 {
        if usage_ratio < self.budget.soft_threshold {
            self.budget.normal_budget
        } else if usage_ratio < self.budget.hard_threshold {
            self.budget.aggressive_budget
        } else {
            self.budget.micro_budget
        }
    }

    /// Calculate confidence decay over time
    pub fn calculate_confidence_decay(&self, initial_confidence: f64, minutes_since: f64) -> f64 {
        initial_confidence * (-self.confidence_config.decay_rate * minutes_since).exp()
    }

    /// Get events for a specific session
    pub fn get_session_events(&self, session_id: &str) -> Vec<&TokenEvent> {
        self.events
            .iter()
            .filter(|e| e.session_id == session_id)
            .collect()
    }

    /// Get compactions for a specific session
    pub fn get_session_compactions(&self, session_id: &str) -> Vec<&CompactionEvent> {
        self.compactions
            .iter()
            .filter(|c| c.session_id == session_id)
            .collect()
    }

    /// Calculate distance between consecutive compactions
    pub fn calculate_compaction_distances(&self) -> Vec<CompactionDistance> {
        let mut distances = Vec::new();

        // Sort compactions by timestamp
        let mut sorted: Vec<&CompactionEvent> = self.compactions.iter().collect();
        sorted.sort_by_key(|c| c.completed_at);

        for window in sorted.windows(2) {
            let (earlier, later) = (window[0], window[1]);

            let time_distance = (later.completed_at - earlier.completed_at).num_seconds();
            let token_distance = earlier.tokens_after.saturating_sub(later.tokens_before);

            // Count messages between events
            let message_distance = self
                .events
                .iter()
                .filter(|e| {
                    e.session_id == earlier.session_id
                        && e.timestamp > earlier.completed_at
                        && e.timestamp < later.completed_at
                })
                .count() as u32;

            // Calculate stability score (0.0 - 1.0)
            // Higher score = more stable = longer time between compactions
            let stability_score =
                self.calculate_stability_score(time_distance, token_distance, message_distance);

            distances.push(CompactionDistance {
                event_id_1: earlier.id.clone(),
                event_id_2: later.id.clone(),
                time_distance_seconds: time_distance,
                token_distance,
                message_distance,
                stability_score,
            });
        }

        distances
    }

    /// Calculate stability score based on distances
    fn calculate_stability_score(
        &self,
        time_seconds: i64,
        token_distance: u32,
        message_distance: u32,
    ) -> f64 {
        // Normalize time: 1 hour = 1.0
        let time_score = (time_seconds as f64 / 3600.0).min(1.0);

        // Token distance: 10000 tokens = 1.0
        let token_score = (token_distance as f64 / 10000.0).min(1.0);

        // Message distance: 50 messages = 1.0
        let message_score = (message_distance as f64 / 50.0).min(1.0);

        // Weighted average: time is most important
        (time_score * 0.5 + token_score * 0.3 + message_score * 0.2).min(1.0)
    }

    /// Get platform statistics
    pub fn get_platform_stats(&self, platform: Platform) -> HashMap<String, f64> {
        let mut stats = HashMap::new();

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

        if !platform_events.is_empty() {
            let avg_tokens: f64 = platform_events
                .iter()
                .map(|e| e.context_tokens as f64)
                .sum::<f64>()
                / platform_events.len() as f64;
            stats.insert("avg_tokens".to_string(), avg_tokens);

            let avg_usage: f64 = platform_events.iter().map(|e| e.usage_ratio).sum::<f64>()
                / platform_events.len() as f64;
            stats.insert("avg_usage_ratio".to_string(), avg_usage);
        }

        stats.insert("event_count".to_string(), platform_events.len() as f64);
        stats.insert(
            "compaction_count".to_string(),
            platform_compactions.len() as f64,
        );

        if !platform_compactions.is_empty() {
            let avg_compression: f64 = platform_compactions
                .iter()
                .map(|c| c.compression_ratio)
                .sum::<f64>()
                / platform_compactions.len() as f64;
            stats.insert("avg_compression".to_string(), avg_compression);

            let avg_duration: f64 = platform_compactions
                .iter()
                .map(|c| c.duration_ms as f64)
                .sum::<f64>()
                / platform_compactions.len() as f64;
            stats.insert("avg_compaction_duration_ms".to_string(), avg_duration);
        }

        stats
    }

    /// Calculate overall statistics
    pub fn get_statistics(&self) -> TokenTrackerStats {
        let total_events = self.events.len() as u64;
        let total_compactions = self.compactions.len() as u64;

        let avg_tokens = if total_events > 0 {
            self.events
                .iter()
                .map(|e| e.context_tokens as f64)
                .sum::<f64>()
                / total_events as f64
        } else {
            0.0
        };

        let avg_interval = if total_compactions > 1 {
            let distances = self.calculate_compaction_distances();
            if !distances.is_empty() {
                distances
                    .iter()
                    .map(|d| d.time_distance_seconds as f64)
                    .sum::<f64>()
                    / distances.len() as f64
            } else {
                0.0
            }
        } else {
            0.0
        };

        let avg_compression = if total_compactions > 0 {
            self.compactions
                .iter()
                .map(|c| c.compression_ratio)
                .sum::<f64>()
                / total_compactions as f64
        } else {
            1.0
        };

        // Find most common compaction type
        let mut type_counts: HashMap<CompactionType, u32> = HashMap::new();
        for c in &self.compactions {
            *type_counts.entry(c.compaction_type).or_insert(0) += 1;
        }
        let most_common = type_counts
            .into_iter()
            .max_by_key(|(_, v)| *v)
            .map(|(t, _)| t)
            .unwrap_or(CompactionType::None);

        // Find most active platform
        let mut platform_counts: HashMap<Platform, u32> = HashMap::new();
        for c in &self.compactions {
            *platform_counts.entry(c.platform).or_insert(0) += 1;
        }
        let most_active = platform_counts
            .into_iter()
            .max_by_key(|(_, v)| *v)
            .map(|(p, _)| p)
            .unwrap_or(Platform::ClaudeCode);

        TokenTrackerStats {
            total_events,
            total_compactions,
            avg_tokens_per_event: avg_tokens,
            avg_compaction_interval: avg_interval,
            avg_compression_ratio: avg_compression,
            most_common_compaction: most_common,
            most_active_platform: most_active,
        }
    }

    /// Predict when next compaction will occur based on current trajectory
    pub fn predict_next_compaction(&self, session_id: &str) -> Option<Prediction> {
        let session_events = self.get_session_events(session_id);
        if session_events.len() < 2 {
            return None;
        }

        // Get platform config
        let platform = self.session_platforms.get(session_id)?;
        let config = self.platform_configs.get(platform)?;

        // Calculate average token growth rate
        let mut growth_rates: Vec<f64> = Vec::new();
        for window in session_events.windows(2) {
            let (earlier, later) = (window[0], window[1]);
            let time_diff = (later.timestamp - earlier.timestamp).num_seconds() as f64;
            if time_diff > 0.0 {
                let token_diff = later.context_tokens as f64 - earlier.context_tokens as f64;
                growth_rates.push(token_diff / time_diff); // tokens per second
            }
        }

        if growth_rates.is_empty() {
            return None;
        }

        let avg_growth_rate: f64 = growth_rates.iter().sum::<f64>() / growth_rates.len() as f64;

        // Get current context
        let current = session_events.last()?;
        let threshold = config.default_threshold * config.default_context_window as f64;
        let tokens_until_threshold = threshold - current.context_tokens as f64;

        if tokens_until_threshold <= 0.0 || avg_growth_rate <= 0.0 {
            return Some(Prediction {
                seconds_until_compaction: 0,
                estimated_tokens_at_compaction: current.context_tokens,
                confidence: 1.0,
            });
        }

        let seconds_until = tokens_until_threshold / avg_growth_rate;

        Some(Prediction {
            seconds_until_compaction: seconds_until as i64,
            estimated_tokens_at_compaction: threshold as u32,
            confidence: self.calculate_confidence_decay(0.95, session_events.len() as f64 / 10.0),
        })
    }
}

/// Prediction of next compaction event
#[derive(Debug, Clone)]
pub struct Prediction {
    /// Estimated seconds until next compaction
    pub seconds_until_compaction: i64,
    /// Estimated token count at compaction
    pub estimated_tokens_at_compaction: u32,
    /// Confidence in the prediction (0.0 - 1.0)
    pub confidence: f64,
}

impl Default for TokenTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_budget_tiers() {
        let tracker = TokenTracker::new();

        assert_eq!(tracker.get_token_budget(0.50), 120); // Normal
        assert_eq!(tracker.get_token_budget(0.70), 60); // Aggressive
        assert_eq!(tracker.get_token_budget(0.90), 20); // Micro
    }

    #[test]
    fn test_confidence_decay() {
        let tracker = TokenTracker::new();

        let initial = 0.95;
        let after_10_min = tracker.calculate_confidence_decay(initial, 10.0);
        let after_30_min = tracker.calculate_confidence_decay(initial, 30.0);

        assert!(after_10_min < initial);
        assert!(after_30_min < after_10_min);
    }

    #[test]
    fn test_stability_score() {
        let tracker = TokenTracker::new();

        // High stability: long time, few tokens, few messages
        let high = tracker.calculate_stability_score(3600, 1000, 5);

        // Low stability: short time, many tokens, many messages
        let low = tracker.calculate_stability_score(60, 50000, 100);

        assert!(high > low);
    }

    #[test]
    fn test_full_workflow() {
        use chrono::Duration;

        let mut tracker = TokenTracker::new();

        // Simulate a session with multiple events and compactions
        let session_id = "test-session-001";

        // Record several token events with increasing usage
        tracker.record_event(
            Platform::ClaudeCode,
            session_id,
            10_000,  // 10K tokens
            200_000, // 200K max
            5,
            10,
        );

        tracker.record_event(
            Platform::ClaudeCode,
            session_id,
            50_000, // 50K tokens
            200_000,
            15,
            25,
        );

        tracker.record_event(
            Platform::ClaudeCode,
            session_id,
            100_000, // 100K tokens (50% usage)
            200_000,
            25,
            40,
        );

        tracker.record_event(
            Platform::ClaudeCode,
            session_id,
            150_000, // 150K tokens (75% usage)
            200_000,
            35,
            55,
        );

        // Record a compaction event
        let start = Utc::now();
        let end = start + Duration::seconds(5);

        tracker.record_compaction(CompactionRecord {
            platform: Platform::ClaudeCode,
            session_id: session_id.to_string(),
            started_at: start,
            completed_at: end,
            tokens_before: 150_000,
            tokens_after: 80_000,
            compaction_type: CompactionType::Summarize,
            is_automatic: true,
        });

        // Get statistics
        let stats = tracker.get_statistics();

        assert_eq!(stats.total_events, 4);
        assert_eq!(stats.total_compactions, 1);

        // Verify token budget tiers
        assert_eq!(tracker.get_token_budget(0.5), 120); // Normal
        assert_eq!(tracker.get_token_budget(0.75), 60); // Aggressive
        assert_eq!(tracker.get_token_budget(0.95), 20); // Micro
    }

    #[test]
    fn test_platform_comparison() {
        let mut tracker = TokenTracker::new();

        // Add events from different platforms
        for i in 0..10 {
            tracker.record_event(
                Platform::ClaudeCode,
                &format!("claude-{}", i),
                50000 + (i as u32 * 1000),
                200_000,
                10,
                20,
            );
        }

        for i in 0..5 {
            tracker.record_event(
                Platform::Codex,
                &format!("codex-{}", i),
                30000 + (i as u32 * 1000),
                128_000,
                8,
                15,
            );
        }

        let _stats = tracker.get_statistics();

        // Verify platform stats
        let claude_stats = tracker.get_platform_stats(Platform::ClaudeCode);
        let codex_stats = tracker.get_platform_stats(Platform::Codex);

        assert!(claude_stats.get("event_count").unwrap() > codex_stats.get("event_count").unwrap());
    }

    #[test]
    fn test_compaction_prediction() {
        use chrono::Duration;

        let mut tracker = TokenTracker::new();

        let session_id = "predict-test";

        // Record events with known growth pattern - need to use direct event creation
        // because record_event uses current timestamp for both events
        // Create events directly with time differences
        let event1 = TokenEvent {
            id: "e1".to_string(),
            platform: Platform::ClaudeCode,
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
            context_tokens: 50_000,
            max_context: 200_000,
            usage_ratio: 0.25,
            message_count: 10,
            tool_call_count: 20,
        };

        let event2 = TokenEvent {
            id: "e2".to_string(),
            platform: Platform::ClaudeCode,
            session_id: session_id.to_string(),
            timestamp: Utc::now() + Duration::seconds(10), // 10 seconds later
            context_tokens: 60_000,                        // 10K growth
            max_context: 200_000,
            usage_ratio: 0.30,
            message_count: 12,
            tool_call_count: 25,
        };

        tracker.events.push(event1);
        tracker.events.push(event2);
        tracker
            .session_platforms
            .insert(session_id.to_string(), Platform::ClaudeCode);

        // Get prediction
        let prediction = tracker.predict_next_compaction(session_id);

        assert!(prediction.is_some());

        let pred = prediction.unwrap();

        // With 10K token growth in 10 seconds = 1000 tokens/second
        // Threshold = 0.85 * 200000 = 170000
        // Tokens until threshold = 170000 - 60000 = 110000
        // Time until = 110000 / 1000 = 110 seconds
        assert!(pred.seconds_until_compaction > 0);
    }
}
