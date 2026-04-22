//! Plugin discovery and registration.
//!
//! Provides a trait-based plugin system for extensible handlers. Plugins are
//! discovered from `.impulse/plugins/`, registered in [`registry::PluginRegistry`],
//! and dispatched via action and context hooks.

pub mod action;
pub mod context;
pub mod registry;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Categories of plugins supported by Impulse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    /// Extracts structured data from sources for context injection
    ContextProvider,
    /// Executes operations on behalf of coding agents
    ActionHandler,
    /// Provides custom knowledge for retrieval
    RetrievalSource,
    /// Analyzes code and provides insights
    AnalysisModule,
}

impl PluginCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginCategory::ContextProvider => "context_provider",
            PluginCategory::ActionHandler => "action_handler",
            PluginCategory::RetrievalSource => "retrieval_source",
            PluginCategory::AnalysisModule => "analysis_module",
        }
    }
}

/// Metadata about a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: PluginCategory,
    pub supported_formats: Vec<String>,
    pub features: Vec<String>,
}

impl PluginMetadata {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        category: PluginCategory,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: String::new(),
            category,
            supported_formats: Vec::new(),
            features: Vec::new(),
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn supports_formats(mut self, formats: Vec<impl Into<String>>) -> Self {
        self.supported_formats = formats.into_iter().map(|f| f.into()).collect();
        self
    }

    pub fn with_features(mut self, features: Vec<impl Into<String>>) -> Self {
        self.features = features.into_iter().map(|f| f.into()).collect();
        self
    }
}

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

/// Errors that can occur in plugin operations
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Input for plugin execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInput {
    pub path: Option<PathBuf>,
    pub query: Option<String>,
    pub options: serde_json::Value,
}

impl PluginInput {
    pub fn new() -> Self {
        Self {
            path: None,
            query: None,
            options: serde_json::json!({}),
        }
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn with_options(mut self, options: serde_json::Value) -> Self {
        self.options = options;
        self
    }
}

impl Default for PluginInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Output from plugin execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginOutput {
    pub content: String,
    pub metadata: serde_json::Value,
    pub chunks: Vec<ContentChunk>,
}

impl PluginOutput {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            metadata: serde_json::json!({}),
            chunks: Vec::new(),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_chunks(mut self, chunks: Vec<ContentChunk>) -> Self {
        self.chunks = chunks;
        self
    }
}

/// A chunk of extracted content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentChunk {
    pub content: String,
    pub chunk_type: String,
    pub index: usize,
}

impl ContentChunk {
    pub fn new(content: impl Into<String>, chunk_type: impl Into<String>, index: usize) -> Self {
        Self {
            content: content.into(),
            chunk_type: chunk_type.into(),
            index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_category_str() {
        assert_eq!(PluginCategory::ContextProvider.as_str(), "context_provider");
        assert_eq!(PluginCategory::ActionHandler.as_str(), "action_handler");
    }

    #[test]
    fn test_plugin_metadata_builder() {
        let meta = PluginMetadata::new("test-plugin", "1.0.0", PluginCategory::ContextProvider)
            .description("A test plugin")
            .supports_formats(vec!["docx", "xlsx"])
            .with_features(vec!["extract", "chunk"]);

        assert_eq!(meta.name, "test-plugin");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.category, PluginCategory::ContextProvider);
        assert_eq!(meta.supported_formats, vec!["docx", "xlsx"]);
    }

    #[test]
    fn test_plugin_input_builder() {
        let input = PluginInput::new()
            .with_path(PathBuf::from("/test/file.docx"))
            .with_query("extract dates")
            .with_options(serde_json::json!({"max_chunks": 10}));

        assert_eq!(input.path, Some(PathBuf::from("/test/file.docx")));
        assert_eq!(input.query, Some("extract dates".to_string()));
    }

    #[test]
    fn test_plugin_output_builder() {
        let output = PluginOutput::new("extracted content")
            .with_metadata(serde_json::json!({"format": "docx"}))
            .with_chunks(vec![
                ContentChunk::new("chunk 1", "paragraph", 0),
                ContentChunk::new("chunk 2", "paragraph", 1),
            ]);

        assert_eq!(output.content, "extracted content");
        assert_eq!(output.chunks.len(), 2);
    }
}
