//! Sandboxed Python execution for the calculator and python_exec tools.
//!
//! Code runs inside Monty, pydantic's Python interpreter written in Rust, in
//! this process. No host functions, filesystem mounts, or inputs are wired,
//! so the interpreter is the whole boundary: programs have no filesystem,
//! network, process, or environment access. Time and memory budgets are
//! enforced by the interpreter and come back as typed faults on
//! [`PythonResult`]. Nothing here falls back to system CPython.
//!
//! Crash isolation is not provided: a stack-overflow or allocator abort inside
//! Monty takes this process down. `monty-pool` (subprocess workers) is the
//! follow-up for that. See ADR-0021.

use anyhow::{anyhow, Result};
use monty::MontyRun;
use monty_types::{
    CompileOptions, ExcType, MontyException, OsPolicy, PrintWriter, ResourceLimits,
    ResourceTracker, SleepMode, MONTY_VERSION,
};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

/// Default wall-clock budget for one program (calculator and python_exec).
pub const DEFAULT_PYTHON_TIMEOUT: Duration = Duration::from_secs(5);
/// Heap budget for one program, enforced by Monty's allocation accounting.
pub const DEFAULT_PYTHON_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Cap on collected `print` output. Exceeding it is a [`fault::MEMORY`] fault.
pub const DEFAULT_PYTHON_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
/// File name shown in tracebacks.
const SCRIPT_NAME: &str = "exec.py";

/// Failure classes reported in [`PythonResult::fault`]. Callers branch on these
/// instead of parsing `error` text.
pub mod fault {
    /// The code did not parse.
    pub const SYNTAX: &str = "syntax";
    /// The code parsed but uses a Python feature or module the sandbox does not implement.
    pub const UNSUPPORTED: &str = "unsupported";
    /// An ordinary Python exception escaped the program.
    pub const RUNTIME: &str = "runtime";
    /// The wall-clock budget was exhausted.
    pub const TIMEOUT: &str = "timeout";
    /// The memory budget (heap or collected print output) was exhausted.
    pub const MEMORY: &str = "memory";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonResult {
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
    /// Which [`fault`] class produced `error`, if any. `None` means the program
    /// ran to completion.
    pub fault: Option<&'static str>,
}

/// Run a Python program in the sandbox under the default budgets.
pub fn execute_python(code: &str) -> Result<PythonResult> {
    execute_python_with_timeout(code, DEFAULT_PYTHON_TIMEOUT)
}

/// Run a Python program in the sandbox with an explicit wall-clock budget.
///
/// Program failures of every class are `Ok(PythonResult { fault: Some(..) })`.
/// `Err` is reserved for the interpreter itself failing (a Rust panic inside
/// Monty), which is a bug in the interpreter, not a property of the program.
pub fn execute_python_with_timeout(code: &str, timeout: Duration) -> Result<PythonResult> {
    let limits = ResourceLimits {
        max_memory: Some(DEFAULT_PYTHON_MEMORY_LIMIT),
        max_feed_duration: Some(timeout),
        ..ResourceLimits::default()
    };
    catch_interpreter_panic(|| run_sandboxed(code, limits))
}

/// Compile and run `code` with no inputs, no host functions, and no mounts.
fn run_sandboxed(code: &str, limits: ResourceLimits) -> PythonResult {
    let mut output = String::new();
    let outcome = MontyRun::new(
        code.to_owned(),
        SCRIPT_NAME,
        vec![],
        CompileOptions::default(),
    )
    .and_then(|runner| {
        // Sleeps return at once, so a program cannot hold the caller for the
        // whole budget while spending none of it.
        let mut runner = runner.with_os_policy(OsPolicy {
            sleep: SleepMode::Zero,
            ..OsPolicy::default()
        });
        let sink = PrintWriter::CollectString(&mut output, Some(DEFAULT_PYTHON_OUTPUT_LIMIT));
        // The final expression's value is dropped, as `python3 -c` did.
        runner
            .run(vec![], ResourceTracker::new(limits), sink)
            .map(|_final_value| ())
    });
    match outcome {
        Ok(()) => PythonResult {
            output,
            error: None,
            exit_code: 0,
            fault: None,
        },
        // Exit code 1 is what `python3 -c` reported for an uncaught exception,
        // so callers that branch on it keep working.
        Err(exc) => PythonResult {
            output,
            error: Some(exc.to_string()),
            exit_code: 1,
            fault: Some(classify(&exc)),
        },
    }
}

/// Map a Monty exception to a [`fault`] class. A program that raises
/// `TimeoutError` or `MemoryError` itself lands in the same class as the
/// matching budget; callers do not need to tell those apart.
fn classify(exc: &MontyException) -> &'static str {
    match exc.exc_type() {
        ExcType::SyntaxError => fault::SYNTAX,
        // Monty raises NotImplementedError for Python it does not implement and
        // ModuleNotFoundError for modules outside its stdlib subset.
        ExcType::NotImplementedError | ExcType::ModuleNotFoundError => fault::UNSUPPORTED,
        ExcType::TimeoutError => fault::TIMEOUT,
        ExcType::MemoryError => fault::MEMORY,
        _ => fault::RUNTIME,
    }
}

