//! Intent detection for agent activities.
//!
//! Detects what an AI agent is trying to do (refactoring, testing, debugging, etc.)
//! from structured activity events derived from PTY output. Uses keyword-based
//! classification for fast, deterministic results.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

// ─── Types ───────────────────────────────────────────────────────────────

/// Type of AI agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    OpenCode,
    Minimax,
    Gpt,
    Shell,
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Minimax => "minimax",
            Self::Gpt => "gpt",
            Self::Shell => "shell",
        }
    }

    // clippy: from_keywords is a domain method, not From trait
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            "minimax" => Some(Self::Minimax),
            "gpt" | "openai" => Some(Self::Gpt),
            "shell" | "bash" | "zsh" => Some(Self::Shell),
            _ => None,
        }
    }
}

/// Intent categories — what an agent is trying to accomplish.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum IntentCategory {
    Refactoring,
    Implementing,
    Testing,
    Debugging,
    Documenting,
    Analyzing,
    Configuring,
    Deploying,
    Unknown,
}

impl IntentCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Refactoring => "refactoring",
            Self::Implementing => "implementing",
            Self::Testing => "testing",
            Self::Debugging => "debugging",
            Self::Documenting => "documenting",
            Self::Analyzing => "analyzing",
            Self::Configuring => "configuring",
            Self::Deploying => "deploying",
            Self::Unknown => "unknown",
        }
    }

    /// Rule-based classification from keywords.
    pub fn from_keywords(keywords: &[&str]) -> Self {
        let kw_set: Vec<String> = keywords.iter().map(|s| s.to_lowercase()).collect();

        if kw_set.iter().any(|k| k.contains("test")) {
            return Self::Testing;
        }
        if kw_set.iter().any(|k| {
            k.contains("fix") || k.contains("bug") || k.contains("error") || k.contains("debug")
        }) {
            return Self::Debugging;
        }
        if kw_set
            .iter()
            .any(|k| k.contains("refactor") || k.contains("restructure") || k.contains("cleanup"))
        {
            return Self::Refactoring;
        }
        if kw_set.iter().any(|k| {
            k.contains("implement")
                || k.contains("add")
                || k.contains("create")
                || k.contains("new")
        }) {
            return Self::Implementing;
        }
        if kw_set
            .iter()
            .any(|k| k.contains("doc") || k.contains("comment") || k.contains("readme"))
        {
            return Self::Documenting;
        }
        if kw_set
            .iter()
            .any(|k| k.contains("config") || k.contains("setup") || k.contains("env"))
        {
            return Self::Configuring;
        }
        if kw_set
            .iter()
            .any(|k| k.contains("deploy") || k.contains("release") || k.contains("build"))
        {
            return Self::Deploying;
        }
        if kw_set
            .iter()
            .any(|k| k.contains("analyze") || k.contains("review") || k.contains("understand"))
        {
            return Self::Analyzing;
        }

        Self::Unknown
    }
}

/// Complexity level of the intent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

impl Complexity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// Context attached to an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentContext {
    pub key: String,
    pub value: String,
}

impl IntentContext {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Represents an agent's current intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIntent {
    pub agent_id: String,
    pub agent_type: AgentType,
    pub intent_category: IntentCategory,
    pub scope: Vec<PathBuf>,
    pub complexity: Complexity,
    pub goal: String,
    pub confidence: f32,
    pub timestamp: DateTime<Utc>,
    pub context: Vec<IntentContext>,
}

impl AgentIntent {
    pub fn new(agent_id: String, agent_type: AgentType) -> Self {
        Self {
            agent_id,
            agent_type,
            intent_category: IntentCategory::Unknown,
            scope: Vec::new(),
            complexity: Complexity::Low,
            goal: String::new(),
            confidence: 0.0,
            timestamp: Utc::now(),
            context: Vec::new(),
        }
    }

    pub fn with_category(mut self, category: IntentCategory) -> Self {
        self.intent_category = category;
        self
    }

