//! Python execution tool — wraps tools::python::execute_python()
//!
//! Code runs in the in-process Monty sandbox (no filesystem, network, or
//! process access; see ADR-0021) and comes back as structured JSON with a
//! `fault` class when the program fails.

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
            description: "Execute Python code in a Monty sandbox (no filesystem, network, or process access; 64 MiB memory, 5s wall clock) and return stdout plus a fault class on failure".into(),
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
                    description: "Wall-clock budget in seconds; capped at the sandbox limit of 5".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(5)),
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

        // A requested budget may only shorten the sandbox's own ceiling.
        let timeout = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .filter(|secs| *secs > 0)
            .map(std::time::Duration::from_secs)
            .unwrap_or(crate::tools::python::DEFAULT_PYTHON_TIMEOUT)
            .min(crate::tools::python::DEFAULT_PYTHON_TIMEOUT);

        match crate::tools::python::execute_python_with_timeout(code, timeout) {
            Ok(result) => {
                let success = result.exit_code == 0;
                Ok(ToolResult::json(serde_json::json!({
                    "success": success,
                    "exit_code": result.exit_code,
                    "stdout": result.output.trim(),
                    "stderr": result.error,
                    "fault": result.fault,
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
        let r = tool
            .execute(serde_json::json!({"code": "print(1+1)"}), &ctx)
            .await
            .expect("the sandbox is in-process, so execution cannot depend on host Python");
        assert_eq!(r.output["success"], true);
        assert_eq!(r.output["stdout"], "2");
        assert_eq!(r.output["fault"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_execute_uncaught_exception_reports_fault() {
        let tool = PythonExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let r = tool
            .execute(serde_json::json!({"code": "1 / 0"}), &ctx)
            .await
            .expect("a Python exception is a result, not a tool error");
        assert_eq!(r.output["success"], false);
        assert_eq!(r.output["exit_code"], 1);
        assert_eq!(r.output["fault"], "runtime");
        assert!(
            r.output["stderr"]
                .as_str()
                .unwrap_or("")
                .contains("ZeroDivisionError"),
            "got {}",
            r.output
        );
    }

    #[tokio::test]
    async fn test_execute_timeout_param_only_shortens_the_budget() {
        let tool = PythonExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let started = std::time::Instant::now();
        let r = tool
            .execute(
                serde_json::json!({"code": "while True:\n    pass", "timeout": 1}),
                &ctx,
            )
            .await
            .expect("a timeout is a result, not a tool error");
        assert_eq!(r.output["fault"], "timeout");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "a 1s budget must not run to the 5s ceiling, took {:?}",
            started.elapsed()
        );
    }
}
