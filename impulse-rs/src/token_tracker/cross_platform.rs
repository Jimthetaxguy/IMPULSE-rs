//! Cross-Platform Token Analysis
//!
//! Advanced analysis tools for comparing token usage patterns across platforms

use crate::token_tracker::{CompactionEvent, MetricsAnalyzer, Platform, TokenEvent, TrendAnalysis};
use std::collections::HashMap;

/// Cross-platform benchmark results
#[derive(Debug, Clone)]
pub struct CrossPlatformBenchmark {
    /// Platform-specific results
    pub results: HashMap<Platform, PlatformBenchmark>,
    /// Comparison metrics
    pub comparison: PlatformComparison,
}

/// Benchmark results for a single platform
#[derive(Debug, Clone)]
pub struct PlatformBenchmark {
    pub platform: Platform,
    /// Average tokens per session
    pub avg_tokens: f64,
    /// Average context usage percentage
    pub avg_usage: f64,
    /// Average time between compactions (seconds)
    pub avg_interval: f64,
    /// Average compression ratio
    pub avg_compression: f64,
    /// Auto compaction rate
    pub auto_rate: f64,
    /// Efficiency score (0-1)
    pub efficiency: f64,
    /// Trend analysis
    pub trend: Option<TrendAnalysis>,
}

/// Comparison between platforms
#[derive(Debug, Clone)]
pub struct PlatformComparison {
    /// Most efficient platform
    pub most_efficient: Platform,
    /// Most stable platform (longest between compactions)
    pub most_stable: Platform,
    /// Platform with best compression
    pub best_compression: Platform,
    /// Overall rankings
    pub rankings: Vec<(Platform, f64)>,
}

/// Analyzes token patterns across multiple platforms
pub struct CrossPlatformAnalyzer {
    events: HashMap<Platform, Vec<TokenEvent>>,
    compactions: HashMap<Platform, Vec<CompactionEvent>>,
}

impl CrossPlatformAnalyzer {
    /// Create a new analyzer
    pub fn new() -> Self {
        Self {
            events: HashMap::new(),
            compactions: HashMap::new(),
        }
    }

    /// Add events for a platform
    pub fn add_events(&mut self, platform: Platform, events: Vec<TokenEvent>) {
        self.events.insert(platform, events);
    }

    /// Add compactions for a platform
    pub fn add_compactions(&mut self, platform: Platform, compactions: Vec<CompactionEvent>) {
        self.compactions.insert(platform, compactions);
    }

    /// Run cross-platform analysis
    pub fn analyze(&self) -> CrossPlatformBenchmark {
        let mut results = HashMap::new();
        // Vecs in the fixed platform order below, not HashMaps: iterating a
        // HashMap made every tie (all platforms empty, say) resolve to a
        // random platform, so the same input gave a different "most stable"
        // and a different ranking on each call.
        let mut efficiencies = Vec::new();
        let mut intervals = Vec::new();
        let mut compressions = Vec::new();

        for platform in [
            Platform::ClaudeCode,
            Platform::Codex,
            Platform::OpenCode,
            Platform::ChatGPT,
            Platform::Gemini,
        ] {
            let events = self.events.get(&platform).cloned().unwrap_or_default();
            let compactions = self.compactions.get(&platform).cloned().unwrap_or_default();

            let analyzer = MetricsAnalyzer::new(events.clone(), compactions.clone());

            let avg_tokens = if !events.is_empty() {
                events.iter().map(|e| e.context_tokens as f64).sum::<f64>() / events.len() as f64
            } else {
                0.0
            };

            let avg_usage = if !events.is_empty() {
                events.iter().map(|e| e.usage_ratio).sum::<f64>() / events.len() as f64
            } else {
                0.0
            };

            let avg_interval = analyzer.avg_time_between_compactions();
            let avg_compression = if !compactions.is_empty() {
                compactions.iter().map(|c| c.compression_ratio).sum::<f64>()
                    / compactions.len() as f64
            } else {
                1.0
            };

            let auto_rate = if !compactions.is_empty() {
                compactions.iter().filter(|c| c.is_automatic).count() as f64
                    / compactions.len() as f64
            } else {
                0.0
            };

            let efficiency = analyzer.efficiency_score(platform);
            let trend = if !events.is_empty() {
                Some(analyzer.platform_trend(platform))
            } else {
                None
            };

            results.insert(
                platform,
                PlatformBenchmark {
                    platform,
                    avg_tokens,
                    avg_usage,
                    avg_interval,
                    avg_compression,
                    auto_rate,
                    efficiency,
                    trend,
                },
            );

            efficiencies.push((platform, efficiency));
            intervals.push((platform, avg_interval));
            compressions.push((platform, avg_compression));
        }

        // Determine rankings and comparisons. The sort is stable, so equal
        // scores keep the fixed platform order.
        let mut rankings = efficiencies;
        rankings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let most_efficient = rankings
            .first()
            .map(|(p, _)| *p)
            .unwrap_or(Platform::ClaudeCode);
        let most_stable = first_extreme(&intervals, |candidate, best| candidate > best);
        let best_compression = first_extreme(&compressions, |candidate, best| candidate < best);

        CrossPlatformBenchmark {
            results,
            comparison: PlatformComparison {
                most_efficient,
                most_stable,
                best_compression,
                rankings,
            },
        }
    }

