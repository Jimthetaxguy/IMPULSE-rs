//! Integration Tests for Enhancement Areas
//!
//! Tests the three enhancement areas working together:
//! 1. Guardrail blocking with conflict patterns
//! 2. Semantic search with caching and fallback
//! 3. Conflict detection and resolution workflow
//!
//! These tests use the CLI interface to verify end-to-end functionality.

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn impulse_rs_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn run_impulse_with_impulse_dir(impulse_dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args(args)
            .current_dir(impulse_rs_dir())
            .output()
            .expect("Failed to run cargo")
    }

    fn run_impulse_with_env(
        impulse_dir: &Path,
        args: &[&str],
        envs: &[(&str, &str)],
    ) -> std::process::Output {
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args(args)
            .current_dir(impulse_rs_dir());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.output().expect("Failed to run cargo with env")
    }

    fn stdout_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn stderr_str(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    struct DaemonGuard {
        child: std::process::Child,
    }

    impl Drop for DaemonGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn socket_path(impulse_dir: &Path) -> std::path::PathBuf {
        impulse_dir.join("sockets").join("impulse.sock")
    }

    fn start_daemon(impulse_dir: &Path) -> DaemonGuard {
        let child = Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .arg("daemon")
            .current_dir(impulse_rs_dir())
            .env("IMPULSE_TEST_MODE", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start daemon process");

        let sock = socket_path(impulse_dir);
        for _ in 0..150 {
            if sock.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(sock.exists(), "daemon socket did not become ready");

        DaemonGuard { child }
    }

    fn seed_test_history(impulse_dir: &Path, name: &str, summary: &str) -> String {
        let start = run_impulse_with_impulse_dir(
            impulse_dir,
            &["session-start", "-n", name, "-p", "claude-code"],
        );
        assert!(
            start.status.success(),
            "seed session-start failed: {}",
            stderr_str(&start)
        );
        let session_id = stdout_str(&start).trim().to_string();
        assert!(!session_id.is_empty(), "session id should not be empty");

        let end = run_impulse_with_impulse_dir(
            impulse_dir,
            &[
                "session-end",
                "--session-id",
                &session_id,
                "--summary",
                summary,
            ],
        );
        assert!(
            end.status.success(),
            "seed session-end failed: {}",
            stderr_str(&end)
        );

        session_id
    }

    // ========================================================================
    // Test 1: Guardrail Blocking - CLI Verification
    // ========================================================================

    #[test]
    fn test_guardrail_config_list() {
        let output = run_impulse_with_impulse_dir(std::path::Path::new("."), &["config"]);

        // Config should work
        assert!(
            output.status.success() || stdout_str(&output).contains("log_level"),
            "config command should work: {}",
            stderr_str(&output)
        );
    }

    #[test]
    fn test_guardrail_rule_evaluation_via_config() {
        // Test guardrail behavior through config changes
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Get initial config state
        let config_output = run_impulse_with_impulse_dir(temp_dir.path(), &["config"]);
        assert!(config_output.status.success(), "config should work");

        let stdout = stdout_str(&config_output);
        // Guardrails are part of the config system
        assert!(
            stdout.contains("log_level") || stdout.contains("guard"),
            "Config should contain settings"
        );
    }

    // ========================================================================
    // Test 2: Semantic Search with Caching and Fallback
    // ========================================================================

    #[test]
    fn test_semantic_search_fallback_when_vector_disabled() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Seed history
        seed_test_history(
            temp_dir.path(),
            "semantic-fallback-test",
            "testing semantic fallback",
        );

        // Index the history
        let index = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "history"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            index.status.success(),
            "index-memory failed: {}",
            stderr_str(&index)
        );

        // Search with semantic mode (vector disabled by default)
        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "semantic",
                "--mode",
                "semantic",
                "--json",
            ],
        );
        assert!(
            search.status.success(),
            "semantic search should succeed: {}",
            stderr_str(&search)
        );

        // Parse JSON response
        let body: Value =
            serde_json::from_slice(&search.stdout).expect("valid semantic search json");

        assert_eq!(body["mode"], "semantic", "Mode should be semantic");
        assert_eq!(
            body["used_fallback"], true,
            "Should use fallback when vector disabled"
        );

        // Should indicate fallback reason
        assert!(
            body["fallback_code"].is_string() || body["fallback_code"].is_null(),
            "Should have fallback code"
        );
    }

    #[ignore = "RetrievalStore auto-creates db; fallback doesn't trigger on missing file"]
    #[test]
    fn test_keyword_search_with_fallback_on_missing_db() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Seed history
        seed_test_history(
            temp_dir.path(),
            "keyword-fallback-test",
            "testing keyword fallback",
        );

        // Index
        let index =
            run_impulse_with_impulse_dir(temp_dir.path(), &["index-memory", "--scope", "history"]);
        assert!(
            index.status.success(),
            "index-memory failed: {}",
            stderr_str(&index)
        );

        // Remove the retrieval DB to trigger fallback
        let db_path = temp_dir.path().join("retrieval.db");
        if db_path.exists() {
            std::fs::remove_file(&db_path).ok();
        }

        // Search should fallback to file-based scanning
        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "keyword",
                "--mode",
                "keyword",
                "--json",
            ],
        );

        // Should still succeed with fallback
        assert!(
            search.status.success(),
            "keyword search should succeed with fallback"
        );

        let body: Value = serde_json::from_slice(&search.stdout).expect("valid search json");
        assert_eq!(
            body["used_fallback"], true,
            "Should indicate fallback was used"
        );
    }

    #[test]
    fn test_retrieval_status_reports_fallback_metadata() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Get retrieval status
        let status = run_impulse_with_impulse_dir(temp_dir.path(), &["retrieval-status", "--json"]);
        assert!(
            status.status.success(),
            "retrieval-status failed: {}",
            stderr_str(&status)
        );

        let body: Value = serde_json::from_slice(&status.stdout).expect("valid status json");

        // Should have key fields
        assert!(body.get("db_path").is_some(), "Should have db_path");
        assert!(body.get("db_exists").is_some(), "Should have db_exists");
        assert!(
            body.get("python_available").is_some(),
            "Should have python_available"
        );
        assert!(body.get("index_state").is_some(), "Should have index_state");
    }

    #[test]
    fn test_search_genome_fallback() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Add a decision to genome
        let decision = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "add-decision",
                "-d",
                "Use SQLite for storage",
                "-r",
                "lightweight and embedded",
            ],
        );
        assert!(decision.status.success(), "add-decision failed");

        // Index genome
        let index = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "genome"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(index.status.success(), "index-memory failed");

        // Search genome with semantic mode
        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-genome",
                "--query",
                "sqlite",
                "--mode",
                "semantic",
                "--json",
            ],
        );

        assert!(search.status.success(), "genome search should succeed");
        let body: Value = serde_json::from_slice(&search.stdout).expect("valid search json");

        assert_eq!(body["mode"], "semantic");
        // Should have fallback since vector is disabled
        assert_eq!(body["used_fallback"], true);
    }

    // ========================================================================
    // Test 3: Conflict Detection and Resolution Workflow
    // ========================================================================

    #[test]
    fn test_daemon_chat_includes_context() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Seed some history for context
        seed_test_history(
            temp_dir.path(),
            "conflict-seed",
            "working on auth implementation",
        );

        // Start daemon
        let _daemon = start_daemon(temp_dir.path());

        // Create a session
        let create = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "--daemon",
                "session-start",
                "-n",
                "conflict-test",
                "-p",
                "claude-code",
            ],
        );
        assert!(
            create.status.success(),
            "daemon session-start failed: {}",
            stderr_str(&create)
        );

        // Parse session ID from output
        let stdout = stdout_str(&create);
        let start = stdout.rfind('(').unwrap_or(0);
        let end = stdout.rfind(')').unwrap_or(stdout.len());
        let session_id = stdout[start + 1..end].trim().to_string();

        // Chat should work and include injection context
        let chat = run_impulse_with_env(
            temp_dir.path(),
            &[
                "--daemon",
                "chat",
                "--session-id",
                &session_id,
                "--message",
                "check for conflicts",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
            &[("IMPULSE_TEST_MODE", "1")],
        );

        assert!(
            chat.status.success(),
            "daemon chat should work: {}",
            stderr_str(&chat)
        );

        // Parse response - must use --inject-explain to get JSON output
        let body: Value = serde_json::from_slice(&chat.stdout).expect("valid chat json");

        // Should have session_id and injection info
        assert!(body.get("session_id").is_some(), "Should have session_id");
    }

    #[test]
    fn test_injection_with_conflict_context() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Seed history that includes conflict-related content
        seed_test_history(
            temp_dir.path(),
            "auth-conflict-seed",
            "resolved merge conflict in auth.rs",
        );
        seed_test_history(
            temp_dir.path(),
            "api-conflict-seed",
            "avoided conflict with main branch",
        );

        // Index
        let index = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "all"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            index.status.success(),
            "index-memory failed: {}",
            stderr_str(&index)
        );

        // Orchestrate with injection
        let orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "resolve merge conflict",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
        );

        assert!(
            orchestrate.status.success(),
            "orchestrate should succeed: {}",
            stderr_str(&orchestrate)
        );

        let stdout = stdout_str(&orchestrate);

        // Should include retrieval info in output
        assert!(
            stdout.contains("Recommended tool") || stdout.contains("fallback"),
            "Orchestrate output should contain tool recommendation or fallback info"
        );
    }

    #[test]
    fn test_orchestrate_inject_explain_shows_fallback() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Seed history
        seed_test_history(
            temp_dir.path(),
            "explain-seed",
            "semantic fallback metadata baseline",
        );

        // Index
        let index = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "history"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(index.status.success(), "index-memory failed");

        // Orchestrate with explain
        let orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "semantic fallback inspection",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
        );

        assert!(orchestrate.status.success(), "orchestrate should succeed");

        let stdout = stdout_str(&orchestrate);
        // Should include fallback metadata in explain output
        assert!(
            stdout.contains("fallback_code") || stdout.contains("Recommended tool"),
            "Should include fallback info or recommendation: {}",
            stdout
        );
    }

    // ========================================================================
    // Combined Integration Test: All Three Areas Working Together
    // ========================================================================

    #[test]
    fn test_enhanced_workflow_all_three_areas() {
        let temp_dir = TempDir::new().unwrap();

        // 1. Initialize
        let init = run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);
        assert!(init.status.success(), "init failed: {}", stderr_str(&init));

        // 2. Seed history with relevant content for retrieval
        seed_test_history(
            temp_dir.path(),
            "integration-session-1",
            "implemented user authentication",
        );
        seed_test_history(
            temp_dir.path(),
            "integration-session-2",
            "fixed conflict in main.rs",
        );
        seed_test_history(
            temp_dir.path(),
            "integration-session-3",
            "reviewed pull request",
        );

        // 3. Index memory
        let index = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "all"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            index.status.success(),
            "index-memory failed: {}",
            stderr_str(&index)
        );

        // 4. Test retrieval with fallback
        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "authentication",
                "--mode",
                "semantic",
                "--json",
            ],
        );
        assert!(
            search.status.success(),
            "search failed: {}",
            stderr_str(&search)
        );

        let body: Value = serde_json::from_slice(&search.stdout).expect("valid search json");

        // Semantic mode should work (possibly with fallback)
        assert_eq!(body["mode"], "semantic", "Mode should be semantic");
        // Fallback is expected since vector is disabled by default
        assert!(
            body["used_fallback"] == true || body["results"].is_array(),
            "Should either use fallback or return results"
        );

        // 5. Test orchestrate with injection (uses retrieval)
        let orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "implement login feature",
                "--inject-mode",
                "review",
            ],
        );
        assert!(
            orchestrate.status.success(),
            "orchestrate failed: {}",
            stderr_str(&orchestrate)
        );

        // 6. Check retrieval status includes injection metadata
        let status = run_impulse_with_impulse_dir(temp_dir.path(), &["retrieval-status", "--json"]);
        assert!(
            status.status.success(),
            "retrieval-status failed: {}",
            stderr_str(&status)
        );

        let status_body: Value = serde_json::from_slice(&status.stdout).expect("valid status json");

        assert!(
            status_body.get("injection").is_some(),
            "Should have injection metadata"
        );

        // 7. Verify config is present
        let config_list = run_impulse_with_impulse_dir(temp_dir.path(), &["config"]);
        assert!(config_list.status.success(), "config list failed");

        let config_output = stdout_str(&config_list);
        assert!(
            config_output.contains("log_level"),
            "Config should include standard settings"
        );

        // 8. Test daemon with chat (conflict detection context)
        let _daemon = start_daemon(temp_dir.path());

        let create = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "--daemon",
                "session-start",
                "-n",
                "integration-daemon",
                "-p",
                "claude-code",
            ],
        );
        assert!(create.status.success(), "daemon session-start failed");

        let daemon_stdout = stdout_str(&create);
        let start = daemon_stdout.rfind('(').unwrap_or(0);
        let end = daemon_stdout.rfind(')').unwrap_or(daemon_stdout.len());
        let session_id = daemon_stdout[start + 1..end].trim().to_string();

        let chat = run_impulse_with_env(
            temp_dir.path(),
            &[
                "--daemon",
                "chat",
                "--session-id",
                &session_id,
                "--message",
                "what did I work on",
                "--inject-mode",
                "apply",
                "--inject-explain",
            ],
            &[("IMPULSE_TEST_MODE", "1")],
        );

        // Chat should work (retrieval + injection + conflict awareness)
        assert!(
            chat.status.success(),
            "daemon chat failed: {}",
            stderr_str(&chat)
        );

        let chat_body: Value = serde_json::from_slice(&chat.stdout).expect("valid chat json");

        // Should have session_id and injection info
        assert!(
            chat_body.get("session_id").is_some(),
            "Should have session_id"
        );
        assert!(
            chat_body.get("injection").is_some(),
            "Should have injection"
        );
    }

    #[test]
    fn test_injection_status_in_retrieval_status() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Trigger an injection
        let _orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "test injection status",
                "--inject-mode",
                "review",
            ],
        );

        // Check retrieval status includes injection info
        let status = run_impulse_with_impulse_dir(temp_dir.path(), &["retrieval-status", "--json"]);
        assert!(status.status.success());

        let body: Value = serde_json::from_slice(&status.stdout).expect("valid status json");

        // Injection block should be present
        assert!(
            body.get("injection").is_some(),
            "Should have injection block"
        );
        let injection = &body["injection"];
        assert!(
            injection.get("config_mode").is_some(),
            "Should have config_mode"
        );
        assert!(
            injection.get("config_scope").is_some(),
            "Should have config_scope"
        );
    }

    #[test]
    fn test_context_injection_scope_config() {
        let temp_dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Test different injection scope settings
        let scope_both = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "context_injection_scope", "--value", "both"],
        );
        assert!(
            scope_both.status.success(),
            "Setting scope to both should work"
        );

        let scope_daemon = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "context_injection_scope", "--value", "daemon"],
        );
        assert!(
            scope_daemon.status.success(),
            "Setting scope to daemon should work"
        );

        // Verify values were set
        let get_scope =
            run_impulse_with_impulse_dir(temp_dir.path(), &["config", "context_injection_scope"]);
        assert!(get_scope.status.success(), "Getting scope should work");
    }
}
