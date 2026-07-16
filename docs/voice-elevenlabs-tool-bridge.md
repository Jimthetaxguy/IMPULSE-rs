# Voice engine — ElevenLabs Agent tool bridge

## Purpose

Wire an ElevenLabs Conversational Agent so its **client tools** / **webhook tools** invoke
Impulse’s real dynamic tool surface (`ToolRegistry::execute`), with mutating tools gated.

## Priority

| Provider | Role |
|----------|------|
| **ElevenLabs Agent** | Primary / default (`IMPULSE_VOICE_PROVIDER` empty or `elevenlabs_agent`) |
| Other | Non-default placeholder only |

## Flow (MCP-shaped Rust server)

```
ElevenLabs Conversational Agent
   │ client tools          │ server tools (webhook)
   ▼                       ▼
VoiceServer::process_request     POST http://127.0.0.1:8787/voice/tools
   methods: tools/list, tools/call, voice/schema
   │
   ▼
VoiceToolBridge  (Arc<ToolRegistry> + ToolContext + VoicePolicy)
   │
   ▼
ToolRegistry::execute  (same as MCP tools/call + daemon InvokeTool)
   │
   ▼
ElevenLabsToolResult  (wait_for_response JSON for agent context)
```

This is intentional parity with `src/mcp/server.rs`: same registry, same execute
path, same stdio/TCP JSON-line discipline (bounded reads), plus an HTTP webhook
transport for ElevenLabs server tools.

## CLI

```bash
impulse-rs voice status --json
impulse-rs voice list-tools --json
impulse-rs voice schema --json          # register as EL client tools
impulse-rs voice serve --transport webhook --port 8787
impulse-rs voice serve --transport stdio   # JSON-line tools/list|tools/call
impulse-rs voice tool-call --name system_info --params '{"include_env":false}' --json
impulse-rs voice tool-call --name bash_exec --params '{"command":"echo hi"}' --json
# expected: denied without --confirmed
impulse-rs voice docs
```

## Env

| Variable | Use |
|----------|-----|
| `ELEVENLABS_API_KEY` | Optional live conversation smoke |
| `IMPULSE_ELEVENLABS_AGENT_ID` | Optional agent id |
| `IMPULSE_VOICE_PROVIDER` | Defaults to ElevenLabs Agent |

## Exposed tools

See `DEFAULT_VOICE_EXPOSED_TOOLS` in `src/voice/policy.rs` and `impulse-rs voice list-tools`.

## Tests

```bash
cd impulse-rs
cargo test --lib voice::
```
