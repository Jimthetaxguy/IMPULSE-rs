//! Genome reader tool — read permanent project decisions from GENOME.md
//!
//! The GENOME file contains durable project decisions, preferences, and
//! patterns that persist across all sessions. This tool lets agents
//! read and search the genome without parsing it manually.

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Characters returned per call when the caller does not specify
/// `max_chars` -- matches `ion_repl::tool_document::DEFAULT_MAX_CHARS`'s
/// value exactly (kept as a separate local constant, not imported: that
/// module is gated behind the `office-support` feature, while
/// `genome_read` is not and must keep working in a
/// `--no-default-features` build).
const DEFAULT_MAX_CHARS: usize = 12_000;
/// Hard ceiling on `max_chars`, whatever the caller asks -- mirrors
/// `ion_repl::tool_document::MAX_CHARS_CAP`.
const MAX_CHARS_CAP: usize = 32_000;

/// Read the project GENOME — permanent decisions and preferences.
///
/// GENOME.md is Impulse's long-term memory: architectural decisions,
/// user preferences, tool configurations, and learned patterns that
/// should survive across all sessions.
///
/// **Paging (review round 5, P2/Codex on PR #54):** this tool used to
/// return the WHOLE `GENOME.md` (or a whole matched section) unbounded --
/// with the loop contract's newest-result compaction floor
/// (`ion_repl::chat`'s context-budget handling), a sufficiently large
/// genome could trip `LoopTrip::ContextBudget` before the provider ever
/// saw the result at all. `execute` now windows its content exactly the
/// way `ion_repl::tool_document::window` does for `document_read`
/// (character-counted, snapped back to the last full line on a truncated
/// cut, never mid-line): `max_chars` (default [`DEFAULT_MAX_CHARS`],
/// capped at [`MAX_CHARS_CAP`]) and `offset` page through either the full
/// file or the matched section's own text, and a truncated response
/// reports `truncated`/`next_offset` so a caller can page through the rest
/// -- the same field names `document_read`'s `DocumentWindow` uses, so a
/// model already familiar with paging `document_read` recognizes this
/// immediately. (Reimplemented locally rather than calling
/// `tool_document::window` directly: that module is gated behind
/// `office-support`, and this tool is not.)
pub struct GenomeReadTool;

/// Windows `text` starting at `offset` (in characters), returning at most
/// `max_chars` characters, snapped back to the last full line if the cut
/// would otherwise land mid-line. Returns `(content, returned_chars,
/// truncated, next_offset)` -- identical shape and behavior to
/// `ion_repl::tool_document::window`, reimplemented here to avoid a
/// dependency on that `office-support`-gated module.
fn window(text: &str, offset: usize, max_chars: usize) -> (String, usize, bool, Option<usize>) {
    let total = text.chars().count();
    let start = offset.min(total);
    let mut content: String = text.chars().skip(start).take(max_chars).collect();
    let mut returned = content.chars().count();
    let truncated = start + returned < total;
    if truncated && !content.ends_with('\n') {
        if let Some(cut) = content.rfind('\n') {
            // '\n' is one byte, so `cut + 1` is a char boundary.
            content.truncate(cut + 1);
            returned = content.chars().count();
        }
    }
    let next_offset = truncated.then_some(start + returned);
    (content, returned, truncated, next_offset)
}

