//! In-memory state with dirty-flag sync and Drop persistence.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). All wrapped in `Arc<RwLock<_>>`
//! for concurrent access. Syncs to `.impulse/` files only when dirty.

use super::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::storage::Storage;

const LIVE_STATE_FILE: &str = "LIVE_STATE.json";
const HISTORY_FILE: &str = "HISTORY.jsonl";
const CONFLICTS_FILE: &str = "CONFLICTS.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEvent {
    pub file_path: String,
    pub session_id: String,
    pub conflicting_sessions: Vec<String>,
    pub detected_at: DateTime<Utc>,
}
const CONFIG_FILE: &str = "config.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub session_id: String,
    pub session_name: String,
    pub platform: Option<Platform>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub summary: String,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
}

pub struct State {
    storage: Storage,
    live_state: RwLock<LiveState>,
    dirty: RwLock<bool>,
    config: RwLock<Config>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("storage", &self.storage.base_path())
            .finish()
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Ok(dirty) = self.dirty.try_read() {
            if *dirty {
                if let Ok(state) = self.live_state.try_read() {
                    if let Err(err) = self.storage.write_json(LIVE_STATE_FILE, &*state) {
                        tracing::error!("failed to persist live state on drop: {}", err);
                    }
                }
            }
        }
    }
}

/// Convert a lock poison error to an anyhow error.
fn lock_err<T: std::fmt::Display>(e: T) -> anyhow::Error {
    anyhow::anyhow!("Lock poisoned: {e}")
}

impl State {
    pub fn new(base_path: std::path::PathBuf) -> Result<Self> {
        let storage = Storage::new(base_path);
        let live_state = storage
            .read_json::<LiveState>(LIVE_STATE_FILE)
            .context("Failed to read live state from disk")?;
        let config = storage
            .read_json::<Config>(CONFIG_FILE)
            .context("Failed to read config from disk")?;

        Ok(Self {
            storage,
            live_state: RwLock::new(live_state),
            dirty: RwLock::new(false),
            config: RwLock::new(config),
        })
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    fn mark_dirty(&self) {
        if let Ok(mut dirty) = self.dirty.try_write() {
            *dirty = true;
        }
    }

    pub async fn sync_immediate(&self) -> Result<()> {
        let state = self.live_state.try_read().map(|s| s.clone())?;
        self.storage
            .write_json(LIVE_STATE_FILE, &state)
            .context("Failed to write live state to disk")?;

        if let Ok(mut dirty) = self.dirty.try_write() {
            *dirty = false;
        }

        Ok(())
    }

    pub async fn create_session(
        &self,
        name: String,
        platform: Option<Platform>,
    ) -> Result<Session> {
        let session = Session::new(name, platform);

        {
            let mut state = self.live_state.try_write().map_err(lock_err)?;
            state.add_session(session.clone());
        }

        self.mark_dirty();
        self.sync_immediate()
            .await
            .context("Failed to sync state after creating session")?;

        Ok(session)
    }

    pub async fn end_session(
        &self,
        session_id: &str,
        summary: String,
    ) -> Result<Option<HistoryEntry>> {
        let history_entry = {
            let mut state = self.live_state.try_write().map_err(lock_err)?;

            if let Some(session) = state.get_session_mut(session_id) {
                session.set_status(SessionStatus::Completed);

                let entry = HistoryEntry {
                    session_id: session.id.clone(),
                    session_name: session.name.clone(),
                    platform: session.platform,
                    started_at: session.created_at,
                    ended_at: Utc::now(),
                    summary,
                    files_touched: session.active_files.clone(),
                    tools_used: session.recent_tools.clone(),
                };

                state.remove_session(session_id);
                Some(entry)
            } else {
                None
            }
        };

        if let Some(ref entry) = history_entry {
            self.storage
                .append_jsonl(HISTORY_FILE, entry)
                .context("Failed to append session to history log")?;
        }

        self.mark_dirty();
        self.sync_immediate()
            .await
            .context("Failed to sync state after ending session")?;

        Ok(history_entry)
    }

    pub async fn track_file(&self, session_id: &str, file_path: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_file(file_path))
            .await
    }

