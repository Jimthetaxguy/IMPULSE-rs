//! Daemon-mode CLI dispatch — routes Commands through the DaemonClient IPC.
//!
//! Extracted from `run_daemon_mode()` in main.rs to keep main.rs focused on
//! argument parsing and top-level dispatch.

use anyhow::Result;
use std::path::Path;

use crate::client::DaemonClient;
use crate::daemon::{DaemonRequest, DaemonResponse};
use crate::{envelope, plugin, semantic_diff, verify, Commands};

use super::{
    capture_hook_evidence, default_session_name, get_session_id, parse_injection_mode, print_json,
    print_verification_report, read_hook_stdin_payload,
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
            handle_daemon(client, stop).await?;
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode: _,
            inject_explain: _,
        } => {
            handle_session_start(client, impulse_dir, name, platform).await?;
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
            .await?;
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
                        println!(
                            "{} - {} ({})",
                            s["id"].as_str().unwrap_or("?"),
                            s["name"].as_str().unwrap_or("?"),
                            s["status"].as_str().unwrap_or("?")
                        );
                    }
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionInfo { id } => match client.get_session(id).await {
            Ok(s) => print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::SessionConflicts { file, session_id } => {
            handle_session_conflicts(client, file, session_id).await?;
        }
        Commands::Status => match client.status().await {
            Ok(s) => print_json(&s)?,
            Err(e) => eprintln!("Error: {}", e),
        },
        Commands::Debug => match client.send(DaemonRequest::DebugSnapshot).await {
            Ok(DaemonResponse::Ok { result }) => print_json(&result)?,
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
            handle_chat(client, session_id, message, inject_mode, inject_explain).await?;
        }
        Commands::Verify => {
            let steps = verify::default_steps(&std::env::current_dir()?);
            let report = verify::run_verification(steps)?;
            print_verification_report(&report);
            if !report.success() {
                anyhow::bail!("Verification failed");
            }
        }
        Commands::Describe => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            super::describe::handle_describe(fmt)?;
        }
        Commands::Schema { command: cmd } => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            super::describe::handle_schema(&cmd, fmt)?;
        }
        Commands::PluginList { json } => {
            handle_plugin_list(client, json).await?;
        }
        Commands::PluginInvoke {
            name,
            path,
            query,
            options,
            json,
        } => {
            handle_plugin_invoke(client, name, path, query, options, json).await?;
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
        let status = client.status().await?;
        print_json(&status)?;
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
            capture_hook_evidence(
                impulse_dir,
                "session_start",
                Some(id.clone()),
                Some(n.clone()),
                Some("daemon".to_string()),
                None,
                None,
                stdin_payload,
                Some("daemon create_session".to_string()),
                1,
            )?;
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
                impulse_dir,
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
    match client
        .end_session(session_id.clone(), summary.clone())
        .await
    {
        Ok(_) => {
            capture_hook_evidence(
                impulse_dir,
                "session_end",
                Some(session_id.clone()),
                None,
                Some("daemon".to_string()),
                Some(summary),
                Some(should_verify),
                stdin_payload,
                Some(format!("Session {} ended", session_id)),
                1,
            )?;
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
                let mut all_files: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut file_to_session: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();

                for s in &sessions {
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
                print_json(&result)?;
            } else if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
                println!("{}", response);
            } else {
                print_json(&result)?;
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
                print_json(&result)?;
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
    let mut input = plugin::PluginInput::new();
    if let Some(p) = path {
        input = input.with_path(std::path::PathBuf::from(p));
    }
    if let Some(q) = query {
        input = input.with_query(q);
    }
    if let Some(opts) = options {
        let parsed: serde_json::Value =
            serde_json::from_str(&opts).unwrap_or_else(|_| serde_json::json!({"raw": opts}));
        input = input.with_options(parsed);
    }
    match client
        .send(DaemonRequest::InvokePlugin { name, input })
        .await
    {
        Ok(DaemonResponse::Ok { result }) => {
            if json {
                print_json(&result)?;
            } else if let Some(content) = result.get("content").and_then(|v| v.as_str()) {
                println!("{}", content);
            } else {
                print_json(&result)?;
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
