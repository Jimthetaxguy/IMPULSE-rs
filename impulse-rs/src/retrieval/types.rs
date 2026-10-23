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

    // ---- Serde round-trips for the persisted index state ----
    //
    // `IndexState` is read back from disk by `retrieval::status`; these types
    // lack PartialEq, so round-trips compare the serialized JSON values.

    fn fixed_time(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_json_round_trip<T>(original: &T) -> T
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let json = serde_json::to_string(original).unwrap();
        let recovered: T = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_value(original).unwrap(),
            serde_json::to_value(&recovered).unwrap()
        );
        recovered
    }

    fn sample_index_state() -> IndexState {
        IndexState {
            version: "1".into(),
            schema_version: 3,
            indexed_at: fixed_time("2026-06-01T12:00:00Z"),
            history_count: 42,
            genome_count: 7,
            vector_enabled: true,
            vector_available: false,
            last_index_duration_ms: 1500,
            last_integrity_check: Some(fixed_time("2026-06-01T12:01:00Z")),
            last_error_code: Some("sqlite_vec_unavailable".into()),
            backend_health: vec!["keyword_fts:ok".into(), "sqlite_vec:missing".into()],
            notes: vec!["rebuilt after schema bump".into()],
        }
    }

    #[test]
    fn round_trip_index_state() {
        let recovered = assert_json_round_trip(&sample_index_state());
        assert_eq!(recovered.schema_version, 3);
        assert_eq!(
            recovered.last_integrity_check,
            Some(fixed_time("2026-06-01T12:01:00Z"))
        );
    }

    #[test]
    fn test_index_state_from_before_optional_fields_existed_deserializes_with_defaults() {
        let json = r#"{"version":"1","indexed_at":"2026-02-25T00:00:00Z","history_count":1,"genome_count":2,"vector_enabled":false,"vector_available":false,"notes":[]}"#;
        let state: IndexState = serde_json::from_str(json).unwrap();
        assert_eq!(state.schema_version, default_schema_version());
        assert_eq!(state.last_index_duration_ms, 0);
        assert_eq!(state.last_integrity_check, None);
        assert_eq!(state.last_error_code, None);
        assert!(state.backend_health.is_empty());
        assert_eq!(state.history_count, 1);
    }

    #[test]
    fn round_trip_retrieval_status() {
        let original = RetrievalStatus {
            db_path: "/repo/.impulse/retrieval.db".into(),
            db_exists: true,
            db_size_bytes: 4096,
            integrity_ok: Some(true),
            integrity_message: Some("ok".into()),
            vector_extension_available: false,
            python_available: true,
            index_state: sample_index_state(),
            injection: InjectionStatus {
                config_mode: "auto".into(),
                config_scope: "history".into(),
                emit_artifacts: false,
                staged_artifact_count: 2,
                last_staged_at: Some("2026-06-01T12:02:00Z".into()),
                last_staged_surface: Some("claude".into()),
                last_staged_status: Some("staged".into()),
                last_staged_artifact: Some("ctx-1.md".into()),
            },
        };
        let recovered = assert_json_round_trip(&original);
        assert_eq!(recovered.injection.staged_artifact_count, 2);
        assert_eq!(recovered.index_state.history_count, 42);
    }

    #[test]
    fn test_retrieval_status_without_injection_uses_default_injection() {
        let mut value = serde_json::to_value(RetrievalStatus {
            db_path: "p".into(),
            db_exists: false,
            db_size_bytes: 0,
            integrity_ok: None,
            integrity_message: None,
            vector_extension_available: false,
            python_available: false,
            index_state: sample_index_state(),
            injection: InjectionStatus::default(),
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("injection");
        let status: RetrievalStatus = serde_json::from_value(value).unwrap();
        assert_eq!(status.injection.config_mode, "review");
        assert_eq!(status.injection.config_scope, "both");
        assert!(status.injection.emit_artifacts);
    }

    #[test]
    fn test_retrieval_mode_wire_names_are_pinned() {
        // Serde uses the variant name, not `as_str`'s lowercase form.
        for (mode, wire) in [
            (RetrievalMode::Keyword, "\"Keyword\""),
            (RetrievalMode::Semantic, "\"Semantic\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), wire);
            assert_eq!(serde_json::from_str::<RetrievalMode>(wire).unwrap(), mode);
        }
    }

    #[test]
    fn test_search_backend_wire_names_are_pinned() {
        for (backend, wire) in [
            (SearchBackend::Auto, "\"Auto\""),
            (SearchBackend::SqliteVec, "\"SqliteVec\""),
            (SearchBackend::RustCosine, "\"RustCosine\""),
            (SearchBackend::Keyword, "\"Keyword\""),
        ] {
            assert_eq!(serde_json::to_string(&backend).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<SearchBackend>(wire).unwrap(),
                backend
            );
        }
    }

    #[test]
    fn test_index_scope_wire_names_are_pinned() {
        for (scope, wire) in [
            (IndexScope::History, "\"History\""),
            (IndexScope::Genome, "\"Genome\""),
            (IndexScope::All, "\"All\""),
        ] {
            assert_eq!(serde_json::to_string(&scope).unwrap(), wire);
            assert_eq!(serde_json::from_str::<IndexScope>(wire).unwrap(), scope);
        }
    }
}
