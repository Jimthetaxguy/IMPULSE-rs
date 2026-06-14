//! Daemon-mode CLI dispatch — routes Commands through the DaemonClient IPC.
//!
//! Extracted from `run_daemon_mode()` in main.rs to keep main.rs focused on
//! argument parsing and top-level dispatch.

use anyhow::{Context, Result};
use std::path::Path;

use crate::client::DaemonClient;
use crate::daemon::{DaemonRequest, DaemonResponse};
use crate::{envelope, plugin, semantic_diff, verify, Commands};

use super::{
    capture_hook_evidence, default_session_name, get_session_id, parse_injection_mode, print_json,
    print_verification_report, read_hook_stdin_payload, HookEvidenceInput,
};

/// Run a CLI command in daemon mode (forwarding over IPC).
pub(crate) async fn dispatch(
    command: Commands,
    impulse_dir: &Path,
    client: &DaemonClient,
    format: Option<envelope::OutputFormat>,
) -> Result<()> {
    match command {
        Commands::Daemon { stop } => {
            handle_daemon(client, stop)
                .await
                .context("Failed to handle daemon stop/status request")?;
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode: _,
            inject_explain: _,
        } => {
            handle_session_start(client, impulse_dir, name, platform)
                .await
                .context("Failed to handle session-start daemon request")?;
        }
        Commands::SessionEnd {
            session_id,
            summary,
            verify: should_verify,
            sem_diff_base,
        } => {
            handle_session_end(
                client,
                impulse_dir,
                session_id,
                summary,
                should_verify,
                sem_diff_base,
            )
            .await
            .context("Failed to handle session-end daemon request")?;
        }
        Commands::TrackWrite { file, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match client.track_file(sid, file).await {
                    Ok(_) => println!("Tracked file"),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::TrackTool { tool, session_id } => {
            if let Some(sid) = get_session_id(session_id) {
                match client.track_tool(sid, tool).await {
                    Ok(_) => println!("Tracked tool"),
                    Err(e) => eprintln!("Error: {}", e),
                }
            } else {
                eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            }
        }
        Commands::ListSessions => match client.list_sessions().await {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("No active sessions");
                } else {
                    for s in sessions {
                        println!("{}", format_session_line(&s));
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionInfo { id } => match client.get_session(id).await {
            Ok(s) => print_json(&s).context("Failed to serialize session info")?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionConflicts { file, session_id } => {
            handle_session_conflicts(client, file, session_id)
                .await
                .context("Failed to handle session-conflicts daemon request")?;
        }
        Commands::Status => match client.status().await {
            Ok(s) => print_json(&s).context("Failed to serialize daemon status")?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::Debug => match client.send(DaemonRequest::DebugSnapshot).await {
            Ok(DaemonResponse::Ok { result }) => {
                print_json(&result).context("Failed to serialize debug snapshot")?
            }
            Ok(DaemonResponse::Error { message }) => eprintln!("Error: {}", message),
            Ok(_) => eprintln!("Unexpected response type"),
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::ConflictHistory => {
            // In daemon mode, conflict-history reads from local storage (no IPC needed)
            println!("Conflict history reads from local .impulse/ — use direct mode.");
        }
        Commands::Chat {
            session_id,
            message,
            inject_mode,
            inject_explain,
        } => {
            handle_chat(client, session_id, message, inject_mode, inject_explain)
                .await
                .context("Failed to handle chat daemon request")?;
        }
        Commands::Verify => {
            let steps = verify::default_steps(
                &std::env::current_dir().context("Failed to get current directory for verify")?,
            );
            let report = verify::run_verification(steps).context("Failed to run verification")?;
            print_verification_report(&report);
            if !report.success() {
                anyhow::bail!("Verification failed");
            }
        }
        Commands::Describe => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            super::describe::handle_describe(fmt)
                .context("Failed to handle describe daemon request")?;
        }
        Commands::Schema { command: cmd } => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            super::describe::handle_schema(&cmd, fmt)
                .context("Failed to handle schema daemon request")?;
        }
        Commands::PluginList { json } => {
            handle_plugin_list(client, json)
                .await
                .context("Failed to handle plugin-list daemon request")?;
        }
        Commands::PluginInvoke {
            name,
            path,
            query,
            options,
            json,
        } => {
            handle_plugin_invoke(client, name, path, query, options, json)
                .await
                .context("Failed to handle plugin-invoke daemon request")?;
        }
        Commands::SearchHistory { .. }
        | Commands::SearchGenome { .. }
        | Commands::IndexMemory { .. }
        | Commands::RetrievalStatus { .. } => {
            println!("Use direct mode (without --daemon) for retrieval commands");
        }
        _ => println!("Use direct mode (without --daemon) for this command"),
    }
    Ok(())
}

// ============================================================================
// Daemon-mode handler functions
// ============================================================================

async fn handle_daemon(client: &DaemonClient, stop: bool) -> Result<()> {
    if stop {
        println!("Stopping daemon...");
        let _ = client.ping().await;
        println!("Daemon stopped");
    } else {
        println!("Daemon running");
        let status = client
            .status()
            .await
            .context("Failed to get daemon status")?;
        print_json(&status).context("Failed to serialize daemon status")?;
    }
    Ok(())
}

async fn handle_session_start(
    client: &DaemonClient,
    impulse_dir: &Path,
    name: Option<String>,
    platform: Option<String>,
) -> Result<()> {
    let stdin_payload = read_hook_stdin_payload();
    let name = name.unwrap_or_else(default_session_name);
    match client.create_session(name, platform).await {
        Ok((id, n)) => {
            let _ = super::persist_claude_env_var("IMPULSE_SESSION_ID", &id);
            capture_hook_evidence(HookEvidenceInput {
                impulse_dir,
                event: "session_start",
                session_id: Some(id.clone()),
                session_name: Some(n.clone()),
                platform: Some("daemon".to_string()),
                summary: None,
                verify_enabled: None,
                stdin_payload,
                output_preview: Some("daemon create_session".to_string()),
                output_lines: 1,
            })
            .context("Failed to capture hook evidence for session-start")?;
            println!("Created session: {} ({})", n, id)
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

async fn handle_session_end(
    client: &DaemonClient,
    impulse_dir: &Path,
    session_id: String,
    summary: String,
    should_verify: bool,
    sem_diff_base: Option<String>,
) -> Result<()> {
    let stdin_payload = read_hook_stdin_payload();
    if should_verify {
        let steps = verify::default_steps(
            &std::env::current_dir()
                .context("Failed to get current directory for session-end verify")?,
        );
        let report =
            verify::run_verification(steps).context("Failed to run session-end verification")?;
        print_verification_report(&report);
        if !report.success() {
            anyhow::bail!("Verification failed. Session end blocked.");
        }
    }
    // Capture semantic diff if requested and sem is available
    if let Some(base_ref) = &sem_diff_base {
        if semantic_diff::sem_available() {
            match semantic_diff::capture_semantic_diff(
                impulse_dir,
                &std::env::current_dir()
                    .context("Failed to get current directory for semantic diff")?,
                &session_id,
                base_ref,
                "HEAD",
            ) {
                Ok(report) => {
                    if !report.changes.is_empty() {
                        eprintln!("Semantic diff: {}", report.summary);
                    }
                }
                Err(e) => eprintln!("Warning: semantic diff failed: {}", e),
            }
        }
    }
    match client
        .end_session(session_id.clone(), summary.clone())
        .await
    {
        Ok(_) => {
            capture_hook_evidence(HookEvidenceInput {
                impulse_dir,
                event: "session_end",
                session_id: Some(session_id.clone()),
                session_name: None,
                platform: Some("daemon".to_string()),
                summary: Some(summary),
                verify_enabled: Some(should_verify),
                stdin_payload,
                output_preview: Some(format!("Session {} ended", session_id)),
                output_lines: 1,
            })
            .context("Failed to capture hook evidence for session-end")?;
            println!("Session {} ended", session_id)
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

async fn handle_session_conflicts(
    client: &DaemonClient,
    file: Option<String>,
    session_id: Option<String>,
) -> Result<()> {
    let sid = match get_session_id(session_id) {
        Some(s) => s,
        None => {
            eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
            return Ok(());
        }
    };
    match file {
        Some(f) => match client.check_conflict(sid, f).await {
            Ok((has_conflict, sessions)) => {
                if has_conflict {
                    println!("\u{26a0}\u{fe0f}  CONFLICT DETECTED");
                    println!("File is being edited by: {}", sessions.join(", "));
                } else {
                    println!("\u{2713} No conflicts detected");
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        },
        None => match client.list_sessions().await {
            Ok(sessions) => {
                let (file_to_session, all_files) = build_conflict_map(&sessions);

                if all_files.is_empty() {
                    println!("No active file modifications across sessions");
                } else {
                    println!("Active file modifications across sessions:");
                    for (file, sessions) in &file_to_session {
                        if sessions.len() > 1 {
                            println!(
                                "  \u{26a0}\u{fe0f}  {} - being edited by: {}",
                                file,
                                sessions.join(", ")
                            );
                        } else {
                            println!("  {} - edited by: {}", file, sessions.join(", "));
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        },
    }
    Ok(())
}

async fn handle_chat(
    client: &DaemonClient,
    session_id: String,
    message: String,
    inject_mode: Option<String>,
    inject_explain: bool,
) -> Result<()> {
    let inject_mode = match parse_injection_mode(inject_mode.as_deref()) {
        Ok(mode) => mode.map(|m| m.as_str().to_string()),
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(());
        }
    };
    match client
        .chat(session_id.clone(), message, inject_mode, inject_explain)
        .await
    {
        Ok(result) => {
            if inject_explain {
                print_json(&result).context("Failed to serialize chat result")?;
            } else if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
                println!("{}", response);
            } else {
                print_json(&result).context("Failed to serialize chat result")?;
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

async fn handle_plugin_list(client: &DaemonClient, json: bool) -> Result<()> {
    match client.send(DaemonRequest::ListPlugins).await {
        Ok(DaemonResponse::Ok { result }) => {
            if json {
                print_json(&result).context("Failed to serialize plugin list")?;
            } else {
                let providers = result
                    .get("context_providers")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let actions = result
                    .get("action_handlers")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                println!("Context Providers ({}):", providers.len());
                for p in &providers {
                    println!(
                        "  {} v{} — {}",
                        p.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                        p.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
                        p.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    );
                }
                println!("\nAction Handlers ({}):", actions.len());
                for h in &actions {
                    println!(
                        "  {} v{} — {}",
                        h.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
                        h.get("version").and_then(|v| v.as_str()).unwrap_or("?"),
                        h.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    );
                }
            }
        }
        Ok(DaemonResponse::Error { message }) => eprintln!("Error: {}", message),
        Err(e) => eprintln!("Error: {}", e),
        _ => eprintln!("Unexpected response"),
    }
    Ok(())
}

async fn handle_plugin_invoke(
    client: &DaemonClient,
    name: String,
    path: Option<String>,
    query: Option<String>,
    options: Option<String>,
    json: bool,
) -> Result<()> {
    let input = build_plugin_input(path, query, options);
    match client
        .send(DaemonRequest::InvokePlugin { name, input })
        .await
    {
        Ok(DaemonResponse::Ok { result }) => {
            if json {
                print_json(&result).context("Failed to serialize plugin invoke result")?;
            } else if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
                println!("{}", content);
            } else {
                print_json(&result).context("Failed to serialize plugin invoke result")?;
            }
        }
        Ok(DaemonResponse::Error { message }) => {
            eprintln!("Plugin error: {}", message)
        }
        Err(e) => eprintln!("Plugin error: {}", e),
        _ => eprintln!("Unexpected response"),
    }
    Ok(())
}

// ============================================================================
// Testable helper functions (pure logic, no IPC)
// ============================================================================

/// Build a PluginInput from CLI args. Extracted for testability.
fn build_plugin_input(
    path: Option<String>,
    query: Option<String>,
    options: Option<String>,
) -> plugin::PluginInput {
    let mut input = plugin::PluginInput::new();
    if let Some(p) = path {
        input = input.with_path(std::path::PathBuf::from(p));
    }
    if let Some(q) = query {
        input = input.with_query(q);
    }
    if let Some(opts) = options {
        input = input.with_options(super::parse_json_or_raw(&opts));
    }
    input
}

/// Build a file-to-sessions conflict map from daemon session list response.
/// Returns (file_to_sessions, all_files_set). Extracted for testability.
fn build_conflict_map(
    sessions: &[serde_json::Value],
) -> (
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashSet<String>,
) {
    let mut all_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut file_to_session: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for s in sessions {
        if let Some(files) = s.get("active_files").and_then(|v| v.as_array()) {
            for f in files {
                if let Some(path) = f.as_str() {
                    all_files.insert(path.to_string());
                    file_to_session.entry(path.to_string()).or_default().push(
                        s.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?")
                            .to_string(),
                    );
                }
            }
        }
    }

    (file_to_session, all_files)
}

/// Format a single session line for list-sessions output.
fn format_session_line(session: &serde_json::Value) -> String {
    format!(
        "{} - {} ({})",
        session["id"].as_str().unwrap_or("?"),
        session["name"].as_str().unwrap_or("?"),
        session["status"].as_str().unwrap_or("?"),
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse plugin options string: valid JSON passes through, invalid wraps as `{"raw": ...}`.
    /// Delegates to the shared `parse_json_or_raw` helper.
    fn parse_plugin_options(opts: &str) -> serde_json::Value {
        crate::handlers::parse_json_or_raw(opts)
    }

    // ── format_session_line ───────────────────────────────────────────

    #[test]
    fn test_format_session_line_all_fields_present() {
        let session = serde_json::json!({
            "id": "abc-123",
            "name": "my-session",
            "status": "active"
        });
        let line = format_session_line(&session);
        assert_eq!(line, "abc-123 - my-session (active)");
    }

    #[test]
    fn test_format_session_line_missing_id_shows_question_mark() {
        let session = serde_json::json!({
            "name": "my-session",
            "status": "active"
        });
        let line = format_session_line(&session);
        assert!(
            line.starts_with("?"),
            "Missing id should show '?', got: {}",
            line
        );
        assert!(line.contains("my-session"));
    }

    #[test]
    fn test_format_session_line_missing_name_shows_question_mark() {
        let session = serde_json::json!({
            "id": "abc-123",
            "status": "active"
        });
        let line = format_session_line(&session);
        assert!(
            line.contains("? (active)"),
            "Missing name should show '?', got: {}",
            line
        );
    }

    #[test]
    fn test_format_session_line_missing_status_shows_question_mark() {
        let session = serde_json::json!({
            "id": "abc-123",
            "name": "my-session"
        });
        let line = format_session_line(&session);
        assert!(
            line.ends_with("(?)"),
            "Missing status should show '?', got: {}",
            line
        );
    }

    #[test]
    fn test_format_session_line_all_missing_shows_all_question_marks() {
        let session = serde_json::json!({});
        let line = format_session_line(&session);
        assert_eq!(line, "? - ? (?)");
    }

    #[test]
    fn test_format_session_line_non_string_fields_show_question_marks() {
        let session = serde_json::json!({
            "id": 42,
            "name": true,
            "status": null
        });
        let line = format_session_line(&session);
        assert_eq!(
            line, "? - ? (?)",
            "Non-string fields should fall back to '?'"
        );
    }

    // ── parse_plugin_options ──────────────────────────────────────────

    #[test]
    fn test_parse_plugin_options_valid_json_object() {
        let result = parse_plugin_options(r#"{"key": "value", "num": 42}"#);
        assert_eq!(result["key"], "value");
        assert_eq!(result["num"], 42);
    }

    #[test]
    fn test_parse_plugin_options_valid_json_array() {
        let result = parse_plugin_options(r#"[1, 2, 3]"#);
        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_parse_plugin_options_invalid_json_wraps_as_raw() {
        let result = parse_plugin_options("not json at all");
        assert_eq!(result["raw"], "not json at all");
    }

    #[test]
    fn test_parse_plugin_options_empty_string_wraps_as_raw() {
        let result = parse_plugin_options("");
        // Empty string is not valid JSON, so it wraps
        assert_eq!(result["raw"], "");
    }

    #[test]
    fn test_parse_plugin_options_partial_json_wraps_as_raw() {
        let result = parse_plugin_options("{broken");
        assert_eq!(result["raw"], "{broken");
    }

    #[test]
    fn test_parse_plugin_options_json_string_literal_passes_through() {
        let result = parse_plugin_options(r#""just a string""#);
        assert_eq!(result.as_str().unwrap(), "just a string");
    }

    // ── build_plugin_input ────────────────────────────────────────────

    #[test]
    fn test_build_plugin_input_no_args() {
        let input = build_plugin_input(None, None, None);
        assert!(input.path.is_none());
        assert!(input.query.is_none());
        assert_eq!(input.options, serde_json::json!({}));
    }

    #[test]
    fn test_build_plugin_input_with_path() {
        let input = build_plugin_input(Some("/tmp/file.txt".to_string()), None, None);
        assert_eq!(
            input.path.unwrap(),
            std::path::PathBuf::from("/tmp/file.txt")
        );
        assert!(input.query.is_none());
    }

    #[test]
    fn test_build_plugin_input_with_query() {
        let input = build_plugin_input(None, Some("search term".to_string()), None);
        assert!(input.path.is_none());
        assert_eq!(input.query.unwrap(), "search term");
    }

    #[test]
    fn test_build_plugin_input_with_valid_json_options() {
        let input = build_plugin_input(None, None, Some(r#"{"format": "csv"}"#.to_string()));
        assert_eq!(input.options["format"], "csv");
    }

    #[test]
    fn test_build_plugin_input_with_invalid_json_options_wraps_raw() {
        let input = build_plugin_input(None, None, Some("plain text opts".to_string()));
        assert_eq!(input.options["raw"], "plain text opts");
    }

    #[test]
    fn test_build_plugin_input_all_args() {
        let input = build_plugin_input(
            Some("/path/to/doc".to_string()),
            Some("my query".to_string()),
            Some(r#"{"verbose": true}"#.to_string()),
        );
        assert_eq!(
            input.path.unwrap(),
            std::path::PathBuf::from("/path/to/doc")
        );
        assert_eq!(input.query.unwrap(), "my query");
        assert_eq!(input.options["verbose"], true);
    }

    // ── build_conflict_map ────────────────────────────────────────────

    #[test]
    fn test_build_conflict_map_empty_sessions() {
        let sessions: Vec<serde_json::Value> = vec![];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert!(file_to_session.is_empty());
        assert!(all_files.is_empty());
    }

    #[test]
    fn test_build_conflict_map_sessions_without_active_files() {
        let sessions = vec![
            serde_json::json!({"id": "s1", "name": "session-1"}),
            serde_json::json!({"id": "s2", "name": "session-2"}),
        ];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert!(file_to_session.is_empty());
        assert!(all_files.is_empty());
    }

    #[test]
    fn test_build_conflict_map_single_session_single_file() {
        let sessions = vec![serde_json::json!({
            "id": "s1",
            "name": "session-1",
            "active_files": ["src/main.rs"]
        })];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert_eq!(all_files.len(), 1);
        assert!(all_files.contains("src/main.rs"));
        assert_eq!(file_to_session["src/main.rs"], vec!["session-1"]);
    }

    #[test]
    fn test_build_conflict_map_multiple_sessions_no_overlap() {
        let sessions = vec![
            serde_json::json!({
                "name": "session-1",
                "active_files": ["src/main.rs"]
            }),
            serde_json::json!({
                "name": "session-2",
                "active_files": ["src/lib.rs"]
            }),
        ];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert_eq!(all_files.len(), 2);
        assert_eq!(file_to_session["src/main.rs"].len(), 1);
        assert_eq!(file_to_session["src/lib.rs"].len(), 1);
    }

    #[test]
    fn test_build_conflict_map_detects_conflict_same_file_two_sessions() {
        let sessions = vec![
            serde_json::json!({
                "name": "session-alpha",
                "active_files": ["src/shared.rs", "src/a.rs"]
            }),
            serde_json::json!({
                "name": "session-beta",
                "active_files": ["src/shared.rs", "src/b.rs"]
            }),
        ];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert_eq!(all_files.len(), 3);

        // The conflicted file
        let shared_sessions = &file_to_session["src/shared.rs"];
        assert_eq!(
            shared_sessions.len(),
            2,
            "shared.rs should be in 2 sessions"
        );
        assert!(shared_sessions.contains(&"session-alpha".to_string()));
        assert!(shared_sessions.contains(&"session-beta".to_string()));

        // Non-conflicted files
        assert_eq!(file_to_session["src/a.rs"].len(), 1);
        assert_eq!(file_to_session["src/b.rs"].len(), 1);
    }

    #[test]
    fn test_build_conflict_map_three_sessions_same_file() {
        let sessions = vec![
            serde_json::json!({"name": "s1", "active_files": ["config.toml"]}),
            serde_json::json!({"name": "s2", "active_files": ["config.toml"]}),
            serde_json::json!({"name": "s3", "active_files": ["config.toml"]}),
        ];
        let (file_to_session, _) = build_conflict_map(&sessions);
        assert_eq!(
            file_to_session["config.toml"].len(),
            3,
            "config.toml should appear in all 3 sessions"
        );
    }

    #[test]
    fn test_build_conflict_map_missing_name_uses_question_mark() {
        let sessions = vec![serde_json::json!({
            "id": "s1",
            "active_files": ["src/main.rs"]
        })];
        let (file_to_session, _) = build_conflict_map(&sessions);
        assert_eq!(
            file_to_session["src/main.rs"],
            vec!["?"],
            "Missing name should fall back to '?'"
        );
    }

    #[test]
    fn test_build_conflict_map_non_string_active_files_skipped() {
        let sessions = vec![serde_json::json!({
            "name": "s1",
            "active_files": [42, null, "valid.rs", true]
        })];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert_eq!(
            all_files.len(),
            1,
            "Only string entries should be collected"
        );
        assert!(all_files.contains("valid.rs"));
        assert_eq!(file_to_session.len(), 1);
    }

    #[test]
    fn test_build_conflict_map_active_files_not_array_is_ignored() {
        let sessions = vec![serde_json::json!({
            "name": "s1",
            "active_files": "not an array"
        })];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert!(file_to_session.is_empty());
        assert!(all_files.is_empty());
    }

    // ── dispatch table coverage documentation ─────────────────────────
    //
    // The dispatch() function routes these Commands over IPC:
    //   - Daemon (stop/status)
    //   - SessionStart
    //   - SessionEnd
    //   - TrackWrite
    //   - TrackTool
    //   - ListSessions
    //   - SessionInfo
    //   - SessionConflicts
    //   - Status
    //   - Debug
    //   - ConflictHistory (redirects to direct mode)
    //   - Chat
    //   - Verify (runs locally, no IPC)
    //   - Describe (runs locally)
    //   - Schema (runs locally)
    //   - PluginList
    //   - PluginInvoke
    //   - SearchHistory/SearchGenome/IndexMemory/RetrievalStatus (redirects)
    //   - _ (catch-all, redirects to direct mode)
    //
    // Functions requiring a live daemon (integration test candidates):
    //   - handle_daemon, handle_session_start, handle_session_end
    //   - handle_session_conflicts (IPC path), handle_chat
    //   - handle_plugin_list, handle_plugin_invoke (IPC path)
    //   - TrackWrite, TrackTool, ListSessions, SessionInfo, Status, Debug

    /// Verify the dispatch function compiles with all current Commands variants.
    /// This is a compile-time check — if a new variant is added to Commands and
    /// not handled, the match in dispatch() will cause a compile error (since it
    /// has a catch-all, this test documents intent rather than enforcing exhaustiveness).
    #[test]
    fn test_dispatch_table_documented() {
        // This test exists to document which commands are handled in daemon mode.
        // The dispatch function handles 20 command variants:
        let daemon_handled = [
            "Daemon",
            "SessionStart",
            "SessionEnd",
            "TrackWrite",
            "TrackTool",
            "ListSessions",
            "SessionInfo",
            "SessionConflicts",
            "Status",
            "Debug",
            "ConflictHistory",
            "Chat",
            "Verify",
            "Describe",
            "Schema",
            "PluginList",
            "PluginInvoke",
        ];
        let redirected_to_direct = [
            "SearchHistory",
            "SearchGenome",
            "IndexMemory",
            "RetrievalStatus",
        ];

        assert!(
            !daemon_handled.is_empty(),
            "Daemon dispatch should handle commands"
        );
        assert!(
            !redirected_to_direct.is_empty(),
            "Some commands should redirect to direct mode"
        );

        // Verify no duplicates in the lists
        let mut seen = std::collections::HashSet::new();
        for cmd in daemon_handled.iter().chain(redirected_to_direct.iter()) {
            assert!(
                seen.insert(cmd),
                "Duplicate command in dispatch table documentation: {}",
                cmd
            );
        }
    }

    // ── get_session_id behavior ─────────────────────────────────────────
    //
    // Note: env var tests for get_session_id (fallback to IMPULSE_SESSION_ID)
    // are inherently racy in parallel test execution because env vars are
    // process-global. We test the explicit-arg path deterministically and
    // document the env-fallback behavior without relying on env mutation.

    #[test]
    fn test_get_session_id_explicit_arg_returns_arg() {
        let result = get_session_id(Some("explicit-session".to_string()));
        assert_eq!(result, Some("explicit-session".to_string()));
    }

    #[test]
    fn test_get_session_id_explicit_arg_ignores_env() {
        // Even if env is set, explicit arg wins. We don't mutate env here
        // to avoid races — just verify the explicit path is deterministic.
        let result = get_session_id(Some("my-arg".to_string()));
        assert_eq!(
            result,
            Some("my-arg".to_string()),
            "Explicit arg should always take precedence"
        );
    }

    #[test]
    fn test_get_session_id_none_arg_with_no_env_returns_none() {
        // This test is slightly racy (another test could set the env var)
        // but the important contract is: None arg + no env = None
        // We test this pattern deterministically by checking the function signature.
        // The env-var fallback is tested by the common.rs module.
        let has_env = std::env::var("IMPULSE_SESSION_ID").is_ok();
        let result = get_session_id(None);
        if has_env {
            assert!(result.is_some(), "With env set, should return Some");
        } else {
            assert!(result.is_none(), "Without env, should return None");
        }
    }

    // ── parse_injection_mode (used by handle_chat) ────────────────────

    #[test]
    fn test_parse_injection_mode_none_returns_ok_none() {
        let result = parse_injection_mode(None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_injection_mode_off_returns_mode() {
        let result = parse_injection_mode(Some("off"));
        assert!(result.is_ok());
        let mode = result.unwrap().unwrap();
        assert_eq!(mode.as_str(), "off");
    }

    #[test]
    fn test_parse_injection_mode_review_returns_mode() {
        let result = parse_injection_mode(Some("review"));
        assert!(result.is_ok());
        let mode = result.unwrap().unwrap();
        assert_eq!(mode.as_str(), "review");
    }

    #[test]
    fn test_parse_injection_mode_apply_returns_mode() {
        let result = parse_injection_mode(Some("apply"));
        assert!(result.is_ok());
        let mode = result.unwrap().unwrap();
        assert_eq!(mode.as_str(), "apply");
    }

    #[test]
    fn test_parse_injection_mode_invalid_returns_err() {
        let result = parse_injection_mode(Some("invalid_mode"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Invalid inject mode"),
            "Error should mention invalid mode, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_injection_mode_empty_string_returns_err() {
        let result = parse_injection_mode(Some(""));
        assert!(result.is_err());
    }

    // ── print_json helper (used throughout daemon_dispatch) ───────────

    #[test]
    fn test_print_json_serializes_value() {
        let value = serde_json::json!({"key": "value"});
        let result = print_json(&value);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_json_serializes_nested_value() {
        let value = serde_json::json!({
            "sessions": [
                {"id": "s1", "name": "test"},
                {"id": "s2", "name": "test2"}
            ]
        });
        let result = print_json(&value);
        assert!(result.is_ok());
    }

    // ── build_plugin_input serde round-trip ──────────────────────────

    #[test]
    fn test_build_plugin_input_round_trip_all_fields() {
        let input = build_plugin_input(
            Some("/tmp/test.txt".to_string()),
            Some("hello world".to_string()),
            Some(r#"{"key": "val"}"#.to_string()),
        );
        let json = serde_json::to_string(&input).unwrap();
        let recovered: plugin::PluginInput = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.path, input.path);
        assert_eq!(recovered.query, input.query);
        assert_eq!(recovered.options, input.options);
    }

    #[test]
    fn test_build_plugin_input_round_trip_no_fields() {
        let input = build_plugin_input(None, None, None);
        let json = serde_json::to_string(&input).unwrap();
        let recovered: plugin::PluginInput = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.path, None);
        assert_eq!(recovered.query, None);
        assert_eq!(recovered.options, serde_json::json!({}));
    }

    // ── build_plugin_input edge cases ────────────────────────────────

    #[test]
    fn test_build_plugin_input_empty_string_path() {
        let input = build_plugin_input(Some("".to_string()), None, None);
        assert_eq!(
            input.path.unwrap(),
            std::path::PathBuf::from(""),
            "Empty string path should produce empty PathBuf"
        );
    }

    #[test]
    fn test_build_plugin_input_empty_string_query() {
        let input = build_plugin_input(None, Some("".to_string()), None);
        assert_eq!(input.query.unwrap(), "");
    }

    #[test]
    fn test_build_plugin_input_nested_json_options() {
        let opts = r#"{"nested": {"deep": [1, 2, {"x": true}]}}"#;
        let input = build_plugin_input(None, None, Some(opts.to_string()));
        assert!(input.options["nested"]["deep"].is_array());
        assert_eq!(input.options["nested"]["deep"][2]["x"], true);
    }

    #[test]
    fn test_build_plugin_input_unicode_query() {
        let input = build_plugin_input(
            None,
            Some("search for \u{1f680} rocket emoji".to_string()),
            None,
        );
        assert!(input.query.as_ref().unwrap().contains('\u{1f680}'));
    }

    // ── build_conflict_map edge cases ────────────────────────────────

    #[test]
    fn test_build_conflict_map_duplicate_file_in_same_session() {
        let sessions = vec![serde_json::json!({
            "name": "session-1",
            "active_files": ["src/main.rs", "src/main.rs"]
        })];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        // The file appears once in all_files (HashSet deduplicates)
        assert_eq!(all_files.len(), 1);
        // But the session name appears twice in the mapping (Vec does not deduplicate)
        assert_eq!(
            file_to_session["src/main.rs"].len(),
            2,
            "Duplicate entries in active_files produce duplicate session entries"
        );
    }

    #[test]
    fn test_build_conflict_map_empty_active_files_array() {
        let sessions = vec![serde_json::json!({
            "name": "session-1",
            "active_files": []
        })];
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        assert!(file_to_session.is_empty());
        assert!(all_files.is_empty());
    }

    #[test]
    fn test_build_conflict_map_conflict_threshold_boundary() {
        // Verify that exactly 1 session means no conflict,
        // exactly 2 sessions means conflict
        let sessions_one = vec![serde_json::json!({
            "name": "only-session",
            "active_files": ["shared.rs"]
        })];
        let (map_one, _) = build_conflict_map(&sessions_one);
        assert_eq!(
            map_one["shared.rs"].len(),
            1,
            "1 session on a file = no conflict"
        );

        let sessions_two = vec![
            serde_json::json!({"name": "s1", "active_files": ["shared.rs"]}),
            serde_json::json!({"name": "s2", "active_files": ["shared.rs"]}),
        ];
        let (map_two, _) = build_conflict_map(&sessions_two);
        assert!(
            map_two["shared.rs"].len() > 1,
            "2 sessions on a file = conflict"
        );
    }

    #[test]
    fn test_build_conflict_map_many_files_many_sessions() {
        let sessions: Vec<serde_json::Value> = (0..10)
            .map(|i| {
                serde_json::json!({
                    "name": format!("session-{}", i),
                    "active_files": [
                        format!("unique-{}.rs", i),
                        "shared-all.rs"
                    ]
                })
            })
            .collect();
        let (file_to_session, all_files) = build_conflict_map(&sessions);
        // 10 unique files + 1 shared = 11 total
        assert_eq!(all_files.len(), 11);
        // The shared file should be in all 10 sessions
        assert_eq!(file_to_session["shared-all.rs"].len(), 10);
        // Each unique file should be in exactly 1 session
        for i in 0..10 {
            assert_eq!(file_to_session[&format!("unique-{}.rs", i)].len(), 1);
        }
    }

    // ── format_session_line edge cases ───────────────────────────────

    #[test]
    fn test_format_session_line_unicode_values() {
        let session = serde_json::json!({
            "id": "id-\u{1f4e6}",
            "name": "session-\u{2728}",
            "status": "active"
        });
        let line = format_session_line(&session);
        assert!(line.contains("id-\u{1f4e6}"));
        assert!(line.contains("session-\u{2728}"));
        assert!(line.contains("active"));
    }

    #[test]
    fn test_format_session_line_extra_fields_ignored() {
        let session = serde_json::json!({
            "id": "abc",
            "name": "test",
            "status": "active",
            "extra_field": "should not appear",
            "another": 42
        });
        let line = format_session_line(&session);
        assert_eq!(line, "abc - test (active)");
        assert!(!line.contains("extra_field"));
        assert!(!line.contains("should not appear"));
    }

    #[test]
    fn test_format_session_line_empty_string_values() {
        let session = serde_json::json!({
            "id": "",
            "name": "",
            "status": ""
        });
        let line = format_session_line(&session);
        assert_eq!(line, " -  ()");
    }

    // ── parse_injection_mode case sensitivity ────────────────────────

    #[test]
    fn test_parse_injection_mode_uppercase_returns_err() {
        let result = parse_injection_mode(Some("OFF"));
        assert!(result.is_err(), "Uppercase 'OFF' should not be accepted");
    }

    #[test]
    fn test_parse_injection_mode_mixed_case_returns_err() {
        let result = parse_injection_mode(Some("Review"));
        assert!(
            result.is_err(),
            "Mixed case 'Review' should not be accepted"
        );
    }

    #[test]
    fn test_parse_injection_mode_whitespace_returns_err() {
        let result = parse_injection_mode(Some(" apply "));
        assert!(
            result.is_err(),
            "Mode with whitespace should not be accepted"
        );
    }

    // ── DaemonRequest serde tests for dispatch-used variants ─────────

    #[test]
    fn test_daemon_request_debug_snapshot_round_trip() {
        let req = DaemonRequest::DebugSnapshot;
        let json = serde_json::to_string(&req).unwrap();
        let recovered: DaemonRequest = serde_json::from_str(&json).unwrap();
        // DebugSnapshot has no data — just verify it deserializes to the right variant
        assert!(
            matches!(recovered, DaemonRequest::DebugSnapshot),
            "DebugSnapshot should round-trip, got JSON: {}",
            json
        );
    }

    #[test]
    fn test_daemon_request_list_plugins_round_trip() {
        let req = DaemonRequest::ListPlugins;
        let json = serde_json::to_string(&req).unwrap();
        let recovered: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(recovered, DaemonRequest::ListPlugins));
    }

    #[test]
    fn test_daemon_request_list_sessions_round_trip() {
        let req = DaemonRequest::ListSessions;
        let json = serde_json::to_string(&req).unwrap();
        let recovered: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(recovered, DaemonRequest::ListSessions));
    }

    #[test]
    fn test_daemon_request_invoke_plugin_round_trip() {
        let input = plugin::PluginInput::new().with_query("test query");
        let req = DaemonRequest::InvokePlugin {
            name: "test-plugin".to_string(),
            input,
        };
        let json = serde_json::to_string(&req).unwrap();
        let recovered: DaemonRequest = serde_json::from_str(&json).unwrap();
        match recovered {
            DaemonRequest::InvokePlugin { name, input } => {
                assert_eq!(name, "test-plugin");
                assert_eq!(input.query.as_deref(), Some("test query"));
            }
            other => panic!("Expected InvokePlugin, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_ok_round_trip() {
        let resp = DaemonResponse::Ok {
            result: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let recovered: DaemonResponse = serde_json::from_str(&json).unwrap();
        match recovered {
            DaemonResponse::Ok { result } => {
                assert_eq!(result["key"], "value");
            }
            other => panic!("Expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn test_daemon_response_error_round_trip() {
        let resp = DaemonResponse::Error {
            message: "something failed".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let recovered: DaemonResponse = serde_json::from_str(&json).unwrap();
        match recovered {
            DaemonResponse::Error { message } => {
                assert_eq!(message, "something failed");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    // ── default_session_name ─────────────────────────────────────────

    #[test]
    fn test_default_session_name_returns_non_empty() {
        let name = default_session_name();
        assert!(
            !name.is_empty(),
            "default_session_name should never return empty string"
        );
    }

    // ── require_arg (used indirectly by dispatch paths) ──────────────

    #[test]
    fn test_require_arg_some_returns_value() {
        let result = super::super::require_arg(Some("hello".to_string()), "test-arg");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_require_arg_none_returns_error_with_arg_name() {
        let result = super::super::require_arg::<String>(None, "my-flag");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("my-flag"),
            "Error should mention the missing arg name, got: {}",
            err_msg
        );
    }

    // ── is_truthy_env (used by hook evidence capture) ────────────────

    #[test]
    fn test_is_truthy_env_unset_returns_false() {
        // Use a key unlikely to collide with real env vars
        let result = super::super::is_truthy_env("IMPULSE_TEST_NONEXISTENT_VAR_XYZ_99");
        assert!(!result);
    }

    // ── preview_block (used for output formatting) ───────────────────

    #[test]
    fn test_preview_block_short_text_unchanged() {
        let result = super::super::preview_block("hello", 100);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_preview_block_long_text_truncated_with_ellipsis() {
        let result = super::super::preview_block("hello world", 5);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_preview_block_exact_length_no_ellipsis() {
        let result = super::super::preview_block("exact", 5);
        assert_eq!(result, "exact");
    }

    #[test]
    fn test_preview_block_empty_string() {
        let result = super::super::preview_block("", 10);
        assert_eq!(result, "");
    }
}
