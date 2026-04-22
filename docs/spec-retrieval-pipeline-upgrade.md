# Retrieval Pipeline Upgrade — Impulse Spec

> **Date:** 2026-03-02
> **Status:** Draft
> **Companion:** `~/.ai-memory/docs/spec-semantic-retrieval-pipeline.md` (cross-platform companion spec, not checked into this repo)
> **Source:** NullClaw analysis §2.2, existing Impulse retrieval system

---

## 1. Current State

### Existing Types (`src/retrieval/types.rs`)

```rust
// Line 5
pub enum RetrievalMode {
    Keyword,    // FTS5 BM25
    Semantic,   // Cosine similarity (sqlite-vec or rust in-memory)
}

// Line 28
pub enum SearchBackend {
    Auto,        // Try SqliteVec → RustCosine fallback
    SqliteVec,   // vec0 virtual table KNN
    RustCosine,  // In-memory cosine over history_vec JSON
    Keyword,     // FTS5 only
}

// Line 75
pub struct SearchResult {
    pub source: String,   // "history" or "genome"
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,       // single score — no per-stage breakdown
}

// Line 84
pub struct SearchResponse {
    pub mode: String,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
    pub fallback_code: Option<FallbackCode>,
    pub backend_used: String,
    pub timing_ms: u64,
    pub candidate_count: usize,
    pub engine_notes: Vec<String>,
    pub results: Vec<SearchResult>,
}
```

### Existing Search Flow (`src/retrieval/query.rs`)

```
search_history(query)
    │
    ├─ resolve_mode() → Keyword or Semantic
    │
    ├─ [Keyword] ──▶ store.search_history_keyword(query, limit)
    │                     │
    │                     └─ FTS5 MATCH with BM25, fallback to LIKE
    │
    └─ [Semantic] ─▶ resolve_semantic_backends()
                         │
                         ├─ [SqliteVec] ──▶ embed_texts() → vec0 KNN
                         │                     distance → similarity: 1/(1+d)
                         │
                         └─ [RustCosine] ─▶ embed_texts() → read all vectors
                                               → cosine_similarity() per doc
                                               → sort, threshold, limit
```

Key function: `cosine_similarity()` at `query.rs:16` — inline f32 dot product.

### Storage Schema (`src/retrieval/store.rs`)

```
  ┌──────────────────────────────────────────────────────────────┐
  │  retrieval.db (SQLite WAL mode)                              │
  │                                                              │
  │  ┌─────────────────┐  ┌────────────────┐                    │
  │  │ history_entries  │  │ genome_decisions│  Standard tables   │
  │  └────────┬────────┘  └───────┬────────┘                    │
  │           │                   │                              │
  │  ┌────────▼────────┐  ┌──────▼─────────┐                    │
  │  │  history_fts    │  │  genome_fts    │  FTS5 virtual       │
  │  │  (session_id,   │  │  (decision_id, │  tables (BM25)      │
  │  │   search_text)  │  │   search_text) │                    │
  │  └─────────────────┘  └────────────────┘                    │
  │                                                              │
  │  ┌─────────────────┐  ┌────────────────┐                    │
  │  │  history_vec    │  │  genome_vec    │  JSON vectors       │
  │  │  (vector_json)  │  │  (vector_json) │  (Rust fallback)    │
  │  └─────────────────┘  └────────────────┘                    │
  │                                                              │
  │  ┌─────────────────┐  ┌────────────────┐                    │
  │  │  history_vec0   │  │  genome_vec0   │  sqlite-vec KNN     │
  │  │  (embedding     │  │  (embedding    │  virtual tables     │
  │  │   float[384])   │  │   float[384])  │                    │
  │  └─────────────────┘  └────────────────┘                    │
  └──────────────────────────────────────────────────────────────┘
```

### Gap Analysis: 9 Stages vs Current Impulse

```
  ┌────┬───────────────────┬─────────────────────────────────────────────┐
  │ #  │ Stage             │ Impulse Status                              │
  ├────┼───────────────────┼─────────────────────────────────────────────┤
  │ 1  │ Query Expansion   │ MISSING — raw query passed to FTS5/embed   │
  │ 2  │ Keyword Search    │ EXISTS  — FTS5 BM25 via store.rs           │
  │ 3  │ Vector Search     │ EXISTS  — cosine via query.rs (2 backends) │
  │ 4  │ Merge RRF         │ MISSING — keyword OR vector, never both    │
  │ 5  │ Min Relevance     │ PARTIAL — retrieval_similarity_threshold   │
  │    │                   │           config, only for semantic mode    │
  │ 6  │ Temporal Decay    │ MISSING — no age-based score adjustment    │
  │ 7  │ MMR Diversity     │ MISSING — no redundancy filtering          │
  │ 8  │ LLM Rerank        │ MISSING — no LLM-based reranking          │
  │ 9  │ Limit             │ EXISTS  — limit parameter on all searches  │
  └────┴───────────────────┴─────────────────────────────────────────────┘
```

