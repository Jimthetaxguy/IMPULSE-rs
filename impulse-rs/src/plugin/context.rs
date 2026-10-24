// Context plugin types and helpers

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_context_config_and_default() {
        let config = ContextConfig::default();
        assert_eq!(config.max_tokens, None);
        assert!(config.chunk_size > 0 && config.include_metadata);
        let recovered: ContextConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(recovered.chunk_size, config.chunk_size);
        assert_eq!(recovered.max_tokens, None);
    }

    #[test]
    fn round_trip_context_bundle_with_metadata() {
        let bundle = ContextBundle::new("report.xlsx", "Q3 totals", 0.8)
            .with_metadata(serde_json::json!({"sheet": "Summary"}));
        assert_eq!(bundle.metadata["sheet"], "Summary");
        let recovered: ContextBundle =
            serde_json::from_str(&serde_json::to_string(&bundle).unwrap()).unwrap();
        assert_eq!(recovered.source, "report.xlsx");
        assert_eq!(recovered.content, "Q3 totals");
        assert!((recovered.relevance_score - 0.8).abs() < f32::EPSILON);
        assert_eq!(recovered.metadata, bundle.metadata);
        assert_eq!(
            ContextBundle::new("s", "c", 0.0).metadata,
            serde_json::json!({})
        );
    }
}
