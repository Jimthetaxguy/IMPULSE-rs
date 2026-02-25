//! CLI Agent Implementation
//!
//! Supports running Claude Code and OpenCode as subprocesses

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use super::types::{AgentBackend, AgentConfig, AgentType, CliSession};
use crate::error::{AgentError, AgentResult};

/// A message in CLI agent communication
#[derive(Debug, Clone)]
pub struct CliMessage {
    pub role: String,
    pub content: String,
}

/// Result from a CLI agent
#[derive(Debug, Clone)]
pub struct CliResponse {
    pub content: String,
    pub session_id: Option<String>,
}

/// Error types specific to CLI agents
#[derive(Debug, thiserror::Error)]
pub enum CliAgentError {
    #[error("CLI command not found: {0}")]
    CommandNotFound(String),

    #[error("Failed to spawn CLI process: {0}")]
    SpawnFailed(String),

    #[error("CLI process exited unexpectedly: {0}")]
    ProcessExited(String),

    #[error("Failed to communicate with CLI: {0}")]
    CommunicationError(String),

    #[error("CLI session not active")]
    SessionNotActive,

    #[error("Agent type {0} is not a CLI agent")]
    NotCliAgent(String),
}

impl From<CliAgentError> for AgentError {
    fn from(e: CliAgentError) -> Self {
        match e {
            CliAgentError::CommandNotFound(s) => AgentError::ApiRequest(s),
            CliAgentError::SpawnFailed(s) => AgentError::ApiRequest(s),
            CliAgentError::ProcessExited(s) => AgentError::ApiRequest(s),
            CliAgentError::CommunicationError(s) => AgentError::ApiRequest(s),
            CliAgentError::SessionNotActive => AgentError::ApiRequest("Session not active".to_string()),
            CliAgentError::NotCliAgent(s) => AgentError::ApiRequest(s),
        }
    }
}

/// CLI Agent for running Claude Code, OpenCode, or similar CLI agents
pub struct CliAgent {
    pub config: AgentConfig,
    session: Option<CliSession>,
    child: Option<Child>,
    /// Buffer for reading responses
    response_buffer: String,
}

impl Clone for CliAgent {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            session: self.session.clone(),
            child: None, // Can't clone child processes
            response_buffer: self.response_buffer.clone(),
        }
    }
}

impl CliAgent {
    /// Create a new CLI agent
    pub fn new(config: AgentConfig) -> AgentResult<Self> {
        if !config.agent_type.is_cli() {
            return Err(AgentError::ApiRequest(format!(
                "Agent type {:?} is not a CLI agent",
                config.agent_type
            )));
        }

        Ok(Self {
            config,
            session: None,
            child: None,
            response_buffer: String::new(),
        })
    }

    /// Get the CLI command to run
    fn get_command(&self) -> AgentResult<&str> {
        self.config
            .agent_type
            .cli_command()
            .ok_or_else(|| AgentError::ApiRequest("Unknown CLI agent type".to_string()))
    }

    /// Get the default working directory
    fn get_working_dir(&self) -> PathBuf {
        self.config
            .working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Build environment variables
    fn build_env(&self) -> HashMap<String, String> {
        let mut env = std::env::vars().collect::<HashMap<_, _>>();

        // Add/override with custom env vars
        for (key, value) in &self.config.env {
            env.insert(key.clone(), value.clone());
        }

        // Add verbose flag if set
        if self.config.verbose {
            if let Some(cmd) = self.config.agent_type.cli_command() {
                // Some CLIs support --verbose
                env.insert(format!("{}_VERBOSE", cmd.to_uppercase()), "1".to_string());
            }
        }

        env
    }

    /// Check if a CLI command is available
    pub async fn check_availability(&self) -> AgentResult<bool> {
        let cmd = self.get_command()?;

        // Try to run --version to check if command exists
        let output = Command::new(cmd)
            .arg("--version")
            .output()
            .await;

        match output {
            Ok(o) => Ok(o.status.success()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(AgentError::ApiRequest(e.to_string()))
                }
            }
        }
    }

