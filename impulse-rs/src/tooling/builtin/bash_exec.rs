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

/// Truncate `s` to at most `max_bytes` bytes without panicking when
/// `max_bytes` falls in the middle of a multi-byte UTF-8 character.
/// `String::truncate` requires the index to land on a char boundary; since
/// `s` comes from `from_utf8_lossy`, a byte offset chosen purely by length
/// (`MAX_OUTPUT_BYTES`) can split a multi-byte char, so this walks backward
/// to the nearest valid boundary first.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

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
        // On timeout, the `wait_with_output` future below is dropped along with
        // the `Child` it owns; `kill_on_drop` ensures that drop sends SIGKILL
        // instead of leaving an orphaned/zombie process behind.
        cmd.kill_on_drop(true);

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

        let stdout_full = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr_full = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout_truncated = stdout_full.len() > MAX_OUTPUT_BYTES;
        let stderr_truncated = stderr_full.len() > MAX_OUTPUT_BYTES;
        let stdout = truncate_at_char_boundary(&stdout_full, MAX_OUTPUT_BYTES);
        let stderr = truncate_at_char_boundary(&stderr_full, MAX_OUTPUT_BYTES);

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

    #[test]
    fn test_truncate_at_char_boundary_does_not_panic_mid_multibyte_char() {
        // Regression test: a byte offset chosen purely by length can land in
        // the middle of a multi-byte UTF-8 character. `String::truncate`
        // panics in that case; this helper must back off to a valid boundary
        // instead of panicking, and must not lose the whole trailing char.
        let s = "a".repeat(9) + "€"; // '€' is 3 bytes, straddling byte 10 if cut at 10
        let truncated = truncate_at_char_boundary(&s, 10);
        assert!(truncated.len() <= 10);
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
        assert_eq!(truncated, "a".repeat(9));
    }

    #[test]
    fn test_truncate_at_char_boundary_leaves_short_strings_untouched() {
        assert_eq!(truncate_at_char_boundary("hi", 10), "hi");
    }

    #[tokio::test]
    async fn test_execute_kills_child_on_timeout_instead_of_orphaning() {
        // Regression test: without `kill_on_drop`, dropping the
        // `wait_with_output` future on timeout left the child process
        // running as an orphan. Spawn a command that writes a marker file
        // shortly after a delay long past the timeout; if the process were
        // still alive it would eventually write the marker. We can't wait
        // out a real orphan in a unit test, so instead assert the intended
        // mechanism directly: sh's own child (through `sleep`) must not
        // still be reachable via its reported pid after the timeout fires.
        //
        // Uses a per-invocation unique sleep duration (not a literal
        // `sleep 5`) because `cargo test` runs tests concurrently and a
        // sibling test in this same file
        // (`test_execute_times_out_on_long_running_command`) also spawns
        // `sleep 5` -- a `pgrep -f "sleep 5"` check would catch that
        // unrelated, still-legitimately-running process and report a false
        // positive. A `# comment` marker doesn't survive into the child's
        // argv (the shell strips it before exec), so the marker has to be a
        // real argument -- a fractional-second duration unique to this
        // process still sleeps ~5s but is distinguishable in `ps`/`pgrep -f`
        // output.
        let unique_duration = format!("5.{}", std::process::id() % 1000);
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"command": format!("sleep {unique_duration}"), "timeout_secs": 1}),
                &ctx,
            )
            .await;
        assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
        // Give the OS a moment to process the kill signal, then confirm no
        // stray process carrying this test's unique duration remains.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let check = tokio::process::Command::new("pgrep")
            .arg("-f")
            .arg(format!("sleep {unique_duration}"))
            .output()
            .await;
        if let Ok(check) = check {
            let stray = String::from_utf8_lossy(&check.stdout);
            assert!(
                stray.trim().is_empty(),
                "expected no orphaned process after timeout, found pids: {stray}"
            );
        }
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