/// Keep an interpreter panic from unwinding into the daemon. Aborts (stack
/// overflow, allocator failure) cannot be caught here; see the module docs.
fn catch_interpreter_panic(run: impl FnOnce() -> PythonResult) -> Result<PythonResult> {
    catch_unwind(AssertUnwindSafe(run)).map_err(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic payload");
        anyhow!("python sandbox panicked: {message}")
    })
}

/// The sandbox is compiled into this binary, so it is always available. Kept
/// for the `system` and `health` surfaces that report interpreter presence.
pub fn is_python_available() -> bool {
    true
}

/// Version of the embedded interpreter. This is Monty's version, not a CPython
/// version; Monty implements a subset of Python.
pub fn get_python_version() -> Option<String> {
    Some(format!("Monty {MONTY_VERSION} (embedded Python sandbox)"))
}

/// True when `expression` is a numeric formula only (digits, `+ - * / % ( ) .`,
/// optional scientific `e`/`E`, whitespace). Rejects identifiers. The sandbox
/// is the security boundary; this keeps the calculator a calculator.
pub fn is_restricted_math_expression(expression: &str) -> bool {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut prev_was_exponent = false;
    for c in trimmed.chars() {
        match c {
            '0'..='9' | '+' | '-' | '*' | '/' | '%' | '(' | ')' | '.' | ' ' | '\t' => {
                prev_was_exponent = false;
            }
            'e' | 'E' => {
                if prev_was_exponent {
                    return false;
                }
                prev_was_exponent = true;
            }
            _ => return false,
        }
    }
    true
}

