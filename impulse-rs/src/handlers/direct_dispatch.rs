//! Direct-mode CLI dispatch — routes Commands to handler functions without IPC.
//!
//! Extracted from `run_direct_mode()` in main.rs to keep the entry point
//! focused on argument parsing and top-level mode selection.

use anyhow::Result;
use std::sync::Arc;

use crate::cli::Cli;
use crate::{daemon, envelope, handlers, state, Commands};

/// Run a CLI command in direct mode (in-process, no daemon IPC).
pub(crate) async fn dispatch(cli: Cli) -> Result<()> {
    let impulse_dir = cli.impulse_dir.clone();
    let verbose = cli.verbose;
    let format = cli.format;
    let state = Arc::new(state::State::new(impulse_dir.clone())?);

    match cli.command {
        Commands::Daemon { .. } => {
            daemon::Daemon::new(state.clone()).start().await?;
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
            .await?;
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
            .await?;
        }
        Commands::TrackWrite { file, session_id } => {
            handlers::session::handle_track_write(&state, file, session_id).await?;
        }
        Commands::TrackTool { tool, session_id } => {
            handlers::session::handle_track_tool(&state, tool, session_id).await?;
        }
        Commands::ListSessions => {
            handlers::session::handle_list_sessions(&state).await?;
        }
        Commands::SessionInfo { id } => {
            handlers::session::handle_session_info(&state, id).await?;
        }
        Commands::SessionConflicts { file, session_id } => {
            handlers::session::handle_session_conflicts(&state, file, session_id).await?;
        }
        Commands::Status => {
            handlers::config::handle_status(&state).await?;
        }
        Commands::Debug => {
            println!("Debug snapshot requires daemon mode. Use: impulse-rs --daemon debug");
        }
        Commands::ConflictHistory => {
            handlers::session::handle_conflict_history(&state)?;
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
            .await?;
        }
        Commands::Genome => {
            handlers::memory::handle_genome(&state)?;
        }
        Commands::History => {
            handlers::memory::handle_history(&state)?;
        }
        Commands::ListProviders => {
            handlers::config::handle_list_providers()?;
        }
        Commands::AddDecision {
            description,
            rationale,
        } => {
            handlers::memory::handle_add_decision(&state, description, rationale)?;
        }
        Commands::Init => {
            handlers::config::handle_init(&state, &impulse_dir)?;
        }
        Commands::Config { key, value, list } => {
            handlers::config::handle_config(&state, key, value, list)?;
        }
        Commands::Extract {
            content,
            session_id,
            json,
        } => {
            handlers::system::handle_extract(content, session_id, json)?;
        }
        Commands::Swarm {
            agent_a,
            agent_b,
            threshold,
            json,
        } => {
            handlers::system::handle_swarm(agent_a, agent_b, threshold, json)?;
        }
        Commands::Activity { limit } => {
            handlers::memory::handle_activity(&state, limit).await?;
        }
        Commands::Hooks { platform } => {
            handlers::system::handle_hooks(&state, platform)?;
        }
        Commands::ValidateHooks { platform } => {
            handlers::system::handle_validate_hooks(platform)?;
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
            .await?;
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
            .await?;
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
            .await?;
        }
        Commands::ComputeInjection { query, limit, json } => {
            handlers::injection_handlers::handle_compute_injection(query, limit, json)?;
        }
        Commands::Verify => {
            handlers::build::handle_verify()?;
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
                &state, query, mode, backend, limit, offset, page, total, explain, json,
            )?;
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
                &state, query, mode, backend, limit, offset, page, total, explain, json,
            )?;
        }
        Commands::IndexMemory { scope, rebuild } => {
            handlers::retrieval::handle_index_memory(&state, scope, rebuild)?;
        }
        Commands::RetrievalStatus { check, json } => {
            handlers::retrieval::handle_retrieval_status(&state, check, json)?;
        }
        Commands::Tools {
            subcommand,
            tool,
            dry_run,
        } => {
            handlers::system::handle_tools(verbose, subcommand, tool, dry_run)?;
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
            .await?;
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
            )?;
        }
        Commands::Office {
            subcommand,
            file,
            goal,
            json,
        } => {
            handlers::office::handle_office(subcommand, file, goal, json)?;
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
            )?;
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
            )?;
        }
        Commands::Calc { expression } => {
            handlers::system::handle_calc(expression)?;
        }
        Commands::Exec { code } => {
            handlers::system::handle_exec(code)?;
        }
        Commands::System {} => {
            handlers::system::handle_system()?;
        }
        Commands::Analyze { session_id, scope } => {
            handlers::stewardship_handlers::handle_analyze(&state, session_id, scope).await?;
        }
        Commands::Health {} => {
            handlers::stewardship_handlers::handle_health(&impulse_dir)?;
        }
        Commands::Summary {} => {
            handlers::stewardship_handlers::handle_summary(&impulse_dir)?;
        }
        Commands::Sweep {
            dry_run,
            path,
            days,
            verbose: _,
        } => {
            handlers::build::handle_sweep(&state, dry_run, path, days, verbose)?;
        }
        Commands::Wipe { dry_run, path } => {
            handlers::build::handle_wipe(&state, dry_run, path)?;
        }
        Commands::CleanAll { dry_run } => {
            handlers::build::handle_clean_all(&state, dry_run)?;
        }
        Commands::SccacheSetup { check, json } => {
            handlers::build::handle_sccache_setup(check, json)?;
        }
        Commands::BuildHealth { json } => {
            handlers::build::handle_build_health(&state, json)?;
        }
        Commands::ToolingList { category, json } => {
            handlers::tooling_handlers::handle_tooling_list(&state, &impulse_dir, category, json)?;
        }
        Commands::ToolingDescribe { tool_id, json } => {
            handlers::tooling_handlers::handle_tooling_describe(
                &state,
                &impulse_dir,
                tool_id,
                json,
            )?;
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
            .await?;
        }
        Commands::ToolingSchema { format: fmt } => {
            handlers::tooling_handlers::handle_tooling_schema(&state, &impulse_dir, fmt)?;
        }
        Commands::ToolingValidate { json } => {
            handlers::tooling_handlers::handle_tooling_validate(&state, &impulse_dir, json)?;
        }
        Commands::ToolingReload { json } => {
            handlers::tooling_handlers::handle_tooling_reload(&state, &impulse_dir, json)?;
        }
        Commands::Mcp { subcommand } => {
            handlers::system::handle_mcp(&state, &impulse_dir, subcommand).await?;
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
            )?;
        }
        Commands::AgentStatus { json } => {
            handlers::agent::handle_agent_status(&state, json)?;
        }
        Commands::AgentQuery { prompt, json } => {
            handlers::agent::handle_agent_query(&state, prompt, json).await?;
        }
        Commands::SemDiff {
            base,
            head,
            json,
            session_id,
        } => {
            handlers::semantic_diff_handlers::handle_sem_diff(
                &state, base, head, json, session_id,
            )?;
        }
        Commands::SemBlame { file, json } => {
            handlers::semantic_diff_handlers::handle_sem_blame(file, json)?;
        }
        Commands::SemImpact { entity, json } => {
            handlers::semantic_diff_handlers::handle_sem_impact(entity, json)?;
        }
        Commands::SemStatus { json } => {
            handlers::semantic_diff_handlers::handle_sem_status(json)?;
        }
        Commands::Guard {
            action,
            target,
            list,
            enable,
            disable,
            json,
        } => {
            handlers::guard::handle_guard(&state, action, target, list, enable, disable, json)?;
        }
        Commands::Analytics {
            subcommand,
            json,
            period,
        } => {
            handlers::guard::handle_analytics(&state, subcommand, json, period)?;
        }
        Commands::Describe => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_describe(fmt)?;
        }
        Commands::Schema { command } => {
            let fmt = format.unwrap_or(envelope::OutputFormat::Json);
            handlers::describe::handle_schema(&command, fmt)?;
        }
        Commands::PluginList { json } => {
            handlers::plugin_handlers::handle_plugin_list(json)?;
        }
        Commands::PluginInvoke {
            name,
            path,
            query,
            options,
            json,
        } => {
            handlers::plugin_handlers::handle_plugin_invoke(name, path, query, options, json)?;
        }
    }

    Ok(())
}
