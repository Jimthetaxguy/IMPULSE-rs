pub mod detector;
pub mod providers;
pub mod types;

pub use detector::{CompositeClassifier, IntentClassifier, RuleBasedClassifier};
pub use providers::{
    ClaudeProvider, IntentProvider, MinimaxProvider, OpenAIProvider, ProviderError,
};
pub use types::{
    Activity, ActivityType, AgentIntent, AgentType, Complexity, IntentCategory, IntentContext,
};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

/// Intent store for managing detected intents
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

    /// Add an activity and get detected intent
    pub fn detect(&self, activity: Activity) -> AgentIntent {
        // Try rule-based first (fast)
        let intent = self.classifier.classify(&activity);

        // Store intent
        if let Ok(mut intents) = self.intents.write() {
            intents
                .entry(activity.agent_id.clone())
                .or_insert_with(Vec::new)
                .push(intent.clone());
        }

        intent
    }

    /// Get current intent for an agent
    pub fn get_current(&self, agent_id: &str) -> Option<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.get(agent_id).and_then(|i| i.last().cloned())
        } else {
            None
        }
    }

    /// Get all intents for an agent
    pub fn get_all(&self, agent_id: &str) -> Vec<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.get(agent_id).cloned().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// Get all current intents
    pub fn get_all_current(&self) -> Vec<AgentIntent> {
        if let Ok(intents) = self.intents.read() {
            intents.values().filter_map(|i| i.last().cloned()).collect()
        } else {
            Vec::new()
        }
    }

    /// Detect conflicts between agent intents
    pub fn detect_conflicts(&self) -> Vec<IntentConflict> {
        let current = self.get_all_current();
        let mut conflicts = Vec::new();

        for i in 0..current.len() {
            for j in (i + 1)..current.len() {
                let a = &current[i];
                let b = &current[j];

                // Check for category overlap
                if a.intent_category == b.intent_category
                    && a.intent_category != IntentCategory::Unknown
                {
                    // Check for scope overlap
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

    /// Clear intents for an agent
    pub fn clear(&self, agent_id: &str) {
        if let Ok(mut intents) = self.intents.write() {
            intents.remove(agent_id);
        }
    }

    /// Clear all intents
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

/// Represents a conflict between agent intents
#[derive(Debug, Clone)]
pub struct IntentConflict {
    pub agent_a: String,
    pub agent_b: String,
    pub category: IntentCategory,
    pub scope: Vec<std::path::PathBuf>,
    pub confidence: f32,
}

/// Intent detection engine
pub struct IntentEngine {
    store: Arc<IntentStore>,
    use_ai: bool,
}

impl IntentEngine {
    pub fn new() -> Self {
        Self {
            store: Arc::new(IntentStore::new()),
            use_ai: false, // Default to rule-based
        }
    }

    pub fn with_ai(mut self, use_ai: bool) -> Self {
        self.use_ai = use_ai;
        self
    }

    pub fn store(&self) -> &Arc<IntentStore> {
        &self.store
    }

    /// Process an activity and return detected intent
    pub fn process(&self, activity: Activity) -> AgentIntent {
        self.store.detect(activity)
    }

    /// Get detected conflicts
    pub fn get_conflicts(&self) -> Vec<IntentConflict> {
        self.store.detect_conflicts()
    }
}

impl Default for IntentEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Activity receiver for async processing
pub struct IntentReceiver {
    rx: mpsc::Receiver<Activity>,
    engine: Arc<IntentEngine>,
}

impl IntentReceiver {
    pub fn new(rx: mpsc::Receiver<Activity>, engine: Arc<IntentEngine>) -> Self {
        Self { rx, engine }
    }

    pub async fn run(&mut self) {
        while let Some(activity) = self.rx.recv().await {
            let intent = self.engine.process(activity);
            // Could emit intent to other parts of the system
            tracing::debug!("Detected intent: {:?}", intent.intent_category);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Add two activities with overlapping scope and category
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
    fn test_intent_engine() {
        let engine = IntentEngine::new();

        let activity = Activity::new(
            "agent-1".to_string(),
            AgentType::Claude,
            ActivityType::ToolCall,
        )
        .with_target("cargo-test".to_string())
        .with_details(vec!["run tests".to_string()]);

        let intent = engine.process(activity);
        assert_eq!(intent.intent_category, IntentCategory::Testing);
    }
}