/// Evaluate a mathematical expression in the sandbox after restricting the
/// input to a numeric formula.
pub fn calculate(expression: &str) -> Result<String> {
    if !is_restricted_math_expression(expression) {
        return Err(anyhow!(
            "calculate() accepts a mathematical expression only (digits and + - * / % ( ) . e); got non-math input"
        ));
    }
    let code = format!(
        "import json; result = {}; print(json.dumps({{'result': str(result)}}))",
        expression
    );

    let result = execute_python(&code)?;

    if result.exit_code != 0 {
        return Err(anyhow!("Calculation error: {:?}", result.error));
    }

    // Parse JSON output
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result.output) {
        if let Some(val) = parsed.get("result") {
            return Ok(val.to_string());
        }
    }

    Ok(result.output.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_is_python_available_always_true_for_embedded_sandbox() {
        assert!(is_python_available());
        let version = get_python_version().expect("embedded interpreter has a version");
        assert!(version.contains("Monty"), "got {version}");
    }

    #[test]
    fn test_get_python_version_format_contains_dotted_number() {
        let v = get_python_version().expect("version");
        assert!(v.contains('.'), "version should contain dot separator: {v}");
        assert!(
            v.chars().any(|c| c.is_ascii_digit()),
            "version should contain digits: {v}"
        );
    }

    #[test]
    fn test_execute_python() {
        let result = execute_python("print('Hello from Python')");
        assert!(result.is_ok());
        let r = result.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.output.contains("Hello from Python"));
    }

    #[test]
    fn test_execute_python_returns_large_stdout_in_full() {
        // 2MB of output sits under the 16 MiB print cap and the 64 MiB heap
        // budget, so it must come back complete and well inside the budget.
        let started = Instant::now();
        let result = execute_python("print('x' * 2000000)").expect("2MB print must succeed");
        assert_eq!(result.exit_code, 0, "stderr={:?}", result.error);
        let xs = result.output.matches('x').count();
        assert_eq!(
            xs,
            2_000_000,
            "got {} x's (len {})",
            xs,
            result.output.len()
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "large output must finish quickly, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn test_catch_interpreter_panic_returns_err_with_message() {
        let err = catch_interpreter_panic(|| panic!("boom in monty"))
            .expect_err("a panic must become Err, not unwind");
        assert!(err.to_string().contains("boom in monty"), "got {err}");
        assert!(
            err.to_string().contains("python sandbox panicked"),
            "got {err}"
        );
    }

    #[test]
    fn test_calculate() {
        let result = calculate("2 + 2");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "\"4\"");
    }

    #[test]
    fn test_calculate_expression() {
        let result = calculate("(10 + 5) * 2");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "\"30\"");
    }

    #[test]
    fn test_calculate_division_by_zero_returns_err() {
        let err = calculate("1 / 0").expect_err("division by zero is a calculation error");
        assert!(
            err.to_string().contains("ZeroDivisionError"),
            "error should carry the Python exception, got {err}"
        );
    }

    #[test]
    fn test_is_restricted_math_expression_accepts_formulas() {
        assert!(is_restricted_math_expression("2 + 2"));
        assert!(is_restricted_math_expression("(10 + 5) * 2"));
        assert!(is_restricted_math_expression("1e-3"));
        assert!(is_restricted_math_expression("  -4.5 / 2  "));
        assert!(!is_restricted_math_expression(""));
        assert!(!is_restricted_math_expression(
            "__import__('os').system('id')"
        ));
        assert!(!is_restricted_math_expression("os.system('echo pwned')"));
        assert!(!is_restricted_math_expression("2 + foo"));
    }

    #[test]
    fn test_calculate_rejects_python_injection() {
        let err = calculate("__import__('os').system('id')")
            .expect_err("arbitrary python must not reach the interpreter");
        let msg = err.to_string();
        assert!(
            msg.contains("mathematical expression only"),
            "error should name the math-only contract, got {msg}"
        );
    }
}

#[cfg(test)]
mod python_sandbox_tests {
    //! Boundary tests. The interpreter is the whole sandbox, so every probe
    //! below must come back as a `PythonResult` carrying a fault and must leave
    //! no trace on the host.
    use super::*;
    use std::time::Instant;

    fn marker_path(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.touched"));
        (dir, path)
    }

    #[test]
    fn test_execute_python_import_subprocess_returns_fault_and_runs_nothing() {
        let (_dir, marker) = marker_path("subprocess");
        let code = format!(
            "import subprocess\nsubprocess.run([\"touch\", {:?}])\n",
            marker.display().to_string()
        );
        let result = execute_python(&code).expect("sandbox faults are results, not errors");
        assert_eq!(
            result.fault,
            Some(fault::UNSUPPORTED),
            "import subprocess must fault as unsupported, got {result:?}"
        );
        assert_ne!(result.exit_code, 0);
        assert!(!marker.exists(), "subprocess.run must not reach the host");
    }

