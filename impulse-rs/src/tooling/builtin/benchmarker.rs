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

        // A program that does not run to completion would be timed all the
        // same, and the numbers would describe the sandbox's failure path, not
        // the workload. Run it once before timing anything and refuse a
        // program that faults. A slow failure such as a timeout is then paid
        // for once, not once per iteration.
        let preflight = crate::tools::python::execute_python(&code)
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox failed: {e}")))?;
        if let Some(reason) = benchmark_refusal(&preflight) {
            return Err(ToolError::ExecutionFailed(reason));
        }

        // Use the existing benchmark module to time Python execution. The
        // first iteration that fails is kept, so a program that fails only
        // some of the time is not reported as timed either.
        let mut first_failure: Option<String> = None;
        let result = crate::tools::benchmark::run_benchmark("python_benchmark", iterations, || {
            let failure = match crate::tools::python::execute_python(&code) {
                Ok(run) => benchmark_refusal(&run),
                Err(e) => Some(format!("sandbox failed: {e}")),
            };
            if first_failure.is_none() {
                first_failure = failure;
            }
        });
        if let Some(reason) = first_failure {
            return Err(ToolError::ExecutionFailed(reason));
        }

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

/// Why a sandbox run cannot be benchmarked, or `None` when it ran to completion.
fn benchmark_refusal(run: &crate::tools::python::PythonResult) -> Option<String> {
    let detail = run.error.as_deref().unwrap_or("no error text").trim();
    if let Some(fault) = run.fault {
        return Some(format!(
            "the program did not run to completion ({fault}), so there is nothing to time: {detail}"
        ));
    }
    if run.exit_code != 0 {
        return Some(format!(
            "the program exited with code {}, so there is nothing to time: {detail}",
            run.exit_code
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox_result(
        fault: Option<&'static str>,
        exit_code: i32,
    ) -> crate::tools::python::PythonResult {
        crate::tools::python::PythonResult {
            output: String::new(),
            error: fault.map(|class| format!("{class} happened")),
            exit_code,
            fault,
        }
    }

    #[test]
    fn test_benchmark_refusal_completed_run_is_none() {
        assert_eq!(benchmark_refusal(&sandbox_result(None, 0)), None);
    }

    #[test]
    fn test_benchmark_refusal_names_a_timeout_and_keeps_the_error_text() {
        let timed_out = sandbox_result(Some(crate::tools::python::fault::TIMEOUT), 1);
        let reason = benchmark_refusal(&timed_out).expect("a timeout cannot be timed");
        assert!(reason.contains("(timeout)"), "unexpected reason: {reason}");
        assert!(
            reason.contains("timeout happened"),
            "unexpected reason: {reason}"
        );
    }

    #[test]
    fn test_benchmark_refusal_nonzero_exit_without_a_fault() {
        let reason =
            benchmark_refusal(&sandbox_result(None, 3)).expect("a non-zero exit cannot be timed");
        assert!(
            reason.contains("exited with code 3"),
            "unexpected reason: {reason}"
        );
        assert!(
            reason.contains("no error text"),
            "unexpected reason: {reason}"
        );
    }

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
    async fn test_execute_times_a_program_that_runs() {
        let tool = BenchmarkerTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"code": "x = 1+1", "iterations": 3}),
                &ctx,
            )
            .await
            .expect("a program that runs is timed");
        assert_eq!(result.output["iterations"], serde_json::json!(3));
        assert!(result.output.get("avg_ms").is_some());
        assert!(result.output.get("summary").is_some());
    }

    /// The refusal for a program the sandbox could not run to completion.
    /// Panics if the tool timed it instead.
    async fn refusal_for(code: &str) -> String {
        let tool = BenchmarkerTool;
        let ctx = ToolContext::with_all_capabilities();
        match tool
            .execute(serde_json::json!({"code": code, "iterations": 3}), &ctx)
            .await
        {
            Err(ToolError::ExecutionFailed(message)) => message,
            Err(other) => panic!("expected ExecutionFailed, got {other:?}"),
            Ok(timed) => panic!("a failing program was timed: {}", timed.output),
        }
    }

    #[tokio::test]
    async fn test_execute_refuses_a_program_the_sandbox_cannot_run() {
        let message = refusal_for("import subprocess").await;
        assert!(
            message.contains("unsupported"),
            "unexpected refusal: {message}"
        );
    }

    #[tokio::test]
    async fn test_execute_refuses_a_program_that_raises() {
        let message = refusal_for("1 / 0").await;
        assert!(message.contains("runtime"), "unexpected refusal: {message}");
    }

    #[tokio::test]
    async fn test_execute_refuses_a_program_that_does_not_parse() {
        let message = refusal_for("def (").await;
        assert!(message.contains("syntax"), "unexpected refusal: {message}");
    }
}
