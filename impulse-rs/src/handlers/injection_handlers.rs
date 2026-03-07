use anyhow::Result;
use std::sync::Arc;

use crate::{injection, monty, orchestration, state};

use super::{get_session_id, parse_injection_mode, print_injection_explain, print_json};

pub async fn handle_orchestrate(
    state: &Arc<state::State>,
    task: String,
    inject_mode: Option<String>,
    inject_explain: bool,
    compute_routing: bool,
) -> Result<()> {
    let mode_override = parse_injection_mode(inject_mode.as_deref())?;
    let mut query_parts = vec![task.clone()];
    if let Some(active_id) = get_session_id(None) {
        if let Some(active_session) = state.get_session(&active_id).await? {
            query_parts.push(active_session.name);
            if !active_session.active_files.is_empty() {
                query_parts.push(active_session.active_files.join(" "));
            }
            if !active_session.recent_tools.is_empty() {
                query_parts.push(active_session.recent_tools.join(" "));
            }
        }
    }

    let config = state.config_snapshot()?;
    let injection_result = injection::run_injection(
        state.storage().base_path(),
        &config,
        injection::InjectionSurface::Orchestrate,
        mode_override,
        &query_parts,
    );

    let mut reasoning_input = task.clone();
    if injection_result.applied {
        if let Some(block) = &injection_result.injected_block {
            reasoning_input = format!("{}\n\n{}", reasoning_input, block);
        }
    }

    if compute_routing {
        let monty_config = monty::MontyConfig::default();
        let context = format!("Task: {}\nContext: {}", task, reasoning_input);

        match monty::execute_computed_routing(&context, &monty_config) {
            Ok(route) => {
                println!("Computed routing result:");
                println!("  Target: {}", route.target.as_str());
                println!("  Confidence: {:.2}", route.confidence);
                println!("  Reasoning: {}", route.reasoning);
            }
            Err(e) => {
                eprintln!("Computed routing failed: {}", e);
                let tool = orchestration::suggest_tool(&reasoning_input);
                println!("Recommended tool: {}", tool.as_str());
            }
        }
    } else {
        let tool = orchestration::suggest_tool(&reasoning_input);
        println!("Recommended tool: {}", tool.as_str());
    }

    println!("Task: {}", task);
    if inject_explain {
        print_injection_explain(&injection_result);
    }
    Ok(())
}

pub async fn handle_handoff(
    state: &Arc<state::State>,
    tool: String,
    task: String,
    session_id: Option<String>,
    notes: Option<String>,
    inject_mode: Option<String>,
    inject_explain: bool,
) -> Result<()> {
    let mode_override = parse_injection_mode(inject_mode.as_deref())?;
    let sid = get_session_id(session_id);
    let session = if let Some(id) = sid {
        state.get_session(&id).await?
    } else {
        None
    };

    let mut query_parts = vec![task.clone()];
    if let Some(n) = &notes {
        query_parts.push(n.clone());
    }
    if let Some(s) = &session {
        query_parts.push(s.name.clone());
        if !s.active_files.is_empty() {
            query_parts.push(s.active_files.join(" "));
        }
        if !s.recent_tools.is_empty() {
            query_parts.push(s.recent_tools.join(" "));
        }
    }
    let config = state.config_snapshot()?;
    let injection_result = injection::run_injection(
        state.storage().base_path(),
        &config,
        injection::InjectionSurface::Handoff,
        mode_override,
        &query_parts,
    );

    let handoff_path = orchestration::write_handoff(
        state.storage().base_path(),
        &tool,
        &task,
        notes.as_deref(),
        session.as_ref(),
    )?;
    if injection_result.applied {
        if let Some(block) = &injection_result.injected_block {
            if let Err(err) = orchestration::append_injected_context(&handoff_path, block) {
                eprintln!("Warning: failed to append injected context: {}", err);
            }
        }
    }
    println!("Wrote handoff file: {}", handoff_path.display());
    if inject_explain {
        print_injection_explain(&injection_result);
    }
    Ok(())
}

pub async fn handle_sync_context(
    state: &Arc<state::State>,
    session_id: Option<String>,
    inject_mode: Option<String>,
    inject_explain: bool,
) -> Result<()> {
    let mode_override = parse_injection_mode(inject_mode.as_deref())?;
    let sid = get_session_id(session_id);
    let session = if let Some(id) = sid {
        state.get_session(&id).await?
    } else {
        None
    };

    let mut query_parts = vec!["sync context".to_string()];
    if let Some(s) = &session {
        query_parts.push(s.name.clone());
        if !s.active_files.is_empty() {
            query_parts.push(s.active_files.join(" "));
        }
        if !s.recent_tools.is_empty() {
            query_parts.push(s.recent_tools.join(" "));
        }
    }
    let config = state.config_snapshot()?;
    let injection_result = injection::run_injection(
        state.storage().base_path(),
        &config,
        injection::InjectionSurface::SyncContext,
        mode_override,
        &query_parts,
    );

    let context_path = orchestration::sync_context(state.storage().base_path(), session.as_ref())?;
    if injection_result.applied {
        if let Some(block) = &injection_result.injected_block {
            if let Err(err) = orchestration::append_injected_context(&context_path, block) {
                eprintln!("Warning: failed to append injected context: {}", err);
            }
        }
    }
    println!("Synced context file: {}", context_path.display());
    if inject_explain {
        print_injection_explain(&injection_result);
    }
    Ok(())
}

pub fn handle_compute_injection(query: String, _limit: usize, json: bool) -> Result<()> {
    let monty_config = monty::MontyConfig::default();
    let context = format!("Query: {}", query);

    match monty::execute_injection_selection(&context, &monty_config) {
        Ok(decisions) => {
            if json {
                print_json(&decisions)?;
            } else {
                println!("Computed injection decisions:");
                for (i, decision) in decisions.iter().enumerate() {
                    println!(
                        "  {}. [{}] {} - {}",
                        i + 1,
                        decision.priority,
                        decision.context_type,
                        decision.reasoning
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Injection selection failed: {}", e);
            anyhow::bail!("Failed to compute injection: {}", e);
        }
    }
    Ok(())
}