    pub fn with_goal(mut self, goal: String) -> Self {
        self.goal = goal;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn with_scope(mut self, scope: Vec<PathBuf>) -> Self {
        self.scope = scope;
        self
    }
}

/// Types of activities that can trigger intent detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    FileEdit,
    FileCreate,
    FileDelete,
    ToolCall,
    CommandRun,
    Output,
    Error,
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileEdit => "file_edit",
            Self::FileCreate => "file_create",
            Self::FileDelete => "file_delete",
            Self::ToolCall => "tool_call",
            Self::CommandRun => "command_run",
            Self::Output => "output",
            Self::Error => "error",
        }
    }
}

/// Activity that triggers intent detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub agent_id: String,
    pub agent_type: AgentType,
    pub activity_type: ActivityType,
    pub target: Option<String>,
    pub details: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

impl Activity {
    pub fn new(agent_id: String, agent_type: AgentType, activity_type: ActivityType) -> Self {
        Self {
            agent_id,
            agent_type,
            activity_type,
            target: None,
            details: Vec::new(),
            timestamp: Utc::now(),
        }
    }

    pub fn with_target(mut self, target: String) -> Self {
        self.target = Some(target);
        self
    }

    pub fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }
}

// ─── Classifier ──────────────────────────────────────────────────────────

/// Rule-based intent classifier.
/// Fast path for simple intent detection without AI calls.
pub struct RuleBasedClassifier {
    keywords: HashMap<IntentCategory, Vec<String>>,
}

