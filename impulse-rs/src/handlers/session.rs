use anyhow::Result;
use std::sync::Arc;

use crate::{guardrail, injection, semantic_diff, state, validate, verify};

use super::{
    capture_hook_evidence, default_session_name, get_session_id, hook_session_start_banner,
    parse_platform, persist_claude_env_var, preview_block, print_verification_report,
    read_hook_stdin_payload,
};

pub async fn handle_session_start(
    state: &Arc<state::State>,
    name: Option<String>,
    platform: Option<String>,
    inject_mode: Option<String>,
    inject_explain: bool,
) -> Result<()> {
    let stdin_payload = read_hook_stdin_payload();
    let name = name.unwrap_or_else(default_session_name);
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

    if inject_explain {
        let _ = serde_json::to_writer_pretty(std::io::stdout(), &injection_result.explain);
        println!();
    }

    let mut output_lines = 0usize;
    if let Some(sentinel) = hook_session_start_banner() {
        println!("{}", sentinel);
        println!();
        output_lines += sentinel.lines().count() + 1;
    }

    if let Some(block) = injection_result.injected_block.clone() {
        let hook_mode = std::env::var("CLAUDE_ENV_FILE").is_ok();
        if hook_mode {
            println!("{}", block);
            output_lines += block.lines().count();
        } else {
            println!("{}\n\n{}", session.id, block);
            output_lines += block.lines().count() + 2;
        }
        capture_hook_evidence(
            state.storage().base_path(),
            "session_start",
            Some(session.id.clone()),
            Some(session.name.clone()),
            session.platform.map(|p| p.as_str().to_string()),
            None,
            None,
            stdin_payload,
            Some(preview_block(&block, 400)),
            output_lines,
        )?;
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
        capture_hook_evidence(
            state.storage().base_path(),
            "session_start",
            Some(session.id.clone()),
            Some(session.name.clone()),
            session.platform.map(|p| p.as_str().to_string()),
            None,
            None,
            stdin_payload,
            Some("no injection block".to_string()),
            output_lines,
        )?;
    }
    Ok(())
}

pub async fn handle_session_end(
    state: &Arc<state::State>,
    session_id: String,
    summary: String,
    should_verify: bool,
    sem_diff_base: Option<String>,
) -> Result<()> {
    validate::validate_session_id(&session_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    validate::reject_control_chars(&summary, "summary")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

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
    match state.end_session(&session_id, summary.clone()).await {
        Ok(Some(_)) => {
            capture_hook_evidence(
                state.storage().base_path(),
                "session_end",
                Some(session_id.clone()),
                None,
                None,
                Some(summary),
                Some(should_verify),
                stdin_payload,
                Some(format!("Session {} ended", session_id)),
                1,
            )?;
            println!("Session {} ended", session_id)
        }
        Ok(None) => {
            capture_hook_evidence(
                state.storage().base_path(),
                "session_end_missing",
                Some(session_id.clone()),
                None,
                None,
                Some(summary),
                Some(should_verify),
                stdin_payload,
                Some(format!("Session not found: {}", session_id)),
                1,
            )?;
            println!("Session not found: {}", session_id)
        }
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

pub async fn handle_track_write(
    state: &Arc<state::State>,
    file: String,
    session_id: Option<String>,
) -> Result<()> {
    validate::validate_file_arg(&file)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if let Some(sid) = get_session_id(session_id) {
        match state.track_file(&sid, &file).await {
            Ok(_) => println!("Tracked: {}", file),
            Err(e) => eprintln!("Error: {}", e),
        }
        if let Ok(config) = state.config_snapshot() {
            if config.guardrails.enabled {
                if let Ok(results) = guardrail::evaluate_action(&file, "any", &config.guardrails) {
                    for result in &results {
                        match result.action {
                            guardrail::GuardAction::Warn => {
                                eprintln!("{}", result.format_human());
                            }
                            guardrail::GuardAction::Log => {
                                let _ = state
                                    .add_tag(&sid, &format!("guard:{}", result.rule_id))
                                    .await;
                            }
                            guardrail::GuardAction::Block => {}
                        }
                    }
                }
            }
        }
    } else {
        eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
    }
    Ok(())
}

pub async fn handle_track_tool(
    state: &Arc<state::State>,
    tool: String,
    session_id: Option<String>,
) -> Result<()> {
    validate::reject_control_chars(&tool, "tool")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if let Some(sid) = get_session_id(session_id) {
        match state.track_tool(&sid, &tool).await {
            Ok(_) => println!("Tracked: {}", tool),
            Err(e) => eprintln!("Error: {}", e),
        }
        if let Ok(config) = state.config_snapshot() {
            if config.guardrails.enabled {
                if let Ok(results) = guardrail::evaluate_action(&tool, "any", &config.guardrails) {
                    for result in &results {
                        match result.action {
                            guardrail::GuardAction::Warn => {
                                eprintln!("{}", result.format_human());
                            }
                            guardrail::GuardAction::Log => {
                                let _ = state
                                    .add_tag(&sid, &format!("guard:{}", result.rule_id))
                                    .await;
                            }
                            guardrail::GuardAction::Block => {}
                        }
                    }
                }
            }
        }
    } else {
        eprintln!("Error: No session_id. Use --session-id or IMPULSE_SESSION_ID");
    }
    Ok(())
}

pub async fn handle_list_sessions(state: &Arc<state::State>) -> Result<()> {
    let sessions = state.list_sessions().await?;
    if sessions.is_empty() {
        println!("No active sessions");
    } else {
        for s in sessions {
            println!("{} - {} ({:?})", s.id, s.name, s.status);
        }
    }
    Ok(())
}

pub async fn handle_session_info(state: &Arc<state::State>, id: String) -> Result<()> {
    match state.get_session(&id).await {
        Ok(Some(s)) => {
            println!("Session: {}", s.name);
            println!("ID: {}", s.id);
            println!("Status: {:?}", s.status);
            println!("Platform: {:?}", s.platform);
            println!("Working Dir: {}", s.working_directory);
            println!("Created: {}", s.created_at);
            println!("Files: {:?}", s.active_files);
            println!("Tools: {:?}", s.recent_tools);
        }
        Ok(None) => println!("Session not found: {}", id),
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
