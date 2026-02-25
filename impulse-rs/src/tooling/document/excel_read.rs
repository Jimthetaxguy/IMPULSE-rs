//! Excel read tool — wraps office::excel for spreadsheet parsing

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Read Excel/CSV files and return structured data (sheets, rows, columns).
///
/// Supports: .xlsx, .xls, .csv
/// Delegates to office::excel::parse_excel for actual parsing.
pub struct ExcelReadTool;

#[async_trait]
impl DynamicTool for ExcelReadTool {
    fn id(&self) -> &str {
        "excel_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "excel_read".into(),
            name: "Excel Read".into(),
            description: "Read Excel/CSV files and extract sheet data as structured JSON".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Document,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Path to the Excel/CSV file".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "sheet".into(),
                    description: "Specific sheet name to read (reads all if omitted)".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => {
                let path = PathBuf::from(p);
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext.to_lowercase().as_str(), "xlsx" | "xls" | "csv") {
                    return Err(ToolError::InvalidParams(format!(
                        "Unsupported format: .{} (expected .xlsx, .xls, or .csv)",
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

        match crate::office::excel::parse_excel(&path) {
            Ok(result) => {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("format".to_string(), result.metadata.format.clone());
                metadata.insert(
                    "size_bytes".to_string(),
                    result.metadata.size_bytes.to_string(),
                );
                metadata.insert("chunk_count".to_string(), result.chunks.len().to_string());

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
        let tool = ExcelReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "excel_read");
        assert_eq!(desc.category, ToolCategory::Document);
        assert_eq!(desc.params.len(), 2);
    }

    #[test]
    fn test_validate_xlsx() {
        let tool = ExcelReadTool;
        let params = serde_json::json!({"path": "test.xlsx"});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_csv() {
        let tool = ExcelReadTool;
        let params = serde_json::json!({"path": "data.csv"});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_unsupported() {
        let tool = ExcelReadTool;
        let params = serde_json::json!({"path": "file.pdf"});
        assert!(tool.validate_params(&params).is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_path_does_not_panic() {
        let tool = ExcelReadTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidParams(_) => {}
            other => panic!("Expected InvalidParams, got: {:?}", other),
        }
    }
}
