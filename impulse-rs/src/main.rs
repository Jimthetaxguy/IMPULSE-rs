use anyhow::Result;
use clap::Parser;

pub mod cli;
pub mod token_tracker;

pub mod agent;
pub mod agent_discovery;
pub mod branding;
pub mod build_hygiene;
pub mod client;
pub mod context_lifecycle;
pub mod credentials;
pub mod daemon;
pub mod delegation;
pub mod docs;
pub mod envelope;
pub mod error;
pub mod guardrail;
pub mod handlers;
pub mod injection;
pub mod integration_tests;
pub mod llm_backends;
pub mod mcp;
pub mod memory;
pub mod monty;
pub mod notification;
pub mod office;
pub mod ops_workbench;
pub mod orchestration;
pub mod plugin;
pub mod retrieval;
pub mod semantic_diff;
pub mod state;
pub mod stewardship;
pub mod storage;
pub mod tooling;
pub mod tools;
pub mod ui;
pub mod validate;
pub mod verify;

// Re-export CLI types at crate root for backward compatibility.
// Existing code (e.g. handlers/daemon_dispatch.rs) imports `crate::Commands`.
pub(crate) use cli::{Commands, McpCommands};

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = cli::Cli::parse();
    cli.impulse_dir = resolve_impulse_dir(cli.impulse_dir);
    if cli.daemon {
        let socket_path = cli
            .socket
            .unwrap_or_else(|| cli.impulse_dir.join("sockets").join("impulse.sock"));
        let client = client::DaemonClient::new(socket_path);
        handlers::daemon_dispatch::dispatch(cli.command, &cli.impulse_dir, &client, cli.format)
            .await
    } else {
        handlers::direct_dispatch::dispatch(cli).await
    }
}

/// Resolve the data directory.
fn resolve_impulse_dir(requested: std::path::PathBuf) -> std::path::PathBuf {
    requested
}
