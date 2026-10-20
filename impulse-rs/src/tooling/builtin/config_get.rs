//! Config reader tool — read Impulse configuration
//!
//! Allows agents to inspect the Impulse config (injection mode,
//! stewardship settings, retrieval config, etc.) without parsing
//! config.json manually.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Read Impulse configuration, optionally filtered by key.
pub struct ConfigGetTool;

#[async_trait]
impl DynamicTool for ConfigGetTool {
    fn id(&self) -> &str {
        "config_get"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "config_get".into(),
            name: "Config Get".into(),
            description: "Read Impulse configuration values".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![
                ToolParam {
                    name: "key".into(),
                    description: "Specific config key to read (omit for all config)".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
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
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        // Without an explicit `impulse_dir`, the session's own directory: a
        // relative `.impulse` named whatever directory the process ran from.
        let impulse_dir = params
            .get("impulse_dir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.impulse_dir.clone());
        let key = params.get("key").and_then(|v| v.as_str());

        let config_path = impulse_dir.join("config.json");

        if !config_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "exists": false,
                "config": null,
                "message": "No config.json found — using defaults"
            })));
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read config: {}", e)))?;

        let config: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid config JSON: {}", e)))?;

        if let Some(k) = key {
            // Return specific key
            let value = config.get(k);
            Ok(ToolResult::json(serde_json::json!({
                "exists": true,
                "key": k,
                "value": value,
                "found": value.is_some(),
            })))
        } else {
            // Return full config
            Ok(ToolResult::json(serde_json::json!({
                "exists": true,
                "config": config,
            })))
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded review P3: with no `impulse_dir`, the tool read a
    /// `.impulse` relative to the process's working directory instead of the
    /// session's own directory.
    #[tokio::test]
    async fn test_execute_defaults_to_the_contexts_impulse_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), r#"{"log_level": "trace"}"#).unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };
        let result = ConfigGetTool
            .execute(serde_json::json!({"key": "log_level"}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            result.output["exists"],
            serde_json::json!(true),
            "{}",
            result.output
        );
        assert!(
            result.output.to_string().contains("trace"),
            "{}",
            result.output
        );
    }

    #[test]
    fn test_descriptor() {
        let tool = ConfigGetTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "config_get");
        assert_eq!(desc.category, ToolCategory::System);
    }

    #[tokio::test]
    async fn test_execute_no_config() {
        let tool = ConfigGetTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"impulse_dir": "/tmp/nonexistent_impulse_xyz"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["exists"], false);
    }
}
