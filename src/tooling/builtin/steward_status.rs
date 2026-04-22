//! Stewardship status tool — check context stewardship state
//!
//! Reports on the stewardship system's configuration, pending proposals,
//! and cross-project patterns. Useful for agents to understand context
//! usage and available optimizations.

use async_trait::async_trait;

use crate::stewardship;
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Check context stewardship status: mode, thresholds, proposals.
pub struct StewardStatusTool;

#[async_trait]
impl DynamicTool for StewardStatusTool {
    fn id(&self) -> &str {
        "steward_status"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "steward_status".into(),
            name: "Steward Status".into(),
            description:
                "Check context stewardship config, pending proposals, and cross-project patterns"
                    .into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![ToolParam {
                name: "impulse_dir".into(),
                description: "Path to .impulse directory (default: .impulse)".into(),
                param_type: ParamType::FilePath,
                required: false,
                default: None,
            }],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
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
        let base = std::path::PathBuf::from(impulse_dir);

        let stew_config = stewardship::StewardshipConfig::default();
        let proposals = stewardship::approval::list_pending(&base).unwrap_or_default();
        let cross = stewardship::cross_project::load_cross_project(&base).unwrap_or_default();

        Ok(ToolResult::json(serde_json::json!({
            "mode": stew_config.mode.as_str(),
            "thresholds": {
                "monitor": stew_config.monitor_threshold,
                "surgical": stew_config.surgical_threshold,
                "thoughtful": stew_config.thoughtful_threshold,
                "emergency": stew_config.emergency_threshold,
            },
            "context_window_tokens": stew_config.context_window_tokens,
            "pending_proposals": proposals.len(),
            "cross_project": {
                "patterns": cross.patterns.len(),
                "learnings": cross.learnings.len(),
                "total_sessions_analyzed": cross.stats.total_sessions_analyzed,
            },
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
        let tool = StewardStatusTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "steward_status");
        assert_eq!(desc.category, ToolCategory::System);
    }

    #[tokio::test]
    async fn test_execute() {
        let tool = StewardStatusTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"impulse_dir": "/tmp/nonexistent_impulse_xyz"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.output.get("mode").is_some());
        assert!(result.output.get("thresholds").is_some());
    }
}
