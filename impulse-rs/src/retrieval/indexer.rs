use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::memory::{Decision, Genome};
use crate::retrieval::embedding::embed_texts;
use crate::retrieval::store::RetrievalStore;
use crate::retrieval::types::{IndexScope, IndexState};
use crate::state::{Config, HistoryEntry, Platform};
use crate::storage::Storage;

struct IndexLockGuard {
    path: PathBuf,
}

impl IndexLockGuard {
    fn acquire(base_path: &Path) -> Result<Self> {
        let path = base_path.join("retrieval.lock");
        let mut file = match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                bail!(
                    "retrieval index is already running (lock active at {}). If stale, remove the file and retry.",
                    path.display()
                );
            }
            Err(e) => {
                return Err(e).context("failed to create retrieval lock file");
            }
        };
        let lock_payload = format!(
            "{{\"pid\":{},\"created_at\":\"{}\"}}\n",
            std::process::id(),
            Utc::now().to_rfc3339()
        );
        let _ = file.write_all(lock_payload.as_bytes());
        let _ = file.sync_all();
        Ok(Self { path })
    }
}

impl Drop for IndexLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn platform_str(platform: Option<Platform>) -> String {
    match platform {
        Some(Platform::ClaudeCode) => "claude-code".to_string(),
        Some(Platform::OpenCode) => "opencode".to_string(),
        None => "unknown".to_string(),
    }
}

fn history_hash(entry: &HistoryEntry) -> String {
    sha256_hex(&[
        &entry.session_id,
        &entry.session_name,
        &platform_str(entry.platform),
        &entry.started_at.to_rfc3339(),
        &entry.ended_at.to_rfc3339(),
        &entry.summary,
        &entry.files_touched.join("|"),
        &entry.tools_used.join("|"),
    ])
}

fn genome_id(decision: &Decision) -> String {
    format!("{}-{}", decision.date.timestamp(), decision.description)
}

fn genome_hash(decision: &Decision) -> String {
    sha256_hex(&[
        &decision.date.to_rfc3339(),
        &decision.description,
        decision.rationale.as_deref().unwrap_or_default(),
        &decision.tags.join("|"),
    ])
}

fn history_search_text(entry: &HistoryEntry) -> String {
    format!(
        "{} {} {} {} {}",
        entry.session_name,
        entry.summary,
        entry.files_touched.join(" "),
        entry.tools_used.join(" "),
        platform_str(entry.platform)
    )
}

fn genome_search_text(decision: &Decision) -> String {
    format!(
        "{} {} {}",
        decision.description,
        decision.rationale.clone().unwrap_or_default(),
        decision.tags.join(" ")
    )
}

fn embed_history_jobs(
    store: &RetrievalStore,
    config: &Config,
    jobs: &[(String, String)],
    vec_available: bool,
    notes: &mut Vec<String>,
) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let batch_size = config.retrieval_batch_size.max(1);
    for chunk in jobs.chunks(batch_size) {
        let texts: Vec<String> = chunk.iter().map(|(_, text)| text.clone()).collect();
        match embed_texts(config, &texts, config.retrieval_index_timeout_secs) {
            Ok(vectors) => {
                if vec_available {
                    if let Some(first) = vectors.first() {
                        store.ensure_history_vec0_table(first.len())?;
                    }
                }
                store.with_transaction(|_tx| {
                    for ((id, _), vec) in chunk.iter().zip(vectors.iter()) {
                        let vector_json = serde_json::to_string(vec)?;
                        store.upsert_history_vector(id, &vector_json)?;
                        if vec_available {
                            store.upsert_history_vector_vec0(id, &vector_json)?;
                        }
                    }
                    Ok(())
                })?;
            }
            Err(e) => {
                notes.push(format!("history vector indexing skipped: {}", e));
                break;
            }
        }
    }
    Ok(())
}

