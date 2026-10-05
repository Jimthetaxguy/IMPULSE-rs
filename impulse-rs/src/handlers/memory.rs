use anyhow::Result;
use std::sync::Arc;

use crate::envelope::{write_envelope, EnvelopeBuilder, OutputFormat};
use crate::{memory, retrieval, state};

pub struct SearchMemoryOptions {
    pub query: String,
    pub mode: Option<String>,
    pub backend: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub page: Option<usize>,
    pub total: bool,
    pub explain: bool,
    pub json: bool,
}

/// Handle the `genome` command.
///
/// Reads `GENOME.md` from storage and prints it as formatted markdown. When
/// format is Json/Ndjson, emits the genome content via an envelope instead.
pub fn handle_genome(state: &Arc<state::State>, format: Option<OutputFormat>) -> Result<()> {
    let genome = state.storage().read_json::<memory::Genome>("GENOME.md")?;
    let markdown = genome.to_markdown();
    if let Some(fmt @ (OutputFormat::Json | OutputFormat::Ndjson)) = format {
        let data = serde_json::json!({ "markdown": markdown });
        let env = EnvelopeBuilder::new("genome").ok(data);
        write_envelope(fmt, &env)?;
    } else {
        println!("{}", markdown);
    }
    Ok(())
}

/// Handle the `history` command.
///
/// Prints the 20 most recent session history entries in reverse
/// chronological order with timestamps, names, and summaries. When format
/// is Json/Ndjson, emits the same entries via an envelope instead.
pub fn handle_history(state: &Arc<state::State>, format: Option<OutputFormat>) -> Result<()> {
    let history = state.get_history_sync()?;
    if let Some(fmt @ (OutputFormat::Json | OutputFormat::Ndjson)) = format {
        let entries = history
            .iter()
            .rev()
            .take(20)
            .map(|entry| {
                serde_json::json!({
                    "ended_at": entry.ended_at.format("%Y-%m-%d %H:%M").to_string(),
                    "session_name": entry.session_name,
                    "summary": entry.summary,
                })
            })
            .collect::<Vec<_>>();
        let data = serde_json::json!({
            "count": entries.len(),
            "entries": entries,
        });
        let env = EnvelopeBuilder::new("history").ok(data);
        write_envelope(fmt, &env)?;
    } else if history.is_empty() {
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

/// Handle the `add-decision` command.
///
/// Appends a new decision to `GENOME.md` with an optional rationale.
/// Duplicate descriptions are silently deduplicated by the Genome layer.
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

/// How many results a search fetches at most. Paging (`--offset`,
/// `--page`) and `--total` work within this window.
const MAX_SEARCH_WINDOW: usize = retrieval::query::MAX_RESULT_WINDOW;

/// The retrieval call a search command runs: `retrieval::search_history`
/// or `retrieval::search_genome`.
type SearchFn = fn(
    &std::path::Path,
    &state::Config,
    &str,
    Option<retrieval::types::RetrievalMode>,
    Option<retrieval::types::SearchBackend>,
    Option<usize>,
) -> Result<retrieval::types::SearchResponse>;

/// Handle the `search-history` command.
///
/// Performs keyword or semantic search across session history entries,
/// with pagination, backend selection, and optional scoring explanation.
pub fn handle_search_history(
    state: &Arc<state::State>,
    options: SearchMemoryOptions,
) -> Result<()> {
    handle_search(state, &options, retrieval::search_history)
}

/// Handle the `search-genome` command.
///
/// Performs keyword or semantic search across decisions in `GENOME.md`,
/// with pagination, backend selection, and optional scoring explanation.
pub fn handle_search_genome(state: &Arc<state::State>, options: SearchMemoryOptions) -> Result<()> {
    handle_search(state, &options, retrieval::search_genome)
}

/// Index of a page's first result: `--offset` plus `--page`'s earlier
/// pages. Pages count from 1; page 0 is page 1.
fn page_start(offset: Option<usize>, page: Option<usize>, page_limit: usize) -> usize {
    let earlier_pages = page
        .map(|p| p.saturating_sub(1).saturating_mul(page_limit))
        .unwrap_or(0);
    offset.unwrap_or(0).saturating_add(earlier_pages)
}

/// Keeps `limit` results from index `start`; returns how many results the
/// search matched before paging.
fn take_page(resp: &mut retrieval::types::SearchResponse, start: usize, limit: usize) -> usize {
    let matched = resp.results.len();
    resp.results = std::mem::take(&mut resp.results)
        .into_iter()
        .skip(start)
        .take(limit)
        .collect();
    matched
}

/// Runs a search and keeps one page of it, returning the page and the
/// index of its first result.
///
/// Every backend ranks its matches and returns the top N, so a page is the
/// top `start + limit` with the first `start` skipped, and `--total`
/// fetches the whole window and counts it. The offset used to be passed
/// down and ignored, so every page was the first page.
fn search_page(
    state: &Arc<state::State>,
    options: &SearchMemoryOptions,
    search: SearchFn,
) -> Result<(retrieval::types::SearchResponse, usize)> {
    let mode = if let Some(m) = options.mode.as_deref() {
        Some(
            retrieval::types::RetrievalMode::parse(m)
                .ok_or_else(|| anyhow::anyhow!("Invalid mode. Use keyword|semantic"))?,
        )
    } else {
        None
    };
    let backend = if let Some(b) = options.backend.as_deref() {
        Some(retrieval::types::SearchBackend::parse(b).ok_or_else(|| {
            anyhow::anyhow!("Invalid backend. Use auto|sqlite-vec|rust-cosine|keyword")
        })?)
    } else {
        None
    };
    let config = state.config_snapshot()?;
    let page_limit = options
        .limit
        .unwrap_or(config.retrieval_default_limit)
        .max(1);
    let start = page_start(options.offset, options.page, page_limit);
    if start >= MAX_SEARCH_WINDOW {
        anyhow::bail!(
            "--offset/--page start at result {}, past the first {MAX_SEARCH_WINDOW} results \
             that search pages through",
            start.saturating_add(1)
        );
    }
    let window = if options.total {
        MAX_SEARCH_WINDOW
    } else {
        start.saturating_add(page_limit).min(MAX_SEARCH_WINDOW)
    };
    let mut resp = search(
        state.storage().base_path(),
        &config,
        &options.query,
        mode,
        backend,
        Some(window),
    )?;
    let matched = take_page(&mut resp, start, page_limit);
    if options.total {
        resp.total_count = Some(matched);
        if matched >= MAX_SEARCH_WINDOW {
            resp.engine_notes
                .push(format!("total counts at most {MAX_SEARCH_WINDOW} matches"));
        }
    }
    Ok((resp, start))
}

fn handle_search(
    state: &Arc<state::State>,
    options: &SearchMemoryOptions,
    search: SearchFn,
) -> Result<()> {
    let (resp, start) = search_page(state, options, search)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&resp)?);
        return Ok(());
    }
    if let Some(total) = resp.total_count {
        let capped = if total >= MAX_SEARCH_WINDOW { "+" } else { "" };
        println!("Total matches: {total}{capped}");
    }
    if resp.used_fallback {
        println!(
            "Mode: {} (fallback) [{}] - {}",
            resp.mode,
            resp.backend_used,
            resp.fallback_reason.as_deref().unwrap_or("unknown reason")
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
                start + idx + 1,
                item.source,
                item.title,
                item.id,
                item.snippet
            );
        }
    }
    if options.explain {
        println!(
            "\nExplain: timing={}ms candidates={} fallback_code={}",
            resp.timing_ms,
            resp.candidate_count,
            resp.fallback_code
                .map(|c| c.as_str().to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        for note in &resp.engine_notes {
            println!("  - {}", note);
        }
    }
    Ok(())
}

/// Handle the `activity` command.
///
/// Aggregates recent file modifications and tool usages across all active
/// sessions, sorted by recency, and prints up to `limit` entries.
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

        all_files.sort_by_key(|b| std::cmp::Reverse(b.2));
        all_tools.sort_by_key(|b| std::cmp::Reverse(b.2));

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
        let result = handle_genome(&st, None);
        assert!(result.is_ok());
    }

    #[test]
    fn genome_json_succeeds() {
        let (tmp, st) = test_state();
        st.storage()
            .write_json("GENOME.md", &memory::Genome::new())
            .unwrap();
        let _ = tmp;
        let result = handle_genome(&st, Some(OutputFormat::Json));
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
        let result = handle_history(&st, None);
        assert!(result.is_ok());
    }

    #[test]
    fn history_json_succeeds() {
        let (_tmp, st) = test_state();
        let result = handle_history(&st, Some(OutputFormat::Json));
        assert!(result.is_ok());
    }

    // ── search paging (review P2-24) ────────────────────────────────────

    fn search_options(query: &str) -> SearchMemoryOptions {
        SearchMemoryOptions {
            query: query.to_string(),
            mode: Some("keyword".to_string()),
            backend: None,
            limit: None,
            offset: None,
            page: None,
            total: false,
            explain: false,
            json: false,
        }
    }

    fn index_history(st: &Arc<state::State>, summaries: &[&str]) {
        let entries = summaries
            .iter()
            .enumerate()
            .map(|(i, summary)| state::HistoryEntry {
                session_id: format!("s{i}"),
                session_name: format!("Session {i}"),
                platform: None,
                started_at: chrono::Utc::now(),
                ended_at: chrono::Utc::now(),
                summary: summary.to_string(),
                files_touched: Vec::new(),
                tools_used: Vec::new(),
            })
            .collect::<Vec<_>>();
        retrieval::index(
            st.storage().base_path(),
            &entries,
            &memory::Genome::default(),
            &st.config_snapshot().unwrap(),
            retrieval::types::IndexScope::History,
            true,
        )
        .unwrap();
    }

    #[test]
    fn test_page_start_combines_offset_and_page() {
        assert_eq!(page_start(None, None, 10), 0);
        assert_eq!(page_start(None, Some(3), 10), 20);
        assert_eq!(page_start(Some(5), Some(2), 10), 15);
        assert_eq!(page_start(None, Some(0), 10), 0);
        assert_eq!(
            page_start(Some(usize::MAX), Some(usize::MAX), 10),
            usize::MAX
        );
    }

    #[test]
    fn test_take_page_skips_and_counts() {
        let mut resp = retrieval::types::SearchResponse {
            results: (0..5)
                .map(|i| retrieval::types::SearchResult {
                    source: "history".to_string(),
                    id: format!("r{i}"),
                    title: String::new(),
                    snippet: String::new(),
                    score: 0.0,
                })
                .collect(),
            ..Default::default()
        };
        assert_eq!(take_page(&mut resp, 2, 2), 5);
        let ids = resp
            .results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["r2", "r3"]);
    }

    #[test]
    fn test_search_pages_through_matches_and_counts_the_total() {
        let (_tmp, st) = test_state();
        index_history(
            &st,
            &[
                "alpha migration",
                "alpha rollout",
                "alpha cleanup",
                "beta only",
            ],
        );

        let first = SearchMemoryOptions {
            limit: Some(1),
            ..search_options("alpha")
        };
        let (page_one, start) = search_page(&st, &first, retrieval::search_history).unwrap();
        assert_eq!(start, 0);
        assert_eq!(page_one.results.len(), 1);
        assert_eq!(page_one.total_count, None);

        let second = SearchMemoryOptions {
            limit: Some(1),
            page: Some(2),
            total: true,
            ..search_options("alpha")
        };
        let (page_two, start) = search_page(&st, &second, retrieval::search_history).unwrap();
        assert_eq!(start, 1);
        assert_eq!(page_two.results.len(), 1);
        assert_ne!(page_two.results[0].id, page_one.results[0].id);
        assert_eq!(page_two.total_count, Some(3));

        let past_the_end = SearchMemoryOptions {
            offset: Some(3),
            ..search_options("alpha")
        };
        let (empty, _) = search_page(&st, &past_the_end, retrieval::search_history).unwrap();
        assert!(empty.results.is_empty());
    }

    #[test]
    fn test_search_refuses_a_page_past_the_window() {
        let (_tmp, st) = test_state();
        let options = SearchMemoryOptions {
            offset: Some(MAX_SEARCH_WINDOW),
            ..search_options("alpha")
        };
        let err = search_page(&st, &options, retrieval::search_history).unwrap_err();
        assert!(
            err.to_string().contains("past the first 1000 results"),
            "{err}"
        );
    }

    #[test]
    fn test_search_rejects_an_unknown_mode() {
        let (_tmp, st) = test_state();
        let options = SearchMemoryOptions {
            mode: Some("fuzzy".to_string()),
            ..search_options("alpha")
        };
        assert!(search_page(&st, &options, retrieval::search_genome).is_err());
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
