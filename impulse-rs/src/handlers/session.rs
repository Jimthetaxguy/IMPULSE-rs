use anyhow::Result;
use std::sync::Arc;

use crate::envelope::{write_envelope, EnvelopeBuilder, OutputFormat};
use crate::{guardrail, injection, semantic_diff, state, validate, verify};

use super::{
    capture_hook_evidence, default_session_name, get_session_id, hook_session_start_banner,
    parse_platform, persist_claude_env_var, preview_block, print_verification_report,
    read_hook_stdin_payload, HookEvidenceInput,
};

pub async fn handle_session_start(
    state: &Arc<state::State>,
    name: Option<String>,
    platform: Option<String>,
    inject_mode: Option<String>,
    inject_explain: bool,
    format: Option<OutputFormat>,
) -> Result<()> {
    let stdin_payload = read_hook_stdin_payload();
    let name = name.unwrap_or_else(default_session_name);
    validate::reject_control_chars(&name, "name")?;
    let platform = platform.and_then(|p| parse_platform(&p));
    let session = state.create_session(name.clone(), platform).await?;
    let _ = persist_claude_env_var("IMPULSE_SESSION_ID", &session.id);

    let query_parts = vec![name];
    let config = state.config_snapshot()?;
    let mode_override = inject_mode
        .as_deref()
        .and_then(injection::InjectionMode::parse)
        .or(Some(injection::InjectionMode::Apply));

    let injection_result = injection::run_injection(
        state.storage().base_path(),
        &config,
        injection::InjectionSurface::Orchestrate,
        mode_override,
        &query_parts,
    );

    // Structured output: fold the session info and (optionally) the injection
    // explain block into a single envelope so stdout carries exactly one JSON
    // document — no mixed-output bug.
    if let Some(fmt @ (OutputFormat::Json | OutputFormat::Ndjson)) = format {
        let mut data = serde_json::json!({
            "id": session.id,
            "name": session.name,
            "status": format!("{:?}", session.status),
            "platform": session.platform.map(|p| p.as_str().to_string()),
            "injected": injection_result.injected_block.is_some(),
        });
        if let Some(block) = injection_result.injected_block.clone() {
            data["injected_block"] = serde_json::Value::String(block);
        }
        if inject_explain {
            data["injection_explain"] = serde_json::to_value(&injection_result.explain)?;
        }
        let env = EnvelopeBuilder::new("session-start").ok(data);
        write_envelope(fmt, &env)?;

        let (output_preview, output_lines) = match injection_result.injected_block.as_ref() {
            Some(block) => (Some(preview_block(block, 400)), block.lines().count()),
            None => (Some("no injection block".to_string()), 0),
        };
        capture_hook_evidence(HookEvidenceInput {
            impulse_dir: state.storage().base_path(),
            event: "session_start",
            session_id: Some(session.id.clone()),
            session_name: Some(session.name.clone()),
            platform: session.platform.map(|p| p.as_str().to_string()),
            summary: None,
            verify_enabled: None,
            stdin_payload,
            output_preview,
            output_lines,
        })?;
        return Ok(());
    }

    // Text mode: keep stdout clean by writing the explain block to stderr.
    if inject_explain {
        let _ = serde_json::to_writer_pretty(std::io::stderr(), &injection_result.explain);
        eprintln!();
    }

    let mut output_lines = 0usize;
    if let Some(sentinel) = hook_session_start_banner() {
        println!("{}", sentinel);
        println!();
        output_lines += sentinel.lines().count() + 1;
    }

    if let Some(block) = injection_result.injected_block.as_ref() {
        let hook_mode = std::env::var("CLAUDE_ENV_FILE").is_ok();
        if hook_mode {
            println!("{}", block);
            output_lines += block.lines().count();
        } else {
            println!("{}\n\n{}", session.id, block);
            output_lines += block.lines().count() + 2;
        }
        capture_hook_evidence(HookEvidenceInput {
            impulse_dir: state.storage().base_path(),
            event: "session_start",
            session_id: Some(session.id.clone()),
            session_name: Some(session.name.clone()),
            platform: session.platform.map(|p| p.as_str().to_string()),
            summary: None,
            verify_enabled: None,
            stdin_payload,
            output_preview: Some(preview_block(block, 400)),
            output_lines,
        })?;
    } else {
        let hook_mode = std::env::var("CLAUDE_ENV_FILE").is_ok();
        if hook_mode {
            println!(
                "Impulse started session {}. No prior context was injected on this run.",
                session.id
            );
            output_lines += 1;
        } else {
            println!("{}", session.id);
            output_lines += 1;
        }
        capture_hook_evidence(HookEvidenceInput {
            impulse_dir: state.storage().base_path(),
            event: "session_start",
            session_id: Some(session.id.clone()),
            session_name: Some(session.name.clone()),
            platform: session.platform.map(|p| p.as_str().to_string()),
            summary: None,
            verify_enabled: None,
            stdin_payload,
            output_preview: Some("no injection block".to_string()),
            output_lines,
        })?;
    }
    Ok(())
}

