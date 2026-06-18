//! `impulse-mcp` CLI — process entry point for the Impulse-RS MCP server.
//!
//! Parses command-line flags, builds an [`impulse_runtime::Orchestrator`]
//! with the requested workspace roots, wraps it in an
//! [`impulse_mcp::ImpulseMcpServer`], and runs the stdio transport
//! forever. The process is expected to be spawned by an MCP client
//! (Cursor, Claude Code, Codex, etc.); stdio is the wire.
//!
//! # Logging
//!
//! All logs go to **stderr**. stdout is reserved for the MCP transport.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use impulse_mcp::{serve_stdio, ImpulseMcpServer};
use impulse_runtime::Orchestrator;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// CLI args for the `impulse-mcp` binary.
#[derive(Debug, Parser)]
#[command(
    name = "impulse-mcp",
    about = "Model Context Protocol server for the Impulse-RS orchestrator (stdio transport).",
    version
)]
struct Cli {
    /// Absolute paths to register as workspace roots on startup. May be
    /// repeated; the orchestrator refuses to start if any path is not
    /// an existing absolute directory.
    #[arg(long = "workspace-roots", value_name = "PATH")]
    workspace_roots: Vec<PathBuf>,

    /// Optional path to the orchestrator binary (forwarded to the
    /// runtime for daemon-mode spawns). When omitted the orchestrator
    /// runs in-process.
    #[arg(long = "orchestrator-binary", value_name = "PATH")]
    orchestrator_binary: Option<PathBuf>,

    /// EnvFilter directive for the tracing subscriber. Defaults to
    /// `info,impulse=debug,impulse_mcp=debug` so the server itself
    /// always logs at debug while downgrading third-party noise.
    #[arg(
        long = "log-filter",
        value_name = "FILTER",
        default_value = "info,impulse=debug,impulse_mcp=debug"
    )]
    log_filter: String,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Tracing may not be initialized yet (e.g. if the env-filter
            // directive itself was malformed), so we print to stderr
            // unconditionally and then try to log through tracing if it
            // is up.
            eprintln!("impulse-mcp: fatal: {err:#}");
            tracing::error!(error = %err, "impulse-mcp exited with error");
            ExitCode::FAILURE
        }
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();

    // 1. Tracing — stderr only, env-filter driven. Stdout is the MCP
    //    transport; we must not pollute it.
    init_tracing(&cli.log_filter).context("failed to initialize tracing")?;

    tracing::info!(
        workspace_roots = cli.workspace_roots.len(),
        orchestrator_binary = ?cli.orchestrator_binary,
        "starting impulse-mcp"
    );

    // 2. Build the orchestrator. Each --workspace-roots entry must be
    //    an existing absolute directory; the orchestrator builder will
    //    surface that as a typed error.
    let mut builder = Orchestrator::builder();
    for root in &cli.workspace_roots {
        builder = builder.with_workspace_root(root.clone());
    }
    let orchestrator: Arc<Orchestrator> = builder
        .build()
        .context("failed to build the Impulse-RS orchestrator")?;

    if let Some(binary) = &cli.orchestrator_binary {
        // Stored as a tracing breadcrumb for now; the orchestrator does
        // not yet consume it in in-process mode. A future daemon-mode
        // spawn will hand the path to the IPC client.
        tracing::info!(binary = %binary.display(), "orchestrator binary override recorded");
    }

    // 3. Wrap and serve. `serve_stdio` returns when the transport
    //    closes (the parent process disconnects or sends a shutdown).
    let server = ImpulseMcpServer::new(orchestrator);
    serve_stdio(server)
        .await
        .context("impulse-mcp stdio transport terminated with an error")
}

/// Initialize the global tracing subscriber.
///
/// Uses [`tracing_subscriber::fmt`] with an [`EnvFilter`] so operators
/// can dial verbosity up or down via `RUST_LOG` or the `--log-filter`
/// CLI flag. The default writer is stderr.
fn init_tracing(filter: &str) -> Result<()> {
    let env_filter = EnvFilter::try_new(filter)
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_ansi(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .try_init()
        .context("tracing subscriber init failed")?;
    Ok(())
}
