//! Progressive Context Stewardship
//!
//! Monitors AI session context usage and progressively curates context
//! quality as the context window fills up. Provides:
//!
//! - Session JSONL analysis (token estimation, pattern detection)
//! - Progressive cleanup at configurable thresholds (30/45/60/80%)
//! - Cross-project learning via YAML-based memory
//! - User approval workflow (auto/review/off modes)
//! - Compaction assistance with refined context injection

pub mod analyzer;
pub mod approval;
pub mod cleanup;
pub mod cross_project;
pub mod monitor;
pub mod types;

pub use types::*;

/// Atomic write helper: temp file + rename for crash safety
pub(crate) fn atomic_write_file(path: &std::path::Path, content: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let temp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temp_path)
        .with_context(|| format!("Failed to create temp file {:?}", temp_path))?;
    file.write_all(content)
        .with_context(|| format!("Failed to write temp file {:?}", temp_path))?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to rename {:?} to {:?}", temp_path, path))?;
    Ok(())
}

use anyhow::Context;
