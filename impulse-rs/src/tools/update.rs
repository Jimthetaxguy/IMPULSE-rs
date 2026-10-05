// Update CLI tools - update installed tools to latest versions

use super::init::{check_tool_installed, run_tool_command, INSTALL_TIMEOUT};
use super::{known_tools, CliTool};
use anyhow::Result;

/// Update a specific tool to the latest version
pub fn update_tool(tool: &CliTool, dry_run: bool) -> Result<(bool, String)> {
    let (installed, version) = match check_tool_installed(tool) {
        Ok(status) => status,
        Err(e) => return Ok((false, format!("version check failed: {e:#}"))),
    };

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

    let output = run_tool_command(&tool.update_cmd, INSTALL_TIMEOUT);

    match output {
        Ok(out) => {
            if out.status.success() {
                // The updater's output is its own log, not a version; ask
                // the tool again.
                let new_version = match check_tool_installed(tool) {
                    Ok((true, Some(version))) => version,
                    _ => "an unknown version".to_string(),
                };
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

/// The installed tools and their versions (or why the version check
/// failed). Whether a newer version exists is not checked: that needs each
/// package manager's registry, and every tool used to be reported "up to
/// date" without it.
pub fn check_updates() -> Result<Vec<(String, String)>> {
    let tools = known_tools();
    let mut results = Vec::new();

    for tool in tools {
        match check_tool_installed(&tool) {
            Ok((true, version)) => {
                results.push((tool.id, version.unwrap_or_else(|| "unknown".to_string())));
            }
            Ok((false, _)) => {}
            Err(e) => results.push((tool.id, format!("version check failed: {e:#}"))),
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

    /// Review finding: `tools update` reported the updater's own output as
    /// the new version ("updated to Successfully installed ...").
    #[test]
    fn test_an_update_reports_the_version_the_tool_now_gives() {
        let tool = CliTool::new(
            "fake",
            "Fake",
            "true",
            "echo 'changed 1 package in 2s'",
            "echo 2.0.0",
            "https://example.invalid",
        );
        let (updated, message) = update_tool(&tool, false).unwrap();
        assert!(updated);
        assert_eq!(message, "updated to 2.0.0");
    }
}
