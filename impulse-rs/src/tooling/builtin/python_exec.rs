//! Python execution tool — wraps tools::python::execute_python()
//!
//! This tool provides a safe way for agents to execute Python code
//! through Impulse, with structured JSON output.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Execute Python code and return stdout/stderr.
///
/// This enables agentic harnesses to run data processing, calculations,
/// and lightweight scripts through Impulse's controlled interface.
pub struct PythonExecTool;

#[async_trait]
impl DynamicTool for PythonExecTool {
    fn id(&self) -> &str {
        "python_exec"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "python_exec".into(),
            name: "Python Execute".into(),
            description: "Execute Python code and return stdout/stderr".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![
                ToolParam {
                    name: "code".into(),
                    description: "Python code to execute".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "timeout".into(),
                    description: "Timeout in seconds (default: 30)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(30)),
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
            .ok_or_else(|| ToolError::InvalidParams("missing 'code'".into()))?;

        match crate::tools::python::execute_python(code) {
            Ok(result) => {
                let success = result.exit_code == 0;
                Ok(ToolResult::json(serde_json::json!({
                    "success": success,
                    "exit_code": result.exit_code,
                    "stdout": result.output.trim(),
                    "stderr": result.error,
                })))
            }
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "Python execution failed: {}",
                e
            ))),
        }
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
        let tool = PythonExecTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "python_exec");
        assert_eq!(desc.params.len(), 2);
    }

    #[test]
    fn test_validate_ok() {
        let tool = PythonExecTool;
        assert!(tool
            .validate_params(&serde_json::json!({"code": "print('hello')"}))
            .is_ok());
    }

    #[test]
    fn test_validate_empty() {
        let tool = PythonExecTool;
        assert!(tool
            .validate_params(&serde_json::json!({"code": ""}))
            .is_err());
    }

    #[tokio::test]
    async fn test_execute() {
        let tool = PythonExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({"code": "print(1+1)"}), &ctx)
            .await;
        if let Ok(r) = result {
            assert_eq!(r.output["success"], true);
            assert_eq!(r.output["stdout"], "2");
        }
    }
}