    pub async fn track_tool(&self, session_id: &str, tool_name: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_tool(tool_name))
            .await
    }

    pub async fn check_file_conflict(
        &self,
        session_id: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let state = self
            .live_state
            .try_read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on live state"))?;

        let mut conflicting = Vec::new();
        let normalized_path = std::path::Path::new(file_path)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(file_path))
            .to_string_lossy()
            .to_string();

        for session in state.sessions.values() {
            if session.id == session_id {
                continue;
            }
            for active_file in &session.active_files {
                let active_normalized = std::path::Path::new(active_file)
                    .canonicalize()
                    .unwrap_or_else(|_| std::path::PathBuf::from(active_file))
                    .to_string_lossy()
                    .to_string();
                if active_normalized == normalized_path {
                    conflicting.push(session.name.clone());
                }
            }
        }
        // Record conflict in audit trail if detected
        if !conflicting.is_empty() {
            let event = ConflictEvent {
                file_path: file_path.to_string(),
                session_id: session_id.to_string(),
                conflicting_sessions: conflicting.clone(),
                detected_at: Utc::now(),
            };
            let _ = self.storage.append_jsonl(CONFLICTS_FILE, &event);
        }

        Ok(conflicting)
    }

    /// Get the conflict audit trail (all historical conflict events).
    pub fn get_conflict_history(&self) -> Result<Vec<ConflictEvent>> {
        self.storage
            .read_jsonl::<ConflictEvent>(CONFLICTS_FILE)
            .context("Failed to read conflict event history from disk")
    }

    pub async fn add_tag(&self, session_id: &str, tag: &str) -> Result<()> {
        self.with_session(session_id, |s| s.add_tag(tag)).await
    }

    pub async fn remove_tag(&self, session_id: &str, tag: &str) -> Result<()> {
        self.with_session(session_id, |s| s.remove_tag(tag)).await
    }

    /// Helper for common session mutation pattern
    async fn with_session<F>(&self, session_id: &str, mut f: F) -> Result<()>
    where
        F: FnMut(&mut Session),
    {
        let mut updated = false;
        {
            let mut state = self.live_state.try_write().map_err(lock_err)?;
            if let Some(session) = state.get_session_mut(session_id) {
                f(session);
                updated = true;
            }
        }
        if !updated {
            anyhow::bail!("Session not found: {}", session_id);
        }
        self.mark_dirty();
        self.sync_immediate()
            .await
            .context("Failed to sync state after session update")?;
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let state = self.live_state.try_read().map_err(lock_err)?;
        Ok(state.get_session(session_id).cloned())
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let state = self.live_state.try_read().map_err(lock_err)?;
        Ok(state.list_sessions().into_iter().cloned().collect())
    }

    pub fn get_history_sync(&self) -> Result<Vec<HistoryEntry>> {
        let entries = self
            .storage
            .read_jsonl::<HistoryEntry>(HISTORY_FILE)
            .context("Failed to read history log from disk")?;
        Ok(entries)
    }

    /// Get a config value by key
    pub fn get_config(&self, key: &str) -> Result<Option<String>> {
        let config = self.config.try_read().map_err(lock_err)?;
        Ok(config.get(key))
    }

    /// Set a config value by key
    pub fn set_config(&self, key: &str, value: &str) -> Result<bool> {
        let mut config = self.config.try_write().map_err(lock_err)?;
        let result = config.set(key, value);
        if result {
            self.storage
                .write_json(CONFIG_FILE, &*config)
                .context("Failed to persist config to disk")?;
        }
        Ok(result)
    }

    /// List all config values
    pub fn list_config(&self) -> Result<Vec<(String, String)>> {
        let config = self.config.try_read().map_err(lock_err)?;
        Ok(config.list())
    }

    pub fn config_snapshot(&self) -> Result<Config> {
        let config = self.config.try_read().map_err(lock_err)?;
        Ok(config.clone())
    }

    /// Update guardrail rules in config and persist to disk
    pub fn update_guardrail_rules(&self, rules: Vec<crate::guardrail::GuardRule>) -> Result<()> {
        let mut config = self.config.try_write().map_err(lock_err)?;
        config.guardrails.rules = rules;
        self.storage
            .write_json(CONFIG_FILE, &*config)
            .context("Failed to persist guardrail rules to config")?;
        Ok(())
    }

    pub fn update_impulse_agent_permissions(
        &self,
        policy: impulse_ops::SupervisorPermissionPolicy,
    ) -> Result<()> {
        let mut config = self.config.try_write().map_err(lock_err)?;
        let mut normalized = policy;
        normalized.normalize();
        config.impulse_agent_permissions = normalized;
        self.storage
            .write_json(CONFIG_FILE, &*config)
            .context("Failed to persist agent permissions to config")?;
        Ok(())
    }

    pub fn get_conflict_analytics(&self) -> Result<ConflictHistory> {
        self.storage
            .read_json("CONFLICTS.json")
            .context("Failed to read conflict analytics from disk")
    }

    pub fn record_conflict(&self, file_path: &str, sessions: Vec<String>) -> Result<()> {
        let mut history: ConflictHistory = self
            .storage
            .read_json("CONFLICTS.json")
            .context("Failed to read conflict history from disk")?;
        history.record_conflict(file_path, sessions);
        self.storage
            .write_json("CONFLICTS.json", &history)
            .context("Failed to persist conflict history to disk")?;
        Ok(())
    }

    pub fn record_conflict_resolution(&self, file_path: &str, resolution: &str) -> Result<()> {
        let mut history: ConflictHistory = self
            .storage
            .read_json("CONFLICTS.json")
            .context("Failed to read conflict history for resolution update")?;
        history.record_resolution(file_path, resolution);
        self.storage
            .write_json("CONFLICTS.json", &history)
            .context("Failed to persist conflict resolution to disk")?;
        Ok(())
    }
}

