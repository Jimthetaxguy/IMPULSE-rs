//! Document processing tools — DynamicTool wrappers for the office/ module
//!
//! These tools expose the existing office::excel and office::word parsing
//! capabilities through the DynamicTool trait, making them available via
//! CLI `tooling-run` and daemon IPC. Parsing runs on the blocking pool under
//! the bounds in `office::bounded`.
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

#[cfg(test)]
mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::tooling::error::ToolError;
    use crate::tooling::traits::{DynamicTool, ToolContext, ToolResult};

    /// Runs `tool` beside a task that counts its own turns, and returns the
    /// result with the number of turns that task got before the tool
    /// finished. On a current-thread runtime a tool that parses inline
    /// never yields, so the count stays at zero; a tool that parses on the
    /// blocking pool yields while it waits.
    pub(super) async fn turns_while_executing(
        tool: &dyn DynamicTool,
        params: serde_json::Value,
    ) -> (Result<ToolResult, ToolError>, usize) {
        let turns = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&turns);
        let ticker = tokio::spawn(async move {
            loop {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
            }
        });
        let result = tool.execute(params, &ToolContext::default()).await;
        let seen = turns.load(Ordering::SeqCst);
        ticker.abort();
        (result, seen)
    }
}
