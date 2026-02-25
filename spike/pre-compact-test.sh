#!/bin/bash
# Spike 1.2: PreCompact Survival Test
#
# Purpose: Validate that a PreCompact hook's stdout survives
# context window compaction.
#
# Setup:
#   1. Register this script in .claude/settings.local.json:
#      {
#        "hooks": {
#          "PreCompact": [
#            {
#              "type": "command",
#              "command": "/path/to/spike/pre-compact-test.sh"
#            }
#          ]
#        }
#      }
#   2. Start a Claude Code session and work until compaction triggers
#      (fill context window with many tool calls)
#   3. After compaction, ask Claude: "What is IMPULSE_COMPACT_MARKER?"
#   4. If Claude knows, the PreCompact hook output survived.
#
# Expected: The text below should persist through compaction.

echo "IMPULSE_COMPACT_MARKER: This text MUST survive context compaction."
echo ""
echo "## Critical Knowledge (PreCompact)"
echo "- Runtime: Bun >= 1.0"
echo "- Memory model: 3 files (GENOME.md, LIVE_STATE.json, HISTORY_INDEX.md)"
echo "- All file writes use atomic temp+rename pattern"
