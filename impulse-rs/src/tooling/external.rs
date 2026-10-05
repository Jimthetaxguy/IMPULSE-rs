//! External-manifest process tools (`ProcessTool`) — commands defined by
//! JSON manifests on disk (e.g. `.impulse/tools.d/*.json`, see
//! `ExternalToolSpec`), loaded via `ToolRegistry::with_runtime()` (the
//! daemon's `InvokeTool` IPC endpoint and the `tooling-run` CLI handler).
//!
//! **Env scrubbing:** `tokio::process::Command::new(&self.spec.command)`
//! inherits the full parent process environment by default, exactly the
//! bug fixed in `src/tooling/builtin/bash_exec.rs` for LLM-triggered shell
//! commands (see that file's module doc). `ProcessTool` is not currently
//! reachable from an LLM tool-calling loop — only from the daemon's
//! `InvokeTool` IPC endpoint (human/GUI-driven, hardcoded tool names) and
//! the `tooling-run` CLI handler (human-typed `--params`) — but the
//! `ExternalToolSpec::env_allowlist` field already signals the original
//! intent to scrub, and this registry is the same one `bash_exec` lives
//! in and that `ion_repl`'s `DynamicToolBridge` can bridge into a chat
//! loop. `execute()` now calls the shared
//! `crate::tooling::env_scrub::scrub_and_allowlist_env` helper (same
//! `.env_clear()` + fixed functional allowlist as `bash_exec`) before
//! re-adding the manifest's own `env_allowlist` entries, instead of
//! relying on full inheritance plus a redundant re-add of already-present
//! vars.
//!
//! **Timeouts and output:** the tool runs in its own process group, and a
//! timed-out or cancelled call kills the group, so nothing it started keeps
//! running (before, the tool itself kept running). A detached group does not
//! receive the terminal's Ctrl-C, so `tooling-run` handles Ctrl-C itself by
//! cancelling the call; a tool that reads the terminal is stopped by the OS
//! and ends at its timeout. Manifest tools get no stdin. Output is read through `tooling::capture`;
//! stdout beyond `MAX_STDOUT_BYTES` fails the call rather than growing this
//! process's memory, since a cut prefix would not parse in JSON mode.

use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::capture::wait_with_capped_output;
use super::env_scrub::scrub_and_allowlist_env;
use super::error::ToolError;
use super::traits::{
    Capability, DynamicTool, ParamType, ToolCategory, ToolContext, ToolDescriptor, ToolParam,
    ToolResult,
};

/// Stdout kept from a manifest tool. Far more than the executor ever returns
/// (`ToolContext::max_output_bytes`, 256 KiB by default), so ordinary output,
/// including pretty-printed JSON that compacts to fit, is unaffected. Kept
/// well below what JSON mode can afford to parse: a parsed tree costs many
/// times its text (16 MB of `[0,0,...]` took about 300 MB).
const MAX_STDOUT_BYTES: usize = 4 * 1024 * 1024;
/// Stderr kept for the error message of a failed call.
const MAX_STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolSource {
    #[default]
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExternalToolOutputMode {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CwdPolicy {
    #[default]
    Current,
    ImpulseDir,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalToolSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub source: ExternalToolSource,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub output_mode: ExternalToolOutputMode,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub cwd_policy: CwdPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestValidationIssue {
    pub file: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestValidationReport {
    pub valid_tools: usize,
    pub invalid_tools: usize,
    pub issues: Vec<ManifestValidationIssue>,
}

impl ExternalToolSpec {
    pub fn validate(&self) -> Result<(), ToolError> {
        if self.id.trim().is_empty() {
            return Err(ToolError::InvalidParams(
                "external tool id cannot be empty".into(),
            ));
        }
        if self.name.trim().is_empty() {
            return Err(ToolError::InvalidParams(
                "external tool name cannot be empty".into(),
            ));
        }
        if self.command.trim().is_empty() {
            return Err(ToolError::InvalidParams(
                "external tool command cannot be empty".into(),
            ));
        }

        let properties = self
            .input_schema
            .get("properties")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ToolError::InvalidParams(
                    "external tool input_schema must contain an object 'properties' map".into(),
                )
            })?;

        for (name, property) in properties {
            let property_type = property
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ToolError::InvalidParams(format!(
                        "input_schema property '{}' is missing a 'type'",
                        name
                    ))
                })?;
            if !["string", "integer", "number", "boolean", "object"].contains(&property_type) {
                return Err(ToolError::InvalidParams(format!(
                    "unsupported input_schema type '{}' for property '{}'",
                    property_type, name
                )));
            }
        }

        Ok(())
    }

    pub fn descriptor(&self) -> Result<ToolDescriptor, ToolError> {
        Ok(ToolDescriptor {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            version: "1.0.0".into(),
            category: ToolCategory::Utility,
            params: schema_to_params(&self.input_schema)?,
        })
    }
}

