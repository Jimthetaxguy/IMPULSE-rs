// Initialize CLI tools - check installation status and install missing tools

use super::{known_tools, CliTool};
use anyhow::{Context, Result};
use std::process::Command;
use std::time::Duration;

/// How long a version check may take. It runs `<tool> --version`.
pub(crate) const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// How long an install or update may take: a global npm install can take
/// minutes, but one that never finishes must not hold the command forever.
pub(crate) const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Runs one of the compile-time tool commands under `timeout`.
pub(crate) fn run_tool_command(
    command: &str,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    // SAFETY: every command is sourced from compile-time known_tools() only.
    // See tools/mod.rs trust boundary documentation.
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    crate::process_util::run_with_timeout(cmd, timeout)
}

/// Check if a tool is installed by running its check command. A check that
/// doesn't finish in [`VERSION_CHECK_TIMEOUT`] is an error rather than "not
/// installed", which would have the tool installed again.
pub fn check_tool_installed(tool: &CliTool) -> Result<(bool, Option<String>)> {
    check_tool_installed_within(tool, VERSION_CHECK_TIMEOUT)
}

fn check_tool_installed_within(
    tool: &CliTool,
    timeout: Duration,
) -> Result<(bool, Option<String>)> {
    match run_tool_command(&tool.check_cmd, timeout) {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Ok((true, Some(version)))
        }
        Ok(_) => Ok((false, None)),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            Err(e).with_context(|| format!("`{}` did not finish", tool.check_cmd))
        }
        Err(_) => Ok((false, None)),
    }
}

/// Check installation status for all known tools. A tool whose check
/// failed is reported with the reason in `check_error`, not as missing.
pub fn check_all_tools() -> Vec<CliTool> {
    check_tools_within(known_tools(), VERSION_CHECK_TIMEOUT)
}

fn check_tools_within(mut tools: Vec<CliTool>, timeout: Duration) -> Vec<CliTool> {
    for tool in &mut tools {
        match check_tool_installed_within(tool, timeout) {
            Ok((installed, version)) => {
                tool.installed = installed;
                tool.version = version;
            }
            Err(e) => tool.check_error = Some(format!("{e:#}")),
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
        let (installed, version) = match check_tool_installed(&tool) {
            Ok(status) => status,
            Err(e) => {
                results.push((tool.id.clone(), format!("version check failed: {e:#}")));
                continue;
            }
        };

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
            let output = run_tool_command(&tool.install_cmd, INSTALL_TIMEOUT);

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

    fn fake_tool(check_cmd: &str, update_cmd: &str) -> CliTool {
        CliTool::new(
            "fake",
            "Fake",
            "true",
            update_cmd,
            check_cmd,
            "https://example.invalid",
        )
    }

    /// Review finding: version checks ran with no time limit, so one that
    /// hung held `tools init`, `update` and `check` forever.
    #[test]
    fn test_a_hung_version_check_is_an_error_not_a_missing_tool() {
        // A duration no other test uses, so the cleanup stops only this one
        // (macOS clock nanoseconds are whole microseconds, so use the pid).
        let duration = format!("30.{:010}", std::process::id());
        let tool = fake_tool(&format!("sleep {duration}"), "true");
        let start = std::time::Instant::now();
        let result = check_tool_installed_within(&tool, Duration::from_millis(300));
        let elapsed = start.elapsed();
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(format!("^sleep {}", duration.replace('.', "\\.")))
            .status();
        let err = result.expect_err("a hung check is not an answer");
        assert!(format!("{err:#}").contains("did not finish"), "{err:#}");
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    /// Review finding: `tools list` showed a check that timed out as "not
    /// installed".
    #[test]
    fn test_a_hung_check_leaves_the_status_unknown() {
        let duration = format!("31.{:010}", std::process::id());
        let tools = vec![
            fake_tool(&format!("sleep {duration}"), "true"),
            fake_tool("echo 2.0.0", "true"),
        ];
        let checked = check_tools_within(tools, Duration::from_millis(300));
        let _ = Command::new("pkill")
            .arg("-f")
            .arg(format!("^sleep {}", duration.replace('.', "\\.")))
            .status();
        assert_eq!(checked[0].status_label(), "unknown");
        assert!(checked[0]
            .check_error
            .as_deref()
            .is_some_and(|error| error.contains("did not finish")));
        assert_eq!(checked[1].status_label(), "installed");
        assert_eq!(checked[1].version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn test_cli_tool_round_trips_with_and_without_a_check_error() {
        let mut tool = fake_tool("echo 1", "true");
        let plain = serde_json::to_string(&tool).unwrap();
        assert!(!plain.contains("check_error"));
        let back: CliTool = serde_json::from_str(&plain).unwrap();
        assert_eq!(back.check_error, None);
        assert_eq!(back.id, tool.id);
        tool.check_error = Some("timed out".to_string());
        let back: CliTool = serde_json::from_str(&serde_json::to_string(&tool).unwrap()).unwrap();
        assert_eq!(back.check_error.as_deref(), Some("timed out"));
        assert_eq!(back.check_cmd, tool.check_cmd);
    }

    #[test]
    fn test_a_finished_version_check_reports_the_version() {
        let tool = fake_tool("echo 2.0.0", "true");
        assert_eq!(
            check_tool_installed(&tool).unwrap(),
            (true, Some("2.0.0".to_string()))
        );
        let missing = fake_tool("exit 3", "true");
        assert_eq!(check_tool_installed(&missing).unwrap(), (false, None));
    }

    /// Review finding: OpenCode was installed and updated with `pip`, from
    /// the PyPI name `opencode`, which is not OpenCode's package.
    #[test]
    fn test_opencode_installs_from_its_npm_package() {
        let opencode = known_tools()
            .into_iter()
            .find(|tool| tool.id == "opencode")
            .unwrap();
        assert_eq!(opencode.install_cmd, "npm install -g opencode-ai");
        assert_eq!(opencode.update_cmd, "npm update -g opencode-ai");
        assert!(known_tools()
            .iter()
            .all(|tool| !tool.install_cmd.contains("pip") && !tool.update_cmd.contains("pip")));
    }
}
