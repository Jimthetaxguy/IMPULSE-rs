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
    ) -> PluginResult<()> {
        let mut providers = self
            .context_providers
            .write()
            .map_err(|_| PluginError::Internal("context_providers lock poisoned".to_string()))?;
        providers.insert(format, provider);
        Ok(())
    }

    pub fn register_action_handler(
        &self,
        name: &str,
        handler: Arc<dyn ActionHandler>,
    ) -> PluginResult<()> {
        let mut handlers = self
            .action_handlers
            .write()
            .map_err(|_| PluginError::Internal("action_handlers lock poisoned".to_string()))?;
        handlers.insert(name.to_string(), handler);
        Ok(())
    }

    pub fn get_context_provider(
        &self,
        format: OfficeFormat,
    ) -> PluginResult<Option<Arc<dyn ContextProvider>>> {
        let providers = self
            .context_providers
            .read()
            .map_err(|_| PluginError::Internal("context_providers lock poisoned".to_string()))?;
        Ok(providers.get(&format).cloned())
    }

    pub fn get_action_handler(&self, name: &str) -> PluginResult<Option<Arc<dyn ActionHandler>>> {
        let handlers = self
            .action_handlers
            .read()
            .map_err(|_| PluginError::Internal("action_handlers lock poisoned".to_string()))?;
        Ok(handlers.get(name).cloned())
    }

    pub fn list_context_formats(&self) -> PluginResult<Vec<OfficeFormat>> {
        let providers = self
            .context_providers
            .read()
            .map_err(|_| PluginError::Internal("context_providers lock poisoned".to_string()))?;
        Ok(providers.keys().copied().collect())
    }

    pub fn list_action_handlers(&self) -> PluginResult<Vec<String>> {
        let handlers = self
            .action_handlers
            .read()
            .map_err(|_| PluginError::Internal("action_handlers lock poisoned".to_string()))?;
        Ok(handlers.keys().cloned().collect())
    }

    pub fn list_context_providers(&self) -> PluginResult<Vec<PluginMetadata>> {
        let providers = self
            .context_providers
            .read()
            .map_err(|_| PluginError::Internal("context_providers lock poisoned".to_string()))?;
        Ok(providers.values().map(|p| p.metadata().clone()).collect())
    }

    pub fn list_action_handlers_metadata(&self) -> PluginResult<Vec<PluginMetadata>> {
        let handlers = self
            .action_handlers
            .read()
            .map_err(|_| PluginError::Internal("action_handlers lock poisoned".to_string()))?;
        Ok(handlers.values().map(|h| h.metadata().clone()).collect())
    }

    pub fn supports_format(&self, format: OfficeFormat) -> PluginResult<bool> {
        let providers = self
            .context_providers
            .read()
            .map_err(|_| PluginError::Internal("context_providers lock poisoned".to_string()))?;
        Ok(providers.contains_key(&format))
    }

    pub fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| PluginError::UnsupportedFormat("No file extension".to_string()))?;

        let format = OfficeFormat::from_extension(ext);

        let provider = self
            .get_context_provider(format)?
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
            .get_context_provider(format)?
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
                // Legacy `.xls` is not listed: `office::parse_document`
                // refuses it, since it cannot be read under the office bounds.
                &[OfficeFormat::Docx, OfficeFormat::Xlsx, OfficeFormat::Csv]
            }

            fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput> {
                let result = office::parse_document(path).map_err(PluginError::ExecutionFailed)?;

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
                    .map_err(PluginError::ExecutionFailed)?;
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

        // Registered for the formats the provider declares, so the
        // registration and the declaration cannot drift apart.
        let provider = OfficeContextProvider::new();
        for &format in provider.formats() {
            if let Err(err) =
                registry.register_context_provider(format, Arc::new(OfficeContextProvider::new()))
            {
                tracing::error!(
                    "failed to register office context provider for format {}: {}",
                    format.as_str(),
                    err
                );
            }
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
        assert!(registry.list_context_formats().unwrap().is_empty());
        assert!(registry.list_action_handlers().unwrap().is_empty());
    }

    #[test]
    fn test_format_support() {
        let registry = PluginRegistry::new();
        assert!(!registry.supports_format(OfficeFormat::Docx).unwrap());
    }

    /// Returns `label` from both extraction calls and records what it was
    /// asked for, so a test can see which provider ran and with what.
    struct LabelProvider {
        label: &'static str,
        metadata: PluginMetadata,
        seen: std::sync::Mutex<Vec<(std::path::PathBuf, Option<String>)>>,
    }

    impl LabelProvider {
        fn new(label: &'static str) -> Arc<Self> {
            Arc::new(Self {
                label,
                metadata: PluginMetadata::new(
                    label,
                    "0.0.0",
                    super::super::PluginCategory::ContextProvider,
                ),
                seen: std::sync::Mutex::new(Vec::new()),
            })
        }
    }

    impl ContextProvider for LabelProvider {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }
        fn formats(&self) -> &[OfficeFormat] {
            &[]
        }
        fn extract(&self, path: &std::path::Path) -> PluginResult<PluginOutput> {
            self.seen.lock().unwrap().push((path.to_path_buf(), None));
            Ok(PluginOutput::new(self.label))
        }
        fn extract_smart(&self, path: &std::path::Path, query: &str) -> PluginResult<String> {
            self.seen
                .lock()
                .unwrap()
                .push((path.to_path_buf(), Some(query.to_string())));
            Ok(format!("{}:{query}", self.label))
        }
        fn chunk(&self, _content: &str, _size: usize) -> Vec<super::super::ContentChunk> {
            Vec::new()
        }
        fn metadata_info(&self, _path: &std::path::Path) -> PluginResult<serde_json::Value> {
            Ok(serde_json::json!({}))
        }
    }

    #[test]
    fn test_extract_without_an_extension_is_unsupported() {
        let registry = PluginRegistry::new();
        for result in [
            registry
                .extract(std::path::Path::new("document"))
                .map(|_| ()),
            registry
                .extract_smart(std::path::Path::new("document"), "q")
                .map(|_| ()),
        ] {
            assert!(matches!(result, Err(PluginError::UnsupportedFormat(_))));
        }
    }

    #[test]
    fn test_extract_with_no_provider_for_the_format_is_not_found() {
        let registry = PluginRegistry::new();
        match registry.extract(std::path::Path::new("notes.txt")) {
            Err(PluginError::NotFound(message)) => assert!(message.contains("txt"), "{message}"),
            other => panic!("expected NotFound, got {:?}", other.map(|o| o.content)),
        }
    }

    #[test]
    fn test_extract_routes_by_extension_case_insensitively() {
        let registry = PluginRegistry::new();
        let xlsx = LabelProvider::new("xlsx");
        registry
            .register_context_provider(OfficeFormat::Xlsx, xlsx.clone())
            .unwrap();
        let path = std::path::Path::new("REPORT.XLSX");
        assert_eq!(registry.extract(path).unwrap().content, "xlsx");
        assert_eq!(xlsx.seen.lock().unwrap()[0].0, path);
        assert!(registry.supports_format(OfficeFormat::Xlsx).unwrap());
        assert_eq!(
            registry.list_context_formats().unwrap(),
            vec![OfficeFormat::Xlsx]
        );
    }

    #[test]
    fn test_extract_smart_forwards_the_path_and_query() {
        let registry = PluginRegistry::new();
        let docx = LabelProvider::new("docx");
        registry
            .register_context_provider(OfficeFormat::Docx, docx.clone())
            .unwrap();
        let path = std::path::Path::new("brief.docx");
        assert_eq!(
            registry.extract_smart(path, "total revenue").unwrap(),
            "docx:total revenue"
        );
        assert_eq!(
            docx.seen.lock().unwrap()[0],
            (path.to_path_buf(), Some("total revenue".to_string()))
        );
    }

    /// Pins today's behavior: a second registration for a format replaces
    /// the first (HashMap::insert), unlike ToolRegistry, which refuses
    /// duplicates. Only the daemon's init registers providers, so nothing
    /// relies on refusal yet; change this test if that changes.
    #[test]
    fn test_registering_a_format_twice_replaces_the_first_provider() {
        let registry = PluginRegistry::new();
        registry
            .register_context_provider(OfficeFormat::Csv, LabelProvider::new("first"))
            .unwrap();
        registry
            .register_context_provider(OfficeFormat::Csv, LabelProvider::new("second"))
            .unwrap();
        let output = registry.extract(std::path::Path::new("data.csv")).unwrap();
        assert_eq!(output.content, "second");
        assert_eq!(registry.list_context_providers().unwrap().len(), 1);
    }

    #[test]
    fn test_a_clone_shares_registrations_with_the_original() {
        let original = PluginRegistry::new();
        let clone = original.clone();
        clone
            .register_context_provider(OfficeFormat::Docx, LabelProvider::new("shared"))
            .unwrap();
        assert!(original.supports_format(OfficeFormat::Docx).unwrap());
        assert_eq!(
            original
                .extract(std::path::Path::new("a.docx"))
                .unwrap()
                .content,
            "shared"
        );
    }
}
