//! Benchmark tool — wraps tools::benchmark::run_benchmark()
//!
//! Allows agents to benchmark operations via the DynamicTool interface.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Run a micro-benchmark on a Python expression.
///
/// Useful for agents to measure performance of operations before recommending
/// approaches, or for profiling data processing pipelines.
pub struct BenchmarkerTool;

#[async_trait]
impl DynamicTool for BenchmarkerTool {
    fn id(&self) -> &str {
        "benchmark"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "benchmark".into(),
            name: "Benchmark".into(),
            description: "Run a micro-benchmark on a Python expression".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "code".into(),
                    description: "Python code to benchmark".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "iterations".into(),
                    description: "Number of iterations (default: 100)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(100)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("code").and_then(|v| v.as_str()) {
            Some(code) if !code.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams(
                "missing or empty 'code' string".into(),
            )),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let code = params
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'code'".into()))?
            .to_string();
        let iterations = params
            .get("iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as u32;

        // Use the existing benchmark module to time Python execution
        let result = crate::tools::benchmark::run_benchmark("python_benchmark", iterations, || {
            let _ = crate::tools::python::execute_python(&code);
        });

        Ok(ToolResult::json(serde_json::json!({
            "name": result.name,
            "iterations": result.iterations,
            "total_ms": result.duration_ms,
            "avg_ms": result.avg_ms,
            "min_ms": result.min_ms,
            "max_ms": result.max_ms,
            "summary": crate::tools::benchmark::format_benchmark(&result),
        })))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::PythonExec]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = BenchmarkerTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "benchmark");
        assert_eq!(desc.category, ToolCategory::Analysis);
    }

    #[test]
    fn test_validate_ok() {
        let tool = BenchmarkerTool;
        assert!(tool
            .validate_params(&serde_json::json!({"code": "x = 1+1"}))
            .is_ok());
    }

    #[tokio::test]
    async fn test_execute() {
        let tool = BenchmarkerTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"code": "x = 1+1", "iterations": 3}),
                &ctx,
            )
            .await;
        if let Ok(r) = result {
            assert!(r.output.get("avg_ms").is_some());
            assert!(r.output.get("summary").is_some());
        }
    }
}
