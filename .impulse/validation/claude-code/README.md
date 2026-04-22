# Claude Hook Validation Kit

This kit validates the real memory loop with Claude Code hooks before claiming the feature works end-to-end.

## What this proves

1. `SessionStart` runs the real `impulse-rs session-start` command and reaches Claude in usable startup context
2. `SessionStart` persists `IMPULSE_SESSION_ID` through `CLAUDE_ENV_FILE` so later hooks can reuse the same session
3. `SessionEnd` runs the real `impulse-rs session-end` command and records hook evidence
4. The persisted history/GENOME files are the source for the next session's recall

## How to run

1. Ensure the daemon is running for memory features:
   - `cargo run -- daemon`
2. Register the hooks from `settings.local.json` into your Claude project settings.
3. Start a fresh Claude session in this project.
4. Ask Claude: `What does IMPULSE_HOOK_SENTINEL mean in this project?`
5. Do a small piece of real work.
6. End the session cleanly.
7. Start a second session and ask:
   - `What happened in the previous Impulse session?`
   - `What does Impulse remember from the last run?`
8. Inspect `.impulse/validation/runtime/hook-events.jsonl`.
9. Record the result in `evidence.md`.

## Pass criteria

- Claude can explain the sentinel from `SessionStart`, not just echo terminal output.
- `.impulse/validation/runtime/hook-events.jsonl` contains both `session_start` and `session_end` events.
- The `session_start` record shows a non-empty output preview and the `session_end` record captures stdin/env metadata.
- `.impulse/HISTORY.jsonl` gains a new entry after the first session ends.
- The next session references the prior summary/files from `.impulse/HISTORY.jsonl` or `GENOME.md`.

## Failure criteria

- Claude cannot see the sentinel on startup.
- `SessionEnd` never records runtime evidence.
- The next session does not recall the prior persisted summary.
