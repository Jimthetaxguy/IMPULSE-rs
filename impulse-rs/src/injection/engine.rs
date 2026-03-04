use chrono::Utc;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

use crate::injection::staging::stage_bundle;
use crate::injection::types::{
    InjectionBundle, InjectionExplain, InjectionMode, InjectionRunResult, InjectionScope,
    InjectionSnippet, InjectionSurface,
};
use crate::retrieval::types::{FallbackCode, RetrievalMode, SearchBackend, SearchResult};
use crate::state::Config;

fn tokenize_terms(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() > 1 && seen.insert(current.clone()) {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty() && current.len() > 1 && seen.insert(current.clone()) {
        out.push(current);
    }

    out
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = input.chars().count();
    if char_count <= max_chars {
        return input.to_string();
    }
    // Need at least 2 chars to show content + ellipsis
    if max_chars < 2 {
        return "\u{2026}".to_string(); // Just ellipsis
    }

    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars - 1 {
            break;
        }
        out.push(ch);
    }
    out.push('\u{2026}'); // ellipsis
    out
}

fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn source_priority(source: &str) -> i32 {
    if source == "history" {
        0
    } else {
        1
    }
}

fn score_compare(a: &InjectionSnippet, b: &InjectionSnippet) -> Ordering {
    b.score
        .partial_cmp(&a.score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| source_priority(&a.source).cmp(&source_priority(&b.source)))
}

