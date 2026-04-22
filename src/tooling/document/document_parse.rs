//! Generic document parse tool — auto-detects format and delegates to office::parse_document

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Parses any supported Office document (XLSX, XLS, CSV, DOCX) and returns extracted content.
///
/// This is the main entry point for document processing — it auto-detects format
/// from file extension and delegates to the appropriate parser.
pub struct DocumentParseTool;

#[async_trait]
impl DynamicTool for DocumentParseTool {
    fn id(&self) -> &str {
        "document_parse"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "document_parse".into(),
            name: "Document Parse".into(),
            description: "Parse Office documents (XLSX, XLS, CSV, DOCX) and extract text content"
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::Document,
            params: vec![ToolParam {
                name: "path".into(),
                description: "Path to the document file".into(),
                param_type: ParamType::FilePath,
                required: true,
                default: None,
            }],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => {
                let path = PathBuf::from(p);
                if !path.exists() {
                    return Err(ToolError::InvalidParams(format!("File not found: {}", p)));
                }
                Ok(())
            }
            _ => Err(ToolError::InvalidParams(
                "missing or empty 'path' string".into(),
            )),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParams("missing 'path' string parameter".into()))?;
        let path = PathBuf::from(path_str);

        match crate::office::parse_document(&path) {
            Ok(result) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("document_type".to_string(), result.document_type.clone());
                metadata.insert("format".to_string(), result.metadata.format.clone());
                metadata.insert(
                    "size_bytes".to_string(),
                    result.metadata.size_bytes.to_string(),
                );
                metadata.insert("chunk_count".to_string(), result.chunks.len().to_string());

                Ok(ToolResult {
                    output: serde_json::json!({
                        "document_type": result.document_type,
                        "content": result.content,
                        "metadata": {
                            "source_path": result.metadata.source_path,
                            "format": result.metadata.format,
                            "size_bytes": result.metadata.size_bytes,
                            "extracted_at": result.metadata.extracted_at,
                        },
                        "chunks": result.chunks.iter().map(|c| {
                            serde_json::json!({
                                "content": c.content,
                                "chunk_type": c.chunk_type,
                                "index": c.index,
                            })
                        }).collect::<Vec<_>>(),
                    }),
                    artifacts: vec![],
                    metadata,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed(e)),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = DocumentParseTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "document_parse");
        assert_eq!(desc.category, ToolCategory::Document);
        assert_eq!(desc.params.len(), 1);
        assert!(desc.params[0].required);
    }

    #[test]
    fn test_validate_missing_path() {
        let tool = DocumentParseTool;
        let params = serde_json::json!({});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_empty_path() {
        let tool = DocumentParseTool;
        let params = serde_json::json!({"path": ""});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_nonexistent_file() {
        let tool = DocumentParseTool;
        let params = serde_json::json!({"path": "/nonexistent/file.xlsx"});
        assert!(tool.validate_params(&params).is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_path_does_not_panic() {
        let tool = DocumentParseTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidParams(_) => {}
            other => panic!("Expected InvalidParams, got: {:?}", other),
        }
    }
}
