//! Word read tool — wraps office::word for DOCX parsing

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Read Word documents (.docx) and extract text, paragraphs, and tables.
///
/// Delegates to office::word::parse_word for actual parsing.
pub struct WordReadTool;

#[async_trait]
impl DynamicTool for WordReadTool {
    fn id(&self) -> &str {
        "word_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "word_read".into(),
            name: "Word Read".into(),
            description: "Read Word documents (.docx) and extract text content".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Document,
            params: vec![ToolParam {
                name: "path".into(),
                description: "Path to the .docx file".into(),
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
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.to_lowercase() != "docx" {
                    return Err(ToolError::InvalidParams(format!(
                        "Expected .docx file, got .{}",
                        ext
                    )));
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

        match crate::office::word::parse_word(&path) {
            Ok(result) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert(
                    "word_count".to_string(),
                    result.content.split_whitespace().count().to_string(),
                );
                metadata.insert(
                    "paragraph_count".to_string(),
                    result.chunks.len().to_string(),
                );

                Ok(ToolResult {
                    output: serde_json::json!({
                        "content": result.content,
                        "metadata": {
                            "source_path": result.metadata.source_path,
                            "format": result.metadata.format,
                            "size_bytes": result.metadata.size_bytes,
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
        let tool = WordReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "word_read");
        assert_eq!(desc.category, ToolCategory::Document);
    }

    #[test]
    fn test_validate_docx() {
        let tool = WordReadTool;
        let params = serde_json::json!({"path": "report.docx"});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_wrong_format() {
        let tool = WordReadTool;
        let params = serde_json::json!({"path": "data.xlsx"});
        assert!(tool.validate_params(&params).is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_path_does_not_panic() {
        let tool = WordReadTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidParams(_) => {}
            other => panic!("Expected InvalidParams, got: {:?}", other),
        }
    }
}
