pub mod embedding;
pub mod indexer;
pub mod pageindex;
pub mod query;
pub mod store;
pub mod types;

use anyhow::Result;
use std::path::Path;
use std::process::Command;
use std::{fs::File, io::Write};

use crate::memory::Genome;
use crate::retrieval::indexer::{index_memory, index_memory_from_storage};
use crate::retrieval::store::RetrievalStore;
use crate::retrieval::types::{
    IndexScope, IndexState, InjectionStatus, RetrievalMode, RetrievalStatus, SearchBackend,
    SearchResponse,
};
use crate::state::{Config, HistoryEntry};
use crate::storage::Storage;

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp_path = path.with_extension("tmp");
    let mut file = File::create(&temp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(temp_path, path)?;
    Ok(())
}

pub fn index(
    base_path: &Path,
    history: &[HistoryEntry],
    genome: &Genome,
    config: &Config,
    scope: IndexScope,
    rebuild: bool,
) -> Result<IndexState> {
    index_memory(base_path, history, genome, config, scope, rebuild)
}

pub fn index_from_storage(
    storage: &Storage,
    config: &Config,
    scope: IndexScope,
    rebuild: bool,
) -> Result<IndexState> {
    index_memory_from_storage(storage, config, scope, rebuild)
}

pub fn search_history(
    base_path: &Path,
    config: &Config,
    query: &str,
    mode: Option<RetrievalMode>,
    backend: Option<SearchBackend>,
    limit: Option<usize>,
) -> Result<SearchResponse> {
    query::search_history(base_path, config, query, mode, backend, limit)
}

pub fn search_genome(
    base_path: &Path,
    config: &Config,
    query: &str,
    mode: Option<RetrievalMode>,
    backend: Option<SearchBackend>,
    limit: Option<usize>,
) -> Result<SearchResponse> {
    query::search_genome(base_path, config, query, mode, backend, limit)
}