impl RuleBasedClassifier {
    pub fn new() -> Self {
        let mut keywords = HashMap::new();

        keywords.insert(
            IntentCategory::Refactoring,
            vec![
                "refactor".to_string(),
                "restructure".to_string(),
                "cleanup".to_string(),
                "simplify".to_string(),
                "improve".to_string(),
                "reorganize".to_string(),
                "extract".to_string(),
                "inline".to_string(),
                "rename".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Implementing,
            vec![
                "implement".to_string(),
                "add".to_string(),
                "create".to_string(),
                "new".to_string(),
                "build".to_string(),
                "make".to_string(),
                "introduce".to_string(),
                "extend".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Testing,
            vec![
                "test".to_string(),
                "spec".to_string(),
                "verify".to_string(),
                "check".to_string(),
                "assert".to_string(),
                "coverage".to_string(),
                "unittest".to_string(),
                "integration".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Debugging,
            vec![
                "fix".to_string(),
                "bug".to_string(),
                "error".to_string(),
                "debug".to_string(),
                "issue".to_string(),
                "problem".to_string(),
                "crash".to_string(),
                "fail".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Documenting,
            vec![
                "document".to_string(),
                "doc".to_string(),
                "comment".to_string(),
                "readme".to_string(),
                "api".to_string(),
                "spec".to_string(),
                "guide".to_string(),
                "tutorial".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Configuring,
            vec![
                "config".to_string(),
                "configure".to_string(),
                "setup".to_string(),
                "env".to_string(),
                "environment".to_string(),
                "setting".to_string(),
                "flag".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Deploying,
            vec![
                "deploy".to_string(),
                "release".to_string(),
                "ship".to_string(),
                "publish".to_string(),
                "push".to_string(),
                "prod".to_string(),
                "staging".to_string(),
            ],
        );

        keywords.insert(
            IntentCategory::Analyzing,
            vec![
                "analyze".to_string(),
                "review".to_string(),
                "understand".to_string(),
                "explore".to_string(),
                "investigate".to_string(),
                "examine".to_string(),
                "research".to_string(),
                "audit".to_string(),
            ],
        );

        Self { keywords }
    }

    /// Classify intent from activity details.
    pub fn classify(&self, activity: &Activity) -> AgentIntent {
        let mut intent = AgentIntent::new(activity.agent_id.clone(), activity.agent_type);

        let all_text: Vec<String> = activity
            .details
            .iter()
            .chain(activity.target.iter())
            .cloned()
            .collect();

        let text_lower: Vec<String> = all_text.iter().map(|s| s.to_lowercase()).collect();

        let mut best_category = IntentCategory::Unknown;
        let mut best_matches = 0;

        for (category, keywords) in &self.keywords {
            let matches: usize = keywords
                .iter()
                .filter(|kw| text_lower.iter().any(|t| t.contains(kw.as_str())))
                .count();

            if matches > best_matches {
                best_matches = matches;
                best_category = *category;
            }
        }

        intent.intent_category = best_category;

        intent.confidence = if best_matches >= 3 {
            0.9
        } else if best_matches >= 2 {
            0.75
        } else if best_matches >= 1 {
            0.6
        } else {
            0.3
        };

        intent.complexity = self.estimate_complexity(activity);

        if let Some(ref target) = activity.target {
            if target.contains('/') || target.contains('\\') {
                intent.scope.push(PathBuf::from(target));
            }
        }

        if !activity.details.is_empty() {
            intent.goal = activity.details.join(" ");
        }

        intent
    }

    fn estimate_complexity(&self, activity: &Activity) -> Complexity {
        let detail_count = activity.details.len();

        if detail_count > 5 {
            return Complexity::High;
        }

        let complexity_indicators = ["refactor", "redesign", "migrate", "overhaul", "rewrite"];
        if activity.details.iter().any(|d| {
            complexity_indicators
                .iter()
                .any(|ci| d.to_lowercase().contains(ci))
        }) {
            return Complexity::High;
        }

        if detail_count > 2 {
            return Complexity::Medium;
        }

        Complexity::Low
    }
}

impl Default for RuleBasedClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Intent Store ────────────────────────────────────────────────────────

/// Intent store for managing detected intents across agents.
pub struct IntentStore {
    intents: RwLock<HashMap<String, Vec<AgentIntent>>>,
    classifier: RuleBasedClassifier,
}

impl IntentStore {
    pub fn new() -> Self {
        Self {
            intents: RwLock::new(HashMap::new()),
            classifier: RuleBasedClassifier::new(),
        }
    }

    /// Add an activity and get detected intent.
    pub fn detect(&self, activity: Activity) -> AgentIntent {
        let intent = self.classifier.classify(&activity);

        if let Ok(mut intents) = self.intents.write() {
            intents
                .entry(activity.agent_id.clone())
                .or_insert_with(Vec::new)
                .push(intent.clone());
        }

        intent
    }

    /// Get current intent for an agent.
    pub fn get_current(&self, agent_id: &str) -> Option<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.get(agent_id).and_then(|i| i.last().cloned())
        } else {
            None
        }
    }

    /// Get all intents for an agent.
    pub fn get_all(&self, agent_id: &str) -> Vec<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.get(agent_id).cloned().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Get all current intents (latest per agent).
    pub fn get_all_current(&self) -> Vec<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.values().filter_map(|i| i.last().cloned()).collect()
        } else {
            Vec::new()
        }
    }

    /// Detect conflicts between agent intents.
    pub fn detect_conflicts(&self) -> Vec<IntentConflict> {
        let current = self.get_all_current();
        let mut conflicts = Vec::new();

        for i in 0..current.len() {
            for j in (i + 1)..current.len() {
                let a = &current[i];
                let b = &current[j];

                if a.intent_category == b.intent_category
                    && a.intent_category != IntentCategory::Unknown
                {
                    let scope_overlap = a.scope.iter().any(|p1| b.scope.iter().any(|p2| p1 == p2));

                    if scope_overlap {
                        conflicts.push(IntentConflict {
                            agent_a: a.agent_id.clone(),
                            agent_b: b.agent_id.clone(),
                            category: a.intent_category,
                            scope: a.scope.clone(),
                            confidence: (a.confidence + b.confidence) / 2.0,
                        });
                    }
                }
            }
        }

        conflicts
    }

    /// Clear intents for an agent.
    pub fn clear(&self, agent_id: &str) {
        if let Ok(mut intents) = self.intents.write() {
            intents.remove(agent_id);
        }
    }

    /// Clear all intents.
    pub fn clear_all(&self) {
        if let Ok(mut intents) = self.intents.write() {
            intents.clear();
        }
    }
}

impl Default for IntentStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a conflict between agent intents.
#[derive(Debug, Clone)]
pub struct IntentConflict {
    pub agent_a: String,
    pub agent_b: String,
    pub category: IntentCategory,
    pub scope: Vec<PathBuf>,
    pub confidence: f32,
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_from_str() {
        assert_eq!(AgentType::from_str("claude"), Some(AgentType::Claude));
        assert_eq!(AgentType::from_str("codex"), Some(AgentType::Codex));
        assert_eq!(AgentType::from_str("opencode"), Some(AgentType::OpenCode));
        assert_eq!(AgentType::from_str("invalid"), None);
    }

    #[test]
    fn test_intent_category_from_keywords() {
        assert_eq!(
            IntentCategory::from_keywords(&["write", "test"]),
            IntentCategory::Testing
        );
        assert_eq!(
            IntentCategory::from_keywords(&["fix", "error"]),
            IntentCategory::Debugging
        );
        assert_eq!(
            IntentCategory::from_keywords(&["add", "function"]),
            IntentCategory::Implementing
        );
    }

    #[test]
    fn test_agent_intent_builder() {
        let intent = AgentIntent::new("agent-1".to_string(), AgentType::Claude)
            .with_category(IntentCategory::Refactoring)
            .with_goal("Simplify token handling".to_string())
            .with_confidence(0.85);

        assert_eq!(intent.agent_id, "agent-1");
        assert_eq!(intent.intent_category, IntentCategory::Refactoring);
        assert_eq!(intent.goal, "Simplify token handling");
        assert_eq!(intent.confidence, 0.85);
    }

    #[test]
    fn test_rule_based_classifier_refactoring() {
        let classifier = RuleBasedClassifier::new();

        let activity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::FileEdit,
        )
        .with_target("src/auth/mod.rs".to_string())
        .with_details(vec![
            "refactoring token handling".to_string(),
            "simplify the validation logic".to_string(),
        ]);

        let intent = classifier.classify(&activity);

        assert_eq!(intent.intent_category, IntentCategory::Refactoring);
        assert!(intent.confidence >= 0.6);
    }

    #[test]
    fn test_rule_based_classifier_testing() {
        let classifier = RuleBasedClassifier::new();

        let activity = Activity::new(
            "agent-2".to_string(),
            AgentType::Codex,
            ActivityType::FileCreate,
        )
        .with_target("tests/auth_test.rs".to_string())
        .with_details(vec!["write tests for auth module".to_string()]);

        let intent = classifier.classify(&activity);

        assert_eq!(intent.intent_category, IntentCategory::Testing);
    }

    #[test]
    fn test_complexity_estimation() {
        let classifier = RuleBasedClassifier::new();

        let low_complexity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::FileEdit,
        )
        .with_details(vec!["fix typo".to_string()]);

        let high_complexity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::FileEdit,
        )
        .with_details(vec![
            "refactor entire auth module".to_string(),
            "redesign token handling".to_string(),
            "migrate to new architecture".to_string(),
            "update tests".to_string(),
            "fix related issues".to_string(),
            "verify coverage".to_string(),
        ]);

