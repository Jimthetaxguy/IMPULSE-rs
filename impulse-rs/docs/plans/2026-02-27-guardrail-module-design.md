# Impulse Guardrail Module — Design Document

**Date:** 2026-02-27
**Status:** Approved
**Author:** Claude + James

---

## Problem

Impulse currently observes agent actions post-hoc (track-write, track-tool) but cannot prevent dangerous operations before they execute. External tools like hookify can provide protection, but Impulse should have built-in guardrails that protect agents regardless of the external hook setup.

## Solution

A new `src/guardrail/` module providing a fully extensible rule engine with two layers:

1. **Pre-execution gating** — Evaluates commands before they run. Blocks critical violations.
2. **Post-observation warnings** — Evaluates completed actions. Logs warnings and audit trail.

## Architecture

### Module Structure

```
src/guardrail/
  mod.rs          — Public API: evaluate(), load_rules(), list_rules()
  types.rs        — GuardRule, GuardAction, GuardTarget, GuardResult, GuardConfig
  engine.rs       — Pattern matching engine (compiled regex cache, evaluation loop)
  defaults.rs     — Built-in rules (git safety, destructive ops, deploy guards)
  config.rs       — Load/merge/save rules from config.json
```

### Data Model

```rust
struct GuardRule {
    id: String,                    // e.g., "block-force-push-main"
    pattern: String,               // Regex pattern to match against
    action: GuardAction,           // Block | Warn | Log
    target: GuardTarget,           // Bash | ToolCall | FileWrite | Any
    reason: String,                // Human-readable explanation
    suggestion: Option<String>,    // What to do instead
    enabled: bool,                 // Can be disabled per-project
    builtin: bool,                 // true = shipped with Impulse
}

enum GuardAction {
    Block,   // Prevent execution, non-zero exit
    Warn,    // Allow but log warning + notify
    Log,     // Silent log for audit trail
}

enum GuardTarget {
    Bash,       // Shell commands
    ToolCall,   // Agent tool invocations
    FileWrite,  // File system writes
    Any,        // All targets
}

struct GuardResult {
    rule_id: String,
    action: GuardAction,
    matched_input: String,
    reason: String,
    suggestion: Option<String>,
}
```

### Config Integration

Rules stored in `config.json` under `"guardrails"` key:

```json
{
  "guardrails": {
    "enabled": true,
    "rules": [
      {
        "id": "block-force-push-main",
        "pattern": "git\\s+push\\s+.*--force.*\\s+(origin\\s+)?main",
        "action": "block",
        "target": "bash",
        "reason": "Force pushing to main rewrites shared history",
        "suggestion": "Create a branch and open a PR instead"
      }
    ]
  }
}
```

Built-in defaults are compiled into the binary. User rules with the same `id` override built-in rules.

## Built-in Default Rules

| ID | Target | Action | Pattern |
|----|--------|--------|---------|
| `block-force-push-main` | Bash | Block | `git\s+push\s+.*--force.*\s+(origin\s+)?main` |
| `block-bulk-git-add` | Bash | Block | `git\s+add\s+(-A\|--all\|\.\s*$)` |
| `block-rm-rf-root` | Bash | Block | `rm\s+-rf\s+[/~]` |
| `block-drop-table` | Bash | Block | `DROP\s+TABLE\|DROP\s+DATABASE` |
| `warn-large-commit` | Bash | Warn | Threshold: >200 staged files |
| `warn-binary-staging` | Bash | Warn | `git\s+add\s+.*\.(zip\|pdf\|exe\|dll\|dmg)` |
| `warn-artifact-staging` | Bash | Warn | `git\s+add\s+.*(node_modules\|\.venv\|__pycache__)` |
| `warn-env-file-staging` | Bash | Warn | `git\s+add\s+.*\.env` |
| `warn-chmod-recursive` | Bash | Warn | `chmod\s+-R\s+777` |
| `log-deploy-commands` | Bash | Log | `deploy\|publish\|release` |

## CLI Interface

New subcommand: `impulse-rs guard`

```
impulse-rs guard --action "git push --force origin main"
impulse-rs guard --action "rm -rf /" --target bash
impulse-rs guard --list                    # Show all active rules
impulse-rs guard --enable <rule-id>
impulse-rs guard --disable <rule-id>
```

Exit codes:
- `0` — No blocking rules matched (proceed)
- `1` — Blocked by a rule (do not proceed)
- Stderr: structured JSON `GuardResult`

## Hook Integration

Pre-execution hooks (new):

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Bash",
      "command": "impulse-rs guard --action \"$TOOL_INPUT\" --target bash"
    }],
    "PreToolUse": [{
      "matcher": "Write",
      "command": "impulse-rs guard --action \"$TOOL_INPUT\" --target file-write"
    }]
  }
}
```

Post-observation hooks (existing `track-tool` enhanced with guardrail evaluation).

## Daemon IPC Integration

New daemon request:

```rust
DaemonRequest::GuardEvaluate {
    target: String,    // "bash", "tool-call", "file-write"
    action: String,    // The command/action to evaluate
}
```

Response: `GuardResult` as JSON.

Fallback: If daemon IPC fails, fall back to direct evaluation (exit code + stderr).

## Post-Observation Layer

Existing `track-tool` hook enhanced:
- After action completes, evaluate Warn/Log rules
- `Warn` results: write to session log + display in TUI activity feed
- `Log` results: write to session log silently

## Testing Strategy

- Unit tests for each rule pattern (positive + negative matches)
- Unit tests for rule merging (built-in + user override)
- Unit tests for engine evaluation order (Block first, then Warn, then Log)
- Integration tests for CLI guard command (exit codes, stderr output)
- Integration tests for daemon IPC GuardEvaluate
- Integration tests for direct-mode fallback when daemon unavailable

## Principles Adherence

- **Never Panic**: All functions return `Result<T>`. Invalid regex patterns return error, don't crash.
- **Atomic Writes**: Config updates use temp + rename.
- **Input Validation**: Regex patterns validated on load. Action strings sanitized.
- **Capability-Based**: Guard evaluation is a read-only operation, no special capabilities needed.
- **Review Before Apply**: Block rules prevent action; user sees reason + suggestion.