pub fn status(base_path: &Path, config: &Config, check: bool) -> Result<RetrievalStatus> {
    let db_path = base_path.join("retrieval.db");
    let db_exists = db_path.exists();
    let db_size_bytes = if db_exists {
        std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    let state_path = base_path.join("retrieval_index_state.json");

    let mut index_state = if state_path.exists() {
        serde_json::from_slice::<IndexState>(&std::fs::read(&state_path)?)?
    } else {
        IndexState::default()
    };

    let vector_extension_available = if db_exists {
        RetrievalStore::open(base_path)
            .and_then(|store| {
                store.init_schema()?;
                store.try_load_vec_extension()
            })
            .unwrap_or(false)
    } else {
        false
    };

    let python_available = Command::new(&config.retrieval_python_cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let injection_log_path = base_path
        .join("context")
        .join("injections")
        .join("injection-log.jsonl");
    let mut staged_artifact_count = 0usize;
    let mut last_staged_at = None;
    let mut last_staged_surface = None;
    let mut last_staged_status = None;
    let mut last_staged_artifact = None;
    if injection_log_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&injection_log_path) {
            for line in content.lines().filter(|line| !line.trim().is_empty()) {
                if let Ok(row) = serde_json::from_str::<serde_json::Value>(line) {
                    let status = row
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if status == "staged" || status == "deduped" {
                        staged_artifact_count = staged_artifact_count.saturating_add(1);
                    }
                    last_staged_at = row
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                        .or(last_staged_at);
                    last_staged_surface = row
                        .get("surface")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                        .or(last_staged_surface);
                    last_staged_status = Some(status);
                    last_staged_artifact = row
                        .get("artifact_path")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                        .or(last_staged_artifact);
                }
            }
        }
    }

    let injection = InjectionStatus {
        config_mode: config.context_injection_mode.clone(),
        config_scope: config.context_injection_scope.clone(),
        emit_artifacts: config.context_injection_emit_artifacts,
        staged_artifact_count,
        last_staged_at,
        last_staged_surface,
        last_staged_status,
        last_staged_artifact,
    };

    let (integrity_ok, integrity_message) = if check && db_exists {
        match RetrievalStore::open(base_path).and_then(|store| {
            store.init_schema()?;
            store.quick_check()
        }) {
            Ok(msg) => {
                let ok = msg.trim().eq_ignore_ascii_case("ok");
                index_state.last_integrity_check = Some(chrono::Utc::now());
                if !ok {
                    index_state.last_error_code = Some("retrieval_db_corrupt".to_string());
                }
                if let Ok(bytes) = serde_json::to_vec_pretty(&index_state) {
                    let _ = write_atomic(&state_path, &bytes);
                }
                (Some(ok), Some(msg))
            }
            Err(e) => (Some(false), Some(e.to_string())),
        }
    } else {
        (None, None)
    };

    Ok(RetrievalStatus {
        db_path: db_path.to_string_lossy().to_string(),
        db_exists,
        db_size_bytes,
        integrity_ok,
        integrity_message,
        vector_extension_available,
        python_available,
        index_state,
        injection,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::memory::Genome;
    use crate::state::{Config, Platform};
    use chrono::{DateTime, Utc};

    #[test]
    fn test_status_default() {
        let tmp = TempDir::new().unwrap();
        let status = status(tmp.path(), &Config::default(), false).unwrap();
        assert!(status.db_path.contains("retrieval.db"));
        assert_eq!(status.index_state.history_count, 0);
        assert!(!status.db_exists);
        assert_eq!(status.injection.config_mode, "review");
        assert_eq!(status.injection.config_scope, "both");
        assert_eq!(status.injection.staged_artifact_count, 0);
    }

    #[test]
    fn test_index_empty() {
        let tmp = TempDir::new().unwrap();
        let config = Config::default();
        let genome = Genome::default();
        let idx = index(tmp.path(), &[], &genome, &config, IndexScope::All, true).unwrap();
        assert_eq!(idx.history_count, 0);
        assert_eq!(idx.genome_count, 0);
    }

    #[test]
    fn test_index_with_history_entries() {
        let tmp = TempDir::new().unwrap();
        let config = Config::default();
        let genome = Genome::default();

        let history_entry = HistoryEntry {
            session_id: "test-001".to_string(),
            session_name: "Test Session".to_string(),
            platform: Some(Platform::ClaudeCode),
            started_at: DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            ended_at: DateTime::parse_from_rfc3339("2026-01-01T11:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            summary: "Fixed authentication bug".to_string(),
            files_touched: vec!["src/auth.rs".to_string()],
            tools_used: vec!["Write".to_string(), "Edit".to_string()],
        };

        let idx = index(
            tmp.path(),
            &[history_entry],
            &genome,
            &config,
            IndexScope::History,
            true,
        )
        .unwrap();
        assert_eq!(idx.history_count, 1);
    }

    #[test]
    fn test_search_history_keyword() {
        let tmp = TempDir::new().unwrap();
        let config = Config::default();

        // First index some history
        let history_entry = HistoryEntry {
            session_id: "test-search-001".to_string(),
            session_name: "Search Test".to_string(),
            platform: Some(Platform::OpenCode),
            started_at: DateTime::parse_from_rfc3339("2026-01-15T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            ended_at: DateTime::parse_from_rfc3339("2026-01-15T11:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            summary: "Implemented search functionality".to_string(),
            files_touched: vec!["src/search.rs".to_string()],
            tools_used: vec!["Write".to_string()],
        };

        // Index and then search
        let genome = Genome::default();
        let _idx = index(
            tmp.path(),
            &[history_entry],
            &genome,
            &config,
            IndexScope::History,
            true,
        )
        .unwrap();

        // Search for the entry
        let result = search_history(tmp.path(), &config, "search", None, None, Some(5)).unwrap();
        assert!(result.candidate_count > 0);
        assert!(!result.results.is_empty());
    }

    #[test]
    fn test_search_genome_keyword() {
        let tmp = TempDir::new().unwrap();
        let config = Config::default();

        // Create genome with decisions
        let mut genome = Genome::default();
        genome.add_decision(
            "Use SQLite for storage".to_string(),
            Some("SQLite is lightweight and embedded".to_string()),
            vec![],
        );
        genome.add_decision(
            "Use tokio for async".to_string(),
            Some("tokio is the standard Rust async runtime".to_string()),
            vec![],
        );

        // Index genome
        let _idx = index(tmp.path(), &[], &genome, &config, IndexScope::Genome, true).unwrap();

        // Search for the entry
        let result = search_genome(tmp.path(), &config, "sqlite", None, None, Some(5)).unwrap();
        assert!(result.candidate_count > 0);
        assert!(!result.results.is_empty());
        // Verify we found the SQLite decision
        let found_sqlite = result.results.iter().any(|r| r.snippet.contains("SQLite"));
        assert!(found_sqlite);
    }
}