pub struct ProcessTool {
    spec: ExternalToolSpec,
    descriptor: ToolDescriptor,
}

impl ProcessTool {
    pub fn new(spec: ExternalToolSpec) -> Result<Self, ToolError> {
        spec.validate()?;
        let descriptor = spec.descriptor()?;
        Ok(Self { spec, descriptor })
    }

    pub fn spec(&self) -> &ExternalToolSpec {
        &self.spec
    }

    fn render_args(&self, params: &serde_json::Value) -> Result<Vec<String>, ToolError> {
        self.spec
            .args
            .iter()
            .map(|arg| render_template(arg, params))
            .collect()
    }
}

#[async_trait]
impl DynamicTool for ProcessTool {
    fn id(&self) -> &str {
        &self.spec.id
    }

    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        validate_against_schema(&self.descriptor.params, params)
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let mut command = tokio::process::Command::new(&self.spec.command);
        command.args(self.render_args(&params)?);

        match self.spec.cwd_policy {
            CwdPolicy::Current => {
                if let Ok(cwd) = std::env::current_dir() {
                    command.current_dir(cwd);
                }
            }
            CwdPolicy::ImpulseDir => {
                command.current_dir(&ctx.impulse_dir);
            }
        }

        // Deny-by-default env: drop the full parent environment and re-add
        // only the shared functional allowlist plus this manifest's own
        // declared `env_allowlist` (e.g. a credential the wrapped external
        // tool genuinely needs). See module doc.
        scrub_and_allowlist_env(&mut command, &self.spec.env_allowlist);
        command.env("IMPULSE_HOME", &ctx.impulse_dir);
        if let Some(session_id) = &ctx.session_id {
            command.env("IMPULSE_SESSION_ID", session_id);
        }
        command.env("IMPULSE_EXECUTION_ORIGIN", ctx.execution_origin.as_str());
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        // A timeout or a cancelled call drops `child`; without this the
        // tool kept running after its caller had given up on it.
        command.kill_on_drop(true);
        // Its own process group, so the guard below also reaches anything
        // the tool starts (a wrapper script's children). Keeping CLI tools in
        // the CLI's group instead let a timeout orphan those children.
        #[cfg(unix)]
        {
            command.process_group(0);
        }

        let mut child = command
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mut process_group_guard = crate::process_group::ProcessGroupGuard::new(child.id());

