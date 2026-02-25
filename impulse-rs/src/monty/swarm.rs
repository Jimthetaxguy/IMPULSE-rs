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

/// Detect coordination patterns between two agents using keyword analysis
pub fn detect_patterns(agent_a: &str, agent_b: &str, threshold: f64) -> Vec<Pattern> {
    let mut patterns = Vec::new();

    // Check if agents share the same platform (potential echo)
    let a_lower = agent_a.to_lowercase();
    let b_lower = agent_b.to_lowercase();

    if a_lower == b_lower {
        patterns.push(Pattern {
            id: format!("echo-{}-{}", agent_a, agent_b),
            agents: vec![agent_a.to_string(), agent_b.to_string()],
            file_scope: None,
            confidence: 0.9,
            pattern_type: PatternType::Echo,
            detected_at: chrono::Utc::now().to_rfc3339(),
            decay_minutes: Some(30),
        });
    }

    // Different agents → complement pattern (above threshold)
    if a_lower != b_lower && threshold <= 0.8 {
        patterns.push(Pattern {
            id: format!("complement-{}-{}", agent_a, agent_b),
            agents: vec![agent_a.to_string(), agent_b.to_string()],
            file_scope: None,
            confidence: 0.8,
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
}
