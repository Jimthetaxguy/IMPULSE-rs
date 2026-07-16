#!/usr/bin/env bash
# Load ElevenLabs_API_Key from Infisical and run impulse-rs voice commands.
# Never prints the secret value.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CODE_ROOT="${CODE_ROOT:-$HOME/code}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.cargo-target-voice}"
BIN="${IMPULSE_BIN:-$CARGO_TARGET_DIR/debug/impulse-rs}"

if [[ ! -x "$BIN" ]]; then
  echo "building impulse-rs (voice worktree)…"
  (cd "$ROOT/impulse-rs" && cargo build -q --bin impulse-rs)
fi

if [[ -z "${ELEVENLABS_API_KEY:-}" ]]; then
  if command -v infisical >/dev/null 2>&1 && [[ -f "$CODE_ROOT/.infisical.json" ]]; then
    ELEVENLABS_API_KEY="$(
      cd "$CODE_ROOT" && infisical secrets get ElevenLabs_API_Key --env=dev --plain --silent 2>/dev/null | tr -d '\r\n'
    )"
    export ELEVENLABS_API_KEY
  fi
fi

if [[ -z "${ELEVENLABS_API_KEY:-}" ]]; then
  echo "error: ELEVENLABS_API_KEY not set and Infisical ElevenLabs_API_Key unavailable" >&2
  exit 1
fi

# Presence-only status line
python3 -c 'import os; k=os.environ["ELEVENLABS_API_KEY"]; print(f"ELEVENLABS_API_KEY loaded (len={len(k)})")'

if [[ $# -eq 0 ]]; then
  exec "$BIN" voice status --json
fi

# Convenience: first arg can omit the "voice" prefix
if [[ "$1" != "voice" ]]; then
  exec "$BIN" voice "$@"
fi
exec "$BIN" "$@"
