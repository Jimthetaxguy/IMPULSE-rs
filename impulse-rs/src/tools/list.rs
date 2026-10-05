// List CLI tools - show installation status of all known tools

use super::CliTool;
use anyhow::Result;
use std::fmt;

/// Display format for CLI tools
impl fmt::Display for CliTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match (&self.check_error, self.installed) {
            (Some(error), _) => format!("? unknown: {error}"),
            (None, true) => format!(
                "✓ installed ({})",
                self.version.as_deref().unwrap_or("unknown")
            ),
            (None, false) => "✗ not installed".to_string(),
        };

        write!(
            f,
            "{} ({})\n  Status: {}\n  Install: {}\n  Update: {}\n  Docs: {}",
            self.name, self.id, status, self.install_cmd, self.update_cmd, self.docs_url
        )
    }
}

/// List all known tools with their installation status
pub fn list_tools(verbose: bool) -> Result<Vec<CliTool>> {
    let tools = super::init::check_all_tools();

    if verbose {
        for tool in &tools {
            println!("{}", tool);
            println!();
        }
    } else {
        // Brief format
        println!("{:<20} {:<15} Version", "Tool", "Status",);
        println!("{:-<20} {:-<-15} ", "", "");

        for tool in &tools {
            println!(
                "{:<20} {:<15} {}",
                tool.name,
                tool.status_label(),
                tool.check_error
                    .as_deref()
                    .or(tool.version.as_deref())
                    .unwrap_or("-")
            );
        }
    }

    Ok(tools)
}

/// Get tools filtered by installation status
pub fn list_installed() -> Result<Vec<CliTool>> {
    let tools = super::init::check_all_tools();
    Ok(tools.into_iter().filter(|t| t.installed).collect())
}

/// Tools known to be missing; a tool whose check failed isn't one.
pub fn list_not_installed() -> Result<Vec<CliTool>> {
    let tools = super::init::check_all_tools();
    Ok(tools
        .into_iter()
        .filter(|t| !t.installed && t.check_error.is_none())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_shows_a_failed_check_as_unknown() {
        let mut tool = CliTool::new("fake", "Fake", "true", "true", "true", "https://x.invalid");
        assert!(tool.to_string().contains("✗ not installed"));
        tool.check_error = Some("`fake --version` did not finish".to_string());
        let shown = tool.to_string();
        assert!(
            shown.contains("? unknown: `fake --version` did not finish"),
            "{shown}"
        );
        assert!(!shown.contains("not installed"), "{shown}");
    }

    #[test]
    fn test_list_tools() {
        let result = list_tools(false);
        assert!(result.is_ok());
        let tools = result.unwrap();
        assert!(!tools.is_empty());
    }
}
