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
//!
//! **Env scrubbing (T9 follow-up, same-day adversarial review):** once T9
//! wired `bash_exec` up to the LLM's own tool-calling loop, a command as
//! innocuous-looking as `env` or `printenv ANTHROPIC_API_KEY` — run through
//! this tool after a user's human-confirmation approval — would print the
//! `ion` process's own secrets straight into `tool_result` content, which
//! then flows back into the model's context and the REPL transcript. The
//! confirmation gate in `ion_repl::chat::ReplToolExecutor` reduces but does
//! not eliminate this: a user can approve a command without realizing it
//! leaks env vars. This tool now calls `.env_clear()` on the child
//! `Command` and re-adds only a small, explicit allowlist
//! (`crate::tooling::env_scrub::ENV_ALLOWLIST`) of variables the shell
//! needs to function (`PATH`, `HOME`, `TERM`, locale/tmp vars), via the
//! shared `env_scrub::scrub_and_allowlist_env` helper — also used by
//! `src/tooling/external.rs`'s `ProcessTool` for the same reason, so the
//! allowlist/heuristic logic isn't duplicated a second time. This is
//! allowlist, not denylist — matching CLAUDE.md Principle #5's
//! deny-by-default capability philosophy: everything not explicitly named
//! is dropped, rather than trying to enumerate every possible secret name.
//! `env_scrub::is_secret_like` is an additional heuristic name-pattern
//! guard applied defensively to the allowlist itself (belt-and-suspenders
//! — none of the allowlisted names should ever match it, and a test in
//! `env_scrub` proves that).

use async_trait::async_trait;
use tokio::time::Duration;

