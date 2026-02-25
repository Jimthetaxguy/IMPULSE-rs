use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::state::Session;

#[derive(Debug, Clone, Copy)]
pub enum TargetTool {
    ClaudeCode,
    Codex,
    OpenCode,
}

impl TargetTool {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetTool::ClaudeCode => "claude-code",
            TargetTool::Codex => "codex",
            TargetTool::OpenCode => "opencode",
        }
    }
}

pub fn suggest_tool(task: &str) -> TargetTool {
    let t = task.to_ascii_lowercase();
    if t.contains("architecture")
        || t.contains("design")
        || t.contains("review")
        || t.contains("refactor")
    {
        return TargetTool::ClaudeCode;
    }
    if t.contains("openwork") || t.contains("opencode") || t.contains("plugin") || t.contains("mcp")
    {
        return TargetTool::OpenCode;
    }
    TargetTool::Codex
}

#[derive(Serialize)]
struct RoutingLogEntry {
    timestamp: String,
    target_tool: String,
    task: String,
    session_id: Option<String>,
}

fn context_dir(impulse_dir: &Path) -> PathBuf {
    impulse_dir.join("context")
}

pub fn ensure_context_dirs(impulse_dir: &Path) -> Result<PathBuf> {
    let root = context_dir(impulse_dir);
    fs::create_dir_all(root.join("session-history"))?;
    Ok(root)
}

pub fn write_handoff(
    impulse_dir: &Path,
    target_tool: &str,
    task: &str,
    notes: Option<&str>,
    session: Option<&Session>,
) -> Result<PathBuf> {
    let root = ensure_context_dirs(impulse_dir)?;
    let handoff_path = root.join(format!("handoff-{}.md", target_tool));

    let (session_id, session_name, files, tools) = if let Some(s) = session {
        (
            s.id.clone(),
            s.name.clone(),
            s.active_files.clone(),
            s.recent_tools.clone(),
        )
    } else {
        (
            "unknown".to_string(),
            "unknown".to_string(),
            Vec::new(),
            Vec::new(),
        )
    };

    let files_block = if files.is_empty() {
        "- (none tracked)".to_string()
    } else {
        files
            .into_iter()
            .map(|f| format!("- {}", f))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tools_block = if tools.is_empty() {
        "- (none tracked)".to_string()
    } else {
        tools
            .into_iter()
            .map(|t| format!("- {}", t))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let notes_block = notes.unwrap_or("None");

    let content = format!(
        "# Task Handoff\n\n\
## Metadata\n\
- Timestamp: {}\n\
- Target Tool: {}\n\
- Session ID: {}\n\
- Session Name: {}\n\n\
## Current Task\n\
{}\n\n\
## Files Involved\n\
{}\n\n\
## Recent Tools\n\
{}\n\n\
## Notes\n\
{}\n",
        Utc::now().to_rfc3339(),
        target_tool,
        session_id,
        session_name,
        task,
        files_block,
        tools_block,
        notes_block
    );

    fs::write(&handoff_path, content)?;

    let log_entry = RoutingLogEntry {
        timestamp: Utc::now().to_rfc3339(),
        target_tool: target_tool.to_string(),
        task: task.to_string(),
        session_id: session.map(|s| s.id.clone()),
    };
    let line = serde_json::to_string(&log_entry)?;
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("routing-log.jsonl"))?;
    use std::io::Write as _;
    writeln!(log, "{}", line)?;

    Ok(handoff_path)
}

pub fn sync_context(impulse_dir: &Path, session: Option<&Session>) -> Result<PathBuf> {
    let root = ensure_context_dirs(impulse_dir)?;
    let current_task_path = root.join("current-task.md");

    let content = if let Some(s) = session {
        format!(
            "# Current Task Context\n\n\
## Session\n\
- ID: {}\n\
- Name: {}\n\
- Status: {:?}\n\
- Last Activity: {}\n\n\
## Active Files\n\
{}\n\n\
## Recent Tools\n\
{}\n",
            s.id,
            s.name,
            s.status,
            s.last_activity.to_rfc3339(),
            if s.active_files.is_empty() {
                "- (none tracked)".to_string()
            } else {
                s.active_files
                    .iter()
                    .map(|f| format!("- {}", f))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            if s.recent_tools.is_empty() {
                "- (none tracked)".to_string()
            } else {
                s.recent_tools
                    .iter()
                    .map(|t| format!("- {}", t))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        )
    } else {
        format!(
            "# Current Task Context\n\nNo active session context available.\nGenerated: {}\n",
            Utc::now().to_rfc3339()
        )
    };

    fs::write(&current_task_path, content)?;
    Ok(current_task_path)
}

pub fn append_injected_context(path: &Path, block: &str) -> Result<()> {
    if block.trim().is_empty() {
        return Ok(());
    }
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str("## Auto-Injected Context\n\n");
    content.push_str(block);
    content.push('\n');
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Session;
    use tempfile::TempDir;

    #[test]
    fn test_suggest_tool() {
        assert_eq!(
            suggest_tool("architecture review for auth").as_str(),
            "claude-code"
        );
        assert_eq!(suggest_tool("build opencode plugin").as_str(), "opencode");
        assert_eq!(suggest_tool("fix failing test quickly").as_str(), "codex");
    }

    #[test]
    fn test_write_handoff_and_sync_context() {
        let temp = TempDir::new().unwrap();
        let mut session = Session::new("test-session".to_string(), None);
        session.add_file("src/main.rs");
        session.add_tool("cargo-test");

        let handoff = write_handoff(
            temp.path(),
            "codex",
            "Fix test failures",
            None,
            Some(&session),
        )
        .unwrap();
        assert!(handoff.exists());

        let current = sync_context(temp.path(), Some(&session)).unwrap();
        assert!(current.exists());

        append_injected_context(&current, "Injected summary block").unwrap();
        let content = std::fs::read_to_string(&current).unwrap();
        assert!(content.contains("Auto-Injected Context"));
    }

    #[test]
    fn test_suggest_tool_variations() {
        // Test various task patterns
        // ClaudeCode: architecture, design, review, refactor
        assert_eq!(suggest_tool("review the auth code").as_str(), "claude-code");
        assert_eq!(suggest_tool("refactor the API").as_str(), "claude-code");
        assert_eq!(
            suggest_tool("design the new system").as_str(),
            "claude-code"
        );

        // OpenCode: openwork, opencode, plugin, mcp
        assert_eq!(suggest_tool("create opencode plugin").as_str(), "opencode");

        // Codex: default for everything else (debug, create, deploy, etc)
        assert_eq!(suggest_tool("debug the login bug").as_str(), "codex");
        assert_eq!(suggest_tool("create a new API endpoint").as_str(), "codex");
        assert_eq!(suggest_tool("deploy to production").as_str(), "codex");
    }
}
