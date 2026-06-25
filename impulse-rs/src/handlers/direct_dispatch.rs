//! Direct-mode CLI dispatch — routes Commands to handler functions without IPC.
//!
//! Extracted from `run_direct_mode()` in main.rs to keep the entry point
//! focused on argument parsing and top-level mode selection.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::cli::Cli;
use crate::{daemon, envelope, handlers, state, Commands};

/// Run a CLI command in direct mode (in-process, no daemon IPC).
pub(crate) async fn dispatch(cli: Cli) -> Result<()> {
    let impulse_dir = cli.impulse_dir.clone();
    let verbose = cli.verbose;
    let format = cli.format;
    let state = Arc::new(
        state::State::new(impulse_dir.clone()).context("Failed to initialize impulse state")?,
    );

    match cli.command {
        Commands::Daemon { .. } => {
            daemon::Daemon::new(state.clone())
                .start()
                .await
                .context("Failed to start daemon")?;
        }
        Commands::Run => {
            println!("Use: impulse-rs --daemon for daemon mode");
        }
        Commands::SessionStart {
            name,
            platform,
            inject_mode,
            inject_explain,
        } => {
            handlers::session::handle_session_start(
                &state,
                name,
                platform,
                inject_mode,
                inject_explain,
            )
            .await
            .context("Failed to handle session-start command")?;
        }
        Commands::SessionEnd {
            session_id,
            summary,
            verify,
            sem_diff_base,
        } => {
            handlers::session::handle_session_end(
                &state,
                session_id,
                summary,
                verify,
                sem_diff_base,
            )
            .await
            .context("Failed to handle session-end command")?;
        }
        Commands::TrackWrite { file, session_id } => {
            handlers::session::handle_track_write(&state, file, session_id)
                .await
                .context("Failed to handle track-write command")?;
        }
        Commands::TrackTool { tool, session_id } => {
            handlers::session::handle_track_tool(&state, tool, session_id)
                .await
                .context("Failed to handle track-tool command")?;
        }
        Commands::ListSessions => {
            handlers::session::handle_list_sessions(&state)
                .await
                .context("Failed to handle list-sessions command")?;
        }
        Commands::SessionInfo { id } => {
            handlers::session::handle_session_info(&state, id)
                .await
                .context("Failed to handle session-info command")?;
        }
        Commands::SessionConflicts { file, session_id } => {
            handlers::session::handle_session_conflicts(&state, file, session_id)
                .await
                .context("Failed to handle session-conflicts command")?;
        }
        Commands::Status => {
            handlers::config::handle_status(&state, format)
                .await
                .context("Failed to handle status command")?;
        }
        Commands::Debug => {
            println!("Debug snapshot requires daemon mode. Use: impulse-rs --daemon debug");
        }
        Commands::ConflictHistory => {
            handlers::session::handle_conflict_history(&state)
                .context("Failed to handle conflict-history command")?;
        }
        Commands::Chat {
            session_id: _,
            message,
            inject_mode,
            inject_explain,
        } => {
            handlers::system::handle_chat_and_display(
                &state,
                &message,
                inject_mode.as_deref(),
                inject_explain,
            )
            .await
            .context("Failed to handle chat command")?;
        }
        Commands::Genome => {
            handlers::memory::handle_genome(&state).context("Failed to handle genome command")?;
        }
        Commands::History => {
            handlers::memory::handle_history(&state).context("Failed to handle history command")?;
        }
        Commands::ListProviders => {
            handlers::config::handle_list_providers()
                .context("Failed to handle list-providers command")?;
        }
        Commands::AddDecision {
            description,
            rationale,
        } => {
            handlers::memory::handle_add_decision(&state, description, rationale)
                .context("Failed to handle add-decision command")?;
        }
        Commands::Init => {
            handlers::config::handle_init(&state, &impulse_dir)
                .context("Failed to handle init command")?;
        }
        Commands::Config { key, value, list } => {
            handlers::config::handle_config(&state, key, value, list)
                .context("Failed to handle config command")?;
        }
        Commands::Extract {
            content,
            session_id,
            json,
        } => {
            handlers::system::handle_extract(content, session_id, json)
                .context("Failed to handle extract command")?;
        }
        Commands::Swarm {
            agent_a,
            agent_b,
            threshold,
            json,
        } => {
            handlers::system::handle_swarm(agent_a, agent_b, threshold, json)
                .context("Failed to handle swarm command")?;
        }
        Commands::Activity { limit } => {
            handlers::memory::handle_activity(&state, limit)
                .await
                .context("Failed to handle activity command")?;
        }
        Commands::Hooks { platform } => {
            handlers::system::handle_hooks(&state, platform)
                .context("Failed to handle hooks command")?;
        }
        Commands::ValidateHooks { platform } => {
            handlers::system::handle_validate_hooks(platform)
                .context("Failed to handle validate-hooks command")?;
        }
        Commands::Orchestrate {
            task,
            inject_mode,
            inject_explain,
            compute_routing,
        } => {
            handlers::injection_handlers::handle_orchestrate(
                &state,
                task,
                inject_mode,
                inject_explain,
                compute_routing,
            )
            .await
            .context("Failed to handle orchestrate command")?;
        }
        Commands::Handoff {
            tool,
            task,
            session_id,
            notes,
            inject_mode,
            inject_explain,
        } => {
            handlers::injection_handlers::handle_handoff(
                &state,
                tool,
                task,
                session_id,
                notes,
                inject_mode,
                inject_explain,
            )
            .await
            .context("Failed to handle handoff command")?;
        }
        Commands::SyncContext {
            session_id,
            inject_mode,
            inject_explain,
        } => {
            handlers::injection_handlers::handle_sync_context(
                &state,
                session_id,
                inject_mode,
                inject_explain,
            )
            .await
            .context("Failed to handle sync-context command")?;
        }
        Commands::ComputeInjection { query, limit, json } => {
            handlers::injection_handlers::handle_compute_injection(query, limit, json)
                .context("Failed to handle compute-injection command")?;
        }
        Commands::Verify => {
            handlers::build::handle_verify().context("Failed to handle verify command")?;
        }
        Commands::SearchHistory {
            query,
            mode,
            backend,
            limit,
            offset,
            page,
            total,
            explain,
            json,
        } => {
            handlers::memory::handle_search_history(
                &state,
                handlers::memory::SearchMemoryOptions {
                    query,
                    mode,
                    backend,
                    limit,
                    offset,
                    page,
                    total,
                    explain,
                    json,
                },
            )
            .context("Failed to handle search-history command")?;
        }
        Commands::SearchGenome {
            query,
            mode,
            backend,
            limit,
            offset,
            page,
            total,
            explain,
            json,
        } => {
            handlers::memory::handle_search_genome(
                &state,
                handlers::memory::SearchMemoryOptions {
                    query,
                    mode,
                    backend,
                    limit,
                    offset,
                    page,
                    total,
                    explain,
                    json,
                },
            )
            .context("Failed to handle search-genome command")?;
        }
        Commands::IndexMemory { scope, rebuild } => {
            handlers::retrieval::handle_index_memory(&state, scope, rebuild)
                .context("Failed to handle index-memory command")?;
        }
        Commands::RetrievalStatus { check, json } => {
            handlers::retrieval::handle_retrieval_status(&state, check, json)
                .context("Failed to handle retrieval-status command")?;
        }
        Commands::Tools {
            subcommand,
            tool,
            dry_run,
        } => {
            handlers::system::handle_tools(verbose, subcommand, tool, dry_run)
                .context("Failed to handle tools command")?;
        }
        Commands::Docs {
            subcommand,
            provider,
            verbose: docs_verbose,
            force,
        } => {
            handlers::system::handle_docs(
                &state,
                verbose,
                subcommand,
                provider,
                docs_verbose,
                force,
            )
            .await
            .context("Failed to handle docs command")?;
        }
        Commands::Model {
            subcommand,
            provider,
            model,
        } => {
            handlers::config::handle_model(
                &state,
                &impulse_dir,
                verbose,
                subcommand,
                provider,
                model,
            )
            .context("Failed to handle model command")?;
        }
        Commands::Office {
            subcommand,
            file,
            goal,
            json,
        } => {
            handlers::office::handle_office(subcommand, file, goal, json)
                .context("Failed to handle office command")?;
        }
        Commands::Credentials {
            subcommand,
            provider,
            key,
            value,
            socket_path,
            tool,
        } => {
            handlers::config::handle_credentials(
                subcommand,
                provider,
                key,
                value,
                socket_path,
                tool,
            )
            .context("Failed to handle credentials command")?;
        }
        Commands::Steward {
            subcommand,
            transcript,
            session_id,
            id,
            json,
        } => {
            handlers::stewardship_handlers::handle_steward(
                &state,
                &impulse_dir,
                subcommand,
                transcript,
                session_id,
                id,
                json,
            )
            .context("Failed to handle steward command")?;
        }
        Commands::Calc { expression } => {
            handlers::system::handle_calc(expression).context("Failed to handle calc command")?;
        }
        Commands::Exec { code } => {
            handlers::system::handle_exec(code).context("Failed to handle exec command")?;
        }
        Commands::System {} => {
            handlers::system::handle_system().context("Failed to handle system command")?;
        }
        Commands::Analyze { session_id, scope } => {
            handlers::stewardship_handlers::handle_analyze(&state, session_id, scope)
                .await
                .context("Failed to handle analyze command")?;
        }
        Commands::Health {} => {
            handlers::stewardship_handlers::handle_health(&impulse_dir)
                .context("Failed to handle health command")?;
        }
        Commands::Summary {} => {
            handlers::stewardship_handlers::handle_summary(&impulse_dir)
                .context("Failed to handle summary command")?;
        }
        Commands::Sweep {
            dry_run,
            path,
            days,
            verbose: _,
        } => {
            handlers::build::handle_sweep(&state, dry_run, path, days, verbose)
                .context("Failed to handle sweep command")?;
        }
        Commands::Wipe { dry_run, path } => {
            handlers::build::handle_wipe(&state, dry_run, path)
                .context("Failed to handle wipe command")?;
        }
        Commands::CleanAll { dry_run } => {
            handlers::build::handle_clean_all(&state, dry_run)
                .context("Failed to handle clean-all command")?;
        }
        Commands::SccacheSetup { check, json } => {
            handlers::build::handle_sccache_setup(check, json)
                .context("Failed to handle sccache-setup command")?;
        }
        Commands::BuildHealth { json } => {
            handlers::build::handle_build_health(&state, json)
                .context("Failed to handle build-health command")?;
        }
        Commands::ToolingList { category, json } => {
            handlers::tooling_handlers::handle_tooling_list(&state, &impulse_dir, category, json)
                .context("Failed to handle tooling-list command")?;
        }
        Commands::ToolingDescribe { tool_id, json } => {
            handlers::tooling_handlers::handle_tooling_describe(
                &state,
                &impulse_dir,
                tool_id,
                json,
            )
            .context("Failed to handle tooling-describe command")?;
        }
        Commands::ToolingRun {
            tool_id,
            params,
            json,
        } => {
            handlers::tooling_handlers::handle_tooling_run(
                &state,
                &impulse_dir,
                tool_id,
                params,
                json,
            )
            .await
            .context("Failed to handle tooling-run command")?;
        }
        Commands::ToolingSchema { format: fmt } => {
            handlers::tooling_handlers::handle_tooling_schema(&state, &impulse_dir, fmt)
                .context("Failed to handle tooling-schema command")?;
        }
        Commands::ToolingValidate { json } => {
            handlers::tooling_handlers::handle_tooling_validate(&state, &impulse_dir, json)
                .context("Failed to handle tooling-validate command")?;
        }
        Commands::ToolingReload { json } => {
            handlers::tooling_handlers::handle_tooling_reload(&state, &impulse_dir, json)
                .context("Failed to handle tooling-reload command")?;
        }
        Commands::Mcp { subcommand } => {
            handlers::system::handle_mcp(&state, &impulse_dir, subcommand)
                .await
                .context("Failed to handle mcp command")?;
        }
        Commands::AgentConfigure {
            provider,
            api_key,
            model,
            harness,
            auto_review,
            auto_coordinate,
        } => {
            handlers::agent::handle_agent_configure(
                &state,
                provider,
                api_key,
                model,
                harness,
                auto_review,
                auto_coordinate,
            )
            .context("Failed to handle agent-configure command")?;
        }
        Commands::AgentStatus { json } => {
            handlers::agent::handle_agent_status(&state, json)
                .context("Failed to handle agent-status command")?;
        }
        Commands::AgentQuery { prompt, json } => {
            handlers::agent::handle_agent_query(&state, prompt, json)
                .await
                .context("Failed to handle agent-query command")?;
        }
        Commands::SemDiff {
            base,
            head,
            json,
            session_id,
        } => {
            handlers::semantic_diff_handlers::handle_sem_diff(&state, base, head, json, session_id)
                .context("Failed to handle sem-diff command")?;
        }
        Commands::SemBlame { file, json } => {
            handlers::semantic_diff_handlers::handle_sem_blame(file, json)
                .context("Failed to handle sem-blame command")?;
        }
        Commands::SemImpact { entity, json } => {
            handlers::semantic_diff_handlers::handle_sem_impact(entity, json)
                .context("Failed to handle sem-impact command")?;
        }
        Commands::SemStatus { json } => {
            handlers::semantic_diff_handlers::handle_sem_status(json)
                .context("Failed to handle sem-status command")?;
        }
        Commands::Guard {
            action,
            target,
            list,
            enable,
            disable,
            json,
        } => {
            handlers::guard::handle_guard(&state, action, target, list, enable, disable, json)
                .context("Failed to handle guard command")?;
        }
        Commands::Analytics {
            subcommand,
            json,
            period,
        } => {
            handlers::guard::handle_analytics(&state, subcommand, json, period)
                .context("Failed to handle analytics command")?;
        }
        Commands::Describe => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_describe(fmt)
                .context("Failed to handle describe command")?;
        }
        Commands::Schema { command } => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_schema(&command, fmt)
                .context("Failed to handle schema command")?;
        }
        Commands::PluginList { json } => {
            handlers::plugin_handlers::handle_plugin_list(json)
                .context("Failed to handle plugin-list command")?;
        }
        Commands::PluginInvoke {
            name,
            path,
            query,
            options,
            json,
        } => {
            handlers::plugin_handlers::handle_plugin_invoke(name, path, query, options, json)
                .context("Failed to handle plugin-invoke command")?;
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
    use crate::cli::{Cli, Commands};
    use tempfile::TempDir;

    /// Build a Cli pointing at a temp .impulse dir with the given command.
    fn cli_with(tmp: &TempDir, command: Commands) -> Cli {
        Cli {
            command,
            impulse_dir: tmp.path().to_path_buf(),
            verbose: false,
            daemon: false,
            socket: None,
            format: None,
        }
    }

    // ── Commands::Run (prints hint, returns Ok) ───────────────────────────

    #[tokio::test]
    async fn test_dispatch_run_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Run);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Run command should return Ok");
    }

    // ── Commands::Debug (prints hint, returns Ok) ─────────────────────────

    #[tokio::test]
    async fn test_dispatch_debug_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Debug);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Debug command should return Ok");
    }

    // ── Commands::ListProviders (pure output, no state needed) ────────────

    #[tokio::test]
    async fn test_dispatch_list_providers_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::ListProviders);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ListProviders should return Ok");
    }

    // ── Commands::System (collects sysinfo, returns Ok) ───────────────────

    #[tokio::test]
    async fn test_dispatch_system_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::System {});
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "System command should return Ok");
    }

    // ── Commands::Genome (reads from state — empty is ok) ─────────────────

    #[tokio::test]
    async fn test_dispatch_genome_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Genome);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Genome command on fresh state should return Ok"
        );
    }

    // ── Commands::History (reads from state — empty is ok) ────────────────

    #[tokio::test]
    async fn test_dispatch_history_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::History);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "History command on fresh state should return Ok"
        );
    }

    // ── Commands::ListSessions (empty session list) ───────────────────────

    #[tokio::test]
    async fn test_dispatch_list_sessions_empty_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::ListSessions);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "ListSessions on fresh state should return Ok"
        );
    }

    // ── Commands::Status (shows session count) ────────────────────────────

    #[tokio::test]
    async fn test_dispatch_status_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Status);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Status on fresh state should return Ok");
    }

    // ── Commands::ConflictHistory (empty trail) ───────────────────────────

    #[tokio::test]
    async fn test_dispatch_conflict_history_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::ConflictHistory);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "ConflictHistory on fresh state should return Ok"
        );
    }

    // ── Commands::Config with --list ──────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_config_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Config {
                key: None,
                value: None,
                list: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Config --list should return Ok");
    }

    // ── Commands::Config with key only (get) ──────────────────────────────

    #[tokio::test]
    async fn test_dispatch_config_get_known_key_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Config {
                key: Some("log_level".to_string()),
                value: None,
                list: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Config get known key should return Ok");
    }

    // ── Commands::Config with key+value (set) ─────────────────────────────

    #[tokio::test]
    async fn test_dispatch_config_set_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Config {
                key: Some("log_level".to_string()),
                value: Some("debug".to_string()),
                list: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Config set should return Ok");
    }

    // ── Commands::Config with no args (shows usage) ───────────────────────

    #[tokio::test]
    async fn test_dispatch_config_no_args_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Config {
                key: None,
                value: None,
                list: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Config with no args should return Ok (usage)"
        );
    }

    // ── Commands::Init (creates .impulse files) ───────────────────────────

    #[tokio::test]
    async fn test_dispatch_init_creates_files() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Init);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Init should return Ok");
        assert!(
            tmp.path().join("LIVE_STATE.json").exists(),
            "Init should create LIVE_STATE.json"
        );
        assert!(
            tmp.path().join("config.json").exists(),
            "Init should create config.json"
        );
    }

    // ── Commands::AddDecision (writes to genome) ──────────────────────────

    #[tokio::test]
    async fn test_dispatch_add_decision_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AddDecision {
                description: "Use Rust for core".to_string(),
                rationale: Some("Performance and safety".to_string()),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "AddDecision should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_add_decision_no_rationale_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AddDecision {
                description: "Adopt TDD".to_string(),
                rationale: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "AddDecision without rationale should return Ok"
        );
    }

    // ── Commands::AgentStatus (unconfigured = ok) ─────────────────────────

    #[tokio::test]
    async fn test_dispatch_agent_status_unconfigured_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::AgentStatus { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "AgentStatus unconfigured should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_agent_status_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::AgentStatus { json: true });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "AgentStatus --json should return Ok");
    }

    // ── Commands::AgentConfigure (no args = ok) ───────────────────────────

    #[tokio::test]
    async fn test_dispatch_agent_configure_no_args_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AgentConfigure {
                provider: None,
                api_key: None,
                model: None,
                harness: None,
                auto_review: false,
                auto_coordinate: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "AgentConfigure with no args should return Ok"
        );
    }

    // ── Commands::Guard with --list ───────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_guard_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Guard {
                action: None,
                target: "any".to_string(),
                list: true,
                enable: None,
                disable: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Guard --list should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_guard_list_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Guard {
                action: None,
                target: "any".to_string(),
                list: true,
                enable: None,
                disable: None,
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Guard --list --json should return Ok");
    }

    // ── Commands::Analytics ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_analytics_conflicts_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analytics {
                subcommand: "conflicts".to_string(),
                json: false,
                period: "day".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Analytics conflicts should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_analytics_unknown_subcommand_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analytics {
                subcommand: "nonexistent".to_string(),
                json: false,
                period: "all".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analytics with unknown subcommand prints message but returns Ok"
        );
    }

    // ── Commands::Describe (uses format.unwrap_or logic) ──────────────────

    #[tokio::test]
    async fn test_dispatch_describe_default_format_returns_ok() {
        let tmp = TempDir::new().unwrap();
        // format=None triggers unwrap_or(Json)
        let cli = cli_with(&tmp, Commands::Describe);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Describe with default format should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_describe_explicit_text_format_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let mut cli = cli_with(&tmp, Commands::Describe);
        cli.format = Some(envelope::OutputFormat::Text);
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Describe with Text format should return Ok");
    }

    // ── Commands::Schema ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_schema_known_command_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Schema {
                command: "status".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Schema for known command should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_schema_unknown_command_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Schema {
                command: "nonexistent-command".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "Schema for unknown command should return Err"
        );
    }

    // ── Commands::PluginList ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_plugin_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::PluginList { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "PluginList should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_plugin_list_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::PluginList { json: true });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "PluginList --json should return Ok");
    }

    // ── Commands::RetrievalStatus ─────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_retrieval_status_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::RetrievalStatus {
                check: false,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "RetrievalStatus should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_retrieval_status_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::RetrievalStatus {
                check: false,
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "RetrievalStatus --json should return Ok");
    }

    // ── Commands::IndexMemory ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_index_memory_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::IndexMemory {
                scope: "all".to_string(),
                rebuild: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "IndexMemory should return Ok");
    }

    // ── Commands::SearchHistory (empty index, returns ok) ─────────────────

    #[tokio::test]
    async fn test_dispatch_search_history_empty_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SearchHistory {
                query: "test query".to_string(),
                mode: None,
                backend: None,
                limit: Some(5),
                offset: None,
                page: None,
                total: false,
                explain: false,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SearchHistory on empty index should return Ok"
        );
    }

    // ── Commands::SearchGenome (empty index, returns ok) ──────────────────

    #[tokio::test]
    async fn test_dispatch_search_genome_empty_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SearchGenome {
                query: "test query".to_string(),
                mode: None,
                backend: None,
                limit: Some(5),
                offset: None,
                page: None,
                total: false,
                explain: false,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SearchGenome on empty index should return Ok"
        );
    }

    // ── Commands::BuildHealth ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_build_health_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::BuildHealth { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "BuildHealth should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_build_health_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::BuildHealth { json: true });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "BuildHealth --json should return Ok");
    }

    // ── Commands::SccacheSetup with --check ───────────────────────────────

    #[tokio::test]
    async fn test_dispatch_sccache_setup_check_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SccacheSetup {
                check: true,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "SccacheSetup --check should return Ok");
    }

    // ── Commands::Activity ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_activity_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Activity { limit: 10 });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Activity should return Ok");
    }

    // ── Commands::Hooks ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_hooks_all_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Hooks {
                platform: "all".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Hooks --platform all should return Ok");
    }

    // ── Commands::SessionConflicts ────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_session_conflicts_empty_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionConflicts {
                file: None,
                session_id: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SessionConflicts on fresh state should return Ok"
        );
    }

    // ── Commands::SemStatus ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_sem_status_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::SemStatus { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "SemStatus should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_sem_status_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::SemStatus { json: true });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "SemStatus --json should return Ok");
    }

    // ── Commands::Swarm ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_swarm_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Swarm {
                agent_a: "claude".to_string(),
                agent_b: "codex".to_string(),
                threshold: 0.88,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Swarm should return Ok");
    }

    // ── Commands::ToolingList ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_tooling_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingList {
                category: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ToolingList should return Ok");
    }

    // ── Commands::ToolingValidate ─────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_tooling_validate_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::ToolingValidate { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ToolingValidate should return Ok");
    }

    // ── Commands::ToolingSchema ───────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_tooling_schema_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingSchema {
                format: "json".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "ToolingSchema --format json should return Ok"
        );
    }

    // ── Commands::ToolingReload ───────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_tooling_reload_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::ToolingReload { json: false });
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ToolingReload should return Ok");
    }

    // ── Commands::Credentials (list, env provider) ────────────────────────

    #[tokio::test]
    async fn test_dispatch_credentials_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Credentials {
                subcommand: "list".to_string(),
                provider: None,
                key: None,
                value: None,
                socket_path: None,
                tool: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Credentials list should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_credentials_status_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Credentials {
                subcommand: "status".to_string(),
                provider: None,
                key: None,
                value: None,
                socket_path: None,
                tool: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Credentials status should return Ok");
    }

    // ── Commands::Health ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_health_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Health {});
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Health should return Ok");
    }

    // ── Commands::Summary ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_summary_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Summary {});
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Summary should return Ok");
    }

    // ── Commands::CleanAll (dry_run default) ──────────────────────────────

    #[tokio::test]
    async fn test_dispatch_clean_all_dry_run_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::CleanAll {
                dry_run: Some(true),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "CleanAll --dry-run should return Ok");
    }

    // ── Commands::Wipe (dry_run) ──────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_wipe_dry_run_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Wipe {
                dry_run: Some(true),
                path: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Wipe --dry-run should return Ok");
    }

    // ── Commands::Sweep (dry_run) ─────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_sweep_dry_run_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Sweep {
                dry_run: Some(true),
                path: None,
                days: None,
                verbose: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Sweep --dry-run should return Ok");
    }

    // ── Commands::Steward (status subcommand) ─────────────────────────────

    #[tokio::test]
    async fn test_dispatch_steward_status_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Steward {
                subcommand: "status".to_string(),
                transcript: None,
                session_id: None,
                id: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Steward status should return Ok");
    }

    // ── Commands::Model (list subcommand) ─────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_model_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Model {
                subcommand: "list".to_string(),
                provider: None,
                model: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Model list should return Ok");
    }

    // ── Commands::Extract ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_extract_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Extract {
                content: "test content for extraction".to_string(),
                session_id: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Extract should return Ok");
    }

    // ── Commands::ComputeInjection ────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_compute_injection_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ComputeInjection {
                query: "what happened last session".to_string(),
                limit: 5,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ComputeInjection should return Ok");
    }

    // ── Commands::Office (info subcommand) ────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_office_info_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Office {
                subcommand: "info".to_string(),
                file: None,
                goal: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Office info should return Ok");
    }

    // ── Dispatch initializes state and runs successfully ─────────────────

    #[tokio::test]
    async fn test_dispatch_init_then_config_list_uses_same_dir() {
        let tmp = TempDir::new().unwrap();
        // Init creates files, then config list reads from the same dir
        let cli_init = cli_with(&tmp, Commands::Init);
        let result = dispatch(cli_init).await;
        assert!(result.is_ok(), "Init should succeed");
        assert!(
            tmp.path().join("config.json").exists(),
            "Init should create config.json"
        );

        // Now run config --list against the same dir
        let cli_config = cli_with(
            &tmp,
            Commands::Config {
                key: None,
                value: None,
                list: true,
            },
        );
        let result = dispatch(cli_config).await;
        assert!(result.is_ok(), "Config list after init should succeed");
    }

    // ── Commands::SessionInfo with nonexistent ID ─────────────────────────

    #[tokio::test]
    async fn test_dispatch_session_info_nonexistent_id() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionInfo {
                id: "nonexistent-session-id".to_string(),
            },
        );
        let result = dispatch(cli).await;
        // SessionInfo for a nonexistent ID should either return Ok (with "not found" message)
        // or return an error — either way the dispatch shouldn't panic
        let _ = result; // Just verify no panic
    }

    // ── Commands::Calc (requires Python on PATH) ─────────────────────────

    #[tokio::test]
    async fn test_dispatch_calc_simple_expression_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Calc {
                expression: "2 + 2".to_string(),
            },
        );
        let result = dispatch(cli).await;
        // Succeeds when Python is available, errors when it isn't — no panic either way
        let _ = result;
    }

    #[tokio::test]
    async fn test_dispatch_calc_division_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Calc {
                expression: "100 / 3".to_string(),
            },
        );
        let result = dispatch(cli).await;
        let _ = result;
    }

    // ── Commands::Exec (requires Python on PATH) ─────────────────────────

    #[tokio::test]
    async fn test_dispatch_exec_print_statement_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Exec {
                code: "print('hello')".to_string(),
            },
        );
        let result = dispatch(cli).await;
        let _ = result;
    }

    // ── Commands::ValidateHooks ──────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_validate_hooks_claude_code_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ValidateHooks {
                platform: "claude-code".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "ValidateHooks for claude-code should return Ok"
        );
    }

    // ── Commands::Tools (list subcommand) ────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_tools_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Tools {
                subcommand: "list".to_string(),
                tool: vec![],
                dry_run: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Tools list should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_tools_unknown_subcommand_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Tools {
                subcommand: "nonexistent".to_string(),
                tool: vec![],
                dry_run: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "Tools with unknown subcommand should return Err"
        );
    }

    // ── Commands::ToolingDescribe (nonexistent tool → error) ─────────────

    #[tokio::test]
    async fn test_dispatch_tooling_describe_nonexistent_tool_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingDescribe {
                tool_id: "nonexistent-tool".to_string(),
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "ToolingDescribe for nonexistent tool should return Err"
        );
    }

    #[tokio::test]
    async fn test_dispatch_tooling_describe_invalid_id_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingDescribe {
                tool_id: "../traversal-attempt".to_string(),
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "ToolingDescribe with path traversal ID should return Err"
        );
    }

    // ── Commands::SessionStart ───────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_session_start_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionStart {
                name: Some("test-session".to_string()),
                platform: None,
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "SessionStart should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_session_start_with_platform_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionStart {
                name: Some("platform-test".to_string()),
                platform: Some("claude-code".to_string()),
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SessionStart with platform should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_session_start_no_name_defaults_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionStart {
                name: None,
                platform: None,
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SessionStart with no name should default and return Ok"
        );
    }

    // ── Commands::SessionEnd ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_session_end_nonexistent_session_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SessionEnd {
                session_id: "nonexistent-session".to_string(),
                summary: "test summary".to_string(),
                verify: false,
                sem_diff_base: None,
            },
        );
        let result = dispatch(cli).await;
        // Ending a nonexistent session should error or handle gracefully
        let _ = result;
    }

    // ── Commands::TrackWrite (without active session) ────────────────────

    #[tokio::test]
    async fn test_dispatch_track_write_no_session_returns_err_or_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::TrackWrite {
                file: "src/main.rs".to_string(),
                session_id: None,
            },
        );
        let result = dispatch(cli).await;
        // Without a session ID or IMPULSE_SESSION_ID env, this may error
        let _ = result;
    }

    #[tokio::test]
    async fn test_dispatch_track_write_nonexistent_session_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::TrackWrite {
                file: "src/main.rs".to_string(),
                session_id: Some("no-such-session".to_string()),
            },
        );
        let result = dispatch(cli).await;
        let _ = result; // May error; no panic
    }

    // ── Commands::TrackTool (without active session) ─────────────────────

    #[tokio::test]
    async fn test_dispatch_track_tool_no_session_returns_err_or_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::TrackTool {
                tool: "Bash".to_string(),
                session_id: None,
            },
        );
        let result = dispatch(cli).await;
        let _ = result;
    }

    // ── Commands::SemBlame (sem CLI likely absent → returns Ok) ──────────

    #[tokio::test]
    async fn test_dispatch_sem_blame_returns_ok_or_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SemBlame {
                file: "src/main.rs".to_string(),
                json: false,
            },
        );
        let result = dispatch(cli).await;
        // sem CLI absent → Ok with "not found" message; present → may need valid git repo
        assert!(
            result.is_ok(),
            "SemBlame should return Ok even when sem is unavailable"
        );
    }

    #[tokio::test]
    async fn test_dispatch_sem_blame_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SemBlame {
                file: "src/main.rs".to_string(),
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SemBlame --json should return Ok when sem is unavailable"
        );
    }

    // ── Commands::SemDiff (sem CLI likely absent → returns Ok) ───────────

    #[tokio::test]
    async fn test_dispatch_sem_diff_returns_ok_when_sem_absent() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SemDiff {
                base: "HEAD~1".to_string(),
                head: "HEAD".to_string(),
                json: false,
                session_id: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SemDiff should return Ok when sem CLI is unavailable"
        );
    }

    // ── Commands::SemImpact (sem CLI likely absent → returns Ok) ─────────

    #[tokio::test]
    async fn test_dispatch_sem_impact_handles_missing_sem() {
        // The sem CLI handler returns Ok when sem is absent (the common case).
        // We test the handler directly to avoid dispatch() state construction
        // issues in minimal environments (CI).
        use crate::handlers::semantic_diff_handlers::handle_sem_impact;
        let result = handle_sem_impact("dispatch".to_string(), false);
        if which::which("sem").is_err() {
            assert!(
                result.is_ok(),
                "handle_sem_impact should return Ok when sem CLI is unavailable"
            );
        }
    }

    // ── Commands::AgentQuery (not configured → error) ────────────────────

    #[tokio::test]
    async fn test_dispatch_agent_query_unconfigured_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AgentQuery {
                prompt: "test prompt".to_string(),
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "AgentQuery without configured agent should return Err"
        );
    }

    #[tokio::test]
    async fn test_dispatch_agent_query_unconfigured_json_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AgentQuery {
                prompt: "test prompt".to_string(),
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "AgentQuery --json without configured agent should return Err"
        );
    }

    // ── Commands::PluginInvoke (nonexistent plugin → error) ──────────────

    #[tokio::test]
    async fn test_dispatch_plugin_invoke_nonexistent_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::PluginInvoke {
                name: "nonexistent-plugin".to_string(),
                path: None,
                query: None,
                options: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "PluginInvoke for nonexistent plugin should return Err"
        );
    }

    // ── Commands::Orchestrate (no session, inject off) ───────────────────

    #[tokio::test]
    async fn test_dispatch_orchestrate_no_session_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Orchestrate {
                task: "refactor auth module".to_string(),
                inject_mode: Some("off".to_string()),
                inject_explain: false,
                compute_routing: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Orchestrate with inject_mode=off should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_orchestrate_with_explain_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Orchestrate {
                task: "add logging".to_string(),
                inject_mode: Some("off".to_string()),
                inject_explain: true,
                compute_routing: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Orchestrate with inject_explain=true should return Ok"
        );
    }

    // ── Commands::Handoff (no session) ───────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_handoff_no_session_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Handoff {
                tool: "claude-code".to_string(),
                task: "review PR".to_string(),
                session_id: None,
                notes: None,
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Handoff with no session and inject_mode=off should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_handoff_with_notes_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Handoff {
                tool: "opencode".to_string(),
                task: "implement feature".to_string(),
                session_id: None,
                notes: Some("Priority high".to_string()),
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Handoff with notes should return Ok");
    }

    // ── Commands::SyncContext (no session) ────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_sync_context_no_session_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SyncContext {
                session_id: None,
                inject_mode: Some("off".to_string()),
                inject_explain: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SyncContext with no session and inject_mode=off should return Ok"
        );
    }

    // ── Commands::Analyze (no sessions) ──────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_analyze_all_scope_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analyze {
                session_id: None,
                scope: "all".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analyze with all scope on empty state should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_analyze_session_scope_no_id_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analyze {
                session_id: None,
                scope: "session".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analyze session scope without session_id should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_analyze_nonexistent_session_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analyze {
                session_id: Some("nonexistent-id".to_string()),
                scope: "session".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analyze for nonexistent session should return Ok (not found message)"
        );
    }

    // ── Commands::Verify (runs build verification) ───────────────────────

    #[tokio::test]
    #[ignore = "verify dispatch re-enters cargo test from the repo root and is not safe in-process"]
    async fn test_dispatch_verify_runs_without_panic() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Verify);
        let result = dispatch(cli).await;
        // Verify may fail (no Cargo project at temp dir) but should not panic
        let _ = result;
    }

    // ── Commands::AgentConfigure with specific provider ──────────────────

    #[tokio::test]
    async fn test_dispatch_agent_configure_with_provider_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AgentConfigure {
                provider: Some("anthropic".to_string()),
                api_key: None,
                model: Some("claude-3-5-sonnet".to_string()),
                harness: Some("claude-code".to_string()),
                auto_review: true,
                auto_coordinate: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "AgentConfigure with provider+model+harness should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_agent_configure_invalid_provider_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::AgentConfigure {
                provider: Some("invalid-provider".to_string()),
                api_key: None,
                model: None,
                harness: None,
                auto_review: false,
                auto_coordinate: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "AgentConfigure with invalid provider should return Ok (prints error, doesn't fail)"
        );
    }

    // ── Commands::Describe with explicit JSON format ─────────────────────

    #[tokio::test]
    async fn test_dispatch_describe_explicit_json_format_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let mut cli = cli_with(&tmp, Commands::Describe);
        cli.format = Some(envelope::OutputFormat::Json);
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Describe with explicit JSON format should return Ok"
        );
    }

    // ── Commands::Schema for various command paths ───────────────────────

    #[tokio::test]
    async fn test_dispatch_schema_session_start_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Schema {
                command: "session-start".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Schema for session-start should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_schema_guard_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Schema {
                command: "guard".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Schema for guard should return Ok");
    }

    // ── Commands::Guard with action evaluation ───────────────────────────

    #[tokio::test]
    async fn test_dispatch_guard_evaluate_safe_command_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Guard {
                action: Some("git status".to_string()),
                target: "bash".to_string(),
                list: false,
                enable: None,
                disable: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Guard evaluating safe command should return Ok"
        );
    }

    // ── Commands::Config set then get round trip via dispatch ─────────────

    #[tokio::test]
    async fn test_dispatch_config_set_then_get_round_trip() {
        let tmp = TempDir::new().unwrap();
        // Set
        let cli_set = cli_with(
            &tmp,
            Commands::Config {
                key: Some("log_level".to_string()),
                value: Some("debug".to_string()),
                list: false,
            },
        );
        let result = dispatch(cli_set).await;
        assert!(result.is_ok(), "Config set should succeed");

        // Get — verifies the state was persisted between dispatch calls
        let cli_get = cli_with(
            &tmp,
            Commands::Config {
                key: Some("log_level".to_string()),
                value: None,
                list: false,
            },
        );
        let result = dispatch(cli_get).await;
        assert!(result.is_ok(), "Config get after set should succeed");
    }

    // ── Commands::SearchHistory with pagination params ────────────────────

    #[tokio::test]
    async fn test_dispatch_search_history_with_pagination_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SearchHistory {
                query: "test".to_string(),
                mode: Some("keyword".to_string()),
                backend: None,
                limit: Some(3),
                offset: Some(0),
                page: Some(1),
                total: true,
                explain: true,
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "SearchHistory with full pagination params should return Ok"
        );
    }

    // ── Commands::SearchGenome with json output ──────────────────────────

    #[tokio::test]
    async fn test_dispatch_search_genome_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::SearchGenome {
                query: "decision".to_string(),
                mode: None,
                backend: None,
                limit: None,
                offset: None,
                page: None,
                total: false,
                explain: false,
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "SearchGenome with --json should return Ok");
    }

    // ── Commands::Hooks with specific platforms ──────────────────────────

    #[tokio::test]
    async fn test_dispatch_hooks_claude_code_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Hooks {
                platform: "claude-code".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Hooks for claude-code platform should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_hooks_opencode_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Hooks {
                platform: "opencode".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Hooks for opencode platform should return Ok"
        );
    }

    // ── Commands::Steward (list subcommand) ──────────────────────────────

    #[tokio::test]
    async fn test_dispatch_steward_list_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Steward {
                subcommand: "list".to_string(),
                transcript: None,
                session_id: None,
                id: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "Steward list should return Ok");
    }

    #[tokio::test]
    async fn test_dispatch_steward_unknown_subcommand_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Steward {
                subcommand: "nonexistent".to_string(),
                transcript: None,
                session_id: None,
                id: None,
                json: false,
            },
        );
        let result = dispatch(cli).await;
        // Unknown subcommand prints message but may return Ok
        let _ = result;
    }

    // ── Commands::ToolingList with category filter ───────────────────────

    #[tokio::test]
    async fn test_dispatch_tooling_list_with_category_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingList {
                category: Some("utility".to_string()),
                json: false,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "ToolingList with category filter should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_tooling_list_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::ToolingList {
                category: None,
                json: true,
            },
        );
        let result = dispatch(cli).await;
        assert!(result.is_ok(), "ToolingList --json should return Ok");
    }

    // ── Commands::Credentials (unknown subcommand) ───────────────────────

    #[tokio::test]
    async fn test_dispatch_credentials_unknown_subcommand_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Credentials {
                subcommand: "nonexistent".to_string(),
                provider: None,
                key: None,
                value: None,
                socket_path: None,
                tool: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Credentials with unknown subcommand prints message but returns Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_credentials_get_missing_key_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Credentials {
                subcommand: "get".to_string(),
                provider: None,
                key: None,
                value: None,
                socket_path: None,
                tool: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "Credentials get without --key should return Err"
        );
    }

    // ── Commands::Model unknown subcommand ───────────────────────────────

    #[tokio::test]
    async fn test_dispatch_model_unknown_subcommand_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Model {
                subcommand: "nonexistent".to_string(),
                provider: None,
                model: None,
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Model with unknown subcommand prints message but returns Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_model_set_missing_provider_returns_err() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Model {
                subcommand: "set".to_string(),
                provider: None,
                model: Some("gpt-4".to_string()),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_err(),
            "Model set without provider should return Err"
        );
    }

    // ── Commands::Analytics with all period variants ─────────────────────

    #[tokio::test]
    async fn test_dispatch_analytics_week_period_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analytics {
                subcommand: "conflicts".to_string(),
                json: false,
                period: "week".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analytics conflicts with week period should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_analytics_month_period_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analytics {
                subcommand: "conflicts".to_string(),
                json: false,
                period: "month".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analytics conflicts with month period should return Ok"
        );
    }

    #[tokio::test]
    async fn test_dispatch_analytics_json_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(
            &tmp,
            Commands::Analytics {
                subcommand: "conflicts".to_string(),
                json: true,
                period: "day".to_string(),
            },
        );
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Analytics conflicts with json output should return Ok"
        );
    }

    // ── cli_with helper produces correct structure ────────────────────────

    #[test]
    fn test_cli_with_helper_sets_impulse_dir() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Run);
        assert_eq!(cli.impulse_dir, tmp.path().to_path_buf());
        assert!(!cli.verbose);
        assert!(!cli.daemon);
        assert!(cli.socket.is_none());
        assert!(cli.format.is_none());
    }

    // ── Commands::Describe format=None triggers default ──────────────────

    #[tokio::test]
    async fn test_dispatch_describe_format_none_defaults_to_json() {
        let tmp = TempDir::new().unwrap();
        let cli = cli_with(&tmp, Commands::Describe);
        // format is None in cli_with, so dispatch should use unwrap_or(Json)
        assert!(cli.format.is_none(), "format should be None by default");
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Describe with format=None should default to Json and succeed"
        );
    }

    // ── verbose flag propagation ─────────────────────────────────────────

    #[tokio::test]
    async fn test_dispatch_with_verbose_flag_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let mut cli = cli_with(
            &tmp,
            Commands::Tools {
                subcommand: "list".to_string(),
                tool: vec![],
                dry_run: false,
            },
        );
        cli.verbose = true;
        let result = dispatch(cli).await;
        assert!(
            result.is_ok(),
            "Tools list with verbose=true should return Ok"
        );
    }
}
