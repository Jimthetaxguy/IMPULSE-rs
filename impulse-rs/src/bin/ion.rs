//! `ion` — Ion harness binary (TUI_SPEC.md T5 skeleton).
//!
//! Bare `ion` prints a placeholder banner (T6 replaces this with the readline
//! REPL). `ion verify` is a one-shot gate run sharing `handle_ion_verify` with
//! `impulse-rs ion-verify` — same flags, same exit-code convention.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ion", version, about = "Ion interactive harness", long_about = None)]
struct IonCli {
    #[command(subcommand)]
    command: Option<IonCommand>,
}

#[derive(Subcommand)]
enum IonCommand {
    /// Run the Ion verification gate (harness #2 — Pi on MiniMax) against a diff
    Verify {
        /// Repository path to verify (defaults to the current directory)
        #[arg(long)]
        repo: Option<String>,
        /// Git ref range to verify, e.g. HEAD~1..HEAD
        #[arg(long, default_value = "HEAD~1..HEAD")]
        diff_ref: String,
        /// Task description passed to the gate
        #[arg(long, default_value = "Verify the pending diff.")]
        description: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = IonCli::parse();
    match cli.command {
        None => {
            print_banner();
            Ok(())
        }
        Some(IonCommand::Verify {
            repo,
            diff_ref,
            description,
            json,
        }) => {
            // impulse-rs's CLI wrapper (TUI_SPEC.md T3's handle_ion_verify):
            // runs the pure run_ion_verify, prints the verdict (text or
            // --json), and maps !response.passed() / contract violation to
            // process exit 1. Reused verbatim so the two binaries cannot
            // drift on the ion-verify surface.
            impulse_rs::handlers::ion::handle_ion_verify(repo, diff_ref, description, json).await
        }
    }
}

fn print_banner() {
    println!(
        "ion {} — Ion interactive harness (REPL coming soon; try 'ion verify --help')",
        env!("CARGO_PKG_VERSION")
    );
}
