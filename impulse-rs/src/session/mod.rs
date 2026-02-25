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
