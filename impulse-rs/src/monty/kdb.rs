//! KDB (Knowledge Database) integration helpers for Monty
//!
//! This module provides functions that can be called from Monty-executed code
//! to interact with the Knowledge Database system.

use serde::{Deserialize, Serialize};

/// Represents a finding extracted from session logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub content: String,
    pub severity: String,
    pub source_session: Option<String>,
}

/// Represents a concept discovered in documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concept {
    pub name: String,
    pub definition: Option<String>,
    pub related_concepts: Vec<String>,
}

/// Represents a risk identified in the codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Risk {
    pub id: String,
    pub description: String,
    pub severity: String,
    pub mitigation: Option<String>,
}

/// KDB contribution data structure
/// This matches the format expected by bulk_contribute.py
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdbContribution {
    pub session_id: String,
    pub findings: Vec<Finding>,
    pub concepts: Vec<Concept>,
    pub risks: Vec<Risk>,
    pub metadata: serde_json::Value,
}

impl KdbContribution {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            findings: Vec::new(),
            concepts: Vec::new(),
            risks: Vec::new(),
            metadata: serde_json::json!({}),
        }
    }

    pub fn add_finding(&mut self, content: String, severity: String) {
        self.findings.push(Finding {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            severity,
            source_session: Some(self.session_id.clone()),
        });
    }

    pub fn add_concept(&mut self, name: String, definition: Option<String>) {
        self.concepts.push(Concept {
            name,
            definition,
            related_concepts: Vec::new(),
        });
    }

    pub fn add_risk(&mut self, description: String, severity: String, mitigation: Option<String>) {
        self.risks.push(Risk {
            id: uuid::Uuid::new_v4().to_string(),
            description,
            severity,
            mitigation,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kdb_contribution() {
        let mut contrib = KdbContribution::new("test-session".to_string());
        contrib.add_finding("Found a bug in auth flow".to_string(), "high".to_string());
        contrib.add_concept("JWT".to_string(), Some("JSON Web Token".to_string()));

        assert_eq!(contrib.findings.len(), 1);
        assert_eq!(contrib.concepts.len(), 1);
    }
}
