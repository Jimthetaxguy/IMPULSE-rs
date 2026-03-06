// Update CLI tools - update installed tools to latest versions

use super::init::check_tool_installed;
use super::{known_tools, CliTool};
use anyhow::Result;
use std::process::Command;

/// Update a specific tool to the latest version
pub fn update_tool(tool: &CliTool, dry_run: bool) -> Result<(bool, String)> {
    let (installed, version) = check_tool_installed(tool)?;

    if !installed {
        return Ok((false, "not installed".to_string()));
    }

    if dry_run {
        return Ok((
            true,
            format!(
                "would update: {} (current: {})",
                tool.update_cmd,
                version.unwrap_or_default()
            ),
        ));
    }

    println!(
        "Updating {} (current: {})...",
        tool.name,
        version.unwrap_or_default()
    );

    // SAFETY: update_cmd is sourced from compile-time known_tools() only.
    let output = Command::new("sh").arg("-c").arg(&tool.update_cmd).output();

    match output {
        Ok(out) => {
            if out.status.success() {
                let new_version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                println!("  ✓ {} updated to {}", tool.name, new_version);
                Ok((true, format!("updated to {}", new_version)))
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                eprintln!("  ✗ {} update failed: {}", tool.name, err);
                Ok((false, format!("update failed: {}", err)))
            }
        }
        Err(e) => {
            eprintln!("  ✗ {} error: {}", tool.name, e);
            Ok((false, format!("error: {}", e)))
        }
    }
}

/// Update multiple tools
pub fn update_tools(
    tool_ids: Option<Vec<String>>,
    dry_run: bool,
) -> Result<Vec<(String, bool, String)>> {
    let all_tools = known_tools();
    let tools_to_update = if let Some(ids) = tool_ids {
        all_tools
            .into_iter()
            .filter(|t| ids.contains(&t.id))
            .collect()
    } else {
        all_tools
    };

    let mut results = Vec::new();

    for tool in tools_to_update {
        let (success, message) = update_tool(&tool, dry_run)?;
        results.push((tool.id, success, message));
    }

    Ok(results)
}

/// Get update status for all installed tools
pub fn check_updates() -> Result<Vec<(String, String, bool)>> {
    let tools = known_tools();
    let mut results = Vec::new();

    for tool in tools {
        let (installed, version) = check_tool_installed(&tool)?;

        if installed {
            // For now, we just report installed status
            // In a more advanced version, we could check for updates
            results.push((
                tool.id,
                version.unwrap_or_else(|| "unknown".to_string()),
                true, // Consider up-to-date for now
            ));
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_updates_empty() {
        // Should not error even with no tools
        let result = check_updates();
        assert!(result.is_ok());
    }
}