        let timeout_ms = self.spec.timeout_ms.unwrap_or(ctx.timeout_ms).max(1);
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            wait_with_capped_output(&mut child, MAX_STDOUT_BYTES, MAX_STDERR_BYTES),
        )
        .await
        .map_err(|_| ToolError::Timeout(timeout_ms))?
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        process_group_guard.disarm();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr.bytes)
                .trim()
                .to_string();
            let message = if stderr.is_empty() {
                format!("external tool exited with status {}", output.status)
            } else {
                stderr
            };
            return Err(ToolError::ExecutionFailed(message));
        }
        if output.stdout.truncated() {
            return Err(ToolError::ExecutionFailed(format!(
                "external tool wrote {} bytes to stdout, more than the {MAX_STDOUT_BYTES}-byte \
                 limit; its output was discarded",
                output.stdout.total
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout.bytes).to_string();
        let result = match self.spec.output_mode {
            ExternalToolOutputMode::Text => ToolResult::text(stdout.trim_end()),
            ExternalToolOutputMode::Json => {
                let value = serde_json::from_str(stdout.trim()).map_err(ToolError::Json)?;
                ToolResult::json(value)
            }
        };

        Ok(result)
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        self.spec.capabilities.clone()
    }
}

pub fn load_process_tools_from_dir(dir: &Path) -> Result<Vec<ProcessTool>, ToolError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut tools = Vec::new();
    let mut seen = HashSet::new();
    let mut entries = std::fs::read_dir(dir)
        .map_err(ToolError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ToolError::Io)?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(ToolError::Io)?;
        let spec: ExternalToolSpec = serde_json::from_str(&content).map_err(ToolError::Json)?;
        if !seen.insert(spec.id.clone()) {
            return Err(ToolError::AlreadyRegistered(spec.id));
        }
        tools.push(ProcessTool::new(spec)?);
    }

    Ok(tools)
}

pub fn validate_manifests_in_dir(dir: &Path) -> ManifestValidationReport {
    let mut report = ManifestValidationReport::default();

    if !dir.exists() {
        return report;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        report.invalid_tools = 1;
        report.issues.push(ManifestValidationIssue {
            file: dir.display().to_string(),
            error: "failed to read external tools directory".into(),
        });
        return report;
    };

    let mut seen = HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        match std::fs::read_to_string(&path)
            .map_err(ToolError::Io)
            .and_then(|content| {
                serde_json::from_str::<ExternalToolSpec>(&content).map_err(ToolError::Json)
            })
            .and_then(|spec| {
                if !seen.insert(spec.id.clone()) {
                    return Err(ToolError::AlreadyRegistered(spec.id));
                }
                spec.validate()
            }) {
            Ok(()) => report.valid_tools += 1,
            Err(err) => {
                report.invalid_tools += 1;
                report.issues.push(ManifestValidationIssue {
                    file: path.display().to_string(),
                    error: err.to_string(),
                });
            }
        }
    }

    report
}

fn schema_to_params(schema: &serde_json::Value) -> Result<Vec<ToolParam>, ToolError> {
    let properties = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::InvalidParams("input_schema is missing properties".into()))?;
    let required: HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut params = Vec::new();
    for (name, property) in properties {
        let property_type = property
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let format = property.get("format").and_then(|v| v.as_str());
        let param_type = match (property_type, format) {
            ("string", Some("file-path")) => ParamType::FilePath,
            ("string", _) => ParamType::String,
            ("integer", _) => ParamType::Integer,
            ("number", _) => ParamType::Float,
            ("boolean", _) => ParamType::Bool,
            ("object", _) => ParamType::Json,
            _ => {
                return Err(ToolError::InvalidParams(format!(
                    "unsupported input_schema type '{}'",
                    property_type
                )))
            }
        };
        params.push(ToolParam {
            name: name.clone(),
            description: property
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            param_type,
            required: required.contains(name),
            default: property.get("default").cloned(),
        });
    }
    params.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(params)
}