    #[test]
    fn test_execute_python_open_etc_passwd_returns_fault_and_reads_nothing() {
        let result = execute_python("print(open(\"/etc/passwd\").read())\n").expect("result");
        assert!(result.fault.is_some(), "open() must fault, got {result:?}");
        assert!(
            !result.output.contains("root:"),
            "host file contents leaked: {}",
            result.output
        );
        assert!(result.output.is_empty(), "got output {:?}", result.output);
    }

    #[test]
    fn test_execute_python_dunder_import_os_system_returns_fault_and_runs_nothing() {
        let (_dir, marker) = marker_path("os-system");
        let code = format!(
            "__import__(\"os\").system(\"touch {}\")\n",
            marker.display()
        );
        let result = execute_python(&code).expect("result");
        assert!(
            result.fault.is_some(),
            "os.system must fault, got {result:?}"
        );
        assert!(!marker.exists(), "os.system must not reach the host");
    }

    #[test]
    fn test_execute_python_infinite_loop_returns_timeout_fault_within_budget() {
        let budget = Duration::from_millis(500);
        let started = Instant::now();
        let result = execute_python_with_timeout("while True:\n    pass\n", budget)
            .expect("a timeout is a result, not an error");
        let elapsed = started.elapsed();
        assert_eq!(result.fault, Some(fault::TIMEOUT), "got {result:?}");
        assert_ne!(result.exit_code, 0);
        assert!(
            elapsed < Duration::from_secs(3),
            "timeout must fire near the budget, took {elapsed:?}"
        );
    }

    #[test]
    fn test_execute_python_large_allocation_returns_memory_fault() {
        let result =
            execute_python("x = [0] * (200 * 1024 * 1024)\nprint(len(x))\n").expect("result");
        assert_eq!(result.fault, Some(fault::MEMORY), "got {result:?}");
        assert!(
            result.output.is_empty(),
            "allocation must fail before print, got {:?}",
            result.output
        );
    }

    #[test]
    fn test_execute_python_print_returns_output_and_no_error() {
        let result = execute_python("print(2 + 2)").expect("result");
        assert_eq!(result.output, "4\n");
        assert_eq!(result.error, None);
        assert_eq!(result.fault, None);
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_execute_python_syntax_error_returns_syntax_fault() {
        let result = execute_python("def (\n").expect("result");
        assert_eq!(result.fault, Some(fault::SYNTAX), "got {result:?}");
        assert_ne!(result.exit_code, 0);
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("SyntaxError"),
            "error should name SyntaxError, got {:?}",
            result.error
        );
    }

    #[test]
    fn test_execute_python_uncaught_exception_returns_runtime_fault() {
        let result = execute_python("print('before')\n1 / 0\n").expect("result");
        assert_eq!(result.fault, Some(fault::RUNTIME), "got {result:?}");
        assert_ne!(result.exit_code, 0);
        assert_eq!(result.output, "before\n", "output before the raise is kept");
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or("")
                .contains("ZeroDivisionError"),
            "error should carry the traceback, got {:?}",
            result.error
        );
    }

    #[test]
    fn test_execute_python_sleep_cannot_stall_past_the_budget() {
        let started = Instant::now();
        let result =
            execute_python("import time\ntime.sleep(30)\nprint('done')\n").expect("result");
        assert_eq!(result.fault, None, "got {result:?}");
        assert_eq!(result.output, "done\n");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "sleep must not block the caller, took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn test_execute_python_print_over_output_cap_returns_memory_fault() {
        // 20 MiB fits the 64 MiB heap but not the 16 MiB print cap.
        let result = execute_python("print('y' * (20 * 1024 * 1024))\n").expect("result");
        assert_eq!(result.fault, Some(fault::MEMORY), "got {result:?}");
        assert!(
            result.output.len() <= DEFAULT_PYTHON_OUTPUT_LIMIT,
            "collected {} bytes past the cap",
            result.output.len()
        );
    }
}