fn build_hash(
    surface: InjectionSurface,
    mode: InjectionMode,
    query: &str,
    snippets: &[InjectionSnippet],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(surface.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(mode.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(query.as_bytes());
    hasher.update([0]);
    for snippet in snippets {
        hasher.update(snippet.source.as_bytes());
        hasher.update([0]);
        hasher.update(snippet.id.as_bytes());
        hasher.update([0]);
        hasher.update(format!("{:.6}", snippet.score).as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn render_injected_block(bundle: &InjectionBundle) -> String {
    let mut out = String::new();
    out.push_str("## Impulse Memory Context (Auto)\n");
    out.push_str(&format!(
        "- source_surface: {} | backend: {} | fallback: {}\n",
        bundle.source_surface, bundle.backend_used, bundle.used_fallback
    ));
    for (idx, snippet) in bundle.snippets.iter().enumerate() {
        out.push_str(&format!(
            "- [{}] [{}] {} ({}, score={:.3})\n  {}\n",
            idx + 1,
            snippet.source,
            snippet.title,
            snippet.id,
            snippet.score,
            snippet.snippet.replace('\n', " ")
        ));
    }
    out
}

fn to_snippets(results: Vec<SearchResult>) -> Vec<InjectionSnippet> {
    results
        .into_iter()
        .map(|item| InjectionSnippet {
            source: item.source,
            id: item.id,
            title: item.title,
            snippet: normalize_whitespace(&item.snippet),
            score: item.score,
        })
        .collect()
}

pub fn run_injection(
    base_path: &Path,
    config: &Config,
    surface: InjectionSurface,
    mode_override: Option<InjectionMode>,
    query_parts: &[String],
) -> InjectionRunResult {
    let configured_mode =
        InjectionMode::parse(&config.context_injection_mode).unwrap_or(InjectionMode::Review);
    let requested_mode = mode_override.unwrap_or(configured_mode);
    let scope =
        InjectionScope::parse(&config.context_injection_scope).unwrap_or(InjectionScope::Both);

    let mut explain = InjectionExplain {
        mode_requested: requested_mode.as_str().to_string(),
        mode_effective: requested_mode.as_str().to_string(),
        scope: scope.as_str().to_string(),
        retrieval_mode: if config.context_injection_use_semantic {
            "semantic".to_string()
        } else {
            "keyword".to_string()
        },
        backend_used: "none".to_string(),
        used_fallback: false,
        fallback_code: None,
        timing_ms: 0,
        candidate_count: 0,
        engine_notes: Vec::new(),
        status: "disabled".to_string(),
        artifact_path: None,
        deduped: false,
        error: None,
    };

    if matches!(requested_mode, InjectionMode::Off) {
        explain.status = "mode_off".to_string();
        return InjectionRunResult {
            surface: surface.as_str().to_string(),
            requested_mode: requested_mode.as_str().to_string(),
            effective_mode: InjectionMode::Off.as_str().to_string(),
            applied: false,
            injected_block: None,
            artifact_path: None,
            deduped: false,
            skipped_reason: Some("context injection mode is off".to_string()),
            explain,
            bundle: None,
        };
    }

    if !scope.allows(surface) {
        explain.mode_effective = InjectionMode::Off.as_str().to_string();
        explain.status = "scope_disabled".to_string();
        return InjectionRunResult {
            surface: surface.as_str().to_string(),
            requested_mode: requested_mode.as_str().to_string(),
            effective_mode: InjectionMode::Off.as_str().to_string(),
            applied: false,
            injected_block: None,
            artifact_path: None,
            deduped: false,
            skipped_reason: Some(format!(
                "surface '{}' is disabled by context_injection_scope='{}'",
                surface.as_str(),
                scope.as_str()
            )),
            explain,
            bundle: None,
        };
    }

    let query = normalize_whitespace(
        &query_parts
            .iter()
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    );

    if query.is_empty() {
        explain.status = "empty_query".to_string();
        return InjectionRunResult {
            surface: surface.as_str().to_string(),
            requested_mode: requested_mode.as_str().to_string(),
            effective_mode: requested_mode.as_str().to_string(),
            applied: false,
            injected_block: None,
            artifact_path: None,
            deduped: false,
            skipped_reason: Some("no query text available for injection".to_string()),
            explain,
            bundle: None,
        };
    }

    let query_terms = tokenize_terms(&query);
    let search_query = query_terms
        .first()
        .cloned()
        .unwrap_or_else(|| query.clone());
    let retrieval_mode = if config.context_injection_use_semantic {
        RetrievalMode::Semantic
    } else {
        RetrievalMode::Keyword
    };

    let max_items = config.context_injection_max_items.max(1);
    let max_chars = config.context_injection_max_chars.max(200);
    let min_score = config.context_injection_min_score.clamp(0.0, 1.0) as f64;
    let candidate_limit = max_items.saturating_mul(2).max(max_items);

    let history_response = crate::retrieval::search_history(
        base_path,
        config,
        &search_query,
        Some(retrieval_mode),
        Some(SearchBackend::Auto),
        Some(candidate_limit),
        None,
    );
    let genome_response = crate::retrieval::search_genome(
        base_path,
        config,
        &search_query,
        Some(retrieval_mode),
        Some(SearchBackend::Auto),
        Some(candidate_limit),
        None,
    );

    let mut snippets = Vec::new();
    let mut backend_set = HashSet::new();
    let mut fallback_code: Option<FallbackCode> = None;

    match history_response {
        Ok(resp) => {
            explain.timing_ms = explain.timing_ms.saturating_add(resp.timing_ms);
            explain.candidate_count = explain.candidate_count.saturating_add(resp.candidate_count);
            explain.used_fallback |= resp.used_fallback;
            if fallback_code.is_none() {
                fallback_code = resp.fallback_code;
            }
            backend_set.insert(resp.backend_used.clone());
            explain.engine_notes.extend(resp.engine_notes);
            snippets.extend(to_snippets(resp.results));
        }
        Err(err) => {
            explain.engine_notes.push(format!(
                "history retrieval failed during injection: {}",
                err
            ));
        }
    }

    match genome_response {
        Ok(resp) => {
            explain.timing_ms = explain.timing_ms.saturating_add(resp.timing_ms);
            explain.candidate_count = explain.candidate_count.saturating_add(resp.candidate_count);
            explain.used_fallback |= resp.used_fallback;
            if fallback_code.is_none() {
                fallback_code = resp.fallback_code;
            }
            backend_set.insert(resp.backend_used.clone());
            explain.engine_notes.extend(resp.engine_notes);
            snippets.extend(to_snippets(resp.results));
        }
        Err(err) => {
            explain
                .engine_notes
                .push(format!("genome retrieval failed during injection: {}", err));
        }
    }

    explain.fallback_code = fallback_code;

    if backend_set.is_empty() {
        explain.backend_used = "none".to_string();
    } else if backend_set.len() == 1 {
        explain.backend_used = backend_set
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| "none".to_string());
    } else {
        explain.backend_used = "mixed".to_string();
    }

    let mut dedup = HashSet::new();
    let mut filtered = snippets
        .into_iter()
        .filter(|snippet| {
            (snippet.score <= 0.0 || snippet.score >= min_score)
                && dedup.insert((snippet.source.clone(), snippet.id.clone()))
        })
        .collect::<Vec<_>>();
    filtered.sort_by(score_compare);

    let mut selected = Vec::new();
    let mut total_chars = 0usize;
    for mut snippet in filtered {
        if selected.len() >= max_items {
            break;
        }

        let remaining = max_chars.saturating_sub(total_chars);
        if remaining == 0 {
            break;
        }

        snippet.snippet = truncate_chars(&snippet.snippet, remaining);
        if snippet.snippet.is_empty() {
            continue;
        }

        total_chars = total_chars.saturating_add(snippet.snippet.chars().count());
        selected.push(snippet);
    }

    if selected.is_empty() {
        explain.status = "no_candidates".to_string();
        return InjectionRunResult {
            surface: surface.as_str().to_string(),
            requested_mode: requested_mode.as_str().to_string(),
            effective_mode: requested_mode.as_str().to_string(),
            applied: false,
            injected_block: None,
            artifact_path: None,
            deduped: false,
            skipped_reason: Some("retrieval returned no candidates above threshold".to_string()),
            explain,
            bundle: None,
        };
    }

    let bundle_hash = build_hash(surface, requested_mode, &query, &selected);
    let bundle = InjectionBundle {
        schema_version: 1,
        generated_at: Utc::now(),
        source_surface: surface.as_str().to_string(),
        mode: requested_mode.as_str().to_string(),
        query,
        query_terms,
        retrieval_mode: retrieval_mode.as_str().to_string(),
        backend_used: explain.backend_used.clone(),
        used_fallback: explain.used_fallback,
        fallback_code: explain.fallback_code,
        timing_ms: explain.timing_ms,
        candidate_count: explain.candidate_count,
        engine_notes: explain.engine_notes.clone(),
        snippets: selected,
        total_chars,
        bundle_hash,
    };

    let mut artifact_path = None;
    let mut deduped = false;
    if config.context_injection_emit_artifacts {
        match stage_bundle(base_path, &bundle) {
            Ok(stage) => {
                explain.status = stage.status.clone();
                explain.artifact_path = stage.artifact_path.clone();
                explain.deduped = stage.deduped;
                artifact_path = stage.artifact_path;
                deduped = stage.deduped;
            }
            Err(err) => {
                explain.status = "stage_error".to_string();
                explain.error = Some(format!("failed to stage injection artifact: {}", err));
            }
        }
    } else {
        explain.status = "artifacts_disabled".to_string();
    }

    let injected_block = if matches!(requested_mode, InjectionMode::Apply) {
        Some(render_injected_block(&bundle))
    } else {
        None
    };

    if !matches!(requested_mode, InjectionMode::Apply) && explain.status == "disabled" {
        explain.status = "ready".to_string();
    }

    InjectionRunResult {
        surface: surface.as_str().to_string(),
        requested_mode: requested_mode.as_str().to_string(),
        effective_mode: requested_mode.as_str().to_string(),
        applied: matches!(requested_mode, InjectionMode::Apply) && injected_block.is_some(),
        injected_block,
        artifact_path,
        deduped,
        skipped_reason: None,
        explain,
        bundle: Some(bundle),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use tempfile::TempDir;

    use super::*;
    use crate::memory::Genome;
    use crate::retrieval::types::IndexScope;
    use crate::state::{Config, HistoryEntry, Platform};

    fn seed_history_index(
        temp: &TempDir,
        session_id: &str,
        session_name: &str,
        summary: &str,
        cfg: &Config,
    ) {
        let started = Utc::now() - Duration::minutes(20);
        let ended = Utc::now() - Duration::minutes(10);
        let history = HistoryEntry {
            session_id: session_id.to_string(),
            session_name: session_name.to_string(),
            platform: Some(Platform::ClaudeCode),
            started_at: started,
            ended_at: ended,
            summary: summary.to_string(),
            files_touched: vec!["src/auth.rs".to_string()],
            tools_used: vec!["Write".to_string(), "Edit".to_string()],
        };
        crate::retrieval::index(
            temp.path(),
            &[history],
            &Genome::default(),
            cfg,
            IndexScope::History,
            true,
        )
        .unwrap();
    }

    #[test]
    fn test_run_injection_off_mode_short_circuit() {
        let temp = TempDir::new().unwrap();
        let cfg = Config::default();
        let result = run_injection(
            temp.path(),
            &cfg,
            InjectionSurface::Orchestrate,
            Some(InjectionMode::Off),
            &["test query".to_string()],
        );
        assert_eq!(result.effective_mode, "off");
        assert!(!result.applied);
        assert!(result.bundle.is_none());
    }

    #[test]
    fn test_run_injection_scope_blocks_surface() {
        let temp = TempDir::new().unwrap();
        let cfg = Config {
            context_injection_scope: "daemon".to_string(),
            ..Config::default()
        };
        let result = run_injection(
            temp.path(),
            &cfg,
            InjectionSurface::Handoff,
            Some(InjectionMode::Review),
            &["handoff context".to_string()],
        );
        assert_eq!(result.effective_mode, "off");
        assert!(result
            .skipped_reason
            .unwrap_or_default()
            .contains("disabled"));
    }

    #[test]
    fn test_run_injection_review_with_keyword_results_and_staging() {
        let temp = TempDir::new().unwrap();
        let cfg = Config {
            retrieval_mode: "keyword".to_string(),
            context_injection_mode: "review".to_string(),
            context_injection_use_semantic: false,
            context_injection_emit_artifacts: true,
            ..Config::default()
        };
        seed_history_index(
            &temp,
            "sess-123",
            "auth-refactor",
            "Refactored auth middleware",
            &cfg,
        );

        let result = run_injection(
            temp.path(),
            &cfg,
            InjectionSurface::Orchestrate,
            None,
            &["auth middleware".to_string()],
        );

        assert_eq!(result.effective_mode, "review");
        assert!(result.bundle.is_some());
        assert!(result.artifact_path.is_some());
    }

    #[test]
    fn test_run_injection_apply_emits_injected_block() {
        let temp = TempDir::new().unwrap();
        let cfg = Config {
            retrieval_mode: "keyword".to_string(),
            context_injection_use_semantic: false,
            context_injection_emit_artifacts: true,
            ..Config::default()
        };
        seed_history_index(
            &temp,
            "sess-apply",
            "apply-path",
            "Applied context injection path",
            &cfg,
        );

        let result = run_injection(
            temp.path(),
            &cfg,
            InjectionSurface::SyncContext,
            Some(InjectionMode::Apply),
            &["apply-path".to_string()],
        );
        assert!(result.applied);
        assert!(result
            .injected_block
            .unwrap_or_default()
            .contains("Impulse Memory Context"));
    }
}
