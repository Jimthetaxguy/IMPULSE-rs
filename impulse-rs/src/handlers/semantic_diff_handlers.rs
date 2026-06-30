//! CLI handlers for semantic diff commands (sem-diff, sem-blame, sem-impact, sem-status).

use anyhow::Result;
use std::sync::Arc;

use crate::{semantic_diff, state};

use super::print_json;

/// Print the standard "sem CLI not found" guidance to stderr.
fn print_sem_not_found() {
    eprintln!("Error: sem CLI not found on PATH.");
    eprintln!("Install: cargo install --git https://github.com/Ataraxy-Labs/sem sem-cli");
    eprintln!("  or:    brew install ataraxy-labs/tap/sem");
}

/// Handle `sem-diff` — compute and display semantic diff between two refs.
pub fn handle_sem_diff(
    state: &Arc<state::State>,
    base: String,
    head: String,
    json: bool,
    session_id: Option<String>,
) -> Result<()> {
    if !semantic_diff::sem_available() {
        print_sem_not_found();
        return Ok(());
    }

    let repo_path = std::env::current_dir()?;
    let changes = semantic_diff::run_semantic_diff(&repo_path, &base, &head)?;

    if let Some(sid) = &session_id {
        // Store the report
        let report = semantic_diff::capture_semantic_diff(
            state.storage().base_path(),
            &repo_path,
            sid,
            &base,
            &head,
        )?;
        if json {
            print_json(&report)?;
        } else {
            println!("{}", report.format_injection_block());
            println!();
            println!("Stored: .impulse/semantic_diffs/{}.json", sid);
        }
    } else if json {
        let report = semantic_diff::SemanticDiffReport::new(String::new(), base, head, changes);
        print_json(&report)?;
    } else {
        let report = semantic_diff::SemanticDiffReport::new(
            String::new(),
            base.clone(),
            head.clone(),
            changes,
        );
        if report.changes.is_empty() {
            println!("No semantic changes between {} and {}", base, head);
        } else {
            println!("{}", report.format_injection_block());
        }
    }

    Ok(())
}

/// Handle `sem-blame` — entity-level git blame.
pub fn handle_sem_blame(file: String, json: bool) -> Result<()> {
    if !semantic_diff::sem_available() {
        print_sem_not_found();
        return Ok(());
    }

    let repo_path = std::env::current_dir()?;
    let entries = semantic_diff::run_semantic_blame(&repo_path, &file)?;

    if json {
        print_json(&entries)?;
    } else if entries.is_empty() {
        println!("No semantic blame entries for {}", file);
    } else {
        println!("Semantic blame for {}", file);
        println!();
        for entry in &entries {
            let msg = entry
                .message
                .as_deref()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            println!(
                "  {} ({}) — {} by {} [{}]",
                entry.entity.name, entry.entity.entity_type, entry.commit, entry.author, msg
            );
        }
    }

    Ok(())
}

/// Handle `sem-impact` — blast radius analysis.
pub fn handle_sem_impact(entity: String, json: bool) -> Result<()> {
    if !semantic_diff::sem_available() {
        print_sem_not_found();
        return Ok(());
    }

    let repo_path = std::env::current_dir()?;
    let result = semantic_diff::run_semantic_impact(&repo_path, &entity)?;

    if json {
        print_json(&result)?;
    } else {
        println!(
            "Impact analysis for {} ({})",
            result.target.name, result.target.entity_type
        );
        println!("Blast radius: {} dependents", result.blast_radius);
        println!();
        if result.dependents.is_empty() {
            println!("  No dependents found.");
        } else {
            for dep in &result.dependents {
                println!("  {} in {}", dep, dep.file_path);
            }
        }
    }

    Ok(())
}

/// Handle `sem-status` — check if sem is available and show version info.
pub fn handle_sem_status(json: bool) -> Result<()> {
    let available = semantic_diff::sem_available();

    if json {
        let status = serde_json::json!({
            "available": available,
            "tool": "sem",
            "install_url": "https://github.com/Ataraxy-Labs/sem",
        });
        print_json(&status)?;
    } else if available {
        println!("sem CLI: available");
        // Try to get version
        let output = std::process::Command::new("sem").arg("--version").output();
        if let Ok(out) = output {
            if out.status.success() {
                let version = String::from_utf8_lossy(&out.stdout);
                println!("Version: {}", version.trim());
            }
        }
        println!("Ready for semantic diffs.");
    } else {
        println!("sem CLI: not found");
        println!();
        println!("Install sem for semantic code diffs:");
        println!("  cargo install --git https://github.com/Ataraxy-Labs/sem sem-cli");
        println!("  brew install ataraxy-labs/tap/sem");
        println!("  https://github.com/Ataraxy-Labs/sem/releases");
    }

    Ok(())
}
