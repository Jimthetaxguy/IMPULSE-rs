//! File write tool — atomic file write for agents (TUI_SPEC.md T7).
//!
//! `ion` is a coding agent (TUI_SPEC.md section 2.3's "Scope clarification"),
//! not a read-only verify console, so its REPL tool registry needs a
//! write-capable file tool alongside `file_read`. Follows CLAUDE.md
//! Principle #2 (Atomic Writes): every write goes through a temp file in the
//! same directory as the target (so the final `rename` is same-filesystem
//! and therefore atomic), with a PID+timestamp-qualified temp name to avoid
//! collisions between concurrent writers.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Write (or overwrite) a file's full contents atomically.
///
/// Optionally creates missing parent directories. Requires
/// `Capability::FileSystemWrite`, and is subject to `ToolContext`'s
/// `allowed_write_roots` sandbox like every other filesystem tool.
pub struct FileWriteTool;

#[async_trait]
impl DynamicTool for FileWriteTool {
    fn id(&self) -> &str {
        "file_write"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "file_write".into(),
            name: "File Write".into(),
            description: "Atomically write (create or overwrite) a file's full contents".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Path to the file to write".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "content".into(),
                    description: "Full file content to write".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "create_dirs".into(),
                    description: "Create missing parent directories (default: true)".into(),
                    param_type: ParamType::Bool,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => {}
            _ => return Err(ToolError::InvalidParams("missing 'path'".into())),
        }
        if params.get("content").and_then(|v| v.as_str()).is_none() {
            return Err(ToolError::InvalidParams("missing 'content' string".into()));
        }
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'path'".into()))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'content'".into()))?;
        let create_dirs = params
            .get("create_dirs")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let path = ctx.resolve_path(path_str);

        if !ctx.is_path_allowed(&path, true) {
            return Err(ToolError::PathNotAllowed(path.display().to_string()));
        }

        let parent = path.parent().ok_or_else(|| {
            ToolError::ExecutionFailed(format!("path has no parent directory: {path_str}"))
        })?;

        if create_dirs {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to create parent directories: {e}"))
            })?;
        } else if !parent.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "parent directory does not exist: {}",
                parent.display()
            )));
        }

        // Atomic write: temp file (PID + timestamp qualified, same
        // directory as the target so rename() is same-filesystem) + rename.
        let temp_name = format!(
            ".{}.tmp.{}.{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file_write"),
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let temp_path = parent.join(temp_name);

        std::fs::write(&temp_path, content)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write temp file: {e}")))?;

        if let Err(e) = std::fs::rename(&temp_path, &path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(ToolError::ExecutionFailed(format!(
                "failed to rename temp file into place: {e}"
            )));
        }

        Ok(ToolResult::json(serde_json::json!({
            "path": path.display().to_string(),
            "bytes_written": content.len(),
        })))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemWrite]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = FileWriteTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "file_write");
        assert_eq!(desc.params.len(), 3);
    }

    #[test]
    fn test_validate_missing_content() {
        let tool = FileWriteTool;
        assert!(tool
            .validate_params(&serde_json::json!({"path": "x.txt"}))
            .is_err());
    }

    #[test]
    fn test_validate_ok() {
        let tool = FileWriteTool;
        assert!(tool
            .validate_params(&serde_json::json!({"path": "x.txt", "content": "hi"}))
            .is_ok());
    }

    #[tokio::test]
    async fn test_execute_writes_file_and_creates_parent_dirs() {
        let tool = FileWriteTool;
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("out.txt");
        let ctx = ToolContext {
            allowed_write_roots: vec![dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };

        let result = tool
            .execute(
                serde_json::json!({
                    "path": target.display().to_string(),
                    "content": "hello world",
                }),
                &ctx,
            )
            .await
            .expect("write should succeed");

        assert_eq!(result.output["bytes_written"], 11);
        assert_eq!(
            std::fs::read_to_string(&target).expect("read back"),
            "hello world"
        );
    }

    #[tokio::test]
    async fn test_execute_overwrites_existing_file() {
        let tool = FileWriteTool;
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.txt");
        std::fs::write(&target, "old content").expect("seed file");
        let ctx = ToolContext {
            allowed_write_roots: vec![dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };

        tool.execute(
            serde_json::json!({"path": target.display().to_string(), "content": "new"}),
            &ctx,
        )
        .await
        .expect("overwrite should succeed");

        assert_eq!(std::fs::read_to_string(&target).expect("read back"), "new");
    }

    #[tokio::test]
    async fn test_execute_rejects_path_outside_write_roots() {
        let tool = FileWriteTool;
        let allowed_dir = tempfile::tempdir().expect("tempdir");
        let outside_dir = tempfile::tempdir().expect("tempdir");
        let target = outside_dir.path().join("escape.txt");
        let ctx = ToolContext {
            allowed_write_roots: vec![allowed_dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };

        let result = tool
            .execute(
                serde_json::json!({"path": target.display().to_string(), "content": "x"}),
                &ctx,
            )
            .await;

        assert!(matches!(result, Err(ToolError::PathNotAllowed(_))));
        assert!(!target.exists());
    }
}
