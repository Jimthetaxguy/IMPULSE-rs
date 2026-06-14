use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetrievalMode {
    Keyword,
    Semantic,
}

impl RetrievalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "keyword" => Some(Self::Keyword),
            "semantic" => Some(Self::Semantic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Semantic => "semantic",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchBackend {
    Auto,
    SqliteVec,
    RustCosine,
    Keyword,
}

impl SearchBackend {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "sqlite-vec" => Some(Self::SqliteVec),
            "rust-cosine" => Some(Self::RustCosine),
            "keyword" => Some(Self::Keyword),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::SqliteVec => "sqlite-vec",
            Self::RustCosine => "rust-cosine",
            Self::Keyword => "keyword",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexScope {
    History,
    Genome,
    All,
}

impl IndexScope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "history" => Some(Self::History),
            "genome" => Some(Self::Genome),
            "all" => Some(Self::All),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub source: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchResponse {
    pub mode: String,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
    pub fallback_code: Option<FallbackCode>,
    pub backend_used: String,
    pub timing_ms: u64,
    pub candidate_count: usize,
    #[serde(default)]
    pub total_count: Option<usize>,
    #[serde(default)]
    pub engine_notes: Vec<String>,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackCode {
    VectorBackendDisabled,
    SqliteVecUnavailable,
    EmbeddingTimeout,
    EmbeddingSpawnFailed,
    EmbeddingProcessFailed,
    EmbeddingNoVector,
    EmbeddingDimensionMismatch,
    RetrievalDbError,
    RetrievalDbCorrupt,
    IndexLockActive,
}

impl FallbackCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FallbackCode::VectorBackendDisabled => "vector_backend_disabled",
            FallbackCode::SqliteVecUnavailable => "sqlite_vec_unavailable",
            FallbackCode::EmbeddingTimeout => "embedding_timeout",
            FallbackCode::EmbeddingSpawnFailed => "embedding_spawn_failed",
            FallbackCode::EmbeddingProcessFailed => "embedding_process_failed",
            FallbackCode::EmbeddingNoVector => "embedding_no_vector",
            FallbackCode::EmbeddingDimensionMismatch => "embedding_dimension_mismatch",
            FallbackCode::RetrievalDbError => "retrieval_db_error",
            FallbackCode::RetrievalDbCorrupt => "retrieval_db_corrupt",
            FallbackCode::IndexLockActive => "index_lock_active",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexState {
    pub version: String,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub indexed_at: DateTime<Utc>,
    pub history_count: usize,
    pub genome_count: usize,
    pub vector_enabled: bool,
    pub vector_available: bool,
    #[serde(default)]
    pub last_index_duration_ms: u64,
    #[serde(default)]
    pub last_integrity_check: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error_code: Option<String>,
    #[serde(default)]
    pub backend_health: Vec<String>,
    pub notes: Vec<String>,
}

fn default_schema_version() -> u32 {
    2
}

impl Default for IndexState {
    fn default() -> Self {
        Self {
            version: "1".to_string(),
            schema_version: default_schema_version(),
            indexed_at: Utc::now(),
            history_count: 0,
            genome_count: 0,
            vector_enabled: false,
            vector_available: false,
            last_index_duration_ms: 0,
            last_integrity_check: None,
            last_error_code: None,
            backend_health: Vec::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalStatus {
    pub db_path: String,
    pub db_exists: bool,
    pub db_size_bytes: u64,
    pub integrity_ok: Option<bool>,
    pub integrity_message: Option<String>,
    pub vector_extension_available: bool,
    pub python_available: bool,
    pub index_state: IndexState,
    #[serde(default)]
    pub injection: InjectionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionStatus {
    pub config_mode: String,
    pub config_scope: String,
    pub emit_artifacts: bool,
    pub staged_artifact_count: usize,
    pub last_staged_at: Option<String>,
    pub last_staged_surface: Option<String>,
    pub last_staged_status: Option<String>,
    pub last_staged_artifact: Option<String>,
}

impl Default for InjectionStatus {
    fn default() -> Self {
        Self {
            config_mode: "review".to_string(),
            config_scope: "both".to_string(),
            emit_artifacts: true,
            staged_artifact_count: 0,
            last_staged_at: None,
            last_staged_surface: None,
            last_staged_status: None,
            last_staged_artifact: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalHealth {
    pub sqlite_vec: bool,
    pub rust_cosine: bool,
    pub keyword_fts: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retrieval_mode_parse_as_str_round_trip() {
        for mode in [RetrievalMode::Keyword, RetrievalMode::Semantic] {
            assert_eq!(RetrievalMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(RetrievalMode::parse("nope"), None);
    }

    #[test]
    fn test_search_backend_parse_as_str_round_trip() {
        // Includes the hyphenated forms where a parse/as_str mismatch would
        // silently break backend selection.
        for backend in [
            SearchBackend::Auto,
            SearchBackend::SqliteVec,
            SearchBackend::RustCosine,
            SearchBackend::Keyword,
        ] {
            assert_eq!(SearchBackend::parse(backend.as_str()), Some(backend));
        }
        assert_eq!(SearchBackend::parse("sqlite_vec"), None); // underscore is not the wire form
        assert_eq!(SearchBackend::parse("unknown"), None);
    }

    #[test]
    fn test_index_scope_parse() {
        assert_eq!(IndexScope::parse("history"), Some(IndexScope::History));
        assert_eq!(IndexScope::parse("genome"), Some(IndexScope::Genome));
        assert_eq!(IndexScope::parse("all"), Some(IndexScope::All));
        assert_eq!(IndexScope::parse("everything"), None);
    }

    #[test]
    fn test_fallback_code_as_str_matches_serde() {
        // FallbackCode carries TWO wire representations — as_str() and the serde
        // snake_case rename. They must agree, or a value serialized via one path
        // can't be read via the other.
        let all = [
            FallbackCode::VectorBackendDisabled,
            FallbackCode::SqliteVecUnavailable,
            FallbackCode::EmbeddingTimeout,
            FallbackCode::EmbeddingSpawnFailed,
            FallbackCode::EmbeddingProcessFailed,
            FallbackCode::EmbeddingNoVector,
            FallbackCode::EmbeddingDimensionMismatch,
            FallbackCode::RetrievalDbError,
            FallbackCode::RetrievalDbCorrupt,
            FallbackCode::IndexLockActive,
        ];
        for code in all {
            let serde_form = serde_json::to_value(code).unwrap();
            assert_eq!(
                serde_form.as_str(),
                Some(code.as_str()),
                "as_str() and serde disagree for {code:?}"
            );
            // And the serde form round-trips back to the same variant.
            let back: FallbackCode = serde_json::from_value(serde_form).unwrap();
            assert_eq!(back, code);
        }
    }

    #[test]
    fn test_search_response_serde_round_trip() {
        let original = SearchResponse {
            mode: "semantic".to_string(),
            used_fallback: true,
            fallback_reason: Some("backend down".to_string()),
            fallback_code: Some(FallbackCode::SqliteVecUnavailable),
            backend_used: "keyword".to_string(),
            timing_ms: 12,
            candidate_count: 3,
            total_count: Some(3),
            engine_notes: vec!["note".to_string()],
            results: vec![SearchResult {
                source: "history".to_string(),
                id: "s1".to_string(),
                title: "t".to_string(),
                snippet: "snip".to_string(),
                score: 0.91,
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode, original.mode);
        assert_eq!(back.fallback_code, original.fallback_code);
        assert_eq!(back.results.len(), 1);
        assert_eq!(back.results[0].score, 0.91);
    }
}
