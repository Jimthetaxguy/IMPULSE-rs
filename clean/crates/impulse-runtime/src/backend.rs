//! Backend adapters — the per-platform glue that turns a [`AgentPlatformKind`]
//! into a real CLI subprocess invocation.

use async_trait::async_trait;
use impulse_contracts::{
    AgentPlatformKind, CliSubprocessSpec, HarnessError, HarnessResult, SessionId,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// A backend adapter produces the CLI spec for a given session and inspects
/// the child's output to know when a tool is asking for approval.
#[async_trait]
pub trait BackendAdapter: Send + Sync {
    /// The platform this adapter drives.
    fn kind(&self) -> AgentPlatformKind;

    /// Build the CLI spec to use when spawning a fresh session.
    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec>;

    /// Whether a given stderr chunk indicates the backend is asking for
    /// human approval (e.g. Claude Code's "Allow this action? [Y/n]" prompt).
    fn detects_approval_prompt(&self, stderr_chunk: &str) -> bool;
}

/// Adapter for Anthropic's Claude Code.
#[derive(Clone, Debug, Default)]
pub struct ClaudeCodeAdapter;

#[async_trait]
impl BackendAdapter for ClaudeCodeAdapter {
    fn kind(&self) -> AgentPlatformKind {
        AgentPlatformKind::ClaudeCode
    }

    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec> {
        let mut env = BTreeMap::new();
        env.insert("IMPULSE_SESSION".to_owned(), session.to_string());
        Ok(CliSubprocessSpec {
            program: which_or_default("claude"),
            args: vec![
                "--print".to_owned(),
                "--resume".to_owned(),
                session.to_string(),
            ],
            env,
            working_dir: Some(PathBuf::from(workspace_root)),
        })
    }

    fn detects_approval_prompt(&self, chunk: &str) -> bool {
        chunk.contains("Do you want to proceed?")
            || chunk.contains("Allow this action?")
            || chunk.contains("Press Enter to continue")
    }
}

/// Adapter for OpenAI's Codex CLI.
#[derive(Clone, Debug, Default)]
pub struct CodexAdapter;

#[async_trait]
impl BackendAdapter for CodexAdapter {
    fn kind(&self) -> AgentPlatformKind {
        AgentPlatformKind::Codex
    }

    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec> {
        let mut env = BTreeMap::new();
        env.insert("IMPULSE_SESSION".to_owned(), session.to_string());
        Ok(CliSubprocessSpec {
            program: which_or_default("codex"),
            args: vec![
                "--quiet".to_owned(),
                "resume".to_owned(),
                session.to_string(),
            ],
            env,
            working_dir: Some(PathBuf::from(workspace_root)),
        })
    }

    fn detects_approval_prompt(&self, chunk: &str) -> bool {
        chunk.contains("Approve? [y/N]") || chunk.contains("Do you want Codex to")
    }
}

/// Adapter for Google's Gemini CLI.
#[derive(Clone, Debug, Default)]
pub struct GeminiCliAdapter;

#[async_trait]
impl BackendAdapter for GeminiCliAdapter {
    fn kind(&self) -> AgentPlatformKind {
        AgentPlatformKind::GeminiCli
    }

    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec> {
        let mut env = BTreeMap::new();
        env.insert("IMPULSE_SESSION".to_owned(), session.to_string());
        Ok(CliSubprocessSpec {
            program: which_or_default("gemini"),
            args: vec!["--resume".to_owned(), session.to_string()],
            env,
            working_dir: Some(PathBuf::from(workspace_root)),
        })
    }

    fn detects_approval_prompt(&self, chunk: &str) -> bool {
        chunk.contains("Allow? [y/n]") || chunk.contains("Confirm execution?")
    }
}

/// Adapter for the legacy OpenCode CLI.
#[derive(Clone, Debug, Default)]
pub struct OpenCodeAdapter;

#[async_trait]
impl BackendAdapter for OpenCodeAdapter {
    fn kind(&self) -> AgentPlatformKind {
        AgentPlatformKind::OpenCode
    }

    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec> {
        let mut env = BTreeMap::new();
        env.insert("IMPULSE_SESSION".to_owned(), session.to_string());
        Ok(CliSubprocessSpec {
            program: which_or_default("opencode"),
            args: vec!["--session".to_owned(), session.to_string()],
            env,
            working_dir: Some(PathBuf::from(workspace_root)),
        })
    }

    fn detects_approval_prompt(&self, chunk: &str) -> bool {
        // OpenCode had multiple variants; be permissive.
        chunk.contains("approve") || chunk.contains("Allow")
    }
}

/// Adapter for arbitrary CLI subprocesses. The orchestrator treats the
/// child as opaque — it captures output but does not parse it.
#[derive(Clone, Debug, Default)]
pub struct GenericCliAdapter {
    program: String,
    args: Vec<String>,
}

impl GenericCliAdapter {
    /// Create a new generic adapter for `program` with the given default args.
    #[must_use]
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

#[async_trait]
impl BackendAdapter for GenericCliAdapter {
    fn kind(&self) -> AgentPlatformKind {
        AgentPlatformKind::GenericCli
    }

    fn cli_for(
        &self,
        session: SessionId,
        workspace_root: &std::path::Path,
    ) -> HarnessResult<CliSubprocessSpec> {
        let mut env = BTreeMap::new();
        env.insert("IMPULSE_SESSION".to_owned(), session.to_string());
        Ok(CliSubprocessSpec {
            program: self.program.clone(),
            args: self.args.clone(),
            env,
            working_dir: Some(PathBuf::from(workspace_root)),
        })
    }

    fn detects_approval_prompt(&self, _chunk: &str) -> bool {
        // Generic backends don't have a known approval protocol; let the
        // user decide.
        false
    }
}

/// Look up `program` on `PATH`; fall back to the literal name if not found.
///
/// We don't fail here — the spawn will fail later with a clearer error. This
/// keeps the adapter deterministic in tests where `which` may not find the
/// real binary.
fn which_or_default(program: &str) -> String {
    which::which(program)
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| program.to_owned())
}

/// Factory: pick the right adapter for a platform.
#[must_use]
pub fn default_adapter_for(kind: AgentPlatformKind) -> Arc<dyn BackendAdapter> {
    match kind {
        AgentPlatformKind::ClaudeCode => Arc::new(ClaudeCodeAdapter),
        AgentPlatformKind::Codex => Arc::new(CodexAdapter),
        AgentPlatformKind::GeminiCli => Arc::new(GeminiCliAdapter),
        AgentPlatformKind::OpenCode => Arc::new(OpenCodeAdapter),
        AgentPlatformKind::GenericCli => Arc::new(GenericCliAdapter::new("", Vec::new())),
    }
}

/// Look up a binary on `PATH` and return a [`HarnessError::BinaryNotFound`] if it's missing.
pub fn require_binary(kind: AgentPlatformKind, program: &str) -> HarnessResult<()> {
    if program.is_empty() {
        return Err(HarnessError::BinaryNotFound {
            platform: kind,
            program: "<empty>".to_owned(),
        });
    }
    if which::which(program).is_err() {
        return Err(HarnessError::BinaryNotFound {
            platform: kind,
            program: program.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_adapter_reports_its_kind() {
        assert_eq!(ClaudeCodeAdapter.kind(), AgentPlatformKind::ClaudeCode);
        assert_eq!(CodexAdapter.kind(), AgentPlatformKind::Codex);
        assert_eq!(GeminiCliAdapter.kind(), AgentPlatformKind::GeminiCli);
        assert_eq!(OpenCodeAdapter.kind(), AgentPlatformKind::OpenCode);
        assert_eq!(
            GenericCliAdapter::new("echo", vec![]).kind(),
            AgentPlatformKind::GenericCli
        );
    }

    #[test]
    fn adapters_emit_non_empty_cli_spec() {
        let root = std::path::Path::new("/tmp");
        let sid = SessionId::new();
        let adapters: Vec<Box<dyn BackendAdapter>> = vec![
            Box::new(ClaudeCodeAdapter),
            Box::new(CodexAdapter),
            Box::new(GeminiCliAdapter),
            Box::new(OpenCodeAdapter),
            Box::new(GenericCliAdapter::new("echo", vec![])),
        ];
        for adapter in &adapters {
            let spec = adapter.cli_for(sid, root).expect("spec");
            assert!(spec.working_dir.is_some());
            assert_eq!(
                spec.env.get("IMPULSE_SESSION").map(String::as_str),
                Some(sid.to_string().as_str())
            );
        }
    }

    #[test]
    fn adapters_detect_approval_prompts() {
        assert!(ClaudeCodeAdapter.detects_approval_prompt("Do you want to proceed?"));
        assert!(!ClaudeCodeAdapter.detects_approval_prompt("Build complete"));
        assert!(CodexAdapter.detects_approval_prompt("Approve? [y/N]"));
        assert!(GeminiCliAdapter.detects_approval_prompt("Allow? [y/n]"));
    }

    #[test]
    fn require_binary_rejects_empty() {
        assert!(require_binary(AgentPlatformKind::GenericCli, "").is_err());
    }

    #[test]
    fn require_binary_finds_existing() {
        // `sh` is universal on unix; skip on non-unix.
        #[cfg(unix)]
        assert!(require_binary(AgentPlatformKind::GenericCli, "sh").is_ok());
    }
}
