use std::collections::HashSet;
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::error::ToolError;
use super::traits::{
    Capability, DynamicTool, ParamType, ToolCategory, ToolContext, ToolDescriptor, ToolParam,
    ToolResult,
};

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

        for name in &self.spec.env_allowlist {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        command.env("IMPULSE_HOME", &ctx.impulse_dir);
        if let Some(session_id) = &ctx.session_id {
            command.env("IMPULSE_SESSION_ID", session_id);
        }
        command.env("IMPULSE_EXECUTION_ORIGIN", ctx.execution_origin.as_str());

        let timeout_ms = self.spec.timeout_ms.unwrap_or(ctx.timeout_ms).max(1);
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            command.output(),
        )
        .await
        .map_err(|_| ToolError::Timeout(timeout_ms))?
        .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("external tool exited with status {}", output.status)
            } else {
                stderr
            };
            return Err(ToolError::ExecutionFailed(message));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
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
}
