// Plugin registry for dynamic handler registration and lookup

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::{PluginError, PluginInput, PluginMetadata, PluginOutput, PluginResult};
use crate::office::OfficeFormat;

/// Trait for context provider plugins (e.g., Office documents, databases)
pub trait ContextProvider: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;

    fn formats(&self) -> &[OfficeFormat];

    fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput>;

    fn extract_smart(&self, path: &std::path::Path, query: &str) -> PluginResult<String>;

    fn chunk(&self, content: &str, size: usize) -> Vec<super::ContentChunk>;

    fn metadata_info(&self, path: &std::path::Path) -> PluginResult<serde_json::Value>;
}

/// Trait for action handler plugins (e.g., deploy, notify)
pub trait ActionHandler: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;

    fn execute(&self, input: &PluginInput) -> PluginResult<PluginOutput>;

    fn validate(&self, input: &PluginInput) -> PluginResult<()>;

    fn rollback(&self, execution_id: &str) -> PluginResult<()>;
}

/// Registry for all plugins in the system
#[derive(Clone)]
pub struct PluginRegistry {
    context_providers: Arc<RwLock<HashMap<OfficeFormat, Arc<dyn ContextProvider>>>>,
    action_handlers: Arc<RwLock<HashMap<String, Arc<dyn ActionHandler>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            context_providers: Arc::new(RwLock::new(HashMap::new())),
            action_handlers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_context_provider(
        &self,
        format: OfficeFormat,
        provider: Arc<dyn ContextProvider>,
    ) {
        let mut providers = self.context_providers.write().unwrap();
        providers.insert(format, provider);
    }

    pub fn register_action_handler(&self, name: &str, handler: Arc<dyn ActionHandler>) {
        let mut handlers = self.action_handlers.write().unwrap();
        handlers.insert(name.to_string(), handler);
    }

    pub fn get_context_provider(&self, format: OfficeFormat) -> Option<Arc<dyn ContextProvider>> {
        let providers = self.context_providers.read().unwrap();
        providers.get(&format).cloned()
    }

    pub fn get_action_handler(&self, name: &str) -> Option<Arc<dyn ActionHandler>> {
        let handlers = self.action_handlers.read().unwrap();
        handlers.get(name).cloned()
    }

    pub fn list_context_formats(&self) -> Vec<OfficeFormat> {
        let providers = self.context_providers.read().unwrap();
        providers.keys().copied().collect()
    }

    pub fn list_action_handlers(&self) -> Vec<String> {
        let handlers = self.action_handlers.read().unwrap();
        handlers.keys().cloned().collect()
    }

    pub fn list_context_providers(&self) -> Vec<PluginMetadata> {
        let providers = self.context_providers.read().unwrap();
        providers.values().map(|p| p.metadata().clone()).collect()
    }

    pub fn list_action_handlers_metadata(&self) -> Vec<PluginMetadata> {
        let handlers = self.action_handlers.read().unwrap();
        handlers.values().map(|h| h.metadata().clone()).collect()
    }

    pub fn supports_format(&self, format: OfficeFormat) -> bool {
        let providers = self.context_providers.read().unwrap();
        providers.contains_key(&format)
    }

    pub fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| PluginError::UnsupportedFormat("No file extension".to_string()))?;

        let format = OfficeFormat::from_extension(ext);

        let provider = self
            .get_context_provider(format)
            .ok_or_else(|| PluginError::NotFound(format!("No provider for format: {}", ext)))?;

        provider.extract(path)
    }

    pub fn extract_smart(&self, path: &std::path::Path, query: &str) -> PluginResult<String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| PluginError::UnsupportedFormat("No file extension".to_string()))?;

        let format = OfficeFormat::from_extension(ext);

        let provider = self
            .get_context_provider(format)
            .ok_or_else(|| PluginError::NotFound(format!("No provider for format: {}", ext)))?;

        provider.extract_smart(path, query)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static REGISTRY: std::sync::OnceLock<PluginRegistry> = std::sync::OnceLock::new();

pub fn global_registry() -> &'static PluginRegistry {
    REGISTRY.get_or_init(PluginRegistry::new)
}

pub fn init_global_registry() -> &'static PluginRegistry {
    let registry = global_registry();

    #[cfg(feature = "office-support")]
    {
        use crate::office;
        use std::sync::Arc;

        struct OfficeContextProvider {
            metadata: PluginMetadata,
        }

        impl OfficeContextProvider {
            fn new() -> Self {
                Self {
                    metadata: PluginMetadata::new(
                        "office",
                        "1.0.0",
                        crate::plugin::PluginCategory::ContextProvider,
                    )
                    .description("Office document parser for DOCX, XLSX, CSV")
                    .supports_formats(vec!["docx", "xlsx", "xls", "csv"])
                    .with_features(vec![
                        "parse",
                        "extract",
                        "chunk",
                        "smart-extract",
                    ]),
                }
            }
        }

        impl ContextProvider for OfficeContextProvider {
            fn metadata(&self) -> &PluginMetadata {
                &self.metadata
            }

            fn formats(&self) -> &[OfficeFormat] {
                &[
                    OfficeFormat::Docx,
                    OfficeFormat::Xlsx,
                    OfficeFormat::Xls,
                    OfficeFormat::Csv,
                ]
            }

            fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput> {
                let result =
                    office::parse_document(path).map_err(|e| PluginError::ExecutionFailed(e))?;

                Ok(PluginOutput::new(result.content)
                    .with_metadata(serde_json::json!({
                        "document_type": result.document_type,
                        "format": result.metadata.format,
                    }))
                    .with_chunks(
                        result
                            .chunks
                            .into_iter()
                            .map(|c| super::ContentChunk {
                                content: c.content,
                                chunk_type: c.chunk_type,
                                index: c.index,
                            })
                            .collect(),
                    ))
            }

            fn extract_smart(&self, path: &std::path::Path, query: &str) -> PluginResult<String> {
                let target = office::extraction::create_extraction_target(path, query)
                    .map_err(|e| PluginError::ExecutionFailed(e))?;
                Ok(target.content)
            }

            fn chunk(&self, content: &str, size: usize) -> Vec<super::ContentChunk> {
                content
                    .split('\n')
                    .collect::<Vec<_>>()
                    .chunks(size)
                    .enumerate()
                    .map(|(i, chunk)| super::ContentChunk::new(chunk.join("\n"), "paragraph", i))
                    .collect()
            }

            fn metadata_info(&self, path: &std::path::Path) -> PluginResult<serde_json::Value> {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

                let format = OfficeFormat::from_extension(ext);

                Ok(serde_json::json!({
                    "format": format.as_str(),
                    "readable": format.is_readable(),
                    "writable": format.is_writable(),
                }))
            }
        }

        for format in [
            OfficeFormat::Docx,
            OfficeFormat::Xlsx,
            OfficeFormat::Xls,
            OfficeFormat::Csv,
        ] {
            registry.register_context_provider(format, Arc::new(OfficeContextProvider::new()));
        }
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = PluginRegistry::new();
        assert!(registry.list_context_formats().is_empty());
        assert!(registry.list_action_handlers().is_empty());
    }

    #[test]
    fn test_format_support() {
        let registry = PluginRegistry::new();
        assert!(!registry.supports_format(OfficeFormat::Docx));
    }
}