**Key structural gap:** Keyword and Semantic are mutually exclusive modes (`RetrievalMode`
enum). There is no Hybrid mode that runs both and merges results via RRF.

---

## 2. Proposed Types

### RetrievalStage Enum

```rust
/// Pipeline stages in canonical execution order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetrievalStage {
    QueryExpansion,    // Stage 1
    KeywordSearch,     // Stage 2
    VectorSearch,      // Stage 3
    MergeRrf,          // Stage 4
    MinRelevance,      // Stage 5
    TemporalDecay,     // Stage 6
    Mmr,               // Stage 7
    LlmRerank,         // Stage 8
    Limit,             // Stage 9
}

impl RetrievalStage {
    pub fn pipeline_order(&self) -> u8 {
        match self {
            Self::QueryExpansion => 1,
            Self::KeywordSearch  => 2,
            Self::VectorSearch   => 3,
            Self::MergeRrf       => 4,
            Self::MinRelevance   => 5,
            Self::TemporalDecay  => 6,
            Self::Mmr            => 7,
            Self::LlmRerank      => 8,
            Self::Limit          => 9,
        }
    }
}
```

### PipelineConfig

```rust
/// Configuration for the full retrieval pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub enabled: bool,

    // Per-stage config
    pub query_expansion: QueryExpansionConfig,
    pub keyword: KeywordStageConfig,
    pub vector: VectorStageConfig,
    pub merge_rrf: MergeRrfConfig,
    pub min_relevance: MinRelevanceConfig,
    pub temporal_decay: TemporalDecayConfig,
    pub mmr: MmrConfig,
    pub llm_rerank: LlmRerankConfig,
    pub limit: LimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRrfConfig {
    pub enabled: bool,
    pub k: u32,              // smoothing constant (default 60)
    pub keyword_weight: f64, // default 0.3
    pub vector_weight: f64,  // default 0.7
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalDecayConfig {
    pub enabled: bool,
    pub lambda: f64,  // decay rate (default 0.01)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmrConfig {
    pub enabled: bool,
    pub lambda: f64,      // relevance vs diversity (default 0.7)
    pub target_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRerankConfig {
    pub enabled: bool,
    pub max_candidates: usize,
}
```

### ScoredResult with Per-Stage Breakdown

```rust
/// Extended search result with per-stage score fields
#[derive(Debug, Clone)]
pub struct ScoredResult {
    pub id: String,
    pub source: String,
    pub title: String,
    pub snippet: String,

    // Per-stage scores (None if stage was skipped)
    pub keyword_score: Option<f64>,    // BM25
    pub vector_score: Option<f64>,     // cosine similarity
    pub rrf_score: Option<f64>,        // fused RRF score
    pub decayed_score: Option<f64>,    // after temporal decay
    pub final_score: f64,              // score used for final ranking

    // Metadata
    pub created_at: Option<DateTime<Utc>>,
    pub embedding: Option<Vec<f32>>,   // needed for MMR computation
}
```

---

## 3. Module Structure

```
  src/retrieval/
  ├── mod.rs              # Existing — add pipeline entry point
  ├── types.rs            # Existing — add RetrievalStage, ScoredResult
  ├── query.rs            # Existing — refactor to use pipeline
  ├── store.rs            # Existing — no changes needed
  ├── embedding.rs        # Existing — no changes needed
  ├── indexer.rs          # Existing — no changes needed
  │
  ├── pipeline.rs         # NEW — Pipeline orchestrator
  │                       #   pub fn run_pipeline(config, query, store) -> Vec<ScoredResult>
  │                       #   Executes stages in order, skips disabled stages
  │
  └── stages/
      ├── mod.rs           # NEW — Stage trait + re-exports
      ├── query_expansion.rs  # NEW — Stopword filtering, synonym expansion
      ├── merge_rrf.rs        # NEW — Reciprocal Rank Fusion
      ├── temporal_decay.rs   # NEW — Exponential decay by age
      ├── mmr.rs              # NEW — Maximal Marginal Relevance
      └── llm_rerank.rs       # NEW — LLM-based reranking (optional)
```

### Stage Trait

```rust
/// Each pipeline stage implements this trait
pub trait PipelineStage: Send + Sync {
    fn name(&self) -> &str;
    fn stage(&self) -> RetrievalStage;
    fn is_enabled(&self, config: &PipelineConfig) -> bool;
    fn execute(
        &self,
        results: Vec<ScoredResult>,
        context: &StageContext,
    ) -> Result<Vec<ScoredResult>>;
}

/// Shared context passed to all stages
pub struct StageContext<'a> {
    pub query: &'a str,
    pub expanded_query: Option<&'a str>,
    pub query_embedding: Option<&'a [f32]>,
    pub config: &'a PipelineConfig,
    pub store: &'a RetrievalStore,
}
```

---

## 4. Migration Path

### Phase A: Add Types (non-breaking)

Add `RetrievalStage`, `PipelineConfig`, `ScoredResult` to `types.rs`. Add
`RetrievalMode::Hybrid` variant. No behavior changes.

### Phase B: Extract Existing Stages

