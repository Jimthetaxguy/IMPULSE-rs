//! Health check tool — wraps tools::health::check_impulse_health()

use async_trait::async_trait;
use std::path::PathBuf;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

pub struct HealthCheckTool;

#[async_trait]
impl DynamicTool for HealthCheckTool {
    fn id(&self) -> &str {
        "health_check"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "health_check".into(),
            name: "Health Check".into(),
            description: "Run health checks on the .impulse/ directory".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![ToolParam {
                name: "impulse_dir".into(),
                description: "Path to .impulse/ directory (defaults to .impulse in cwd)".into(),
                param_type: ParamType::FilePath,
                required: false,
                default: Some(serde_json::json!(".impulse")),
            }],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let impulse_dir = params
            .get("impulse_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.impulse_dir.clone());

        let report = crate::tools::health::check_impulse_health(&impulse_dir);
        let python_health = crate::tools::health::check_python_health();

        let output = serde_json::json!({
            "overall_status": report.overall_status,
            "checks": report.checks,
            "python": {
                "name": python_health.name,
                "status": python_health.status,
                "message": python_health.message,
            },
        });

        Ok(ToolResult::json(output))
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
        let tool = HealthCheckTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "health_check");
        assert_eq!(desc.category, ToolCategory::System);
    }

    #[tokio::test]
    async fn test_execute() {
        let tool = HealthCheckTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert!(result.output.get("overall_status").is_some());
        assert!(result.output.get("python").is_some());
    }
}
