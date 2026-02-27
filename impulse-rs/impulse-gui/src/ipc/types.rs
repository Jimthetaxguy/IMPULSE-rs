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
    CreateSession {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
    },
    EndSession {
        session_id: String,
        summary: String,
    },
    TrackFile {
        session_id: String,
        file_path: String,
    },
    TrackTool {
        session_id: String,
        tool_name: String,
    },
    Chat {
        session_id: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        inject_mode: Option<String>,
        #[serde(default)]
        inject_explain: bool,
    },
    StewardStatus,
    StewardProposals {
        action: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    StewardMemory,
    ListTools {
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<String>,
    },
    DescribeTool {
        name: String,
    },
    InvokeTool {
        name: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    ToolSchema,
    AgentAssist {
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
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

// ---------------------------------------------------------------------------
// Phase 3 forward-declared types (consumed when Stewardship/Tools views land)
// ---------------------------------------------------------------------------

/// A daemon tool descriptor (from ListTools response).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub category: String,
    pub params: Vec<ToolParam>,
}

/// A parameter for a daemon tool.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ToolParam {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub description: String,
}

#[allow(dead_code)]
impl ToolInfo {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: v.get("name").and_then(|s| s.as_str())?.to_string(),
            description: v
                .get("description")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            category: v
                .get("category")
                .and_then(|s| s.as_str())
                .unwrap_or("uncategorized")
                .to_string(),
            params: v
                .get("params")
                .or_else(|| v.get("parameters"))
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().filter_map(ToolParam::from_value).collect())
                .unwrap_or_default(),
        })
    }
}

#[allow(dead_code)]
impl ToolParam {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: v.get("name").and_then(|s| s.as_str())?.to_string(),
            param_type: v
                .get("type")
                .and_then(|s| s.as_str())
                .unwrap_or("string")
                .to_string(),
            required: v.get("required").and_then(|b| b.as_bool()).unwrap_or(false),
            description: v
                .get("description")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Stewardship types
// ---------------------------------------------------------------------------

/// Stewardship system status.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StewardshipStatus {
    pub mode: String,
    pub monitor_threshold: f32,
    pub surgical_threshold: f32,
    pub thoughtful_threshold: f32,
    pub emergency_threshold: f32,
    pub pending_proposals: usize,
}

#[allow(dead_code)]
impl StewardshipStatus {
    pub fn from_value(v: &serde_json::Value) -> Self {
        Self {
            mode: v
                .get("mode")
                .and_then(|s| s.as_str())
                .unwrap_or("review")
                .to_string(),
            monitor_threshold: v
                .get("monitor_threshold")
                .and_then(|n| n.as_f64())
                .unwrap_or(0.30) as f32,
            surgical_threshold: v
                .get("surgical_threshold")
                .and_then(|n| n.as_f64())
                .unwrap_or(0.45) as f32,
            thoughtful_threshold: v
                .get("thoughtful_threshold")
                .and_then(|n| n.as_f64())
                .unwrap_or(0.60) as f32,
            emergency_threshold: v
                .get("emergency_threshold")
                .and_then(|n| n.as_f64())
                .unwrap_or(0.80) as f32,
            pending_proposals: v
                .get("pending_proposals")
                .or_else(|| v.get("pending"))
                .and_then(|n| n.as_u64())
                .unwrap_or(0) as usize,
        }
    }
}

/// A stewardship proposal (compaction/cleanup action).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: String,
    pub action: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
}

#[allow(dead_code)]
impl Proposal {
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        Some(Self {
            id: v.get("id").and_then(|s| s.as_str())?.to_string(),
            action: v
                .get("action")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            description: v
                .get("description")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            status: v
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("pending")
                .to_string(),
            created_at: v
                .get("created_at")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// Cross-project memory patterns from stewardship.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct CrossProjectMemory {
    pub patterns: Vec<String>,
    pub learnings: Vec<String>,
}

#[allow(dead_code)]
impl CrossProjectMemory {
    pub fn from_value(v: &serde_json::Value) -> Self {
        fn strings_from(v: &serde_json::Value, key: &str) -> Vec<String> {
            v.get(key)
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        }
        Self {
            patterns: strings_from(v, "patterns"),
            learnings: strings_from(v, "learnings"),
        }
    }
}

// ---------------------------------------------------------------------------
// Chat response types
// ---------------------------------------------------------------------------

/// Parsed response from the daemon Chat endpoint.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub injection_applied: bool,
    pub injection_surface: Option<String>,
}

#[allow(dead_code)]
impl ChatResponse {
    pub fn from_value(v: &serde_json::Value) -> Self {
        Self {
            content: v
                .get("content")
                .or_else(|| v.get("response"))
                .or_else(|| v.get("text"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            injection_applied: v
                .get("injection_applied")
                .or_else(|| v.get("applied"))
                .and_then(|b| b.as_bool())
                .unwrap_or(false),
            injection_surface: v
                .get("injection_surface")
                .or_else(|| v.get("surface"))
                .and_then(|s| s.as_str())
                .map(String::from),
        }
    }
}
