//! SessionStart Hook Validation Harness (PR 1.1)
//!
//! Validates that the SessionStart hook mechanism correctly:
//! 1. Emits the IMPULSE_HOOK_SENTINEL marker to stdout when `IMPULSE_HOOK_SENTINEL=1`
//! 2. Writes hook evidence to `.impulse/validation/runtime/hook-events.jsonl`
//!    when `IMPULSE_HOOK_EVIDENCE=1`
//!
//! Success criteria: Sentinel marker found in stdout AND evidence record found in hook-events.jsonl.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn impulse_rs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn run_session_start(impulse_dir: &Path, envs: &[(&str, &str)]) -> std::process::Output {
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args([
                "session-start",
                "-n",
                "hook-validation-test",
                "-p",
                "claude-code",
            ])
            .current_dir(impulse_rs_dir());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.output().expect("Failed to run cargo for session-start")
    }

    fn stdout_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn stderr_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    fn wait_for_file(path: &Path, max_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < max_ms as u128 {
            if path.exists() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    // =========================================================================
    // Test: session_start sentinel emitted to stdout
    // =========================================================================

    #[test]
    fn test_session_start_sentinel_emitted_to_stdout() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_session_start(
            impulse_dir,
            &[
                ("IMPULSE_HOOK_SENTINEL", "1"),
                ("IMPULSE_HOOK_EVIDENCE", "1"),
            ],
        );

        assert!(
            output.status.success(),
            "session-start command failed unexpectedly.\nstdout: {}\nstderr: {}",
            stdout_str(&output),
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("IMPULSE_HOOK_SENTINEL:"),
            "Sentinel marker not found in stdout.\nGot: {}",
            stdout
        );
        assert!(
            stdout.contains("If you can explain this marker"),
            "Sentinel explanation text not found in stdout.\nGot: {}",
            stdout
        );
    }

    // =========================================================================
    // Test: session_start hook evidence written to hook-events.jsonl
    // =========================================================================

    #[test]
    fn test_session_start_evidence_written_to_jsonl() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_session_start(
            impulse_dir,
            &[
                ("IMPULSE_HOOK_SENTINEL", "1"),
                ("IMPULSE_HOOK_EVIDENCE", "1"),
            ],
        );

        assert!(
            output.status.success(),
            "session-start command failed.\nstderr: {}",
            stderr_str(&output)
        );

        let evidence_path = impulse_dir
            .join("validation")
            .join("runtime")
            .join("hook-events.jsonl");

        let found = wait_for_file(&evidence_path, 2000);
        assert!(
            found,
            "hook-events.jsonl was not created at {:?}",
            evidence_path
        );

        let content = fs::read_to_string(&evidence_path).expect("Failed to read hook-events.jsonl");
        assert!(!content.trim().is_empty(), "hook-events.jsonl is empty");

        let lines: Vec<&str> = content.lines().collect();
        assert!(
            !lines.is_empty(),
            "Expected at least one JSONL record, got {} lines",
            lines.len()
        );

        let first_record: serde_json::Value = lines[0]
            .parse()
            .expect("First hook-events.jsonl line is not valid JSON");
        assert_eq!(
            first_record.get("event").and_then(|v| v.as_str()),
            Some("session_start"),
            "Expected event type 'session_start', got: {}",
            first_record
        );
        assert!(
            first_record
                .get("session_id")
                .and_then(|v| v.as_str())
                .is_some(),
            "Expected session_id in evidence record: {}",
            first_record
        );
    }

    // =========================================================================
    // Test: sentinel NOT emitted when IMPULSE_HOOK_SENTINEL is not set
    // =========================================================================

    #[test]
    fn test_session_start_no_sentinel_without_env_flag() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_session_start(
            impulse_dir,
            &[
                ("IMPULSE_HOOK_SENTINEL", "0"),
                ("IMPULSE_HOOK_EVIDENCE", "1"),
            ],
        );

        assert!(
            output.status.success(),
            "session-start command failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            !stdout.contains("IMPULSE_HOOK_SENTINEL:"),
            "Sentinel should NOT appear when IMPULSE_HOOK_SENTINEL=0.\nGot: {}",
            stdout
        );
    }

    // =========================================================================
    // Test: evidence NOT written when IMPULSE_HOOK_EVIDENCE is not set
    // =========================================================================

    #[test]
    fn test_session_start_no_evidence_without_env_flag() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_session_start(
            impulse_dir,
            &[
                ("IMPULSE_HOOK_SENTINEL", "1"),
                ("IMPULSE_HOOK_EVIDENCE", "0"),
            ],
        );

        assert!(
            output.status.success(),
            "session-start command failed.\nstderr: {}",
            stderr_str(&output)
        );

        let evidence_path = impulse_dir
            .join("validation")
            .join("runtime")
            .join("hook-events.jsonl");

        assert!(
            !evidence_path.exists(),
            "hook-events.jsonl should NOT be created when IMPULSE_HOOK_EVIDENCE=0.\nPath: {:?}",
            evidence_path
        );
    }

    // =========================================================================
    // Test: both sentinel and evidence with IMPULSE_HOOK_SENTINEL=true (truthy)
    // =========================================================================

    #[test]
    fn test_session_start_sentinel_and_evidence_with_truthy_env() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_session_start(
            impulse_dir,
            &[
                ("IMPULSE_HOOK_SENTINEL", "true"),
                ("IMPULSE_HOOK_EVIDENCE", "true"),
            ],
        );

        assert!(
            output.status.success(),
            "session-start command failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("IMPULSE_HOOK_SENTINEL:"),
            "Sentinel should appear with IMPULSE_HOOK_SENTINEL=true.\nGot: {}",
            stdout
        );

        let evidence_path = impulse_dir
            .join("validation")
            .join("runtime")
            .join("hook-events.jsonl");
        let found = wait_for_file(&evidence_path, 2000);
        assert!(
            found,
            "hook-events.jsonl should be created with IMPULSE_HOOK_EVIDENCE=true"
        );
    }
}
