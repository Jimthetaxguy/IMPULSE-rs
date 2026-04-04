//! Context injection — surfaces retrieved knowledge to coding agents.
//!
//! Runs a retrieval query, ranks and deduplicates snippets, then stages an
//! injection bundle for the target surface (CLAUDE.md, AGENTS.md, etc.).
//! Supports review mode (show what would be injected) and auto mode (write
//! directly). The staging pipeline handles dedup hashing and audit logging.

pub mod engine;
pub mod staging;
pub mod types;

pub use engine::run_injection;
pub use types::{InjectionMode, InjectionRunResult, InjectionScope, InjectionSurface};
