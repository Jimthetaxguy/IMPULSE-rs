use anyhow::Result;
use std::sync::Arc;

use crate::{agent, branding, state};

pub fn handle_agent_configure(
    state: &Arc<state::State>,
    provider: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    harness: Option<String>,
    auto_review: bool,
    auto_coordinate: bool,
) -> Result<()> {
    if let Some(ref p) = provider {
        if state.set_config("impulse_agent_provider", p)? {
            println!("Set impulse_agent_provider = {}", p);
        } else {
            eprintln!("Invalid provider: {} (use: anthropic, openai, minimax)", p);
        }
    }
    if let Some(ref key) = api_key {
        let _ = state.set_config("impulse_agent_api_key", key)?;
        println!("Set impulse_agent_api_key = ***");
    }
    if let Some(ref m) = model {
        let _ = state.set_config("impulse_agent_model", m)?;
        println!("Set impulse_agent_model = {}", m);
    }
    if let Some(ref h) = harness {
        if state.set_config("impulse_agent_harness", h)? {
            println!("Set impulse_agent_harness = {}", h);
        } else {
            eprintln!("Invalid harness: {} (use: claude-code, opencode)", h);
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

    let config = state.config_snapshot()?;
    let agent = agent::resolve_from_config(
        config.impulse_agent_provider.as_deref(),
        config.impulse_agent_api_key.as_deref(),
        config.impulse_agent_model.as_deref(),
        config.impulse_agent_harness.as_deref(),
    );
    match agent {
        Some(a) => println!("\nImpulse Agent: {}", a.status_summary()),
        None => println!("\nImpulse Agent: not configured"),
    }
    Ok(())
}

pub fn handle_agent_status(state: &Arc<state::State>, json: bool) -> Result<()> {
    let config = state.config_snapshot()?;

    let agent = agent::resolve_from_config(
        config.impulse_agent_provider.as_deref(),
        config.impulse_agent_api_key.as_deref(),
        config.impulse_agent_model.as_deref(),
        config.impulse_agent_harness.as_deref(),
    );

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
                println!("    impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY");
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

pub async fn handle_agent_query(
    state: &Arc<state::State>,
    prompt: String,
    json: bool,
) -> Result<()> {
    let config = state.config_snapshot()?;

    let mut agent = agent::resolve_from_config(
        config.impulse_agent_provider.as_deref(),
        config.impulse_agent_api_key.as_deref(),
        config.impulse_agent_model.as_deref(),
        config.impulse_agent_harness.as_deref(),
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "Impulse Agent not configured. Run: impulse-rs agent-configure --provider anthropic --api-key YOUR_KEY"
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