pub type SharedState = Arc<State>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConflictHistory {
    #[serde(default)]
    pub conflict_history: Vec<ConflictEntry>,
}

impl ConflictHistory {
    pub fn new() -> Self {
        Self {
            conflict_history: Vec::new(),
        }
    }

    pub fn record_conflict(&mut self, file_path: &str, detected_sessions: Vec<String>) {
        if let Some(entry) = self
            .conflict_history
            .iter_mut()
            .find(|e| e.file_path == file_path)
        {
            entry.detection_count += 1;
            entry.last_detected = Utc::now();
            entry.involved_sessions = detected_sessions;
        } else {
            self.conflict_history.push(ConflictEntry {
                file_path: file_path.to_string(),
                detection_count: 1,
                first_detected: Utc::now(),
                last_detected: Utc::now(),
                involved_sessions: detected_sessions,
                resolution: None,
                resolved_at: None,
            });
        }
    }

    pub fn record_resolution(&mut self, file_path: &str, resolution: &str) {
        if let Some(entry) = self
            .conflict_history
            .iter_mut()
            .find(|e| e.file_path == file_path)
        {
            entry.resolution = Some(resolution.to_string());
            entry.resolved_at = Some(Utc::now());
        }
    }

    pub fn get_conflict_history(&self) -> &[ConflictEntry] {
        &self.conflict_history
    }

