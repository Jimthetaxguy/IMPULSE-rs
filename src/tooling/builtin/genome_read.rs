//! Genome reader tool — read permanent project decisions from GENOME.md
//!
//! The GENOME file contains durable project decisions, preferences, and
//! patterns that persist across all sessions. This tool lets agents
//! read and search the genome without parsing it manually.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Read the project GENOME — permanent decisions and preferences.
///
/// GENOME.md is Impulse's long-term memory: architectural decisions,
/// user preferences, tool configurations, and learned patterns that
/// should survive across all sessions.
pub struct GenomeReadTool;

#[async_trait]
impl DynamicTool for GenomeReadTool {
    fn id(&self) -> &str {
        "genome_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "genome_read".into(),
            name: "Genome Read".into(),
            description: "Read permanent project decisions and preferences from GENOME.md".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "section".into(),
                    description:
                        "Optional section filter (e.g., 'decisions', 'preferences', 'patterns')"
                            .into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "impulse_dir".into(),
                    description: "Path to .impulse directory (default: .impulse)".into(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                },
            ],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(()) // All params optional
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let impulse_dir = params
            .get("impulse_dir")
            .and_then(|v| v.as_str())
            .unwrap_or(".impulse");
        let section_filter = params.get("section").and_then(|v| v.as_str());

        let genome_path = std::path::PathBuf::from(impulse_dir).join("GENOME.md");

        if !genome_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "exists": false,
                "content": null,
                "message": "No GENOME.md found — project has no permanent decisions recorded yet"
            })));
        }

        let content = std::fs::read_to_string(&genome_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read GENOME.md: {}", e)))?;

        // If section filter specified, extract just that section
        if let Some(section) = section_filter {
            let section_lower = section.to_lowercase();
            let mut in_section = false;
            let mut section_lines = Vec::new();
            let mut section_found = false;

            for line in content.lines() {
                if line.starts_with("## ") || line.starts_with("# ") {
                    if in_section {
                        break; // End of target section
                    }
                    if line.to_lowercase().contains(&section_lower) {
                        in_section = true;
                        section_found = true;
                        section_lines.push(line.to_string());
                        continue;
                    }
                }
                if in_section {
                    section_lines.push(line.to_string());
                }
            }

            if section_found {
                Ok(ToolResult::json(serde_json::json!({
                    "exists": true,
                    "section": section,
                    "content": section_lines.join("\n"),
                    "total_length": content.len(),
                })))
            } else {
                // List available sections to help the agent
                let sections: Vec<&str> = content
                    .lines()
                    .filter(|l| l.starts_with("## ") || l.starts_with("# "))
                    .collect();
                Ok(ToolResult::json(serde_json::json!({
                    "exists": true,
                    "section": section,
                    "content": null,
                    "message": format!("Section '{}' not found", section),
                    "available_sections": sections,
                })))
            }
        } else {
            // Return full genome with basic stats
            let line_count = content.lines().count();
            let sections: Vec<&str> = content
                .lines()
                .filter(|l| l.starts_with("## ") || l.starts_with("# "))
                .collect();

            Ok(ToolResult::json(serde_json::json!({
                "exists": true,
                "content": content,
                "lines": line_count,
                "sections": sections,
                "size_bytes": content.len(),
            })))
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = GenomeReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "genome_read");
        assert_eq!(desc.category, ToolCategory::Analysis);
    }

    #[tokio::test]
    async fn test_execute_no_genome() {
        let tool = GenomeReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"impulse_dir": "/tmp/nonexistent_impulse_xyz"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(result.output["exists"], false);
    }
}
