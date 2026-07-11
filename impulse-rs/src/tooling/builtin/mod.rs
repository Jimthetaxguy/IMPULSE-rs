//! Built-in tools — thin wrappers around existing tools/ implementations
//!
//! These wrap the existing functionality in tools::python, tools::system,
//! tools::health, tools::benchmark, and build_hygiene with the DynamicTool
//! trait interface. Each tool is a zero-cost wrapper — no code duplication.

mod bash_exec;
mod benchmarker;
mod build_health;
mod calculator;
mod config_get;
mod document_extract;
mod file_read;
mod file_write;
mod genome_read;
mod health_check;
mod memory_search;
mod python_exec;
mod session_query;
mod steward_status;
mod system_info;

pub use bash_exec::BashExecTool;
pub use benchmarker::BenchmarkerTool;
pub use build_health::{
    BuildHealthTool, CleanAllTool, SccacheSetupTool, SccacheStatusTool, SweepTool,
    ToolAvailabilityTool, WipeTool,
};
pub use calculator::CalculatorTool;
pub use config_get::ConfigGetTool;
pub use document_extract::DocumentExtractTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use genome_read::GenomeReadTool;
pub use health_check::HealthCheckTool;
pub use memory_search::MemorySearchTool;
pub use python_exec::PythonExecTool;
pub use session_query::SessionQueryTool;
pub use steward_status::StewardStatusTool;
pub use system_info::SystemInfoTool;

use super::error::ToolError;
use super::registry::ToolRegistry;

/// Register all built-in tools into a registry
pub fn register_all(registry: &mut ToolRegistry) -> Result<(), ToolError> {
    registry.register(Box::new(BashExecTool))?;
    registry.register(Box::new(BenchmarkerTool))?;
    registry.register(Box::new(BuildHealthTool))?;
    registry.register(Box::new(CalculatorTool))?;
    registry.register(Box::new(CleanAllTool))?;
    registry.register(Box::new(ConfigGetTool))?;
    registry.register(Box::new(DocumentExtractTool))?;
    registry.register(Box::new(FileReadTool))?;
    registry.register(Box::new(FileWriteTool))?;
    registry.register(Box::new(GenomeReadTool))?;
    registry.register(Box::new(HealthCheckTool))?;
    registry.register(Box::new(MemorySearchTool))?;
    registry.register(Box::new(PythonExecTool))?;
    registry.register(Box::new(SccacheSetupTool))?;
    registry.register(Box::new(SccacheStatusTool))?;
    registry.register(Box::new(SessionQueryTool))?;
    registry.register(Box::new(StewardStatusTool))?;
    registry.register(Box::new(SweepTool))?;
    registry.register(Box::new(SystemInfoTool))?;
    registry.register(Box::new(ToolAvailabilityTool))?;
    registry.register(Box::new(WipeTool))?;
    Ok(())
}
