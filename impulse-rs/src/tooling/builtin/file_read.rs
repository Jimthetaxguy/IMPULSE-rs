//! File read tool — safe file reading with size limits
//!
//! Provides controlled file reading for agents with size limits
//! and line-range selection. Prevents accidental large file reads
//! that could overwhelm context windows.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

const MAX_FILE_BYTES: usize = 1_000_000; // 1MB hard limit

/// Read a file with size limits and optional line range.
///
/// Designed for agents that need to read source code, config files,
/// or data files without risking context window overflow.
pub struct FileReadTool;

#[async_trait]
impl DynamicTool for FileReadTool {
    fn id(&self) -> &str {
        "file_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "file_read".into(),
            name: "File Read".into(),
            description: "Read a file with size limits and optional line range".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![
                ToolParam {
                    name: "path".into(),
                    description: "Path to the file to read".into(),
                    param_type: ParamType::FilePath,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "start_line".into(),
                    description: "First line to read (1-based, default: 1)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(1)),
                },
                ToolParam {
                    name: "max_lines".into(),
                    description: "Maximum lines to return (default: 200)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(200)),
                },
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams("missing 'path'".into())),
        }
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
        // Clamp to usize::MAX rather than letting a huge model-supplied
        // start_line (e.g. u64::MAX) overflow the `start_line - 1 + max_lines`
        // arithmetic below -- overflow panics in debug builds and wraps to a
        // bogus small `end` in release, producing a slice-index panic either
        // way once `total_lines` is known.
        let start_line = params
            .get("start_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1)
            .min(usize::MAX as u64) as usize;
        let max_lines = params
            .get("max_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(usize::MAX as u64) as usize;

        let path = ctx.resolve_path(path_str);

        if !ctx.is_path_allowed(&path, false) {
            return Err(ToolError::PathNotAllowed(path.display().to_string()));
        }

        if !path.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "File not found: {}",
                path_str
            )));
        }

        // Check file size first
        let metadata = std::fs::metadata(&path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Cannot stat file: {}", e)))?;

        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Ok(ToolResult::json(serde_json::json!({
                "path": path_str,
                "error": "File too large",
                "size_bytes": metadata.len(),
                "max_bytes": MAX_FILE_BYTES,
                "hint": "Use start_line and max_lines to read a portion"
            })));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        let start = start_line.saturating_sub(1);
        let end = start.saturating_add(max_lines);
        let mut selected = String::new();
        let mut lines_returned = 0;
        let mut total_lines = 0;
        for (index, line) in content.lines().enumerate() {
            total_lines = index + 1;
            if index >= start && index < end {
                if lines_returned > 0 {
                    selected.push('\n');
                }
                selected.push_str(line);
                lines_returned += 1;
            }
        }
        let truncated = end < total_lines;

        Ok(ToolResult::json(serde_json::json!({
            "path": path_str,
            "content": selected,
            "start_line": start_line,
            "lines_returned": lines_returned,
            "total_lines": total_lines,
            "truncated": truncated,
            "size_bytes": metadata.len(),
        })))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_execute_page_matches_prior_line_semantics(
            characters in proptest::collection::vec(any::<char>(), 0..512),
            start_line in prop_oneof![0u64..32, Just(u64::MAX)],
            max_lines in prop_oneof![0u64..32, Just(u64::MAX)],
        ) {
            let content: String = characters.into_iter().collect();
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("page.txt");
            std::fs::write(&path, &content).unwrap();
            let ctx = ToolContext::with_all_capabilities();
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let result = runtime.block_on(FileReadTool.execute(
                serde_json::json!({"path": path, "start_line": start_line, "max_lines": max_lines}),
                &ctx,
            )).unwrap();
            let normalized_start = start_line.max(1).min(usize::MAX as u64) as usize;
            let normalized_max = max_lines.min(usize::MAX as u64) as usize;
            let all_lines: Vec<_> = content.lines().collect();
            let start = normalized_start.saturating_sub(1).min(all_lines.len());
            let end = start.saturating_add(normalized_max).min(all_lines.len());
            prop_assert_eq!(result.output, serde_json::json!({
                "path": path,
                "content": all_lines[start..end].join("\n"),
                "start_line": normalized_start,
                "lines_returned": end - start,
                "total_lines": all_lines.len(),
                "truncated": end < all_lines.len(),
                "size_bytes": content.len(),
            }));
        }
    }

    #[tokio::test]
    async fn test_execute_line_boundaries_preserve_content_and_counts() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("page.txt");
        let ctx = ToolContext::with_all_capabilities();
        for content in ["", "\n", "\n\n", "alpha\r\nβeta\n終\r", "alpha\n\n", "only"] {
            std::fs::write(&path, content).unwrap();
            for (start_line, max_lines) in [
                (1usize, 0usize),
                (1, 200),
                (2, 1),
                (4, 4),
                (usize::MAX, usize::MAX),
            ] {
                let result = FileReadTool.execute(
                    serde_json::json!({"path": path, "start_line": start_line, "max_lines": max_lines}),
                    &ctx,
                ).await.unwrap();
                let all_lines: Vec<_> = content.lines().collect();
                let start = start_line.saturating_sub(1).min(all_lines.len());
                let end = start.saturating_add(max_lines).min(all_lines.len());
                assert_eq!(result.output["content"], all_lines[start..end].join("\n"));
                assert_eq!(result.output["total_lines"], all_lines.len());
                assert_eq!(result.output["lines_returned"], end - start);
                assert_eq!(result.output["truncated"], end < all_lines.len());
            }
        }
    }

    #[tokio::test]
    async fn test_execute_dense_newline_file_returns_bounded_page() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dense.txt");
        std::fs::write(&path, "\n".repeat(MAX_FILE_BYTES)).unwrap();
        let result = FileReadTool
            .execute(
                serde_json::json!({"path": path}),
                &ToolContext::with_all_capabilities(),
            )
            .await
            .unwrap();
        assert_eq!(result.output["content"], "\n".repeat(199));
        assert_eq!(result.output["lines_returned"], 200);
        assert_eq!(result.output["total_lines"], MAX_FILE_BYTES);
        assert_eq!(result.output["truncated"], true);
    }

    #[tokio::test]
    #[ignore = "manual dense-file performance measurement"]
    async fn test_benchmark_dense_file_page() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("dense.txt");
        std::fs::write(&path, "\n".repeat(MAX_FILE_BYTES)).unwrap();
        let ctx = ToolContext::with_all_capabilities();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            let result = FileReadTool
                .execute(serde_json::json!({"path": path}), &ctx)
                .await
                .unwrap();
            assert_eq!(result.output["lines_returned"], 200);
            assert_eq!(result.output["total_lines"], MAX_FILE_BYTES);
        }
        eprintln!(
            "dense_file_page_iterations=100 elapsed_us={}",
            started.elapsed().as_micros()
        );
    }

    #[test]
    fn test_descriptor() {
        let tool = FileReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "file_read");
        assert_eq!(desc.category, ToolCategory::Utility);
        assert_eq!(desc.params.len(), 3);
    }

    #[test]
    fn test_validate_ok() {
        let tool = FileReadTool;
        assert!(tool
            .validate_params(&serde_json::json!({"path": "Cargo.toml"}))
            .is_ok());
    }

    #[test]
    fn test_validate_missing() {
        let tool = FileReadTool;
        assert!(tool.validate_params(&serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn test_execute_not_found() {
        let tool = FileReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"path": "/tmp/nonexistent_file_xyz.txt"}),
                &ctx,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_cargo_toml() {
        let tool = FileReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"path": "Cargo.toml", "max_lines": 5}),
                &ctx,
            )
            .await;
        if let Ok(r) = result {
            assert!(r.output["lines_returned"].as_u64().unwrap() <= 5);
        }
    }

    #[tokio::test]
    async fn test_execute_does_not_panic_or_overflow_on_near_u64_max_start_line() {
        // Regression for a fresh-eyes review finding: start_line/max_lines
        // come straight from model-supplied tool-call JSON. A near-u64::MAX
        // start_line used to overflow `start_line - 1 + max_lines` (panics
        // in debug, wraps to a bogus small `end` in release -> slice-index
        // panic). Both should now be handled gracefully, returning zero
        // lines rather than panicking.
        let tool = FileReadTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"path": "Cargo.toml", "start_line": u64::MAX, "max_lines": 200}),
                &ctx,
            )
            .await
            .expect("should not panic or error, just return an empty range");
        assert_eq!(result.output["lines_returned"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_execute_denies_a_path_outside_allowed_read_roots() {
        // Regression for a fresh-eyes review finding: unlike file_write,
        // file_read never consulted ToolContext's read-root allowlist,
        // so a model (or prompt-injected content) could read any file
        // on disk -- e.g. ~/.ssh/id_rsa or a .env -- and have its
        // contents streamed straight into the model's own context.
        let tool = FileReadTool;
        let temp = tempfile::tempdir().expect("tempdir");
        let allowed_root = temp.path().join("allowed");
        let outside_root = temp.path().join("outside");
        std::fs::create_dir_all(&allowed_root).unwrap();
        std::fs::create_dir_all(&outside_root).unwrap();
        let secret_file = outside_root.join("secret.txt");
        std::fs::write(&secret_file, "top-secret-content").unwrap();

        let ctx = ToolContext {
            allowed_read_roots: vec![allowed_root],
            ..ToolContext::with_all_capabilities()
        };

        let result = tool
            .execute(
                serde_json::json!({"path": secret_file.display().to_string()}),
                &ctx,
            )
            .await;

        assert!(
            matches!(result, Err(ToolError::PathNotAllowed(_))),
            "expected PathNotAllowed, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_execute_refuses_the_daemon_operator_capability_file() {
        // Regression: the default read roots include `.impulse`, so a voice
        // or MCP caller could read the daemon's operator token through this
        // tool and present it on the socket as an operator.
        let tool = FileReadTool;
        let temp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let sockets = root.join(".impulse/sockets");
        std::fs::create_dir_all(&sockets).unwrap();
        let cap = impulse_ops::operator_capability::path_for_socket(&sockets.join("impulse.sock"));
        let token = "ab".repeat(32);
        std::fs::write(&cap, &token).unwrap();

        let ctx = ToolContext {
            allowed_read_roots: vec![root.clone(), root.join(".impulse")],
            ..ToolContext::with_all_capabilities()
        };
        let result = tool
            .execute(serde_json::json!({"path": cap.display().to_string()}), &ctx)
            .await;

        assert!(
            matches!(result, Err(ToolError::PathNotAllowed(_))),
            "expected PathNotAllowed, got: {result:?}"
        );
    }
}
