// List CLI tools - show installation status of all known tools

use super::init::check_tool_installed;
use super::{known_tools, CliTool};
use anyhow::Result;
use std::fmt;

/// Display format for CLI tools
impl fmt::Display for CliTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.installed {
            format!(
                "✓ installed ({})",
                self.version.as_deref().unwrap_or("unknown")
            )
        } else {
            "✗ not installed".to_string()
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
    let mut tools = known_tools();

    for tool in &mut tools {
        if let Ok((installed, version)) = check_tool_installed(tool) {
            tool.installed = installed;
            tool.version = version;
        }
    }

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
            let status = if tool.installed {
                "installed"
            } else {
                "not installed"
            };
            println!(
                "{:<20} {:<15} {}",
                tool.name,
                status,
                tool.version.as_deref().unwrap_or("-")
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

pub fn list_not_installed() -> Result<Vec<CliTool>> {
    let tools = super::init::check_all_tools();
    Ok(tools.into_iter().filter(|t| !t.installed).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_tools() {
        let result = list_tools(false);
        assert!(result.is_ok());
        let tools = result.unwrap();
        assert!(!tools.is_empty());
    }
}
