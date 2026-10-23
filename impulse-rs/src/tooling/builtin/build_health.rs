//! Build hygiene tools — wraps build_hygiene module for artifact management
//!
//! Provides DynamicTool wrappers for build artifact analysis, sweeping,
//! wiping, cleaning, and sccache management. All destructive operations
//! default to dry-run for safety.

use async_trait::async_trait;

use crate::build_hygiene::{self, BuildHygieneConfig};
use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

/// Runs synchronous build-hygiene work off the async runtime. These tools
/// walk directories and run `cargo` and `sccache` with no timeout; on the
/// runtime's own thread they held it, and a caller's timeout (the voice
/// webhook's request deadline, an MCP or daemon request) could not stop
/// them.
async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ToolError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("build hygiene task failed: {e}")))
}

// ============================================================================
// Build Health — analyze disk usage
// ============================================================================

/// Analyze Rust build artifact disk usage across projects.
pub struct BuildHealthTool;

#[async_trait]
impl DynamicTool for BuildHealthTool {
    fn id(&self) -> &str {
        "build_health"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "build_health".into(),
            name: "Build Health".into(),
            description: "Analyze Rust build artifact disk usage across projects".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![
                ToolParam {
                    name: "scan_paths".into(),
                    description: "Comma-separated directories to scan (supports ~). Defaults to ~/projects,~/Desktop".into(),
                    param_type: ParamType::String,
                    required: false,
                    default: None,
                },
                ToolParam {
                    name: "threshold_gb".into(),
                    description: "Size threshold in GB for warnings (default: 10.0)".into(),
                    param_type: ParamType::Float,
                    required: false,
                    default: Some(serde_json::json!(10.0)),
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
        let config = parse_hygiene_config(&params, ctx, false)?;
        let report = off_runtime(move || {
            let paths = config.expanded_scan_paths();
            let projects = build_hygiene::discover_rust_projects(&paths);
            build_hygiene::measurement::generate_report(&projects, config.size_threshold_gb)
        })
        .await?;
        Ok(ToolResult::json(
            serde_json::to_value(&report).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
        ))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead]
    }
}

// ============================================================================
// Sweep — remove stale artifacts
// ============================================================================

/// Clean stale Rust build artifacts using cargo-sweep.
pub struct SweepTool;

#[async_trait]
impl DynamicTool for SweepTool {
    fn id(&self) -> &str {
        "sweep"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "sweep".into(),
            name: "Sweep".into(),
            description:
                "Clean stale Rust build artifacts not accessed in N days (dry-run default)".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![
                ToolParam {
                    name: "days".into(),
                    description: "Remove artifacts older than this many days (default: 30)".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    default: Some(serde_json::json!(30)),
                },
                ToolParam {
                    name: "dry_run".into(),
                    description: "Only show what would be cleaned (default: true)".into(),
                    param_type: ParamType::Bool,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
                ToolParam {
                    name: "path".into(),
                    description: "Directory to sweep. Default: configured scan_paths".into(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                },
                scan_paths_param(),
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
        let config = parse_hygiene_config(&params, ctx, true)?;
        let days = params
            .get("days")
            .and_then(|v| v.as_u64())
            .unwrap_or(config.age_threshold_days as u64) as u32;
        let dry_run = params
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(config.dry_run_default);
        let paths = if let Some(p) = params.get("path").and_then(|v| v.as_str()) {
            vec![std::path::PathBuf::from(p)]
        } else {
            config.expanded_scan_paths()
        };

        let opts = build_hygiene::SweepOptions {
            days,
            dry_run,
            paths,
            recursive: true,
            verbose: false,
        };

        match off_runtime(move || build_hygiene::sweep::run_sweep(&opts)).await? {
            Ok(result) => Ok(ToolResult::json(
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            )),
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead, Capability::FileSystemWrite]
    }
}

// ============================================================================
// Wipe — aggressive target/ removal
// ============================================================================

/// Aggressively remove all target/ directories.
pub struct WipeTool;

#[async_trait]
impl DynamicTool for WipeTool {
    fn id(&self) -> &str {
        "wipe"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "wipe".into(),
            name: "Wipe".into(),
            description: "Remove all Rust target/ directories (dry-run default)".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![
                ToolParam {
                    name: "dry_run".into(),
                    description: "Only show what would be wiped (default: true)".into(),
                    param_type: ParamType::Bool,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
                ToolParam {
                    name: "path".into(),
                    description: "Directory to scan".into(),
                    param_type: ParamType::FilePath,
                    required: false,
                    default: None,
                },
                scan_paths_param(),
            ],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let config = parse_hygiene_config(&params, ctx, true)?;
        let dry_run = params
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(config.dry_run_default);
        let paths = if let Some(p) = params.get("path").and_then(|v| v.as_str()) {
            vec![std::path::PathBuf::from(p)]
        } else {
            config.expanded_scan_paths()
        };

        let opts = build_hygiene::wipe::WipeOptions {
            dry_run,
            paths,
            include_node_modules: false,
        };

        match off_runtime(move || build_hygiene::wipe::run_wipe(&opts)).await? {
            Ok(result) => Ok(ToolResult::json(
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            )),
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead, Capability::FileSystemWrite]
    }
}

// ============================================================================
// Clean All — cargo clean across projects
// ============================================================================

/// Run cargo clean across all discovered Rust projects.
pub struct CleanAllTool;

#[async_trait]
impl DynamicTool for CleanAllTool {
    fn id(&self) -> &str {
        "clean_all"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "clean_all".into(),
            name: "Clean All".into(),
            description: "Run cargo clean across all discovered Rust projects (dry-run default)"
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![
                ToolParam {
                    name: "dry_run".into(),
                    description: "Only show what would be cleaned (default: true)".into(),
                    param_type: ParamType::Bool,
                    required: false,
                    default: Some(serde_json::json!(true)),
                },
                scan_paths_param(),
            ],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let config = parse_hygiene_config(&params, ctx, true)?;
        let dry_run = params
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(config.dry_run_default);
        let paths = config.expanded_scan_paths();

        match off_runtime(move || build_hygiene::clean_all::clean_all_projects(&paths, dry_run))
            .await?
        {
            Ok(result) => Ok(ToolResult::json(
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            )),
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemRead, Capability::FileSystemWrite]
    }
}

// ============================================================================
// Sccache Status — check compilation cache
// ============================================================================

/// Check sccache installation and cache statistics.
pub struct SccacheStatusTool;

#[async_trait]
impl DynamicTool for SccacheStatusTool {
    fn id(&self) -> &str {
        "sccache_status"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "sccache_status".into(),
            name: "Sccache Status".into(),
            description: "Check sccache installation, configuration, and cache statistics".into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let status = off_runtime(build_hygiene::sccache::sccache_status).await?;
        Ok(ToolResult::json(
            serde_json::to_value(&status).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
        ))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::SystemInfo]
    }
}

// ============================================================================
// Sccache Setup — configure compilation cache
// ============================================================================

/// Configure sccache as the Rust compilation cache.
pub struct SccacheSetupTool;

#[async_trait]
impl DynamicTool for SccacheSetupTool {
    fn id(&self) -> &str {
        "sccache_setup"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "sccache_setup".into(),
            name: "Sccache Setup".into(),
            description: "Configure sccache as the Rust compilation cache in ~/.cargo/config.toml"
                .into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![ToolParam {
                name: "check_only".into(),
                description: "Only check if configured, don't modify (default: false)".into(),
                param_type: ParamType::Bool,
                required: false,
                default: Some(serde_json::json!(false)),
            }],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let check_only = params
            .get("check_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match off_runtime(move || build_hygiene::sccache::sccache_setup(check_only)).await? {
            Ok(result) => Ok(ToolResult::json(
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?,
            )),
            Err(e) => Err(ToolError::ExecutionFailed(e.to_string())),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::FileSystemWrite]
    }
}

// ============================================================================
// Tool Availability — check which cargo tools are installed
// ============================================================================

/// Check which cargo build tools are installed.
pub struct ToolAvailabilityTool;

#[async_trait]
impl DynamicTool for ToolAvailabilityTool {
    fn id(&self) -> &str {
        "tool_availability"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "tool_availability".into(),
            name: "Tool Availability".into(),
            description:
                "Check which cargo build tools are installed (cargo-sweep, cargo-wipe, sccache)"
                    .into(),
            version: "0.1.0".into(),
            category: ToolCategory::System,
            params: vec![],
        }
    }

    fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tools_status: Vec<serde_json::Value> = off_runtime(|| {
            vec![
                (
                    "cargo-sweep",
                    build_hygiene::is_cargo_tool_installed("sweep"),
                ),
                ("cargo-wipe", build_hygiene::is_cargo_tool_installed("wipe")),
                (
                    "cargo-clean-all",
                    build_hygiene::is_cargo_tool_installed("clean-all"),
                ),
                (
                    "sccache",
                    build_hygiene::sccache::sccache_status().installed,
                ),
                // Host interpreter on PATH, not the embedded sandbox: this list
                // describes the machine's build toolchain.
                (
                    "python",
                    which::which("python3")
                        .or_else(|_| which::which("python"))
                        .is_ok(),
                ),
            ]
            .into_iter()
            .map(|(name, installed)| {
                serde_json::json!({
                    "name": name,
                    "installed": installed,
                })
            })
            .collect()
        })
        .await?;

        Ok(ToolResult::json(
            serde_json::json!({ "tools": tools_status }),
        ))
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::SystemInfo]
    }
}

// ============================================================================
// Helper
// ============================================================================

/// `scan_paths`, which every hygiene tool reads through
/// [`parse_hygiene_config`]. Declared on each of them so callers can see it
/// and the path-param guard test covers it; it is a comma-separated String,
/// so the tool itself checks each entry against the context's roots.
fn scan_paths_param() -> ToolParam {
    ToolParam {
        name: "scan_paths".into(),
        description: "Comma-separated directories to scan (supports ~). Each must be inside \
                      the caller's allowed roots. Defaults to the configured scan paths"
            .into(),
        param_type: ParamType::String,
        required: false,
        default: None,
    }
}

/// Builds the hygiene config from `params`, checking every caller-supplied
/// `scan_paths` entry against `ctx`'s roots (`write` for tools that delete).
///
/// `scan_paths` is a comma-separated `ParamType::String`, so the executor's
/// `validate_paths`, which only checks `FilePath` params, never saw it: an
/// MCP caller limited to one read root could scan, and with write capability
/// wipe, `target/` directories anywhere. The configured defaults are trusted
/// and stay unchecked; an unrestricted (CLI) context allows every path.
fn parse_hygiene_config(
    params: &serde_json::Value,
    ctx: &ToolContext,
    write: bool,
) -> Result<BuildHygieneConfig, ToolError> {
    let mut config = BuildHygieneConfig::default();
    if let Some(paths) = params.get("scan_paths").and_then(|v| v.as_str()) {
        config.scan_paths = paths.split(',').map(|s| s.trim().to_string()).collect();
        if let Some(refused) = config
            .expanded_scan_paths()
            .into_iter()
            .find(|path| !ctx.is_path_allowed(path, write))
        {
            return Err(ToolError::PathNotAllowed(refused.display().to_string()));
        }
    }
    if let Some(threshold) = params.get("threshold_gb").and_then(|v| v.as_f64()) {
        config.size_threshold_gb = threshold;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context limited to `root` for reads and writes, like a restricted
    /// (non-CLI) caller.
    fn restricted_to(root: &std::path::Path) -> ToolContext {
        let mut ctx = ToolContext::with_all_capabilities();
        ctx.allowed_read_roots = vec![root.to_path_buf()];
        ctx.allowed_write_roots = vec![root.to_path_buf()];
        ctx
    }

    /// A fake Rust project with a `target/` directory under `dir`.
    fn fake_project(dir: &std::path::Path) -> std::path::PathBuf {
        let project = dir.join("victim");
        std::fs::create_dir_all(project.join("target/debug")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"victim\"\n").unwrap();
        std::fs::write(project.join("target/debug/blob"), vec![0u8; 4096]).unwrap();
        project
    }

    /// Path audit: `scan_paths` is a String param, so the executor's
    /// FilePath sandbox never saw it. A restricted caller could scan
    /// outside its roots with build_health and delete with wipe.
    #[tokio::test]
    async fn test_scan_paths_outside_the_roots_are_refused_and_nothing_is_wiped() {
        let allowed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = fake_project(outside.path());
        let ctx = restricted_to(allowed.path());
        let scan = serde_json::json!({
            "scan_paths": outside.path().display().to_string(),
            "dry_run": false,
        });

        for (name, result) in [
            (
                "build_health",
                BuildHealthTool
                    .execute(scan.clone(), &ctx)
                    .await
                    .map(|_| ()),
            ),
            (
                "sweep",
                SweepTool.execute(scan.clone(), &ctx).await.map(|_| ()),
            ),
            (
                "wipe",
                WipeTool.execute(scan.clone(), &ctx).await.map(|_| ()),
            ),
            (
                "clean_all",
                CleanAllTool.execute(scan.clone(), &ctx).await.map(|_| ()),
            ),
        ] {
            assert!(
                matches!(result, Err(ToolError::PathNotAllowed(_))),
                "{name} must refuse a scan path outside its roots, got {result:?}"
            );
        }
        assert!(
            victim.join("target/debug/blob").exists(),
            "nothing outside was deleted"
        );
    }

    #[tokio::test]
    async fn test_scan_paths_inside_the_roots_and_unrestricted_contexts_still_work() {
        let allowed = tempfile::tempdir().unwrap();
        fake_project(allowed.path());
        let inside = serde_json::json!({"scan_paths": allowed.path().display().to_string()});
        let restricted = restricted_to(allowed.path());
        assert!(BuildHealthTool
            .execute(inside.clone(), &restricted)
            .await
            .is_ok());
        // The CLI context has no roots and keeps scanning anywhere.
        let unrestricted = ToolContext::with_all_capabilities();
        assert!(BuildHealthTool.execute(inside, &unrestricted).await.is_ok());
    }

    /// Round 3 (reviewer B): the scans and subprocesses ran on the runtime's
    /// own thread. On a single-threaded runtime, work run inline would hold
    /// the only thread and the quick task would finish after it.
    #[tokio::test(flavor = "current_thread")]
    async fn test_off_runtime_leaves_the_runtime_free() {
        let started = std::time::Instant::now();
        let slow = off_runtime(|| std::thread::sleep(std::time::Duration::from_millis(500)));
        let quick = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            started.elapsed()
        };
        let (slow, quick_elapsed) = tokio::join!(slow, quick);
        slow.unwrap();
        assert!(
            quick_elapsed < std::time::Duration::from_millis(400),
            "the quick task waited {quick_elapsed:?}"
        );
    }

    #[test]
    fn test_build_health_descriptor() {
        let tool = BuildHealthTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "build_health");
        assert_eq!(desc.category, ToolCategory::System);
    }

    #[test]
    fn test_sweep_descriptor() {
        let tool = SweepTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "sweep");
    }

    #[test]
    fn test_tool_availability_descriptor() {
        let tool = ToolAvailabilityTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "tool_availability");
    }

    #[tokio::test]
    async fn test_build_health_execute() {
        let tool = BuildHealthTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(
                serde_json::json!({"scan_paths": "/tmp/nonexistent_xyz"}),
                &ctx,
            )
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_sccache_setup_descriptor() {
        let tool = SccacheSetupTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "sccache_setup");
        assert_eq!(desc.category, ToolCategory::System);
        assert_eq!(desc.params.len(), 1);
    }

    #[tokio::test]
    async fn test_sccache_setup_check_only() {
        let tool = SccacheSetupTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({"check_only": true}), &ctx)
            .await;
        // check_only may fail if sccache not installed — both outcomes valid
        let _ = result;
    }

    #[tokio::test]
    async fn test_sccache_status_execute() {
        let tool = SccacheStatusTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tool_availability_execute() {
        let tool = ToolAvailabilityTool;
        let ctx = ToolContext::with_all_capabilities();
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        let tools = result.output.get("tools").unwrap().as_array().unwrap();
        assert!(tools.len() >= 4);
    }
}
