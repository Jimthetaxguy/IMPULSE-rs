//! Semantic diff integration using the `sem` CLI tool.
//!
//! Wraps the open-source `sem` tool (<https://github.com/Ataraxy-Labs/sem>)
//! to provide entity-level code change understanding instead of line-level diffs.
//!
//! `sem` uses Tree-sitter to parse code into entities (functions, structs, classes)
//! and computes structural hashes to detect meaningful changes.  Impulse calls
//! `sem` as a subprocess so there is no build-time tree-sitter dependency.
//!
//! ## Supported operations
//!
//! - **diff** — semantic diff between two Git refs (or staged/working tree)
//! - **blame** — entity-level git blame (who last changed each function/class)
//! - **impact** — blast-radius analysis for a given entity
//!
//! ## Data flow
//!
//! ```text
//! session-end hook
//!   └─ capture_semantic_diff(session_start_ref, HEAD)
//!        └─ runs `sem diff --format json <base>..<head>`
//!             └─ parses SemanticDiffReport
//!                  └─ stored in .impulse/semantic_diffs/<session_id>.json
//! ```

mod runner;
mod types;

pub use runner::{
    capture_semantic_diff, run_semantic_blame, run_semantic_diff, run_semantic_impact,
    sem_available,
};
pub use types::{
    ChangeKind, EntityChange, EntityInfo, ImpactResult, SemanticBlameEntry, SemanticDiffReport,
    SemanticDiffSummary,
};