    /// Generate recommendations for a platform
    pub fn recommendations(&self, platform: Platform) -> Vec<String> {
        let mut recs = Vec::new();

        let events = self.events.get(&platform);
        let compactions = self.compactions.get(&platform);

        if let Some(events) = events {
            let avg_usage: f64 =
                events.iter().map(|e| e.usage_ratio).sum::<f64>() / events.len() as f64;

            if avg_usage > 0.8 {
                recs.push(format!(
                    "High context usage ({:.1}%). Consider more aggressive pruning.",
                    avg_usage * 100.0
                ));
            }

            if let Some(compactions) = compactions {
                let auto_rate = if !compactions.is_empty() {
                    compactions.iter().filter(|c| c.is_automatic).count() as f64
                        / compactions.len() as f64
                } else {
                    0.0
                };

                if auto_rate < 0.5 {
                    recs.push(
                        "Low automatic compaction rate. Consider enabling auto-compaction."
                            .to_string(),
                    );
                }
            }
        } else {
            recs.push("No data available for this platform. Start tracking events.".to_string());
        }

        recs
    }
}

/// The first platform whose value `beats` every earlier one, so ties go to
/// the earliest platform in `values`' order (and NaN never wins).
fn first_extreme(values: &[(Platform, f64)], beats: impl Fn(f64, f64) -> bool) -> Platform {
    values
        .iter()
        .fold(
            None::<(Platform, f64)>,
            |best, &(platform, value)| match best {
                Some((_, best_value)) if !beats(value, best_value) => best,
                _ if value.is_nan() => best,
                _ => Some((platform, value)),
            },
        )
        .map(|(platform, _)| platform)
        .unwrap_or(Platform::ClaudeCode)
}

impl Default for CrossPlatformAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate token growth rate for a session
pub fn calculate_growth_rate(events: &[TokenEvent]) -> f64 {
    if events.len() < 2 {
        return 0.0;
    }

    let mut sorted: Vec<&TokenEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.timestamp);

    let mut total_rate = 0.0;
    let mut count = 0;

    for i in 1..sorted.len() {
        let earlier = sorted[i - 1];
        let later = sorted[i];
        let time_diff = (later.timestamp - earlier.timestamp).num_seconds() as f64;

        if time_diff > 0.0 {
            let token_diff = later.context_tokens as f64 - earlier.context_tokens as f64;
            total_rate += token_diff / time_diff;
            count += 1;
        }
    }

    if count > 0 {
        total_rate / count as f64
    } else {
        0.0
    }
}

