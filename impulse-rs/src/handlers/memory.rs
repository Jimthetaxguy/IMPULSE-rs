use anyhow::Result;
use std::sync::Arc;

use crate::{memory, retrieval, state};

pub fn handle_genome(state: &Arc<state::State>) -> Result<()> {
    let genome = state.storage().read_json::<memory::Genome>("GENOME.md")?;
    println!("{}", genome.to_markdown());
    Ok(())
}

pub fn handle_history(state: &Arc<state::State>) -> Result<()> {
    let history = state.get_history_sync()?;
    if history.is_empty() {
        println!("No session history");
    } else {
        for entry in history.iter().rev().take(20) {
            println!(
                "[{}] {} - {}",
                entry.ended_at.format("%Y-%m-%d %H:%M"),
                entry.session_name,
                entry.summary
            );
        }
    }
    Ok(())
}

pub fn handle_add_decision(
    state: &Arc<state::State>,
    description: String,
    rationale: Option<String>,
) -> Result<()> {
    let mut genome: memory::Genome = state.storage().read_json("GENOME.md")?;
    genome.add_decision(description, rationale, Vec::new());
    state.storage().write_json("GENOME.md", &genome)?;
    println!("Added decision to GENOME");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn handle_search_history(
    state: &Arc<state::State>,
    query: String,
    mode: Option<String>,
    backend: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    page: Option<usize>,
    total: bool,
    explain: bool,
    json: bool,
) -> Result<()> {
    let mode = if let Some(m) = mode.as_deref() {
        Some(
            retrieval::types::RetrievalMode::parse(m)
                .ok_or_else(|| anyhow::anyhow!("Invalid mode. Use keyword|semantic"))?,
        )
    } else {
        None
    };
    let backend = if let Some(b) = backend.as_deref() {
        Some(retrieval::types::SearchBackend::parse(b).ok_or_else(|| {
            anyhow::anyhow!("Invalid backend. Use auto|sqlite-vec|rust-cosine|keyword")
        })?)
    } else {
        None
    };
    let page_limit = limit.unwrap_or(10);
    let page_offset = offset.unwrap_or(0)
        + page
            .map(|p| (p.saturating_sub(1)) * page_limit)
            .unwrap_or(0);
    let config = state.config_snapshot()?;
    let resp = retrieval::search_history(
        state.storage().base_path(),
        &config,
        &query,
        mode,
        backend,
        limit,
        Some(page_offset),
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        if total {
            if let Some(tc) = resp.total_count {
                println!("Total matches: {}", tc);
            }
        }
        if resp.used_fallback {
            println!(
                "Mode: {} (fallback) [{}] - {}",
                resp.mode,
                resp.backend_used,
                resp.fallback_reason
                    .unwrap_or_else(|| "unknown reason".to_string())
            );
        } else {
            println!("Mode: {} [{}]", resp.mode, resp.backend_used);
        }
        if resp.results.is_empty() {
            println!("No results");
        } else {
            for (idx, item) in resp.results.iter().enumerate() {
                println!(
                    "{}. [{}] {} ({})\n   {}",
                    idx + 1,
                    item.source,
                    item.title,
                    item.id,
                    item.snippet
                );
            }
        }
        if explain {
            println!(
                "\nExplain: timing={}ms candidates={} fallback_code={}",
                resp.timing_ms,
                resp.candidate_count,
                resp.fallback_code
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            for note in resp.engine_notes {
                println!("  - {}", note);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn handle_search_genome(
    state: &Arc<state::State>,
    query: String,
    mode: Option<String>,
    backend: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    page: Option<usize>,
    total: bool,
    explain: bool,
    json: bool,
) -> Result<()> {
    let mode = if let Some(m) = mode.as_deref() {
        Some(
            retrieval::types::RetrievalMode::parse(m)
                .ok_or_else(|| anyhow::anyhow!("Invalid mode. Use keyword|semantic"))?,
        )
    } else {
        None
    };
    let backend = if let Some(b) = backend.as_deref() {
        Some(retrieval::types::SearchBackend::parse(b).ok_or_else(|| {
            anyhow::anyhow!("Invalid backend. Use auto|sqlite-vec|rust-cosine|keyword")
        })?)
    } else {
        None
    };
    let page_limit = limit.unwrap_or(10);
    let page_offset = offset.unwrap_or(0)
        + page
            .map(|p| (p.saturating_sub(1)) * page_limit)
            .unwrap_or(0);
    let config = state.config_snapshot()?;
    let resp = retrieval::search_genome(
        state.storage().base_path(),
        &config,
        &query,
        mode,
        backend,
        limit,
        Some(page_offset),
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
    } else {
        if total {
            if let Some(tc) = resp.total_count {
                println!("Total matches: {}", tc);
            }
        }
        if resp.used_fallback {
            println!(
                "Mode: {} (fallback) [{}] - {}",
                resp.mode,
                resp.backend_used,
                resp.fallback_reason
                    .unwrap_or_else(|| "unknown reason".to_string())
            );
        } else {
            println!("Mode: {} [{}]", resp.mode, resp.backend_used);
        }
        if resp.results.is_empty() {
            println!("No results");
        } else {
            for (idx, item) in resp.results.iter().enumerate() {
                println!(
                    "{}. [{}] {} ({})\n   {}",
                    idx + 1,
                    item.source,
                    item.title,
                    item.id,
                    item.snippet
                );
            }
        }
        if explain {
            println!(
                "\nExplain: timing={}ms candidates={} fallback_code={}",
                resp.timing_ms,
                resp.candidate_count,
                resp.fallback_code
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            for note in resp.engine_notes {
                println!("  - {}", note);
            }
        }
    }
    Ok(())
}

pub async fn handle_activity(state: &Arc<state::State>, limit: usize) -> Result<()> {
    let sessions = state.list_sessions().await?;
    if sessions.is_empty() {
        println!("No sessions found");
    } else {
        println!(
            "Recent Activity (showing {} most recent):\n=========================================",
            limit
        );

        let mut all_files: Vec<_> = sessions
            .iter()
            .flat_map(|s| {
                s.active_files
                    .iter()
                    .map(|f| (s.name.clone(), f.clone(), s.last_activity))
            })
            .collect();
        let mut all_tools: Vec<_> = sessions
            .iter()
            .flat_map(|s| {
                s.recent_tools
                    .iter()
                    .map(|t| (s.name.clone(), t.clone(), s.last_activity))
            })
            .collect();

        all_files.sort_by(|a, b| b.2.cmp(&a.2));
        all_tools.sort_by(|a, b| b.2.cmp(&a.2));

        println!("\n\u{1f4dd} Files Modified:");
        for (name, file, time) in all_files.iter().take(limit) {
            println!("  [{}] {} - {}", time.format("%H:%M"), name, file);
        }
        println!("\n\u{1f527} Tools Used:");
        for (name, tool, time) in all_tools.iter().take(limit) {
            println!("  [{}] {} - {}", time.format("%H:%M"), name, tool);
        }
    }
    Ok(())
}
