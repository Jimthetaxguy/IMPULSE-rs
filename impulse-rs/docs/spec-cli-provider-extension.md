# CLI Provider Extension — Impulse Spec

> **Date:** 2026-03-02
> **Status:** Draft
> **Companion:** Private local design note, not included in this public repository.
> **Source:** NullClaw analysis §2.6, existing Impulse `CliAgent` stub

---

## 1. Current State

### Existing Types (`src/llm_backends/types.rs`)

```rust
// Line 11 — Backend discriminator
pub enum AgentBackend {
    Api,   // Direct API call
    Cli,   // Spawn CLI subprocess
}

// Line 27 — Agent type (determines CLI command)
pub enum AgentType {
    ClaudeCode,  // cli_command() → "claude"
    OpenCode,    // cli_command() → "opencode"
    Anthropic,   // API only
    OpenAi,      // API only
    Minimax,     // API only
    Custom,      // API only
}

// AgentConfig — full builder pattern with:
//   id, name, agent_type, backend, model, api_key, api_endpoint,
//   working_dir, env, system_prompt, verbose
```

### CliAgent (`src/llm_backends/cli.rs:65`)

```rust
pub struct CliAgent {
    pub config: AgentConfig,
    session: Option<CliSession>,
    child: Option<Child>,           // tokio::process::Child
    response_buffer: String,
}
```

**What works:**
- `check_availability()` — runs `--version` to verify CLI exists
- `start_session()` — spawns subprocess with stdin/stdout/stderr piped
- Builder pattern (`CliAgentBuilder`)
- Process lifecycle (spawn, kill on drop)

**What's incomplete:**
- `send_message()` at line 239 — returns placeholder string:
  ```rust
  Ok(CliResponse {
      content: format!("[CLI Agent {}] Message sent: {}", self.config.name, message),
      session_id: session.session_id.clone(),
  })
  ```
- No output parsing (stdout is captured but never read)
- No streaming support
- No structured protocol (JSON vs plain text)
- No custom CLI support (only ClaudeCode and OpenCode hardcoded)

### UnifiedAgent (`src/llm_backends/factory.rs`)

```rust
// Line 15
pub trait UnifiedAgent: Send + Sync {
    fn name(&self) -> &str;
    fn id(&self) -> &str;
    fn agent_type(&self) -> AgentType;
    async fn is_available(&self) -> bool;
    async fn send(&self, message: &str) -> AgentResult<String>;
}
```

`CliUnifiedAgent.send()` returns an error:
```rust
Err(AgentError::ApiRequest(
    "CLI agent requires session management - use AgentManager instead".to_string(),
))
```

### LlmProvider (`src/llm_backends/mod.rs:50`)

```rust
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> AgentResult<ChatResponse>;
    fn supported_models(&self) -> Vec<&str>;
}
```

Currently only implemented for API providers (Anthropic, OpenAI, Minimax). No CLI
provider implements `LlmProvider`.

---

## 2. Proposed: CliProtocol Trait

```rust
/// Defines how to communicate with a specific CLI tool
pub trait CliProtocol: Send + Sync {
    /// Human-readable protocol name
    fn name(&self) -> &str;

    /// Build the Command with appropriate args for sending a prompt
    fn build_command(
        &self,
        config: &AgentConfig,
        prompt: &str,
        system_prompt: Option<&str>,
    ) -> Command;

    /// Parse a completed response from stdout
    fn parse_response(&self, stdout: &str, stderr: &str) -> Result<CliResponse>;

    /// Whether this protocol supports streaming
    fn supports_streaming(&self) -> bool;

    /// Parse a single streaming chunk (for streaming protocols)
    fn parse_stream_chunk(&self, line: &str) -> Option<StreamChunk>;
}

#[derive(Debug, Clone)]
pub enum StreamChunk {
    Text(String),          // Partial response text
    Done(CliResponse),     // Final response with metadata
    Error(String),         // Stream error
}
```

---

## 3. Protocol Implementations

```
  src/llm_backends/protocols/
  ├── mod.rs             # Re-exports + CliProtocol trait
  ├── claude_code.rs     # claude --print --output-format json
  ├── opencode.rs        # opencode --prompt
  ├── ollama.rs          # ollama run <model>
  └── generic.rs         # Configurable: stdin JSON → stdout JSON
```