#[async_trait]
impl DynamicTool for GenomeReadTool {
    fn id(&self) -> &str {
        "genome_read"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "genome_read".into(),
            name: "Genome Read".into(),
            description: "Read permanent project decisions and preferences from GENOME.md".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "section".into(),
                    description:
                        "Optional section filter (e.g., 'decisions', 'preferences', 'patterns')"
                            .into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "impulse_dir".into(),
                    description: "Path to .impulse directory (default: the project's own \
                                   state directory; an explicit value must be that same \
                                   directory or the configured IMPULSE_HOME)"
                        .into(),
                    // Review round 5, P1 Codex: deliberately NOT
                    // `ParamType::FilePath` -- see
                    // `builtin::resolve_and_validate_memory_dir`'s doc
                    // comment for why this must not be checked against the
                    // shared `allowed_read_roots`.
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "max_chars".into(),
                    description: format!(
                        "Characters returned per call (default {DEFAULT_MAX_CHARS}, capped at \
                         {MAX_CHARS_CAP}); page through a large genome with `offset`"
                    ),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(DEFAULT_MAX_CHARS)),
                },
                ToolParam {
                    name: "offset".into(),
                    description: "Character offset to resume from (from a prior call's \
                                   `next_offset`); default 0"
                        .into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(0)),
                },
            ],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(()) // All params optional
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        // Review round 1 (P2-2/P2-3), refined review round 5 (P1 Codex,
        // items 2/3): default from `ctx.impulse_dir` (the project's own
        // state directory as of round 5, not `$HOME/.impulse`) rather than
        // the bare literal ".impulse". An explicitly-supplied `impulse_dir`
        // is validated against a CLOSED set (the project default, or an
        // explicitly-configured `IMPULSE_HOME`) by `resolve_and_validate_
        // memory_dir`, deliberately NOT the shared `allowed_read_roots`
        // `file_read`/`bash_exec` use -- see that function's doc comment
        // (in `builtin/mod.rs`) for the full rationale, and
        // `memory_search.rs`'s identical call for the sibling tool.
        let impulse_dir = super::resolve_and_validate_memory_dir(
            params.get("impulse_dir").and_then(|v| v.as_str()),
            ctx,
        )?;
        let section_filter = params.get("section").and_then(|v| v.as_str());
        // Review round 5, P2/Codex: `max_chars` defaults to
        // `DEFAULT_MAX_CHARS` and is capped at `MAX_CHARS_CAP` regardless
        // of what the caller asks -- mirrors `document_read`'s clamp
        // (`ion_repl::tool_document::parse_request`).
        //
        // Review round 6, LOW: the ORIGINAL version of this block used
        // `.and_then(|v| v.as_u64())...unwrap_or(...)`, which silently
        // treats `max_chars: 0`, a negative `max_chars`/`offset`, and a
        // non-integer value (a JSON string or a non-whole float, since
        // `Value::as_u64` returns `None` for all of those) the same way it
        // treats "the caller omitted the field" -- clamping 0 up to 1 and
        // falling every other rejected value back to the default/zero
        // rather than reporting the mistake. `genome_read`'s own doc
        // comment above claims parity with `document_read`, whose
        // `parse_request` bails on exactly these cases instead of
        // guessing what the caller meant -- mirrored here so the claimed
        // parity is real, not just the windowing algorithm and field
        // names.
        let max_chars = match params.get("max_chars") {
            None | Some(serde_json::Value::Null) => DEFAULT_MAX_CHARS,
            Some(v) => {
                let requested = v.as_u64().ok_or_else(|| {
                    ToolError::InvalidParams(format!(
                        "'max_chars' must be a positive integer, got {v}"
                    ))
                })?;
                if requested == 0 {
                    return Err(ToolError::InvalidParams(
                        "'max_chars' must be at least 1".to_string(),
                    ));
                }
                usize::try_from(requested)
                    .unwrap_or(MAX_CHARS_CAP)
                    .min(MAX_CHARS_CAP)
            }
        };
        let offset = match params.get("offset") {
            None | Some(serde_json::Value::Null) => 0,
            Some(v) => {
                let requested = v.as_u64().ok_or_else(|| {
                    ToolError::InvalidParams(format!(
                        "'offset' must be a non-negative integer, got {v}"
                    ))
                })?;
                usize::try_from(requested)
                    .map_err(|_| ToolError::InvalidParams("'offset' is too large".to_string()))?
            }
        };

        let genome_path = impulse_dir.join("GENOME.md");

        if !genome_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "exists": false,
                "content": null,
                "message": "No GENOME.md found — project has no permanent decisions recorded yet"
            })));
        }

        let content = std::fs::read_to_string(&genome_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read GENOME.md: {}", e)))?;

        // If section filter specified, extract just that section
        if let Some(section) = section_filter {
            let section_lower = section.to_lowercase();
            let mut in_section = false;
            let mut section_lines = Vec::new();
            let mut section_found = false;

            for line in content.lines() {
                if line.starts_with("## ") || line.starts_with("# ") {
                    if in_section {
                        break; // End of target section
                    }
                    if line.to_lowercase().contains(&section_lower) {
                        in_section = true;
                        section_found = true;
                        section_lines.push(line.to_string());
                        continue;
                    }
                }
                if in_section {
                    section_lines.push(line.to_string());
                }
            }

            if section_found {
                let section_text = section_lines.join("\n");
                let total_chars = section_text.chars().count();
                let (windowed, returned_chars, truncated, next_offset) =
                    window(&section_text, offset, max_chars);
                Ok(ToolResult::json(serde_json::json!({
                    "exists": true,
                    "section": section,
                    "content": windowed,
                    "total_length": content.len(),
                    "total_chars": total_chars,
                    "offset": offset.min(total_chars),
                    "returned_chars": returned_chars,
                    "truncated": truncated,
                    "next_offset": next_offset,
                })))
            } else {
                // List available sections to help the agent
                let sections: Vec<&str> = content
                    .lines()
                    .filter(|l| l.starts_with("## ") || l.starts_with("# "))
                    .collect();
                Ok(ToolResult::json(serde_json::json!({
                    "exists": true,
                    "section": section,
                    "content": null,
                    "message": format!("Section '{}' not found", section),
                    "available_sections": sections,
                })))
            }
        } else {
            // Return the full genome, windowed, with basic stats.
            let line_count = content.lines().count();
            let sections: Vec<&str> = content
                .lines()
                .filter(|l| l.starts_with("## ") || l.starts_with("# "))
                .collect();
            let total_chars = content.chars().count();
            let (windowed, returned_chars, truncated, next_offset) =
                window(&content, offset, max_chars);

            Ok(ToolResult::json(serde_json::json!({
                "exists": true,
                "content": windowed,
                "lines": line_count,
                "sections": sections,
                "size_bytes": content.len(),
                "total_chars": total_chars,
                "offset": offset.min(total_chars),
                "returned_chars": returned_chars,
                "truncated": truncated,
                "next_offset": next_offset,
            })))
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

#[cfg(test)]
// clippy: some tests here hold `impulse_home_env_lock()`/`env_lock()`
// across an `.await` (must span the whole IMPULSE_HOME-dependent async
// call so a concurrent test can't mutate the env var mid-call); see
// ion_repl::mod's identical justification for the same pattern.
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = GenomeReadTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "genome_read");
        assert_eq!(desc.category, ToolCategory::Analysis);
    }

    #[tokio::test]
    async fn test_execute_no_genome() {
        // Review round 5, P1 Codex (item 3): an explicit `impulse_dir` is no
        // longer honored unconditionally -- see
        // `resolve_and_validate_memory_dir` -- so this exercises the
        // nonexistent-directory path via `ctx.impulse_dir` itself.
        let tool = GenomeReadTool;
        let ctx = ToolContext {
            impulse_dir: std::path::PathBuf::from("/tmp/nonexistent_impulse_xyz"),
            ..ToolContext::with_all_capabilities()
        };
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert_eq!(result.output["exists"], false);
    }

    #[tokio::test]
    async fn test_execute_defaults_impulse_dir_from_ctx_when_the_param_is_omitted() {
        // Review round 1, P2-2/P2-3: an omitted `impulse_dir` must resolve
        // via `ctx.impulse_dir` (what `ReplContext::sandbox_tool_context`
        // sets from `history::impulse_home()`), not a bare ".impulse"
        // relative to the process's own working directory.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("GENOME.md"), "# From ctx.impulse_dir").unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();

        assert_eq!(result.output["exists"], true);
        assert!(result.output["content"]
            .as_str()
            .unwrap()
            .contains("From ctx.impulse_dir"));
    }

    /// Serializes tests that mutate the process-global `IMPULSE_HOME` env
    /// var. Delegates to the crate-wide `test_support::
    /// impulse_home_env_lock`, shared with `ion_repl::history`/`ion_repl::mod`/
    /// `memory_search.rs` -- a per-file lock only serializes within that
    /// one file, not against the others, which all mutate the same
    /// process-global var under `cargo test`'s default multi-threaded
    /// execution.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::impulse_home_env_lock()
    }

    #[tokio::test]
    async fn test_execute_refuses_an_explicit_impulse_dir_outside_the_allowed_set() {
        // Review round 5, P1 Codex (items 2/3): an explicit `impulse_dir`
        // that is neither `ctx.impulse_dir` nor the configured
        // `IMPULSE_HOME` must be refused outright.
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        std::env::remove_var("IMPULSE_HOME");

        let ctx_dir = tempfile::TempDir::new().unwrap();
        let explicit_dir = tempfile::TempDir::new().unwrap();
        std::fs::write(ctx_dir.path().join("GENOME.md"), "# ctx default").unwrap();
        std::fs::write(explicit_dir.path().join("GENOME.md"), "# explicit override").unwrap();
        let ctx = ToolContext {
            impulse_dir: ctx_dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool
            .execute(
                serde_json::json!({"impulse_dir": explicit_dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;

        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }

        assert!(
            matches!(result, Err(ToolError::PathNotAllowed(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn test_execute_when_impulse_home_is_set_both_project_and_home_impulse_dir_work() {
        // Review round 5, item 3's original acceptance test, extended in
        // review round 6 (MEDIUM REFUTED) to also prove the project's own
        // directory STILL works once `IMPULSE_HOME` is set -- the exact
        // regression the review found: with `ctx.impulse_dir` and
        // `ctx.project_impulse_dir` constructed the way
        // `ReplContext::sandbox_tool_context` REALLY produces them when
        // `IMPULSE_HOME` is configured (`impulse_dir` becomes
        // `IMPULSE_HOME` itself; `project_impulse_dir` stays the project's
        // own directory, set independently), an earlier version of
        // `resolve_and_validate_memory_dir` checked an explicit override
        // against `{ctx.impulse_dir, IMPULSE_HOME}` -- which collapsed to
        // `{IMPULSE_HOME, IMPULSE_HOME}` here, so `genome_read
        // {"impulse_dir": "<project>/.impulse"}` was wrongly DENIED.
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        let home_dir = tempfile::TempDir::new().unwrap();
        std::fs::write(home_dir.path().join("GENOME.md"), "# from IMPULSE_HOME").unwrap();
        std::env::set_var("IMPULSE_HOME", home_dir.path());

        let project_dir = tempfile::TempDir::new().unwrap();
        std::fs::write(project_dir.path().join("GENOME.md"), "# from project").unwrap();
        // Mirrors what `sandbox_tool_context` actually produces once
        // `IMPULSE_HOME` is set: `impulse_dir` == the home directory,
        // `project_impulse_dir` == the project's own, independently.
        let ctx = ToolContext {
            impulse_dir: home_dir.path().to_path_buf(),
            project_impulse_dir: project_dir.path().to_path_buf(),
            allowed_read_roots: vec![project_dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };
        let tool = GenomeReadTool;

        let home_result = tool
            .execute(
                serde_json::json!({"impulse_dir": home_dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await
            .expect("the configured IMPULSE_HOME must remain reachable");
        let project_result = tool
            .execute(
                serde_json::json!({"impulse_dir": project_dir.path().to_str().unwrap()}),
                &ctx,
            )
            .await;

        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }

        assert!(home_result.output["content"]
            .as_str()
            .unwrap()
            .contains("from IMPULSE_HOME"));
        let project_result =
            project_result.expect("the project's own .impulse must stay reachable too");
        assert!(project_result.output["content"]
            .as_str()
            .unwrap()
            .contains("from project"));
    }

    // ----------------------------------------------------------------
    // Review round 5, P2/Codex: max_chars/offset paging.
    // ----------------------------------------------------------------

    #[test]
    fn test_window_accepts_everything_when_it_fits() {
        let (content, returned, truncated, next_offset) = window("hello world", 0, 100);
        assert_eq!(content, "hello world");
        assert_eq!(returned, 11);
        assert!(!truncated);
        assert_eq!(next_offset, None);
    }

    #[test]
    fn test_window_snaps_a_truncated_cut_to_the_last_full_line() {
        let text = "line one\nline two\nline three\n";
        // Cut mid-way through "line two" (offset 0, cap at 12 chars would
        // land inside "line two"): the window must snap back to the end of
        // "line one\n" rather than returning a partial line.
        let (content, returned, truncated, next_offset) = window(text, 0, 12);
        assert_eq!(content, "line one\n");
        assert_eq!(returned, 9);
        assert!(truncated);
        assert_eq!(next_offset, Some(9));
    }

    #[test]
    fn test_window_past_end_is_empty_and_not_truncated() {
        let (content, returned, truncated, next_offset) = window("short", 100, 10);
        assert_eq!(content, "");
        assert_eq!(returned, 0);
        assert!(!truncated);
        assert_eq!(next_offset, None);
    }

    #[tokio::test]
    async fn test_execute_paginates_a_large_genome_at_the_default_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        // One line per number, comfortably over DEFAULT_MAX_CHARS so the
        // response must be truncated.
        let big = (0..3000)
            .map(|i| format!("line {i} of a very large genome file"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(big.chars().count() > DEFAULT_MAX_CHARS);
        std::fs::write(dir.path().join("GENOME.md"), &big).unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();

        assert_eq!(result.output["truncated"], true);
        assert!(result.output["next_offset"].as_u64().is_some());
        let returned = result.output["content"].as_str().unwrap();
        assert!(
            returned.chars().count() <= DEFAULT_MAX_CHARS,
            "returned {} chars, over the default cap",
            returned.chars().count()
        );
        // The whole file must not have been dumped into `content`.
        assert!(returned.chars().count() < big.chars().count());
    }

    #[tokio::test]
    async fn test_execute_max_chars_is_capped_regardless_of_what_the_caller_asks() {
        let dir = tempfile::TempDir::new().unwrap();
        let big = "x".repeat(MAX_CHARS_CAP * 2);
        std::fs::write(dir.path().join("GENOME.md"), &big).unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool
            .execute(serde_json::json!({"max_chars": MAX_CHARS_CAP * 10}), &ctx)
            .await
            .unwrap();

        let returned = result.output["content"].as_str().unwrap();
        assert!(
            returned.chars().count() <= MAX_CHARS_CAP,
            "returned {} chars, over MAX_CHARS_CAP",
            returned.chars().count()
        );
    }

    // Review round 6, LOW: `max_chars: 0` used to be silently clamped up
    // to 1 and a negative/non-integer `max_chars`/`offset` used to fall
    // back to the default/zero rather than reporting the mistake --
    // `genome_read`'s own doc comment claims parity with `document_read`'s
    // `parse_request`, which bails on all four of these instead. The four
    // tests below prove the mirrored bail behavior.

    #[tokio::test]
    async fn test_execute_rejects_a_zero_max_chars() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("GENOME.md"), "content").unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool
            .execute(serde_json::json!({"max_chars": 0}), &ctx)
            .await;

        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
    }

    #[tokio::test]
    async fn test_execute_rejects_a_negative_max_chars() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("GENOME.md"), "content").unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool
            .execute(serde_json::json!({"max_chars": -5}), &ctx)
            .await;

        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
    }

    #[tokio::test]
    async fn test_execute_rejects_a_non_integer_max_chars() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("GENOME.md"), "content").unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        // A JSON string is rejected outright...
        let result = tool
            .execute(serde_json::json!({"max_chars": "100"}), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
        // ...and so is a non-whole float, which `Value::as_u64` also
        // refuses (it is stored/parsed as an f64, not a u64).
        let result = tool
            .execute(serde_json::json!({"max_chars": 100.5}), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
    }

    #[tokio::test]
    async fn test_execute_rejects_a_negative_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("GENOME.md"), "content").unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool.execute(serde_json::json!({"offset": -1}), &ctx).await;

        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
    }

    #[tokio::test]
    async fn test_execute_offset_resumes_from_a_prior_next_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let big = (0..3000)
            .map(|i| format!("line {i} of a very large genome file"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.path().join("GENOME.md"), &big).unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let first = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        let next_offset = first.output["next_offset"].as_u64().unwrap();

        let second = tool
            .execute(serde_json::json!({"offset": next_offset}), &ctx)
            .await
            .unwrap();

        let first_content = first.output["content"].as_str().unwrap();
        let second_content = second.output["content"].as_str().unwrap();
        assert_ne!(
            first_content, second_content,
            "resuming from next_offset must return different content"
        );
        assert!(big.contains(second_content));
    }

    #[tokio::test]
    async fn test_execute_paginates_a_matched_section_too() {
        let dir = tempfile::TempDir::new().unwrap();
        let section_body = (0..2000)
            .map(|i| format!("decision {i}: some detail text"))
            .collect::<Vec<_>>()
            .join("\n");
        let genome = format!("# Decisions\n{section_body}\n# Preferences\nshort\n");
        assert!(genome.chars().count() > DEFAULT_MAX_CHARS);
        std::fs::write(dir.path().join("GENOME.md"), &genome).unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = GenomeReadTool;
        let result = tool
            .execute(serde_json::json!({"section": "decisions"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result.output["exists"], true);
        assert_eq!(result.output["truncated"], true);
        let returned = result.output["content"].as_str().unwrap();
        assert!(returned.chars().count() <= DEFAULT_MAX_CHARS);
    }
}
