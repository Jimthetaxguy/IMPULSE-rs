#!/usr/bin/env bash
# Sync Impulse read-only tools → ElevenLabs CLI project (client tools) and push.
# Uses Infisical for the API key. Does not print secrets.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AGENT_DIR="$ROOT/elevenlabs-agent"
CODE_ROOT="${CODE_ROOT:-$HOME/code}"
# Exported here (not later) because the Python sync heredoc below reads it
# via os.environ before the tail of the script runs.
export SCRATCH="${SCRATCH:-$(mktemp -d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.cargo-target-voice}"
BIN="${IMPULSE_BIN:-$CARGO_TARGET_DIR/debug/impulse-rs}"

if [[ -z "${ELEVENLABS_API_KEY:-}" ]]; then
  ELEVENLABS_API_KEY="$(
    cd "$CODE_ROOT" && infisical secrets get ElevenLabs_API_Key --env=dev --plain --silent 2>/dev/null | tr -d '\r\n'
  )"
  export ELEVENLABS_API_KEY
fi
[[ -n "${ELEVENLABS_API_KEY:-}" ]] || { echo "missing API key" >&2; exit 1; }

if [[ ! -x "$BIN" ]]; then
  (cd "$ROOT/impulse-rs" && cargo build -q --bin impulse-rs)
fi

mkdir -p "$AGENT_DIR"
cd "$AGENT_DIR"
if [[ ! -f agents.json ]]; then
  elevenlabs agents init .
fi

"$BIN" voice schema --json >"$SCRATCH/schemas.json"
python3 - <<'PY'
import json
from pathlib import Path
import os
schemas = json.loads(Path(os.environ["SCRATCH"]).joinpath("schemas.json").read_text())
prefer = {"system_info", "health_check", "config_get", "steward_status"}
out_dir = Path("tool_configs")
out_dir.mkdir(exist_ok=True)
created = []
for s in schemas:
    name = s["name"]
    if name not in prefer:
        continue
    props_in = (s.get("parameters") or {}).get("properties") or {}
    required = (s.get("parameters") or {}).get("required") or []
    properties = {
        pname: {
            "type": pdef.get("type", "string"),
            "description": pdef.get("description") or pname,
            "is_system_provided": False,
            "dynamic_variable": "",
            "allowed_values_dynamic_variable": "",
            "constant_value": "",
            "is_omitted": False,
        }
        for pname, pdef in props_in.items()
    }
    cfg = {
        "type": "client",
        "name": name,
        "description": s.get("description") or name,
        "response_timeout_secs": 20,
        "disable_interruptions": False,
        "interruption_mode": "allow",
        "force_pre_tool_speech": False,
        "pre_tool_speech": "auto",
        "assignments": [],
        "tool_call_sound_behavior": "auto",
        "tool_error_handling_mode": "auto",
        "parameters": {
            "type": "object",
            "required": required,
            "description": "Impulse tool parameters (case-sensitive)",
            "properties": properties,
        },
        "dynamic_variables": {"dynamic_variable_placeholders": {}},
        "execution_mode": "immediate",
        "expects_response": True,
        "execution_platform": "client",
    }
    (out_dir / f"impulse_{name}.json").write_text(json.dumps(cfg, indent=2) + "\n")
    created.append(name)

tools_path = Path("tools.json")
data = json.loads(tools_path.read_text()) if tools_path.exists() else {"tools": []}
existing = {(t.get("config") or "") for t in data.get("tools", [])}
for name in created:
    rel = f"tool_configs/impulse_{name}.json"
    if rel not in existing:
        data.setdefault("tools", []).append({"type": "client", "config": rel})
tools_path.write_text(json.dumps(data, indent=2) + "\n")
print("impulse_client_tools", created)
PY

# push only impulse_* tools if CLI supports filtering; otherwise push all local configs
printf 'y\n' | elevenlabs tools push 2>&1 | tail -40

echo "done — tools pushed (see elevenlabs-agent/tools.json for ids)"
