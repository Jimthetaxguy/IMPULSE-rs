use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::injection::types::{InjectionBundle, StageResult};
use crate::storage::Storage;

const LOG_WINDOW_FOR_DEDUPE: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionLogEntry {
    pub timestamp: String,
    pub surface: String,
    pub mode: String,
    pub status: String,
    pub query: String,
    pub query_terms: Vec<String>,
    pub retrieval_mode: String,
    pub backend_used: String,
    pub used_fallback: bool,
    pub fallback_code: Option<String>,
    pub timing_ms: u64,
    pub candidate_count: usize,
    pub bundle_hash: String,
    pub bundle_size: usize,
    pub artifact_path: Option<String>,
}

fn injections_dir(base_path: &Path) -> PathBuf {
    base_path.join("context").join("injections")
}

fn log_path(base_path: &Path) -> PathBuf {
    injections_dir(base_path).join("injection-log.jsonl")
}

fn artifact_filename(surface: &str) -> String {
    // Defense-in-depth: `surface` is currently an InjectionSurface enum string,
    // but a path-building helper must not trust its input — sanitize so it can
    // never inject a path separator into the staged artifact filename.
    let safe_surface = crate::storage::sanitize_filename(surface);
    format!(
        "inject-{}-{}.md",
        Utc::now().format("%Y%m%d-%H%M%S"),
        safe_surface
    )
}

fn parse_log_entries(path: &Path) -> Vec<InjectionLogEntry> {
    if !path.exists() {
        return Vec::new();
    }

    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<InjectionLogEntry>(&line) {
            out.push(entry);
        }
    }
    out
}

fn render_markdown(bundle: &InjectionBundle) -> String {
    let mut out = String::new();
    out.push_str("# Injection Bundle\n\n");
    out.push_str("## Metadata\n");
    out.push_str(&format!(
        "- Generated: {}\n",
        bundle.generated_at.to_rfc3339()
    ));
    out.push_str(&format!("- Surface: {}\n", bundle.source_surface));
    out.push_str(&format!("- Mode: {}\n", bundle.mode));
    out.push_str(&format!("- Query: {}\n", bundle.query));
    out.push_str(&format!(
        "- Query Terms: {}\n",
        if bundle.query_terms.is_empty() {
            "(none)".to_string()
        } else {
            bundle.query_terms.join(", ")
        }
    ));
    out.push_str(&format!("- Retrieval Mode: {}\n", bundle.retrieval_mode));
    out.push_str(&format!("- Backend Used: {}\n", bundle.backend_used));
    out.push_str(&format!("- Used Fallback: {}\n", bundle.used_fallback));
    out.push_str(&format!(
        "- Fallback Code: {}\n",
        bundle
            .fallback_code
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "none".to_string())
    ));
    out.push_str(&format!("- Timing (ms): {}\n", bundle.timing_ms));
    out.push_str(&format!("- Candidate Count: {}\n", bundle.candidate_count));
    out.push_str(&format!("- Total Chars: {}\n", bundle.total_chars));
    out.push_str(&format!("- Bundle Hash: {}\n", bundle.bundle_hash));

    if !bundle.engine_notes.is_empty() {
        out.push_str("\n## Engine Notes\n");
        for note in &bundle.engine_notes {
            out.push_str(&format!("- {}\n", note));
        }
    }

    out.push_str("\n## Selected Snippets\n");
    if bundle.snippets.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for (idx, snippet) in bundle.snippets.iter().enumerate() {
            out.push_str(&format!(
                "\n### {}. [{}] {} ({})\n",
                idx + 1,
                snippet.source,
                snippet.title,
                snippet.id
            ));
            out.push_str(&format!("- Score: {:.4}\n\n", snippet.score));
            out.push_str(&snippet.snippet);
            out.push('\n');
        }
    }

    out
}

