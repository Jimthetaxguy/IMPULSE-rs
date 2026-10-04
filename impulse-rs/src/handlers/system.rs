use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::{mcp, monty, state, stewardship, tooling};

use super::{
    build_claude_hook_config, build_opencode_hook_config, build_tool_context, build_tool_registry,
    get_session_id, print_json, refresh_capabilities_manifest, write_hook_validation_kit,
};

pub fn handle_system() -> Result<()> {
    use crate::tools::system::{get_impulse_env_vars, SystemInfo};

    let info = SystemInfo::collect();

    println!("=== System Information ===");
    println!("OS: {}", info.os);
    println!("Architecture: {}", info.arch);
    println!("Home Directory: {:?}", info.home_dir);
    println!("Current Directory: {}", info.current_dir);
    println!("Python Available: {}", info.python_available);
    if let Some(version) = info.python_version {
        println!("Python Version: {}", version.trim());
    }

    let impulse_vars = get_impulse_env_vars();
    if !impulse_vars.is_empty() {
        println!("\n=== Impulse Environment Variables ===");
        for var in impulse_vars {
            println!("{}: {}", var.key, var.value);
        }
    }
    Ok(())
}

pub fn handle_calc(expression: String) -> Result<()> {
    use crate::tools::python;

    match python::calculate(&expression) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            eprintln!("Calculation error: {}", e);
            return Err(anyhow::anyhow!("Calculation failed"));
        }
    }
    Ok(())
}

pub fn handle_exec(code: String) -> Result<()> {
    use crate::tools::python;

    match python::execute_python(&code) {
        Ok(result) => {
            if let Some(error) = &result.error {
                eprintln!("Error:\n{}", error);
            }
            if !result.output.is_empty() {
                print!("{}", result.output);
            }
            exec_outcome(&result)
        }
        Err(e) => {
            eprintln!("Execution error: {}", e);
            Err(anyhow::anyhow!("Execution failed"))
        }
    }
}

/// Whether a sandbox run counts as a successful `exec` command.
///
/// The sandbox reports every program failure as a result carrying a fault
/// class, not as an `Err`, so a caller can tell a timeout from a syntax error.
/// On the command line that still has to end in a non-zero exit. A script or
/// an agent that checks only the exit status would otherwise accept a program
/// that timed out, raised, or never parsed. Output printed before the failure
/// has already been written by the caller.
fn exec_outcome(result: &crate::tools::python::PythonResult) -> Result<()> {
    if let Some(fault) = result.fault {
        anyhow::bail!("Execution failed ({fault})");
    }
    if result.exit_code != 0 {
        anyhow::bail!("Execution failed (exit code {})", result.exit_code);
    }
    Ok(())
}

pub fn handle_extract(content: String, session_id: Option<String>, json: bool) -> Result<()> {
    let sid = session_id.unwrap_or_else(|| "unknown".to_string());
    let mut contrib = monty::kdb_extraction::KdbContribution::new(sid.clone());

    let lower = content.to_lowercase();
    if lower.contains("finding") || lower.contains("found") {
        contrib.add_finding(
            content.clone(),
            if lower.contains("critical") || lower.contains("urgent") {
                "high".to_string()
            } else {
                "medium".to_string()
            },
        );
    }
    if lower.contains("risk") || lower.contains("concern") {
        contrib.add_risk(content.clone(), "medium".to_string(), None);
    }

    if json {
        print_json(&contrib)?;
    } else {
        println!("Extracted from session: {}", sid);
        if !contrib.findings.is_empty() {
            println!("\nFindings ({}):", contrib.findings.len());
            for f in &contrib.findings {
                println!("  - [{}] {}", f.severity, f.content);
            }
        }
        if !contrib.risks.is_empty() {
            println!("\nRisks ({}):", contrib.risks.len());
            for r in &contrib.risks {
                println!("  - [{}] {}", r.severity, r.description);
            }
        }
        if contrib.findings.is_empty() && contrib.risks.is_empty() {
            println!("No structured findings extracted.");
        }
    }
    Ok(())
}