### Claude Code Protocol

`ClaudeCodeProtocol` builds: `claude --print --output-format json [--system-prompt SP]
[--model M] PROMPT`. Parses JSON response extracting `result` field. Streaming parses
NDJSON lines with `type: "assistant"` messages.

### Generic Protocol (User-configurable)

```rust
pub struct GenericCliProtocol {
    pub command: String,
    pub args_template: Vec<String>,       // "{prompt}" placeholder
    pub input_mode: InputMode,            // Positional | Flag | Stdin
    pub output_format: OutputFormat,      // Json { content_path } | Text
}
```

---

## 4. Updated CliAgent

The key change: inject `CliProtocol` into `CliAgent` so `send_message()` actually works.

```rust
pub struct CliAgent {
    pub config: AgentConfig,
    protocol: Box<dyn CliProtocol>,  // NEW — injected protocol
    session: Option<CliSession>,
    child: Option<Child>,
    response_buffer: String,
}

impl CliAgent {
    pub fn new(config: AgentConfig, protocol: Box<dyn CliProtocol>) -> AgentResult<Self> {
        // ...
    }

    pub async fn send_message(&mut self, message: &str) -> AgentResult<CliResponse> {
        // 1. Build command via protocol
        let mut cmd = self.protocol.build_command(
            &self.config,
            message,
            self.config.system_prompt.as_deref(),
        );
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // 2. Spawn and wait
        let child = cmd.spawn().map_err(|e|
            AgentError::ApiRequest(format!("Failed to spawn: {}", e))
        )?;
        let output = child.wait_with_output().await?;

        // 3. Parse via protocol
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(AgentError::ApiRequest(
                format!("CLI exited {}: {}", output.status, stderr)
            ));
        }

        self.protocol.parse_response(&stdout, &stderr)
    }
}
```

---

## 5. Updated AgentType Enum

```rust
pub enum AgentType {
    ClaudeCode,   // Existing
    OpenCode,     // Existing
    Anthropic,    // Existing
    OpenAi,       // Existing
    Minimax,      // Existing
    Custom,       // Existing (API)
    Ollama,       // NEW — local models via ollama CLI
    GenericCli,   // NEW — user-configured CLI tool
}

impl AgentType {
    pub fn cli_command(&self) -> Option<&'static str> {
        match self {
            Self::ClaudeCode => Some("claude"),
            Self::OpenCode   => Some("opencode"),
            Self::Ollama     => Some("ollama"),
            Self::GenericCli => None,  // command comes from config
            _ => None,
        }
    }

    pub fn default_protocol(&self) -> Option<Box<dyn CliProtocol>> {
        match self {
            Self::ClaudeCode => Some(Box::new(ClaudeCodeProtocol)),
            Self::OpenCode   => Some(Box::new(OpenCodeProtocol)),
            Self::Ollama     => Some(Box::new(OllamaProtocol)),
            _ => None,
        }
    }
}
```

---

## 6. AgentConfig Extensions

Add `cli_protocol: Option<CliProtocolConfig>` field to `AgentConfig`. The config struct
mirrors `GenericCliProtocol` fields (command, args_template, input_mode, output_format,
streaming, streaming_format) as serializable strings for `config.json`.

---

## 7. Streaming Architecture

Streaming uses `tokio::io::BufReader` on the child's stdout, yielding `StreamChunk`
values via an `mpsc` channel. Each line is passed to `protocol.parse_stream_chunk()`.

```rust
pub async fn send_streaming(
    &mut self,
    message: &str,
    tx: mpsc::Sender<StreamChunk>,
) -> AgentResult<()> {
    let mut cmd = self.protocol.build_command(/* ... */);
    cmd.stdout(Stdio::piped());
    let mut child = cmd.spawn()?;
    let reader = BufReader::new(child.stdout.take().unwrap());
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(chunk) = self.protocol.parse_stream_chunk(&line) {
            tx.send(chunk).await.ok();
        }
    }
    Ok(())
}
```

---

## 8. Multi-Provider Routing

Config-driven routing with availability-based fallback. Integrates with `AgentManager`
at `factory.rs:118`:

