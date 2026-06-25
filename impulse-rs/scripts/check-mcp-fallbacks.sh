#!/usr/bin/env bash
set -euo pipefail
# check-mcp-fallbacks.sh - static allowlist for documented unwrap_or in mcp.rs
# Fail if any disallowed .unwrap_or / _or_else / _or_default remain.

MCP_RS="impulse-rs/impulse-desktop/src/mcp.rs"

if [[ ! -f "$MCP_RS" ]]; then
  echo "ERROR: $MCP_RS not found"
  exit 1
fi

# Allowed patterns (documented):
# - unwrap_or(20) for limit default
# - unwrap_or(0) // epoch fallback ...
# - expect path for serialize (the to_string expect)
# - any map_err or proper propagation

DISALLOWED=$(grep -n -E '\.unwrap_or\(|\.unwrap_or_else\(|\.unwrap_or_default\(' "$MCP_RS" | grep -v -E 'unwrap_or\(20\)|unwrap_or\(0\).*epoch|expect\("arguments should always serialize"\)|map_err' || true)

if [[ -n "$DISALLOWED" ]]; then
  echo "ERROR: Disallowed silent fallbacks found in $MCP_RS:"
  echo "$DISALLOWED"
  exit 1
fi

echo "check-mcp-fallbacks: OK (only documented allowlist present)"
exit 0
