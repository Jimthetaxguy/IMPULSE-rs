//! PreCompact Survival Validation Harness (PR 1.2)
//!
//! Validates that content survives through the steward compaction pipeline:
//! 1. Content with decision-like language in a transcript is extracted by
//!    `steward analyze` (extract_decisions)
//! 2. The refined context built by `steward compact --transcript` includes
//!    the extracted decision in its output
//!
//! Success criteria: Content with decision patterns survives extraction
//! and appears in `steward compact --transcript` stdout.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use tempfile::TempDir;

    fn impulse_rs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn run_steward_compact(
        impulse_dir: &Path,
        transcript_path: &Path,
        session_id: &str,
    ) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args([
                "steward",
                "compact",
                "--session-id",
                session_id,
                "--transcript",
            ])
            .arg(transcript_path)
            .current_dir(impulse_rs_dir())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("Failed to run cargo for steward compact")
    }

    fn run_steward_compact_no_transcript(
        impulse_dir: &Path,
        session_id: &str,
    ) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args(["steward", "compact", "--session-id", session_id])
            .current_dir(impulse_rs_dir())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("Failed to run cargo for steward compact")
    }

    fn stdout_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn stderr_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    // =========================================================================
    // Test: decision content survives through compact --transcript pipeline
    // =========================================================================

    #[test]
    fn test_decision_content_survives_steward_compact() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        // The "MUST_SURVIVE" marker must appear INSIDE a decision sentence,
        // with "decided to" (exact pattern). The sentence ends at the first period,
        // so the marker must be before the period.
        // Note: parser uses "type" field, not "role".
        let transcript_content = r#"{"type":"assistant","content":"I decided to use MUST_SURVIVE: TEST_CONTENT in the architecture because it should survive compaction through the decision extraction pipeline."}
{"type":"assistant","content":"I chose to keep the agent harness protocol simple with a JSON-line format."}
{"type":"assistant","content":"I will use tokio for async runtime and ratatui for the terminal UI."}
{"type":"assistant","content":"I decided to proceed with the implementation using the agent harness JSON protocol."}
"#;
        let transcript_path = tmp.path().join("transcript.jsonl");
        fs::write(&transcript_path, transcript_content.trim()).expect("Failed to write transcript");

        let output = run_steward_compact(impulse_dir, &transcript_path, "test-session-123");

        assert!(
            output.status.success(),
            "steward compact command failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("MUST_SURVIVE: TEST_CONTENT"),
            "Marker content not found in steward compact output.\n\
            The extraction pipeline only captures content within decision-pattern \
            sentences ('decided to', 'chose to', 'will use'). If MUST_SURVIVE is \
            in a plain message, it won't appear in output.\nOutput:\n{}",
            stdout
        );
    }

    // =========================================================================
    // Test: multiple decisions extracted from same transcript
    // =========================================================================

    #[test]
    fn test_multiple_decisions_extracted() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let transcript_content = r#"
{"type":"assistant","content":"I decided to use SQLite for the retrieval database."}
{"type":"assistant","content":"I chose to use rusqlite with bundled mode."}
{"type":"assistant","content":"I will use a simple JSON-line protocol for IPC between the daemon and workbench."}
"#;
        let transcript_path = tmp.path().join("transcript2.jsonl");
        fs::write(&transcript_path, transcript_content.trim()).expect("Failed to write transcript");

        let output = run_steward_compact(impulse_dir, &transcript_path, "multi-decision-session");

        assert!(
            output.status.success(),
            "steward compact failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("decided to use"),
            "First decision pattern not found in output: {}",
            stdout
        );
        assert!(
            stdout.contains("chose to use"),
            "Second decision pattern not found in output: {}",
            stdout
        );
        assert!(
            stdout.contains("will use"),
            "Third decision pattern not found in output: {}",
            stdout
        );
    }

    // =========================================================================
    // Test: files touched extracted from transcript
    // =========================================================================

    #[test]
    fn test_files_touched_extracted_from_transcript() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        // The correct JSONL format uses typed content arrays with "type":"tool_use".
        // extract_files_touched looks for file_path in Write/Read/Edit/Glob tool inputs.
        // IMPORTANT: input must be a JSON object (not a string). From existing test at
        // analyzer.rs:481: input:{"file_path":"/tmp/test.rs"} — no quotes around the object.
        // Note: parser uses "type" field, not "role".
        let transcript_content = r#"{"type":"assistant","content":[{"type":"tool_use","id":"f1","name":"Write","input":{"file_path":"src/main.rs","content":"fn main() {}"}}]}
{"type":"assistant","content":"Created the initial Rust project structure."}
{"type":"assistant","content":[{"type":"tool_use","id":"f2","name":"Write","input":{"file_path":"src/lib.rs","content":"pub fn example() {}"}}]}
"#;
        let transcript_path = tmp.path().join("transcript3.jsonl");
        fs::write(&transcript_path, transcript_content.trim()).expect("Failed to write transcript");

        let output = run_steward_compact(impulse_dir, &transcript_path, "files-session");

        assert!(
            output.status.success(),
            "steward compact failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("src/main.rs"),
            "Expected 'src/main.rs' in compact output (file_path from Write tool).\nOutput: {}",
            stdout
        );
    }

    // =========================================================================
    // Test: compact without transcript still produces output
    // =========================================================================

    #[test]
    fn test_compact_without_transcript_produces_output() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let output = run_steward_compact_no_transcript(impulse_dir, "no-transcript-session");

        assert!(
            output.status.success(),
            "steward compact (no transcript) failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("Stewardship Context") || stdout.contains("Cross-Project"),
            "Expected stewardship context header in output.\nGot: {}",
            stdout
        );
    }

    // =========================================================================
    // Test: non-decision content is NOT incorrectly preserved
    // (plain statements without decision language should not appear in output)
    // =========================================================================

    #[test]
    fn test_non_decision_content_not_in_output() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let transcript_content = r#"
{"type":"user","content":"Hello"}
{"type":"assistant","content":"Hello! How can I help you today?"}
{"type":"assistant","content":"This is a regular assistant message without any decision language. MUST_SURVIVE: this should not appear because it lacks decision patterns."}
{"type":"user","content":"Thanks"}
{"type":"assistant","content":"You're welcome."}
"#;
        let transcript_path = tmp.path().join("transcript4.jsonl");
        fs::write(&transcript_path, transcript_content.trim()).expect("Failed to write transcript");

        let output = run_steward_compact(impulse_dir, &transcript_path, "regular-session");

        assert!(
            output.status.success(),
            "steward compact failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        assert!(
            !stdout.contains("MUST_SURVIVE: this should not appear"),
            "Non-decision content incorrectly appeared in output.\n\
            Only decision-language content should survive extraction.\n\
            Output:\n{}",
            stdout
        );
    }
}
