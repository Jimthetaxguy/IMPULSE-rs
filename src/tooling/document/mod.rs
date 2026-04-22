//! Document processing tools — DynamicTool wrappers for the office/ module
//!
//! These tools expose the existing office::excel and office::word parsing
//! capabilities through the DynamicTool trait, making them available via
//! CLI `tooling-run` and daemon IPC.
//!
//! Feature-gated behind `office-support`.

mod document_parse;
mod excel_read;
mod word_read;

pub use document_parse::DocumentParseTool;
pub use excel_read::ExcelReadTool;
pub use word_read::WordReadTool;

use super::error::ToolError;
use super::registry::ToolRegistry;
use super::traits::ToolSource;

/// Register all document tools into a registry
pub fn register_all(registry: &mut ToolRegistry) -> Result<(), ToolError> {
    registry.register_with_source(Box::new(DocumentParseTool), ToolSource::Document)?;
    registry.register_with_source(Box::new(ExcelReadTool), ToolSource::Document)?;
    registry.register_with_source(Box::new(WordReadTool), ToolSource::Document)?;
    Ok(())
}
