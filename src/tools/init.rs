// Initialize CLI tools - check installation status and install missing tools

use super::{known_tools, CliTool};
use anyhow::Result;
use std::process::Command;

/// Check if a tool is installed by running its check command
pub fn check_tool_installed(tool: &CliTool) -> Result<(bool, Option<String>)> {
    // SAFETY: check_cmd is sourced from compile-time known_tools() only.
    // See tools/mod.rs trust boundary documentation.
    let output = Command::new("sh").arg("-c").arg(&tool.check_cmd).output();

    match output {
        Ok(out) => {
            if out.status.success() {
                let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                Ok((true, Some(version)))
            } else {
                Ok((false, None))
            }
        }
        Err(_) => Ok((false, None)),
    }
}

/// Check installation status for all known tools
pub fn check_all_tools() -> Vec<CliTool> {
    let mut tools = known_tools();
    for tool in &mut tools {
        if let Ok((installed, version)) = check_tool_installed(tool) {
            tool.installed = installed;
            tool.version = version;
        }
    }
    tools
}

/// Initialize missing tools (install those not yet installed)
pub fn init_tools(tool_ids: Option<Vec<String>>, dry_run: bool) -> Result<Vec<(String, String)>> {
    let all_tools = known_tools();
    let tools_to_init = if let Some(ids) = tool_ids {
        all_tools
            .into_iter()
            .filter(|t| ids.contains(&t.id))
            .collect()
    } else {
        all_tools
    };

    let mut results = Vec::new();

    for tool in tools_to_init {
        let (installed, version) = check_tool_installed(&tool)?;

        if installed {
            results.push((
                tool.id.clone(),
                format!("already installed: {}", version.unwrap_or_default()),
            ));
            continue;
        }

        if dry_run {
            results.push((
                tool.id.clone(),
                format!("would install: {}", tool.install_cmd),
            ));
        } else {
            println!("Installing {}...", tool.name);
            // SAFETY: install_cmd is sourced from compile-time known_tools() only.
            let output = Command::new("sh").arg("-c").arg(&tool.install_cmd).output();

            match output {
                Ok(out) => {
                    if out.status.success() {
                        results.push((tool.id.clone(), "installed successfully".to_string()));
                        println!("  ✓ {} installed", tool.name);
                    } else {
                        let err = String::from_utf8_lossy(&out.stderr);
                        results.push((tool.id.clone(), format!("install failed: {}", err)));
                        eprintln!("  ✗ {} failed: {}", tool.name, err);
                    }
                }
                Err(e) => {
                    results.push((tool.id.clone(), format!("error: {}", e)));
                    eprintln!("  ✗ {} error: {}", tool.name, e);
                }
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_tools() {
        let tools = known_tools();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.id == "claude-code"));
        assert!(tools.iter().any(|t| t.id == "opencode"));
    }

    #[test]
    fn test_check_tool_installed() {
        // This will vary by environment, but shouldn't error
        let tool = CliTool::new(
            "test",
            "Test",
            "echo test",
            "echo test",
            "nonexistentcmd",
            "http://test.com",
        );
        let result = check_tool_installed(&tool);
        assert!(result.is_ok());
    }

    #[test]
    fn test_known_tools_commands_no_shell_metacharacters() {
        let dangerous = [';', '|', '&', '$', '`', '>', '<', '#'];
        for tool in known_tools() {
            for (cmd_name, cmd) in [
                ("check_cmd", &tool.check_cmd),
                ("install_cmd", &tool.install_cmd),
                ("update_cmd", &tool.update_cmd),
            ] {
                for ch in &dangerous {
                    assert!(
                        !cmd.contains(*ch),
                        "Tool '{}' {} contains dangerous char '{}': {}",
                        tool.id,
                        cmd_name,
                        ch,
                        cmd
                    );
                }
            }
        }
    }
}
