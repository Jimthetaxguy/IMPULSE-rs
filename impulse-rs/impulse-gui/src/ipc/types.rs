//! Wire types matching the Impulse daemon JSON protocol.
//!
//! The daemon uses `#[serde(tag = "type", content = "data")]` internally-tagged
//! enums. Messages are newline-delimited JSON over a Unix domain socket.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request / Response envelopes (must match daemon exactly)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    Ping,
    Status,
    ListSessions,
    GetSession {
        session_id: String,
    },
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonResponse {
    Ok { result: serde_json::Value },
    Error { message: String },
    AgentAssistResult { success: bool, response: String },
}

// ---------------------------------------------------------------------------
// GUI domain types (deserialized from daemon JSON payloads)
// ---------------------------------------------------------------------------

/// Daemon status summary.
#[derive(Debug, Clone, Default)]
pub struct DaemonStatus {
    pub sessions: usize,
    pub active: usize,
}

/// An active or recently-ended session.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub status: String,
    pub created_at: String,
    #[allow(dead_code)]
    pub last_activity: String,
    pub active_files: Vec<String>,
    pub recent_tools: Vec<String>,
}

impl Session {
    /// Parse a session from the daemon's JSON value.
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            id: v
                .get("id")
                .or_else(|| v.get("session_id"))?
                .as_str()?
                .to_string(),
            name: v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unnamed")
                .to_string(),
            platform: v
                .get("platform")
                .and_then(|p| p.as_str())
                .unwrap_or("unknown")
                .to_string(),
            status: v
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("active")
                .to_string(),
            created_at: v
                .get("created_at")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
            last_activity: v
                .get("last_activity")
                .and_then(|l| l.as_str())
                .unwrap_or("")
                .to_string(),
            active_files: v
                .get("active_files")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            recent_tools: v
                .get("recent_tools")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// A history entry from HISTORY.jsonl (returned by daemon tool).
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub session_id: String,
    pub session_name: String,
    pub platform: String,
    pub started_at: String,
    pub ended_at: String,
    pub summary: String,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
}

impl HistoryEntry {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            session_id: v
                .get("session_id")
                .or_else(|| v.get("id"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            session_name: v
                .get("session_name")
                .or_else(|| v.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("unnamed")
                .to_string(),
            platform: v
                .get("platform")
                .and_then(|p| p.as_str())
                .unwrap_or("unknown")
                .to_string(),
            started_at: v
                .get("started_at")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            ended_at: v
                .get("ended_at")
                .and_then(|e| e.as_str())
                .unwrap_or("")
                .to_string(),
            summary: v
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            files_touched: v
                .get("files_touched")
                .or_else(|| v.get("files"))
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            tools_used: v
                .get("tools_used")
                .or_else(|| v.get("tools"))
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// Genome data — decisions, preferences, constraints.
#[derive(Debug, Clone, Default)]
pub struct Genome {
    pub decisions: Vec<Decision>,
    pub raw_text: String,
    pub last_updated: String,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub date: String,
    pub description: String,
    pub rationale: String,
    pub tags: Vec<String>,
}

impl Genome {
    pub fn from_value(v: &serde_json::Value) -> Self {
        let decisions = v
            .get("decisions")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|d| Decision {
                        date: d
                            .get("date")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: d
                            .get("description")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        rationale: d
                            .get("rationale")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        tags: d
                            .get("tags")
                            .and_then(|t| t.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|t| t.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Self {
            decisions,
            raw_text: v
                .get("raw")
                .or_else(|| v.get("content"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            last_updated: v
                .get("last_updated")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        }
    }
}

/// A search result from the daemon's retrieval system.
#[derive(Debug, Clone)]
pub struct SearchResult {
    #[allow(dead_code)]
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
    pub source_type: String,
}

impl SearchResult {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            id: v
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            title: v
                .get("title")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            snippet: v
                .get("snippet")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            score: v.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0) as f32,
            source_type: v
                .get("source_type")
                .or_else(|| v.get("type"))
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
        })
    }
}
