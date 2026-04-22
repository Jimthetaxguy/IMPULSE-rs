use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::retrieval::types::FallbackCode;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InjectionMode {
    Off,
    Review,
    Apply,
}

impl InjectionMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "review" => Some(Self::Review),
            "apply" => Some(Self::Apply),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Review => "review",
            Self::Apply => "apply",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InjectionScope {
    Daemon,
    Direct,
    Both,
}

impl InjectionScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "daemon" => Some(Self::Daemon),
            "direct" => Some(Self::Direct),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Direct => "direct",
            Self::Both => "both",
        }
    }

    pub fn allows(self, surface: InjectionSurface) -> bool {
        match self {
            Self::Both => true,
            Self::Daemon => matches!(surface, InjectionSurface::DaemonChat),
            Self::Direct => !matches!(surface, InjectionSurface::DaemonChat),
        }
    }

    /// Whether this scope allows agent pane injections (init + refresh).
    pub fn allows_agent_pane(self) -> bool {
        matches!(self, Self::Direct | Self::Both)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InjectionSurface {
    DaemonChat,
    Orchestrate,
    Handoff,
    SyncContext,
    AgentPaneInit,
    AgentPaneRefresh,
}

impl InjectionSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DaemonChat => "daemon_chat",
            Self::Orchestrate => "orchestrate",
            Self::Handoff => "handoff",
            Self::SyncContext => "sync_context",
            Self::AgentPaneInit => "agent_pane_init",
            Self::AgentPaneRefresh => "agent_pane_refresh",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionSnippet {
    pub source: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionBundle {
    pub schema_version: u32,
    pub generated_at: DateTime<Utc>,
    pub source_surface: String,
    pub mode: String,
    pub query: String,
    pub query_terms: Vec<String>,
    pub retrieval_mode: String,
    pub backend_used: String,
    pub used_fallback: bool,
    pub fallback_code: Option<FallbackCode>,
    pub timing_ms: u64,
    pub candidate_count: usize,
    pub engine_notes: Vec<String>,
    pub snippets: Vec<InjectionSnippet>,
    pub total_chars: usize,
    pub bundle_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionExplain {
    pub mode_requested: String,
    pub mode_effective: String,
    pub scope: String,
    pub retrieval_mode: String,
    pub backend_used: String,
    pub used_fallback: bool,
    pub fallback_code: Option<FallbackCode>,
    pub timing_ms: u64,
    pub candidate_count: usize,
    pub engine_notes: Vec<String>,
    pub status: String,
    pub artifact_path: Option<String>,
    pub deduped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionRunResult {
    pub surface: String,
    pub requested_mode: String,
    pub effective_mode: String,
    pub applied: bool,
    pub injected_block: Option<String>,
    pub artifact_path: Option<String>,
    pub deduped: bool,
    pub skipped_reason: Option<String>,
    pub explain: InjectionExplain,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<InjectionBundle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub status: String,
    pub artifact_path: Option<String>,
    pub deduped: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_injection_mode_parse() {
        assert_eq!(InjectionMode::parse("off"), Some(InjectionMode::Off));
        assert_eq!(InjectionMode::parse("review"), Some(InjectionMode::Review));
        assert_eq!(InjectionMode::parse("apply"), Some(InjectionMode::Apply));
        assert_eq!(InjectionMode::parse("invalid"), None);
    }

    #[test]
    fn test_injection_mode_as_str() {
        assert_eq!(InjectionMode::Off.as_str(), "off");
        assert_eq!(InjectionMode::Review.as_str(), "review");
        assert_eq!(InjectionMode::Apply.as_str(), "apply");
    }

    #[test]
    fn test_injection_scope_parse() {
        assert_eq!(
            InjectionScope::parse("daemon"),
            Some(InjectionScope::Daemon)
        );
        assert_eq!(
            InjectionScope::parse("direct"),
            Some(InjectionScope::Direct)
        );
        assert_eq!(InjectionScope::parse("both"), Some(InjectionScope::Both));
        assert_eq!(InjectionScope::parse("invalid"), None);
    }
}
