//! Dynamic tooling framework for Impulse
//!
//! Provides a trait-based extensible tool system that both Impulse CLI
//! and agentic harnesses (Claude Code, OpenCode) can invoke.
//!
//! ## Architecture
//!
//! - `DynamicTool` trait: async, Send+Sync — any tool implements this
//! - `ToolRegistry`: central registration and dispatch
//! - `ToolContext`: capability-based security (deny-by-default)
//! - Feature-gated modules: `office-support` for XLSX/DOCX processing
//!
//! ## Usage
//!
//! ```rust,no_run
//! use impulse_rs::tooling::{ToolRegistry, ToolContext};
//!
//! let registry = ToolRegistry::with_defaults();
//! let tools = registry.list();
//! ```

pub(crate) mod env_scrub;
mod error;
mod executor;
mod external;
mod registry;
mod traits;

pub mod builtin;

#[cfg(feature = "office-support")]
pub mod document;

pub use error::ToolError;
pub use external::{
    validate_manifests_in_dir, CwdPolicy, ExternalToolOutputMode, ExternalToolSource,
    ExternalToolSpec, ManifestValidationIssue, ManifestValidationReport, ProcessTool,
};
pub use registry::ToolRegistry;
pub use traits::{
    Capability, DynamicTool, ExecutionOrigin, ManifestTool, ParamType, ToolCategory, ToolContext,
    ToolDescriptor, ToolParam, ToolResult, ToolSource,
};