Refactor `search_history_keyword()` and `semantic_history_rust()` into `PipelineStage`
implementations. Existing behavior preserved — just reorganized.

### Phase C: Implement RRF + Min Relevance

Add `merge_rrf.rs` and wire it into the Hybrid path. This is the first new capability —
running keyword AND vector in parallel, then fusing with RRF.

### Phase D: Implement Temporal Decay + MMR

Add `temporal_decay.rs` and `mmr.rs`. These operate on the fused result list.
Temporal decay requires `created_at` on `ScoredResult` (already available from
`history_entries.ended_at`).

### Phase E: Implement LLM Rerank

Add `llm_rerank.rs`. This requires a working `LlmProvider` (see Spec 3 — CLI Provider).
Initially disabled by default; opt-in via config.

### Phase F: Wire into Search Functions

Replace the current `search_history()` / `search_genome()` entry points to use
`run_pipeline()` when `PipelineConfig.enabled = true`. Existing callers unchanged.

---

## 5. Backward Compatibility

Existing `RetrievalMode` variants map to pipeline stage subsets:

```
  ┌──────────────────┬──────────────────────────────────────┐
  │ Mode             │ Active Stages                        │
  ├──────────────────┼──────────────────────────────────────┤
  │ Keyword          │ 2 (keyword) → 5 (min) → 9 (limit)   │
  │ Semantic         │ 3 (vector) → 5 (min) → 9 (limit)    │
  │ Hybrid (NEW)     │ 1-9 (full pipeline)                  │
  └──────────────────┴──────────────────────────────────────┘
```

When `PipelineConfig.enabled = false` (default initially), the existing code path in
`query.rs` runs unchanged. This allows incremental migration with zero risk to existing
users.

---

## 6. Config Integration

New `retrieval_pipeline` key in Impulse's `config.json`:

```json
{
  "retrieval_pipeline": {
    "enabled": false,
    "merge_rrf": { "enabled": true, "k": 60 },
    "temporal_decay": { "enabled": true, "lambda": 0.01 },
    "mmr": { "enabled": true, "lambda": 0.7, "target_count": 20 },
    "llm_rerank": { "enabled": false, "max_candidates": 20 }
  }
}
```

When `retrieval_pipeline.enabled` is `false`, the system uses the existing
`RetrievalMode`-based code path. When `true`, it routes through `pipeline.rs`.

---

## 7. Testing Strategy

| Test | Type | Description |
|------|------|-------------|
| `test_rrf_merge_basic` | Unit | Two 3-item lists, verify fused order |
| `test_rrf_k60_dampening` | Unit | Verify rank 1 vs 2 score difference is small |
| `test_rrf_disjoint_lists` | Unit | Lists with no overlap merge correctly |
| `test_temporal_decay_1day` | Unit | 1-day age → ~1% penalty (lambda=0.01) |
| `test_temporal_decay_90day` | Unit | 90-day age → ~59% penalty |
| `test_mmr_diversity` | Unit | Known embeddings, verify diverse selection |
| `test_mmr_lambda_1` | Unit | lambda=1.0 → pure relevance (no diversity) |
| `test_pipeline_keyword_compat` | Integration | Keyword mode → stages 2,5,9 only |
| `test_pipeline_semantic_compat` | Integration | Semantic mode → stages 3,5,9 only |
| `test_pipeline_hybrid_full` | Integration | Hybrid mode → all 9 stages |
| `test_pipeline_golden_file` | Regression | Known corpus → expected result order |
| `bench_pipeline_1000_docs` | Benchmark | Full pipeline < 200ms |

---

## 9. Cross-References

- **Companion (cross-cutting):** `~/.ai-memory/docs/spec-semantic-retrieval-pipeline.md` (cross-platform companion spec, not checked into this repo)
- **Spec 3 — CLI Provider:** [`spec-cli-provider-extension.md`](./spec-cli-provider-extension.md) — LLM rerank stage needs `LlmProvider`
- **Spec 4 — Agent Patterns:** [`spec-nullclaw-agent-patterns.md`](./spec-nullclaw-agent-patterns.md) — §3 BackendDescriptor for capability queries
- **Phase 3 Research:** [`PHASE3_SQLITE_VEC_RESEARCH.md`](./PHASE3_SQLITE_VEC_RESEARCH.md) — existing sqlite-vec roadmap
- **Impulse source:**
  - `src/retrieval/types.rs:5` — `RetrievalMode` enum
  - `src/retrieval/types.rs:28` — `SearchBackend` enum
  - `src/retrieval/types.rs:75` — `SearchResult` struct
  - `src/retrieval/types.rs:84` — `SearchResponse` struct
  - `src/retrieval/query.rs:16` — `cosine_similarity()` function
  - `src/retrieval/store.rs` — `RetrievalStore`, FTS5 + vec0 tables
  - `src/retrieval/embedding.rs` — `embed_texts()` subprocess
- **NullClaw source:**
  - `src/memory/retrieval/engine.zig` — 9-stage pipeline, RetrievalStage enum
  - `src/memory/vector/math.zig` — cosine, RRF, hybrid merge
