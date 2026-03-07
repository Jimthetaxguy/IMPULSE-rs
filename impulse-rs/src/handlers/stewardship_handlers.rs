use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{branding, state, stewardship};

pub fn handle_steward(
    state: &Arc<state::State>,
    impulse_dir: &Path,
    subcommand: String,
    transcript: Option<PathBuf>,
    session_id: Option<String>,
    id: Option<String>,
    json: bool,
) -> Result<()> {
    let base = impulse_dir;
    let config = state.config_snapshot()?;
    let stew_config = stewardship::StewardshipConfig::from_config(&config);

    match subcommand.as_str() {
        "status" => {
            let proposals = stewardship::approval::list_pending(base)?;
            let cross = stewardship::cross_project::load_cross_project(base)?;

            if json {
                let status = serde_json::json!({
                    "mode": stew_config.mode.as_str(),
                    "thresholds": {
                        "monitor": stew_config.monitor_threshold,
                        "surgical": stew_config.surgical_threshold,
                        "thoughtful": stew_config.thoughtful_threshold,
                        "emergency": stew_config.emergency_threshold,
                    },
                    "context_window_tokens": stew_config.context_window_tokens,
                    "pending_proposals": proposals.len(),
                    "cross_project_patterns": cross.patterns.len(),
                    "cross_project_learnings": cross.learnings.len(),
                });
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                branding::print_header("Stewardship Status");
                println!("  Mode: {:?}", stew_config.mode);
                println!(
                    "  Thresholds: {:.0}% / {:.0}% / {:.0}% / {:.0}%",
                    stew_config.monitor_threshold * 100.0,
                    stew_config.surgical_threshold * 100.0,
                    stew_config.thoughtful_threshold * 100.0,
                    stew_config.emergency_threshold * 100.0,
                );
                println!(
                    "  Context window: {} tokens",
                    stew_config.context_window_tokens
                );
                println!("  Pending proposals: {}", proposals.len());
                for p in &proposals {
                    println!(
                        "    - {} [{}] ~{} tokens freed",
                        p.id,
                        p.strategy.as_str(),
                        p.estimated_tokens_freed
                    );
                }
                println!("  Cross-project patterns: {}", cross.patterns.len());
                println!("  Cross-project learnings: {}", cross.learnings.len());
            }
        }
        "analyze" => {
            let transcript_path =
                transcript.ok_or_else(|| anyhow::anyhow!("--transcript required for analyze"))?;
            let sid = session_id.as_deref().unwrap_or("unknown");
            let cwd = std::env::current_dir().unwrap_or_default();
            let phash = stewardship::cross_project::project_hash(&cwd.to_string_lossy());
            let analysis =
                stewardship::analyzer::analyze_session(&transcript_path, sid, &phash, &config)?;

            if json {
                let out = serde_json::json!({
                    "session_id": analysis.session_id,
                    "message_count": analysis.message_count,
                    "estimated_tokens": analysis.estimated_tokens,
                    "estimated_context_pct": analysis.estimated_context_pct,
                    "decisions": analysis.decisions.len(),
                    "files_touched": analysis.files_touched,
                    "duplicate_regions": analysis.duplicate_regions.len(),
                    "rot_candidates": analysis.rot_candidates.len(),
                    "key_insights": analysis.key_insights,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                branding::print_header("Session Analysis");
                println!("  Session: {}", analysis.session_id);
                println!("  Messages: {}", analysis.message_count);
                println!(
                    "  Tokens: ~{} ({:.1}% of window)",
                    analysis.estimated_tokens,
                    analysis.estimated_context_pct * 100.0
                );
                println!("  Decisions: {}", analysis.decisions.len());
                println!("  Files touched: {}", analysis.files_touched.len());
                println!("  Duplicate regions: {}", analysis.duplicate_regions.len());
                println!("  Rot candidates: {}", analysis.rot_candidates.len());
                if !analysis.key_insights.is_empty() {
                    println!("  Insights:");
                    for insight in &analysis.key_insights {
                        println!("    - {}", insight);
                    }
                }
            }
        }
        "list" => {
            let proposals = stewardship::approval::list_pending(base)?;
            if json {
                let out: Vec<_> = proposals
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "id": p.id,
                            "strategy": p.strategy.as_str(),
                            "threshold": p.threshold.as_str(),
                            "estimated_tokens_freed": p.estimated_tokens_freed,
                            "regions": p.regions.len(),
                            "status": p.status.as_str(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Pending Proposals ({}):", proposals.len());
                for p in &proposals {
                    println!(
                        "  {} [{:?}] {} \u{2014} ~{} tokens freed",
                        p.id,
                        p.threshold,
                        p.strategy.as_str(),
                        p.estimated_tokens_freed
                    );
                    for region in &p.regions {
                        println!(
                            "    Region: {} ({} messages, ~{} tokens)",
                            region.description,
                            region.message_indices.len(),
                            region.estimated_tokens
                        );
                    }
                }
            }
        }
        "approve" => {
            let pid = id.ok_or_else(|| anyhow::anyhow!("--id required for approve"))?;
            match stewardship::approval::approve_proposal(base, &pid)? {
                true => println!("Proposal {} approved and moved to applied.", pid),
                false => println!("Proposal {} not found in pending.", pid),
            }
        }
        "reject" => {
            let pid = id.ok_or_else(|| anyhow::anyhow!("--id required for reject"))?;
            match stewardship::approval::reject_proposal(base, &pid)? {
                true => println!("Proposal {} rejected and removed.", pid),
                false => println!("Proposal {} not found in pending.", pid),
            }
        }
        "memory" => {
            let cross = stewardship::cross_project::load_cross_project(base)?;
            if json {
                let out = serde_json::json!({
                    "version": cross.version,
                    "updated": cross.updated.to_rfc3339(),
                    "patterns": cross.patterns.iter().map(|p| serde_json::json!({
                        "id": p.id,
                        "type": p.pattern_type,
                        "description": p.description,
                        "occurrences": p.occurrences,
                        "projects": p.projects,
                        "insight": p.insight,
                    })).collect::<Vec<_>>(),
                    "learnings": cross.learnings,
                    "stats": {
                        "total_patterns": cross.stats.total_patterns,
                        "total_sessions": cross.stats.total_sessions_analyzed,
                        "total_learnings": cross.stats.total_learnings,
                    },
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                branding::print_header("Cross-Project Memory");
                println!("  Version: {}", cross.version);
                println!("  Updated: {}", cross.updated.format("%Y-%m-%d %H:%M"));
                println!("  Patterns ({}):", cross.patterns.len());
                for p in &cross.patterns {
                    println!(
                        "    [{}] {} (seen {} times across {} projects)",
                        p.pattern_type,
                        p.description,
                        p.occurrences,
                        p.projects.len()
                    );
                    println!("      Insight: {}", p.insight);
                }
                println!("  Learnings ({}):", cross.learnings.len());
                for l in &cross.learnings {
                    println!("    - {}", l);
                }
            }
        }
        "compact" => {
            let sid =
                session_id.ok_or_else(|| anyhow::anyhow!("--session-id required for compact"))?;
            let cross = stewardship::cross_project::load_cross_project(base)?;

            if let Some(transcript_path) = transcript {
                let sid_ref = sid.as_str();
                let cwd = std::env::current_dir().unwrap_or_default();
                let phash = stewardship::cross_project::project_hash(&cwd.to_string_lossy());
                let analysis = stewardship::analyzer::analyze_session(
                    &transcript_path,
                    sid_ref,
                    &phash,
                    &config,
                )?;
                let context = stewardship::cleanup::build_refined_context(&analysis, &cross);
                print!("{}", context);
            } else {
                let mut context = format!("# Session {} \u{2014} Stewardship Context\n\n", sid);
                if !cross.learnings.is_empty() {
                    context.push_str("## Cross-Project Learnings\n");
                    for l in &cross.learnings {
                        context.push_str(&format!("- {}\n", l));
                    }
                }
                if !cross.patterns.is_empty() {
                    context.push_str("\n## Relevant Patterns\n");
                    for p in &cross.patterns {
                        context.push_str(&format!("- {} ({})\n", p.description, p.insight));
                    }
                }
                print!("{}", context);
            }
        }
        _ => {
            eprintln!("Unknown steward subcommand: '{}'. Available: status, analyze, list, approve, reject, memory, compact", subcommand);
        }
    }
    Ok(())
}

pub async fn handle_analyze(
    state: &Arc<state::State>,
    session_id: Option<String>,
    scope: String,
) -> Result<()> {
    println!("=== Impulse Analysis ===");

    match scope.as_str() {
        "session" | "sessions" => {
            if let Some(sid) = session_id {
                println!("\nAnalyzing session: {}", sid);
                match state.get_session(&sid).await {
                    Ok(Some(s)) => {
                        println!("Session: {} ({})", s.name, s.id);
                        println!("Files: {}", s.active_files.len());
                        println!("Tools: {}", s.recent_tools.len());
                    }
                    Ok(None) => {
                        println!("Session not found: {}", sid);
                    }
                    Err(e) => {
                        eprintln!("Error fetching session: {}", e);
                    }
                }
            } else {
                println!("\nUsage: --session-id required for session analysis");
            }
        }
        "token" | "tokens" => {
            println!("\nToken analysis:");
            println!("Use `impulse-rs activity` for token tracking details");
        }
        "all" | "*" => {
            println!("\nAvailable analysis scopes:");
            println!("  session  - Analyze specific session (requires --session-id)");
            println!("  tokens   - Token usage analysis");
            println!("  all      - This help message");
        }
        _ => {
            eprintln!("Unknown scope: {}. Use: session, tokens, all", scope);
        }
    }
    Ok(())
}

pub fn handle_health(impulse_dir: &Path) -> Result<()> {
    use crate::tools::health::{check_impulse_health, check_python_health, HealthStatus};

    println!("=== Impulse Health Check ===\n");

    let python_check = check_python_health();
    let status_icon = match python_check.status {
        HealthStatus::Healthy => "\u{2713}",
        HealthStatus::Warning => "\u{26a0}",
        HealthStatus::Error => "\u{2717}",
    };
    print!("Python: {} ", status_icon);
    match python_check.status {
        HealthStatus::Healthy => println!("OK"),
        HealthStatus::Warning => println!("Warning: {:?}", python_check.message),
        HealthStatus::Error => println!("Error: {:?}", python_check.message),
    }

    let report = check_impulse_health(impulse_dir);

    let overall_icon = match report.overall_status {
        HealthStatus::Healthy => "\u{2713}",
        HealthStatus::Warning => "\u{26a0}",
        HealthStatus::Error => "\u{2717}",
    };

    println!("\nOverall Status: {} ", overall_icon);
    match report.overall_status {
        HealthStatus::Healthy => println!("All systems operational"),
        HealthStatus::Warning => println!("Some issues detected"),
        HealthStatus::Error => println!("Critical issues found"),
    }

    println!("\nDetailed Checks:");
    for check in &report.checks {
        let icon = match check.status {
            HealthStatus::Healthy => "\u{2713}",
            HealthStatus::Warning => "\u{26a0}",
            HealthStatus::Error => "\u{2717}",
        };
        print!("  {} {}", icon, check.name);
        if let Some(msg) = &check.message {
            println!(" - {}", msg);
        } else {
            println!();
        }
    }
    Ok(())
}

pub fn handle_summary(impulse_dir: &Path) -> Result<()> {
    println!("=== Impulse Summary ===\n");

    println!("Impulse Directory: {}", impulse_dir.display());
    println!("\nQuick Commands:");
    println!("  impulse-rs status     - Detailed status");
    println!("  impulse-rs health    - Health check");
    println!("  impulse-rs activity  - Recent activity");
    println!("  impulse-rs history   - Session history");
    println!("  impulse-rs list      - List sessions");
    println!("  impulse-rs system    - System info");

    println!("\nCLI Tools Tracked:");
    for tool in crate::tools::known_tools() {
        println!("  - {} ({})", tool.name, tool.id);
    }

    println!("\nBuild Hygiene:");
    println!("  impulse-rs sweep         - Clean stale build artifacts");
    println!("  impulse-rs wipe          - Aggressive target/ cleanup");
    println!("  impulse-rs clean-all     - Workspace-wide cargo clean");
    println!("  impulse-rs sccache-setup - Configure compilation cache");
    println!("  impulse-rs build-health  - Disk usage report");
    Ok(())
}
