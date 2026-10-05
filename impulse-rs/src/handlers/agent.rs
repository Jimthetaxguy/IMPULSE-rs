use anyhow::Result;
use std::sync::Arc;

use crate::{agent, branding, state};

/// Handle the `agent-configure` command.
///
/// Applies any provided agent settings (provider, API key, model, harness,
/// auto-review, auto-coordinate) to the config store and prints the resolved
/// agent status.
pub fn handle_agent_configure(
    state: &Arc<state::State>,
    provider: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    harness: Option<String>,
    auto_review: bool,
    auto_coordinate: bool,
) -> Result<()> {
    // Checked before anything is written, so a refused command changes
    // nothing (it used to save --provider and then fail on --api-key).
    if api_key.is_some() {
        // `impulse_agent_api_key` is never written to config.json (it is
        // `skip_serializing`), so this used to report success for a key that
        // was gone when the command exited.
        anyhow::bail!(
            "--api-key is not stored; set the provider's key in the environment \
             (for example ANTHROPIC_API_KEY) or keep it with `impulse-rs credentials set`"
        );
    }
    if let Some(ref p) = provider {
        if state.set_config("impulse_agent_provider", p)? {
            println!("Set impulse_agent_provider = {}", p);
        } else {
            anyhow::bail!("Invalid provider: {p} (use: anthropic, openai, minimax)");
        }
    }
    if let Some(ref m) = model {
        let _ = state.set_config("impulse_agent_model", m)?;
        println!("Set impulse_agent_model = {}", m);
    }
    if let Some(ref h) = harness {
        if state.set_config("impulse_agent_harness", h)? {
            println!("Set impulse_agent_harness = {}", h);
        } else {
            anyhow::bail!("Invalid harness: {h} (use: claude-code, opencode)");
        }
    }
    if auto_review {
        let _ = state.set_config("impulse_agent_auto_review", "true")?;
        println!("Enabled auto-review");
    }
    if auto_coordinate {
        let _ = state.set_config("impulse_agent_auto_coordinate", "true")?;
        println!("Enabled auto-coordinate");
    }

    let agent = resolve_configured_agent(state);
    match agent {
        Some(a) => println!("\nImpulse Agent: {}", a.status_summary()),
        None => println!("\nImpulse Agent: not configured"),
    }
    Ok(())
}

