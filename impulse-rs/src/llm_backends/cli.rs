//! CLI Agent Implementation
//!
//! Supports running Claude Code, OpenCode, and generic CLI agents as subprocesses.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use super::types::{AgentConfig, AgentType, CliProtocol, CliSession};
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
            CliAgentError::CommandNotFound(s) => AgentError::InvalidRequest(s),
            CliAgentError::SpawnFailed(s) => AgentError::InvalidRequest(s),
            CliAgentError::NotCliAgent(s) => AgentError::InvalidRequest(s),
            CliAgentError::ProcessExited(s) => AgentError::ApiRequest(s),
            CliAgentError::CommunicationError(s) => AgentError::ApiRequest(s),
            CliAgentError::SessionNotActive => {
                AgentError::SessionNotFound { id: "cli".to_string() }
            }
        }
    }
}

struct CliRuntimeState {
    session: Option<CliSession>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    response_buffer: String,
}

impl CliRuntimeState {
    fn new() -> Self {
        Self {
            session: None,
            child: None,
            stdin: None,
            stdout: None,
            response_buffer: String::new(),
        }
    }

    fn session_id(&self) -> Option<String> {
        self.session.as_ref().and_then(|session| session.session_id.clone())
    }
}

/// CLI Agent for running Claude Code, OpenCode, or similar CLI agents
pub struct CliAgent {
    pub config: AgentConfig,
    state: Arc<Mutex<CliRuntimeState>>,
}

impl Clone for CliAgent {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
        }
    }
}

impl CliAgent {
    /// Create a new CLI agent
    pub fn new(config: AgentConfig) -> AgentResult<Self> {
        if !config.agent_type.is_cli() {
            return Err(CliAgentError::NotCliAgent(format!("{:?}", config.agent_type)).into());
        }
        if matches!(config.agent_type, AgentType::GenericCli)
            && config
                .cli_command
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            return Err(CliAgentError::NotCliAgent(
                "generic-cli requires an explicit cli_command".to_string(),
            )
            .into());
        }

