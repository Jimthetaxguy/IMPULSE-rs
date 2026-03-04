//! Memory search tool — query GENOME and history via retrieval index
//!
//! Allows agents to search Impulse's persistent memory (GENOME.md decisions
//! and session history) using the retrieval index. Supports keyword and
//! semantic search modes.

use async_trait::async_trait;

use crate::retrieval;
use crate::retrieval::types::{RetrievalMode, SearchBackend};
use crate::state::Config;
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Search Impulse's persistent memory (GENOME and history).
///
/// Agents can search for past decisions, patterns, and session context
/// using keyword or semantic search against the retrieval index.
pub struct MemorySearchTool;

#[async_trait]
impl DynamicTool for MemorySearchTool {
    fn id(&self) -> &str {
        "memory_search"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "memory_search".into(),
            name: "Memory Search".into(),
            description: "Search GENOME decisions and session history via retrieval index".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "query".into(),
                    description: "Search query text".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "scope".into(),
                    description: "What to search: 'history', 'genome', or 'all' (default: all)"
                        .into(),
                    param_type: ParamType::String,
                    required: false,
                    default: Some(serde_json::json!("all")),
                },
                ToolParam {
                    name: "mode".into(),
                    description: "Search mode: 'keyword' or 'semantic' (default: keyword)".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: Some(serde_json::json!("keyword")),
                },
                ToolParam {
                    name: "limit".into(),
                    description: "Maximum results (default: 5)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(5)),
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

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams("missing or empty 'query'".into())),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'query'".into()))?;
        let scope = params
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("all");
        let mode_str = params
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("keyword");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let impulse_dir = params
            .get("impulse_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".impulse");

        let base_path = std::path::PathBuf::from(impulse_dir);

        if !base_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "results": [],
                "error": "Impulse directory not found",
            })));
        }

        let mode = match mode_str {
            "semantic" => Some(RetrievalMode::Semantic),
            _ => Some(RetrievalMode::Keyword),
        };
        let backend = Some(SearchBackend::Auto);
        let config = Config::default();

        let mut all_results = Vec::new();

        if scope == "history" || scope == "all" {
            match retrieval::search_history(
                &base_path,
                &config,
                query,
                mode,
                backend,
                Some(limit),
                None,
            ) {
                Ok(response) => {
                    for result in response.results {
                        all_results.push(serde_json::json!({
                            "source": "history",
                            "id": result.id,
                            "title": result.title,
                            "snippet": result.snippet,
                            "score": result.score,
                        }));
                    }
                }
                Err(e) => {
                    all_results.push(serde_json::json!({
                        "source": "history",
                        "error": e.to_string(),
                    }));
                }
            }
        }

        if scope == "genome" || scope == "all" {
            match retrieval::search_genome(
                &base_path,
                &config,
                query,
                mode,
                backend,
                Some(limit),
                None,
            ) {
                Ok(response) => {
                    for result in response.results {
                        all_results.push(serde_json::json!({
                            "source": "genome",
                            "id": result.id,
                            "title": result.title,
                            "snippet": result.snippet,
                            "score": result.score,
                        }));
                    }
                }
                Err(e) => {
                    all_results.push(serde_json::json!({
                        "source": "genome",
                        "error": e.to_string(),
                    }));
                }
            }
        }

        // Sort by score descending
        all_results.sort_by(|a, b| {
            let sa = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let sb = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(limit);
        let count = all_results.len();

        Ok(ToolResult::json(serde_json::json!({
            "query": query,
            "scope": scope,
            "mode": mode_str,
            "results": all_results,
            "count": count,
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
        let tool = MemorySearchTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "memory_search");
        assert_eq!(desc.category, ToolCategory::Analysis);
        assert_eq!(desc.params.len(), 5);
    }

    #[test]
    fn test_validate_ok() {
        let tool = MemorySearchTool;
        assert!(tool
            .validate_params(&serde_json::json!({"query": "auth"}))
            .is_ok());
    }

    #[test]
    fn test_validate_missing_query() {
        let tool = MemorySearchTool;
        assert!(tool.validate_params(&serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn test_execute_no_impulse() {
        let tool = MemorySearchTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({
                    "query": "auth",
                    "impulse_dir": "/tmp/nonexistent_impulse_xyz"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.get("error").is_some() || result.output["count"] == 0);
    }
}