        assert_eq!(
            classifier.estimate_complexity(&low_complexity),
            Complexity::Low
        );
        assert_eq!(
            classifier.estimate_complexity(&high_complexity),
            Complexity::High
        );
    }

    #[test]
    fn test_intent_store() {
        let store = IntentStore::new();

        let activity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::FileEdit,
        )
        .with_target("src/auth/mod.rs".to_string())
        .with_details(vec!["refactor token handling".to_string()]);

        let intent = store.detect(activity);
        assert_eq!(intent.intent_category, IntentCategory::Refactoring);
    }

    #[test]
    fn test_conflict_detection() {
        let store = IntentStore::new();

        let activity1 = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::FileEdit,
        )
        .with_target("src/auth/mod.rs".to_string())
        .with_details(vec!["refactor token handling".to_string()]);

        let activity2 = Activity::new(
            "agent-2".to_string(),
            AgentType::Codex,
            ActivityType::FileEdit,
        )
        .with_target("src/auth/mod.rs".to_string())
        .with_details(vec!["refactor auth module".to_string()]);

        store.detect(activity1);
        store.detect(activity2);

        let conflicts = store.detect_conflicts();
        assert!(!conflicts.is_empty());
    }

    #[test]
    fn test_intent_store_engine() {
        let store = IntentStore::new();

        let activity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::ToolCall,
        )
        .with_target("cargo-test".to_string())
        .with_details(vec!["run tests".to_string()]);

        let intent = store.detect(activity);
        assert_eq!(intent.intent_category, IntentCategory::Testing);
    }
}
