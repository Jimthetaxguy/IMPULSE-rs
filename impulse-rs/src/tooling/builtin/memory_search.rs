//! Memory search tool — query GENOME and history via retrieval index
//!
//! Allows agents to search Impulse's persistent memory (GENOME.md decisions
//! and session history) using the retrieval index. Supports keyword and
//! semantic search modes.

use async_trait::async_trait;

use crate::retrieval::store::RetrievalStore;
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Search Impulse's persistent memory (GENOME and history).
///
/// Agents can search for past decisions, patterns, and session context
/// using keyword or semantic search against the retrieval index.
pub struct MemorySearchTool;

#[async_trait]
impl DynamicTool for MemorySearchTool {
    fn id(&self) -> &str {
        "memory_search"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "memory_search".into(),
            name: "Memory Search".into(),
            description: "Search GENOME decisions and session history via retrieval index".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Analysis,
            params: vec![
                ToolParam {
                    name: "query".into(),
                    description: "Search query text".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "scope".into(),
                    description: "What to search: 'history', 'genome', or 'all' (default: all)"
                        .into(),
                    param_type: ParamType::String,
                    required: false,
                    default: Some(serde_json::json!("all")),
                },
                ToolParam {
                    name: "mode".into(),
                    description: "Search mode: 'keyword' or 'semantic' (default: keyword)".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: Some(serde_json::json!("keyword")),
                },
                ToolParam {
                    name: "limit".into(),
                    description: "Maximum results (default: 5)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(5)),
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
            ],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("query").and_then(|v| v.as_str()) {
            Some(q) if !q.trim().is_empty() => Ok(()),
            _ => Err(ToolError::InvalidParams("missing or empty 'query'".into())),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'query'".into()))?;
        let scope = params
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("all");
        let mode_str = params
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("keyword");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        // Review round 1 (P2-2/P2-3), refined review round 5 (P1 Codex,
        // items 2/3): default from `ctx.impulse_dir` (the project's own
        // state directory as of round 5, not `$HOME/.impulse`) rather than
        // the bare literal ".impulse". An explicitly-supplied `impulse_dir`
        // is validated by `resolve_and_validate_memory_dir` against a
        // CLOSED set (the project default, or an explicitly-configured
        // `IMPULSE_HOME`) -- deliberately NOT the shared `allowed_read_roots`
        // `file_read`/`bash_exec` use, so granting this tool access to an
        // out-of-repo `IMPULSE_HOME` never widens what those other tools
        // can reach. See that function's doc comment for the full
        // rationale.
        let base_path = super::resolve_and_validate_memory_dir(
            params.get("impulse_dir").and_then(|v| v.as_str()),
            ctx,
        )?;

        if !base_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "results": [],
                "error": "Impulse directory not found",
            })));
        }

        // Review round 5, MEDIUM/Cursor on PR #54: `memory_search` is
        // registered ungated (read-only, `Capability::FileSystemRead`
        // only), but `retrieval::search_history`/`search_genome` call
        // `RetrievalStore::open`, which does `create_dir_all` plus a
        // plain `Connection::open` (creates `retrieval.db` if missing)
        // plus WAL journal-mode pragma writes (creates `-wal`/`-shm`
        // sidecars) -- so after a `/allow` grant on any directory for an
        // unrelated reason, the model could cause SQLite files to be
        // CREATED there, a write side effect a read-only tool must never
        // have. Fixed by checking for `retrieval.db`'s existence up front
        // (no directory/file is ever created just by checking) and, when
        // present, opening it via `RetrievalStore::open_read_only` (no
        // `create_dir_all`, `OpenFlags::SQLITE_OPEN_READ_ONLY`, no pragma
        // writes, no schema init) rather than the write-capable `open`.
        //
        // This intentionally narrows semantic/vector search to keyword
        // search for THIS tool specifically (the tool's own default mode):
        // vector search needs the optional `sqlite-vec` native extension
        // loaded and, in this codebase's current implementation, is wired
        // through the same write-capable `RetrievalStore`/`Config`-driven
        // path used by indexing. Re-plumbing that through a strictly
        // read-only connection is a larger, separate change; the safety
        // property (no side effects from a read-only tool) matters more
        // here than semantic-mode parity, and `mode` is still echoed back
        // in the response unchanged so a caller can see what it asked for.
        let db_path = base_path.join("retrieval.db");
        if !db_path.exists() {
            return Ok(ToolResult::json(serde_json::json!({
                "results": [],
                "error": "No retrieval index found",
            })));
        }
        let store = match RetrievalStore::open_read_only(&base_path) {
            Ok(store) => store,
            Err(e) => {
                return Ok(ToolResult::json(serde_json::json!({
                    "results": [],
                    "error": format!("retrieval index unavailable: {e}"),
                })));
            }
        };

        let mut all_results = Vec::new();

        if scope == "history" || scope == "all" {
            match store.search_history_keyword(query, limit) {
                Ok(rows) => {
                    for result in rows {
                        all_results.push(serde_json::json!({
                            "source": "history",
                            "id": result.id,
                            "title": result.title,
                            "snippet": result.snippet,
                            "score": result.score,
                        }));
                    }
                }
                Err(e) => {
                    all_results.push(serde_json::json!({
                        "source": "history",
                        "error": e.to_string(),
                    }));
                }
            }
        }

        if scope == "genome" || scope == "all" {
            match store.search_genome_keyword(query, limit) {
                Ok(rows) => {
                    for result in rows {
                        all_results.push(serde_json::json!({
                            "source": "genome",
                            "id": result.id,
                            "title": result.title,
                            "snippet": result.snippet,
                            "score": result.score,
                        }));
                    }
                }
                Err(e) => {
                    all_results.push(serde_json::json!({
                        "source": "genome",
                        "error": e.to_string(),
                    }));
                }
            }
        }

        // Sort by score descending
        all_results.sort_by(|a, b| {
            let sa = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let sb = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(limit);
        let count = all_results.len();

        Ok(ToolResult::json(serde_json::json!({
            "query": query,
            "scope": scope,
            "mode": mode_str,
            "results": all_results,
            "count": count,
        })))
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
        let tool = MemorySearchTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "memory_search");
        assert_eq!(desc.category, ToolCategory::Analysis);
        assert_eq!(desc.params.len(), 5);
    }

    #[test]
    fn test_validate_ok() {
        let tool = MemorySearchTool;
        assert!(tool
            .validate_params(&serde_json::json!({"query": "auth"}))
            .is_ok());
    }

    #[test]
    fn test_validate_missing_query() {
        let tool = MemorySearchTool;
        assert!(tool.validate_params(&serde_json::json!({})).is_err());
    }

    #[tokio::test]
    async fn test_execute_no_impulse() {
        // Review round 5, P1 Codex (item 3): an explicit `impulse_dir` is no
        // longer honored unconditionally -- it must resolve to `ctx.
        // impulse_dir` or an explicitly-configured `IMPULSE_HOME` (see
        // `resolve_and_validate_memory_dir`), so this exercises the
        // nonexistent-directory path via `ctx.impulse_dir` itself instead.
        let tool = MemorySearchTool;
        let ctx = ToolContext {
            impulse_dir: std::path::PathBuf::from("/tmp/nonexistent_impulse_xyz"),
            ..ToolContext::with_all_capabilities()
        };
        let result = tool
            .execute(serde_json::json!({"query": "auth"}), &ctx)
            .await
            .unwrap();
        assert!(result.output.get("error").is_some() || result.output["count"] == 0);
    }

    #[tokio::test]
    async fn test_execute_defaults_impulse_dir_from_ctx_when_the_param_is_omitted() {
        // Review round 1, P2-2/P2-3: an omitted `impulse_dir` must resolve
        // via `ctx.impulse_dir`, not a bare ".impulse" relative to the
        // process's own working directory -- see
        // `ReplContext::sandbox_tool_context`'s doc comment. A nonexistent
        // `ctx.impulse_dir` reports "Impulse directory not found" just
        // like an explicit nonexistent path did before this fix, proving
        // the default is actually consulted.
        let ctx = ToolContext {
            impulse_dir: std::path::PathBuf::from("/tmp/nonexistent_impulse_from_ctx_xyz"),
            ..ToolContext::with_all_capabilities()
        };

        let tool = MemorySearchTool;
        let result = tool
            .execute(serde_json::json!({"query": "auth"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result.output["error"], "Impulse directory not found");
    }

    /// Serializes tests that mutate the process-global `IMPULSE_HOME` env
    /// var. Delegates to the crate-wide `test_support::
    /// impulse_home_env_lock`, shared with `ion_repl::history`/`ion_repl::mod`/
    /// `genome_read.rs` -- a per-file lock only serializes within that one
    /// file, not against the others, which all mutate the same
    /// process-global var under `cargo test`'s default multi-threaded
    /// execution.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::impulse_home_env_lock()
    }

    #[tokio::test]
    async fn test_execute_refuses_an_explicit_impulse_dir_outside_the_allowed_set() {
        // Review round 5, P1 Codex (items 2/3): an explicit `impulse_dir`
        // that is neither `ctx.impulse_dir` nor the configured
        // `IMPULSE_HOME` must be refused outright -- NOT silently accepted
        // the way any explicit value was before this round (which is what
        // made widening the shared `allowed_read_roots` to cover
        // `IMPULSE_HOME` look necessary in the first place).
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        std::env::remove_var("IMPULSE_HOME");

        let ctx_dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            impulse_dir: ctx_dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };
        let tool = MemorySearchTool;
        let result = tool
            .execute(
                serde_json::json!({
                    "query": "auth",
                    "impulse_dir": "/tmp/nonexistent_impulse_explicit_xyz"
                }),
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
    async fn test_execute_accepts_an_explicit_impulse_dir_matching_configured_impulse_home() {
        // Review round 5, item 3's exact acceptance test (the genome_read/
        // memory_search half of it -- `file_read` denying the same path is
        // proven at the `ReplContext::sandbox_tool_context` level in
        // `ion_repl::mod`'s own tests): an explicit `impulse_dir` equal to
        // the process's own configured `IMPULSE_HOME` is accepted, even
        // though it is outside `ctx.impulse_dir` (the project default) and
        // never added to the shared `allowed_read_roots`.
        let _guard = env_lock();
        let prev = std::env::var("IMPULSE_HOME").ok();
        let home_dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("IMPULSE_HOME", home_dir.path());

        let project_dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            impulse_dir: project_dir.path().to_path_buf(),
            allowed_read_roots: vec![project_dir.path().to_path_buf()],
            ..ToolContext::with_all_capabilities()
        };
        let tool = MemorySearchTool;
        let result = tool
            .execute(
                serde_json::json!({
                    "query": "auth",
                    "impulse_dir": home_dir.path().to_str().unwrap()
                }),
                &ctx,
            )
            .await;

        match prev {
            Some(value) => std::env::set_var("IMPULSE_HOME", value),
            None => std::env::remove_var("IMPULSE_HOME"),
        }

        // "No retrieval index found" (not a PathNotAllowed refusal) proves
        // the path was ACCEPTED and the tool got as far as checking for an
        // index -- home_dir has no retrieval.db, so this is the expected
        // outcome for an allowed-but-unindexed directory.
        let result = result.unwrap();
        assert_eq!(result.output["error"], "No retrieval index found");
    }

    #[tokio::test]
    async fn test_execute_reports_no_retrieval_index_when_impulse_dir_exists_but_db_does_not() {
        // Review round 5, MEDIUM/Cursor: a directory that exists (e.g. the
        // project's real `.impulse/`, or anything `/allow`-granted) but has
        // never been indexed must be refused with a distinct, typed
        // message -- NOT silently treated as "create the index here".
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = MemorySearchTool;
        let result = tool
            .execute(serde_json::json!({"query": "auth"}), &ctx)
            .await
            .unwrap();

        assert_eq!(result.output["error"], "No retrieval index found");
    }

    #[tokio::test]
    async fn test_execute_creates_no_files_under_a_granted_directory_with_no_index() {
        // Review round 5, MEDIUM/Cursor (CONFIRMED): before this fix,
        // querying a directory with no existing index caused
        // `RetrievalStore::open` to create `retrieval.db` (plus WAL
        // `-wal`/`-shm` sidecars) right there -- a write side effect an
        // ungated, read-only-registered tool must never have. Assert the
        // directory is byte-for-byte empty after the call, not just that
        // the response looks right.
        let dir = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };

        let tool = MemorySearchTool;
        tool.execute(serde_json::json!({"query": "auth"}), &ctx)
            .await
            .unwrap();

        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(
            entries.is_empty(),
            "memory_search must not create any file in a directory with no retrieval index, \
             found: {entries:?}"
        );
    }

    #[tokio::test]
    async fn test_execute_finds_results_via_the_read_only_path_against_a_real_index() {
        use crate::retrieval::store::{HistoryUpsert, RetrievalStore};

        let dir = tempfile::TempDir::new().unwrap();
        let store = RetrievalStore::open(dir.path()).unwrap();
        store.init_schema().unwrap();
        store
            .upsert_history(HistoryUpsert {
                session_id: "s1",
                session_name: "session one",
                platform: None,
                started_at: "2026-01-01",
                ended_at: "2026-01-01",
                summary: "summary",
                files_touched_json: "[]",
                tools_used_json: "[]",
                search_text: "a very particular needle phrase",
                content_hash: "",
            })
            .unwrap();
        store.refresh_fts().unwrap();
        drop(store);

        let ctx = ToolContext {
            impulse_dir: dir.path().to_path_buf(),
            ..ToolContext::with_all_capabilities()
        };
        let tool = MemorySearchTool;
        let result = tool
            .execute(
                serde_json::json!({"query": "needle", "scope": "history"}),
                &ctx,
            )
            .await
            .unwrap();

        assert_eq!(result.output["count"], 1, "{}", result.output);
        assert_eq!(result.output["results"][0]["source"], "history");
    }
}