pub fn handle_swarm(agent_a: String, agent_b: String, threshold: f64, json: bool) -> Result<()> {
    let patterns = monty::swarm_coordination::detect_patterns(&agent_a, &agent_b, threshold);

    if json {
        print_json(&patterns)?;
    } else {
        println!("SWARM Pattern Detection:");
        println!("  Agent A: {}", agent_a);
        println!("  Agent B: {}", agent_b);
        println!("  Threshold: {}", threshold);
        if patterns.is_empty() {
            println!("\nNo patterns detected at threshold {}", threshold);
        } else {
            println!("\nDetected {} pattern(s):", patterns.len());
            for p in &patterns {
                println!("  - {:?} (confidence: {:.2})", p.pattern_type, p.confidence);
            }
        }
    }
    Ok(())
}

pub fn handle_hooks(state: &Arc<state::State>, platform: String) -> Result<()> {
    install_hook_templates(Path::new("."), &platform)?;
    println!("\nImpulse path: {}", state.storage().base_path().display());
    Ok(())
}

/// Writes the hook templates under `project_root` (`.claude/hooks/hooks.json`,
/// `.opencode/impulse.json`) for `platform`. Taking the root explicitly keeps
/// tests out of the source tree they run in.
pub(crate) fn install_hook_templates(project_root: &Path, platform: &str) -> Result<()> {
    let claude = platform == "claude-code" || platform == "all";
    let opencode = platform == "opencode" || platform == "all";
    if !claude && !opencode {
        anyhow::bail!("unknown hooks platform '{platform}' (use claude-code, opencode, or all)");
    }

    if claude {
        println!("Setting up Claude Code hooks...");
        let hooks_dir = project_root.join(".claude").join("hooks");
        std::fs::create_dir_all(&hooks_dir).context("Failed to create .claude/hooks")?;
        let hook_config = build_claude_hook_config();
        let hook_json = serde_json::to_string_pretty(&hook_config)
            .context("Failed to serialize the Claude hook template")?;
        let hook_path = hooks_dir.join("hooks.json");
        stewardship::atomic_write_file(&hook_path, hook_json.as_bytes())
            .context("Failed to write .claude/hooks/hooks.json")?;
        println!("  \u{2713} Wrote the hook template to .claude/hooks/hooks.json");
        // Claude Code reads hooks from its settings files, not this path, so
        // say exactly what makes the guard enforce anything.
        let guard_entry =
            serde_json::json!({ "hooks": { "PreToolUse": hook_config["hooks"]["PreToolUse"] } });
        println!(
            "\nClaude Code does not load .claude/hooks/hooks.json; it is a template. To enforce \
             the Impulse guard, add this to .claude/settings.local.json (merge with any hooks \
             already there):\n{}\nThe template's session-tracking entries need your session wiring; \
             see `impulse-rs validate-hooks claude-code`.",
            serde_json::to_string_pretty(&guard_entry)
                .context("Failed to serialize the guard hook entry")?
        );
    }

    if opencode {
        println!("\nSetting up OpenCode integration...");
        let opencode_dir = project_root.join(".opencode");
        std::fs::create_dir_all(&opencode_dir).context("Failed to create .opencode")?;
        let opencode_config = build_opencode_hook_config();
        let opencode_json = serde_json::to_string_pretty(&opencode_config)
            .context("Failed to serialize the OpenCode config")?;
        let opencode_path = opencode_dir.join("impulse.json");
        stewardship::atomic_write_file(&opencode_path, opencode_json.as_bytes())
            .context("Failed to write .opencode/impulse.json")?;
        println!("  \u{2713} Created .opencode/impulse.json");
    }
    Ok(())
}

