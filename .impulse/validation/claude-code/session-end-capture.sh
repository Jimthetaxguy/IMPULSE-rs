#!/bin/bash
set -eu

ROOT="${CLAUDE_PROJECT_DIR:-$(pwd)}"
VALIDATION_DIR="$ROOT/.impulse/validation/claude-code"
ARTIFACT_DIR="$VALIDATION_DIR/artifacts"
mkdir -p "$ARTIFACT_DIR"

PAYLOAD_PATH="$ARTIFACT_DIR/session-end.stdin.json"
if [ ! -t 0 ]; then
  cat > "$PAYLOAD_PATH"
else
  : > "$PAYLOAD_PATH"
fi

IMPULSE_HOOK_EVIDENCE=1 \
impulse-rs -c "$ROOT/.impulse" session-end --session-id "${IMPULSE_SESSION_ID:-missing}" --summary "${CLAUDE_SESSION_SUMMARY:-missing}" < "$PAYLOAD_PATH"
