use anyhow::Result;
use std::sync::Arc;

use crate::{retrieval, state};

pub fn handle_index_memory(state: &Arc<state::State>, scope: String, rebuild: bool) -> Result<()> {
    let scope = retrieval::types::IndexScope::parse(&scope)
        .ok_or_else(|| anyhow::anyhow!("Invalid scope '{}'. Use history|genome|all", scope))?;
    let config = state.config_snapshot()?;
    let index_state = retrieval::index_from_storage(state.storage(), &config, scope, rebuild)?;
    println!(
        "Indexed memory: history={} genome={} vector_enabled={} vector_available={} duration={}ms",
        index_state.history_count,
        index_state.genome_count,
        index_state.vector_enabled,
        index_state.vector_available,
        index_state.last_index_duration_ms
    );
    for note in index_state.notes {
        println!("Note: {}", note);
    }
    Ok(())
}

pub fn handle_retrieval_status(state: &Arc<state::State>, check: bool, json: bool) -> Result<()> {
    let config = state.config_snapshot()?;
    let status = retrieval::status(state.storage().base_path(), &config, check)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("Retrieval DB: {}", status.db_path);
        println!(
            "Exists: {} ({} bytes)",
            status.db_exists, status.db_size_bytes
        );
        println!(
            "Indexed at: {} (duration={}ms)",
            status.index_state.indexed_at.to_rfc3339(),
            status.index_state.last_index_duration_ms
        );
        println!(
            "Counts: history={} genome={}",
            status.index_state.history_count, status.index_state.genome_count
        );
        println!(
            "Vector: enabled={} extension_available={}",
            status.index_state.vector_enabled, status.vector_extension_available
        );
        println!("Python cmd available: {}", status.python_available);
        println!(
            "Injection: mode={} scope={} emit_artifacts={} staged={} last_status={} last_artifact={}",
            status.injection.config_mode,
            status.injection.config_scope,
            status.injection.emit_artifacts,
            status.injection.staged_artifact_count,
            status
                .injection
                .last_staged_status
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            status
                .injection
                .last_staged_artifact
                .clone()
                .unwrap_or_else(|| "none".to_string())
        );
        if let Some(ok) = status.integrity_ok {
            println!(
                "Integrity check: {} ({})",
                if ok { "ok" } else { "failed" },
                status
                    .integrity_message
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
        if !status.index_state.notes.is_empty() {
            for n in status.index_state.notes {
                println!("Note: {}", n);
            }
        }
    }
    Ok(())
}