    pub fn get_analytics(&self) -> ConflictAnalytics {
        ConflictAnalytics::from_history(&self.conflict_history)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictEntry {
    pub file_path: String,
    pub detection_count: usize,
    pub first_detected: DateTime<Utc>,
    pub last_detected: DateTime<Utc>,
    pub involved_sessions: Vec<String>,
    pub resolution: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConflictAnalytics {
    pub total_conflicts: usize,
    pub resolved_count: usize,
    pub unresolved_count: usize,
    pub resolution_rate: f64,
    pub conflicts_by_day: HashMap<String, usize>,
    pub conflicts_by_week: HashMap<String, usize>,
    pub conflicts_by_month: HashMap<String, usize>,
    pub most_common_files: Vec<(String, usize)>,
    pub resolution_methods: HashMap<String, usize>,
    pub avg_time_to_resolution_secs: Option<i64>,
}

impl ConflictAnalytics {
    pub fn from_history(entries: &[ConflictEntry]) -> Self {
        let total_conflicts = entries.len();
        let resolved_count = entries.iter().filter(|e| e.resolution.is_some()).count();
        let unresolved_count = total_conflicts - resolved_count;
        let resolution_rate = if total_conflicts > 0 {
            (resolved_count as f64 / total_conflicts as f64) * 100.0
        } else {
            0.0
        };

        let mut conflicts_by_day: HashMap<String, usize> = HashMap::new();
        let mut conflicts_by_week: HashMap<String, usize> = HashMap::new();
        let mut conflicts_by_month: HashMap<String, usize> = HashMap::new();
        let mut file_counts: HashMap<String, usize> = HashMap::new();
        let mut resolution_methods: HashMap<String, usize> = HashMap::new();
        let mut total_resolution_time = 0i64;
        let mut resolved_with_time = 0usize;

        for entry in entries {
            let day = entry.first_detected.format("%Y-%m-%d").to_string();
            let week = entry.first_detected.format("%Y-W%U").to_string();
            let month = entry.first_detected.format("%Y-%m").to_string();

            *conflicts_by_day.entry(day).or_insert(0) += 1;
            *conflicts_by_week.entry(week).or_insert(0) += 1;
            *conflicts_by_month.entry(month).or_insert(0) += 1;
            *file_counts.entry(entry.file_path.clone()).or_insert(0) += 1;

            if let Some(ref resolution) = entry.resolution {
                *resolution_methods.entry(resolution.clone()).or_insert(0) += 1;

                if let Some(resolved_at) = entry.resolved_at {
                    let duration = (resolved_at - entry.first_detected).num_seconds();
                    total_resolution_time += duration;
                    resolved_with_time += 1;
                }
            }
        }

        let mut most_common_files: Vec<_> = file_counts.into_iter().collect();
        most_common_files.sort_by(|a, b| b.1.cmp(&a.1));

        let avg_time_to_resolution_secs = if resolved_with_time > 0 {
            Some(total_resolution_time / resolved_with_time as i64)
        } else {
            None
        };

        Self {
            total_conflicts,
            resolved_count,
            unresolved_count,
            resolution_rate,
            conflicts_by_day,
            conflicts_by_week,
            conflicts_by_month,
            most_common_files,
            resolution_methods,
            avg_time_to_resolution_secs,
        }
    }

    pub fn format_time_to_resolution(&self) -> String {
        if let Some(secs) = self.avg_time_to_resolution_secs {
            if secs < 60 {
                format!("{}s", secs)
            } else if secs < 3600 {
                format!("{}m", secs / 60)
            } else {
                let hours = secs / 3600;
                let mins = (secs % 3600) / 60;
                format!("{}h {}m", hours, mins)
            }
        } else {
            "N/A".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.log_level, "info");
        assert_eq!(config.default_platform, None);
        assert!(!config.verbose);
        assert_eq!(config.sync_interval_secs, 30);
        assert_eq!(config.max_history_entries, 1000);
        assert_eq!(config.retrieval_mode, "keyword");
        assert_eq!(config.retrieval_backend, "fts");
        assert_eq!(config.retrieval_default_limit, 10);
        assert_eq!(config.retrieval_similarity_threshold, 0.75);
        assert_eq!(config.retrieval_embedding_provider, "python-st");
        assert_eq!(config.retrieval_python_cmd, "python3");
        assert!(!config.retrieval_vector_enabled);
        assert_eq!(config.retrieval_semantic_strategy, "auto");
        assert_eq!(config.retrieval_query_timeout_secs, 10);
        assert_eq!(config.retrieval_index_timeout_secs, 60);
        assert_eq!(config.retrieval_batch_size, 64);
        assert_eq!(config.retrieval_candidate_pool, 200);
        assert!(!config.retrieval_experimental_pageindex_enabled);
        assert_eq!(config.retrieval_pageindex_mode, "local-structure");
        assert_eq!(config.context_injection_mode, "review");
        assert_eq!(config.context_injection_scope, "both");
        assert_eq!(config.context_injection_max_items, 5);
        assert_eq!(config.context_injection_max_chars, 2000);
        assert_eq!(config.context_injection_min_score, 0.60);
        assert!(config.context_injection_use_semantic);
        assert!(config.context_injection_emit_artifacts);
    }

    #[test]
    fn test_config_get() {
        let config = Config::default();
        assert_eq!(config.get("log_level"), Some("info".to_string()));
        assert_eq!(config.get("verbose"), Some("false".to_string()));
        assert_eq!(config.get("nonexistent"), None);
    }

    #[test]
    fn test_config_set() {
        let mut config = Config::default();

        assert!(config.set("log_level", "debug"));
        assert_eq!(config.log_level, "debug");

        assert!(!config.set("log_level", "invalid"));

        assert!(config.set("verbose", "true"));
        assert!(config.verbose);

        assert!(config.set("default_platform", "claude-code"));
        assert_eq!(config.default_platform, Some(Platform::ClaudeCode));

        assert!(config.set("default_platform", "none"));
        assert_eq!(config.default_platform, None);

        assert!(config.set("sync_interval_secs", "60"));
        assert_eq!(config.sync_interval_secs, 60);

        assert!(config.set("retrieval_mode", "semantic"));
        assert_eq!(config.retrieval_mode, "semantic");
        assert!(!config.set("retrieval_mode", "bad"));

        assert!(config.set("retrieval_backend", "fts+vec"));
        assert_eq!(config.retrieval_backend, "fts+vec");
        assert!(!config.set("retrieval_backend", "db"));

        assert!(config.set("retrieval_default_limit", "25"));
        assert_eq!(config.retrieval_default_limit, 25);

        assert!(config.set("retrieval_similarity_threshold", "0.9"));
        assert_eq!(config.retrieval_similarity_threshold, 0.9);
        assert!(!config.set("retrieval_similarity_threshold", "1.1"));

        assert!(config.set("retrieval_embedding_provider", "python-st"));
        assert!(config.set("retrieval_python_cmd", "python3"));
        assert!(config.set("retrieval_vector_enabled", "true"));
        assert!(config.retrieval_vector_enabled);
        assert!(!config.set("retrieval_vector_enabled", "enabled"));

        assert!(config.set("retrieval_semantic_strategy", "rust-only"));
        assert_eq!(config.retrieval_semantic_strategy, "rust-only");
        assert!(!config.set("retrieval_semantic_strategy", "none"));

        assert!(config.set("retrieval_query_timeout_secs", "15"));
        assert_eq!(config.retrieval_query_timeout_secs, 15);
        assert!(!config.set("retrieval_query_timeout_secs", "0"));

        assert!(config.set("retrieval_index_timeout_secs", "90"));
        assert_eq!(config.retrieval_index_timeout_secs, 90);
        assert!(!config.set("retrieval_index_timeout_secs", "5"));

        assert!(config.set("retrieval_batch_size", "32"));
        assert_eq!(config.retrieval_batch_size, 32);
        assert!(!config.set("retrieval_batch_size", "9999"));

        assert!(config.set("retrieval_candidate_pool", "350"));
        assert_eq!(config.retrieval_candidate_pool, 350);
        assert!(!config.set("retrieval_candidate_pool", "2"));

        assert!(config.set("retrieval_experimental_pageindex_enabled", "true"));
        assert!(config.retrieval_experimental_pageindex_enabled);
        assert!(!config.set("retrieval_experimental_pageindex_enabled", "enabled"));

        assert!(config.set("retrieval_pageindex_mode", "api-augmented"));
        assert_eq!(config.retrieval_pageindex_mode, "api-augmented");
        assert!(!config.set("retrieval_pageindex_mode", "hybrid"));

        assert!(config.set("context_injection_mode", "apply"));
        assert_eq!(config.context_injection_mode, "apply");
        assert!(!config.set("context_injection_mode", "auto"));

        assert!(config.set("context_injection_scope", "direct"));
        assert_eq!(config.context_injection_scope, "direct");
        assert!(!config.set("context_injection_scope", "none"));

        assert!(config.set("context_injection_max_items", "8"));
        assert_eq!(config.context_injection_max_items, 8);
        assert!(!config.set("context_injection_max_items", "0"));

        assert!(config.set("context_injection_max_chars", "4096"));
        assert_eq!(config.context_injection_max_chars, 4096);
        assert!(!config.set("context_injection_max_chars", "128"));

        assert!(config.set("context_injection_min_score", "0.7"));
        assert_eq!(config.context_injection_min_score, 0.7);
        assert!(!config.set("context_injection_min_score", "1.5"));

        assert!(config.set("context_injection_use_semantic", "false"));
        assert!(!config.context_injection_use_semantic);
        assert!(!config.set("context_injection_use_semantic", "maybe"));

        assert!(config.set("context_injection_emit_artifacts", "false"));
        assert!(!config.context_injection_emit_artifacts);
        assert!(!config.set("context_injection_emit_artifacts", "enabled"));
    }

    #[test]
    fn test_config_list() {
        let config = Config::default();
        let items = config.list();

        assert!(items.len() >= 45); // May increase as modules are added
        assert!(items.iter().any(|(k, _)| k == "retrieval_mode"));
        assert!(items.iter().any(|(k, _)| k == "stewardship_mode"));
        assert!(items.iter().any(|(k, _)| k == "build_hygiene_enabled"));
        assert!(items.iter().any(|(k, _)| k == "build_hygiene_scan_paths"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_backend"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_default_limit"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_similarity_threshold"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_embedding_provider"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_python_cmd"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_vector_enabled"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_semantic_strategy"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_query_timeout_secs"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_index_timeout_secs"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_batch_size"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_candidate_pool"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "retrieval_experimental_pageindex_enabled"));
        assert!(items.iter().any(|(k, _)| k == "retrieval_pageindex_mode"));
        assert!(items.iter().any(|(k, _)| k == "context_injection_mode"));
        assert!(items.iter().any(|(k, _)| k == "context_injection_scope"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_max_items"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_max_chars"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_min_score"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_use_semantic"));
        assert!(items
            .iter()
            .any(|(k, _)| k == "context_injection_emit_artifacts"));
    }

    #[test]
    fn test_session_new() {
        let session = Session::new("test-session".to_string(), Some(Platform::ClaudeCode));

        assert!(!session.id.is_empty());
        assert_eq!(session.name, "test-session");
        assert_eq!(session.platform, Some(Platform::ClaudeCode));
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.active_files.is_empty());
        assert!(session.recent_tools.is_empty());
    }

    #[test]
    fn test_session_add_file() {
        let mut session = Session::new("test".to_string(), None);

        session.add_file("src/main.rs");
        assert!(session.active_files.contains(&"src/main.rs".to_string()));

        session.add_file("src/main.rs");
        assert_eq!(session.active_files.len(), 1);
    }

    #[test]
    fn test_session_add_tool() {
        let mut session = Session::new("test".to_string(), None);

        session.add_tool("Write");
        session.add_tool("Read");
        session.add_tool("Edit");

        assert_eq!(session.recent_tools.len(), 3);
        assert_eq!(session.recent_tools[0], "Edit");

        session.add_tool("Write");
        assert_eq!(session.recent_tools.len(), 3);
        assert_eq!(session.recent_tools[0], "Write");
    }

    #[test]
    fn test_session_set_status() {
        let mut session = Session::new("test".to_string(), None);

        session.set_status(SessionStatus::Idle);
        assert_eq!(session.status, SessionStatus::Idle);

        session.set_status(SessionStatus::Completed);
        assert_eq!(session.status, SessionStatus::Completed);
    }

    #[test]
    fn test_live_state_new() {
        let state = LiveState::new();
        assert!(state.sessions.is_empty());
    }

    #[test]
    fn test_live_state_add_session() {
        let mut state = LiveState::new();
        let session = Session::new("test".to_string(), None);

        state.add_session(session.clone());

        assert_eq!(state.sessions.len(), 1);
        assert!(state.get_session(&session.id).is_some());
    }

    #[test]
    fn test_live_state_remove_session() {
        let mut state = LiveState::new();
        let session = Session::new("test".to_string(), None);
        let id = session.id.clone();

        state.add_session(session);
        let removed = state.remove_session(&id);

        assert!(removed.is_some());
        assert!(state.get_session(&id).is_none());
    }

    #[test]
    fn test_live_state_list_sessions() {
        let mut state = LiveState::new();

        state.add_session(Session::new("session1".to_string(), None));
        state.add_session(Session::new("session2".to_string(), None));

        let sessions = state.list_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let storage = crate::storage::Storage::new(temp_dir.path().to_path_buf());

        let config = Config::default();
        storage.write_json("config.json", &config).unwrap();

        let loaded: Config = storage.read_json("config.json").unwrap();
        assert_eq!(loaded.log_level, "info");
    }

    #[tokio::test]
    async fn test_check_file_conflict_same_file() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();
        let session2 = state
            .create_session("session2".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();
        state.track_file(&session2.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/main.rs")
            .await
            .unwrap();
        assert!(!conflicting.is_empty());
        assert!(conflicting.contains(&"session2".to_string()));
    }

    #[tokio::test]
    async fn test_check_file_conflict_different_files() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();
        let _session2 = state
            .create_session("session2".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/lib.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_check_file_conflict_no_other_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session1.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session1.id, "src/main.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_check_file_conflict_self_excluded() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session = state
            .create_session("session1".to_string(), None)
            .await
            .unwrap();

        state.track_file(&session.id, "src/main.rs").await.unwrap();

        let conflicting = state
            .check_file_conflict(&session.id, "src/main.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());
    }

    #[tokio::test]
    async fn test_track_file_missing_session_errors() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let err = state
            .track_file("missing-session", "src/main.rs")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Session not found"));
    }

    #[test]
    fn test_state_new_surfaces_config_corruption() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path()).unwrap();
        std::fs::write(temp_dir.path().join("config.json"), "{not-json").unwrap();

        let err = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Failed to read config") || msg.contains("Failed to parse JSON"),
            "expected config read/parse error, got: {msg}"
        );
    }

    #[test]
    fn test_conflict_history_new() {
        let history = ConflictHistory::new();
        assert!(history.conflict_history.is_empty());
    }

    #[test]
    fn test_conflict_history_record_conflict() {
        let mut history = ConflictHistory::new();
        history.record_conflict(
            "src/main.rs",
            vec!["session1".to_string(), "session2".to_string()],
        );

        assert_eq!(history.conflict_history.len(), 1);
        assert_eq!(history.conflict_history[0].file_path, "src/main.rs");
        assert_eq!(history.conflict_history[0].detection_count, 1);
        assert_eq!(history.conflict_history[0].involved_sessions.len(), 2);
    }

    #[test]
    fn test_conflict_history_record_conflict_increments_count() {
        let mut history = ConflictHistory::new();
        history.record_conflict("src/main.rs", vec!["session1".to_string()]);
        history.record_conflict("src/main.rs", vec!["session2".to_string()]);

        assert_eq!(history.conflict_history.len(), 1);
        assert_eq!(history.conflict_history[0].detection_count, 2);
    }

    #[test]
    fn test_conflict_history_record_resolution() {
        let mut history = ConflictHistory::new();
        history.record_conflict("src/main.rs", vec!["session1".to_string()]);
        history.record_resolution("src/main.rs", "merge");

        assert_eq!(
            history.conflict_history[0].resolution,
            Some("merge".to_string())
        );
        assert!(history.conflict_history[0].resolved_at.is_some());
    }

    #[test]
    fn test_conflict_analytics_from_history_empty() {
        let analytics = ConflictAnalytics::from_history(&[]);
        assert_eq!(analytics.total_conflicts, 0);
        assert_eq!(analytics.resolved_count, 0);
        assert_eq!(analytics.unresolved_count, 0);
        assert_eq!(analytics.resolution_rate, 0.0);
    }

    #[test]
    fn test_conflict_analytics_from_history_with_data() {
        use chrono::Duration;

        let mut entry1 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec!["session1".to_string()],
            resolution: Some("merge".to_string()),
            resolved_at: Some(chrono::Utc::now()),
        };
        entry1.resolved_at = Some(entry1.first_detected + Duration::seconds(60));

        let entry2 = ConflictEntry {
            file_path: "src/lib.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec!["session2".to_string()],
            resolution: None,
            resolved_at: None,
        };

        let analytics = ConflictAnalytics::from_history(&[entry1, entry2]);

        assert_eq!(analytics.total_conflicts, 2);
        assert_eq!(analytics.resolved_count, 1);
        assert_eq!(analytics.unresolved_count, 1);
        assert_eq!(analytics.resolution_rate, 50.0);
    }

    #[test]
    fn test_conflict_analytics_most_common_files() {
        let entry1 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };
        let entry2 = ConflictEntry {
            file_path: "src/main.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };
        let entry3 = ConflictEntry {
            file_path: "src/lib.rs".to_string(),
            detection_count: 1,
            first_detected: chrono::Utc::now(),
            last_detected: chrono::Utc::now(),
            involved_sessions: vec![],
            resolution: None,
            resolved_at: None,
        };

        let analytics = ConflictAnalytics::from_history(&[entry1, entry2, entry3]);

        assert_eq!(analytics.most_common_files.len(), 2);
        assert_eq!(analytics.most_common_files[0].0, "src/main.rs");
        assert_eq!(analytics.most_common_files[0].1, 2);
    }

    #[test]
    fn test_conflict_analytics_format_time_to_resolution() {
        let analytics_empty = ConflictAnalytics::default();
        assert_eq!(analytics_empty.format_time_to_resolution(), "N/A");

        let analytics_with_time = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(30),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_with_time.format_time_to_resolution(), "30s");

        let analytics_minutes = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(120),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_minutes.format_time_to_resolution(), "2m");

