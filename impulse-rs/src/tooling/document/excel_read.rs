//! Excel read tool — wraps office::excel for spreadsheet parsing

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Read Excel/CSV files and return structured data (sheets, rows, columns).
///
/// Supports: .xlsx, .csv (legacy .xls is refused)
/// Delegates to office::excel::parse_excel, which reads under the bounds in
/// office::bounded, on the blocking pool.
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
                match ext.to_lowercase().as_str() {
                    "xlsx" | "csv" => Ok(()),
                    "xls" => Err(ToolError::InvalidParams(
                        "legacy .xls workbooks are not read: their binary format cannot be \
                         bounded before parsing; convert to .xlsx"
                            .into(),
                    )),
                    _ => Err(ToolError::InvalidParams(format!(
                        "Unsupported format: .{} (expected .xlsx or .csv)",
                        ext
                    ))),
                }
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

        // Parsing is synchronous, so it runs on the blocking pool; the office
        // bounds cap how long it can take.
        let parsed = tokio::task::spawn_blocking(move || crate::office::excel::parse_excel(&path))
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("excel_read parse task failed: {e}"))
            })?;
        match parsed {
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

    #[test]
    fn test_validate_refuses_legacy_xls() {
        let tool = ExcelReadTool;
        let params = serde_json::json!({"path": "old.XLS"});
        match tool.validate_params(&params) {
            Err(ToolError::InvalidParams(message)) => {
                assert!(message.contains("legacy .xls"), "{message}")
            }
            other => panic!("expected InvalidParams, got {other:?}"),
        }
    }

    /// The parse runs on the blocking pool, so the async runtime keeps
    /// serving other tasks while a workbook is read.
    #[tokio::test]
    async fn test_execute_parses_off_the_async_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let (result, turns) = super::super::test_support::turns_while_executing(
            &ExcelReadTool,
            serde_json::json!({"path": path}),
        )
        .await;
        assert_eq!(result.unwrap().output["content"], "a,b\n1,2\n");
        assert!(turns > 0, "the parse ran on the async runtime thread");
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
