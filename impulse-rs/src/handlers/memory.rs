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

// TODO(refactor): extract params into struct
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

// TODO(refactor): extract params into struct
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    // ── handle_genome ───────────────────────────────────────────────────

    #[test]
    fn genome_empty_succeeds() {
        let (tmp, st) = test_state();
        // Write an empty genome so the file exists
        st.storage()
            .write_json("GENOME.md", &memory::Genome::new())
            .unwrap();
        let _ = tmp; // keep alive
        let result = handle_genome(&st);
        assert!(result.is_ok());
    }

    // ── handle_add_decision ─────────────────────────────────────────────

    #[test]
    fn add_decision_writes_genome() {
        let (tmp, st) = test_state();
        st.storage()
            .write_json("GENOME.md", &memory::Genome::new())
            .unwrap();
        let result = handle_add_decision(
            &st,
            "Use Rust for all new modules".to_string(),
            Some("Performance and safety".to_string()),
        );
        assert!(result.is_ok());

        // Verify the decision was persisted
        let genome: memory::Genome = st.storage().read_json("GENOME.md").unwrap();
        assert_eq!(genome.decisions.len(), 1);
        assert_eq!(
            genome.decisions[0].description,
            "Use Rust for all new modules"
        );
        let _ = tmp;
    }

    #[test]
    fn add_decision_dedup_guard() {
        let (tmp, st) = test_state();
        st.storage()
            .write_json("GENOME.md", &memory::Genome::new())
            .unwrap();
        // Add same decision twice
        handle_add_decision(&st, "Same decision".to_string(), None).unwrap();
        handle_add_decision(&st, "Same decision".to_string(), None).unwrap();

        let genome: memory::Genome = st.storage().read_json("GENOME.md").unwrap();
        // Genome::add_decision has dedup guard — should only be 1
        assert_eq!(genome.decisions.len(), 1);
        let _ = tmp;
    }

    #[test]
    fn add_decision_different_decisions() {
        let (tmp, st) = test_state();
        st.storage()
            .write_json("GENOME.md", &memory::Genome::new())
            .unwrap();
        handle_add_decision(&st, "Decision A".to_string(), None).unwrap();
        handle_add_decision(&st, "Decision B".to_string(), Some("rationale".to_string())).unwrap();

        let genome: memory::Genome = st.storage().read_json("GENOME.md").unwrap();
        assert_eq!(genome.decisions.len(), 2);
        let _ = tmp;
    }

    // ── handle_history ──────────────────────────────────────────────────

    #[test]
    fn history_empty_succeeds() {
        let (_tmp, st) = test_state();
        let result = handle_history(&st);
        assert!(result.is_ok());
    }

    // ── handle_activity ─────────────────────────────────────────────────

    #[tokio::test]
    async fn activity_no_sessions() {
        let (_tmp, st) = test_state();
        let result = handle_activity(&st, 10).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn activity_with_session_and_files() {
        let (_tmp, st) = test_state();
        let session = st
            .create_session("test-session".to_string(), None)
            .await
            .unwrap();
        st.track_file(&session.id, "src/main.rs").await.unwrap();
        st.track_tool(&session.id, "read_file").await.unwrap();

        let result = handle_activity(&st, 5).await;
        assert!(result.is_ok());
    }
}