fn validate_against_schema(
    params_spec: &[ToolParam],
    params: &serde_json::Value,
) -> Result<(), ToolError> {
    for param in params_spec {
        let value = params.get(&param.name);
        if param.required && value.is_none() {
            return Err(ToolError::InvalidParams(format!(
                "missing required parameter '{}'",
                param.name
            )));
        }
        let Some(value) = value else { continue };
        match (&param.param_type, value) {
            (ParamType::String | ParamType::FilePath, serde_json::Value::String(_)) => {}
            (ParamType::Integer, serde_json::Value::Number(n)) if n.is_i64() || n.is_u64() => {}
            (ParamType::Float, serde_json::Value::Number(_)) => {}
            (ParamType::Bool, serde_json::Value::Bool(_)) => {}
            (ParamType::Json, serde_json::Value::Object(_)) => {}
            _ => {
                return Err(ToolError::InvalidParams(format!(
                    "parameter '{}' has invalid type",
                    param.name
                )))
            }
        }
    }
    Ok(())
}

fn render_template(template: &str, params: &serde_json::Value) -> Result<String, ToolError> {
    let mut rendered = template.to_string();

    if let Some(object) = params.as_object() {
        for (key, value) in object {
            let placeholder = format!("{{{}}}", key);
            if rendered.contains(&placeholder) {
                rendered = rendered.replace(&placeholder, &value_to_arg(value)?);
            }
        }
    }

    if rendered.contains('{') && rendered.contains('}') {
        return Err(ToolError::InvalidParams(format!(
            "unresolved placeholder in arg template '{}'",
            template
        )));
    }

    Ok(rendered)
}