use crate::tooling::env_scrub::scrub_and_allowlist_env;
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
        // Deny-by-default env: drop the full parent environment (which may
        // hold API keys/tokens the `ion` process itself needs) and re-add
        // only the small functional allowlist. See module doc for why this
        // is an allowlist rather than a denylist.
        scrub_and_allowlist_env(&mut cmd, &[]);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdin(std::process::Stdio::null());
        // On timeout, the `wait_with_output` future below is dropped along with
        // the `Child` it owns; `kill_on_drop` ensures that drop sends SIGKILL
        // instead of leaving an orphaned/zombie process behind.
        cmd.kill_on_drop(true);

        // `kill_on_drop`/SIGKILL only reaches the *direct* `sh` child, not
        // any process `sh` itself forks. `sh -c "<command>"` only
        // exec-replaces itself with the child for a single simple command;
        // a compound command (`cmd1 && cmd2`, `cmd1 | cmd2`) or an
        // explicitly backgrounded one (`sleep 999 &`) makes `sh` fork real
        // children instead, which survive a SIGKILL to `sh` alone -- proven
        // live (`sh -c "sleep 30 & wait"`, kill the `sh` pid, the `sleep`
        // pid is still running) and by the identical bug this exact session
        // found and fixed in `agent::harness_query_structured_with_timeout`
        // for a wrapper-script harness. Since `bash_exec` runs
        // arbitrary LLM-generated commands -- the primary surface this
        // session spent the day hardening (env-scrubbing, confirmation
        // gate, guardrail scanning) -- a hung compound/backgrounded command
        // would leave an orphan even with `kill_on_drop` alone. Fixed the
        // same way as `impulse_ion::pi_adapter`'s watchdog and
        // `agent::harness_query_structured_with_timeout`: put `sh` in its
        // own process group at spawn (pgid == its own pid) so a timeout can
        // kill the whole group, not just `sh` itself.
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn shell: {e}")))?;
        // Synchronous Drop cleanup covers explicit timeout and arbitrary
        // task cancellation; `kill_on_drop` alone reaches only direct `sh`.
        let mut process_group_guard = crate::process_group::ProcessGroupGuard::new(child.id());

        let output =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
            {
                Ok(result) => {
                    let output = result.map_err(|e| {
                        ToolError::ExecutionFailed(format!("failed to wait on child: {e}"))
                    })?;
                    process_group_guard.disarm();
                    output
                }
                Err(_elapsed) => {
                    // Returning drops the still-armed group guard after the
                    // Child future is dropped, killing backgrounded work too.
                    return Err(ToolError::ExecutionFailed(format!(
                        "command timed out after {timeout_secs}s: {command}"
                    )));
                }
            };

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

    /// Serializes tests in this module that mutate process-global secret
    /// env vars (`ANTHROPIC_API_KEY`/`OPENAI_API_KEY`-shaped names).
    /// `cargo test` runs this crate's unit tests in one process across
    /// multiple threads, so a test that sets/removes such a var must not
    /// race a sibling test doing the same — mirrors the pattern in
    /// `src/test_support.rs` (a per-file lock is fine as long as no other
    /// file's tests touch these exact var names, which is true here: the
    /// names below are test-scoped and not read/set anywhere else).
    fn secret_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// RAII guard that sets an env var for the duration of a test and
    /// restores whatever was there before (or removes it if it was unset),
    /// so a test never permanently mutates process-global state for its
    /// siblings even on an early return/panic.
    struct EnvVarGuard {
        name: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var(name).ok();
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    // `is_secret_like`/`ENV_ALLOWLIST` themselves are tested in
    // `crate::tooling::env_scrub`'s own test module (now the shared home of
    // this logic); this file keeps the integration-level regression test
    // proving `bash_exec`'s `execute()` actually applies the scrub end to
    // end (below).

    #[tokio::test]
    // clippy: the lock is a test-only std::sync::Mutex<()> (never contended
    // by production code) and must span the whole `execute().await` call so
    // no sibling test in this file can mutate the same secret env vars
    // mid-spawn — matches the justified pattern in `ion_repl::chat`'s tests.
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_scrubs_secret_env_vars_from_child_process() {
        let _lock = secret_env_lock();
        let _anthropic = EnvVarGuard::set("ANTHROPIC_API_KEY", "sk-ant-test-should-not-leak");
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", "sk-oai-test-should-not-leak");

        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({"command": "env"}), &ctx)
            .await
            .expect("env should succeed");
        let stdout = result.output["stdout"].as_str().unwrap();

        assert!(
            !stdout.contains("ANTHROPIC_API_KEY"),
            "child env leaked ANTHROPIC_API_KEY:\n{stdout}"
        );
        assert!(
            !stdout.contains("OPENAI_API_KEY"),
            "child env leaked OPENAI_API_KEY:\n{stdout}"
        );
        assert!(
            !stdout.contains("sk-ant-test-should-not-leak"),
            "child env leaked the ANTHROPIC_API_KEY value:\n{stdout}"
        );
        assert!(
            !stdout.contains("sk-oai-test-should-not-leak"),
            "child env leaked the OPENAI_API_KEY value:\n{stdout}"
        );
    }

    #[tokio::test]
    async fn test_execute_still_has_path_and_can_run_ordinary_commands() {
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();

        // PATH must survive the scrub so `sh -c` can still resolve builtins
        // and external commands like `echo`/`ls`.
        let env_result = tool
            .execute(serde_json::json!({"command": "env"}), &ctx)
            .await
            .expect("env should succeed");
        assert!(env_result.output["stdout"]
            .as_str()
            .unwrap()
            .contains("PATH="));

        let echo_result = tool
            .execute(serde_json::json!({"command": "echo still-works"}), &ctx)
            .await
            .expect("echo should still succeed after env scrub");
        assert_eq!(echo_result.output["stdout"], "still-works\n");
        assert_eq!(echo_result.output["exit_code"], 0);
    }

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
        //
        // Regression note (fresh Fable review, same day): `std::process::id()
        // % 1000` alone is NOT unique across files -- `agent::mod`'s own
        // orphan-kill test computed its marker the same way and could
        // collide (same process, same formula), letting one test's pgrep
        // catch the other's still-legitimately-running sleep. Nanoseconds
        // (matching `storage::atomic_write_path`'s own PID+nanos precedent)
        // are effectively unique per call, not just per process.
        let unique_duration = format!(
            "5.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
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
            .await
            .expect(
                "pgrep unavailable -- cannot verify orphan-kill; install pgrep or adjust the check",
            );
        let stray = String::from_utf8_lossy(&check.stdout);
        assert!(
            stray.trim().is_empty(),
            "expected no orphaned process after timeout, found pids: {stray}"
        );
    }

    #[tokio::test]
    async fn test_execute_kills_grandchild_of_a_backgrounded_compound_command_on_timeout() {
        // Regression test for the gap the above test does NOT cover: `sh -c
        // "sleep N"` (a single simple command) gets exec-replaced by `sh`,
        // so killing the `sh` pid alone happens to kill the real work too --
        // that's a property of *that* invocation shape, not a guarantee.
        // `sh -c "cmd1 & wait"` (explicitly backgrounded) makes `sh` FORK a
        // real child instead of exec-replacing itself, so a SIGKILL to `sh`
        // alone leaves that child running -- confirmed live before this
        // fix (`sh -c "sleep 30 & wait"`, kill the sh pid, the sleep pid
        // survives) and matches the identical bug this same session found
        // in `agent::harness_query_structured_with_timeout` for a
        // wrapper-script harness. Since `bash_exec` runs arbitrary
        // LLM-generated commands, an orphaned backgrounded process is a
        // real resource leak, not a hypothetical.
        //
        // Nanoseconds, not `std::process::id() % 1000` (fresh Fable review,
        // same day) -- pid-based markers aren't unique ACROSS files in the
        // same test process; see the sibling test above for the full note.
        let unique_duration = format!(
            "9.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({
                    "command": format!("sleep {unique_duration} & wait"),
                    "timeout_secs": 1
                }),
                &ctx,
            )
            .await;
        assert!(matches!(result, Err(ToolError::ExecutionFailed(_))));
        tokio::time::sleep(Duration::from_millis(300)).await;
        let check = tokio::process::Command::new("pgrep")
            .arg("-f")
            .arg(format!("sleep {unique_duration}"))
            .output()
            .await
            .expect(
                "pgrep unavailable -- cannot verify orphan-kill; install pgrep or adjust the check",
            );
        let stray = String::from_utf8_lossy(&check.stdout);
        assert!(
            stray.trim().is_empty(),
            "expected the backgrounded grandchild to be killed via the process group, \
             found pids: {stray}"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_aborting_execute_kills_the_whole_process_group() {
        let unique_duration = format!(
            "9.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let pattern = format!("sleep {unique_duration}");
        let tool = BashExecTool;
        let ctx = ToolContext::with_all_capabilities();
        let task = tokio::spawn(async move {
            tool.execute(
                serde_json::json!({
                    "command": format!("sleep {unique_duration} & wait"),
                    "timeout_secs": 30
                }),
                &ctx,
            )
            .await
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let check = tokio::process::Command::new("pgrep")
                    .arg("-f")
                    .arg(&pattern)
                    .output()
                    .await
                    .expect("pgrep should run");
                if !String::from_utf8_lossy(&check.stdout).trim().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("backgrounded command did not start");

        task.abort();
        let _ = task.await;
        let gone = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let check = tokio::process::Command::new("pgrep")
                    .arg("-f")
                    .arg(&pattern)
                    .output()
                    .await
                    .expect("pgrep should run");
                if String::from_utf8_lossy(&check.stdout).trim().is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if gone.is_err() {
            let check = tokio::process::Command::new("pgrep")
                .arg("-f")
                .arg(&pattern)
                .output()
                .await
                .expect("pgrep should run");
            let stray = String::from_utf8_lossy(&check.stdout).trim().to_string();
            let _ = tokio::process::Command::new("pkill")
                .arg("-f")
                .arg(&pattern)
                .status()
                .await;
            panic!("aborting bash_exec must kill backgrounded grandchildren; found pids: {stray}");
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