/// Estimate cost based on token usage
pub fn estimate_cost(tokens: u32, platform: Platform) -> f64 {
    // Approximate cost per 1K tokens (in cents)
    let cost_per_1k = match platform {
        Platform::ClaudeCode => 15.0, // Claude 3.5 Sonnet
        Platform::Codex => 3.0,       // GPT-4o mini
        Platform::OpenCode => 3.0,    // Codex mini
        Platform::ChatGPT => 3.0,     // GPT-4o mini
        Platform::Gemini => 1.75,     // Gemini 1.5 Flash
    };

    (tokens as f64 / 1000.0) * cost_per_1k
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_growth_rate() {
        let events = vec![
            TokenEvent {
                id: "1".to_string(),
                platform: Platform::ClaudeCode,
                session_id: "s1".to_string(),
                timestamp: Utc::now(),
                context_tokens: 10000,
                max_context: 200000,
                usage_ratio: 0.05,
                message_count: 5,
                tool_call_count: 10,
            },
            TokenEvent {
                id: "2".to_string(),
                platform: Platform::ClaudeCode,
                session_id: "s1".to_string(),
                timestamp: Utc::now() + Duration::seconds(10),
                context_tokens: 20000,
                max_context: 200000,
                usage_ratio: 0.10,
                message_count: 10,
                tool_call_count: 20,
            },
        ];

        let rate = calculate_growth_rate(&events);
        // 10000 tokens / 10 seconds = 1000 tokens/second
        assert!((rate - 1000.0).abs() < 1.0);
    }

    /// Before: the same input gave a different "most stable" platform and a
    /// different ranking on each call (all five platforms and 39 distinct
    /// orders over 50 calls on an empty analyzer).
    #[test]
    fn test_analyze_is_deterministic_and_breaks_ties_in_platform_order() {
        let analyzer = CrossPlatformAnalyzer::new();
        let first = analyzer.analyze();
        for _ in 0..50 {
            let again = analyzer.analyze();
            assert_eq!(again.comparison.most_stable, first.comparison.most_stable);
            assert_eq!(
                again.comparison.best_compression,
                first.comparison.best_compression
            );
            assert_eq!(
                again
                    .comparison
                    .rankings
                    .iter()
                    .map(|(p, _)| *p)
                    .collect::<Vec<_>>(),
                first
                    .comparison
                    .rankings
                    .iter()
                    .map(|(p, _)| *p)
                    .collect::<Vec<_>>()
            );
        }
        // With no data every platform ties, so the first in order wins.
        assert_eq!(first.comparison.most_stable, Platform::ClaudeCode);
        assert_eq!(first.comparison.best_compression, Platform::ClaudeCode);
        assert_eq!(first.results.len(), 5);
    }

    #[test]
    fn test_first_extreme_prefers_the_earliest_tie_and_skips_nan() {
        let values = [
            (Platform::Codex, 2.0),
            (Platform::Gemini, 5.0),
            (Platform::ChatGPT, 5.0),
            (Platform::OpenCode, f64::NAN),
        ];
        assert_eq!(first_extreme(&values, |a, b| a > b), Platform::Gemini);
        assert_eq!(first_extreme(&values, |a, b| a < b), Platform::Codex);
        assert_eq!(first_extreme(&[], |a, b| a > b), Platform::ClaudeCode);
    }

    #[test]
    fn test_recommendations_without_data_and_with_high_usage() {
        let mut analyzer = CrossPlatformAnalyzer::new();
        assert!(analyzer.recommendations(Platform::Codex)[0].contains("No data"));
        let mut event = TokenEvent {
            id: "e".to_string(),
            platform: Platform::Codex,
            session_id: "s".to_string(),
            timestamp: Utc::now(),
            context_tokens: 100_000,
            max_context: 110_000,
            usage_ratio: 0.9,
            message_count: 1,
            tool_call_count: 0,
        };
        analyzer.add_events(Platform::Codex, vec![event.clone()]);
        let recs = analyzer.recommendations(Platform::Codex);
        assert!(
            recs.iter().any(|r| r.contains("High context usage")),
            "{recs:?}"
        );
        event.usage_ratio = 0.1;
        let mut low = CrossPlatformAnalyzer::new();
        low.add_events(Platform::Codex, vec![event]);
        assert!(low.recommendations(Platform::Codex).is_empty());
    }

    #[test]
    fn test_growth_rate_needs_two_events_with_time_between_them() {
        assert_eq!(calculate_growth_rate(&[]), 0.0);
        let now = Utc::now();
        let event = |tokens: u32| TokenEvent {
            id: tokens.to_string(),
            platform: Platform::Gemini,
            session_id: "s".to_string(),
            timestamp: now,
            context_tokens: tokens,
            max_context: 1_000_000,
            usage_ratio: 0.0,
            message_count: 0,
            tool_call_count: 0,
        };
        assert_eq!(calculate_growth_rate(&[event(10)]), 0.0);
        // Same timestamp: no elapsed time, so no rate.
        assert_eq!(calculate_growth_rate(&[event(10), event(500)]), 0.0);
    }

    #[test]
    fn test_cost_estimation() {
        let cost = estimate_cost(50000, Platform::ClaudeCode);
        // 50K tokens / 1K = 50 * 15 cents = 750 cents
        assert!((cost - 750.0).abs() < 0.01);
    }
}
