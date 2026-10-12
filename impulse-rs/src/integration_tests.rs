//! Integration Tests for Hooks
//! End-to-end tests for CLI hooks and session lifecycle

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use serde_json::Value;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Get the impulse-rs directory
    fn impulse_rs_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Test helper: Run cargo with args from impulse-rs directory
    fn run_impulse(args: &[&str]) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--"])
            .args(args)
            .current_dir(impulse_rs_dir())
            .output()
            .expect("Failed to run cargo")
    }

    /// Test helper: Run cargo with custom impulse dir
    fn run_impulse_with_impulse_dir(
        impulse_dir: &std::path::Path,
        args: &[&str],
    ) -> std::process::Output {
        Command::new("cargo")
            .args(["run", "--", "-c"])
            .arg(impulse_dir)
            .args(args)
            .current_dir(impulse_rs_dir())
            .output()
            .expect("Failed to run cargo")
    }

    fn run_impulse_with_env(
        impulse_dir: &std::path::Path,
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

    /// Create a TempDir and initialize it as an impulse directory.
    /// Returns the TempDir (caller must hold it to keep the dir alive).
    fn init_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        run_impulse_with_impulse_dir(dir.path(), &["init"]);
        dir
    }

    /// Create a TempDir with a `.impulse` subdirectory (for steward tests that
    /// need the nested layout without running `init`).
    fn init_steward_dir() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let impulse_dir = dir.path().join(".impulse");
        std::fs::create_dir_all(&impulse_dir).unwrap();
        (dir, impulse_dir)
    }

    /// Initialize a test dir, start a session, and return (TempDir, session_id).
    fn start_session(session_name: &str) -> (TempDir, String) {
        let dir = init_test_dir();
        let start = run_impulse_with_impulse_dir(
            dir.path(),
            &["session-start", "-n", session_name, "-p", "claude-code"],
        );
        assert!(start.status.success(), "session-start failed");
        let session_id = stdout_str(&start).trim().to_string();
        assert!(!session_id.is_empty(), "session id should not be empty");
        (dir, session_id)
    }

    /// End a session with a summary.
    fn end_session(impulse_dir: &std::path::Path, session_id: &str, summary: &str) {
        let end = run_impulse_with_impulse_dir(
            impulse_dir,
            &[
                "session-end",
                "--session-id",
                session_id,
                "--summary",
                summary,
            ],
        );
        assert!(
            end.status.success(),
            "session-end failed: {}",
            stderr_str(&end)
        );
    }

    /// Parse stdout of an Output as JSON Value.
    fn parse_json_stdout(output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("valid JSON in stdout")
    }

    struct DaemonGuard {
        child: Child,
    }

    impl Drop for DaemonGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn socket_path(impulse_dir: &std::path::Path) -> std::path::PathBuf {
        impulse_dir.join("sockets").join("impulse.sock")
    }

    fn start_daemon(impulse_dir: &std::path::Path) -> DaemonGuard {
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
        // Wait up to 15 seconds for daemon socket (build can be slow under load)
        for _ in 0..150 {
            if sock.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(sock.exists(), "daemon socket did not become ready");

        DaemonGuard { child }
    }

    fn parse_daemon_session_id(output: &str) -> Option<String> {
        let start = output.rfind('(')?;
        let end = output.rfind(')')?;
        if end <= start + 1 {
            return None;
        }
        Some(output[start + 1..end].trim().to_string())
    }

    fn seed_retrieval_history(impulse_dir: &std::path::Path, name: &str, summary: &str) {
        let start = run_impulse_with_impulse_dir(
            impulse_dir,
            &["session-start", "-n", name, "-p", "claude-code"],
        );
        assert!(start.status.success(), "seed session-start failed");
        let session_id = stdout_str(&start).trim().to_string();
        assert!(
            !session_id.is_empty(),
            "seed session id should not be empty"
        );

        end_session(impulse_dir, &session_id, summary);

        let index = run_impulse_with_env(
            impulse_dir,
            &["index-memory", "--scope", "all"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            index.status.success(),
            "seed index-memory failed: {}",
            stderr_str(&index)
        );
    }

    // =========================================================================
    // Deduplication helpers — coordinator / insight tests
    // =========================================================================

    /// Build an `ExtractedInsight` in one line instead of seven.
    fn insight(
        pane_id: usize,
        agent_kind: crate::context_lifecycle::types::AgentKind,
        insight_type: crate::context_lifecycle::types::InsightType,
        content: &str,
    ) -> crate::context_lifecycle::types::ExtractedInsight {
        crate::context_lifecycle::types::ExtractedInsight {
            pane_id,
            agent_kind,
            timestamp: chrono::Utc::now(),
            insight_type,
            content: content.to_string(),
            intent: None,
        }
    }

    /// Shorthand: file-modified insight (the most common variant).
    fn file_mod(
        pane_id: usize,
        agent_kind: crate::context_lifecycle::types::AgentKind,
        file: &str,
    ) -> crate::context_lifecycle::types::ExtractedInsight {
        insight(
            pane_id,
            agent_kind,
            crate::context_lifecycle::types::InsightType::FileModified,
            file,
        )
    }

    // =========================================================================
    // Deduplication helpers — State / config tests
    // =========================================================================

    /// Create a `TempDir` + `State` pair for agent-config unit tests.
    fn new_state() -> (TempDir, crate::state::State) {
        let dir = TempDir::new().unwrap();
        let state =
            crate::state::State::new(dir.path().to_path_buf()).expect("State::new should succeed");
        (dir, state)
    }

    // =========================================================================
    // Deduplication helpers — search / retrieval tests
    // =========================================================================

    /// Create an isolated session, end it, and index history — ready for
    /// search-history assertions.
    fn session_with_history_index(name: &str, summary: &str) -> (TempDir, String) {
        let (dir, session_id) = start_session(name);
        end_session(dir.path(), &session_id, summary);
        let index =
            run_impulse_with_impulse_dir(dir.path(), &["index-memory", "--scope", "history"]);
        assert!(
            index.status.success(),
            "index-memory --scope history failed: {}",
            stderr_str(&index)
        );
        (dir, session_id)
    }

    /// Set the three config keys needed for vector-search tests.
    fn configure_vector_search(impulse_dir: &std::path::Path) {
        let _ = run_impulse_with_impulse_dir(
            impulse_dir,
            &["config", "retrieval_backend", "--value", "fts+vec"],
        );
        let _ = run_impulse_with_impulse_dir(
            impulse_dir,
            &["config", "retrieval_vector_enabled", "--value", "true"],
        );
    }

    // =========================================================================
    // Tests
    // =========================================================================

    /// Test: Initialize impulse directory
    #[test]
    fn test_init_command() {
        let temp_dir = TempDir::new().unwrap();

        let output = run_impulse_with_impulse_dir(temp_dir.path(), &["init"]);

        // Check .impulse directory was created inside temp_dir (or use as impulse dir directly)
        // When -c is passed, that IS the impulse directory, not the parent
        let impulse_dir = temp_dir.path();

        // The init should have created the sockets subdirectory
        assert!(
            impulse_dir.join("sockets").exists() || output.status.success(),
            "Impulse directory should be initialized: {:?}",
            output
        );
        assert!(
            impulse_dir.join("context").exists() || output.status.success(),
            "Context directory should be initialized: {:?}",
            output
        );
    }

    /// Test: Session lifecycle (create, track, end)
    #[test]
    fn test_session_lifecycle() {
        let temp_dir = init_test_dir();

        // Create session
        let output = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "session-start",
                "-n",
                "integration-test",
                "-p",
                "claude-code",
            ],
        );

        let stdout = stdout_str(&output);

        // Should output session ID or succeed
        assert!(
            output.status.success() || stdout.contains("session") || stdout.contains("Session"),
            "Session should be created: {} - {}",
            stdout,
            stderr_str(&output)
        );
    }

    /// Test: Config get/set operations
    #[test]
    fn test_config_operations() {
        // List config
        let output = run_impulse(&["config"]);
        let stdout = stdout_str(&output);

        // Should show config values
        assert!(
            stdout.contains("log_level") || output.status.success(),
            "Config list should work: {}",
            stdout
        );

        // Get specific config
        let output = run_impulse(&["config", "log_level"]);
        let stdout = stdout_str(&output);

        // Should show log_level value
        assert!(
            stdout.contains("info") || stdout.contains("debug") || output.status.success(),
            "Config get should work: {}",
            stdout
        );
    }

    /// Test: Status command
    #[test]
    fn test_status_command() {
        // Check status
        let output = run_impulse(&["status"]);
        let stdout = stdout_str(&output);

        // Status should work even with no sessions
        assert!(
            output.status.success() || stdout.contains("0") || stdout.contains("sessions"),
            "Status should work: {}",
            stdout
        );
    }

    /// Test: History command
    #[test]
    fn test_history_command() {
        // Check history
        let output = run_impulse(&["history"]);

        // Should work with empty history
        assert!(
            output.status.success(),
            "History should work: {}",
            stderr_str(&output)
        );
    }

    /// Test: List providers
    #[test]
    fn test_list_providers() {
        // List providers
        let output = run_impulse(&["list-providers"]);
        let stdout = stdout_str(&output);

        // Should show available providers
        assert!(
            stdout.contains("anthropic")
                || stdout.contains("openai")
                || stdout.contains("minimax")
                || output.status.success(),
            "List providers should work: {}",
            stdout
        );
    }

    /// Test: Activity command
    #[test]
    fn test_activity_command() {
        // Check activity
        let output = run_impulse(&["activity", "--limit", "5"]);

        // Should show activity (may be empty)
        assert!(
            output.status.success(),
            "Activity should work: {}",
            stderr_str(&output)
        );
    }

    /// Test: Genome command
    #[test]
    fn test_genome_command() {
        // Check genome
        let output = run_impulse(&["genome"]);

        // Should work
        assert!(
            output.status.success(),
            "Genome should work: {}",
            stderr_str(&output)
        );
    }

    /// Test: Add decision to genome (uses isolated temp dir to avoid polluting real GENOME)
    #[test]
    fn test_add_decision() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Initialize the temp impulse dir with a valid genome
        let genome_path = tmp.path().join("GENOME.md");
        std::fs::write(
            &genome_path,
            r#"{"decisions":[],"preferences":[],"constraints":[],"last_updated":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let output = run_impulse_with_impulse_dir(
            tmp.path(),
            &[
                "add-decision",
                "-d",
                "Test decision for integration",
                "-r",
                "Testing integration flow",
            ],
        );

        let stdout = stdout_str(&output);

        // Should add decision
        assert!(
            output.status.success() || stdout.contains("decision") || stdout.contains("added"),
            "Add decision should work: {}",
            stdout
        );

        // Verify it was written to the temp dir, not the real one
        let genome_content = std::fs::read_to_string(&genome_path).unwrap();
        assert!(
            genome_content.contains("Test decision for integration"),
            "Decision should be in temp genome: {}",
            genome_content
        );
    }

    /// Test: Session info for non-existent session
    #[test]
    fn test_session_info_nonexistent() {
        // Get info for non-existent session
        let output = run_impulse(&["session-info", "nonexistent-123"]);
        let stdout = stdout_str(&output);

        // The command handles a missing session as an explicit, successful
        // lookup result. Do not let an unrelated initialization failure satisfy
        // this assertion merely because the process exited nonzero.
        assert!(
            output.status.success() && stdout.contains("Session not found"),
            "Session info should report the missing ID: stdout={stdout:?}, stderr={:?}",
            stderr_str(&output)
        );
    }

    /// Test: List sessions when empty
    #[test]
    fn test_list_sessions_empty() {
        // List sessions
        let output = run_impulse(&["list-sessions"]);

        // Should work with empty list
        assert!(
            output.status.success(),
            "List sessions should work: {}",
            stderr_str(&output)
        );
    }

    /// Test: Orchestration helper command
    #[test]
    fn test_orchestrate_command() {
        let output = run_impulse(&["orchestrate", "--task", "architecture review"]);
        let stdout = stdout_str(&output);
        assert!(
            output.status.success() && stdout.contains("Recommended tool"),
            "Orchestrate should return a recommendation: {}",
            stdout
        );
    }

    /// Test: Handoff and sync context commands
    #[test]
    fn test_handoff_and_sync_context_commands() {
        let temp_dir = init_test_dir();

        let handoff = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["handoff", "--tool", "codex", "--task", "Fix tests"],
        );
        assert!(
            handoff.status.success(),
            "handoff should work: {}",
            stderr_str(&handoff)
        );

        let sync = run_impulse_with_impulse_dir(temp_dir.path(), &["sync-context"]);
        assert!(
            sync.status.success(),
            "sync-context should work: {}",
            stderr_str(&sync)
        );
    }

    #[test]
    fn test_index_memory_success_on_seeded_data() {
        let (temp_dir, session_id) = start_session("retrieval-seeded");
        end_session(temp_dir.path(), &session_id, "retrieval foundation summary");

        let decision = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "add-decision",
                "-d",
                "Prefer FTS baseline first",
                "-r",
                "safe rollout",
            ],
        );
        assert!(
            decision.status.success(),
            "add-decision failed: {}",
            stderr_str(&decision)
        );

        let index =
            run_impulse_with_impulse_dir(temp_dir.path(), &["index-memory", "--scope", "all"]);
        assert!(
            index.status.success(),
            "index-memory failed: {}",
            stderr_str(&index)
        );
    }

    #[test]
    fn test_search_history_keyword_returns_expected_session() {
        let (temp_dir, session_id) =
            session_with_history_index("retrieval-history", "keyword retrieval smoke summary");

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
        assert!(
            search.status.success(),
            "search-history failed: {}",
            stderr_str(&search)
        );

        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["mode"], "keyword");
        assert_eq!(body["used_fallback"], false);
        let results = body["results"].as_array().expect("results array");
        assert!(!results.is_empty(), "expected at least one history result");
        assert_eq!(results[0]["id"], session_id);
    }

    #[test]
    fn test_search_genome_keyword_returns_expected_decision() {
        let temp_dir = init_test_dir();

        let decision = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "add-decision",
                "-d",
                "Use retrieval status checks",
                "-r",
                "ops visibility",
            ],
        );
        assert!(decision.status.success(), "add-decision failed");

        let index =
            run_impulse_with_impulse_dir(temp_dir.path(), &["index-memory", "--scope", "genome"]);
        assert!(index.status.success(), "index-memory failed");

        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-genome",
                "--query",
                "retrieval",
                "--mode",
                "keyword",
                "--json",
            ],
        );
        assert!(
            search.status.success(),
            "search-genome failed: {}",
            stderr_str(&search)
        );
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["mode"], "keyword");
        let results = body["results"].as_array().expect("results array");
        assert!(!results.is_empty(), "expected at least one genome result");
        assert_eq!(results[0]["source"], "genome");
    }

    #[test]
    fn test_search_semantic_falls_back_when_vector_disabled() {
        let (temp_dir, _session_id) =
            session_with_history_index("retrieval-semantic", "semantic fallback summary");

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
            "semantic search should still succeed"
        );
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["mode"], "semantic");
        assert_eq!(body["used_fallback"], true);
        assert_ne!(body["fallback_code"], Value::Null);
        assert!(
            body["fallback_reason"]
                .as_str()
                .unwrap_or_default()
                .contains("fallback"),
            "fallback reason should be included"
        );
    }

    #[test]
    fn test_retrieval_status_reports_health_fields() {
        let temp_dir = init_test_dir();

        let status = run_impulse_with_impulse_dir(temp_dir.path(), &["retrieval-status"]);
        assert!(
            status.status.success(),
            "retrieval-status failed: {}",
            stderr_str(&status)
        );
        let stdout = stdout_str(&status);
        assert!(stdout.contains("Retrieval DB:"), "db path should be shown");
        assert!(stdout.contains("Counts:"), "counts should be shown");
        assert!(stdout.contains("Vector:"), "vector health should be shown");
    }

    #[test]
    fn test_retrieval_status_check_json() {
        let temp_dir = init_test_dir();
        let status = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["retrieval-status", "--check", "--json"],
        );
        assert!(
            status.status.success(),
            "retrieval-status --check --json failed: {}",
            stderr_str(&status)
        );
        let body: Value = parse_json_stdout(&status);
        assert!(body.get("db_path").is_some());
        assert!(body.get("db_exists").is_some());
        assert!(body.get("python_available").is_some());
    }

    #[test]
    fn test_search_history_explain_output() {
        let (temp_dir, _session_id) =
            session_with_history_index("retrieval-explain", "explain path summary");
        let out = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "explain",
                "--backend",
                "auto",
                "--explain",
            ],
        );
        assert!(
            out.status.success(),
            "search-history --explain failed: {}",
            stderr_str(&out)
        );
        let stdout = stdout_str(&out);
        assert!(
            stdout.contains("Explain:"),
            "explain section should be printed"
        );
    }

    #[test]
    fn test_keyword_search_sets_fallback_metadata_on_db_query_failure() {
        let (temp_dir, _session_id) =
            session_with_history_index("keyword-fallback", "query failure fallback");

        let db_path = temp_dir.path().join("retrieval.db");
        let conn = Connection::open(&db_path).expect("open retrieval db");
        conn.execute("DROP TABLE IF EXISTS history_entries", [])
            .expect("drop history_entries");
        conn.execute(
            "CREATE TABLE history_entries (session_id TEXT PRIMARY KEY, session_name TEXT NOT NULL, ended_at TEXT NOT NULL)",
            [],
        )
        .expect("create malformed history_entries");

        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "fallback",
                "--mode",
                "keyword",
                "--json",
            ],
        );
        assert!(
            search.status.success(),
            "keyword search should fallback safely"
        );
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["mode"], "keyword");
        assert_eq!(body["used_fallback"], true);
        assert_eq!(body["fallback_code"], "retrieval_db_error");
    }

    #[test]
    fn test_index_memory_lock_file_fails_safely() {
        let temp_dir = init_test_dir();
        let lock_path = temp_dir.path().join("retrieval.lock");
        std::fs::write(&lock_path, "stale lock").expect("write lock file");

        let out =
            run_impulse_with_impulse_dir(temp_dir.path(), &["index-memory", "--scope", "all"]);
        assert!(
            !out.status.success(),
            "index-memory should fail on active lock"
        );
        let stderr = stderr_str(&out);
        assert!(
            stderr.contains("lock active"),
            "expected lock-active guidance, got: {}",
            stderr
        );
    }

    #[test]
    #[ignore = "flaky: embedding subprocess env propagation race — falls back to keyword under load"]
    fn test_search_history_rust_backend_reports_backend_used() {
        let (temp_dir, session_id) = start_session("backend-rust");
        end_session(temp_dir.path(), &session_id, "backend rust cosine");

        configure_vector_search(temp_dir.path());
        let _ = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "retrieval_similarity_threshold", "--value", "0.0"],
        );

        let _ = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "history"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );

        let search = run_impulse_with_env(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "backend",
                "--mode",
                "semantic",
                "--backend",
                "rust-cosine",
                "--json",
            ],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            search.status.success(),
            "rust backend semantic search should succeed"
        );
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["backend_used"], "rust-cosine");
        assert_eq!(body["used_fallback"], false);
    }

    #[test]
    fn test_search_history_sqlite_backend_fallback_without_extension() {
        let (temp_dir, session_id) = start_session("backend-sqlite");
        end_session(temp_dir.path(), &session_id, "backend sqlite unavailable");
        configure_vector_search(temp_dir.path());
        let _ = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "history"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );

        let search = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "backend",
                "--mode",
                "semantic",
                "--backend",
                "sqlite-vec",
                "--json",
            ],
        );
        assert!(search.status.success());
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["used_fallback"], true);
        assert_eq!(body["fallback_code"], "sqlite_vec_unavailable");
        assert_eq!(body["backend_used"], "keyword");
    }

    #[test]
    fn test_search_history_sqlite_backend_used_when_extension_available() {
        if std::env::var("IMPULSE_SQLITE_VEC_EXT").is_err() {
            eprintln!("Skipping sqlite-vec native path test (IMPULSE_SQLITE_VEC_EXT not set)");
            return;
        }

        let (temp_dir, session_id) = start_session("backend-sqlite-native");
        end_session(temp_dir.path(), &session_id, "backend sqlite native");
        configure_vector_search(temp_dir.path());
        let _ = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "retrieval_similarity_threshold", "--value", "0.0"],
        );
        let _ = run_impulse_with_env(
            temp_dir.path(),
            &["index-memory", "--scope", "history"],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );

        let search = run_impulse_with_env(
            temp_dir.path(),
            &[
                "search-history",
                "--query",
                "sqlite",
                "--mode",
                "semantic",
                "--backend",
                "sqlite-vec",
                "--json",
            ],
            &[("IMPULSE_EMBED_ALLOW_FAKE", "1")],
        );
        assert!(
            search.status.success(),
            "sqlite-vec semantic search should run: {}",
            stderr_str(&search)
        );
        let body: Value = parse_json_stdout(&search);
        assert_eq!(body["backend_used"], "sqlite-vec");
        assert_eq!(body["used_fallback"], false);
    }

    #[test]
    fn test_daemon_chat_review_mode_stages_artifact_without_apply() {
        let temp_dir = init_test_dir();
        seed_retrieval_history(
            temp_dir.path(),
            "daemon-review-seed",
            "review mode alpha memory",
        );

        let _daemon = start_daemon(temp_dir.path());
        let create = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "--daemon",
                "session-start",
                "-n",
                "daemon-review",
                "-p",
                "claude-code",
            ],
        );
        assert!(
            create.status.success(),
            "daemon session-start failed: {}",
            stderr_str(&create)
        );
        let session_id = parse_daemon_session_id(&stdout_str(&create)).expect("daemon session id");

        let chat = run_impulse_with_env(
            temp_dir.path(),
            &[
                "--daemon",
                "chat",
                "--session-id",
                &session_id,
                "--message",
                "alpha",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
            &[("IMPULSE_TEST_MODE", "1")],
        );
        assert!(
            chat.status.success(),
            "daemon chat failed: {}",
            stderr_str(&chat)
        );

        let body: Value = parse_json_stdout(&chat);
        assert_eq!(body["session_id"], session_id);
        assert_eq!(body["injection"]["applied"], false);
        let artifact_path = body["injection"]["artifact_path"]
            .as_str()
            .expect("artifact path in review mode");
        assert!(std::path::Path::new(artifact_path).exists());
    }

    #[test]
    fn test_daemon_chat_apply_mode_includes_injected_block() {
        let temp_dir = init_test_dir();
        seed_retrieval_history(
            temp_dir.path(),
            "daemon-apply-seed",
            "apply mode beta memory",
        );

        let _daemon = start_daemon(temp_dir.path());
        let create = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "--daemon",
                "session-start",
                "-n",
                "daemon-apply",
                "-p",
                "claude-code",
            ],
        );
        let session_id = parse_daemon_session_id(&stdout_str(&create)).expect("daemon session id");

        let chat = run_impulse_with_env(
            temp_dir.path(),
            &[
                "--daemon",
                "chat",
                "--session-id",
                &session_id,
                "--message",
                "beta",
                "--inject-mode",
                "apply",
                "--inject-explain",
            ],
            &[("IMPULSE_TEST_MODE", "1")],
        );
        assert!(chat.status.success(), "daemon chat apply should succeed");
        let body: Value = parse_json_stdout(&chat);
        assert_eq!(body["injection"]["applied"], true);
        assert!(
            body["injection"]["injected_block"]
                .as_str()
                .unwrap_or_default()
                .contains("Impulse Memory Context"),
            "apply mode should return injected context block"
        );
    }

    #[test]
    fn test_direct_injection_review_for_orchestrate_handoff_sync_context() {
        let temp_dir = init_test_dir();
        seed_retrieval_history(
            temp_dir.path(),
            "direct-review-seed",
            "direct review retrieval baseline",
        );

        let orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "review auth and memory drift",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
        );
        assert!(orchestrate.status.success(), "orchestrate review failed");

        let handoff = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "handoff",
                "--tool",
                "codex",
                "--task",
                "continue auth validation",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
        );
        assert!(handoff.status.success(), "handoff review failed");

        let sync = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "sync-context",
                "--inject-mode",
                "review",
                "--inject-explain",
            ],
        );
        assert!(sync.status.success(), "sync-context review failed");

        let injections_dir = temp_dir.path().join("context").join("injections");
        assert!(
            injections_dir.exists(),
            "injections directory should be created"
        );
        let entries = std::fs::read_dir(&injections_dir).unwrap().count();
        assert!(
            entries > 0,
            "expected staged injection artifacts/log entries"
        );
    }

    #[test]
    fn test_injection_off_mode_keeps_baseline_and_emits_no_artifact() {
        let temp_dir = init_test_dir();
        seed_retrieval_history(temp_dir.path(), "off-mode-seed", "off mode baseline");

        let orchestrate = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "baseline tool recommendation",
                "--inject-mode",
                "off",
            ],
        );
        assert!(orchestrate.status.success());
        assert!(
            stdout_str(&orchestrate).contains("Recommended tool"),
            "baseline orchestrate output should remain intact"
        );
        assert!(
            !temp_dir.path().join("context").join("injections").exists(),
            "off mode should not create injection artifacts"
        );
    }

    #[test]
    fn test_injection_explain_contains_fallback_metadata() {
        let temp_dir = init_test_dir();
        seed_retrieval_history(
            temp_dir.path(),
            "fallback-meta-seed",
            "semantic fallback metadata baseline",
        );

        let out = run_impulse_with_impulse_dir(
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
        assert!(
            out.status.success(),
            "orchestrate with injection explain should succeed"
        );
        let stdout = stdout_str(&out);
        assert!(
            stdout.contains("fallback_code=vector_backend_disabled"),
            "expected vector-disabled fallback metadata, got: {}",
            stdout
        );
    }

    #[test]
    fn test_retrieval_status_json_includes_injection_summary() {
        let temp_dir = init_test_dir();

        let _ = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &[
                "orchestrate",
                "--task",
                "status json injection check",
                "--inject-mode",
                "review",
            ],
        );

        let status = run_impulse_with_impulse_dir(temp_dir.path(), &["retrieval-status", "--json"]);
        assert!(status.status.success());
        let body: Value = parse_json_stdout(&status);
        assert!(
            body["injection"].is_object(),
            "status json should include injection block"
        );
        assert!(body["injection"]["config_mode"].is_string());
        assert!(body["injection"]["config_scope"].is_string());
    }

    // ========================================================================
    // Stewardship Integration Tests
    // ========================================================================

    #[test]
    fn test_steward_status_json() {
        let (_dir, impulse_dir) = init_steward_dir();

        let output = run_impulse_with_impulse_dir(&impulse_dir, &["steward", "status", "--json"]);
        assert!(
            output.status.success(),
            "steward status --json should succeed"
        );

        let body: Value = parse_json_stdout(&output);
        assert!(body["mode"].is_string(), "should have mode field");
        assert!(body["thresholds"].is_object(), "should have thresholds");
        assert!(
            body["pending_proposals"].is_number(),
            "should have pending_proposals count"
        );
        assert!(
            body["cross_project_patterns"].is_number(),
            "should have patterns count"
        );
    }

    #[test]
    fn test_steward_list_empty() {
        let (_dir, impulse_dir) = init_steward_dir();

        let output = run_impulse_with_impulse_dir(&impulse_dir, &["steward", "list", "--json"]);
        assert!(
            output.status.success(),
            "steward list --json should succeed"
        );

        let body: Value = parse_json_stdout(&output);
        assert!(body.is_array(), "list should return array");
        assert_eq!(
            body.as_array().unwrap().len(),
            0,
            "should start with no proposals"
        );
    }

    #[test]
    fn test_steward_memory_json() {
        let (_dir, impulse_dir) = init_steward_dir();

        let output = run_impulse_with_impulse_dir(&impulse_dir, &["steward", "memory", "--json"]);
        assert!(
            output.status.success(),
            "steward memory --json should succeed"
        );

        let body: Value = parse_json_stdout(&output);
        assert!(body["version"].is_string(), "should have version");
        assert!(body["patterns"].is_array(), "should have patterns array");
        assert!(body["learnings"].is_array(), "should have learnings array");
    }

    #[test]
    fn test_steward_analyze_with_fixture() {
        let (_dir, impulse_dir) = init_steward_dir();

        let fixture_path = impulse_rs_dir()
            .join("tests")
            .join("fixtures")
            .join("sample-session.jsonl");

        assert!(
            fixture_path.exists(),
            "Sample session fixture should exist at {:?}",
            fixture_path
        );

        let output = run_impulse_with_impulse_dir(
            &impulse_dir,
            &[
                "steward",
                "analyze",
                "--transcript",
                fixture_path.to_str().unwrap(),
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "steward analyze should succeed. stderr: {}",
            stderr_str(&output)
        );

        let body: Value = parse_json_stdout(&output);
        assert!(
            body["message_count"].is_number(),
            "should have message_count"
        );
        let msg_count = body["message_count"].as_u64().unwrap();
        assert!(msg_count > 0, "should have parsed messages from fixture");
        assert!(
            body["estimated_tokens"].is_number(),
            "should have estimated_tokens"
        );
        assert!(
            body["files_touched"].is_array(),
            "should have files_touched"
        );
    }

    #[test]
    fn test_steward_compact_with_session_id() {
        let (_dir, impulse_dir) = init_steward_dir();

        let output = run_impulse_with_impulse_dir(
            &impulse_dir,
            &["steward", "compact", "--session-id", "test-session-123"],
        );
        assert!(output.status.success(), "steward compact should succeed");

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("test-session-123"),
            "compact output should contain session id"
        );
    }

    #[test]
    fn test_steward_approve_nonexistent() {
        let (_dir, impulse_dir) = init_steward_dir();

        let output = run_impulse_with_impulse_dir(
            &impulse_dir,
            &["steward", "approve", "--id", "nonexistent-id"],
        );
        assert!(
            output.status.success(),
            "approve of nonexistent should not crash"
        );

        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("not found"),
            "should report proposal not found"
        );
    }

    #[test]
    fn test_stewardship_config_fields() {
        let (_dir, impulse_dir) = init_steward_dir();

        // Init to create config
        let _ = run_impulse_with_impulse_dir(&impulse_dir, &["init"]);

        // Check default stewardship mode
        let output = run_impulse_with_impulse_dir(&impulse_dir, &["config", "stewardship_mode"]);
        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("review"),
            "default stewardship_mode should be 'review'"
        );

        // Set stewardship mode to auto
        let output = run_impulse_with_impulse_dir(
            &impulse_dir,
            &["config", "stewardship_mode", "--value", "auto"],
        );
        assert!(
            output.status.success(),
            "setting stewardship_mode should succeed"
        );

        // Verify it was set
        let output = run_impulse_with_impulse_dir(&impulse_dir, &["config", "stewardship_mode"]);
        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("auto"),
            "stewardship_mode should be 'auto' after set"
        );
    }

    // =========================================================================
    // Impulse Agent Integration Tests
    // =========================================================================

    /// Test (a): Config round-trip for impulse_agent_provider via State.set_config() / get_config()
    #[test]
    fn test_impulse_agent_config_provider_round_trip() {
        let (_temp_dir, state) = new_state();

        // Default: provider is None
        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(val, None, "default provider should be None");

        // Set to anthropic
        let ok = state
            .set_config("impulse_agent_provider", "anthropic")
            .expect("set_config should not error");
        assert!(ok, "setting 'anthropic' should succeed");

        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(
            val,
            Some("anthropic".to_string()),
            "provider should round-trip as 'anthropic'"
        );

        // Set to openai
        let ok = state
            .set_config("impulse_agent_provider", "openai")
            .expect("set_config should not error");
        assert!(ok, "setting 'openai' should succeed");
        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(val, Some("openai".to_string()));

        // Set to minimax
        let ok = state
            .set_config("impulse_agent_provider", "minimax")
            .expect("set_config should not error");
        assert!(ok, "setting 'minimax' should succeed");
        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(val, Some("minimax".to_string()));

        // Clear with "none"
        let ok = state
            .set_config("impulse_agent_provider", "none")
            .expect("set_config should not error");
        assert!(ok, "clearing with 'none' should succeed");
        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(val, None, "provider should be None after clearing");
    }

    /// Test (b): Config validation — invalid provider returns false
    #[test]
    fn test_impulse_agent_config_rejects_invalid_provider() {
        let (_temp_dir, state) = new_state();

        let ok = state
            .set_config("impulse_agent_provider", "invalid_provider")
            .expect("set_config should not error");
        assert!(!ok, "setting an invalid provider name should return false");

        let ok = state
            .set_config("impulse_agent_provider", "bedrock")
            .expect("set_config should not error");
        assert!(!ok, "setting 'bedrock' should return false");

        let ok = state
            .set_config("impulse_agent_provider", "")
            .expect("set_config should not error");
        assert!(ok, "setting empty string should succeed (clears provider)");
        let val = state
            .get_config("impulse_agent_provider")
            .expect("get_config should not error");
        assert_eq!(val, None, "empty string should clear the provider to None");
    }

    /// Test (b continued): Config validation — invalid harness returns false
    #[test]
    fn test_impulse_agent_config_rejects_invalid_harness() {
        let (_temp_dir, state) = new_state();

        let ok = state
            .set_config("impulse_agent_harness", "cursor")
            .expect("set_config should not error");
        assert!(!ok, "setting an invalid harness name should return false");

        let ok = state
            .set_config("impulse_agent_harness", "claude-code")
            .expect("set_config should not error");
        assert!(ok, "setting 'claude-code' should succeed");
        let val = state
            .get_config("impulse_agent_harness")
            .expect("get_config should not error");
        assert_eq!(val, Some("claude-code".to_string()));
    }

    /// Test (c): API key masking — set an API key, verify get_config returns "***"
    #[test]
    fn test_impulse_agent_api_key_masking() {
        let (_temp_dir, state) = new_state();

        // Default: no key
        let val = state
            .get_config("impulse_agent_api_key")
            .expect("get_config should not error");
        assert_eq!(val, None, "default api_key should be None");

        // Set an API key
        let ok = state
            .set_config("impulse_agent_api_key", "test-api-key-not-real-12345")
            .expect("set_config should not error");
        assert!(ok, "setting API key should succeed");

        // get_config should return masked value, not the actual key
        let val = state
            .get_config("impulse_agent_api_key")
            .expect("get_config should not error");
        assert_eq!(
            val,
            Some("***".to_string()),
            "API key should be masked as '***'"
        );

        // The actual key should be stored in the underlying config
        let config = state
            .config_snapshot()
            .expect("config_snapshot should work");
        assert_eq!(
            config.impulse_agent_api_key,
            Some("test-api-key-not-real-12345".to_string()),
            "underlying config should store the actual key"
        );

        // Clear the key
        let ok = state
            .set_config("impulse_agent_api_key", "")
            .expect("set_config should not error");
        assert!(ok, "clearing API key should succeed");
        let val = state
            .get_config("impulse_agent_api_key")
            .expect("get_config should not error");
        assert_eq!(val, None, "cleared api_key should be None");
    }

    /// Test (d): Agent resolve from config — set provider + api_key + model, call resolve_from_config()
    #[test]
    fn test_impulse_agent_resolve_from_config_api_mode() {
        use crate::agent::{resolve_from_config, AgentMode, ImpulseProvider};

        // Resolve with provider + key + model
        let agent = resolve_from_config(
            Some("anthropic"),
            Some("test-api-key-for-resolve"),
            Some("claude-sonnet-4-6"),
            None,
            None,
        );
        assert!(agent.is_some(), "resolve should create an agent");
        let agent = agent.unwrap();
        assert!(agent.is_ready(), "API agent with key should be ready");
        match agent.config().mode {
            AgentMode::Api {
                provider,
                ref model,
            } => {
                assert_eq!(provider, ImpulseProvider::Anthropic);
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-6"));
            }
            _ => panic!("Expected API mode"),
        }

        // Resolve with openai + key, no model (should use default)
        let agent = resolve_from_config(
            Some("openai"),
            Some("test-openai-key-not-real"),
            None,
            None,
            None,
        );
        assert!(agent.is_some());
        let agent = agent.unwrap();
        assert!(agent.is_ready());
        let summary = agent.status_summary();
        assert!(
            summary.contains("openai"),
            "status should mention openai: {}",
            summary
        );
        assert!(
            summary.contains("gpt-4o"),
            "should use default model gpt-4o: {}",
            summary
        );

        // Resolve with minimax
        let agent = resolve_from_config(Some("minimax"), Some("mm-key"), None, None, None);
        assert!(agent.is_some());
        let summary = agent.unwrap().status_summary();
        assert!(summary.contains("minimax"));
    }

    /// Test (e): Agent resolve with harness — set harness to "claude_code", verify harness mode
    #[test]
    fn test_impulse_agent_resolve_from_config_harness_mode() {
        use crate::agent::{resolve_from_config, AgentMode, ImpulseHarness};

        // Resolve with claude-code harness
        let agent = resolve_from_config(None, None, None, Some("claude-code"), None);
        assert!(agent.is_some(), "resolve with harness should return Some");
        let agent = agent.unwrap();
        match agent.config().mode {
            AgentMode::Harness { harness } => {
                assert_eq!(harness, ImpulseHarness::ClaudeCode);
                assert_eq!(harness.command(), "claude");
            }
            _ => panic!("Expected Harness mode"),
        }

        // Resolve with opencode harness
        let agent = resolve_from_config(None, None, None, Some("opencode"), None);
        assert!(agent.is_some());
        let agent = agent.unwrap();
        match agent.config().mode {
            AgentMode::Harness { harness } => {
                assert_eq!(harness, ImpulseHarness::OpenCode);
                assert_eq!(harness.command(), "opencode");
            }
            _ => panic!("Expected Harness mode"),
        }

        // Harness takes priority over provider when both specified
        let agent = resolve_from_config(
            Some("anthropic"),
            Some("key"),
            None,
            Some("claude-code"),
            None,
        );
        assert!(agent.is_some());
        assert!(
            matches!(agent.unwrap().config().mode, AgentMode::Harness { .. }),
            "harness should take priority over API mode"
        );
    }

    /// Test (f): Agent disabled by default — with no config set, resolve returns None
    #[test]
    fn test_impulse_agent_disabled_by_default() {
        use crate::agent::{resolve_from_config, AgentMode, ImpulseAgentConfig};

        // No parameters: should return None (disabled)
        let agent = resolve_from_config(None, None, None, None, None);
        assert!(
            agent.is_none(),
            "with no config, resolve_from_config should return None (disabled)"
        );

        // Invalid provider: should also return None
        let agent = resolve_from_config(Some("invalid"), None, None, None, None);
        assert!(agent.is_none(), "invalid provider should result in None");

        // Invalid harness: should also return None
        let agent = resolve_from_config(None, None, None, Some("invalid"), None);
        assert!(agent.is_none(), "invalid harness should result in None");

        // Verify ImpulseAgentConfig::default() is disabled
        let default_config = ImpulseAgentConfig::default();
        assert!(!default_config.is_enabled());
        assert_eq!(default_config.mode, AgentMode::Disabled);
    }

    /// Test (g): Coordinator end-to-end — mixed insights produce correct recommendations
    #[test]
    fn test_impulse_agent_coordinator_end_to_end() {
        use crate::agent::coordinator::{self, RecommendationType};
        use crate::context_lifecycle::types::{AgentKind, InsightType};

        // Build a realistic multi-pane scenario:
        // - Pane 1 (ClaudeCode): modified src/lib.rs and src/main.rs
        // - Pane 2 (OpenCode): modified src/lib.rs (conflict!) and got an error
        // - Pane 3 (Codex): modified tests/integration.rs and completed a task
        let insights = vec![
            file_mod(1, AgentKind::ClaudeCode, "src/lib.rs"),
            file_mod(1, AgentKind::ClaudeCode, "src/main.rs"),
            file_mod(2, AgentKind::OpenCode, "src/lib.rs"),
            insight(
                2,
                AgentKind::OpenCode,
                InsightType::ErrorEncountered,
                "error[E0433]: failed to resolve: use of undeclared crate",
            ),
            file_mod(3, AgentKind::Codex, "tests/integration.rs"),
            insight(
                3,
                AgentKind::Codex,
                InsightType::TaskCompleted,
                "all tests passing",
            ),
        ];

        let recs = coordinator::run_local_coordination(&insights);

        // Should detect the file conflict on src/lib.rs
        let file_conflicts: Vec<_> = recs
            .iter()
            .filter(|r| r.recommendation_type == RecommendationType::FileConflict)
            .collect();
        assert_eq!(
            file_conflicts.len(),
            1,
            "should detect exactly 1 file conflict (src/lib.rs)"
        );
        assert!(
            file_conflicts[0].description.contains("src/lib.rs"),
            "conflict should mention src/lib.rs"
        );
        assert!(
            file_conflicts[0]
                .panes_involved
                .contains(&"pane-1".to_string()),
            "conflict should involve pane-1"
        );
        assert!(
            file_conflicts[0]
                .panes_involved
                .contains(&"pane-2".to_string()),
            "conflict should involve pane-2"
        );

        // Should detect cross-pane error (pane 2 error correlated with pane 1 modifications)
        let error_assists: Vec<_> = recs
            .iter()
            .filter(|r| r.recommendation_type == RecommendationType::ErrorAssist)
            .collect();
        assert!(
            !error_assists.is_empty(),
            "should detect at least one cross-pane error assist"
        );
        assert!(
            error_assists[0].description.contains("pane-2"),
            "error assist should reference pane-2 where the error occurred"
        );

        // Total recommendations should be file_conflicts + error_assists
        assert_eq!(
            recs.len(),
            file_conflicts.len() + error_assists.len(),
            "total recs should be sum of conflict + error recs"
        );
    }

    /// Test (g continued): Coordinator with no conflicts produces empty recommendations
    #[test]
    fn test_impulse_agent_coordinator_no_conflicts() {
        use crate::agent::coordinator;
        use crate::context_lifecycle::types::{AgentKind, InsightType};

        // Each pane modifies different files — no overlap
        let insights = vec![
            file_mod(1, AgentKind::ClaudeCode, "src/auth.rs"),
            file_mod(2, AgentKind::OpenCode, "src/database.rs"),
            insight(
                3,
                AgentKind::Codex,
                InsightType::TaskCompleted,
                "refactoring complete",
            ),
        ];

        let recs = coordinator::run_local_coordination(&insights);
        assert!(
            recs.is_empty(),
            "non-overlapping work should produce no recommendations"
        );
    }

    /// Test (g continued): Coordinator with multiple file conflicts
    #[test]
    fn test_impulse_agent_coordinator_multiple_file_conflicts() {
        use crate::agent::coordinator::{self, RecommendationType};
        use crate::context_lifecycle::types::AgentKind;

        // Three panes all touching the same two files
        let insights = vec![
            file_mod(1, AgentKind::ClaudeCode, "Cargo.toml"),
            file_mod(2, AgentKind::OpenCode, "Cargo.toml"),
            file_mod(3, AgentKind::Codex, "Cargo.toml"),
            file_mod(1, AgentKind::ClaudeCode, "src/config.rs"),
            file_mod(2, AgentKind::OpenCode, "src/config.rs"),
        ];

        let recs = coordinator::run_local_coordination(&insights);
        let file_conflicts: Vec<_> = recs
            .iter()
            .filter(|r| r.recommendation_type == RecommendationType::FileConflict)
            .collect();
        assert_eq!(
            file_conflicts.len(),
            2,
            "should detect 2 file conflicts (Cargo.toml and src/config.rs)"
        );

        // Verify that the Cargo.toml conflict involves all 3 panes
        let cargo_conflict = file_conflicts
            .iter()
            .find(|r| r.description.contains("Cargo.toml"))
            .expect("should find Cargo.toml conflict");
        assert_eq!(
            cargo_conflict.panes_involved.len(),
            3,
            "Cargo.toml conflict should involve all 3 panes"
        );
    }

    /// Test (g continued): Coordinator ImpulseAgent.coordinate_local() accumulates recommendations
    #[test]
    fn test_impulse_agent_coordinate_local_accumulates() {
        use crate::agent::{ImpulseAgent, ImpulseAgentConfig, ImpulseProvider};
        use crate::context_lifecycle::types::AgentKind;

        let config =
            ImpulseAgentConfig::api(ImpulseProvider::Anthropic).with_api_key("test-key-accumulate");
        let mut agent = ImpulseAgent::new(config).expect("agent creation should work");
        assert!(
            agent.recommendations().is_empty(),
            "should start with no recs"
        );

        // First batch: one conflict
        let batch1 = vec![
            file_mod(1, AgentKind::ClaudeCode, "src/a.rs"),
            file_mod(2, AgentKind::OpenCode, "src/a.rs"),
        ];
        let recs1 = agent.coordinate_local(&batch1);
        assert_eq!(recs1.len(), 1);
        assert_eq!(agent.recommendations().len(), 1, "should accumulate 1 rec");

        // Second batch: another conflict
        let batch2 = vec![
            file_mod(3, AgentKind::Codex, "src/b.rs"),
            file_mod(4, AgentKind::GenericShell, "src/b.rs"),
        ];
        let recs2 = agent.coordinate_local(&batch2);
        assert_eq!(recs2.len(), 1);
        assert_eq!(
            agent.recommendations().len(),
            2,
            "should accumulate 2 recs total"
        );
    }

    /// Test (h): Prompt builder formatting — build_coordination_prompt() with multi-pane summaries
    #[test]
    fn test_impulse_agent_coordination_prompt_format() {
        use crate::agent::prompts::build_coordination_prompt;

        let summaries = vec![
            (
                "claude-main".to_string(),
                vec![
                    "[file_modified] src/auth.rs".to_string(),
                    "[file_modified] src/routes.rs".to_string(),
                    "[task_completed] auth module refactored".to_string(),
                ],
            ),
            (
                "opencode-tests".to_string(),
                vec![
                    "[file_modified] tests/auth_test.rs".to_string(),
                    "[error_encountered] test auth_login failed".to_string(),
                ],
            ),
            (
                "codex-docs".to_string(),
                vec!["[file_modified] README.md".to_string()],
            ),
        ];

        let prompt = build_coordination_prompt(&summaries);

        // Verify structure: should have the header
        assert!(
            prompt.starts_with("Current activity across all agent panes:"),
            "prompt should start with the expected header"
        );

        // Verify each pane section is present
        assert!(
            prompt.contains("## Pane: claude-main"),
            "should have claude-main section"
        );
        assert!(
            prompt.contains("## Pane: opencode-tests"),
            "should have opencode-tests section"
        );
        assert!(
            prompt.contains("## Pane: codex-docs"),
            "should have codex-docs section"
        );

        // Verify insights are listed as bullet points
        assert!(prompt.contains("- [file_modified] src/auth.rs"));
        assert!(prompt.contains("- [file_modified] src/routes.rs"));
        assert!(prompt.contains("- [task_completed] auth module refactored"));
        assert!(prompt.contains("- [file_modified] tests/auth_test.rs"));
        assert!(prompt.contains("- [error_encountered] test auth_login failed"));
        assert!(prompt.contains("- [file_modified] README.md"));
    }

    /// Test (h continued): build_review_prompt() format verification
    #[test]
    fn test_impulse_agent_review_prompt_format() {
        use crate::agent::prompts::build_review_prompt;

        let insights = vec![
            "Modified src/main.rs: added error handling".to_string(),
            "Modified src/lib.rs: new public API".to_string(),
            "Created tests/new_test.rs".to_string(),
        ];
        let prompt = build_review_prompt("claude-pane-1", &insights);

        assert!(
            prompt.contains("claude-pane-1"),
            "should reference the pane name"
        );
        assert!(prompt.contains("- Modified src/main.rs: added error handling"));
        assert!(prompt.contains("- Modified src/lib.rs: new public API"));
        assert!(prompt.contains("- Created tests/new_test.rs"));
    }

    /// Test (h continued): build_error_prompt() format verification
    #[test]
    fn test_impulse_agent_error_prompt_format() {
        use crate::agent::prompts::build_error_prompt;

        let error_text = "error[E0308]: mismatched types\n  --> src/lib.rs:42:5\n  |\n42 |     foo()\n  |     ^^^^^ expected `i32`, found `String`";
        let prompt = build_error_prompt("opencode-2", error_text);

        assert!(prompt.contains("opencode-2"), "should reference pane name");
        assert!(prompt.contains("E0308"), "should include the error code");
        assert!(prompt.contains("```"), "should wrap error in code block");
    }

    /// Test (h continued): build_summary_prompt() truncation behavior
    #[test]
    fn test_impulse_agent_summary_prompt_truncation() {
        use crate::agent::prompts::build_summary_prompt;

        // Short output: should not be truncated
        let short = "cargo test\n  Running 10 tests\n  10 passed, 0 failed";
        let prompt = build_summary_prompt("test-pane", short);
        assert!(
            prompt.contains(short),
            "short output should not be truncated"
        );

        // Long output: should be truncated to last 4000 chars
        let long = "x".repeat(6000);
        let prompt = build_summary_prompt("test-pane", &long);
        // The prompt includes framing text + last 4000 chars of the output
        assert!(
            prompt.len() < 4200,
            "should be truncated to reasonable size"
        );
        assert!(
            prompt.contains("test-pane"),
            "should still reference pane name"
        );
    }

    /// Test: aggregate_pane_summaries groups correctly across panes
    #[test]
    fn test_impulse_agent_aggregate_pane_summaries() {
        use crate::agent::coordinator::aggregate_pane_summaries;
        use crate::context_lifecycle::types::{AgentKind, InsightType};

        let insights = vec![
            file_mod(1, AgentKind::ClaudeCode, "src/main.rs"),
            insight(
                1,
                AgentKind::ClaudeCode,
                InsightType::DecisionMade,
                "use async runtime",
            ),
            insight(
                2,
                AgentKind::OpenCode,
                InsightType::ErrorEncountered,
                "build failed",
            ),
            insight(
                3,
                AgentKind::Codex,
                InsightType::TaskCompleted,
                "refactor done",
            ),
        ];

        let summaries = aggregate_pane_summaries(&insights);
        assert_eq!(summaries.len(), 3, "should have 3 panes");

        // BTreeMap ensures sorted order by pane_id
        assert_eq!(summaries[0].0, "pane-1");
        assert_eq!(summaries[0].1.len(), 2, "pane-1 should have 2 insights");
        assert!(summaries[0].1[0].contains("[file_modified] src/main.rs"));
        assert!(summaries[0].1[1].contains("[decision_made] use async runtime"));

        assert_eq!(summaries[1].0, "pane-2");
        assert_eq!(summaries[1].1.len(), 1);
        assert!(summaries[1].1[0].contains("[error_encountered] build failed"));

        assert_eq!(summaries[2].0, "pane-3");
        assert_eq!(summaries[2].1.len(), 1);
        assert!(summaries[2].1[0].contains("[task_completed] refactor done"));
    }

    /// Test: Full config round-trip for all impulse_agent config fields
    #[test]
    fn test_impulse_agent_config_all_fields_round_trip() {
        let (_temp_dir, state) = new_state();

        // Set all impulse agent config fields
        assert!(state
            .set_config("impulse_agent_provider", "anthropic")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_api_key", "test-api-key-all-fields")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_model", "claude-sonnet-4-6")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_harness", "opencode")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_auto_review", "true")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_auto_coordinate", "true")
            .unwrap());
        assert!(state
            .set_config("impulse_agent_escalate_model", "claude-opus-4-6")
            .unwrap());

        // Verify all fields
        assert_eq!(
            state.get_config("impulse_agent_provider").unwrap(),
            Some("anthropic".to_string())
        );
        assert_eq!(
            state.get_config("impulse_agent_api_key").unwrap(),
            Some("***".to_string()),
            "api key should be masked"
        );
        assert_eq!(
            state.get_config("impulse_agent_model").unwrap(),
            Some("claude-sonnet-4-6".to_string())
        );
        assert_eq!(
            state.get_config("impulse_agent_harness").unwrap(),
            Some("opencode".to_string())
        );
        assert_eq!(
            state.get_config("impulse_agent_auto_review").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(
            state.get_config("impulse_agent_auto_coordinate").unwrap(),
            Some("true".to_string())
        );
        assert_eq!(
            state.get_config("impulse_agent_escalate_model").unwrap(),
            Some("claude-opus-4-6".to_string())
        );

        // Verify config snapshot has the actual values
        let config = state.config_snapshot().unwrap();
        assert_eq!(config.impulse_agent_provider, Some("anthropic".to_string()));
        assert_eq!(
            config.impulse_agent_api_key,
            Some("test-api-key-all-fields".to_string())
        );
        assert_eq!(
            config.impulse_agent_model,
            Some("claude-sonnet-4-6".to_string())
        );
        assert_eq!(config.impulse_agent_harness, Some("opencode".to_string()));
        assert!(config.impulse_agent_auto_review);
        assert!(config.impulse_agent_auto_coordinate);
        assert_eq!(
            config.impulse_agent_escalate_model.as_deref(),
            Some("claude-opus-4-6")
        );
    }

    /// Test: ImpulseAgent.status_summary() correctness for each mode
    #[test]
    fn test_impulse_agent_status_summary_all_modes() {
        use crate::agent::{ImpulseAgent, ImpulseAgentConfig, ImpulseHarness, ImpulseProvider};

        // Disabled
        let config = ImpulseAgentConfig::default();
        let agent = ImpulseAgent::new(config).unwrap();
        assert_eq!(agent.status_summary(), "Disabled");

        // API mode with key
        let config = ImpulseAgentConfig::api(ImpulseProvider::Anthropic)
            .with_api_key("test-key")
            .with_model("claude-opus-4-5-20250514");
        let agent = ImpulseAgent::new(config).unwrap();
        let summary = agent.status_summary();
        assert!(summary.contains("API"));
        assert!(summary.contains("anthropic"));
        assert!(summary.contains("claude-opus-4-5-20250514"));
        assert!(summary.contains("ready"));

        // Harness mode
        let config = ImpulseAgentConfig::harness(ImpulseHarness::ClaudeCode);
        let agent = ImpulseAgent::new(config).unwrap();
        let summary = agent.status_summary();
        assert!(summary.contains("Harness"));
        assert!(summary.contains("claude-code"));
    }

    /// Test: CLI config set/get for impulse_agent fields via subprocess
    #[test]
    fn test_impulse_agent_config_cli_round_trip() {
        let temp_dir = init_test_dir();

        // Set provider via CLI
        let output = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "impulse_agent_provider", "--value", "anthropic"],
        );
        assert!(
            output.status.success(),
            "setting impulse_agent_provider via CLI should succeed"
        );

        // Get provider via CLI
        let output =
            run_impulse_with_impulse_dir(temp_dir.path(), &["config", "impulse_agent_provider"]);
        assert!(output.status.success());
        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("anthropic"),
            "CLI get should return 'anthropic', got: {}",
            stdout
        );

        // Set harness via CLI
        let output = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "impulse_agent_harness", "--value", "claude-code"],
        );
        assert!(output.status.success());
        let output =
            run_impulse_with_impulse_dir(temp_dir.path(), &["config", "impulse_agent_harness"]);
        let stdout = stdout_str(&output);
        assert!(
            stdout.contains("claude-code"),
            "CLI get should return 'claude-code', got: {}",
            stdout
        );

        // Set invalid provider via CLI — should fail
        let output = run_impulse_with_impulse_dir(
            temp_dir.path(),
            &["config", "impulse_agent_provider", "--value", "invalid_llm"],
        );
        // The CLI may still return success with an error message, or may return non-zero
        let stderr = stderr_str(&output);
        let stdout = stdout_str(&output);
        let failed = !output.status.success()
            || stderr.contains("invalid")
            || stderr.contains("Unknown")
            || stdout.contains("invalid")
            || stdout.contains("Unknown");
        assert!(
            failed,
            "setting invalid provider via CLI should indicate failure"
        );
    }
}