fn value_to_arg(value: &serde_json::Value) -> Result<String, ToolError> {
    match value {
        serde_json::Value::String(v) => Ok(v.clone()),
        serde_json::Value::Number(v) => Ok(v.to_string()),
        serde_json::Value::Bool(v) => Ok(v.to_string()),
        serde_json::Value::Object(_) => serde_json::to_string(value).map_err(ToolError::Json),
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::Array(_) => Err(ToolError::InvalidParams(
            "array values are not supported in process tool templates".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_descriptor_from_schema() {
        let spec = ExternalToolSpec {
            id: "echo_json".into(),
            name: "Echo JSON".into(),
            description: "Echo".into(),
            source: ExternalToolSource::Process,
            command: "echo".into(),
            args: vec!["{path}".into()],
            env_allowlist: vec![],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "format": "file-path", "description": "Input"}
                },
                "required": ["path"]
            }),
            output_mode: ExternalToolOutputMode::Text,
            capabilities: vec![Capability::FileSystemRead],
            timeout_ms: Some(1000),
            cwd_policy: CwdPolicy::Current,
        };

        let descriptor = spec.descriptor().unwrap();
        assert_eq!(descriptor.params[0].param_type, ParamType::FilePath);
    }

    #[test]
    fn test_validate_manifests_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tool.json"),
            serde_json::to_string_pretty(&ExternalToolSpec {
                id: "echo_json".into(),
                name: "Echo JSON".into(),
                description: "Echo".into(),
                source: ExternalToolSource::Process,
                command: "echo".into(),
                args: vec!["ok".into()],
                env_allowlist: vec![],
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                output_mode: ExternalToolOutputMode::Text,
                capabilities: vec![],
                timeout_ms: None,
                cwd_policy: CwdPolicy::Current,
            })
            .unwrap(),
        )
        .unwrap();

        let report = validate_manifests_in_dir(dir.path());
        assert_eq!(report.valid_tools, 1);
        assert_eq!(report.invalid_tools, 0);
    }

    #[test]
    fn test_validate_manifests_reports_duplicate_ids() {
        let dir = tempfile::tempdir().unwrap();
        let spec = ExternalToolSpec {
            id: "echo_json".into(),
            name: "Echo JSON".into(),
            description: "Echo".into(),
            source: ExternalToolSource::Process,
            command: "echo".into(),
            args: vec!["ok".into()],
            env_allowlist: vec![],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            output_mode: ExternalToolOutputMode::Text,
            capabilities: vec![],
            timeout_ms: None,
            cwd_policy: CwdPolicy::Current,
        };
        std::fs::write(
            dir.path().join("tool-a.json"),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tool-b.json"),
            serde_json::to_string_pretty(&spec).unwrap(),
        )
        .unwrap();

        let report = validate_manifests_in_dir(dir.path());
        assert_eq!(report.valid_tools, 1);
        assert_eq!(report.invalid_tools, 1);
        assert!(report.issues[0].error.contains("already registered"));
    }

    /// Serializes tests in this module that mutate process-global secret
    /// env vars. `cargo test` runs this crate's unit tests in one process
    /// across multiple threads, so a test that sets/removes such a var
    /// must not race a sibling test doing the same — mirrors the pattern
    /// in `builtin/bash_exec.rs`'s test module.
    fn secret_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// RAII guard that sets an env var for the duration of a test and
    /// restores whatever was there before (or removes it if it was unset).
    struct EnvVarGuard {
        name: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var(name).ok();
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn env_dumping_spec(env_allowlist: Vec<String>) -> ExternalToolSpec {
        ExternalToolSpec {
            id: "env_dump".into(),
            name: "Env Dump".into(),
            description: "Dumps the child process environment".into(),
            source: ExternalToolSource::Process,
            command: "sh".into(),
            args: vec!["-c".into(), "env".into()],
            env_allowlist,
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            output_mode: ExternalToolOutputMode::Text,
            capabilities: vec![],
            timeout_ms: Some(5_000),
            cwd_policy: CwdPolicy::Current,
        }
    }

    /// Regression test (mirrors `bash_exec.rs`'s
    /// `test_execute_scrubs_secret_env_vars_from_child_process`): a
    /// `ProcessTool` whose manifest does not declare a secret-shaped name
    /// in `env_allowlist` must not leak the parent `ion`/daemon process's
    /// own secrets into the child's environment (and therefore into
    /// `ToolResult` content) even though `tokio::process::Command`
    /// inherits the full parent environment by default.
    #[tokio::test]
    // clippy: the lock is a test-only std::sync::Mutex<()> and must span
    // the whole `execute().await` call so no sibling test in this file can
    // mutate the same secret env vars mid-spawn.
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_scrubs_secret_env_vars_from_child_process() {
        let _lock = secret_env_lock();
        let _anthropic =
            EnvVarGuard::set("ANTHROPIC_API_KEY", "sk-ant-test-should-not-leak-external");
        let _openai = EnvVarGuard::set("OPENAI_API_KEY", "sk-oai-test-should-not-leak-external");

        let tool = ProcessTool::new(env_dumping_spec(vec![])).unwrap();
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .expect("env should succeed");
        let stdout = result.output.as_str().unwrap();

        assert!(
            !stdout.contains("ANTHROPIC_API_KEY"),
            "child env leaked ANTHROPIC_API_KEY:\n{stdout}"
        );
        assert!(
            !stdout.contains("OPENAI_API_KEY"),
            "child env leaked OPENAI_API_KEY:\n{stdout}"
        );
        assert!(
            !stdout.contains("sk-ant-test-should-not-leak-external"),
            "child env leaked the ANTHROPIC_API_KEY value:\n{stdout}"
        );
        assert!(
            !stdout.contains("sk-oai-test-should-not-leak-external"),
            "child env leaked the OPENAI_API_KEY value:\n{stdout}"
        );
    }

    /// A manifest that explicitly opts a var into `env_allowlist` (the
    /// per-tool credential-passthrough mechanism external tools need, e.g.
    /// a wrapped CLI that itself requires an API key) must still see that
    /// var — the scrub must not silently defeat the manifest's own opt-in.
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_execute_forwards_explicitly_allowlisted_manifest_vars() {
        let _lock = secret_env_lock();
        let _guard = EnvVarGuard::set("EXTERNAL_TOOL_TEST_TOKEN", "forwarded-on-purpose");

        let tool = ProcessTool::new(env_dumping_spec(vec![
            "EXTERNAL_TOOL_TEST_TOKEN".to_string()
        ]))
        .unwrap();
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .expect("env should succeed");
        let stdout = result.output.as_str().unwrap();

        assert!(
            stdout.contains("EXTERNAL_TOOL_TEST_TOKEN=forwarded-on-purpose"),
            "manifest-declared env_allowlist entry must still be forwarded:\n{stdout}"
        );
    }

    /// PATH must survive the scrub so `sh -c` can still resolve the
    /// commands it runs.
    #[tokio::test]
    async fn test_execute_still_has_path_after_scrub() {
        let tool = ProcessTool::new(env_dumping_spec(vec![])).unwrap();
        let ctx = ToolContext::with_all_capabilities();
        let result = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .expect("env should succeed");
        let stdout = result.output.as_str().unwrap();
        assert!(stdout.contains("PATH="), "PATH must survive the scrub");
    }

    fn script_spec(script: &str, timeout_ms: u64) -> ExternalToolSpec {
        ExternalToolSpec {
            id: "script".into(),
            name: "Script".into(),
            description: "Runs a fixed shell script".into(),
            source: ExternalToolSource::Process,
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            env_allowlist: vec![],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            output_mode: ExternalToolOutputMode::Text,
            capabilities: vec![],
            timeout_ms: Some(timeout_ms),
            cwd_policy: CwdPolicy::Current,
        }
    }

    use crate::tooling::ExecutionOrigin;

    fn origin_context(execution_origin: ExecutionOrigin) -> ToolContext {
        ToolContext {
            execution_origin,
            ..ToolContext::with_all_capabilities()
        }
    }

    /// Review P3-9: a timed-out tool was left running, along with anything
    /// it had started (outside the CLI, which detaches it into its own group).
    #[tokio::test]
    #[cfg(unix)]
    async fn test_execute_timeout_kills_the_tool_and_its_children() {
        let pattern = format!(
            "sleep 7.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let tool = ProcessTool::new(script_spec(&format!("{pattern} & wait"), 300)).unwrap();
        let result = tool
            .execute(
                serde_json::json!({}),
                &origin_context(ExecutionOrigin::Daemon),
            )
            .await;
        assert!(matches!(result, Err(ToolError::Timeout(300))), "{result:?}");
        if let Err(stray) = crate::test_support::wait_for_no_matching_process(
            &pattern,
            std::time::Duration::from_secs(2),
        )
        .await
        {
            panic!("a timed-out tool must not keep running; found pids: {stray}");
        }
    }

    /// Verification round on d033932: from the CLI, a timeout orphaned
    /// what a wrapper script had started. Every origin now kills the group.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_cli_timeout_kills_what_the_tool_started() {
        let pattern = format!(
            "sleep 6.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let tool = ProcessTool::new(script_spec(&format!("{pattern} & wait"), 300)).unwrap();
        let result = tool
            .execute(serde_json::json!({}), &origin_context(ExecutionOrigin::Cli))
            .await;
        assert!(matches!(result, Err(ToolError::Timeout(300))), "{result:?}");
        if let Err(stray) = crate::test_support::wait_for_no_matching_process(
            &pattern,
            std::time::Duration::from_secs(2),
        )
        .await
        {
            panic!("a timed-out CLI tool must not leave children; found pids: {stray}");
        }
    }

    /// Review P3-9: stdout was buffered whole, however large.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_execute_refuses_stdout_past_the_cap() {
        let tool = ProcessTool::new(script_spec("head -c 20000000 /dev/zero", 30_000)).unwrap();
        let err = tool
            .execute(serde_json::json!({}), &ToolContext::with_all_capabilities())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed(message) if message.contains("20000000 bytes")),
            "{err}"
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_execute_failure_reports_stderr() {
        let tool = ProcessTool::new(script_spec("echo boom >&2; exit 4", 5_000)).unwrap();
        let err = tool
            .execute(serde_json::json!({}), &ToolContext::with_all_capabilities())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, ToolError::ExecutionFailed(message) if message == "boom"),
            "{err}"
        );
    }
}