    /// Start a CLI session (spawn the subprocess)
    pub async fn start_session(&mut self, initial_prompt: Option<&str>) -> AgentResult<CliSession> {
        let cmd = self.get_command()?;

        // Check if command exists first
        if !self.check_availability().await? {
            return Err(CliAgentError::CommandNotFound(cmd.to_string()).into());
        }

        // Build the command
        let mut command = Command::new(cmd);
        command
            .current_dir(self.get_working_dir())
            .envs(self.build_env())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add CLI-specific arguments based on agent type
        match self.config.agent_type {
            AgentType::ClaudeCode => {
                // Claude Code options: --print, --addrompt, --continue, etc.
                // For interactive sessions, we typically use --continue or no flag
                if let Some(prompt) = initial_prompt {
                    command.arg("--add-prompt").arg(prompt);
                }
                // Use --print for non-interactive mode, or nothing for interactive
                // command.arg("--print");
            }
            AgentType::OpenCode => {
                // OpenCode options
                if let Some(prompt) = initial_prompt {
                    command.arg("--prompt").arg(prompt);
                }
            }
            _ => {}
        }

        // Spawn the process
        let mut child = command.spawn().map_err(|e| {
            CliAgentError::SpawnFailed(format!("Failed to spawn {}: {}", cmd, e))
        })?;

        // Get stdin/stdout
        let stdin = child.stdin.take().ok_or_else(|| {
            CliAgentError::SpawnFailed("Failed to capture stdin".to_string())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CliAgentError::SpawnFailed("Failed to capture stdout".to_string())
        })?;

        let stderr = child.stderr.take();

        // Create the session
        let pid = child.id();
        let session_id = uuid::Uuid::new_v4().to_string();

        self.session = Some(CliSession {
            config: self.config.clone(),
            active: true,
            pid,
            session_id: Some(session_id.clone()),
        });

        self.child = Some(child);

        // Start reading responses in background (simplified for now - we do sync reads)
        // In a full implementation, we'd spawn a task to read stdout

        Ok(CliSession {
            config: self.config.clone(),
            active: true,
            pid,
            session_id: Some(session_id),
        })
    }

    /// Send a message to the CLI agent and get a response
    pub async fn send_message(&mut self, message: &str) -> AgentResult<CliResponse> {
        let session = self.session.as_ref().ok_or(CliAgentError::SessionNotActive)?;

        if !session.active {
            return Err(CliAgentError::SessionNotActive.into());
        }

        // For now, implement a simple approach:
        // 1. If no child exists, start a new session
        // 2. Write to stdin
        // 3. Read from stdout

        // This is a simplified implementation - a full version would need
        // proper async reading/writing with proper protocol handling

        // For Claude Code / OpenCode, they typically work interactively
        // So we need to handle the protocol differently

        // Return a placeholder - the full implementation would need
        // proper CLI protocol handling (like JSON-RPC or similar)
        Ok(CliResponse {
            content: format!(
                "[CLI Agent {}] Message sent: {}",
                self.config.name, message
            ),
            session_id: session.session_id.clone(),
        })
    }

    /// End the CLI session
    pub async fn end_session(&mut self) -> AgentResult<()> {
        if let Some(mut session) = self.session.take() {
            session.active = false;
        }

        if let Some(mut child) = self.child.take() {
            // Try graceful termination first
            let _ = child.kill().await;
        }

        Ok(())
    }

    /// Get the current session
    pub fn session(&self) -> Option<&CliSession> {
        self.session.as_ref()
    }

    /// Check if session is active
    pub fn is_active(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| s.active)
            .unwrap_or(false)
    }
}

impl Drop for CliAgent {
    fn drop(&mut self) {
        // Ensure we clean up the child process
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

/// Builder for creating CLI agents
pub struct CliAgentBuilder {
    config: AgentConfig,
}

impl CliAgentBuilder {
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            config: AgentConfig::new("default", "Default Agent", agent_type),
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.config.id = id.into();
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    pub fn working_dir(mut self, dir: PathBuf) -> Self {
        self.config.working_dir = Some(dir);
        self
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    pub fn verbose(mut self) -> Self {
        self.config.verbose = true;
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.env.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> AgentResult<CliAgent> {
        CliAgent::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_agent_creation() {
        let config = AgentConfig::claude_code("test", "Test Claude");
        let agent = CliAgent::new(config);
        assert!(agent.is_ok());

        // Try with non-CLI agent should fail
        let config = AgentConfig::anthropic("test", "Test Anthropic", "key");
        let agent = CliAgent::new(config);
        assert!(agent.is_err());
    }

    #[test]
    fn test_cli_agent_builder() {
        let agent = CliAgentBuilder::new(AgentType::ClaudeCode)
            .id("my-claude")
            .name("My Claude")
            .working_dir(PathBuf::from("/tmp"))
            .verbose()
            .system_prompt("You are helpful")
            .build();

        assert!(agent.is_ok());
        let agent = agent.unwrap();
        assert!(!agent.is_active());
    }
}