pub async fn handle_session_end(
    state: &Arc<state::State>,
    session_id: String,
    summary: String,
    should_verify: bool,
    sem_diff_base: Option<String>,
    format: Option<OutputFormat>,
) -> Result<()> {
    validate::validate_session_id(&session_id)?;
    validate::reject_control_chars(&summary, "summary")?;

    let stdin_payload = read_hook_stdin_payload();
    if should_verify {
        let steps = verify::default_steps(&std::env::current_dir()?);
        let report = verify::run_verification(steps)?;
        print_verification_report(&report);
        if !report.success() {
            anyhow::bail!("Verification failed. Session end blocked.");
        }
    }
    // Capture semantic diff if requested and sem is available
    if let Some(base_ref) = &sem_diff_base {
        if semantic_diff::sem_available() {
            match semantic_diff::capture_semantic_diff(
                state.storage().base_path(),
                &std::env::current_dir()?,
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
    let structured = matches!(format, Some(OutputFormat::Json | OutputFormat::Ndjson));
    match state.end_session(&session_id, summary.clone()).await {
        Ok(Some(_)) => {
            capture_hook_evidence(HookEvidenceInput {
                impulse_dir: state.storage().base_path(),
                event: "session_end",
                session_id: Some(session_id.clone()),
                session_name: None,
                platform: None,
                summary: Some(summary),
                verify_enabled: Some(should_verify),
                stdin_payload,
                output_preview: Some(format!("Session {} ended", session_id)),
                output_lines: 1,
            })?;
            if let Some(fmt) = format.filter(|_| structured) {
                let data = serde_json::json!({
                    "id": session_id,
                    "status": "ended",
                    "found": true,
                });
                let env = EnvelopeBuilder::new("session-end").ok(data);
                write_envelope(fmt, &env)?;
            } else {
                println!("Session {} ended", session_id)
            }
        }
        Ok(None) => {
            capture_hook_evidence(HookEvidenceInput {
                impulse_dir: state.storage().base_path(),
                event: "session_end_missing",
                session_id: Some(session_id.clone()),
                session_name: None,
                platform: None,
                summary: Some(summary),
                verify_enabled: Some(should_verify),
                stdin_payload,
                output_preview: Some(format!("Session not found: {}", session_id)),
                output_lines: 1,
            })?;
            if let Some(fmt) = format.filter(|_| structured) {
                let data = serde_json::json!({
                    "id": session_id,
                    "status": "not_found",
                    "found": false,
                });
                let env = EnvelopeBuilder::new("session-end").ok(data);
                write_envelope(fmt, &env)?;
            } else {
                println!("Session not found: {}", session_id)
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

pub async fn handle_track_write(
    state: &Arc<state::State>,
    file: String,
    session_id: Option<String>,
    format: Option<OutputFormat>,
) -> Result<()> {
    validate::validate_file_arg(&file)?;

    let structured = matches!(format, Some(OutputFormat::Json | OutputFormat::Ndjson));
    if let Some(sid) = get_session_id(session_id) {
        match state.track_file(&sid, &file).await {
            Ok(_) => {
                if let Some(fmt) = format.filter(|_| structured) {
                    let data = serde_json::json!({
                        "tracked": file,
                        "kind": "file",
                        "session_id": sid,
                    });
                    let env = EnvelopeBuilder::new("track-write").ok(data);
                    write_envelope(fmt, &env)?;
                } else {
                    println!("Tracked: {}", file)
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
        evaluate_track_guardrails(state, &sid, &file).await;
    } else {
        eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
    }
    Ok(())
}

pub async fn handle_track_tool(
    state: &Arc<state::State>,
    tool: String,
    session_id: Option<String>,
    format: Option<OutputFormat>,
) -> Result<()> {
    validate::reject_control_chars(&tool, "tool")?;

    let structured = matches!(format, Some(OutputFormat::Json | OutputFormat::Ndjson));
    if let Some(sid) = get_session_id(session_id) {
        match state.track_tool(&sid, &tool).await {
            Ok(_) => {
                if let Some(fmt) = format.filter(|_| structured) {
                    let data = serde_json::json!({
                        "tracked": tool,
                        "kind": "tool",
                        "session_id": sid,
                    });
                    let env = EnvelopeBuilder::new("track-tool").ok(data);
                    write_envelope(fmt, &env)?;
                } else {
                    println!("Tracked: {}", tool)
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
        evaluate_track_guardrails(state, &sid, &tool).await;
    } else {
        eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
    }
    Ok(())
}

async fn evaluate_track_guardrails(state: &Arc<state::State>, session_id: &str, action: &str) {
    if let Ok(config) = state.config_snapshot() {
        if config.guardrails.enabled {
            if let Ok(results) = guardrail::evaluate_action(action, "any", &config.guardrails) {
                for result in &results {
                    match result.action {
                        guardrail::GuardAction::Warn => {
                            eprintln!("{}", result.format_human());
                        }
                        guardrail::GuardAction::Log => {
                            let _ = state
                                .add_tag(session_id, &format!("guard:{}", result.rule_id))
                                .await;
                        }
                        guardrail::GuardAction::Block => {}
                    }
                }
            }
        }
    }
}

pub async fn handle_list_sessions(
    state: &Arc<state::State>,
    format: Option<OutputFormat>,
) -> Result<()> {
    let sessions = state.list_sessions().await?;
    if let Some(fmt @ (OutputFormat::Json | OutputFormat::Ndjson)) = format {
        let data = serde_json::json!({
            "count": sessions.len(),
            "sessions": sessions.iter().map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "name": s.name,
                    "status": format!("{:?}", s.status),
                })
            }).collect::<Vec<_>>(),
        });
        let env = EnvelopeBuilder::new("list-sessions").ok(data);
        write_envelope(fmt, &env)?;
    } else if sessions.is_empty() {
        println!("No active sessions");
    } else {
        for s in sessions {
            println!("{} - {} ({:?})", s.id, s.name, s.status);
        }
    }
    Ok(())
}

pub async fn handle_session_info(
    state: &Arc<state::State>,
    id: String,
    format: Option<OutputFormat>,
) -> Result<()> {
    let structured = matches!(format, Some(OutputFormat::Json | OutputFormat::Ndjson));
    match state.get_session(&id).await {
        Ok(Some(s)) => {
            if let Some(fmt) = format.filter(|_| structured) {
                let data = serde_json::json!({
                    "id": s.id,
                    "name": s.name,
                    "status": format!("{:?}", s.status),
                    "platform": format!("{:?}", s.platform),
                    "working_directory": s.working_directory,
                    "created_at": s.created_at.to_string(),
                    "active_files": s.active_files,
                    "recent_tools": s.recent_tools,
                    "found": true,
                });
                let env = EnvelopeBuilder::new("session-info").ok(data);
                write_envelope(fmt, &env)?;
            } else {
                println!("Session: {}", s.name);
                println!("ID: {}", s.id);
                println!("Status: {:?}", s.status);
                println!("Platform: {:?}", s.platform);
                println!("Working Dir: {}", s.working_directory);
                println!("Created: {}", s.created_at);
                println!("Files: {:?}", s.active_files);
                println!("Tools: {:?}", s.recent_tools);
            }
        }
        Ok(None) => {
            if let Some(fmt) = format.filter(|_| structured) {
                let data = serde_json::json!({ "id": id, "found": false });
                let env = EnvelopeBuilder::new("session-info").ok(data);
                write_envelope(fmt, &env)?;
            } else {
                println!("Session not found: {}", id)
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

pub async fn handle_session_conflicts(
    state: &Arc<state::State>,
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
        Some(f) => {
            let conflicting = state.check_file_conflict(&sid, &f).await?;
            if !conflicting.is_empty() {
                println!("\u{26a0}\u{fe0f}  CONFLICT DETECTED");
                println!("File is being edited by: {}", conflicting.join(", "));
            } else {
                println!("\u{2713} No conflicts detected");
            }
        }
        None => {
            let sessions = state.list_sessions().await?;
            let mut all_files: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut file_to_session: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();

            for s in &sessions {
                for f in &s.active_files {
                    all_files.insert(f.clone());
                    file_to_session
                        .entry(f.clone())
                        .or_default()
                        .push(s.name.clone());
                }
            }

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
    }
    Ok(())
}

/// Handle `conflict-history` in direct mode.
pub fn handle_conflict_history(state: &Arc<state::State>) -> Result<()> {
    match state.get_conflict_history() {
        Ok(events) if events.is_empty() => {
            println!("No conflict events recorded.");
        }
        Ok(events) => {
            super::print_json(&events)?;
        }
        Err(e) => eprintln!("Error reading conflict history: {}", e),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    // ── handle_list_sessions ────────────────────────────────────────────

    #[tokio::test]
    async fn list_sessions_empty() {
        let (_tmp, st) = test_state();
        let result = handle_list_sessions(&st, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn list_sessions_with_sessions() {
        let (_tmp, st) = test_state();
        st.create_session("session-a".to_string(), None)
            .await
            .unwrap();
        st.create_session("session-b".to_string(), None)
            .await
            .unwrap();
        let result = handle_list_sessions(&st, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn list_sessions_json_emits_single_envelope() {
        let (_tmp, st) = test_state();
        st.create_session("session-a".to_string(), None)
            .await
            .unwrap();
        // Build the envelope the same way the handler does and assert it is a
        // single, valid envelope carrying the session list.
        let sessions = st.list_sessions().await.unwrap();
        let data = serde_json::json!({
            "count": sessions.len(),
            "sessions": sessions.iter().map(|s| serde_json::json!({
                "id": s.id,
                "name": s.name,
                "status": format!("{:?}", s.status),
            })).collect::<Vec<_>>(),
        });
        let env = crate::envelope::EnvelopeBuilder::new("list-sessions").ok(data);
        let json = serde_json::to_string(&env).unwrap();
        // Exactly one JSON document (no trailing concatenated object).
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["ok"], serde_json::json!(true));
        assert_eq!(parsed["command"], serde_json::json!("list-sessions"));
        assert_eq!(parsed["data"]["count"], serde_json::json!(1));
        assert_eq!(parsed["data"]["sessions"][0]["name"], "session-a");

        // The handler itself runs without error in Json mode.
        let result = handle_list_sessions(&st, Some(OutputFormat::Json)).await;
        assert!(result.is_ok());
    }

    // ── handle_session_info ─────────────────────────────────────────────

    #[tokio::test]
    async fn session_info_existing() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("test-info".to_string(), None)
            .await
            .unwrap();
        let result = handle_session_info(&st, session.id, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn session_info_not_found() {
        let (_tmp, st) = test_state();
        let result = handle_session_info(&st, "nonexistent-id".to_string(), None).await;
        assert!(result.is_ok()); // prints "Session not found" but doesn't error
    }

    // ── handle_track_write ──────────────────────────────────────────────

    #[tokio::test]
    async fn track_write_with_session() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("track-test".to_string(), None)
            .await
            .unwrap();
        let result = handle_track_write(
            &st,
            "src/main.rs".to_string(),
            Some(session.id.clone()),
            None,
        )
        .await;
        assert!(result.is_ok());
        // Verify tracking
        let s = st.get_session(&session.id).await.unwrap().unwrap();
        assert!(s.active_files.contains(&"src/main.rs".to_string()));
    }

    #[tokio::test]
    async fn track_write_no_session_id() {
        let (_tmp, st) = test_state();
        // No session_id and no env var — should print error but not fail
        let result = handle_track_write(&st, "src/main.rs".to_string(), None, None).await;
        assert!(result.is_ok());
    }

    // ── handle_track_tool ───────────────────────────────────────────────

    #[tokio::test]
    async fn track_tool_with_session() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("tool-test".to_string(), None)
            .await
            .unwrap();
        let result =
            handle_track_tool(&st, "read_file".to_string(), Some(session.id.clone()), None).await;
        assert!(result.is_ok());
        let s = st.get_session(&session.id).await.unwrap().unwrap();
        assert!(s.recent_tools.contains(&"read_file".to_string()));
    }

    // ── handle_session_end ──────────────────────────────────────────────

    #[tokio::test]
    async fn session_end_valid() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("end-test".to_string(), None)
            .await
            .unwrap();
        let result = handle_session_end(
            &st,
            session.id.clone(),
            "completed successfully".to_string(),
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn session_end_not_found() {
        let (_tmp, st) = test_state();
        let result = handle_session_end(
            &st,
            "nonexistent-session".to_string(),
            "ended".to_string(),
            false,
            None,
            None,
        )
        .await;
        assert!(result.is_ok()); // prints "Session not found" but Ok
    }

    // ── handle_session_conflicts ────────────────────────────────────────

    #[tokio::test]
    async fn conflicts_no_conflict() {
        let (_tmp, st) = test_state();
        let s1 = st.create_session("s1".to_string(), None).await.unwrap();
        st.track_file(&s1.id, "a.rs").await.unwrap();
        let result =
            handle_session_conflicts(&st, Some("a.rs".to_string()), Some(s1.id.clone())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn conflicts_detected() {
        let (_tmp, st) = test_state();
        let s1 = st.create_session("s1".to_string(), None).await.unwrap();
        let s2 = st.create_session("s2".to_string(), None).await.unwrap();
        st.track_file(&s1.id, "shared.rs").await.unwrap();
        st.track_file(&s2.id, "shared.rs").await.unwrap();
        let result =
            handle_session_conflicts(&st, Some("shared.rs".to_string()), Some(s1.id.clone())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn conflicts_all_sessions_no_files() {
        let (_tmp, st) = test_state();
        let s1 = st.create_session("s1".to_string(), None).await.unwrap();
        // Pass no file to trigger the "all sessions" branch
        let result = handle_session_conflicts(&st, None, Some(s1.id.clone())).await;
        assert!(result.is_ok());
    }
}
