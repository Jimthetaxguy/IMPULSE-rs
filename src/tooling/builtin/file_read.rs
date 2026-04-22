//! File read tool — safe file reading with size limits
//!
//! Provides controlled file reading for agents with size limits
//! and line-range selection. Prevents accidental large file reads
//! that could overwhelm context windows.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

const MAX_FILE_BYTES: usize = 1_000_000; // 1MB hard limit

/// Read a file with size limits and optional line range.
///
/// Designed for agents that need to read source code, config files,
/// or data files without risking context window overflow.
pub struct FileReadTool;

#[async_trait]
impl DynamicTool for FileReadTool {
    fn id(&self) -> &str {
        "file_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "file_read".into(),
            name: "File Read".into(),
            description: "Read a file with size limits and optional line range".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Path to the file to read".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "start_line".into(),
                    description: "First line to read (1-based, default: 1)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(1)),
                },
                ToolParam {
                    name: "max_lines".into(),
                    description: "Maximum lines to return (default: 200)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(200)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams("missing 'path'".into())),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'path'".into()))?;
        let start_line = params
            .get("start_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let max_lines = params
            .get("max_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as usize;

        let path = std::path::PathBuf::from(path_str);

        if !path.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "File not found: {}",
                path_str
            )));
        }

        // Check file size first
        let metadata = std::fs::metadata(&path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot stat file: {}", e)))?;

        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Ok(ToolResult::json(serde_json::json!({
                "path": path_str,
                "error": "File too large",
                "size_bytes": metadata.len(),
                "max_bytes": MAX_FILE_BYTES,
                "hint": "Use start_line and max_lines to read a portion"
            })));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        // Collect once to avoid double traversal
        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len();
        let end = (start_line - 1 + max_lines).min(total_lines);
        let lines = &all_lines[(start_line - 1).min(total_lines)..end];
        let truncated = end < total_lines;

        Ok(ToolResult::json(serde_json::json!({
            "path": path_str,
            "content": lines.join("\n"),
            "start_line": start_line,
            "lines_returned": lines.len(),
            "total_lines": total_lines,
            "truncated": truncated,
            "size_bytes": metadata.len(),
        })))
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
        let tool = FileReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "file_read");
        assert_eq!(desc.category, ToolCategory::Utility);
        assert_eq!(desc.params.len(), 3);
    }

    #[test]
    fn test_validate_ok() {
        let tool = FileReadTool;
        assert!(tool
            .validate_params(&serde_json::json!({"path": "Cargo.toml"}))
            .is_ok());
    }

    #[test]
    fn test_validate_missing() {
        let tool = FileReadTool;
        assert!(tool.validate_params(&serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn test_execute_not_found() {
        let tool = FileReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"path": "/tmp/nonexistent_file_xyz.txt"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_cargo_toml() {
        let tool = FileReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"path": "Cargo.toml", "max_lines": 5}),
                &ctx,
            )
            .await;
        if let Ok(r) = result {
            assert!(r.output["lines_returned"].as_u64().unwrap() <= 5);
        }
    }
}
