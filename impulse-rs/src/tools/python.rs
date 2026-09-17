// Python integration module - execute Python code for calculations and data processing
// Provides a safe way to run Python code from Rust

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PythonResult {
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
}

/// Execute Python code and return the result
/// Uses system Python interpreter
pub fn execute_python(code: &str) -> Result<PythonResult> {
    // Try python3 first, then python
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let output = Command::new(python_cmd)
        .args(["-c", code])
        .output()
        .context("Failed to execute Python")?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(PythonResult {
        output: stdout,
        error: if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        },
        exit_code,
    })
}

/// Check if Python is available
pub fn is_python_available() -> bool {
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    Command::new(python_cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get Python version
pub fn get_python_version() -> Option<String> {
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    Command::new(python_cmd)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).to_string())
            } else {
                None
            }
        })
}

/// Execute a Python script file
pub fn execute_script(script_path: &PathBuf) -> Result<PythonResult> {
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let output = Command::new(python_cmd)
        .arg(script_path)
        .output()
        .context("Failed to execute Python script")?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok(PythonResult {
        output: stdout,
        error: if stderr.is_empty() {
            None
        } else {
            Some(stderr)
        },
        exit_code,
    })
}

/// True when `expression` is a numeric formula only (digits, `+ - * / % ( ) .`,
/// optional scientific `e`/`E`, whitespace). Rejects identifiers so
/// `calculate()` cannot interpolate arbitrary Python into `python3 -c`.
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

/// Calculate expression using Python (`python3 -c`), after restricting the
/// input to a mathematical expression. The interpolating format string is
/// not a sandbox; the allowlist is.
pub fn calculate(expression: &str) -> Result<String> {
    if !is_restricted_math_expression(expression) {
        return Err(anyhow::anyhow!(
            "calculate() accepts a mathematical expression only (digits and + - * / % ( ) . e); got non-math input"
        ));
    }
    let code = format!(
        "import json; result = {}; print(json.dumps({{'result': str(result)}}))",
        expression
    );

    let result = execute_python(&code)?;

    if result.exit_code != 0 {
        return Err(anyhow::anyhow!("Calculation error: {:?}", result.error));
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

    #[test]
    fn test_is_python_available_consistent_with_version() {
        let available = is_python_available();
        if available {
            assert!(
                get_python_version().is_some(),
                "if python is available, version should be Some"
            );
        }
    }

    #[test]
    fn test_get_python_version_format_when_present() {
        let version = get_python_version();
        if let Some(v) = version {
            // Version may be "3.12.0" or "Python 3.12.0" depending on platform
            assert!(v.contains('.'), "version should contain dot separator: {v}");
            assert!(
                v.chars().any(|c| c.is_ascii_digit()),
                "version should contain digits: {v}"
            );
        }
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
    fn test_is_restricted_math_expression_accepts_formulas() {
        assert!(is_restricted_math_expression("2 + 2"));
        assert!(is_restricted_math_expression("(10 + 5) * 2"));
        assert!(is_restricted_math_expression("1e-3"));
        assert!(is_restricted_math_expression("  -4.5 / 2  "));
        assert!(!is_restricted_math_expression(""));
        assert!(!is_restricted_math_expression("__import__('os').system('id')"));
        assert!(!is_restricted_math_expression("os.system('echo pwned')"));
        assert!(!is_restricted_math_expression("2 + foo"));
    }

    #[test]
    fn test_calculate_rejects_python_injection() {
        let err = calculate("__import__('os').system('id')")
            .expect_err("arbitrary python must not reach python3 -c");
        let msg = err.to_string();
        assert!(
            msg.contains("mathematical expression only"),
            "error should name the math-only contract, got {msg}"
        );
    }
}
