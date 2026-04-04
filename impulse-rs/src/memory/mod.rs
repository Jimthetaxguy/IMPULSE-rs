//! Genome storage — permanent project decisions, preferences, and constraints.
//!
//! Defines the [`Genome`] type that tracks long-lived project knowledge in
//! `GENOME.md`. Supports adding decisions (with dedup), rendering to Markdown,
//! and serde round-tripping. Also re-exports [`HistoryEntry`] for the
//! append-only session log (`HISTORY.jsonl`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Impulse GENOME - permanent project decisions and preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genome {
    pub decisions: Vec<Decision>,
    pub preferences: Vec<Preference>,
    pub constraints: Vec<Constraint>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub date: DateTime<Utc>,
    pub description: String,
    pub rationale: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preference {
    pub category: String,
    pub description: String,
    pub since: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    pub description: String,
    pub reason: String,
    pub since: DateTime<Utc>,
}

impl Genome {
    pub fn new() -> Self {
        Self {
            decisions: Vec::new(),
            preferences: Vec::new(),
            constraints: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    pub fn add_decision(
        &mut self,
        description: String,
        rationale: Option<String>,
        tags: Vec<String>,
    ) {
        // Dedup guard: skip if the last decision has the same description.
        if let Some(last) = self.decisions.last() {
            if last.description == description {
                return;
            }
        }

        self.decisions.push(Decision {
            date: Utc::now(),
            description,
            rationale,
            tags,
        });
        self.last_updated = Utc::now();
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Impulse GENOME\n\n*Permanent project knowledge and decisions*\n\n");

        if !self.decisions.is_empty() {
            md.push_str("## Decisions\n\n");
            for decision in &self.decisions {
                md.push_str(&format!(
                    "- **[{}]({}):** {}\n",
                    decision.date.format("%Y-%m-%d"),
                    decision.date.format("%H:%M"),
                    decision.description
                ));
                if let Some(ref r) = decision.rationale {
                    md.push_str(&format!("  - Rationale: {}\n", r));
                }
                if !decision.tags.is_empty() {
                    md.push_str(&format!("  - Tags: {}\n", decision.tags.join(", ")));
                }
            }
            md.push('\n');
        }

        if !self.preferences.is_empty() {
            md.push_str("## Preferences\n\n");
            for pref in &self.preferences {
                md.push_str(&format!(
                    "- **[{}]({})** ({}): {}\n",
                    pref.since.format("%Y-%m-%d"),
                    pref.since.format("%H:%M"),
                    pref.category,
                    pref.description
                ));
            }
            md.push('\n');
        }

        if !self.constraints.is_empty() {
            md.push_str("## Constraints\n\n");
            for c in &self.constraints {
                md.push_str(&format!("- {} — {}\n", c.description, c.reason));
            }
            md.push('\n');
        }

        md.push_str(&format!(
            "\n---\n*Last updated: {}*\n",
            self.last_updated.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        md
    }
}

impl Default for Genome {
    fn default() -> Self {
        Self::new()
    }
}

/// Session history entry for HISTORY.jsonl
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub session_id: String,
    pub session_name: String,
    pub platform: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub summary: String,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_decision_dedup() {
        let mut genome = Genome::new();
        genome.add_decision("Test decision".into(), None, vec![]);
        genome.add_decision("Test decision".into(), None, vec![]);
        genome.add_decision("Test decision".into(), None, vec![]);
        // Should only keep one — dedup blocks consecutive identical descriptions
        assert_eq!(genome.decisions.len(), 1);
    }

    #[test]
    fn test_add_decision_different_descriptions_allowed() {
        let mut genome = Genome::new();
        genome.add_decision("Decision A".into(), None, vec![]);
        genome.add_decision("Decision B".into(), None, vec![]);
        assert_eq!(genome.decisions.len(), 2);
    }

    #[test]
    fn test_add_decision_same_after_different_allowed() {
        let mut genome = Genome::new();
        genome.add_decision("Decision A".into(), None, vec![]);
        genome.add_decision("Decision B".into(), None, vec![]);
        genome.add_decision("Decision A".into(), None, vec![]);
        // A-B-A is fine — dedup only blocks consecutive duplicates
        assert_eq!(genome.decisions.len(), 3);
    }
}
