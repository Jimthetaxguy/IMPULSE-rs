//! System info tool — wraps tools::system::SystemInfo::collect()

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

pub struct SystemInfoTool;

#[async_trait]
impl DynamicTool for SystemInfoTool {
    fn id(&self) -> &str {
        "system_info"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "system_info".into(),
            name: "System Info".into(),
            description: "Collect system information (OS, arch, Python availability, env vars)"
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![ToolParam {
                name: "include_env".into(),
                description: "Include IMPULSE_ environment variables in output".into(),
                param_type: ParamType::Bool,
                required: false,
                default: Some(serde_json::json!(false)),
            }],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        // All params are optional
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let info = crate::tools::system::SystemInfo::collect();
        let include_env = params
            .get("include_env")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut output = serde_json::json!({
            "os": info.os,
            "arch": info.arch,
            "home_dir": info.home_dir,
            "current_dir": info.current_dir,
            "python_available": info.python_available,
            "python_version": info.python_version,
        });

        if include_env {
            let env_vars = crate::tools::system::get_impulse_env_vars();
            let env_map: serde_json::Map<String, serde_json::Value> = env_vars
                .into_iter()
                .map(|e| (e.key, serde_json::Value::String(e.value)))
                .collect();
            output["env"] = serde_json::Value::Object(env_map);
        }

        Ok(ToolResult::json(output))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::SystemInfo]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = SystemInfoTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "system_info");
        assert_eq!(desc.category, ToolCategory::System);
    }

    #[tokio::test]
    async fn test_execute_basic() {
        let tool = SystemInfoTool;
        let ctx = ToolContext::default();
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert!(result.output.get("os").is_some());
        assert!(result.output.get("arch").is_some());
    }

    #[tokio::test]
    async fn test_execute_with_env() {
        let tool = SystemInfoTool;
        let ctx = ToolContext::default();
        let result = tool
            .execute(serde_json::json!({"include_env": true}), &ctx)
            .await
            .unwrap();
        assert!(result.output.get("env").is_some());
    }
}
