use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Represents an agent's current intent
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

/// Type of AI agent
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

/// Intent categories
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

    /// Rule-based classification keywords
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

/// Complexity level of the intent
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

/// Context attached to an intent
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

/// Activity that triggers intent detection
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

/// Types of activities that can trigger intent detection
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
}
