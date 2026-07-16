# Voice engine — ElevenLabs Agent tool bridge

## Purpose

Wire an ElevenLabs Conversational Agent so its **client tools** / **webhook tools** invoke
Impulse’s real dynamic tool surface (`ToolRegistry::execute`), with mutating tools gated.

## Priority

| Provider | Role |
|----------|------|
| **ElevenLabs Agent** | Primary / default (`IMPULSE_VOICE_PROVIDER` empty or `elevenlabs_agent`) |
| Other | Non-default placeholder only |

## Flow

```
ElevenLabs agent tool call
        │
        ▼
ElevenLabsClientToolRequest  (client JSON or webhook body)
        │
        ▼
VoicePolicy  (allowlist + mutate deny-by-default unless confirmed)
        │
        ▼
ToolRegistry::execute  (same as daemon InvokeTool / tooling-run)
        │
        ▼
ElevenLabsToolResult  (ok | denied | error; wait_for_response for agent context)
```

## CLI

```bash
impulse-rs voice status --json
impulse-rs voice list-tools --json
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