```rust
pub enum RoutingStrategy {
    Priority,                        // First available in order
    CostAware { local_first: bool }, // Prefer local (ollama) for simple tasks
    RoundRobin,                      // Distribute load
}
```

---

## 9. Migration Path

### Phase 1: Add CliProtocol + ClaudeCode Implementation

- Create `src/llm_backends/protocols/` directory
- Implement `CliProtocol` trait and `ClaudeCodeProtocol`
- No changes to existing `CliAgent` yet

### Phase 2: Refactor send_message()

- Inject `CliProtocol` into `CliAgent` constructor
- Replace placeholder in `send_message()` with protocol-based implementation
- `CliUnifiedAgent.send()` now works (delegates to `send_message()`)

### Phase 3: Add OpenCode + Ollama Protocols

- Implement `OpenCodeProtocol` and `OllamaProtocol`
- Add `AgentType::Ollama` variant

### Phase 4: Add GenericCli

- Implement `GenericCliProtocol` with configurable command/args/format
- Add `AgentType::GenericCli` and `CliProtocolConfig` to `AgentConfig`

### Phase 5: Add Streaming

- Add `send_streaming()` method to `CliAgent`
- Implement `parse_stream_chunk()` for each protocol
- Wire streaming into Impulse's TUI output

### Phase 6: Wire Provider Routing

- Implement `ProviderRouter`
- Add routing configuration to config.json
- Availability-based fallback chain

---

## 10. Backward Compatibility

Existing `AgentConfig::claude_code()` and `AgentConfig::opencode()` builders continue
to work. `CliProtocol` is auto-selected based on `AgentType`:

```
  AgentType::ClaudeCode → ClaudeCodeProtocol (auto)
  AgentType::OpenCode   → OpenCodeProtocol   (auto)
  AgentType::Ollama     → OllamaProtocol     (auto)
  AgentType::GenericCli → GenericCliProtocol  (from cli_protocol config)
```

The `AgentManager.create_agent()` method at `factory.rs:145` detects CLI agents and
injects the appropriate protocol automatically.

---

## 11. Testing Strategy

| Test | Type | Description |
|------|------|-------------|
| `test_claude_build_command` | Unit | Verify --print --output-format json args |
| `test_claude_parse_response` | Unit | Known JSON → CliResponse |
| `test_ollama_build_command` | Unit | Verify "run" + model arg |
| `test_generic_stdin_mode` | Unit | Verify stdin JSON formatting |
| `test_generic_flag_mode` | Unit | Verify --prompt flag placement |
| `test_mock_cli_send` | Integration | Mock subprocess, verify round-trip |
| `test_stream_chunks` | Unit | Known NDJSON lines → StreamChunk sequence |
| `test_error_exit_code` | Unit | Non-zero exit → AgentError |
| `test_backward_compat` | Integration | Existing claude_code() config still works |
| `test_router_fallback` | Unit | Primary unavailable → secondary selected |

---

## 12. Cross-References

- **Companion (cross-cutting):** Private local design note, not included in this public repository.
- **Spec 1 — Retrieval Pipeline:** [`spec-retrieval-pipeline-upgrade.md`](./spec-retrieval-pipeline-upgrade.md) — LLM rerank stage needs working LlmProvider
- **Spec 4 — Agent Patterns:** [`spec-nullclaw-agent-patterns.md`](./spec-nullclaw-agent-patterns.md) — §5 NullClaw as Impulse panel, vtable comparison
- **Impulse source:**
  - `src/llm_backends/types.rs:11` — `AgentBackend` enum
  - `src/llm_backends/types.rs:27` — `AgentType` enum
  - `src/llm_backends/cli.rs:65` — `CliAgent` struct
  - `src/llm_backends/cli.rs:239` — `send_message()` stub
  - `src/llm_backends/factory.rs:15` — `UnifiedAgent` trait
  - `src/llm_backends/factory.rs:118` — `AgentManager`
  - `src/llm_backends/mod.rs:50` — `LlmProvider` trait
  - `src/llm_backends/mod.rs:58` — `Agent` struct
- **NullClaw source:**
  - `src/providers/claude_cli.zig` — Claude Code as provider
  - `src/providers/codex_cli.zig` — Codex CLI as provider
  - `src/providers/router.zig` — Multi-provider routing
