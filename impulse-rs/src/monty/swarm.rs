//! SWARM coordination helpers
//!
//! This module provides types and functions for multi-agent coordination.
//! Pattern detection uses keyword analysis of agent names and context.

use serde::{Deserialize, Serialize};

/// Represents an agent in the SWARM system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStatus {
    Active,
    Idle,
    Coordinating,
    Waiting,
}

/// Represents a coordination pattern between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub id: String,
    pub agents: Vec<String>,
    pub file_scope: Option<String>,
    pub confidence: f64,
    pub pattern_type: PatternType,
    pub detected_at: String,
    pub decay_minutes: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternType {
    Echo,       // Agents repeating each other
    Complement, // Agents working on different parts
    Conflict,   // Agents with conflicting approaches
    Parallel,   // Agents working in parallel
}

/// A suggestion for agent coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationSuggestion {
    pub from_agent: String,
    pub to_agent: String,
    pub action: String,
    pub reasoning: String,
    pub priority: String,
}

/// Confidence assigned to an Echo pattern (identical agents).
const ECHO_CONFIDENCE: f64 = 0.9;
/// Confidence assigned to a Complement pattern (distinct agents).
const COMPLEMENT_CONFIDENCE: f64 = 0.8;

/// Detect coordination patterns between two agents using keyword analysis.
///
/// `threshold` is a confidence floor: a pattern is reported only when its
/// confidence is at least `threshold`. (Previously Echo ignored the threshold
/// entirely, so a high threshold filtered Complement but let lower-confidence
/// Echo patterns leak through.)
pub fn detect_patterns(agent_a: &str, agent_b: &str, threshold: f64) -> Vec<Pattern> {
    let mut patterns = Vec::new();

    let a_lower = agent_a.to_lowercase();
    let b_lower = agent_b.to_lowercase();

    // Identical agents → echo (one repeating the other).
    if a_lower == b_lower && ECHO_CONFIDENCE >= threshold {
        patterns.push(Pattern {
            id: format!("echo-{}-{}", agent_a, agent_b),
            agents: vec![agent_a.to_string(), agent_b.to_string()],
            file_scope: None,
            confidence: ECHO_CONFIDENCE,
            pattern_type: PatternType::Echo,
            detected_at: chrono::Utc::now().to_rfc3339(),
            decay_minutes: Some(30),
        });
    }

    // Distinct agents → complement (working on different parts).
    if a_lower != b_lower && COMPLEMENT_CONFIDENCE >= threshold {
        patterns.push(Pattern {
            id: format!("complement-{}-{}", agent_a, agent_b),
            agents: vec![agent_a.to_string(), agent_b.to_string()],
            file_scope: None,
            confidence: COMPLEMENT_CONFIDENCE,
            pattern_type: PatternType::Complement,
            detected_at: chrono::Utc::now().to_rfc3339(),
            decay_minutes: Some(60),
        });
    }

    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_patterns_echo() {
        let patterns = detect_patterns("claude-code", "claude-code", 0.88);
        assert!(patterns
            .iter()
            .any(|p| matches!(p.pattern_type, PatternType::Echo)));
    }

    #[test]
    fn test_detect_patterns_complement() {
        let patterns = detect_patterns("claude-code", "codex", 0.5);
        assert!(patterns
            .iter()
            .any(|p| matches!(p.pattern_type, PatternType::Complement)));
    }

    #[test]
    fn test_detect_patterns_high_threshold() {
        let patterns = detect_patterns("claude-code", "codex", 0.95);
        // High threshold means complement won't trigger
        assert!(!patterns
            .iter()
            .any(|p| matches!(p.pattern_type, PatternType::Complement)));
    }

    #[test]
    fn test_detect_patterns_echo_respects_threshold() {
        // Echo confidence is 0.9, so a threshold above it must suppress Echo
        // (previously Echo ignored the threshold and always fired).
        let patterns = detect_patterns("claude-code", "claude-code", 0.95);
        assert!(
            patterns.is_empty(),
            "echo (confidence 0.9) must not be reported when threshold is 0.95"
        );

        // At a threshold equal to its confidence, Echo is reported.
        let patterns = detect_patterns("claude-code", "claude-code", 0.9);
        assert!(patterns
            .iter()
            .any(|p| matches!(p.pattern_type, PatternType::Echo)));
    }
}
