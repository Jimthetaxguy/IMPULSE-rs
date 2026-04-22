// Context plugin types and helpers

#[allow(unused_imports)]
use super::{PluginInput, PluginOutput};

/// Configuration for context extraction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextConfig {
    pub max_tokens: Option<usize>,
    pub chunk_size: usize,
    pub include_metadata: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: None,
            chunk_size: 10,
            include_metadata: true,
        }
    }
}

/// A context bundle ready for injection
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextBundle {
    pub source: String,
    pub content: String,
    pub relevance_score: f32,
    pub metadata: serde_json::Value,
}

impl ContextBundle {
    pub fn new(source: impl Into<String>, content: impl Into<String>, relevance: f32) -> Self {
        Self {
            source: source.into(),
            content: content.into(),
            relevance_score: relevance,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}
