use anyhow::Result;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use crate::memory::Genome;
use crate::retrieval::embedding::{embed_texts, EmbeddingFailureKind};
use crate::retrieval::store::RetrievalStore;
use crate::retrieval::types::{
    FallbackCode, RetrievalMode, SearchBackend, SearchResponse, SearchResult,
};
use crate::state::{Config, HistoryEntry};
use crate::storage::Storage;

pub fn highlight_matches(text: &str, _query: &str) -> String {
    text.to_string()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

fn resolve_mode(config: &Config, mode_override: Option<RetrievalMode>) -> RetrievalMode {
    if let Some(m) = mode_override {
        return m;
    }
    RetrievalMode::parse(&config.retrieval_mode).unwrap_or(RetrievalMode::Keyword)
}

fn resolve_semantic_backends(
    config: &Config,
    backend_override: Option<SearchBackend>,
) -> Vec<SearchBackend> {
    if let Some(backend) = backend_override {
        return match backend {
            SearchBackend::Auto => vec![SearchBackend::SqliteVec, SearchBackend::RustCosine],
            SearchBackend::Keyword => vec![SearchBackend::Keyword],
            SearchBackend::SqliteVec => vec![SearchBackend::SqliteVec],
            SearchBackend::RustCosine => vec![SearchBackend::RustCosine],
        };
    }

    match config.retrieval_semantic_strategy.as_str() {
        "sqlite-only" => vec![SearchBackend::SqliteVec],
        "rust-only" => vec![SearchBackend::RustCosine],
        _ => vec![SearchBackend::SqliteVec, SearchBackend::RustCosine],
    }
}

fn keyword_response(
    mode: RetrievalMode,
    results: Vec<SearchResult>,
    used_fallback: bool,
    fallback_code: Option<FallbackCode>,
    fallback_reason: Option<String>,
    started: Instant,
    mut engine_notes: Vec<String>,
) -> SearchResponse {
    if engine_notes.is_empty() {
        engine_notes.push("fts5 keyword search".to_string());
    }
    SearchResponse {
        mode: mode.as_str().to_string(),
        used_fallback,
        fallback_reason,
        fallback_code,
        backend_used: SearchBackend::Keyword.as_str().to_string(),
        timing_ms: started.elapsed().as_millis() as u64,
        candidate_count: results.len(),
        engine_notes,
        results,
        total_count: None,
    }
}

fn embedding_failure_to_fallback(kind: EmbeddingFailureKind) -> FallbackCode {
    match kind {
        EmbeddingFailureKind::Timeout => FallbackCode::EmbeddingTimeout,
        EmbeddingFailureKind::SpawnFailed => FallbackCode::EmbeddingSpawnFailed,
        EmbeddingFailureKind::DimMismatch => FallbackCode::EmbeddingDimensionMismatch,
        EmbeddingFailureKind::CountMismatch => FallbackCode::EmbeddingDimensionMismatch,
        EmbeddingFailureKind::MissingScript
        | EmbeddingFailureKind::StdinWriteFailed
        | EmbeddingFailureKind::ProcessFailed
        | EmbeddingFailureKind::InvalidOutput => FallbackCode::EmbeddingProcessFailed,
    }
}

fn distance_to_similarity(distance: f64) -> f32 {
    1.0 / (1.0 + distance.max(0.0) as f32)
}

fn file_fallback_history(base_path: &Path, query: &str, limit: usize) -> Vec<SearchResult> {
    let storage = Storage::new(base_path.to_path_buf());
    let q = query.to_lowercase();
    let mut rows = storage
        .read_jsonl::<HistoryEntry>("HISTORY.jsonl")
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| {
            entry.session_name.to_lowercase().contains(&q)
                || entry.summary.to_lowercase().contains(&q)
                || entry
                    .files_touched
                    .iter()
                    .any(|f| f.to_lowercase().contains(&q))
                || entry
                    .tools_used
                    .iter()
                    .any(|t| t.to_lowercase().contains(&q))
        })
        .collect::<Vec<_>>();

    rows.sort_by_key(|b| std::cmp::Reverse(b.ended_at));
    rows.into_iter()
        .take(limit)
        .map(|entry| SearchResult {
            source: "history".to_string(),
            id: entry.session_id,
            title: entry.session_name,
            snippet: entry.summary,
            score: 0.0,
        })
        .collect()
}

