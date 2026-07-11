//! Bash execution tool — run a shell command as a child process (TUI_SPEC.md T7).
//!
//! `ion` is a coding agent in the same category as `claude`/`codex`
//! (TUI_SPEC.md section 2.3's "Scope clarification"), so its REPL tool
//! registry needs a shell-execution capability alongside file read/write.
//! Requires the dedicated `Capability::ShellExec` (deny-by-default, checked
//! by `ToolRegistry::execute` before this tool ever runs — CLAUDE.md
//! Principle #5). Runs via `tokio::process::Command` (not
//! `std::process::Command`) so the command's own I/O doesn't block a runtime
//! worker thread, with a hard wall-clock timeout that kills the child on
//! expiry.

use async_trait::async_trait;
use tokio::time::Duration;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Execute a shell command (`sh -c <command>`) and return its stdout,
/// stderr, and exit code.
pub struct BashExecTool;

#[async_trait]
impl DynamicTool for BashExecTool {
    fn id(&self) -> &str {
        "bash_exec"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "bash_exec".into(),
            name: "Bash Execute".into(),
            description: "Run a shell command and return stdout/stderr/exit code".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![
                ToolParam {
                    name: "command".into(),
                    description: "Shell command to run via `sh -c`".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "cwd".into(),
                    description: "Working directory (default: current directory)".into(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "timeout_secs".into(),
                    description: "Timeout in seconds (default: 30)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(DEFAULT_TIMEOUT_SECS)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("command").and_then(|v| v.as_str()) {
            Some(cmd) if !cmd.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams(
                "missing or empty 'command' string".into(),
            )),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'command'".into()))?;
        let timeout_secs = params
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .max(1);
        let cwd = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|c| ctx.resolve_path(c));

        if let Some(cwd) = &cwd {
            if !ctx.is_path_allowed(cwd, true) {
                return Err(ToolError::PathNotAllowed(cwd.display().to_string()));
            }
        }

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        if let Some(cwd) = &cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn shell: {e}")))?;

        let output =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
                .map_err(|_| {
                    ToolError::ExecutionFailed(format!(
                        "command timed out after {timeout_secs}s: {command}"
                    ))
                })?
                .map_err(|e| ToolError::ExecutionFailed(format!("failed to wait on child: {e}")))?;

        let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout_truncated = stdout.len() > MAX_OUTPUT_BYTES;
        let stderr_truncated = stderr.len() > MAX_OUTPUT_BYTES;
        stdout.truncate(MAX_OUTPUT_BYTES);
        stderr.truncate(MAX_OUTPUT_BYTES);

        Ok(ToolResult::json(serde_json::json!({
            "command": command,
            "exit_code": output.status.code(),
            "success": output.status.success(),
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
        })))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::ShellExec]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = BashExecTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "bash_exec");
        assert_eq!(desc.params.len(), 3);
    }

    #[test]
    fn test_validate_empty_command() {
        let tool = BashExecTool;
        assert!(tool
            .validate_params(&serde_json::json!({"command": "   "}))
            .is_err());
    }

    #[test]
    fn test_validate_ok() {
        let tool = BashExecTool;
        assert!(tool
            .validate_params(&serde_json::json!({"command": "echo hi"}))
            .is_ok());
    }

    #[tokio::test]
    async fn test_execute_captures_stdout_and_exit_code() {
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}), &ctx)
            .await
            .expect("echo should succeed");
        assert_eq!(result.output["exit_code"], 0);
        assert_eq!(result.output["success"], true);
        assert_eq!(result.output["stdout"], "hello\n");
    }

    #[tokio::test]
    async fn test_execute_reports_nonzero_exit_code() {
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({"command": "exit 3"}), &ctx)
            .await
            .expect("command should run even though it exits non-zero");
        assert_eq!(result.output["exit_code"], 3);
        assert_eq!(result.output["success"], false);
    }

    #[tokio::test]
    async fn test_execute_times_out_on_long_running_command() {
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"command": "sleep 5", "timeout_secs": 1}),
                &ctx,
            )
            .await;
        assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
    }

    #[tokio::test]
    async fn test_execute_respects_cwd() {
        let tool = BashExecTool;
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("marker.txt"), "x").expect("seed file");
        let ctx = ToolContext {
            allowed_write_roots: vec![dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };
        let result = tool
            .execute(
                serde_json::json!({"command": "ls", "cwd": dir.path().display().to_string()}),
                &ctx,
            )
            .await
            .expect("ls should succeed");
        assert!(result.output["stdout"]
            .as_str()
            .unwrap()
            .contains("marker.txt"));
    }
}
