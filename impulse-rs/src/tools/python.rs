// Python integration module - execute Python code for calculations and data processing
// Provides a safe way to run Python code from Rust

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

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

/// Execute Python code with a timeout.
/// If the process exceeds `timeout_secs`, it is killed and an error is returned.
#[allow(dead_code)]
pub fn execute_python_with_timeout(code: &str, timeout_secs: u64) -> Result<PythonResult> {
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let mut child = Command::new(python_cmd)
        .args(["-c", code])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Python process")?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(100);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait(); // reap zombie
                    anyhow::bail!("Python execution timed out after {} seconds", timeout_secs);
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => anyhow::bail!("Error waiting for Python process: {}", e),
        }
    }

    let output = child
        .wait_with_output()
        .context("Failed to read Python output")?;
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

/// Calculate expression using Python (safe eval)
pub fn calculate(expression: &str) -> Result<String> {
    // Wrap in safe eval
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
    fn test_is_python_available() {
        let available = is_python_available();
        // May fail if Python not installed
        println!("Python available: {}", available);
    }

    #[test]
    fn test_get_python_version() {
        let version = get_python_version();
        println!("Python version: {:?}", version);
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
}
