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

    let current = version.unwrap_or_default();
    if dry_run {
        return Ok((
            true,
            format!("would update: {} (current: {})", tool.update_cmd, current),
        ));
    }

    println!("Updating {} (current: {})...", tool.name, current);

    let output = run_tool_command(&tool.update_cmd, INSTALL_TIMEOUT);

    match output {
        Ok(out) => {
            if out.status.success() {
                // The updater's output is its own log, not a version; ask
                // the tool again. An updater that found nothing newer still
                // succeeds, so say when the version didn't move.
                let message = match check_tool_installed(tool) {
                    Ok((true, Some(new))) if new == current => format!("unchanged at {new}"),
                    Ok((true, Some(new))) => format!("updated to {new}"),
                    _ => "updated; the new version could not be read".to_string(),
                };
                println!("  ✓ {}: {}", tool.name, message);
                Ok((true, message))
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
        let dir = tempfile::TempDir::new().unwrap();
        let version_file = dir.path().join("version");
        std::fs::write(&version_file, "1.0.0").unwrap();
        let tool = CliTool::new(
            "fake",
            "Fake",
            "true",
            &format!(
                "printf 2.0.0 > '{}' && echo 'changed 1 package in 2s'",
                version_file.display()
            ),
            &format!("cat '{}'", version_file.display()),
            "https://example.invalid",
        );
        let (updated, message) = update_tool(&tool, false).unwrap();
        assert!(updated);
        assert_eq!(message, "updated to 2.0.0");
    }

    /// Review finding: an update that found nothing newer said "updated to"
    /// the version the tool already had.
    #[test]
    fn test_an_update_that_changes_nothing_says_so() {
        let tool = CliTool::new(
            "fake",
            "Fake",
            "true",
            "echo 'up to date'",
            "echo 2.0.0",
            "https://example.invalid",
        );
        let (updated, message) = update_tool(&tool, false).unwrap();
        assert!(updated);
        assert_eq!(message, "unchanged at 2.0.0");
    }
}
