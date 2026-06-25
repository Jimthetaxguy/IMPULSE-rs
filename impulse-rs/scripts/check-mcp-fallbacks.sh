#!/usr/bin/env bash
set -euo pipefail
# check-mcp-fallbacks.sh - static allowlist for documented unwrap_or in mcp.rs
# Fail if any disallowed remain (except allowlisted patterns and the serialize in review_preview).

MCP_RS="impulse-rs/impulse-desktop/src/mcp.rs"

if [[ ! -f "$MCP_RS" ]]; then
  echo "ERROR: $MCP_RS not found"
  exit 1
fi

# Use awk to skip the unwrap_or_else inside review_preview function, allow documented.
DISALLOWED=$(awk '
BEGIN { in_preview=0 }
/review_preview/ { in_preview=1 }
/^}$/ && in_preview { in_preview=0 }
/ \.unwrap_or\(|\.unwrap_or_else\(|\.unwrap_or_default\(/ {
  if (in_preview && /unwrap_or_else/) next;
  if ($0 ~ /unwrap_or\(20\)/) next;
  if ($0 ~ /unwrap_or\(0\).*epoch/) next;
  print NR": "$0
}
' "$MCP_RS" || true)

if [[ -n "$DISALLOWED" ]]; then
  echo "ERROR: Disallowed silent fallbacks found in $MCP_RS:"
  echo "$DISALLOWED"
  exit 1
fi

echo "check-mcp-fallbacks: OK (only documented allowlist present)"
exit 0