pub fn handle_validate_hooks(platform: String) -> Result<()> {
    let written = write_hook_validation_kit(&platform)?;
    println!("Generated hook validation kit for {}:", platform);
    for path in written {
        println!("  - {}", path.display());
    }
    println!(
        "\nNext steps:\n  1. Copy .impulse/validation/{}/settings.local.json into your Claude local settings.\n  2. Run a real Claude session and inspect .impulse/validation/runtime/hook-events.jsonl for captured SessionStart/SessionEnd evidence.\n  3. Record the outcome in .impulse/validation/{}/evidence.md and compare it against .impulse/HISTORY.jsonl / .impulse/GENOME.md",
        platform, platform
    );
    Ok(())
}

pub async fn handle_chat(
    state: &Arc<state::State>,
    message: &str,
    inject_mode: Option<&str>,
    inject_explain: bool,
) -> Result<serde_json::Value> {
    use crate::injection::{engine::run_injection, types::InjectionMode, types::InjectionSurface};
    use crate::llm_backends::anthropic::AnthropicProvider;
    use crate::llm_backends::{ChatRequest, LlmProvider, Message, Role};

    // Validate inject_mode first (pure input check, no side effects)
    let mode_override = inject_mode.and_then(InjectionMode::parse);
    if inject_mode.is_some() && mode_override.is_none() {
        anyhow::bail!("Invalid inject_mode. Use off|review|apply");
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("CLAUDE_API_KEY"))
        .unwrap_or_default();

    if api_key.is_empty() {
        anyhow::bail!("ANTHROPIC_API_KEY or CLAUDE_API_KEY not set");
    }

    let config = state.config_snapshot()?;
    let mut context_prompt = message.to_string();

    let injection_result = run_injection(
        state.storage().base_path(),
        &config,
        InjectionSurface::DaemonChat,
        mode_override,
        &[message.to_string()],
    );

    if injection_result.applied {
        if let Some(block) = &injection_result.injected_block {
            context_prompt = format!("{}\n\n{}", block, context_prompt);
        }
    }

    let provider = AnthropicProvider::new(api_key);
    let configured =
        std::env::var("IMPULSE_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
    let step_ctx = crate::agent::step_model::HarnessStepContext::ion_api(configured.clone());
    let model = crate::agent::step_model::resolve_step_model(&step_ctx, &configured, None);
    let request = ChatRequest {
        model,
        messages: vec![Message::text(Role::User, context_prompt)],
        temperature: 0.7,
        max_tokens: Some(4096),
        tools: Vec::new(),
    };

    let response = provider
        .chat(request)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let injection_json = if inject_explain {
        serde_json::to_value(&injection_result)
            .unwrap_or_else(|_| serde_json::json!({"status": "serialization_error"}))
    } else {
        serde_json::json!({
            "applied": injection_result.applied,
        })
    };

    Ok(serde_json::json!({
        "response": response.content,
        "model": response.model,
        "injection": injection_json,
    }))
}

/// Handle `chat` command in direct mode with display output.
pub async fn handle_chat_and_display(
    state: &Arc<state::State>,
    message: &str,
    inject_mode: Option<&str>,
    inject_explain: bool,
) -> Result<()> {
    let result = handle_chat(state, message, inject_mode, inject_explain).await?;
    if inject_explain {
        super::print_json(&result)?;
    } else if let Some(response) = result.get("response").and_then(|v| v.as_str()) {
        println!("{}", response);
    } else {
        super::print_json(&result)?;
    }
    Ok(())
}

pub async fn handle_docs(
    state: &Arc<state::State>,
    cli_verbose: bool,
    subcommand: String,
    provider: Option<String>,
    verbose: bool,
    force: bool,
) -> Result<()> {
    use crate::docs::{cache, fetch, models as model_mgr};

    let cache = cache::create_cache(state.storage().base_path())?;

    match subcommand.as_str() {
        "fetch" | "update" => {
            println!("Fetching latest model information...");
            let openai_key = std::env::var("OPENAI_API_KEY").ok();
            let models = fetch::fetch_all_models(openai_key.as_deref()).await?;
            let providers = crate::docs::known_providers();

            cache.save_models(&models)?;
            cache.save_providers(&providers)?;

            let metadata = cache::CacheMetadata {
                last_updated: std::time::SystemTime::now(),
                model_count: models.len(),
                provider_count: providers.len(),
                source: "api".to_string(),
            };
            cache.save_metadata(&metadata)?;

            println!(
                "\u{2713} Fetched {} models from {} providers",
                models.len(),
                providers.len()
            );
        }
        "list" | "ls" => {
            let models = if force {
                println!("Fetching latest models...");
                let openai_key = std::env::var("OPENAI_API_KEY").ok();
                fetch::fetch_all_models(openai_key.as_deref()).await?
            } else {
                cache.load_models().unwrap_or_else(|_| {
                    println!("No cached models. Use --force to fetch latest.");
                    Vec::new()
                })
            };

            let filter = model_mgr::ModelFilter {
                provider: provider.clone(),
                ..Default::default()
            };

            let filtered = filter.apply(&models);
            println!(
                "{}",
                model_mgr::format_models(&filtered, verbose || cli_verbose)
            );
        }
        "providers" => {
            let providers = crate::docs::known_providers();
            println!("Available Providers:\n");
            for p in &providers {
                println!(
                    "{} ({})\n  API: {}\n  Docs: {}\n",
                    p.name, p.id, p.api_url, p.docs_url
                );
            }
        }
        "status" => {
            let metadata = cache.load_metadata()?;
            let age = cache.age_seconds().unwrap_or(0);
            println!("Cache Status:");
            println!("  Last updated: {} seconds ago", age);
            println!("  Models cached: {}", metadata.model_count);
            println!("  Providers cached: {}", metadata.provider_count);
            println!("  Source: {}", metadata.source);

            if cache.is_stale(std::time::Duration::from_secs(86400)) {
                println!("  Status: STALE (older than 24 hours)");
            } else {
                println!("  Status: Fresh");
            }
        }
        _ => {
            eprintln!(
                "Unknown docs subcommand: {}. Use: fetch, list, providers, status",
                subcommand
            );
        }
    }
    Ok(())
}

pub(crate) async fn handle_mcp(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    subcommand: crate::McpCommands,
) -> Result<()> {
    match subcommand {
        crate::McpCommands::Serve { transport, port } => {
            let config = state.config_snapshot()?;
            let registry = Arc::new(build_tool_registry(impulse_dir, &config)?);
            let tool_context = build_tool_context(
                impulse_dir,
                &config,
                tooling::ExecutionOrigin::Mcp,
                false,
                get_session_id(None),
            );
            let _ = refresh_capabilities_manifest(state.storage().base_path(), registry.as_ref())?;
            let transport = match transport.as_str() {
                "stdio" => mcp::server::McpTransport::Stdio,
                "tcp" => mcp::server::McpTransport::Tcp(port.unwrap_or(8765)),
                _ => anyhow::bail!("Unknown MCP transport: {} (use: stdio or tcp)", transport),
            };
            mcp::McpServer::new(registry, tool_context)
                .serve(transport)
                .await?;
        }
    }
    Ok(())
}

pub fn handle_tools(
    cli_verbose: bool,
    subcommand: String,
    tool: Vec<String>,
    dry_run: bool,
) -> Result<()> {
    use crate::tools::{init, list, update};

    let tool_ids = if tool.is_empty() { None } else { Some(tool) };

    match subcommand.as_str() {
        "list" | "ls" => {
            let _ = list::list_tools(cli_verbose)?;
        }
        "init" | "install" => {
            let results = init::init_tools(tool_ids, dry_run)?;
            for (id, status) in &results {
                println!("{}: {}", id, status);
            }
        }
        "update" | "upgrade" => {
            let results = update::update_tools(tool_ids, dry_run)?;
            for (id, success, msg) in &results {
                let status = if *success { "\u{2713}" } else { "\u{2717}" };
                println!("{} {}: {}", status, id, msg);
            }
        }
        "check" => {
            let tools = update::check_updates()?;
            if tools.is_empty() {
                println!("No tools installed");
            } else {
                println!("{:<20} {:<15} Status", "Tool", "Version");
                println!("{:-<20} {:-<-15} ", "", "");
                for (id, version, up_to_date) in tools {
                    let status = if up_to_date {
                        "up to date"
                    } else {
                        "update available"
                    };
                    println!("{:<20} {:<15} {}", id, version, status);
                }
            }
        }
        _ => {
            anyhow::bail!(
                "Unknown tools subcommand: {}. Use: list, init, update, check",
                subcommand
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    fn test_state() -> (tempfile::TempDir, Arc<state::State>) {
        let dir = tempfile::TempDir::new().unwrap();
        let st = state::State::new(dir.path().to_path_buf()).unwrap();
        (dir, Arc::new(st))
    }

    #[tokio::test]
    async fn test_chat_rejects_invalid_inject_mode() {
        let (_dir, st) = test_state();
        // inject_mode validation happens before API call, so no key needed
        let result = handle_chat(&st, "hello", Some("invalid_mode"), false).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("inject_mode"),
            "Expected inject_mode error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_chat_valid_modes_accepted() {
        // "off", "review", "apply" are valid inject modes — they won't fail on mode validation
        // (they'll fail later at the API call stage, but mode parsing succeeds)
        let (_dir, st) = test_state();
        for mode in &["off", "review", "apply"] {
            let result = handle_chat(&st, "hello", Some(mode), false).await;
            // Should NOT fail with inject_mode error — may fail with API key or network
            if let Err(e) = &result {
                assert!(
                    !e.to_string().contains("inject_mode"),
                    "Valid mode '{}' rejected: {}",
                    mode,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;

    #[test]
    fn test_handle_exec_completed_program_returns_ok() {
        assert!(handle_exec("print('hello')".to_string()).is_ok());
    }

    #[test]
    fn test_handle_exec_runtime_fault_returns_err_naming_the_fault() {
        let error = handle_exec("1 / 0".to_string())
            .expect_err("a program that raised did not run to completion");
        assert!(
            format!("{error}").contains("runtime"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_handle_exec_syntax_fault_returns_err_naming_the_fault() {
        let error = handle_exec("def (".to_string())
            .expect_err("a program that does not parse did not run to completion");
        assert!(
            format!("{error}").contains("syntax"),
            "unexpected error: {error}"
        );
    }

    fn sandbox_result(
        fault: Option<&'static str>,
        exit_code: i32,
    ) -> crate::tools::python::PythonResult {
        crate::tools::python::PythonResult {
            output: String::new(),
            error: fault.map(|class| format!("{class} happened")),
            exit_code,
            fault,
        }
    }

    #[test]
    fn test_exec_outcome_completed_run_returns_ok() {
        assert!(exec_outcome(&sandbox_result(None, 0)).is_ok());
    }

    /// The case from the review: a program that never terminates comes back
    /// as a timeout fault, which used to exit 0. Built by hand so the test does
    /// not spend the five-second wall-clock budget.
    #[test]
    fn test_exec_outcome_timeout_fault_returns_err_naming_the_fault() {
        let timed_out = sandbox_result(Some(crate::tools::python::fault::TIMEOUT), 1);
        let error = exec_outcome(&timed_out).expect_err("a timeout is a failed run");
        assert!(
            format!("{error}").contains("timeout"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_exec_outcome_memory_fault_returns_err_naming_the_fault() {
        let out_of_memory = sandbox_result(Some(crate::tools::python::fault::MEMORY), 1);
        let error = exec_outcome(&out_of_memory).expect_err("a memory fault is a failed run");
        assert!(
            format!("{error}").contains("memory"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_exec_outcome_nonzero_exit_without_a_fault_returns_err() {
        let error = exec_outcome(&sandbox_result(None, 3))
            .expect_err("a non-zero exit code is a failed run");
        assert!(
            format!("{error}").contains("exit code 3"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_handle_exec_unsupported_fault_returns_err_naming_the_fault() {
        let error = handle_exec("import subprocess".to_string())
            .expect_err("a program the sandbox cannot run did not run to completion");
        assert!(
            format!("{error}").contains("unsupported"),
            "unexpected error: {error}"
        );
    }
}
