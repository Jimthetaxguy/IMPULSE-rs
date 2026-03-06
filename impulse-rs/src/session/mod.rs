//! Session tracking data structures and event types.
//!
//! Defines [`Session`] (active agent session with file/tool tracking) and
//! [`SessionEvent`] (timestamped lifecycle events). Used by the daemon for
//! multi-session state management. Phase 2 API surface.

#![allow(dead_code)]
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub status: SessionStatus,
    pub working_directory: Option<String>,
    pub active_files: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Idle,
    Waiting,
    Completed,
    Error,
}

impl Session {
    pub fn new(name: String, working_directory: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            created_at: now,
            last_activity: now,
            status: SessionStatus::Active,
            working_directory,
            active_files: Vec::new(),
            metadata: HashMap::new(),
            tags: Vec::new(),
        }
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
        self.status = SessionStatus::Active;
    }

    pub fn add_file(&mut self, file: String) {
        if !self.active_files.contains(&file) {
            self.active_files.push(file);
        }
        self.update_activity();
    }

    pub fn remove_file(&mut self, file: &str) {
        self.active_files.retain(|f| f != file);
    }

    pub fn set_status(&mut self, status: SessionStatus) {
        self.status = status;
    }
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub session_id: String,
    pub event_type: SessionEventType,
    pub timestamp: DateTime<Utc>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEventType {
    Started {
        name: String,
    },
    FileOpened {
        path: String,
    },
    FileEdited {
        path: String,
    },
    CommandExecuted {
        command: String,
    },
    AgentMessage {
        agent_id: String,
    },
    StatusChanged {
        from: SessionStatus,
        to: SessionStatus,
    },
    Idle,
    Completed,
    Error {
        message: String,
    },
}

impl SessionEvent {
    pub fn new(session_id: String, event_type: SessionEventType) -> Self {
        Self {
            session_id,
            event_type,
            timestamp: Utc::now(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: String) -> Self {
        self.data = Some(data);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new_fields() {
        let session = Session::new("test-session".into(), Some("/tmp".into()));
        assert_eq!(session.name, "test-session");
        assert_eq!(session.working_directory.as_deref(), Some("/tmp"));
        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.active_files.is_empty());
        assert!(!session.id.is_empty());
    }

    #[test]
    fn test_session_update_activity_changes_timestamp() {
        let mut session = Session::new("test".into(), None);
        let before = session.last_activity;
        std::thread::sleep(std::time::Duration::from_millis(10));
        session.set_status(SessionStatus::Idle);
        session.update_activity();
        assert!(session.last_activity >= before);
        assert_eq!(session.status, SessionStatus::Active);
    }

    #[test]
    fn test_session_add_file_dedup() {
        let mut session = Session::new("test".into(), None);
        session.add_file("main.rs".into());
        session.add_file("main.rs".into());
        session.add_file("lib.rs".into());
        assert_eq!(session.active_files.len(), 2);
    }

    #[test]
    fn test_session_remove_file() {
        let mut session = Session::new("test".into(), None);
        session.add_file("main.rs".into());
        session.add_file("lib.rs".into());
        session.remove_file("main.rs");
        assert_eq!(session.active_files, vec!["lib.rs"]);
    }

    #[test]
    fn test_session_status_serde_roundtrip() {
        let status = SessionStatus::Active;
        let json = serde_json::to_string(&status).unwrap();
        let parsed: SessionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_session_event_new_and_with_data() {
        let event = SessionEvent::new("s1".into(), SessionEventType::Idle);
        assert_eq!(event.session_id, "s1");
        assert!(event.data.is_none());

        let event = event.with_data("extra".into());
        assert_eq!(event.data.as_deref(), Some("extra"));
    }

    #[test]
    fn test_session_event_type_serde_roundtrip() {
        let event_type = SessionEventType::FileEdited {
            path: "src/main.rs".into(),
        };
        let json = serde_json::to_string(&event_type).unwrap();
        let parsed: SessionEventType = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SessionEventType::FileEdited { path } if path == "src/main.rs"));
    }
}