pub fn stage_bundle(base_path: &Path, bundle: &InjectionBundle) -> Result<StageResult> {
    let dir = injections_dir(base_path);
    std::fs::create_dir_all(&dir)?;
    let log = log_path(base_path);

    let existing = parse_log_entries(&log);
    if let Some(prior) = existing
        .iter()
        .rev()
        .take(LOG_WINDOW_FOR_DEDUPE)
        .find(|entry| {
            entry.bundle_hash == bundle.bundle_hash
                && entry.surface == bundle.source_surface
                && entry.mode == bundle.mode
                && entry.status == "staged"
        })
    {
        let dedupe_entry = InjectionLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            surface: bundle.source_surface.clone(),
            mode: bundle.mode.clone(),
            status: "deduped".to_string(),
            query: bundle.query.clone(),
            query_terms: bundle.query_terms.clone(),
            retrieval_mode: bundle.retrieval_mode.clone(),
            backend_used: bundle.backend_used.clone(),
            used_fallback: bundle.used_fallback,
            fallback_code: bundle.fallback_code.map(|c| c.as_str().to_string()),
            timing_ms: bundle.timing_ms,
            candidate_count: bundle.candidate_count,
            bundle_hash: bundle.bundle_hash.clone(),
            bundle_size: bundle.snippets.len(),
            artifact_path: prior.artifact_path.clone(),
        };
        let mut file = OpenOptions::new().create(true).append(true).open(&log)?;
        writeln!(file, "{}", serde_json::to_string(&dedupe_entry)?)?;
        file.sync_all()?;

        return Ok(StageResult {
            status: "deduped".to_string(),
            artifact_path: prior.artifact_path.clone(),
            deduped: true,
        });
    }

    let filename = artifact_filename(&bundle.source_surface);
    let artifact_path = dir.join(filename);
    let markdown = render_markdown(bundle);
    Storage::atomic_write_path(&artifact_path, markdown.as_bytes())?;

    let entry = InjectionLogEntry {
        timestamp: Utc::now().to_rfc3339(),
        surface: bundle.source_surface.clone(),
        mode: bundle.mode.clone(),
        status: "staged".to_string(),
        query: bundle.query.clone(),
        query_terms: bundle.query_terms.clone(),
        retrieval_mode: bundle.retrieval_mode.clone(),
        backend_used: bundle.backend_used.clone(),
        used_fallback: bundle.used_fallback,
        fallback_code: bundle.fallback_code.map(|c| c.as_str().to_string()),
        timing_ms: bundle.timing_ms,
        candidate_count: bundle.candidate_count,
        bundle_hash: bundle.bundle_hash.clone(),
        bundle_size: bundle.snippets.len(),
        artifact_path: Some(artifact_path.to_string_lossy().to_string()),
    };

    let mut file = OpenOptions::new().create(true).append(true).open(&log)?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    file.sync_all()?;

    Ok(StageResult {
        status: "staged".to_string(),
        artifact_path: Some(artifact_path.to_string_lossy().to_string()),
        deduped: false,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::injection::types::{InjectionBundle, InjectionSnippet};

    #[test]
    fn test_artifact_filename_sanitizes_surface() {
        // A surface containing path separators must not produce a filename that
        // can escape the staging directory.
        let name = artifact_filename("../../etc/evil");
        assert!(!name.contains('/') && !name.contains('\\'), "name: {name}");
        assert!(name.starts_with("inject-") && name.ends_with(".md"));
        // A normal enum surface is preserved verbatim.
        assert!(artifact_filename("review").ends_with("-review.md"));
    }

    fn sample_bundle() -> InjectionBundle {
        InjectionBundle {
            schema_version: 1,
            generated_at: Utc::now(),
            source_surface: "orchestrate".to_string(),
            mode: "review".to_string(),
            query: "review auth changes".to_string(),
            query_terms: vec!["review".to_string(), "auth".to_string()],
            retrieval_mode: "semantic".to_string(),
            backend_used: "rust-cosine".to_string(),
            used_fallback: false,
            fallback_code: None,
            timing_ms: 123,
            candidate_count: 8,
            engine_notes: vec!["using rust-cosine".to_string()],
            snippets: vec![InjectionSnippet {
                source: "history".to_string(),
                id: "sess-1".to_string(),
                title: "auth-refactor".to_string(),
                snippet: "Switched token parser to stricter mode".to_string(),
                score: 0.87,
            }],
            total_chars: 38,
            bundle_hash: "abc123".to_string(),
        }
    }

    #[test]
    fn test_stage_bundle_writes_artifact_and_log() {
        let temp = TempDir::new().unwrap();
        let result = stage_bundle(temp.path(), &sample_bundle()).unwrap();
        assert_eq!(result.status, "staged");
        let artifact = result.artifact_path.unwrap();
        assert!(Path::new(&artifact).exists());

        let log = temp.path().join("context/injections/injection-log.jsonl");
        assert!(log.exists());
    }

    #[test]
    fn test_stage_bundle_dedupes_recent_hash() {
        let temp = TempDir::new().unwrap();
        let first = stage_bundle(temp.path(), &sample_bundle()).unwrap();
        assert_eq!(first.status, "staged");

        let second = stage_bundle(temp.path(), &sample_bundle()).unwrap();
        assert_eq!(second.status, "deduped");
        assert!(second.deduped);
        assert_eq!(first.artifact_path, second.artifact_path);
    }
}