/// Handle the `agent-status` command.
///
/// Resolves the current agent configuration and prints its readiness,
/// provider, model, and harness details in text or JSON format.
pub fn handle_agent_status(state: &Arc<state::State>, json: bool) -> Result<()> {
    let config = state.config_snapshot()?;

    let agent = resolve_configured_agent(state);

    if json {
        let status = serde_json::json!({
            "configured": agent.is_some(),
            "ready": agent.as_ref().map(|a| a.is_ready()).unwrap_or(false),
            "status": agent.as_ref().map(|a| a.status_summary()).unwrap_or_else(|| "not configured".to_string()),
            "provider": config.impulse_agent_provider,
            "model": config.impulse_agent_model,
            "harness": config.impulse_agent_harness,
            "auto_review": config.impulse_agent_auto_review,
            "auto_coordinate": config.impulse_agent_auto_coordinate,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        branding::print_header("Impulse Agent Status");
        match agent {
            Some(a) => {
                println!("  Status: {}", a.status_summary());
                println!("  Ready:  {}", if a.is_ready() { "yes" } else { "no" });
            }
            None => {
                println!("  Status: not configured");
                println!("\n  Configure with:");
                println!("    export ANTHROPIC_API_KEY=...; impulse-rs agent-configure --provider anthropic");
                println!("    impulse-rs agent-configure --harness claude-code");
            }
        }
        println!("  Auto-review:     {}", config.impulse_agent_auto_review);
        println!(
            "  Auto-coordinate: {}",
            config.impulse_agent_auto_coordinate
        );
    }
    Ok(())
}

/// Handle the `agent-query` command.
///
/// Sends a prompt to the configured Impulse Agent and prints the response.
/// Returns an error if the agent is not configured or not ready.
pub async fn handle_agent_query(
    state: &Arc<state::State>,
    prompt: String,
    json: bool,
) -> Result<()> {
    let mut agent = resolve_configured_agent(state).ok_or_else(|| {
        anyhow::anyhow!(
            "Impulse Agent not configured. Set ANTHROPIC_API_KEY, then run: impulse-rs agent-configure --provider anthropic"
        )
    })?;

    if !agent.is_ready() {
        anyhow::bail!(
            "Impulse Agent is configured but not ready (check API key or harness installation)"
        );
    }

    let response = agent
        .query(agent::prompts::CODE_REVIEW_SYSTEM, &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("Agent query failed: {}", e))?;

    if json {
        let result = serde_json::json!({
            "response": response,
            "agent_status": agent.status_summary(),
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{}", response);
    }
    Ok(())
}

/// Single constructor path used by CLI configure/status/query.
fn resolve_configured_agent(state: &state::State) -> Option<agent::ImpulseAgent> {
    let config = state.config_snapshot().ok()?;
    agent::resolve_from_config(
        config.impulse_agent_provider.as_deref(),
        config.impulse_agent_api_key.as_deref(),
        config.impulse_agent_model.as_deref(),
        config.impulse_agent_harness.as_deref(),
        config.impulse_agent_escalate_model.as_deref(),
    )
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

    // ── handle_agent_configure ─────────────────────────────────────────

    #[test]
    fn test_handle_agent_configure_set_provider_reflects_in_config() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            Some("anthropic".to_string()),
            None,
            None,
            None,
            false,
            false,
        );
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert_eq!(config.impulse_agent_provider.as_deref(), Some("anthropic"));
    }

    #[test]
    fn test_handle_agent_configure_set_model_reflects_in_config() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            None,
            None,
            Some("claude-3-5-sonnet".to_string()),
            None,
            false,
            false,
        );
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert_eq!(
            config.impulse_agent_model.as_deref(),
            Some("claude-3-5-sonnet")
        );
    }

    #[test]
    fn test_handle_agent_configure_set_harness_reflects_in_config() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            None,
            None,
            None,
            Some("claude-code".to_string()),
            false,
            false,
        );
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert_eq!(config.impulse_agent_harness.as_deref(), Some("claude-code"));
    }

    #[test]
    fn test_handle_agent_configure_invalid_provider_is_an_error_and_does_not_set() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            Some("invalid_provider".to_string()),
            None,
            None,
            None,
            false,
            false,
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid provider"), "{err}");
        let config = st.config_snapshot().unwrap();
        // Invalid provider should not be stored
        assert!(config.impulse_agent_provider.is_none());
    }

    /// Refutation review of 99e858a: the provider was saved before
    /// --api-key failed the command.
    #[test]
    fn test_handle_agent_configure_refused_api_key_changes_nothing() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            Some("openai".to_string()),
            Some("sk-test-not-a-real-key".to_string()),
            None,
            None,
            false,
            false,
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("--api-key is not stored"));
        assert!(st
            .config_snapshot()
            .unwrap()
            .impulse_agent_provider
            .is_none());
    }

    #[test]
    fn test_handle_agent_configure_empty_provider_clears_value() {
        let (_tmp, st) = test_state();
        // First set a valid provider
        handle_agent_configure(
            &st,
            Some("openai".to_string()),
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
        let config = st.config_snapshot().unwrap();
        assert_eq!(config.impulse_agent_provider.as_deref(), Some("openai"));

        // Then clear it with empty string
        let result =
            handle_agent_configure(&st, Some("".to_string()), None, None, None, false, false);
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert!(config.impulse_agent_provider.is_none());
    }

    #[test]
    fn test_handle_agent_configure_auto_review_flag() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(&st, None, None, None, None, true, false);
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert!(config.impulse_agent_auto_review);
    }

    #[test]
    fn test_handle_agent_configure_auto_coordinate_flag() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(&st, None, None, None, None, false, true);
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert!(config.impulse_agent_auto_coordinate);
    }

    /// Review P2: `impulse_agent_api_key` is never written to config.json,
    /// so `--api-key` reported success for a key that vanished on exit.
    #[test]
    fn test_handle_agent_configure_api_key_is_refused_with_guidance() {
        let (_tmp, st) = test_state();
        let err = handle_agent_configure(
            &st,
            None,
            Some("test-api-key-not-real".to_string()),
            None,
            None,
            false,
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not stored"), "{err}");
        assert!(!err.to_string().contains("test-api-key-not-real"), "{err}");
    }

    #[test]
    fn test_handle_agent_configure_multiple_fields_at_once() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(
            &st,
            Some("anthropic".to_string()),
            None,
            Some("claude-3-opus".to_string()),
            None,
            true,
            true,
        );
        assert!(result.is_ok());
        let config = st.config_snapshot().unwrap();
        assert_eq!(config.impulse_agent_provider.as_deref(), Some("anthropic"));
        assert_eq!(config.impulse_agent_model.as_deref(), Some("claude-3-opus"));
        assert!(config.impulse_agent_auto_review);
        assert!(config.impulse_agent_auto_coordinate);
    }

    #[test]
    fn test_handle_agent_configure_no_args_succeeds() {
        let (_tmp, st) = test_state();
        let result = handle_agent_configure(&st, None, None, None, None, false, false);
        assert!(result.is_ok());
    }

    // ── handle_agent_status ────────────────────────────────────────────

    #[test]
    fn test_handle_agent_status_not_configured() {
        let (_tmp, st) = test_state();
        let result = handle_agent_status(&st, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_agent_status_json_not_configured() {
        let (_tmp, st) = test_state();
        let result = handle_agent_status(&st, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_agent_status_json_after_configure() {
        let (_tmp, st) = test_state();
        // Configure a harness so agent resolves
        handle_agent_configure(
            &st,
            None,
            None,
            None,
            Some("claude-code".to_string()),
            false,
            false,
        )
        .unwrap();
        let result = handle_agent_status(&st, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_agent_status_text_after_configure() {
        let (_tmp, st) = test_state();
        handle_agent_configure(
            &st,
            None,
            None,
            None,
            Some("claude-code".to_string()),
            false,
            false,
        )
        .unwrap();
        let result = handle_agent_status(&st, false);
        assert!(result.is_ok());
    }

    // ── handle_agent_query ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_agent_query_not_configured_returns_error() {
        let (_tmp, st) = test_state();
        let result = handle_agent_query(&st, "test prompt".to_string(), false).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not configured"),
            "Error should mention 'not configured', got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_handle_agent_query_not_configured_json_returns_error() {
        let (_tmp, st) = test_state();
        let result = handle_agent_query(&st, "test prompt".to_string(), true).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_configured_agent_applies_escalate_model_from_config() {
        let (_tmp, st) = test_state();
        assert!(st
            .set_config("impulse_agent_provider", "anthropic")
            .unwrap());
        assert!(st.set_config("impulse_agent_api_key", "test-key").unwrap());
        assert!(st
            .set_config("impulse_agent_escalate_model", "claude-opus-4-6")
            .unwrap());
        let agent = resolve_configured_agent(&st).expect("CLI path should resolve an agent");
        assert_eq!(agent.escalate_model(), Some("claude-opus-4-6"));
    }

    #[test]
    fn test_resolve_configured_agent_without_escalate_key_is_none() {
        let (_tmp, st) = test_state();
        assert!(st
            .set_config("impulse_agent_provider", "anthropic")
            .unwrap());
        assert!(st.set_config("impulse_agent_api_key", "test-key").unwrap());
        let agent = resolve_configured_agent(&st).expect("CLI path should resolve an agent");
        assert!(agent.escalate_model().is_none());
    }

    #[tokio::test]
    async fn test_handle_agent_query_configured_but_not_ready_returns_error() {
        let (_tmp, st) = test_state();
        // Configure with provider but no API key — agent exists but isn't ready
        handle_agent_configure(
            &st,
            Some("anthropic".to_string()),
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
        let result = handle_agent_query(&st, "test prompt".to_string(), false).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not ready") || err_msg.contains("not configured"),
            "Error should mention readiness issue, got: {}",
            err_msg
        );
    }
}
