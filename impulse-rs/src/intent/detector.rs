use crate::intent::types::{Activity, AgentIntent, Complexity, IntentCategory};
use std::collections::HashMap;

/// Rule-based intent classifier
/// Fast path for simple intent detection without AI calls
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

    /// Classify intent from activity details
    pub fn classify(&self, activity: &Activity) -> AgentIntent {
        let mut intent = AgentIntent::new(activity.agent_id.clone(), activity.agent_type);

        // Extract keywords from activity
        let all_text: Vec<String> = activity
            .details
            .iter()
            .chain(activity.target.iter())
            .cloned()
            .collect();

        let text_lower: Vec<String> = all_text.iter().map(|s| s.to_lowercase()).collect();

        // Find matching category
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

        // Set confidence based on keyword matches
        intent.confidence = if best_matches >= 3 {
            0.9
        } else if best_matches >= 2 {
            0.75
        } else if best_matches >= 1 {
            0.6
        } else {
            0.3
        };

        // Determine complexity from activity patterns
        intent.complexity = self.estimate_complexity(activity);

        // Extract target as scope if it's a file path
        if let Some(ref target) = activity.target {
            if target.contains('/') || target.contains('\\') {
                intent.scope.push(std::path::PathBuf::from(target));
            }
        }

        // Generate goal from details
        if !activity.details.is_empty() {
            intent.goal = activity.details.join(" ");
        }

        intent
    }

    /// Estimate complexity based on activity patterns
    fn estimate_complexity(&self, activity: &Activity) -> Complexity {
        let detail_count = activity.details.len();

        // More details suggests higher complexity
        if detail_count > 5 {
            return Complexity::High;
        }

        // Check for complexity indicators in details
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

/// Intent classifier trait for extensibility
pub trait IntentClassifier: Send + Sync {
    /// Classify intent from activity
    fn classify(&self, activity: &Activity) -> AgentIntent;

    /// Check if this classifier is available
    fn is_available(&self) -> bool;

    /// Get classifier name
    fn name(&self) -> &'static str;
}

/// Composite classifier that tries multiple classifiers
pub struct CompositeClassifier {
    classifiers: Vec<Box<dyn IntentClassifier>>,
}

impl CompositeClassifier {
    pub fn new() -> Self {
        Self {
            classifiers: Vec::new(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn add<C: IntentClassifier + 'static>(mut self, classifier: C) -> Self {
        self.classifiers.push(Box::new(classifier));
        self
    }

    /// Classify using first available classifier
    pub fn classify(&self, activity: &Activity) -> Option<AgentIntent> {
        for classifier in &self.classifiers {
            if classifier.is_available() {
                return Some(classifier.classify(activity));
            }
        }

        // Fallback to rule-based if no AI classifiers available
        let rule_classifier = RuleBasedClassifier::new();
        Some(rule_classifier.classify(activity))
    }
}

impl Default for CompositeClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::types::{ActivityType, AgentType};

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
}
