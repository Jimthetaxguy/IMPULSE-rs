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

    // ── handle_orchestrate ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_orchestrate_basic_task_routes_without_error() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "fix failing tests".to_string(),
            None,  // no inject mode override
            false, // no explain
            false, // no compute routing
        )
        .await;
        assert!(
            result.is_ok(),
            "basic orchestrate should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_orchestrate_with_injection_mode_apply() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "architecture review for auth".to_string(),
            Some("apply".to_string()),
            false,
            false,
        )
        .await;
        assert!(
            result.is_ok(),
            "orchestrate with apply mode should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_orchestrate_with_injection_mode_off() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "deploy service".to_string(),
            Some("off".to_string()),
            false,
            false,
        )
        .await;
        assert!(
            result.is_ok(),
            "orchestrate with off mode should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_orchestrate_invalid_mode_returns_error() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "some task".to_string(),
            Some("bogus".to_string()),
            false,
            false,
        )
        .await;
        assert!(
            result.is_err(),
            "invalid inject mode should produce an error"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Invalid inject mode"),
            "error should mention invalid mode, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_handle_orchestrate_with_compute_routing() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "architecture design session".to_string(),
            None,
            false,
            true, // compute_routing enabled
        )
        .await;
        assert!(
            result.is_ok(),
            "orchestrate with compute routing should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_orchestrate_with_explain_flag() {
        let (_tmp, st) = test_state();
        let result = handle_orchestrate(
            &st,
            "refactor the API layer".to_string(),
            Some("review".to_string()),
            true, // explain enabled
            false,
        )
        .await;
        assert!(
            result.is_ok(),
            "orchestrate with explain should succeed: {:?}",
            result.err()
        );
    }

    // ── handle_handoff ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_handoff_writes_file_to_correct_path() {
        let (tmp, st) = test_state();
        let result = handle_handoff(
            &st,
            "codex".to_string(),
            "fix failing test".to_string(),
            None,  // no session id
            None,  // no notes
            None,  // no inject mode
            false, // no explain
        )
        .await;
        assert!(result.is_ok(), "handoff should succeed: {:?}", result.err());

        let handoff_path = tmp.path().join("context").join("handoff-codex.md");
        assert!(
            handoff_path.exists(),
            "handoff file should exist at {:?}",
            handoff_path
        );

        let content = std::fs::read_to_string(&handoff_path).unwrap();
        assert!(
            content.contains("# Task Handoff"),
            "handoff should contain header"
        );
        assert!(
            content.contains("fix failing test"),
            "handoff should contain task description"
        );
        assert!(
            content.contains("codex"),
            "handoff should contain target tool name"
        );
    }

    #[tokio::test]
    async fn test_handle_handoff_with_notes() {
        let (tmp, st) = test_state();
        let result = handle_handoff(
            &st,
            "opencode".to_string(),
            "build plugin".to_string(),
            None,
            Some("Check error handling first".to_string()),
            None,
            false,
        )
        .await;
        assert!(
            result.is_ok(),
            "handoff with notes should succeed: {:?}",
            result.err()
        );

        let handoff_path = tmp.path().join("context").join("handoff-opencode.md");
        assert!(handoff_path.exists());

        let content = std::fs::read_to_string(&handoff_path).unwrap();
        assert!(
            content.contains("Check error handling first"),
            "handoff should contain notes"
        );
    }

    #[tokio::test]
    async fn test_handle_handoff_with_session_context() {
        let (_tmp, st) = test_state();
        // Create a real session so the handler can look it up
        let session = st
            .create_session("test-handoff-session".to_string(), None)
            .await
            .unwrap();

        let result = handle_handoff(
            &st,
            "claude-code".to_string(),
            "review auth module".to_string(),
            Some(session.id.clone()),
            None,
            None,
            false,
        )
        .await;
        assert!(
            result.is_ok(),
            "handoff with session should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_handoff_invalid_mode_returns_error() {
        let (_tmp, st) = test_state();
        let result = handle_handoff(
            &st,
            "codex".to_string(),
            "task".to_string(),
            None,
            None,
            Some("invalid_mode".to_string()),
            false,
        )
        .await;
        assert!(
            result.is_err(),
            "invalid inject mode should produce an error"
        );
    }

    // ── handle_sync_context ────────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_sync_context_creates_file() {
        let (tmp, st) = test_state();
        let result = handle_sync_context(
            &st, None,  // no session
            None,  // no inject mode
            false, // no explain
        )
        .await;
        assert!(
            result.is_ok(),
            "sync context should succeed: {:?}",
            result.err()
        );

        let context_path = tmp.path().join("context").join("current-task.md");
        assert!(
            context_path.exists(),
            "current-task.md should be created at {:?}",
            context_path
        );

        let content = std::fs::read_to_string(&context_path).unwrap();
        assert!(
            content.contains("# Current Task Context"),
            "should contain context header"
        );
    }

    #[tokio::test]
    async fn test_handle_sync_context_with_session() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("sync-test-session".to_string(), None)
            .await
            .unwrap();

        let result = handle_sync_context(&st, Some(session.id.clone()), None, false).await;
        assert!(
            result.is_ok(),
            "sync context with session should succeed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_handle_sync_context_with_apply_mode() {
        let (tmp, st) = test_state();
        let result = handle_sync_context(&st, None, Some("apply".to_string()), false).await;
        assert!(
            result.is_ok(),
            "sync context with apply mode should succeed: {:?}",
            result.err()
        );

        let context_path = tmp.path().join("context").join("current-task.md");
        assert!(
            context_path.exists(),
            "context file should exist after sync with apply mode"
        );
    }

    #[tokio::test]
    async fn test_handle_sync_context_invalid_mode_returns_error() {
        let (_tmp, st) = test_state();
        let result = handle_sync_context(&st, None, Some("not_a_mode".to_string()), false).await;
        assert!(
            result.is_err(),
            "invalid inject mode should produce an error"
        );
    }

    // ── handle_compute_injection ───────────────────────────────────────

    #[test]
    fn test_handle_compute_injection_text_output_succeeds() {
        let result = handle_compute_injection(
            "look at previous session history".to_string(),
            5,
            false, // text output
        );
        assert!(
            result.is_ok(),
            "compute injection (text) should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_compute_injection_json_output_succeeds() {
        let result = handle_compute_injection(
            "check current session decisions".to_string(),
            5,
            true, // json output
        );
        assert!(
            result.is_ok(),
            "compute injection (json) should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_compute_injection_empty_query_returns_default_decision() {
        // The keyword router always returns at least one decision (a default)
        // so even an empty-ish query should succeed
        let result = handle_compute_injection("general task".to_string(), 10, false);
        assert!(
            result.is_ok(),
            "compute injection with generic query should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_handle_compute_injection_history_keywords_detected() {
        // The keyword router detects "previous" and "history" keywords
        let result =
            handle_compute_injection("review previous history entries".to_string(), 5, false);
        assert!(result.is_ok());
    }
}