fn file_fallback_genome(base_path: &Path, query: &str, limit: usize) -> Vec<SearchResult> {
    let storage = Storage::new(base_path.to_path_buf());
    let genome = storage.read_json::<Genome>("GENOME.md").unwrap_or_default();
    let q = query.to_lowercase();
    let mut rows = genome
        .decisions
        .into_iter()
        .filter(|decision| {
            decision.description.to_lowercase().contains(&q)
                || decision
                    .rationale
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&q)
                || decision.tags.iter().any(|t| t.to_lowercase().contains(&q))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|b| std::cmp::Reverse(b.date));

    rows.into_iter()
        .take(limit)
        .map(|decision| SearchResult {
            source: "genome".to_string(),
            id: format!("{}-{}", decision.date.timestamp(), decision.description),
            title: decision.description,
            snippet: decision.rationale.unwrap_or_default(),
            score: 0.0,
        })
        .collect()
}

fn semantic_history_rust(
    store: &RetrievalStore,
    config: &Config,
    query: &str,
    limit: usize,
    backend: SearchBackend,
    started: Instant,
    mut engine_notes: Vec<String>,
) -> Result<SearchResponse> {
    let query_vec = match embed_texts(
        config,
        &[query.to_string()],
        config.retrieval_query_timeout_secs,
    ) {
        Ok(v) if !v.is_empty() => v[0].clone(),
        Ok(_) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(
                    "embedding returned no vector; used keyword fallback".to_string(),
                ),
                fallback_code: Some(FallbackCode::EmbeddingNoVector),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_history_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
        Err(e) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(format!("embedding failed: {}; used keyword fallback", e)),
                fallback_code: Some(embedding_failure_to_fallback(e.kind)),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_history_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
    };

    let candidate_limit = config.retrieval_candidate_pool.max(10);
    let keyword_candidates = store
        .search_history_keyword(query, candidate_limit)
        .unwrap_or_default();
    let candidate_ids: Option<HashSet<String>> = if keyword_candidates.is_empty() {
        None
    } else {
        Some(keyword_candidates.into_iter().map(|r| r.id).collect())
    };

    let mut vectors = store.read_history_vectors()?;
    if let Some(ids) = &candidate_ids {
        vectors.retain(|(id, _)| ids.contains(id));
        engine_notes.push(format!("candidate_pool_applied={}", ids.len()));
    } else {
        engine_notes.push("candidate_pool_applied=all_vectors".to_string());
    }

    let candidate_count = vectors.len();
    let mut scored: Vec<(String, f32)> = vectors
        .into_iter()
        .map(|(id, vec)| (id, cosine_similarity(&query_vec, &vec)))
        .filter(|(_, s)| *s >= config.retrieval_similarity_threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    let mut results = Vec::new();
    for (id, score) in scored.into_iter().take(limit) {
        if let Some((title, snippet)) = store.get_history_by_id(&id)? {
            results.push(SearchResult {
                source: "history".to_string(),
                id,
                title,
                snippet,
                score: score as f64,
            });
        }
    }

    Ok(SearchResponse {
        mode: "semantic".to_string(),
        used_fallback: false,
        fallback_reason: None,
        fallback_code: None,
        backend_used: backend.as_str().to_string(),
        timing_ms: started.elapsed().as_millis() as u64,
        candidate_count,
        engine_notes,
        results,
        total_count: None,
    })
}

fn semantic_history_sqlite_vec(
    store: &RetrievalStore,
    config: &Config,
    query: &str,
    limit: usize,
    backend: SearchBackend,
    started: Instant,
    mut engine_notes: Vec<String>,
) -> Result<SearchResponse> {
    let query_vec = match embed_texts(
        config,
        &[query.to_string()],
        config.retrieval_query_timeout_secs,
    ) {
        Ok(v) if !v.is_empty() => v[0].clone(),
        Ok(_) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(
                    "embedding returned no vector; used keyword fallback".to_string(),
                ),
                fallback_code: Some(FallbackCode::EmbeddingNoVector),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_history_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
        Err(e) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(format!("embedding failed: {}; used keyword fallback", e)),
                fallback_code: Some(embedding_failure_to_fallback(e.kind)),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_history_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
    };

    let candidate_limit = config.retrieval_candidate_pool.max(limit.max(10));
    let query_json = serde_json::to_string(&query_vec)?;
    let candidates = store.search_history_vec_knn(&query_json, candidate_limit)?;
    engine_notes.push(format!("candidate_pool_applied={}", candidate_limit));

    let candidate_count = candidates.len();
    let mut results = Vec::new();
    for (id, distance) in candidates {
        let similarity = distance_to_similarity(distance);
        if similarity < config.retrieval_similarity_threshold {
            continue;
        }
        if let Some((title, snippet)) = store.get_history_by_id(&id)? {
            results.push(SearchResult {
                source: "history".to_string(),
                id,
                title,
                snippet,
                score: similarity as f64,
            });
        }
        if results.len() >= limit {
            break;
        }
    }

    Ok(SearchResponse {
        mode: "semantic".to_string(),
        used_fallback: false,
        fallback_reason: None,
        fallback_code: None,
        backend_used: backend.as_str().to_string(),
        timing_ms: started.elapsed().as_millis() as u64,
        candidate_count,
        engine_notes,
        results,
        total_count: None,
    })
}

fn semantic_genome_rust(
    store: &RetrievalStore,
    config: &Config,
    query: &str,
    limit: usize,
    backend: SearchBackend,
    started: Instant,
    mut engine_notes: Vec<String>,
) -> Result<SearchResponse> {
    let query_vec = match embed_texts(
        config,
        &[query.to_string()],
        config.retrieval_query_timeout_secs,
    ) {
        Ok(v) if !v.is_empty() => v[0].clone(),
        Ok(_) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(
                    "embedding returned no vector; used keyword fallback".to_string(),
                ),
                fallback_code: Some(FallbackCode::EmbeddingNoVector),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_genome_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
        Err(e) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(format!("embedding failed: {}; used keyword fallback", e)),
                fallback_code: Some(embedding_failure_to_fallback(e.kind)),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_genome_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
    };

    let candidate_limit = config.retrieval_candidate_pool.max(10);
    let keyword_candidates = store
        .search_genome_keyword(query, candidate_limit)
        .unwrap_or_default();
    let candidate_ids: Option<HashSet<String>> = if keyword_candidates.is_empty() {
        None
    } else {
        Some(keyword_candidates.into_iter().map(|r| r.id).collect())
    };

    let mut vectors = store.read_genome_vectors()?;
    if let Some(ids) = &candidate_ids {
        vectors.retain(|(id, _)| ids.contains(id));
        engine_notes.push(format!("candidate_pool_applied={}", ids.len()));
    } else {
        engine_notes.push("candidate_pool_applied=all_vectors".to_string());
    }

    let candidate_count = vectors.len();
    let mut scored: Vec<(String, f32)> = vectors
        .into_iter()
        .map(|(id, vec)| (id, cosine_similarity(&query_vec, &vec)))
        .filter(|(_, s)| *s >= config.retrieval_similarity_threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    let mut results = Vec::new();
    for (id, score) in scored.into_iter().take(limit) {
        if let Some((title, snippet)) = store.get_genome_by_id(&id)? {
            results.push(SearchResult {
                source: "genome".to_string(),
                id,
                title,
                snippet,
                score: score as f64,
            });
        }
    }

    Ok(SearchResponse {
        mode: "semantic".to_string(),
        used_fallback: false,
        fallback_reason: None,
        fallback_code: None,
        backend_used: backend.as_str().to_string(),
        timing_ms: started.elapsed().as_millis() as u64,
        candidate_count,
        engine_notes,
        results,
        total_count: None,
    })
}

fn semantic_genome_sqlite_vec(
    store: &RetrievalStore,
    config: &Config,
    query: &str,
    limit: usize,
    backend: SearchBackend,
    started: Instant,
    mut engine_notes: Vec<String>,
) -> Result<SearchResponse> {
    let query_vec = match embed_texts(
        config,
        &[query.to_string()],
        config.retrieval_query_timeout_secs,
    ) {
        Ok(v) if !v.is_empty() => v[0].clone(),
        Ok(_) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(
                    "embedding returned no vector; used keyword fallback".to_string(),
                ),
                fallback_code: Some(FallbackCode::EmbeddingNoVector),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_genome_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
        Err(e) => {
            return Ok(SearchResponse {
                mode: "semantic".to_string(),
                used_fallback: true,
                fallback_reason: Some(format!("embedding failed: {}; used keyword fallback", e)),
                fallback_code: Some(embedding_failure_to_fallback(e.kind)),
                backend_used: SearchBackend::Keyword.as_str().to_string(),
                timing_ms: started.elapsed().as_millis() as u64,
                candidate_count: 0,
                total_count: None,
                engine_notes,
                results: store
                    .search_genome_keyword(query, limit)
                    .unwrap_or_default(),
            });
        }
    };

    let candidate_limit = config.retrieval_candidate_pool.max(limit.max(10));
    let query_json = serde_json::to_string(&query_vec)?;
    let candidates = store.search_genome_vec_knn(&query_json, candidate_limit)?;
    engine_notes.push(format!("candidate_pool_applied={}", candidate_limit));

    let candidate_count = candidates.len();
    let mut results = Vec::new();
    for (id, distance) in candidates {
        let similarity = distance_to_similarity(distance);
        if similarity < config.retrieval_similarity_threshold {
            continue;
        }
        if let Some((title, snippet)) = store.get_genome_by_id(&id)? {
            results.push(SearchResult {
                source: "genome".to_string(),
                id,
                title,
                snippet,
                score: similarity as f64,
            });
        }
        if results.len() >= limit {
            break;
        }
    }

    Ok(SearchResponse {
        mode: "semantic".to_string(),
        used_fallback: false,
        fallback_reason: None,
        fallback_code: None,
        backend_used: backend.as_str().to_string(),
        timing_ms: started.elapsed().as_millis() as u64,
        candidate_count,
        engine_notes,
        results,
        total_count: None,
    })
}

pub fn search_history(
    base_path: &Path,
    config: &Config,
    query: &str,
    mode_override: Option<RetrievalMode>,
    backend_override: Option<SearchBackend>,
    limit_override: Option<usize>,
    offset_override: Option<usize>,
) -> Result<SearchResponse> {
    let started = Instant::now();
    let limit = limit_override.unwrap_or(config.retrieval_default_limit.max(1));
    let offset = offset_override.unwrap_or(0);
    let _ = offset; // offset parameter reserved for future use
    let mode = resolve_mode(config, mode_override);

    if query.trim().is_empty() {
        return Ok(keyword_response(
            mode,
            Vec::new(),
            false,
            None,
            None,
            started,
            vec!["empty query".to_string()],
        ));
    }

    let store = match RetrievalStore::open(base_path).and_then(|s| {
        s.init_schema()?;
        Ok(s)
    }) {
        Ok(s) => s,
        Err(_) => {
            let rows = file_fallback_history(base_path, query, limit);
            return Ok(keyword_response(
                mode,
                rows,
                true,
                Some(FallbackCode::RetrievalDbCorrupt),
                Some("retrieval db unavailable/corrupt; used file fallback".to_string()),
                started,
                vec!["history.jsonl scan fallback".to_string()],
            ));
        }
    };

    if matches!(mode, RetrievalMode::Keyword)
        || matches!(backend_override, Some(SearchBackend::Keyword))
    {
        let (results, used_fallback, fallback_code, fallback_reason, notes) =
            match store.search_history_keyword(query, limit) {
                Ok(rows) => (
                    rows,
                    false,
                    None,
                    None,
                    vec!["fts5 keyword search".to_string()],
                ),
                Err(e) => (
                    file_fallback_history(base_path, query, limit),
                    true,
                    Some(FallbackCode::RetrievalDbError),
                    Some(format!(
                        "keyword retrieval db error: {}; used file fallback",
                        e
                    )),
                    vec!["history.jsonl scan fallback".to_string()],
                ),
            };
        return Ok(keyword_response(
            RetrievalMode::Keyword,
            results,
            used_fallback,
            fallback_code,
            fallback_reason,
            started,
            notes,
        ));
    }

    if !(config.retrieval_vector_enabled && config.retrieval_backend == "fts+vec") {
        let results = store
            .search_history_keyword(query, limit)
            .unwrap_or_else(|_| file_fallback_history(base_path, query, limit));
        return Ok(keyword_response(
            RetrievalMode::Semantic,
            results,
            true,
            Some(FallbackCode::VectorBackendDisabled),
            Some("vector backend disabled; used keyword fallback".to_string()),
            started,
            vec!["semantic disabled by config".to_string()],
        ));
    }

    let backends = resolve_semantic_backends(config, backend_override);
    if backends.is_empty() {
        let results = store
            .search_history_keyword(query, limit)
            .unwrap_or_default();
        return Ok(keyword_response(
            RetrievalMode::Semantic,
            results,
            true,
            Some(FallbackCode::VectorBackendDisabled),
            Some("no semantic backend selected; used keyword fallback".to_string()),
            started,
            vec!["no semantic backend selection".to_string()],
        ));
    }

    let mut notes = Vec::new();
    for backend in backends {
        match backend {
            SearchBackend::SqliteVec => {
                if !store.try_load_vec_extension(None).unwrap_or(false) {
                    notes.push("sqlite-vec unavailable".to_string());
                    continue;
                }
                notes.push("sqlite-vec extension detected; using vec0 knn scorer".to_string());
                let attempt_notes = std::mem::take(&mut notes);
                match semantic_history_sqlite_vec(
                    &store,
                    config,
                    query,
                    limit,
                    backend,
                    started,
                    attempt_notes,
                ) {
                    Ok(resp) => return Ok(resp),
                    Err(e) => {
                        notes.push(format!("sqlite-vec query failed: {}", e));
                        continue;
                    }
                }
            }
            SearchBackend::RustCosine => {
                notes.push("using rust-cosine scorer".to_string());
                return semantic_history_rust(
                    &store, config, query, limit, backend, started, notes,
                );
            }
            SearchBackend::Keyword => {}
            SearchBackend::Auto => {}
        }
    }

    let results = store
        .search_history_keyword(query, limit)
        .unwrap_or_else(|_| file_fallback_history(base_path, query, limit));
    Ok(keyword_response(
        RetrievalMode::Semantic,
        results,
        true,
        Some(FallbackCode::SqliteVecUnavailable),
        Some("semantic backend unavailable; used keyword fallback".to_string()),
        started,
        notes,
    ))
}

pub fn search_genome(
    base_path: &Path,
    config: &Config,
    query: &str,
    mode_override: Option<RetrievalMode>,
    backend_override: Option<SearchBackend>,
    limit_override: Option<usize>,
    offset_override: Option<usize>,
) -> Result<SearchResponse> {
    let started = Instant::now();
    let limit = limit_override.unwrap_or(config.retrieval_default_limit.max(1));
    let offset = offset_override.unwrap_or(0);
    let _ = offset; // offset parameter reserved for future use
    let mode = resolve_mode(config, mode_override);

    if query.trim().is_empty() {
        return Ok(keyword_response(
            mode,
            Vec::new(),
            false,
            None,
            None,
            started,
            vec!["empty query".to_string()],
        ));
    }

    let store = match RetrievalStore::open(base_path).and_then(|s| {
        s.init_schema()?;
        Ok(s)
    }) {
        Ok(s) => s,
        Err(_) => {
            let rows = file_fallback_genome(base_path, query, limit);
            return Ok(keyword_response(
                mode,
                rows,
                true,
                Some(FallbackCode::RetrievalDbCorrupt),
                Some("retrieval db unavailable/corrupt; used file fallback".to_string()),
                started,
                vec!["genome fallback scan".to_string()],
            ));
        }
    };

    if matches!(mode, RetrievalMode::Keyword)
        || matches!(backend_override, Some(SearchBackend::Keyword))
    {
        let (results, used_fallback, fallback_code, fallback_reason, notes) =
            match store.search_genome_keyword(query, limit) {
                Ok(rows) => (
                    rows,
                    false,
                    None,
                    None,
                    vec!["fts5 keyword search".to_string()],
                ),
                Err(e) => (
                    file_fallback_genome(base_path, query, limit),
                    true,
                    Some(FallbackCode::RetrievalDbError),
                    Some(format!(
                        "keyword retrieval db error: {}; used file fallback",
                        e
                    )),
                    vec!["genome fallback scan".to_string()],
                ),
            };
        return Ok(keyword_response(
            RetrievalMode::Keyword,
            results,
            used_fallback,
            fallback_code,
            fallback_reason,
            started,
            notes,
        ));
    }

    if !(config.retrieval_vector_enabled && config.retrieval_backend == "fts+vec") {
        let results = store
            .search_genome_keyword(query, limit)
            .unwrap_or_else(|_| file_fallback_genome(base_path, query, limit));
        return Ok(keyword_response(
            RetrievalMode::Semantic,
            results,
            true,
            Some(FallbackCode::VectorBackendDisabled),
            Some("vector backend disabled; used keyword fallback".to_string()),
            started,
            vec!["semantic disabled by config".to_string()],
        ));
    }

    let backends = resolve_semantic_backends(config, backend_override);
    if backends.is_empty() {
        let results = store
            .search_genome_keyword(query, limit)
            .unwrap_or_default();
        return Ok(keyword_response(
            RetrievalMode::Semantic,
            results,
            true,
            Some(FallbackCode::VectorBackendDisabled),
            Some("no semantic backend selected; used keyword fallback".to_string()),
            started,
            vec!["no semantic backend selection".to_string()],
        ));
    }

    let mut notes = Vec::new();
    for backend in backends {
        match backend {
            SearchBackend::SqliteVec => {
                if !store.try_load_vec_extension(None).unwrap_or(false) {
                    notes.push("sqlite-vec unavailable".to_string());
                    continue;
                }
                notes.push("sqlite-vec extension detected; using vec0 knn scorer".to_string());
                let attempt_notes = std::mem::take(&mut notes);
                match semantic_genome_sqlite_vec(
                    &store,
                    config,
                    query,
                    limit,
                    backend,
                    started,
                    attempt_notes,
                ) {
                    Ok(resp) => return Ok(resp),
                    Err(e) => {
                        notes.push(format!("sqlite-vec query failed: {}", e));
                        continue;
                    }
                }
            }
            SearchBackend::RustCosine => {
                notes.push("using rust-cosine scorer".to_string());
                return semantic_genome_rust(&store, config, query, limit, backend, started, notes);
            }
            SearchBackend::Keyword => {}
            SearchBackend::Auto => {}
        }
    }

    let results = store
        .search_genome_keyword(query, limit)
        .unwrap_or_else(|_| file_fallback_genome(base_path, query, limit));
    Ok(keyword_response(
        RetrievalMode::Semantic,
        results,
        true,
        Some(FallbackCode::SqliteVecUnavailable),
        Some("semantic backend unavailable; used keyword fallback".to_string()),
        started,
        notes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "identical vectors should have similarity ~1.0, got {}",
            sim
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            sim.abs() < 1e-6,
            "orthogonal vectors should have similarity ~0.0, got {}",
            sim
        );
    }

    #[test]
    fn test_cosine_similarity_empty_vectors() {
        let empty: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&empty, &empty), 0.0);
    }

    #[test]
    fn test_cosine_similarity_mismatched_length() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![1.0, 2.0, 3.0];
        let zero = vec![0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &zero), 0.0);
    }

    #[test]
    fn test_resolve_mode_default() {
        let config = Config::default();
        assert_eq!(resolve_mode(&config, None), RetrievalMode::Keyword);
    }

    #[test]
    fn test_resolve_mode_override() {
        let config = Config::default();
        assert_eq!(
            resolve_mode(&config, Some(RetrievalMode::Semantic)),
            RetrievalMode::Semantic
        );
    }

    #[test]
    fn test_distance_to_similarity_zero() {
        let sim = distance_to_similarity(0.0);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "distance 0 should give similarity 1.0, got {}",
            sim
        );
    }

    #[test]
    fn test_distance_to_similarity_large() {
        let sim = distance_to_similarity(99.0);
        assert!(
            sim < 0.02,
            "large distance should give low similarity, got {}",
            sim
        );
    }

    #[test]
    fn test_distance_to_similarity_negative_clamped() {
        let sim = distance_to_similarity(-5.0);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "negative distance should clamp to similarity 1.0, got {}",
            sim
        );
    }

    #[test]
    fn test_file_fallback_history_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let results = file_fallback_history(tmp.path(), "anything", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_file_fallback_genome_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let results = file_fallback_genome(tmp.path(), "anything", 10);
        assert!(results.is_empty());
    }
}
