//! Project state shared by every process that opens a project.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). Each process (the daemon and every
//! direct-mode hook) keeps its own in-memory copy, so a change is never written
//! by saving that copy whole: it runs as one read-modify-write of the file on
//! disk under [`Storage::lock_exclusive`], and the copy is refreshed from what
//! was written. See [`State::update_live_state`] and [`State::update_config`].

use super::*;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use crate::storage::Storage;

const LIVE_STATE_FILE: &str = "LIVE_STATE.json";
/// How long a state update waits for another process's update to finish
/// before failing. Generous: the longest holder appends and syncs one history
/// line or one memory decision.
const STATE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
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
    pub(super) governed_tasks: std::sync::Mutex<super::governed_task::GovernedTaskLedger>,
    pub(super) memory_candidates: std::sync::Mutex<super::memory_candidate::MemoryCandidateLedger>,
    /// Whether `MEMORY_CANDIDATES.json` existed when this `State` opened.
    ///
    /// Captured before any reconciliation writes it, because candidate
    /// reconciliation re-creates the file — so a later probe would report
    /// `Local` on every machine and a fresh clone's tracked memory log would be
    /// misread as a pile of interrupted decisions (ADR-0020 rule 3b).
    pub(super) memory_ledger_origin: super::memory_record::LedgerOrigin,
    /// Verified view of the append-only promoted-memory log (ADR-0020).
    /// Always locked *after* `memory_candidates`, never before.
    pub(super) memory_log: std::sync::Mutex<super::memory_record::MemoryLog>,
    pub(super) producer_reservations:
        std::sync::Mutex<super::producer_reservation::ProducerReservationLedger>,
    governed_producer_locks: tokio::sync::Mutex<
        HashMap<impulse_ops::governed_task::GovernedTaskId, Arc<tokio::sync::Mutex<()>>>,
    >,
    /// This process's copy of `LIVE_STATE.json`, as of its last read or write.
    /// Only [`State::update_live_state`] replaces it.
    live_state: RwLock<LiveState>,
    /// This process's copy of `config.json`. Only [`State::update_config`]
    /// replaces it.
    config: RwLock<Config>,
    /// Sessions whose history line `end_session` appended but whose removal
    /// from `LIVE_STATE.json` failed to save, with that line. A retry here
    /// finishes the end instead of recording the session twice.
    half_ended_sessions: std::sync::Mutex<HashMap<String, HistoryEntry>>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("storage", &self.storage.base_path())
            .finish()
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
        let governed_tasks = Self::load_governed_task_ledger(&storage)?;
        let (memory_candidates, memory_ledger_origin) =
            Self::load_memory_candidate_ledger(&storage)?;
        let producer_reservations = Self::load_producer_reservation_ledger(&storage)?;

        let state = Self {
            storage,
            governed_tasks,
            memory_candidates,
            memory_ledger_origin,
            memory_log: std::sync::Mutex::new(Default::default()),
            producer_reservations,
            governed_producer_locks: tokio::sync::Mutex::new(HashMap::new()),
            live_state: RwLock::new(live_state),
            config: RwLock::new(config),
            half_ended_sessions: std::sync::Mutex::new(HashMap::new()),
        };
        state.reconcile_accepted_run_memory_candidates()?;
        // Must follow candidate reconciliation: the log is cross-checked against
        // the reconciled review statuses, not the pre-migration ones.
        state.reconcile_promoted_memory_log()?;
        // Producer reservations are deliberately not reconciled here. Every
        // direct-mode command (including the hooks that fire on each agent
        // tool call) builds a `State`, and a reservation that is open on disk
        // may belong to a daemon that is still running its side effect. Only
        // the daemon reconciles, once it owns the socket; see
        // `State::reconcile_producer_reservations`.
        Ok(state)
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Take the project-wide lock every read-modify-write of shared state
    /// holds, failing after [`STATE_LOCK_WAIT`] rather than hanging behind a
    /// holder that never finishes. Not re-entrant.
    pub(super) fn lock_project_state(&self, purpose: &str) -> Result<crate::storage::StorageLock> {
        self.storage
            .lock_exclusive_within(STATE_LOCK_WAIT)
            .with_context(|| format!("Failed to lock project state to {purpose}"))
    }

    /// Serialize daemon-owned producer side effects per governed task.
    ///
    /// The guard spans the authoritative re-read, any Git/command/model work,
    /// and persistence. This keeps duplicate and competing requests from
    /// executing the same expensive producer before CAS can reject one.
    pub(crate) async fn acquire_governed_producer_lock(
        &self,
        task_id: &impulse_ops::governed_task::GovernedTaskId,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.governed_producer_locks.lock().await;
            locks
                .entry(task_id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    /// Apply `change` to `LIVE_STATE.json` as it is on disk now, then save it.
    ///
    /// Every process that opens the project holds its own copy of the live
    /// state, and saving that copy whole replaced whatever another process had
    /// saved since it was loaded: a session one hook created vanished when a
    /// concurrent hook saved, and a stale daemon task could rename an older
    /// snapshot over a newer one. Each change now runs under the storage
    /// directory lock against the current file, and this process's copy is
    /// replaced with what was written. A failed `change` writes nothing.
    ///
    /// The lock is not re-entrant, so `change` must not start another update.
    fn update_live_state<T>(&self, change: impl FnOnce(&mut LiveState) -> Result<T>) -> Result<T> {
        let _lock = self.lock_project_state("update the live state")?;
        let mut current = self
            .storage
            .read_json::<LiveState>(LIVE_STATE_FILE)
            .context("Failed to read live state from disk")?;
        let result = change(&mut current)?;
        self.storage
            .write_json(LIVE_STATE_FILE, &current)
            .context("Failed to write live state to disk")?;
        *self.live_state.write().map_err(lock_err)? = current;
        Ok(result)
    }

    pub async fn create_session(
        &self,
        name: String,
        platform: Option<Platform>,
    ) -> Result<Session> {
        let session = Session::new(name, platform);
        self.update_live_state(|state| {
            state.add_session(session.clone());
            Ok(())
        })
        .context("Failed to sync state after creating session")?;
        Ok(session)
    }

    pub async fn end_session(
        &self,
        session_id: &str,
        summary: String,
    ) -> Result<Option<HistoryEntry>> {
        // The history line is appended inside the update, before the session
        // is removed. A failed append writes nothing, so the session stays and
        // can be ended again. A concurrent end of the same session, in this
        // process or another, waits for the lock and then finds the session
        // gone, so it is never recorded twice. The in-memory copy is not locked
        // during the append, so readers never wait on the disk.
        let ended = self.update_live_state(|state| {
            let Some(session) = state.get_session(session_id) else {
                return Ok(None);
            };
            // An earlier end here appended its history line but failed to
            // save the live state, so the session is still here. Finish that
            // end rather than recording the session twice.
            let half_ended = self
                .half_ended_sessions
                .lock()
                .map_err(lock_err)?
                .get(session_id)
                .cloned();
            if let Some(recorded) = half_ended {
                state.remove_session(session_id);
                return Ok(Some(recorded));
            }
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
            self.storage
                .append_jsonl(HISTORY_FILE, &entry)
                .context("Failed to append session to history log")?;
            // Remembered until the live-state save below succeeds.
            self.half_ended_sessions
                .lock()
                .map_err(lock_err)?
                .insert(session_id.to_string(), entry.clone());
            state.remove_session(session_id);
            Ok(Some(entry))
        });
        if ended.is_ok() {
            self.half_ended_sessions
                .lock()
                .map_err(lock_err)?
                .remove(session_id);
        }
        ended
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
            .read()
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
            if let Err(err) = self.storage.append_jsonl(CONFLICTS_FILE, &event) {
                tracing::error!(
                    "failed to append conflict event to audit trail ({}) for session {} on {}: {}",
                    CONFLICTS_FILE,
                    session_id,
                    file_path,
                    err
                );
            }
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
    async fn with_session<F>(&self, session_id: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut Session),
    {
        self.update_live_state(|state| {
            let session = state
                .get_session_mut(session_id)
                .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;
            f(session);
            Ok(())
        })
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let state = self.live_state.read().map_err(lock_err)?;
        Ok(state.get_session(session_id).cloned())
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let state = self.live_state.read().map_err(lock_err)?;
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
        let config = self.config.read().map_err(lock_err)?;
        Ok(config.get(key))
    }

    /// Apply `change` to `config.json` as it is on disk now, saving it when
    /// `change` reports a change, and refresh this process's copy.
    ///
    /// Writing this process's copy whole, loaded when it started, reverted any
    /// setting another process had saved since: a daemon that recorded a
    /// supervisor permission grant undid a `config` command run meanwhile.
    fn update_config(&self, change: impl FnOnce(&mut Config) -> bool) -> Result<bool> {
        let _lock = self.lock_project_state("update the config")?;
        let mut current = self
            .storage
            .read_json::<Config>(CONFIG_FILE)
            .context("Failed to read config from disk")?;
        current.keep_process_local(&*self.config.read().map_err(lock_err)?);
        let changed = change(&mut current);
        if changed {
            self.storage
                .write_json(CONFIG_FILE, &current)
                .context("Failed to persist config to disk")?;
        }
        *self.config.write().map_err(lock_err)? = current;
        Ok(changed)
    }

    /// Set a config value by key
    pub fn set_config(&self, key: &str, value: &str) -> Result<bool> {
        self.update_config(|config| config.set(key, value))
    }

    /// List all config values
    pub fn list_config(&self) -> Result<Vec<(String, String)>> {
        let config = self.config.read().map_err(lock_err)?;
        Ok(config.list())
    }

    pub fn config_snapshot(&self) -> Result<Config> {
        let config = self.config.read().map_err(lock_err)?;
        Ok(config.clone())
    }

    /// Change the guardrail rules as they are on disk now and persist them.
    ///
    /// Takes the change rather than a finished list, so a rule toggled by
    /// another process since this one loaded its config is kept.
    pub fn update_guardrail_rules(
        &self,
        change: impl FnOnce(&mut Vec<crate::guardrail::GuardRule>),
    ) -> Result<()> {
        self.update_config(|config| {
            change(&mut config.guardrails.rules);
            true
        })
        .context("Failed to persist guardrail rules to config")?;
        Ok(())
    }

    /// Change the supervisor's baseline permission policy as it is on disk now,
    /// normalize and persist it, and return the result.
    ///
    /// Takes the change rather than a finished policy: a grant built from this
    /// process's copy reinstated a permission revoked from the CLI since.
    pub fn update_impulse_agent_permissions(
        &self,
        change: impl FnOnce(&mut impulse_ops::SupervisorPermissionPolicy),
    ) -> Result<impulse_ops::SupervisorPermissionPolicy> {
        let mut updated = None;
        self.update_config(|config| {
            change(&mut config.impulse_agent_permissions);
            config.impulse_agent_permissions.normalize();
            updated = Some(config.impulse_agent_permissions.clone());
            true
        })
        .context("Failed to persist agent permissions to config")?;
        updated.context("agent permission update did not run")
    }

    /// The config as `config.json` holds it now, refreshing this process's
    /// copy. For decisions that must see a change another process made, such
    /// as a permission revoked from the CLI while the daemon runs; the atomic
    /// rename means an unlocked read never sees a torn file.
    pub fn current_config(&self) -> Result<Config> {
        let mut current = self
            .storage
            .read_json::<Config>(CONFIG_FILE)
            .context("Failed to read config from disk")?;
        let mut copy = self.config.write().map_err(lock_err)?;
        current.keep_process_local(&copy);
        *copy = current.clone();
        Ok(current)
    }

    /// Conflict analytics, built from the audit trail conflict detection
    /// appends to (`CONFLICTS.jsonl`). This used to read a separate
    /// `CONFLICTS.json` that only test code wrote, so `analytics conflicts`
    /// always reported none.
    pub fn get_conflict_analytics(&self) -> Result<ConflictHistory> {
        Ok(ConflictHistory::from_events(&self.get_conflict_history()?))
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

    /// One entry per detection event, with the sessions involved (the one
    /// that hit the conflict and those it conflicted with). The analytics
    /// count entries, so totals, per-day counts and the most common files are
    /// all counts of detections; grouping by file first made three
    /// detections on one file count as one. Nothing records resolutions
    /// yet, so every entry is unresolved.
    pub fn from_events(events: &[ConflictEvent]) -> Self {
        Self {
            conflict_history: events
                .iter()
                .map(|event| {
                    let mut involved_sessions = vec![event.session_id.clone()];
                    for session in &event.conflicting_sessions {
                        if !involved_sessions.contains(session) {
                            involved_sessions.push(session.clone());
                        }
                    }
                    ConflictEntry {
                        file_path: event.file_path.clone(),
                        detection_count: 1,
                        first_detected: event.detected_at,
                        last_detected: event.detected_at,
                        involved_sessions,
                        resolution: None,
                        resolved_at: None,
                    }
                })
                .collect(),
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
        most_common_files.sort_by_key(|b| std::cmp::Reverse(b.1));

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

    /// Two `State`s on one project stand in for two processes: a direct-mode
    /// hook and the daemon each load their own copy. Each used to save its
    /// copy whole, so the second save dropped the first one's session.
    #[tokio::test]
    async fn test_two_states_on_one_project_keep_each_others_sessions() {
        let dir = tempfile::TempDir::new().unwrap();
        let hook = State::new(dir.path().to_path_buf()).unwrap();
        let daemon = State::new(dir.path().to_path_buf()).unwrap();

        let first = hook.create_session("hook".to_string(), None).await.unwrap();
        let second = daemon
            .create_session("daemon".to_string(), None)
            .await
            .unwrap();
        // The daemon's copy predates the hook's session; tracking it must
        // still find it on disk.
        daemon.track_file(&first.id, "src/lib.rs").await.unwrap();

        let reopened = State::new(dir.path().to_path_buf()).unwrap();
        let sessions = reopened.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
        let tracked = reopened.get_session(&first.id).await.unwrap().unwrap();
        assert_eq!(tracked.active_files, vec!["src/lib.rs".to_string()]);
        assert!(reopened.get_session(&second.id).await.unwrap().is_some());
    }

    /// A stale copy must neither resurrect an ended session nor record its
    /// history twice.
    #[tokio::test]
    async fn test_a_session_ended_elsewhere_is_not_ended_twice() {
        let dir = tempfile::TempDir::new().unwrap();
        let first = State::new(dir.path().to_path_buf()).unwrap();
        let session = first.create_session("s".to_string(), None).await.unwrap();
        let stale = State::new(dir.path().to_path_buf()).unwrap();

        assert!(first
            .end_session(&session.id, "done".to_string())
            .await
            .unwrap()
            .is_some());
        assert!(stale
            .end_session(&session.id, "again".to_string())
            .await
            .unwrap()
            .is_none());
        stale
            .create_session("other".to_string(), None)
            .await
            .unwrap();

        assert_eq!(first.get_history_sync().unwrap().len(), 1);
        let reopened = State::new(dir.path().to_path_buf()).unwrap();
        assert!(reopened.get_session(&session.id).await.unwrap().is_none());
    }

    /// The daemon's copy of the config predates a `config` command run while
    /// it is up; recording a permission grant used to write that copy back.
    #[test]
    fn test_two_states_on_one_project_keep_each_others_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let daemon = State::new(dir.path().to_path_buf()).unwrap();
        let cli = State::new(dir.path().to_path_buf()).unwrap();

        assert!(cli.set_config("log_level", "debug").unwrap());
        daemon
            .update_impulse_agent_permissions(|policy| {
                *policy = impulse_ops::SupervisorPermissionPolicy::default();
            })
            .unwrap();

        let reopened = State::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened.get_config("log_level").unwrap().as_deref(),
            Some("debug")
        );
        assert_eq!(
            daemon.get_config("log_level").unwrap().as_deref(),
            Some("debug"),
            "the writer's own copy is refreshed from disk"
        );
    }

    /// The API key is never written to `config.json`; refreshing the copy
    /// from disk on the next update must not drop it.
    #[test]
    fn test_config_update_keeps_the_process_local_api_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        assert!(state
            .set_config("impulse_agent_api_key", "test-key")
            .unwrap());
        assert!(state.set_config("log_level", "debug").unwrap());

        assert_eq!(
            state
                .config_snapshot()
                .unwrap()
                .impulse_agent_api_key
                .as_deref(),
            Some("test-key")
        );
        let on_disk = std::fs::read_to_string(dir.path().join(CONFIG_FILE)).unwrap();
        assert!(!on_disk.contains("test-key"));
    }

    /// Review: the history append landed but the live-state save failed, so
    /// the session stayed and ending it again recorded it a second time.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_end_after_a_failed_save_does_not_record_the_session_twice() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        let session = state.create_session("s".to_string(), None).await.unwrap();
        // The history file exists, so its append still works when the
        // directory is read-only; the atomic live-state write does not.
        std::fs::write(dir.path().join(HISTORY_FILE), "").unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let first = state.end_session(&session.id, "done".to_string()).await;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(first.is_err(), "the live-state save should have failed");

        let second = state
            .end_session(&session.id, "again".to_string())
            .await
            .unwrap()
            .expect("the session is still recorded as live");
        assert_eq!(second.summary, "done");
        assert_eq!(state.get_history_sync().unwrap().len(), 1);
        assert!(state.get_session(&session.id).await.unwrap().is_none());
    }

    /// A permission revoked from the CLI while the daemon runs must survive
    /// the daemon's next grant, which used to start from its stale copy.
    #[test]
    fn test_a_permission_grant_keeps_a_revocation_made_elsewhere() {
        use impulse_ops::SupervisorActionPermission as Permission;
        let dir = tempfile::TempDir::new().unwrap();
        let daemon = State::new(dir.path().to_path_buf()).unwrap();
        let cli = State::new(dir.path().to_path_buf()).unwrap();
        assert!(daemon
            .config_snapshot()
            .unwrap()
            .impulse_agent_permissions
            .allows_action(Permission::ModifyPermissions));

        cli.update_impulse_agent_permissions(|policy| {
            policy
                .allowed_actions
                .retain(|action| *action != Permission::ModifyPermissions);
        })
        .unwrap();
        let granted = daemon
            .update_impulse_agent_permissions(|policy| policy.grant_action(Permission::FocusAgent))
            .unwrap();

        assert!(!granted.allows_action(Permission::ModifyPermissions));
        assert!(!daemon
            .current_config()
            .unwrap()
            .impulse_agent_permissions
            .allows_action(Permission::ModifyPermissions));
    }

    #[tokio::test]
    async fn test_failed_live_state_write_changes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        // A directory at the target path makes the final rename fail.
        std::fs::create_dir_all(dir.path().join(LIVE_STATE_FILE).join("blocker")).unwrap();

        assert!(state.create_session("s".to_string(), None).await.is_err());
        assert!(state.list_sessions().await.unwrap().is_empty());
    }

    #[test]
    fn test_corrupt_config_on_disk_fails_an_update_instead_of_overwriting_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        std::fs::write(dir.path().join(CONFIG_FILE), "{not json").unwrap();

        assert!(state.set_config("log_level", "debug").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(CONFIG_FILE)).unwrap(),
            "{not json"
        );
    }

    /// Review P2: try-locks on the Tokio RwLocks failed any call that
    /// overlapped another, which the daemon's concurrent connections do.
    #[test]
    fn test_reads_and_writes_wait_for_a_held_lock_instead_of_failing() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = std::sync::Arc::new(State::new(dir.path().to_path_buf()).unwrap());
        // `set_config` writes the config lock; holding it for reading makes
        // the writer wait (the try-lock version failed at once).
        let held = state.config.read().unwrap();
        let writer = std::sync::Arc::clone(&state);
        let handle = std::thread::spawn(move || writer.set_config("log_level", "debug"));
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(held);
        assert!(handle.join().unwrap().unwrap());
        assert_eq!(
            state.get_config("log_level").unwrap().as_deref(),
            Some("debug")
        );
    }

    /// Recorded review item: the state's write lock was held across the
    /// history append and its fsync, so every state reader waited on the
    /// disk. Writers still wait (the end holds the project lock), readers
    /// must not. A FIFO stands in for a slow disk: nobody reads it, so a history
    /// line larger than its buffer leaves the append waiting in `write`.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_end_session_does_not_hold_the_state_lock_while_writing_history() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let state = Arc::new(State::new(dir.path().to_path_buf()).unwrap());
        let ending = state.create_session("s".to_string(), None).await.unwrap();
        let other = state.create_session("t".to_string(), None).await.unwrap();
        let fifo = dir.path().join(HISTORY_FILE);
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo failed");

        let ender = Arc::clone(&state);
        let ending_id = ending.id.clone();
        let summary = "x".repeat(1024 * 1024);
        let end = tokio::spawn(async move { ender.end_session(&ending.id, summary).await });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!end.is_finished(), "the append did not wait on the FIFO");

        // A second end and a tracking update, as another process would send
        // them, wait for the end's lock instead of racing it. Run on a plain
        // thread so a blocked call cannot stall the runtime.
        let (writes_sent, writes_received) = std::sync::mpsc::channel();
        let writer = Arc::clone(&state);
        let writer_id = ending_id.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            let second_end = runtime.block_on(writer.end_session(&writer_id, "again".to_string()));
            let tracking = runtime.block_on(writer.track_file(&writer_id, "late.rs"));
            let _ = writes_sent.send((
                second_end.map(|entry| entry.is_none()).ok(),
                tracking.is_err(),
            ));
        });
        let writes_waited = writes_received
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err();

        // Read another session from a plain thread: a held std lock would
        // block it, and a runtime thread blocked that way could not time out.
        let (sent, received) = std::sync::mpsc::channel();
        let reader = Arc::clone(&state);
        let other_id = other.id.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            let found = runtime.block_on(reader.get_session(&other_id));
            let _ = sent.send(found.map(|session| session.is_some()).ok());
        });
        let readable = received.recv_timeout(std::time::Duration::from_secs(2));

        // Drain the FIFO without ever blocking, so the writer finishes and the
        // test cannot hang whatever the outcome.
        let mut drain = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let started = std::time::Instant::now();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match std::io::Read::read(&mut drain, &mut buf) {
                // A FIFO also reads as empty before the writer has connected;
                // closing it then breaks the writer's pipe and fails the end,
                // so keep draining until the end has actually finished.
                Ok(0)
                    if end.is_finished()
                        || started.elapsed() > std::time::Duration::from_secs(10) =>
                {
                    break
                }
                Ok(0) => std::thread::sleep(std::time::Duration::from_millis(5)),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if end.is_finished() || started.elapsed() > std::time::Duration::from_secs(10) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("draining the FIFO: {error}"),
            }
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), end).await;

        let writes = writes_received.recv_timeout(std::time::Duration::from_secs(10));

        assert_eq!(
            readable,
            Ok(Some(true)),
            "state blocked while history was written"
        );
        assert!(
            writes_waited,
            "a write did not wait for the end in progress"
        );
        // Whether the end itself succeeds depends on the platform's FIFO
        // semantics (Linux opens a FIFO read-write without a peer and then
        // refuses `fsync` on it), so only check that the waiting writes ran
        // once the lock was released. What they must find after an end is
        // checked on a regular file in
        // `test_writes_after_an_end_find_the_session_gone`.
        assert!(writes.is_ok(), "the waiting writes never ran");
    }

    /// Round 4/5 (reviewer A): a second end must not record the session
    /// again, and tracking it must not report success and then vanish.
    #[tokio::test]
    async fn test_writes_after_an_end_find_the_session_gone() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        let ending = state.create_session("s".to_string(), None).await.unwrap();
        let other = state.create_session("t".to_string(), None).await.unwrap();

        assert!(state
            .end_session(&ending.id, "done".to_string())
            .await
            .unwrap()
            .is_some());
        assert!(state
            .end_session(&ending.id, "again".to_string())
            .await
            .unwrap()
            .is_none());
        assert!(state.track_file(&ending.id, "late.rs").await.is_err());

        assert_eq!(state.get_history_sync().unwrap().len(), 1);
        let saved = std::fs::read_to_string(dir.path().join(LIVE_STATE_FILE)).unwrap();
        assert!(!saved.contains(&ending.id));
        assert!(saved.contains(&other.id));
    }

    /// Review P2: the session was removed before its history was appended,
    /// so a failed append lost both.
    #[tokio::test]
    async fn test_end_session_keeps_the_session_when_history_cannot_be_written() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = State::new(dir.path().to_path_buf()).unwrap();
        let session = state.create_session("s".to_string(), None).await.unwrap();
        std::fs::create_dir_all(dir.path().join(HISTORY_FILE)).unwrap();

        assert!(state
            .end_session(&session.id, "done".to_string())
            .await
            .is_err());
        assert!(state.get_session(&session.id).await.unwrap().is_some());

        std::fs::remove_dir(dir.path().join(HISTORY_FILE)).unwrap();
        assert!(state
            .end_session(&session.id, "done".to_string())
            .await
            .unwrap()
            .is_some());
        assert!(state.get_session(&session.id).await.unwrap().is_none());
    }

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
        assert_eq!(config.retrieval_ollama_url, "http://localhost:11434");
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
        assert!(config.set("retrieval_ollama_url", "http://localhost:11500"));
        assert_eq!(config.retrieval_ollama_url, "http://localhost:11500");
        assert!(!config.set("retrieval_ollama_url", ""));
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

    fn conflict_event(file_path: &str, session_id: &str, at: &str) -> ConflictEvent {
        ConflictEvent {
            file_path: file_path.to_string(),
            session_id: session_id.to_string(),
            conflicting_sessions: vec!["other".to_string()],
            detected_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    /// Refutation review of 1c74f05: grouping by file counted three
    /// detections on one file as one conflict.
    #[test]
    fn test_conflict_analytics_count_detections() {
        let history = ConflictHistory::from_events(&[
            conflict_event("src/main.rs", "s2", "2026-10-02T10:00:00Z"),
            conflict_event("src/lib.rs", "s1", "2026-10-03T10:00:00Z"),
            conflict_event("src/main.rs", "s1", "2026-10-01T10:00:00Z"),
        ]);

        assert_eq!(history.conflict_history.len(), 3);
        assert_eq!(
            history.conflict_history[0].involved_sessions,
            ["s2", "other"]
        );
        assert!(history.conflict_history[0].resolution.is_none());
        let analytics = history.get_analytics();
        assert_eq!(analytics.total_conflicts, 3);
        assert_eq!(
            analytics.most_common_files[0],
            ("src/main.rs".to_string(), 2)
        );
        assert_eq!(analytics.conflicts_by_day.len(), 3);
    }

    #[test]
    fn test_conflict_history_from_no_events_is_empty() {
        assert!(ConflictHistory::from_events(&[])
            .conflict_history
            .is_empty());
    }

    #[test]
    fn test_conflict_history_record_resolution() {
        let mut history = ConflictHistory::from_events(&[conflict_event(
            "src/main.rs",
            "session1",
            "2026-10-01T10:00:00Z",
        )]);
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
