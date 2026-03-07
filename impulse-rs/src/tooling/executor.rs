use std::time::Instant;

use super::error::ToolError;
use super::traits::{Capability, DynamicTool, ParamType, ToolContext, ToolDescriptor, ToolResult};

pub struct ToolExecutor;

impl ToolExecutor {
    pub async fn execute(
        tool: &dyn DynamicTool,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        for cap in tool.required_capabilities() {
            if !ctx.has_capability(cap) {
                return Err(ToolError::MissingCapability(cap));
            }
        }

        tool.validate_params(&params)?;

        let descriptor = tool.descriptor();
        Self::validate_paths(&descriptor, &params, ctx, &tool.required_capabilities())?;

        let started = Instant::now();
        let future = tool.execute(params, ctx);
        let mut result = tokio::time::timeout(
            std::time::Duration::from_millis(ctx.timeout_ms.max(1)),
            future,
        )
        .await
        .map_err(|_| ToolError::Timeout(ctx.timeout_ms))??;

        result = Self::enforce_artifact_limit(result, ctx);
        result = Self::enforce_output_limit(result, ctx)?;
        result.metadata.insert(
            "duration_ms".into(),
            started.elapsed().as_millis().to_string(),
        );
        result.metadata.insert(
            "execution_origin".into(),
            ctx.execution_origin.as_str().to_string(),
        );

        Ok(result)
    }

    fn validate_paths(
        descriptor: &ToolDescriptor,
        params: &serde_json::Value,
        ctx: &ToolContext,
        capabilities: &[Capability],
    ) -> Result<(), ToolError> {
        let needs_read = capabilities.contains(&Capability::FileSystemRead);
        let needs_write = capabilities.contains(&Capability::FileSystemWrite);

        if !needs_read && !needs_write {
            return Ok(());
        }

        for param in &descriptor.params {
            if param.param_type != ParamType::FilePath {
                continue;
            }

            let Some(path_str) = params.get(&param.name).and_then(|v| v.as_str()) else {
                continue;
            };

            let resolved = ctx.resolve_path(path_str);
            let allowed = if needs_read && needs_write {
                ctx.is_path_allowed(&resolved, false) || ctx.is_path_allowed(&resolved, true)
            } else if needs_write {
                ctx.is_path_allowed(&resolved, true)
            } else {
                ctx.is_path_allowed(&resolved, false)
            };

            if !allowed {
                return Err(ToolError::PathNotAllowed(resolved.display().to_string()));
            }
        }

        Ok(())
    }

    fn enforce_artifact_limit(mut result: ToolResult, ctx: &ToolContext) -> ToolResult {
        if result.artifacts.len() > ctx.max_artifacts {
            result.artifacts.truncate(ctx.max_artifacts);
            result
                .metadata
                .insert("artifacts_truncated".into(), "true".to_string());
        }
        result
    }

    fn enforce_output_limit(
        mut result: ToolResult,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let serialized = serde_json::to_vec(&result.output)?;
        if serialized.len() <= ctx.max_output_bytes {
            return Ok(result);
        }

        let preview = String::from_utf8_lossy(&serialized[..ctx.max_output_bytes]).to_string();
        result.output = match result.output {
            serde_json::Value::String(_) => serde_json::Value::String(preview),
            _ => serde_json::json!({
                "truncated": true,
                "preview": preview,
                "original_bytes": serialized.len(),
            }),
        };
        result
            .metadata
            .insert("output_truncated".into(), "true".to_string());
        result
            .metadata
            .insert("original_output_bytes".into(), serialized.len().to_string());

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::tooling::{Capability, DynamicTool, ExecutionOrigin, ToolCategory, ToolParam};

    struct LargeOutputTool;

    #[async_trait]
    impl DynamicTool for LargeOutputTool {
        fn id(&self) -> &str {
            "large_output"
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: self.id().into(),
                name: "Large Output".into(),
                description: "Returns a large string".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![],
            }
        }

        fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
            Ok(())
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text("x".repeat(2048)))
        }

        fn required_capabilities(&self) -> Vec<Capability> {
            vec![]
        }
    }

    struct PathTool;

    struct SlowTool;

    struct ArtifactTool;

    #[async_trait]
    impl DynamicTool for PathTool {
        fn id(&self) -> &str {
            "path_tool"
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: self.id().into(),
                name: "Path Tool".into(),
                description: "Reads a path".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![ToolParam {
                    name: "path".into(),
                    description: "Input path".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                }],
            }
        }

        fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
            Ok(())
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text("ok"))
        }

        fn required_capabilities(&self) -> Vec<Capability> {
            vec![Capability::FileSystemRead]
        }
    }

    #[async_trait]
    impl DynamicTool for SlowTool {
        fn id(&self) -> &str {
            "slow_tool"
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: self.id().into(),
                name: "Slow Tool".into(),
                description: "Sleeps beyond the timeout".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![],
            }
        }

        fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
            Ok(())
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(ToolResult::text("late"))
        }

        fn required_capabilities(&self) -> Vec<Capability> {
            vec![]
        }
    }

    #[async_trait]
    impl DynamicTool for ArtifactTool {
        fn id(&self) -> &str {
            "artifact_tool"
        }

        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: self.id().into(),
                name: "Artifact Tool".into(),
                description: "Returns many artifacts".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![],
            }
        }

        fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
            Ok(())
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                output: serde_json::json!({"ok": true}),
                artifacts: vec![
                    std::path::PathBuf::from("one.txt"),
                    std::path::PathBuf::from("two.txt"),
                    std::path::PathBuf::from("three.txt"),
                ],
                metadata: std::collections::HashMap::new(),
            })
        }

        fn required_capabilities(&self) -> Vec<Capability> {
            vec![]
        }
    }

    #[tokio::test]
    async fn test_output_truncation() {
        let ctx = ToolContext {
            execution_origin: ExecutionOrigin::Test,
            max_output_bytes: 128,
            ..ToolContext::with_all_capabilities()
        };
        let result = ToolExecutor::execute(&LargeOutputTool, serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(
            result.metadata.get("output_truncated").map(String::as_str),
            Some("true")
        );
    }

    #[tokio::test]
    async fn test_path_restriction() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ToolContext {
            execution_origin: ExecutionOrigin::Test,
            allowed_read_roots: vec![tmp.path().to_path_buf()],
            ..ToolContext::default()
        };
        let err = ToolExecutor::execute(&PathTool, serde_json::json!({"path": "Cargo.toml"}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PathNotAllowed(_)));
    }

    #[tokio::test]
    async fn test_timeout_enforced() {
        let ctx = ToolContext {
            execution_origin: ExecutionOrigin::Test,
            timeout_ms: 10,
            ..ToolContext::with_all_capabilities()
        };
        let err = ToolExecutor::execute(&SlowTool, serde_json::json!({}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Timeout(10)));
    }

    #[tokio::test]
    async fn test_artifact_limit_enforced() {
        let ctx = ToolContext {
            execution_origin: ExecutionOrigin::Test,
            max_artifacts: 2,
            ..ToolContext::with_all_capabilities()
        };
        let result = ToolExecutor::execute(&ArtifactTool, serde_json::json!({}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.artifacts.len(), 2);
        assert_eq!(
            result
                .metadata
                .get("artifacts_truncated")
                .map(String::as_str),
            Some("true")
        );
    }
}