        let analytics_hours = ConflictAnalytics {
            avg_time_to_resolution_secs: Some(3665),
            ..ConflictAnalytics::default()
        };
        assert_eq!(analytics_hours.format_time_to_resolution(), "1h 1m");
    }

    #[tokio::test]
    async fn test_conflict_audit_trail_recorded() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state.create_session("s1".to_string(), None).await.unwrap();
        let session2 = state.create_session("s2".to_string(), None).await.unwrap();

        state.track_file(&session1.id, "src/lib.rs").await.unwrap();
        state.track_file(&session2.id, "src/lib.rs").await.unwrap();

        // Before conflict check, no events
        let history = state.get_conflict_history().unwrap();
        assert!(history.is_empty());

        // Trigger conflict detection
        let conflicting = state
            .check_file_conflict(&session1.id, "src/lib.rs")
            .await
            .unwrap();
        assert!(!conflicting.is_empty());

        // Audit trail should now have one event
        let history = state.get_conflict_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].file_path, "src/lib.rs");
        assert_eq!(history[0].session_id, session1.id);
        assert!(history[0].conflicting_sessions.contains(&"s2".to_string()));
    }

    #[tokio::test]
    async fn test_conflict_audit_not_recorded_when_no_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let state = crate::state::State::new(temp_dir.path().to_path_buf()).unwrap();

        let session1 = state.create_session("s1".to_string(), None).await.unwrap();
        state.track_file(&session1.id, "src/main.rs").await.unwrap();

        // No conflict (only one session)
        let conflicting = state
            .check_file_conflict(&session1.id, "src/main.rs")
            .await
            .unwrap();
        assert!(conflicting.is_empty());

        // No audit event
        let history = state.get_conflict_history().unwrap();
        assert!(history.is_empty());
    }
}
