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

    if let Some(sid) = &session_id {
        // Computes and stores the report with a single `sem diff` run.
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
        return Ok(());
    }

    let changes = semantic_diff::run_semantic_diff(&repo_path, &base, &head)?;
    if json {
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
        if let Ok(version) = semantic_diff::sem_version() {
            println!("Version: {}", version);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    /// A shell script standing in for `sem` that appends one line per run
    /// to the returned log, then runs `body`.
    fn fake_sem(body: &str) -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("runs.log");
        let script = dir.path().join("sem");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$*\" >> '{}'\n{body}\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, script, log)
    }

    /// Review finding: with a session id, `sem-diff` ran `sem diff` once,
    /// threw the result away, then ran it again to store the report.
    #[test]
    fn test_handle_sem_diff_with_a_session_runs_sem_once() {
        let (_state_dir, state) = test_state();
        let (_dir, sem, log) = fake_sem("echo '{\"changes\": []}'");
        semantic_diff::with_test_sem(&sem, Duration::from_secs(10), || {
            handle_sem_diff(
                &state,
                "abc".to_string(),
                "HEAD".to_string(),
                true,
                Some("sess-once".to_string()),
            )
        })
        .unwrap();
        let runs = std::fs::read_to_string(&log).unwrap();
        assert_eq!(runs.lines().count(), 1, "sem ran: {runs:?}");
        assert!(state
            .storage()
            .base_path()
            .join("semantic_diffs/sess-once.json")
            .exists());
    }

    /// Review finding: `sem-status` waited on `sem --version` with no
    /// timeout.
    #[test]
    fn test_handle_sem_status_returns_by_the_timeout_when_the_version_hangs() {
        let marker = crate::process_util::test_sleep::unique_duration(5);
        let (_dir, sem, _log) = fake_sem(&format!("sleep {marker}"));
        let start = Instant::now();
        let result = semantic_diff::with_test_sem(&sem, Duration::from_millis(300), || {
            handle_sem_status(false)
        });
        let elapsed = start.elapsed();
        crate::process_util::test_sleep::stop(&marker);
        result.unwrap();
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    }
}
