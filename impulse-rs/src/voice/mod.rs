//! ElevenLabs-first voice engine for Impulse — **MCP-shaped**, registry-backed.
//!
//! Architecture mirrors [`crate::mcp::server::McpServer`]:
//! - hold `Arc<ToolRegistry>` + [`ToolContext`](crate::tooling::ToolContext)
//! - expose `tools/list` + `tools/call` over stdio / TCP JSON-line
//! - serve `POST /voice/tools` for ElevenLabs **server tools** (webhook)
//! - export client-tool schemas from the live registry (no second tool list)
//!
//! Primary backend: **ElevenLabs Conversational Agent**. Mutating capabilities
//! are deny-by-default unless confirmed. Live EL WebSocket session I/O is
//! optional; core path is fixture-tested without network.

mod adapter;
mod envelope;
mod policy;
mod provider;
mod schema;
mod secrets;
mod server;
mod webhook;

pub use adapter::{
    invoke_elevenlabs_client_tool, invoke_elevenlabs_client_tool_with, VoiceToolBridge,
};
pub use envelope::{
    ElevenLabsClientToolRequest, ElevenLabsToolResult, ElevenLabsToolResultStatus,
    VoiceToolCallSource,
};
pub use policy::{
    classify_tool_risk, VoicePolicy, VoicePolicyDecision, VoiceToolRisk,
    DEFAULT_VOICE_EXPOSED_TOOLS,
};
pub use provider::{default_voice_provider, VoiceProvider};
pub use schema::{elevenlabs_client_tool_schemas, ElevenLabsClientToolSchema};
pub use secrets::{ensure_elevenlabs_env, load_elevenlabs_api_key, SecretSource};
pub use server::{VoiceServer, VoiceTransport};
pub use webhook::{parse_webhook_tool_request, WebhookToolRequest};

/// Module-level docs for operators / agent config (also returned by CLI).
pub fn voice_engine_docs() -> &'static str {
    r#"# Impulse voice engine (ElevenLabs Agent first)

## Primary provider
ElevenLabs Conversational Agent (Agents product with tools).

## Tool bridge (Rust / Impulse-native)
Implemented like `McpServer`:
- `VoiceServer` + `VoiceToolBridge` hold `Arc<ToolRegistry>` + `ToolContext`
- JSON-line: `tools/list`, `tools/call`, `voice/schema` (stdio or TCP)
- HTTP webhook: `POST /voice/tools` for ElevenLabs server tools
- Client-tool schemas exported from the live registry for agent config
- Case-sensitive tool ids (`system_info`, `file_read`, `bash_exec`, …)

```bash
impulse-rs voice serve --transport webhook --port 8787
# Point ElevenLabs server tool URL at http://127.0.0.1:8787/voice/tools
impulse-rs voice schema --json   # register client tools on the agent
```

## Default exposed tools (read-oriented)
- system_info, health_check, config_get, steward_status
- session_query, memory_search, file_read, genome_read
- build_health / sccache_status / tool_availability

## Mutating tools (deny-by-default without confirmation)
- calculator (PythonExec), file_write, bash_exec, python_exec
- clean_all / wipe / sweep / sccache_setup (build hygiene write paths)
Any tool requiring FileSystemWrite, ShellExec, PythonExec, or Network is gated.

## Wait for response
Successful invokes return a structured JSON tool result for the conversation
turn. Failures return an error envelope without panicking the session.

## Config (env, never hardcode secrets)
- ELEVENLABS_API_KEY
- IMPULSE_ELEVENLABS_AGENT_ID (optional agent id for live smoke)
"#
}
