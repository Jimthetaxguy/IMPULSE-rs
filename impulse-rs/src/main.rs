//! Impulse CLI entry point — mode dispatch and argument parsing.
//!
//! Parses CLI arguments via `clap` and routes to one of two execution paths:
//! **daemon mode** (`--daemon`) forwards commands over a Unix socket to a running
//! daemon process, while **direct mode** (default) executes commands in-process
//! and exits. The `--impulse-dir` flag controls where `.impulse/` data lives.
//!
//! Thin binary entrypoint: the module tree lives in the `impulse_rs` library
//! crate (`src/lib.rs`, TUI_SPEC.md T5) so the sibling `ion` binary
//! (`src/bin/ion.rs`) can also depend on it without duplicating handler logic.

use anyhow::Result;
use clap::Parser;

use impulse_rs::{cli, client, handlers, resolve_impulse_dir};

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
