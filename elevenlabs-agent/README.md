# Impulse ↔ ElevenLabs agent project

Managed with **ElevenLabs CLI** + **Infisical** (`ElevenLabs_API_Key` from `~/code/.infisical.json` / env `dev`).

## Live agent

| Field | Value |
|-------|--------|
| Name | Impulse Control Plane |
| ID | `agent_7001kxmm5n58erer2v0yh73eqepw` |
| ASR | `scribe_realtime` |

### Attached client tools (case-sensitive Impulse ids)

| Tool | ElevenLabs tool id |
|------|--------------------|
| `system_info` | `tool_4701kxmm3ezbetwb91k61e11hngg` |
| `health_check` | `tool_2201kxmm3etee4may4w6erszr217` |
| `config_get` | `tool_0401kxmm3eq3ezgt6jyg52nncn28` |
| `steward_status` | `tool_1101kxmm3ewyfkstpadw11nsq0v8` |

These are **client tools**: a conversation client (or Impulse `voice tool-call`) must execute them and return results. For cloud **server tools**, point webhooks at `impulse-rs voice serve --transport webhook` (needs a public URL / tunnel).

## Secrets

```bash
# Infisical (preferred on this machine)
export ELEVENLABS_API_KEY="$(cd ~/code && infisical secrets get ElevenLabs_API_Key --env=dev --plain --silent)"

# Or helper
../scripts/voice-with-infisical.sh status --json
```

Never commit `.env` or keys.

## Sync

```bash
../scripts/voice-el-sync-tools.sh   # export schema → client tool configs → push
elevenlabs agents push              # from this directory
```
