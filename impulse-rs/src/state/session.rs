//! In-memory state with dirty-flag sync and Drop persistence.
//!
//! Core types: [`Config`] (runtime settings), [`State`] (session/file tracking),
//! [`LiveState`] (ephemeral session state). All wrapped in `Arc<RwLock<_>>`
//! for concurrent access. Syncs to `.impulse/` files only when dirty.

use chrono::{DateTime, Utc};
use impulse_ops::{AgentRole, MachineTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::storage::{get_working_dir_name, sanitize_filename};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub platform: Option<Platform>,
    pub working_directory: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub active_files: Vec<String>,
    pub recent_tools: Vec<String>,
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Role in coordinator/worker delegation pattern (Phase 2B).
    #[serde(default)]
    pub role: Option<AgentRole>,
    /// Parent session for worker sessions (Phase 2B).
    #[serde(default)]
    pub parent_session_id: Option<String>,
    /// Associated delegation ID (Phase 2B).
    #[serde(default)]
    pub delegation_id: Option<String>,
    /// Target machine where the agent operates (Phase 3A).
    #[serde(default)]
    pub target: Option<MachineTarget>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    ClaudeCode,
    Codex,
    OpenCode,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::ClaudeCode => "claude-code",
            Platform::Codex => "codex",
            Platform::OpenCode => "opencode",
        }
    }

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SessionStatus {
    #[default]
    Active,
    Idle,
    Waiting,
    Completed,
    Error,
}

impl Session {
    pub fn new(name: String, platform: Option<Platform>) -> Self {
        let now = Utc::now();
        let working_dir = get_working_dir_name();
        Self {
            id: format!(
                "{}-{}-{}",
                sanitize_filename(&working_dir),
                now.format("%Y%m%d-%H%M%S"),
                &uuid::Uuid::new_v4().to_string()[..8]
            ),
            name,
            platform,
            working_directory: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            status: SessionStatus::Active,
            created_at: now,
            last_activity: now,
            active_files: Vec::new(),
            recent_tools: Vec::new(),
            metadata: HashMap::new(),
            tags: Vec::new(),
            role: None,
            parent_session_id: None,
            delegation_id: None,
            target: None,
        }
    }

    pub fn add_tag(&mut self, tag: &str) {
        if !self.tags.contains(&tag.to_string()) {
            self.tags.push(tag.to_string());
        }
        self.last_activity = Utc::now();
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
        self.last_activity = Utc::now();
    }

    pub fn add_file(&mut self, path: &str) {
        if !self.active_files.contains(&path.to_string()) {
            self.active_files.push(path.to_string());
        }
        self.last_activity = Utc::now();
    }

    pub fn add_tool(&mut self, tool: &str) {
        self.recent_tools.retain(|t| t != tool);
        self.recent_tools.insert(0, tool.to_string());
        if self.recent_tools.len() > 20 {
            self.recent_tools.truncate(20);
        }
        self.last_activity = Utc::now();
    }

    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
        self.last_activity = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_codex_roundtrip_serializes_lowercase() {
        let json = serde_json::to_string(&Platform::Codex).unwrap();
        assert_eq!(json, "\"codex\"");
        let parsed: Platform = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Platform::Codex);
    }

    #[test]
    fn test_session_with_codex_platform_roundtrips() {
        let session = Session::new("codex-agent".to_string(), Some(Platform::Codex));
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"platform\":\"codex\""));
        let parsed: Session = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.platform, Some(Platform::Codex));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveState {
    pub sessions: HashMap<String, Session>,
    pub last_updated: DateTime<Utc>,
}

impl Default for LiveState {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveState {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            last_updated: Utc::now(),
        }
    }

    pub fn add_session(&mut self, session: Session) {
        self.sessions.insert(session.id.clone(), session);
        self.last_updated = Utc::now();
    }

    pub fn get_session(&self, id: &str) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn remove_session(&mut self, id: &str) -> Option<Session> {
        let removed = self.sessions.remove(id);
        if removed.is_some() {
            self.last_updated = Utc::now();
        }
        removed
    }

    pub fn list_sessions(&self) -> Vec<&Session> {
        self.sessions.values().collect()
    }
}
