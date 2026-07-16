//! ElevenLabs-first voice engine adapter for Impulse.
//!
//! Primary backend is an **ElevenLabs Conversational Agent** that issues
//! **client tools** (in-process) or **webhook tools** (HTTP into a local
//! Impulse endpoint). Both shapes normalize into the same tool-call envelope
//! and execute against the real [`crate::tooling::ToolRegistry`] — not a
//! parallel toy registry.
//!
//! ## Priority
//!
//! [`VoiceProvider::ElevenLabsAgent`] is the default and only first-class
//! voice mode. Other backends are non-default placeholders.
//!
//! ## Safety
//!
//! Mutating tool classes (`FileSystemWrite`, `ShellExec`, `PythonExec`,
//! `Network`) are **deny-by-default** on the voice path unless the call carries
//! an explicit confirmation flag that the policy accepts.
//!
//! ## Live network
//!
//! Session WebSocket I/O to ElevenLabs is intentionally behind traits /
//! optional live smoke. Core mapping + policy is unit-tested with fixtures.

mod adapter;
mod envelope;
mod policy;
mod provider;
mod webhook;

pub use adapter::{
    invoke_elevenlabs_client_tool, invoke_elevenlabs_client_tool_with, VoiceToolBridge,
};
pub use envelope::{
    ElevenLabsClientToolRequest, ElevenLabsToolResult, ElevenLabsToolResultStatus,
    VoiceToolCallSource,
};
pub use policy::{
    classify_tool_risk, VoicePolicy, VoicePolicyDecision, VoiceToolRisk, DEFAULT_VOICE_EXPOSED_TOOLS,
};
pub use provider::{default_voice_provider, VoiceProvider};
pub use webhook::{parse_webhook_tool_request, WebhookToolRequest};

/// Module-level docs for operators / agent config (also returned by CLI).
pub fn voice_engine_docs() -> &'static str {
    r#"# Impulse voice engine (ElevenLabs Agent first)

## Primary provider
ElevenLabs Conversational Agent (Agents product with tools).

## Tool bridge
Client-tool and webhook invocations map 1:1 to `ToolRegistry::execute` using
case-sensitive tool ids (e.g. `system_info`, `file_read`, `bash_exec`).

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
