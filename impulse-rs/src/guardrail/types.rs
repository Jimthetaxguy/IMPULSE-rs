use serde::{Deserialize, Serialize};

// ============================================================================
// Guard Action
// ============================================================================

/// What to do when a guardrail rule matches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardAction {
    /// Block the operation entirely
    Block,
    /// Warn the user but allow the operation
    Warn,
    /// Log the match silently
    Log,
}

impl GuardAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Warn => "warn",
            Self::Log => "log",
        }
    }
}

impl std::fmt::Display for GuardAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ============================================================================
// Guard Target
// ============================================================================

/// What type of operation a guardrail rule applies to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardTarget {
    /// Shell/bash commands
    Bash,
    /// Tool invocations
    ToolCall,
    /// File write operations
    FileWrite,
    /// Matches any target
    Any,
}

impl GuardTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::ToolCall => "toolcall",
            Self::FileWrite => "filewrite",
            Self::Any => "any",
        }
    }

    /// Returns true if this target matches the other target.
    /// `Any` matches everything; otherwise targets must be equal.
    pub fn matches(&self, other: &GuardTarget) -> bool {
        *self == GuardTarget::Any || *other == GuardTarget::Any || *self == *other
    }
}

impl std::fmt::Display for GuardTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ============================================================================
// Guard Rule
// ============================================================================

/// A single guardrail rule that matches against operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardRule {
    /// Unique identifier for this rule
    pub id: String,
    /// Regex or glob pattern to match against the operation
    pub pattern: String,
    /// Action to take when the rule matches
    pub action: GuardAction,
    /// What type of operation this rule applies to
    pub target: GuardTarget,
    /// Human-readable reason for this rule
    pub reason: String,
    /// Optional suggestion for what to do instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Whether this rule is active
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether this rule is built-in (not user-defined)
    #[serde(default)]
    pub builtin: bool,
}

fn default_true() -> bool {
    true
}

// ============================================================================
// Guard Config
// ============================================================================

/// Configuration for the guardrail system
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardConfig {
    /// Whether the guardrail system is enabled
    pub enabled: bool,
    /// List of guardrail rules
    pub rules: Vec<GuardRule>,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: Vec::new(),
        }
    }
}

// ============================================================================
// Guard Result
// ============================================================================

/// The result of evaluating a guardrail rule against an operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardResult {
    /// ID of the rule that matched
    pub rule_id: String,
    /// Action dictated by the matched rule
    pub action: GuardAction,
    /// The input that was matched
    pub matched_input: String,
    /// Human-readable reason for the match
    pub reason: String,
    /// Optional suggestion for what to do instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl GuardResult {
    /// Returns true if this result blocks the operation
    pub fn is_blocked(&self) -> bool {
        self.action == GuardAction::Block
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_action_serde_roundtrip() {
        for action in &[GuardAction::Block, GuardAction::Warn, GuardAction::Log] {
            let json = serde_json::to_string(action).unwrap();
            let deserialized: GuardAction = serde_json::from_str(&json).unwrap();
            assert_eq!(*action, deserialized);
        }
    }

    #[test]
    fn test_guard_target_serde_roundtrip() {
        for target in &[
            GuardTarget::Bash,
            GuardTarget::ToolCall,
            GuardTarget::FileWrite,
            GuardTarget::Any,
        ] {
            let json = serde_json::to_string(target).unwrap();
            let deserialized: GuardTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(*target, deserialized);
        }
    }

    #[test]
    fn test_guard_rule_serde() {
        let rule = GuardRule {
            id: "no-rm-rf".to_string(),
            pattern: "rm\\s+-rf".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Dangerous recursive delete".to_string(),
            suggestion: Some("Use trash-put instead".to_string()),
            enabled: true,
            builtin: true,
        };

        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: GuardRule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "no-rm-rf");
        assert_eq!(deserialized.action, GuardAction::Block);
        assert_eq!(deserialized.target, GuardTarget::Bash);
        assert!(deserialized.enabled);
        assert!(deserialized.builtin);
        assert_eq!(
            deserialized.suggestion,
            Some("Use trash-put instead".to_string())
        );
    }

    #[test]
    fn test_guard_action_display() {
        assert_eq!(GuardAction::Block.as_str(), "block");
        assert_eq!(GuardAction::Warn.as_str(), "warn");
        assert_eq!(GuardAction::Log.as_str(), "log");
    }

    #[test]
    fn test_guard_target_display() {
        assert_eq!(GuardTarget::Bash.as_str(), "bash");
        assert_eq!(GuardTarget::ToolCall.as_str(), "toolcall");
        assert_eq!(GuardTarget::FileWrite.as_str(), "filewrite");
        assert_eq!(GuardTarget::Any.as_str(), "any");
    }

    #[test]
    fn test_guard_config_default() {
        let config = GuardConfig::default();
        assert!(config.enabled);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_guard_result_blocked() {
        let blocked = GuardResult {
            rule_id: "test-rule".to_string(),
            action: GuardAction::Block,
            matched_input: "rm -rf /".to_string(),
            reason: "Dangerous".to_string(),
            suggestion: None,
        };
        assert!(blocked.is_blocked());

        let warned = GuardResult {
            rule_id: "test-rule".to_string(),
            action: GuardAction::Warn,
            matched_input: "something".to_string(),
            reason: "Careful".to_string(),
            suggestion: None,
        };
        assert!(!warned.is_blocked());

        let logged = GuardResult {
            rule_id: "test-rule".to_string(),
            action: GuardAction::Log,
            matched_input: "something".to_string(),
            reason: "Noted".to_string(),
            suggestion: None,
        };
        assert!(!logged.is_blocked());
    }
}
