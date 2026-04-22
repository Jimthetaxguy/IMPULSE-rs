use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};

use super::analyzer;
use super::types::*;
use crate::state::Config;

/// Context monitor that tracks JSONL growth and detects threshold crossings
pub struct ContextMonitor {
    pub config: StewardshipConfig,
    pub session_id: String,
    pub transcript_path: PathBuf,
    pub checks: Vec<MonitorCheck>,
    pub last_threshold: ThresholdLevel,
}

impl ContextMonitor {
    /// Create a new monitor for a session
    pub fn new(session_id: &str, transcript_path: &Path, config: &Config) -> Self {
        Self {
            config: StewardshipConfig::from_config(config),
            session_id: session_id.to_string(),
            transcript_path: transcript_path.to_path_buf(),
            checks: Vec::new(),
            last_threshold: ThresholdLevel::Passive,
        }
    }

    /// Quick check: file size based estimation (fast, no JSONL parsing)
    pub fn quick_check(&mut self) -> Result<MonitorCheck> {
        let (file_size, estimated_tokens, estimated_pct) = analyzer::quick_estimate_from_file(
            &self.transcript_path,
            self.config.context_window_tokens,
        )?;

        let threshold = self.config.resolve_threshold(estimated_pct);
        let check = MonitorCheck {
            timestamp: Utc::now(),
            file_size_bytes: file_size,
            estimated_tokens,
            estimated_pct,
            threshold,
            action_taken: None,
        };

        self.checks.push(check.clone());
        self.last_threshold = threshold;
        Ok(check)
    }

    /// Full check: parse JSONL for accurate token count (slower)
    pub fn full_check(&mut self, app_config: &Config) -> Result<MonitorCheck> {
        let analysis =
            analyzer::analyze_session(&self.transcript_path, &self.session_id, "", app_config)?;

        let threshold = self
            .config
            .resolve_threshold(analysis.estimated_context_pct);
        let check = MonitorCheck {
            timestamp: Utc::now(),
            file_size_bytes: std::fs::metadata(&self.transcript_path)
                .map(|m| m.len())
                .unwrap_or(0),
            estimated_tokens: analysis.estimated_tokens,
            estimated_pct: analysis.estimated_context_pct,
            threshold,
            action_taken: None,
        };

        self.checks.push(check.clone());
        self.last_threshold = threshold;
        Ok(check)
    }

    /// Check if a threshold was crossed since last check
    pub fn threshold_crossed(&self) -> Option<(ThresholdLevel, ThresholdLevel)> {
        if self.checks.len() < 2 {
            return None;
        }
        let prev = &self.checks[self.checks.len() - 2];
        let curr = &self.checks[self.checks.len() - 1];
        if curr.threshold != prev.threshold {
            Some((prev.threshold, curr.threshold))
        } else {
            None
        }
    }

    /// Get the most recent check
    pub fn latest(&self) -> Option<&MonitorCheck> {
        self.checks.last()
    }

    /// Get current context percentage
    pub fn current_pct(&self) -> f32 {
        self.checks.last().map(|c| c.estimated_pct).unwrap_or(0.0)
    }

    /// Get current threshold level
    pub fn current_threshold(&self) -> ThresholdLevel {
        self.last_threshold
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_monitor_quick_check() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        // Write some content
        let mut f = std::fs::File::create(&path).unwrap();
        for i in 0..100 {
            writeln!(f, r#"{{"type":"user","content":"Message {}"}}"#, i).unwrap();
        }
        drop(f);

        let config = Config::default();
        let mut monitor = ContextMonitor::new("test-session", &path, &config);
        let check = monitor.quick_check().unwrap();

        assert!(check.file_size_bytes > 0);
        assert!(check.estimated_tokens > 0);
        assert!(check.estimated_pct >= 0.0);
        assert_eq!(check.threshold, ThresholdLevel::Passive); // Small file
    }

    #[test]
    fn test_threshold_crossing_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "x".repeat(100)).unwrap();

        let config = Config::default();
        let mut monitor = ContextMonitor::new("test", &path, &config);

        // First check
        monitor.quick_check().unwrap();
        assert!(monitor.threshold_crossed().is_none()); // Only 1 check

        // Grow the file significantly
        std::fs::write(&path, "x".repeat(100)).unwrap();
        monitor.quick_check().unwrap();

        // Both passive, no crossing
        assert!(monitor.threshold_crossed().is_none());
    }

    #[test]
    fn test_monitor_latest() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "test content").unwrap();

        let config = Config::default();
        let mut monitor = ContextMonitor::new("test", &path, &config);
        assert!(monitor.latest().is_none());

        monitor.quick_check().unwrap();
        assert!(monitor.latest().is_some());
    }
}