fn embed_genome_jobs(
    store: &RetrievalStore,
    config: &Config,
    jobs: &[(String, String)],
    vec_available: bool,
    notes: &mut Vec<String>,
) -> Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let batch_size = config.retrieval_batch_size.max(1);
    for chunk in jobs.chunks(batch_size) {
        let texts: Vec<String> = chunk.iter().map(|(_, text)| text.clone()).collect();
        match embed_texts(config, &texts, config.retrieval_index_timeout_secs) {
            Ok(vectors) => {
                if vec_available {
                    if let Some(first) = vectors.first() {
                        store.ensure_genome_vec0_table(first.len())?;
                    }
                }
                store.with_transaction(|_tx| {
                    for ((id, _), vec) in chunk.iter().zip(vectors.iter()) {
                        let vector_json = serde_json::to_string(vec)?;
                        store.upsert_genome_vector(id, &vector_json)?;
                        if vec_available {
                            store.upsert_genome_vector_vec0(id, &vector_json)?;
                        }
                    }
                    Ok(())
                })?;
            }
            Err(e) => {
                notes.push(format!("genome vector indexing skipped: {}", e));
                break;
            }
        }
    }
    Ok(())
}

pub fn index_memory(
    base_path: &Path,
    history: &[HistoryEntry],
    genome: &Genome,
    config: &Config,
    scope: IndexScope,
    rebuild: bool,
) -> Result<IndexState> {
    let _lock = IndexLockGuard::acquire(base_path)?;
    let started = Instant::now();

    let store = RetrievalStore::open(base_path)?;
    store.init_schema()?;
    if rebuild {
        store.clear_all()?;
    }

    let mut notes = Vec::new();
    let mut history_count = 0usize;
    let mut genome_count = 0usize;
    let mut history_seen = HashSet::new();
    let mut genome_seen = HashSet::new();
    let mut history_embed_jobs: Vec<(String, String)> = Vec::new();
    let mut genome_embed_jobs: Vec<(String, String)> = Vec::new();
    let vector_enabled = config.retrieval_vector_enabled && config.retrieval_backend == "fts+vec";
    let mut vector_available = false;
    if vector_enabled {
        vector_available = store.try_load_vec_extension(None).unwrap_or(false);
        if !vector_available {
            notes.push(
                "sqlite-vec extension unavailable; semantic sqlite path will fall back to rust-cosine/keyword"
                    .to_string(),
            );
        }
    }

    if matches!(scope, IndexScope::History | IndexScope::All) {
        store.with_transaction(|_tx| {
            for h in history {
                let id = h.session_id.clone();
                let hash = history_hash(h);
                let existing_hash = store.get_history_hash(&id)?;
                let changed = rebuild || existing_hash.as_deref() != Some(hash.as_str());
                let vector_missing = !store.has_history_vector(&id)?
                    || (vector_available && !store.has_history_vec0(&id)?);

                if changed {
                    store.delete_history_vector(&id)?;
                    let files_json = serde_json::to_string(&h.files_touched)?;
                    let tools_json = serde_json::to_string(&h.tools_used)?;
                    let search_text = history_search_text(h);
                    store.upsert_history(
                        &id,
                        &h.session_name,
                        Some(&platform_str(h.platform)),
                        &h.started_at.to_rfc3339(),
                        &h.ended_at.to_rfc3339(),
                        &h.summary,
                        &files_json,
                        &tools_json,
                        &search_text,
                        &hash,
                    )?;
                }

                if config.retrieval_vector_enabled
                    && config.retrieval_backend == "fts+vec"
                    && (changed || vector_missing)
                {
                    history_embed_jobs.push((id.clone(), history_search_text(h)));
                }

                history_seen.insert(id);
                history_count += 1;
            }
            Ok(())
        })?;
        store.delete_history_except(&history_seen)?;
    }

    if matches!(scope, IndexScope::Genome | IndexScope::All) {
        store.with_transaction(|_tx| {
            for d in &genome.decisions {
                let id = genome_id(d);
                let hash = genome_hash(d);
                let existing_hash = store.get_genome_hash(&id)?;
                let changed = rebuild || existing_hash.as_deref() != Some(hash.as_str());
                let vector_missing = !store.has_genome_vector(&id)?
                    || (vector_available && !store.has_genome_vec0(&id)?);

                if changed {
                    store.delete_genome_vector(&id)?;
                    let tags_json = serde_json::to_string(&d.tags)?;
                    let search_text = genome_search_text(d);
                    store.upsert_genome(
                        &id,
                        &d.date.to_rfc3339(),
                        &d.description,
                        d.rationale.as_deref(),
                        &tags_json,
                        &search_text,
                        &hash,
                    )?;
                }

                if config.retrieval_vector_enabled
                    && config.retrieval_backend == "fts+vec"
                    && (changed || vector_missing)
                {
                    genome_embed_jobs.push((id.clone(), genome_search_text(d)));
                }

                genome_seen.insert(id);
                genome_count += 1;
            }
            Ok(())
        })?;
        store.delete_genome_except(&genome_seen)?;
    }

    store.refresh_fts()?;

    if vector_enabled {
        embed_history_jobs(
            &store,
            config,
            &history_embed_jobs,
            vector_available,
            &mut notes,
        )?;
        embed_genome_jobs(
            &store,
            config,
            &genome_embed_jobs,
            vector_available,
            &mut notes,
        )?;
    }

    let mut backend_health = Vec::new();
    if vector_enabled {
        backend_health.push(format!(
            "sqlite_vec_extension_available={}",
            vector_available
        ));
    } else {
        backend_health.push("vector_backend_disabled=true".to_string());
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let state = IndexState {
        version: "1".to_string(),
        schema_version: 2,
        indexed_at: Utc::now(),
        history_count,
        genome_count,
        vector_enabled,
        vector_available,
        last_index_duration_ms: duration_ms,
        last_integrity_check: None,
        last_error_code: None,
        backend_health,
        notes,
    };

    let state_path = base_path.join("retrieval_index_state.json");
    Storage::atomic_write_path(&state_path, &serde_json::to_vec_pretty(&state)?)?;

    let _ = std::fs::create_dir_all(base_path.join("embeddings"));
    let marker_path = base_path.join("embeddings/index-meta.json");
    let marker = json!({
        "indexed_at": state.indexed_at.to_rfc3339(),
        "history_count": state.history_count,
        "genome_count": state.genome_count,
        "vector_enabled": state.vector_enabled,
        "vector_available": state.vector_available,
        "last_index_duration_ms": state.last_index_duration_ms,
    });
    Storage::atomic_write_path(&marker_path, &serde_json::to_vec_pretty(&marker)?)?;

    Ok(state)
}

pub fn index_memory_from_storage(
    storage: &Storage,
    config: &Config,
    scope: IndexScope,
    rebuild: bool,
) -> Result<IndexState> {
    let _lock = IndexLockGuard::acquire(storage.base_path())?;
    let started = Instant::now();

    let store = RetrievalStore::open(storage.base_path())?;
    store.init_schema()?;
    if rebuild {
        store.clear_all()?;
    }

    let mut notes = Vec::new();
    let mut history_count = 0usize;
    let mut genome_count = 0usize;
    let mut history_seen = HashSet::new();
    let mut genome_seen = HashSet::new();
    let mut history_embed_jobs: Vec<(String, String)> = Vec::new();
    let mut genome_embed_jobs: Vec<(String, String)> = Vec::new();
    let vector_enabled = config.retrieval_vector_enabled && config.retrieval_backend == "fts+vec";
    let mut vector_available = false;
    if vector_enabled {
        vector_available = store.try_load_vec_extension(None).unwrap_or(false);
        if !vector_available {
            notes.push(
                "sqlite-vec extension unavailable; semantic sqlite path will fall back to rust-cosine/keyword"
                    .to_string(),
            );
        }
    }

    if matches!(scope, IndexScope::History | IndexScope::All) {
        store.with_transaction(|_tx| {
            let _ = storage.read_jsonl_stream::<HistoryEntry, _>("HISTORY.jsonl", |h| {
                let id = h.session_id.clone();
                let hash = history_hash(&h);
                let existing_hash = store.get_history_hash(&id)?;
                let changed = rebuild || existing_hash.as_deref() != Some(hash.as_str());
                let vector_missing = !store.has_history_vector(&id)?
                    || (vector_available && !store.has_history_vec0(&id)?);

                if changed {
                    store.delete_history_vector(&id)?;
                    let files_json = serde_json::to_string(&h.files_touched)?;
                    let tools_json = serde_json::to_string(&h.tools_used)?;
                    let search_text = history_search_text(&h);
                    store.upsert_history(
                        &id,
                        &h.session_name,
                        Some(&platform_str(h.platform)),
                        &h.started_at.to_rfc3339(),
                        &h.ended_at.to_rfc3339(),
                        &h.summary,
                        &files_json,
                        &tools_json,
                        &search_text,
                        &hash,
                    )?;
                }

                if config.retrieval_vector_enabled
                    && config.retrieval_backend == "fts+vec"
                    && (changed || vector_missing)
                {
                    history_embed_jobs.push((id.clone(), history_search_text(&h)));
                }

                history_seen.insert(id);
                history_count += 1;
                Ok(())
            })?;
            Ok(())
        })?;
        store.delete_history_except(&history_seen)?;
    }

    if matches!(scope, IndexScope::Genome | IndexScope::All) {
        let genome: Genome = storage.read_json("GENOME.md")?;
        store.with_transaction(|_tx| {
            for d in &genome.decisions {
                let id = genome_id(d);
                let hash = genome_hash(d);
                let existing_hash = store.get_genome_hash(&id)?;
                let changed = rebuild || existing_hash.as_deref() != Some(hash.as_str());
                let vector_missing = !store.has_genome_vector(&id)?
                    || (vector_available && !store.has_genome_vec0(&id)?);

                if changed {
                    store.delete_genome_vector(&id)?;
                    let tags_json = serde_json::to_string(&d.tags)?;
                    let search_text = genome_search_text(d);
                    store.upsert_genome(
                        &id,
                        &d.date.to_rfc3339(),
                        &d.description,
                        d.rationale.as_deref(),
                        &tags_json,
                        &search_text,
                        &hash,
                    )?;
                }

                if config.retrieval_vector_enabled
                    && config.retrieval_backend == "fts+vec"
                    && (changed || vector_missing)
                {
                    genome_embed_jobs.push((id.clone(), genome_search_text(d)));
                }

                genome_seen.insert(id);
                genome_count += 1;
            }
            Ok(())
        })?;
        store.delete_genome_except(&genome_seen)?;
    }

    store.refresh_fts()?;

    if vector_enabled {
        embed_history_jobs(
            &store,
            config,
            &history_embed_jobs,
            vector_available,
            &mut notes,
        )?;
        embed_genome_jobs(
            &store,
            config,
            &genome_embed_jobs,
            vector_available,
            &mut notes,
        )?;
    }

    let mut backend_health = Vec::new();
    if vector_enabled {
        backend_health.push(format!(
            "sqlite_vec_extension_available={}",
            vector_available
        ));
    } else {
        backend_health.push("vector_backend_disabled=true".to_string());
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let state = IndexState {
        version: "1".to_string(),
        schema_version: 2,
        indexed_at: Utc::now(),
        history_count,
        genome_count,
        vector_enabled,
        vector_available,
        last_index_duration_ms: duration_ms,
        last_integrity_check: None,
        last_error_code: None,
        backend_health,
        notes,
    };

    let state_path = storage.base_path().join("retrieval_index_state.json");
    Storage::atomic_write_path(&state_path, &serde_json::to_vec_pretty(&state)?)?;

    let _ = std::fs::create_dir_all(storage.base_path().join("embeddings"));
    let marker_path = storage.base_path().join("embeddings/index-meta.json");
    let marker = json!({
        "indexed_at": state.indexed_at.to_rfc3339(),
        "history_count": state.history_count,
        "genome_count": state.genome_count,
        "vector_enabled": state.vector_enabled,
        "vector_available": state.vector_available,
        "last_index_duration_ms": state.last_index_duration_ms,
    });
    Storage::atomic_write_path(&marker_path, &serde_json::to_vec_pretty(&marker)?)?;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sample_history(summary: &str) -> HistoryEntry {
        let started = Utc::now() - Duration::minutes(1);
        HistoryEntry {
            session_id: "history-incremental-1".to_string(),
            session_name: "incremental".to_string(),
            platform: Some(Platform::ClaudeCode),
            started_at: started,
            ended_at: Utc::now(),
            summary: summary.to_string(),
            files_touched: vec!["src/main.rs".to_string()],
            tools_used: vec!["Write".to_string()],
        }
    }

    #[test]
    #[ignore = "requires vector embedding which is disabled in this environment"]
    fn test_changed_rows_invalidate_existing_vectors_before_reembed() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        std::env::set_var("IMPULSE_EMBED_ALLOW_FAKE", "1");
        let cfg = Config {
            retrieval_backend: "fts+vec".to_string(),
            retrieval_vector_enabled: true,
            ..Config::default()
        };

        let genome = Genome::default();
        let original = sample_history("original-summary");
        index_memory(
            temp.path(),
            std::slice::from_ref(&original),
            &genome,
            &cfg,
            IndexScope::History,
            true,
        )
        .unwrap();

        let store = RetrievalStore::open(temp.path()).unwrap();
        store.init_schema().unwrap();
        assert!(store.has_history_vector(&original.session_id).unwrap());

        let mut failing_cfg = cfg.clone();
        failing_cfg.retrieval_python_cmd = "false".to_string();
        let changed = sample_history("changed-summary");
        let _ = index_memory(
            temp.path(),
            &[changed],
            &genome,
            &failing_cfg,
            IndexScope::History,
            false,
        )
        .unwrap();

        let store_after = RetrievalStore::open(temp.path()).unwrap();
        store_after.init_schema().unwrap();
        assert!(
            !store_after
                .has_history_vector(&original.session_id)
                .unwrap(),
            "vector must be removed when changed row re-embed fails"
        );
        std::env::remove_var("IMPULSE_EMBED_ALLOW_FAKE");
    }

    #[test]
    fn test_index_succeeds_with_vector_disabled() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        let cfg = Config {
            retrieval_vector_enabled: false,
            retrieval_backend: "fts".to_string(),
            ..Config::default()
        };

        let history = vec![sample_history("vector-disabled test session")];
        let genome = Genome::default();

        let state =
            index_memory(temp.path(), &history, &genome, &cfg, IndexScope::All, true).unwrap();

        assert_eq!(state.history_count, 1);
        assert!(!state.vector_enabled);
        assert!(!state.vector_available);
        assert!(
            state
                .backend_health
                .iter()
                .any(|h| h.contains("vector_backend_disabled=true")),
            "backend health should note vector is disabled"
        );

        // FTS should still be populated.
        let store = RetrievalStore::open(temp.path()).unwrap();
        store.init_schema().unwrap();
        let results = store.search_history_keyword("vector-disabled", 10).unwrap();
        assert_eq!(results.len(), 1, "FTS search should find the entry");
    }

    #[test]
    fn test_index_succeeds_when_embed_script_missing() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        // Point to a nonexistent script so embedding will fail with MissingScript.
        std::env::set_var("IMPULSE_EMBED_SCRIPT", "/tmp/nonexistent_embed_script.py");
        let cfg = Config {
            retrieval_vector_enabled: true,
            retrieval_backend: "fts+vec".to_string(),
            ..Config::default()
        };

        let history = vec![sample_history("embed-fallback session")];
        let genome = Genome::default();

        let state =
            index_memory(temp.path(), &history, &genome, &cfg, IndexScope::All, true).unwrap();
        std::env::remove_var("IMPULSE_EMBED_SCRIPT");

        // Indexing should succeed — document stored, vector skipped.
        assert_eq!(state.history_count, 1);
        assert!(state.vector_enabled);

        // Notes should mention the skip.
        let has_skip_note = state
            .notes
            .iter()
            .any(|n| n.contains("vector indexing skipped") || n.contains("extension unavailable"));
        assert!(
            has_skip_note,
            "notes should record embedding failure: {:?}",
            state.notes
        );

        // FTS search should still work.
        let store = RetrievalStore::open(temp.path()).unwrap();
        store.init_schema().unwrap();
        let results = store.search_history_keyword("embed-fallback", 10).unwrap();
        assert_eq!(
            results.len(),
            1,
            "FTS should still find entry without vectors"
        );
    }

    #[test]
    fn test_index_genome_without_vectors() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        let cfg = Config {
            retrieval_vector_enabled: false,
            ..Config::default()
        };

        let genome = Genome {
            decisions: vec![Decision {
                date: Utc::now(),
                description: "Use Rust for all new services".to_string(),
                rationale: Some("Performance and safety".to_string()),
                tags: vec!["architecture".to_string()],
            }],
            ..Genome::default()
        };

        let state =
            index_memory(temp.path(), &[], &genome, &cfg, IndexScope::Genome, true).unwrap();

        assert_eq!(state.genome_count, 1);
        assert!(!state.vector_enabled);

        // Keyword search should find the genome entry.
        let store = RetrievalStore::open(temp.path()).unwrap();
        store.init_schema().unwrap();
        let results = store.search_genome_keyword("architecture", 10).unwrap();
        assert_eq!(
            results.len(),
            1,
            "FTS should find genome entry without vectors"
        );
    }

    #[test]
    fn test_incremental_index_preserves_existing_fts() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        let cfg = Config {
            retrieval_vector_enabled: false,
            ..Config::default()
        };

        let h1 = HistoryEntry {
            session_id: "session-first".to_string(),
            session_name: "first-run".to_string(),
            platform: Some(Platform::ClaudeCode),
            started_at: Utc::now() - Duration::minutes(10),
            ended_at: Utc::now() - Duration::minutes(5),
            summary: "Initial scaffolding of project".to_string(),
            files_touched: vec!["main.rs".to_string()],
            tools_used: vec!["Write".to_string()],
        };

        // First index — rebuild.
        index_memory(
            temp.path(),
            std::slice::from_ref(&h1),
            &Genome::default(),
            &cfg,
            IndexScope::History,
            true,
        )
        .unwrap();

        let h2 = HistoryEntry {
            session_id: "session-second".to_string(),
            session_name: "second-run".to_string(),
            platform: Some(Platform::OpenCode),
            started_at: Utc::now() - Duration::minutes(4),
            ended_at: Utc::now(),
            summary: "Added error handling module".to_string(),
            files_touched: vec!["error.rs".to_string()],
            tools_used: vec!["Edit".to_string()],
        };

        // Second index — incremental (rebuild=false).
        let state = index_memory(
            temp.path(),
            &[h1, h2],
            &Genome::default(),
            &cfg,
            IndexScope::History,
            false,
        )
        .unwrap();

        assert_eq!(state.history_count, 2);

        // Both entries should be searchable.
        let store = RetrievalStore::open(temp.path()).unwrap();
        store.init_schema().unwrap();
        let r1 = store.search_history_keyword("scaffolding", 10).unwrap();
        assert_eq!(
            r1.len(),
            1,
            "first entry still searchable after incremental"
        );
        let r2 = store.search_history_keyword("error handling", 10).unwrap();
        assert_eq!(r2.len(), 1, "second entry searchable after incremental");
    }

    #[test]
    fn test_empty_inputs_produce_zero_counts() {
        let _guard = env_lock().lock().unwrap();
        let temp = TempDir::new().unwrap();

        let cfg = Config::default();
        let state = index_memory(
            temp.path(),
            &[],
            &Genome::default(),
            &cfg,
            IndexScope::All,
            true,
        )
        .unwrap();

        assert_eq!(state.history_count, 0);
        assert_eq!(state.genome_count, 0);
    }
}
