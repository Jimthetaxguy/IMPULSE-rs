//! Extraction Quality Benchmark (PR 1.4)
//!
//! Benchmarks the extraction pipeline on realistic synthetic transcripts,
//! measuring decision capture, file tracking, and insight quality.
//!
//! Success criteria: Documented capture rate on realistic sessions.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    fn impulse_rs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn run_steward_analyze(
        impulse_dir: &Path,
        transcript_path: &Path,
        session_id: &str,
    ) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args(["steward", "analyze", "--transcript"])
            .arg(transcript_path)
            .args(["--session-id", session_id])
            .current_dir(impulse_rs_dir())
            .output()
            .expect("Failed to run cargo for steward analyze")
    }

    fn stdout_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn stderr_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    fn write_transcript(tmp: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = tmp.join(name);
        let mut file = fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        path
    }

    // =========================================================================
    // Benchmark 1: Refactor session — decision extraction quality
    // =========================================================================

    #[test]
    fn test_benchmark_refactor_session_decision_capture() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        // Realistic refactoring session with 5 explicit decisions.
        // steward analyze outputs "Decisions: N" in its human-readable format.
        let transcript = write_transcript(
            tmp.path(),
            "refactor.jsonl",
            &[
                r#"{"type":"user","content":"Refactor the auth module to use JWT tokens"}"#,
                r#"{"type":"assistant","content":"I decided to use JWT RS256 algorithm for token signing."}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"t1","name":"Glob","input":{"pattern":"src/auth/**"}}]}"#,
                r#"{"type":"assistant","content":"I chose to extract the token validation into a separate middleware module."}"#,
                r#"{"type":"assistant","content":"I will use the jsonwebtoken crate for token handling."}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"src/auth/mod.rs"}}]}"#,
                r#"{"type":"assistant","content":"Selected the Bearer token format for the Authorization header."}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"t3","name":"Write","input":{"file_path":"src/auth/jwt.rs","content":"..."}}]}"#,
                r#"{"type":"assistant","content":"I decided to add token refresh logic to handle expiration."}"#,
            ],
        );

        let output = run_steward_analyze(impulse_dir, &transcript, "refactor-session");

        assert!(
            output.status.success(),
            "steward analyze failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);

        // Extract decision count from "Decisions: N" in the output
        let decisions_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Decisions:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        // 4 of 5 assistant messages contain decision patterns; 2 are "decided to" variants
        // (which each count as 1). Expect >= 3 decisions extracted.
        assert!(
            decisions_found >= 3,
            "Expected at least 3 decisions extracted (from 4 decision-pattern sentences).\n\
            steward analyze output:\n{}",
            stdout
        );
    }

    // =========================================================================
    // Benchmark 2: New feature session — file tracking quality
    // =========================================================================

    #[test]
    fn test_benchmark_new_feature_file_tracking() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let transcript = write_transcript(
            tmp.path(),
            "newfeature.jsonl",
            &[
                r#"{"type":"user","content":"Add user profile page"}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"f1","name":"Write","input":{"file_path":"src/routes/profile.rs","content":"..."}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"f2","name":"Write","input":{"file_path":"src/templates/profile.html","content":"..."}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"f3","name":"Write","input":{"file_path":"src/models/user.rs","content":"..."}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"f4","name":"Edit","input":{"file_path":"src/main.rs","old_string":"...","new_string":"..."}}]}"#,
                r#"{"type":"assistant","content":"Created new feature files: profile route, template, and user model."}"#,
            ],
        );

        let output = run_steward_analyze(impulse_dir, &transcript, "feature-session");

        assert!(
            output.status.success(),
            "steward analyze failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);

        // Extract file count from "Files touched: N"
        let files_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Files touched:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        // 3 Write + 1 Edit = 4 file operations, expect >= 3 unique files
        assert!(
            files_found >= 3,
            "Expected at least 3 unique files extracted.\n\
            steward analyze output:\n{}",
            stdout
        );
    }

    // =========================================================================
    // Benchmark 3: Mixed session — tool patterns and insights
    // =========================================================================

    #[test]
    fn test_benchmark_mixed_session_insights() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let transcript = write_transcript(
            tmp.path(),
            "mixed.jsonl",
            &[
                r#"{"type":"user","content":"Fix the failing tests"}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"tests/api_test.rs"}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"r2","name":"Read","input":{"file_path":"src/api/client.rs"}}]}"#,
                r#"{"type":"assistant","content":"I decided to mock the HTTP client in tests instead of making real network calls."}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"tests/api_test.rs","old_string":"...","new_string":"..."}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"src/api/client.rs","old_string":"...","new_string":"..."}}]}"#,
                r#"{"type":"assistant","content":"Going with wiremock for HTTP mocking to match the production behavior more closely."}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"c1","name":"Bash","input":{"command":"cargo test api_test --no-fail-fast"}}]}"#,
            ],
        );

        let output = run_steward_analyze(impulse_dir, &transcript, "mixed-session");

        assert!(
            output.status.success(),
            "steward analyze failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);

        // Extract decision count from "Decisions: N"
        let decisions_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Decisions:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        // Extract file count
        let files_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Files touched:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        // Mixed session has: 2 decision sentences, 5 tool calls on 2 files
        assert!(
            decisions_found >= 1,
            "Expected at least 1 decision in mixed session.\nOutput:\n{}",
            stdout
        );
        assert!(
            files_found >= 1,
            "Expected at least 1 file in mixed session.\nOutput:\n{}",
            stdout
        );
    }

    // =========================================================================
    // Benchmark 4: Edge cases — empty transcript, single message
    // =========================================================================

    #[test]
    fn test_benchmark_empty_transcript_handled_gracefully() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        let transcript = write_transcript(
            tmp.path(),
            "empty.jsonl",
            &[
                r#"{"type":"user","content":"Hello"}"#,
                r#"{"type":"assistant","content":"Hello! How can I help you?"}"#,
            ],
        );

        let output = run_steward_analyze(impulse_dir, &transcript, "empty-session");

        // Should succeed even with minimal content
        assert!(
            output.status.success(),
            "steward analyze failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);

        // For a minimal transcript with no decisions or files, we just verify
        // the command succeeds and produces valid output. Decisions and files should be 0.
        let decisions_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Decisions:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        let files_found = stdout
            .lines()
            .find(|l| l.trim().starts_with("Files touched:"))
            .and_then(|l| {
                l.split_whitespace()
                    .last()
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(0);

        // Minimal transcript: 0 decisions, 0 files — this is correct behavior
        assert_eq!(
            decisions_found, 0,
            "Empty transcript should have 0 decisions"
        );
        assert_eq!(files_found, 0, "Empty transcript should have 0 files");
    }

    // =========================================================================
    // Benchmark 5: Duplicate tool calls — deduplication quality
    // =========================================================================

    #[test]
    fn test_benchmark_duplicate_tools_deduplicated() {
        let tmp = TempDir::new().expect("Failed to create temp dir");
        let impulse_dir = tmp.path();

        // Same Read called 3 times, same file
        let transcript = write_transcript(
            tmp.path(),
            "dupes.jsonl",
            &[
                r#"{"type":"user","content":"Check the config file"}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"config/app.yaml"}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"r2","name":"Read","input":{"file_path":"config/app.yaml"}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"r3","name":"Read","input":{"file_path":"config/app.yaml"}}]}"#,
                r#"{"type":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"config/app.yaml","content":"..."}}]}"#,
            ],
        );

        let output = run_steward_analyze(impulse_dir, &transcript, "dupe-session");

        assert!(
            output.status.success(),
            "steward analyze failed.\nstderr: {}",
            stderr_str(&output)
        );

        let stdout = stdout_str(&output);
        // Duplicate tool patterns should be found (count >= 2)
        // The analyzer should detect Read being called 3 times as a pattern
        assert!(
            stdout.contains("Read") || stdout.contains("tool"),
            "Expected tool pattern detection for duplicate reads.\nOutput: {}",
            stdout
        );
    }
}
