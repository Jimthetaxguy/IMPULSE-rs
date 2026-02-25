#!/bin/bash
# Spike 1.1: SessionStart Hook Injection Test
#
# Purpose: Validate that a SessionStart hook's stdout appears
# in Claude Code's system context.
#
# Setup:
#   1. Register this script in .claude/settings.local.json:
#      {
#        "hooks": {
#          "SessionStart": [
#            {
#              "type": "command",
#              "command": "/path/to/spike/session-start-test.sh"
#            }
#          ]
#        }
#      }
#   2. Start a new Claude Code session in this project
#   3. Ask Claude: "What does 'IMPULSE_SPIKE_MARKER' mean?"
#   4. If Claude can see this text, the hook injection works.
#
# Expected: The text below should appear as system context.

echo "IMPULSE_SPIKE_MARKER: SessionStart hook injection verified at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""
echo "## Spike 1.1 Test Data"
echo "- Decision: Use Bun as primary runtime"
echo "- Decision: File-first memory architecture"
echo "- This is test data from the Impulse pre-phase spike."
