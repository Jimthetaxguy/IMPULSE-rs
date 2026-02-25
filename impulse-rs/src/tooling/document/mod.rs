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

use super::registry::ToolRegistry;

/// Register all document tools into a registry
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(Box::new(DocumentParseTool)).unwrap();
    registry.register(Box::new(ExcelReadTool)).unwrap();
    registry.register(Box::new(WordReadTool)).unwrap();
}