        Ok(Self {
            config,
            state: Arc::new(Mutex::new(CliRuntimeState::new())),
        })
    }

    /// Get the CLI command to run
    fn get_command(&self) -> AgentResult<String> {
        self.config
            .cli_command
            .clone()
            .or_else(|| self.config.agent_type.cli_command().map(str::to_string))
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

        for (key, value) in &self.config.env {
            env.insert(key.clone(), value.clone());
        }

        env.insert(
            "IMPULSE_CLI_PROTOCOL".to_string(),
            match self.config.cli_protocol {
                CliProtocol::PromptOnce => "prompt-once",
                CliProtocol::LineDelimited => "line-delimited",
                CliProtocol::JsonLines => "json-lines",
            }
            .to_string(),
        );

        if self.config.verbose {
            if let Some(cmd) = self.config.agent_type.cli_command() {
                env.insert(format!("{}_VERBOSE", cmd.to_uppercase()), "1".to_string());
            }
        }

        env
    }

    fn ensure_session(state: &mut CliRuntimeState, config: &AgentConfig) {
        if state.session.is_none() {
            state.session = Some(CliSession {
                config: config.clone(),
                active: true,
                pid: None,
                session_id: Some(uuid::Uuid::new_v4().to_string()),
            });
            return;
        }

        if let Some(session) = &mut state.session {
            session.active = true;
        }
    }

    fn build_command(&self) -> AgentResult<Command> {
        let cmd = self.get_command()?;
        let mut command = Command::new(cmd);
        command
            .current_dir(self.get_working_dir())
            .envs(self.build_env())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(command)
    }

    fn render_static_args(&self, prompt: Option<&str>) -> Vec<String> {
        let mut args = Vec::new();
        let mut prompt_consumed = false;
        let system_prompt = self.config.system_prompt.as_deref().unwrap_or("");

        for arg in &self.config.cli_args {
            let mut rendered = arg.replace("{system_prompt}", system_prompt);
            if let Some(prompt) = prompt {
                if rendered.contains("{prompt}") {
                    rendered = rendered.replace("{prompt}", prompt);
                    prompt_consumed = true;
                }
            }
            args.push(rendered);
        }

        match self.config.agent_type {
            AgentType::ClaudeCode => {
                if matches!(self.config.cli_protocol, CliProtocol::PromptOnce) {
                    args.push("--print".to_string());
                }
            }
            AgentType::OpenCode => {}
            AgentType::GenericCli => {}
            AgentType::Anthropic | AgentType::OpenAi | AgentType::Minimax | AgentType::Custom => {}
        }

        if let Some(prompt) = prompt {
            match self.config.agent_type {
                AgentType::ClaudeCode | AgentType::GenericCli => {
                    if !prompt_consumed {
                        args.push(prompt.to_string());
                    }
                }
                AgentType::OpenCode => {
                    if !prompt_consumed {
                        args.push("--prompt".to_string());
                        args.push(prompt.to_string());
                    }
                }
                AgentType::Anthropic
                | AgentType::OpenAi
                | AgentType::Minimax
                | AgentType::Custom => {}
            }
        }

        args
    }

    /// Check if a CLI command is available
    pub async fn check_availability(&self) -> AgentResult<bool> {
        let cmd = self.get_command()?;

        match Command::new(&cmd).arg("--version").output().await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(AgentError::ApiRequest(e.to_string())),
        }
    }

    async fn spawn_persistent_process(&self) -> AgentResult<(Child, ChildStdin, BufReader<ChildStdout>)> {
        let mut command = self.build_command()?;
        command.args(self.render_static_args(None));

        let mut child = command
            .spawn()
            .map_err(|e| CliAgentError::SpawnFailed(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CliAgentError::SpawnFailed("Failed to capture stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CliAgentError::SpawnFailed("Failed to capture stdout".to_string()))?;

        Ok((child, stdin, BufReader::new(stdout)))
    }

    async fn ensure_persistent_session(
        &self,
        state: &mut CliRuntimeState,
    ) -> AgentResult<()> {
        let needs_spawn = match state.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        };

        if !needs_spawn && state.stdin.is_some() && state.stdout.is_some() {
            return Ok(());
        }

        let (child, stdin, stdout) = self.spawn_persistent_process().await?;
        let pid = child.id();
        Self::ensure_session(state, &self.config);
        if let Some(session) = &mut state.session {
            session.pid = pid;
        }
        state.child = Some(child);
        state.stdin = Some(stdin);
        state.stdout = Some(stdout);
        Ok(())
    }

    async fn read_line_delimited_response(
        stdout: &mut BufReader<ChildStdout>,
        response_buffer: &mut String,
    ) -> AgentResult<Vec<String>> {
        let mut lines = Vec::new();

        loop {
            let mut line = String::new();
            let timeout = if lines.is_empty() {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(250)
            };

            match tokio::time::timeout(timeout, stdout.read_line(&mut line)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        if !lines.is_empty() {
                            break;
                        }
                        continue;
                    }
                    response_buffer.push_str(trimmed);
                    response_buffer.push('\n');
                    lines.push(trimmed.to_string());
                }
                Ok(Err(err)) => {
                    return Err(
                        CliAgentError::CommunicationError(format!("failed to read response: {}", err))
                            .into(),
                    )
                }
                Err(_) if lines.is_empty() => {
                    return Err(CliAgentError::CommunicationError(
                        "timed out waiting for CLI response".to_string(),
                    )
                    .into())
                }
                Err(_) => break,
            }
        }

        if lines.is_empty() {
            return Err(CliAgentError::CommunicationError(
                "CLI returned no response".to_string(),
            )
            .into());
        }

        Ok(lines)
    }

    fn parse_json_lines(lines: &[String]) -> AgentResult<String> {
        let mut fragments = Vec::new();

        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line)
                .map_err(|err| CliAgentError::CommunicationError(err.to_string()))?;

            if let Some(text) = value.get("content").and_then(|item| item.as_str()) {
                fragments.push(text.to_string());
                continue;
            }
            if let Some(text) = value.get("text").and_then(|item| item.as_str()) {
                fragments.push(text.to_string());
                continue;
            }
            if let Some(text) = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(|item| item.as_str())
            {
                fragments.push(text.to_string());
                continue;
            }
            if let Some(text) = value
                .get("result")
                .and_then(|result| result.get("content"))
                .and_then(|item| item.as_str())
            {
                fragments.push(text.to_string());
                continue;
            }

            fragments.push(value.to_string());
        }

        Ok(fragments.join("\n"))
    }

    async fn send_via_persistent_session(
        &self,
        state: &mut CliRuntimeState,
        message: &str,
        json_mode: bool,
    ) -> AgentResult<String> {
        self.ensure_persistent_session(state).await?;

        let stdin = state
            .stdin
            .as_mut()
            .ok_or(CliAgentError::SessionNotActive)?;
        stdin
            .write_all(message.as_bytes())
            .await
            .map_err(|e| CliAgentError::CommunicationError(e.to_string()))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| CliAgentError::CommunicationError(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| CliAgentError::CommunicationError(e.to_string()))?;

        let stdout = state
            .stdout
            .as_mut()
            .ok_or(CliAgentError::SessionNotActive)?;
        let lines =
            Self::read_line_delimited_response(stdout, &mut state.response_buffer).await?;

        if json_mode {
            Self::parse_json_lines(&lines)
        } else {
            Ok(lines.join("\n"))
        }
    }

    async fn run_prompt_once(&self, state: &mut CliRuntimeState, message: &str) -> AgentResult<String> {
        Self::ensure_session(state, &self.config);

        let mut command = self.build_command()?;
        command.args(self.render_static_args(Some(message)));

        let output = command
            .output()
            .await
            .map_err(|e| CliAgentError::SpawnFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(CliAgentError::ProcessExited(detail).into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Start a CLI session
    pub async fn start_session(&self, initial_prompt: Option<&str>) -> AgentResult<CliSession> {
        let mut state = self.state.lock().await;
        Self::ensure_session(&mut state, &self.config);

        match self.config.cli_protocol {
            CliProtocol::PromptOnce => {
                if let Some(prompt) = initial_prompt {
                    let _ = self.run_prompt_once(&mut state, prompt).await?;
                }
            }
            CliProtocol::LineDelimited | CliProtocol::JsonLines => {
                self.ensure_persistent_session(&mut state).await?;
                if let Some(prompt) = initial_prompt {
                    let _ = self
                        .send_via_persistent_session(
                            &mut state,
                            prompt,
                            matches!(self.config.cli_protocol, CliProtocol::JsonLines),
                        )
                        .await?;
                }
            }
        }

        state.session.clone().ok_or(CliAgentError::SessionNotActive.into())
    }

    /// Send a message to the CLI agent and get a response
    pub async fn send_message(&self, message: &str) -> AgentResult<CliResponse> {
        let mut state = self.state.lock().await;
        let content = match self.config.cli_protocol {
            CliProtocol::PromptOnce => self.run_prompt_once(&mut state, message).await?,
            CliProtocol::LineDelimited => {
                self.send_via_persistent_session(&mut state, message, false)
                    .await?
            }
            CliProtocol::JsonLines => {
                self.send_via_persistent_session(&mut state, message, true)
                    .await?
            }
        };

        Ok(CliResponse {
            content,
            session_id: state.session_id(),
        })
    }

    /// End the CLI session
    pub async fn end_session(&self) -> AgentResult<()> {
        let mut state = self.state.lock().await;
        if let Some(session) = &mut state.session {
            session.active = false;
        }
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
        }
        state.stdin = None;
        state.stdout = None;
        Ok(())
    }

    /// Get the current session snapshot
    pub fn session(&self) -> Option<CliSession> {
        self.state
            .try_lock()
            .ok()
            .and_then(|state| state.session.clone())
    }

    /// Check if session is active
    pub fn is_active(&self) -> bool {
        self.state
            .try_lock()
            .ok()
            .and_then(|state| state.session.as_ref().map(|session| session.active))
            .unwrap_or(false)
    }
}

impl Drop for CliAgent {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(child) = state.child.as_mut() {
                let _ = child.start_kill();
            }
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

    pub fn command(mut self, command: impl Into<String>) -> Self {
        self.config.cli_command = Some(command.into());
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.config.cli_args.push(arg.into());
        self
    }

    pub fn protocol(mut self, protocol: CliProtocol) -> Self {
        self.config.cli_protocol = protocol;
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

    #[test]
    fn test_generic_cli_builder() {
        let agent = CliAgentBuilder::new(AgentType::GenericCli)
            .id("generic")
            .name("Generic")
            .command("echo")
            .arg("{prompt}")
            .protocol(CliProtocol::PromptOnce)
            .build();

        assert!(agent.is_ok());
    }

    #[test]
    fn test_parse_json_lines_content() {
        let parsed = CliAgent::parse_json_lines(&[
            r#"{"content":"hello"}"#.to_string(),
            r#"{"text":"world"}"#.to_string(),
        ])
        .unwrap();
        assert_eq!(parsed, "hello\nworld");
    }
}
