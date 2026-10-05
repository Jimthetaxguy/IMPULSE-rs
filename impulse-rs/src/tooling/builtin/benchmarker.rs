//! Benchmark tool — wraps tools::benchmark::run_benchmark()
//!
//! Allows agents to benchmark operations via the DynamicTool interface.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Most iterations one call may request.
const MAX_BENCHMARK_ITERATIONS: u64 = 10_000;

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
                    description: "Number of iterations (default: 100, at most 10000; the \
                                  estimated total must fit the session's tool time limit)"
                        .into(),
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
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let code = params
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'code'".into()))?
            .to_string();
        // `as u32` used to truncate a large count (2^32 + 5 ran 5 times), and
        // nothing bounded it: the timing loop runs without an await, so the
        // executor's timeout cannot stop it once it starts.
        let iterations = params
            .get("iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(100);
        if !(1..=MAX_BENCHMARK_ITERATIONS).contains(&iterations) {
            return Err(ToolError::InvalidParams(format!(
                "iterations must be between 1 and {MAX_BENCHMARK_ITERATIONS}, got {iterations}"
            )));
        }
        let iterations = iterations as u32;

        // A program that does not run to completion would be timed all the
        // same, and the numbers would describe the sandbox's failure path, not
        // the workload. Run it once before timing anything and refuse a
        // program that faults. A slow failure such as a timeout is then paid
        // for once, not once per iteration.
        let preflight_started = std::time::Instant::now();
        let preflight = crate::tools::python::execute_python(&code)
            .map_err(|e| ToolError::ExecutionFailed(format!("sandbox failed: {e}")))?;
        if let Some(reason) = benchmark_refusal(&preflight) {
            return Err(ToolError::ExecutionFailed(reason));
        }
        // One run's time stands in for the rest: refuse work that would run
        // past the session's tool time limit, which could not stop it.
        let one_run = preflight_started.elapsed();
        let estimate = one_run.saturating_mul(iterations);
        let limit = std::time::Duration::from_millis(ctx.timeout_ms.max(1));
        if estimate > limit {
            return Err(ToolError::InvalidParams(format!(
                "{iterations} iterations of a {} ms program would take about {} s, past this \
                 session's {} s tool time limit; use fewer iterations",
                one_run.as_millis(),
                estimate.as_secs(),
                limit.as_secs()
            )));
        }

        // The estimate trusts one run; a program slower after its first run
        // would still outlast the limit, so the loop also stops at it, and
        // each run gets only the time left before it.
        let deadline = std::time::Instant::now() + limit.saturating_sub(one_run);
        let runs = time_runs(iterations, deadline, |remaining| {
            let budget = remaining.min(crate::tools::python::DEFAULT_PYTHON_TIMEOUT);
            match crate::tools::python::execute_python_with_timeout(&code, budget) {
                Ok(run)
                    if run.fault == Some(crate::tools::python::fault::TIMEOUT)
                        && budget < crate::tools::python::DEFAULT_PYTHON_TIMEOUT =>
                {
                    RunOutcome::OutOfTime
                }
                Ok(run) => {
                    benchmark_refusal(&run).map_or(RunOutcome::Completed, RunOutcome::Refused)
                }
                Err(e) => RunOutcome::Refused(format!("sandbox failed: {e}")),
            }
        });
        if let Some(reason) = runs.first_failure {
            return Err(ToolError::ExecutionFailed(reason));
        }
        if runs.past_deadline {
            return Err(ToolError::ExecutionFailed(format!(
                "stopped at this session's {} s tool time limit before {iterations} iterations \
                 finished; the program ran slower than its first run",
                limit.as_secs()
            )));
        }
        let result = runs.result;

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

/// How [`time_runs`] ended.
struct TimedRuns {
    result: crate::tools::benchmark::BenchmarkResult,
    /// The first iteration that could not be timed, so a program that fails
    /// only some of the time is not reported as timed either.
    first_failure: Option<String>,
    /// Whether `deadline` passed before every iteration ran.
    past_deadline: bool,
}

/// How one timed run ended.
enum RunOutcome {
    Completed,
    /// The run cannot be timed: why.
    Refused(String),
    /// The run hit the time it was given, which was all that was left.
    OutOfTime,
}

/// Times `iterations` calls of `run_once`, which is given the time left
/// before `deadline`. The loop has no await, so the executor's timeout
/// cannot stop it; a run that starts just before the deadline used to run
/// its full sandbox budget past it, and once the deadline passes the
/// remaining iterations do nothing.
fn time_runs(
    iterations: u32,
    deadline: std::time::Instant,
    mut run_once: impl FnMut(std::time::Duration) -> RunOutcome,
) -> TimedRuns {
    let mut first_failure: Option<String> = None;
    let mut past_deadline = false;
    let result = crate::tools::benchmark::run_benchmark("python_benchmark", iterations, || {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if past_deadline || remaining.is_zero() {
            past_deadline = true;
            return;
        }
        match run_once(remaining) {
            RunOutcome::Completed => {}
            RunOutcome::Refused(reason) => {
                first_failure.get_or_insert(reason);
            }
            RunOutcome::OutOfTime => past_deadline = true,
        }
    });
    TimedRuns {
        result,
        first_failure,
        past_deadline,
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

    /// Verification round on af63095: the time estimate trusted the first
    /// run, and a program slower afterwards kept the loop going past the
    /// session's limit.
    #[test]
    fn test_time_runs_stops_at_the_deadline() {
        let mut calls = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
        let runs = time_runs(1_000, deadline, |_| {
            calls += 1;
            std::thread::sleep(std::time::Duration::from_millis(10));
            RunOutcome::Completed
        });
        assert!(runs.past_deadline);
        // Each call takes at least 10 ms, so at most five start before 50 ms.
        assert!(calls <= 5, "ran {calls} times past a 50 ms deadline");
        assert_eq!(runs.first_failure, None);
    }

    #[test]
    fn test_time_runs_keeps_the_first_failure_and_runs_every_iteration() {
        let mut calls = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        let runs = time_runs(5, deadline, |_| {
            calls += 1;
            match calls {
                2 => RunOutcome::Refused("second run failed".to_string()),
                3 => RunOutcome::Refused("third run failed".to_string()),
                _ => RunOutcome::Completed,
            }
        });
        assert!(!runs.past_deadline);
        assert_eq!(calls, 5);
        assert_eq!(runs.result.iterations, 5);
        assert_eq!(runs.first_failure.as_deref(), Some("second run failed"));
    }

    /// Round 3 (reviewer B): a run that started just before the deadline
    /// ran its whole sandbox budget past it (4.64 s under a 2 s limit).
    #[test]
    fn test_time_runs_gives_each_run_only_the_time_left() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        let mut given = Vec::new();
        let runs = time_runs(1_000, deadline, |remaining| {
            given.push(remaining);
            if given.len() == 3 {
                // The sandbox stopped this run at the time it was given.
                return RunOutcome::OutOfTime;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            RunOutcome::Completed
        });
        assert!(runs.past_deadline);
        assert_eq!(given.len(), 3, "no run starts after one runs out of time");
        assert!(given
            .iter()
            .all(|g| *g <= std::time::Duration::from_millis(200)));
        assert!(given.windows(2).all(|pair| pair[1] < pair[0]), "{given:?}");
        assert_eq!(runs.first_failure, None);
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
    async fn test_execute_refuses_an_iteration_count_out_of_range() {
        let ctx = ToolContext::with_all_capabilities();
        for iterations in [0u64, MAX_BENCHMARK_ITERATIONS + 1, (1u64 << 32) + 5] {
            let result = BenchmarkerTool
                .execute(
                    serde_json::json!({"code": "x = 1", "iterations": iterations}),
                    &ctx,
                )
                .await;
            assert!(
                matches!(result, Err(ToolError::InvalidParams(_))),
                "{iterations}: {result:?}"
            );
        }
    }

    /// Recorded review P3: the loop cannot be interrupted, so a request whose
    /// estimate passes the session's time limit is refused before it starts.
    #[tokio::test]
    async fn test_execute_refuses_work_past_the_time_limit() {
        let ctx = ToolContext {
            timeout_ms: 1_000,
            ..ToolContext::with_all_capabilities()
        };
        let slow = "total = 0\nfor i in range(200000):\n    total += i\n";
        let result = BenchmarkerTool
            .execute(serde_json::json!({"code": slow, "iterations": 10000}), &ctx)
            .await;
        assert!(
            matches!(&result, Err(ToolError::InvalidParams(message)) if message.contains("tool time limit")),
            "{result:?}"
        );
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
