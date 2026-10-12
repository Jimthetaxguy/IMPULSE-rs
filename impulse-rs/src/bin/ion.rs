//! `ion` — Ion harness binary (TUI_SPEC.md T5 skeleton + T6 REPL).
//!
//! Bare `ion` prints a startup banner and drops into the readline REPL
//! (`impulse_rs::ion_repl::run`, TUI_SPEC.md T6). `ion verify` is a one-shot
//! gate run sharing `handle_ion_verify` with `impulse-rs ion-verify` — same
//! flags, same exit-code convention.

use std::path::PathBuf;

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
    /// Internal: renders one PDF's text layer, bounded, in this isolated
    /// process, and prints it as JSON to stdout. Spawned only by
    /// `document_read`'s `run_pdf_extraction_child` (review round 1,
    /// P0-1/P0-2) as a crash- and memory-isolated child of the running
    /// `ion` process itself (`std::env::current_exe()` inside `ion`
    /// resolves to this binary). Shares `handlers::internal_pdf_text::run`
    /// with `impulse-rs internal-pdf-text` so the two binaries cannot
    /// drift on this surface. Not a public interface; `hide = true` keeps
    /// it out of `--help`.
    #[command(hide = true)]
    InternalPdfText {
        path: PathBuf,
        #[arg(long)]
        max_chars: usize,
        #[arg(long)]
        max_pages: usize,
        /// Memory-watchdog ceiling override in bytes (review round 3, F2);
        /// see `impulse_rs::cli::Commands::InternalPdfText`'s doc comment.
        #[arg(long)]
        memory_limit_bytes: Option<u64>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = IonCli::parse();
    match cli.command {
        None => impulse_rs::ion_repl::run().await,
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
        Some(IonCommand::InternalPdfText {
            path,
            max_chars,
            max_pages,
            memory_limit_bytes,
        }) => impulse_rs::handlers::internal_pdf_text::run(
            &path,
            max_chars,
            max_pages,
            memory_limit_bytes,
        ),
    }
}
