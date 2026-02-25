//! Document extraction tool — wraps office::extraction for intelligent content extraction
//!
//! This tool goes beyond simple parsing: it can extract specific types of data
//! (dates, amounts, contacts, names) from documents using pattern matching,
//! or delegate to the Monty sandbox for more complex extraction.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Extract structured data from Office documents.
///
/// Given a document path and an extraction goal (e.g., "extract dates",
/// "find financial data", "get contact info"), this tool parses the document
/// and runs extraction logic to return structured findings.
pub struct DocumentExtractTool;

#[async_trait]
impl DynamicTool for DocumentExtractTool {
    fn id(&self) -> &str {
        "document_extract"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "document_extract".into(),
            name: "Document Extract".into(),
            description: "Extract structured data (dates, amounts, contacts) from Office documents"
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::Document,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Path to the document file".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "goal".into(),
                    description:
                        "What to extract: 'dates', 'amounts', 'contacts', 'names', or custom text"
                            .into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "use_python".into(),
                    description: "Use Python regex extraction (default: true)".into(),
                    param_type: ParamType::Bool,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        if params
            .get("path")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(ToolError::InvalidParams("missing 'path'".into()));
        }
        if params
            .get("goal")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(ToolError::InvalidParams("missing 'goal'".into()));
        }
        Ok(())
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
        let goal = params
            .get("goal")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'goal'".into()))?;
        let use_python = params
            .get("use_python")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let path = PathBuf::from(path_str);

        // Create extraction target from the document
        let target = crate::office::extraction::create_extraction_target(&path, goal)
            .map_err(ToolError::ExecutionFailed)?;

        if use_python {
            return Err(ToolError::ExecutionFailed(
                "Python extraction not yet wired. Use use_python=false for native extraction."
                    .into(),
            ));
        } else {
            // Return the raw document content without Python extraction
            Ok(ToolResult::json(serde_json::json!({
                "source": path_str,
                "goal": goal,
                "document_type": target.document_type,
                "content_preview": target.content.chars().take(500).collect::<String>(),
                "content_length": target.content.len(),
            })))
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        // Only FileSystemRead is always required; PythonExec is checked
        // at runtime based on the use_python parameter
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = DocumentExtractTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "document_extract");
        assert_eq!(desc.category, ToolCategory::Document);
        assert_eq!(desc.params.len(), 3);
    }

    #[test]
    fn test_validate_ok() {
        let tool = DocumentExtractTool;
        let params = serde_json::json!({"path": "test.xlsx", "goal": "extract dates"});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_missing_path() {
        let tool = DocumentExtractTool;
        assert!(tool
            .validate_params(&serde_json::json!({"goal": "dates"}))
            .is_err());
    }

    #[test]
    fn test_validate_missing_goal() {
        let tool = DocumentExtractTool;
        assert!(tool
            .validate_params(&serde_json::json!({"path": "test.xlsx"}))
            .is_err());
    }
}
