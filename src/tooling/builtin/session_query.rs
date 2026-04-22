//! Session query tool — search session history
//!
//! Allows agents to query Impulse's session history for cross-session
//! awareness: finding past sessions, reviewing what files were touched,
//! what tools were used, and what was accomplished.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Query session history from .impulse/HISTORY.jsonl.
///
/// Enables agents to search past sessions by name, platform, or time range,
/// providing cross-session memory that persists across conversations.
pub struct SessionQueryTool;

#[async_trait]
impl DynamicTool for SessionQueryTool {
    fn id(&self) -> &str {
        "session_query"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "session_query".into(),
            name: "Session Query".into(),
            description: "Search session history for cross-session awareness".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "query".into(),
                    description: "Text to search for in session names and summaries".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "limit".into(),
                    description: "Maximum sessions to return (default: 10)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(10)),
                },
                ToolParam {
                    name: "impulse_dir".into(),
                    description: "Path to .impulse directory (default: .impulse)".into(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                },
            ],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(()) // All params optional
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let impulse_dir = params
            .get("impulse_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".impulse");
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let history_path = std::path::PathBuf::from(impulse_dir).join("HISTORY.jsonl");

        if !history_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "sessions": [],
                "total": 0,
                "message": "No history file found"
            })));
        }

        let content = std::fs::read_to_string(&history_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read history: {}", e)))?;

        let mut entries: Vec<serde_json::Value> = content
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect();

        // Filter by query if provided
        if !query.is_empty() {
            let q = query.to_lowercase();
            entries.retain(|entry| {
                let name = entry
                    .get("session_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let summary = entry.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                name.to_lowercase().contains(&q) || summary.to_lowercase().contains(&q)
            });
        }

        // Most recent first
        entries.reverse();
        let total = entries.len();
        entries.truncate(limit);

        Ok(ToolResult::json(serde_json::json!({
            "sessions": entries,
            "total": total,
            "returned": entries.len(),
            "query": query,
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
        let tool = SessionQueryTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "session_query");
        assert_eq!(desc.category, ToolCategory::Analysis);
    }

    #[tokio::test]
    async fn test_execute_no_history() {
        let tool = SessionQueryTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"impulse_dir": "/tmp/nonexistent_impulse_xyz"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["total"], 0);
    }
}
