---
status: accepted
phase: 1
audience: builder
tags: [decision, distribution, npm]
last_updated: 2026-02-20
---

# ADR-005: npm Package with Auto-Configuration

> **Status:** Accepted
> **Date:** 2026-02-20

---

## Context

Impulse's value depends on frictionless setup. If installation requires more than 2-3 commands, most developers won't bother. The Claude Code hook system requires specific configuration in `.claude/settings.local.json`, and the `.impulse/` directory must be created with proper templates.

### Claude Code Hook Configuration (AGENT-HARNESS-ANALYSIS.md §2.1)

Claude Code hooks are configured in JSON settings files. The relevant file for per-project, gitignored configuration is `.claude/settings.local.json`. Each hook specifies a command to run:

```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{ "type": "command", "command": "impulse-session-start" }]
    }]
  }
}
```

For this to work, the `impulse-session-start` command must be on the user's PATH.

### mise Auto-Init (TERMINAL-LAYER-ANALYSIS.md §3.2)

mise's `enter` hook fires when `cd`-ing into a project directory where a `mise.toml` exists. This enables zero-friction initialization:

```toml
[hooks]
enter = """
  if [ ! -d ".impulse" ]; then
    mkdir -p .impulse
    # ... seed files
  fi
"""
```

With `MISE_PROJECT_ROOT` available, the hook knows exactly where to create `.impulse/`.

### The Manual Prompt Engineering Problem (LLM-CODING-PROBLEMS.md §8)

Developers currently hand-author CLAUDE.md files, manually configure hooks, and tune extraction prompts per project. Impulse should eliminate this friction: install once, configure automatically, GENOME.md self-evolves from there.

---

## Decision

**Impulse is distributed as an npm package providing CLI commands for hook execution and a setup wizard for auto-configuration.**

### Package Name

`impulse-memory` (npm)

### Installation Flow

```bash
# Step 1: Install globally
npm install -g impulse-memory

# Step 2: Initialize in a project
cd my-project
impulse init
```

The `impulse init` command:

1. **Creates `.impulse/` directory** with three files:
   - `GENOME.md` — seeded from built-in template (project name, creation date, empty sections)
   - `LIVE_STATE.json` — empty initial state (`{"agents":[],"lastUpdated":""}`)
   - `HISTORY_INDEX.md` — empty with header (`# Session History`)

2. **Patches `.claude/settings.local.json`** to register four hooks:
   - Creates `.claude/` directory if it doesn't exist
   - Reads existing `settings.local.json` (if any), preserving existing hooks
   - Merges Impulse's four hook configurations
   - Writes back the merged configuration
   - This file is gitignored by Claude Code by default — no impact on teammates

3. **Adds `.impulse/LIVE_STATE.json` to `.gitignore`** (if not already present)

4. **Prints success message** with status summary:
   ```
   Impulse initialized!
   - .impulse/GENOME.md (0 decisions)
   - .impulse/LIVE_STATE.json (gitignored)
   - .impulse/HISTORY_INDEX.md (0 sessions)
   - .claude/settings.local.json (4 hooks registered)

   Run `impulse status` to check state.
   ```

### CLI Commands

| Command | Purpose |
|---------|---------|
| `impulse init` | Scaffold `.impulse/`, configure Claude Code hooks |
| `impulse status` | Show GENOME.md line count, active agents, recent sessions, extraction stats |
| `impulse-session-start` | Hook CLI: reads stdin JSON, loads 3 files, prints context to stdout |
| `impulse-post-tool-use` | Hook CLI: reads stdin JSON, updates LIVE_STATE.json |
| `impulse-session-end` | Hook CLI: reads transcript, runs extraction, writes GENOME.md + HISTORY |
| `impulse-pre-compact` | Hook CLI: reads GENOME.md top lines, prints to stdout |

The `impulse-*` commands are not user-facing — they're invoked by Claude Code hooks. They read JSON from stdin and (for start/compact) write context to stdout.

### Configuration Hierarchy

```
~/.impulse/config.json          # Global defaults (API key, model preference)
.impulse/config.json            # Project overrides (extraction rules, max lines)
Environment variables           # Runtime overrides (CI/CD, testing)
```

**Global config (`~/.impulse/config.json`):**
```json
{
  "apiKey": "sk-...",
  "model": "claude-haiku-4-5-20251001",
  "maxGenomeLines": 200,
  "maxHistoryEntries": 100,
  "autoExtract": true
}
```

**Environment variable overrides:**
| Variable | Purpose | Default |
|----------|---------|---------|
| `IMPULSE_API_KEY` | LLM API key for extraction | Falls back to `ANTHROPIC_API_KEY` |
| `IMPULSE_MODEL` | Model for extraction | `claude-haiku-4-5-20251001` |
| `IMPULSE_MAX_GENOME_LINES` | Warn threshold | `200` |
| `IMPULSE_AUTO_EXTRACT` | Enable/disable extraction | `true` |

### mise Integration (Optional)

For projects using mise, `impulse init --mise` appends to `mise.toml`:

```toml
[hooks]
enter = """
  if [ ! -d ".impulse" ]; then
    impulse init --quiet 2>/dev/null || true
  fi
"""
```

This auto-initializes `.impulse/` on project enter for any developer who has Impulse installed.

---

## Consequences

### Positive

- **Single `npm install -g impulse-memory && impulse init` to get running** — Two commands, zero configuration files to hand-edit.
- **`.claude/settings.local.json` is gitignored** — Impulse doesn't affect teammates who don't use it. Each developer opts in independently.
- **Configuration hierarchy supports both personal and project preferences** — Global API key in `~/.impulse/config.json`, project-specific rules in `.impulse/config.json`.
- **mise integration provides zero-friction onboarding** — `cd` into a project and `.impulse/` appears automatically.
- **`impulse status` provides instant visibility** — One command shows GENOME size, active agents, and recent history.

### Negative

- **Requires npm/Node ecosystem** — Developers who don't have Node.js installed must install it first. Mitigated by: most developers already have Node; Bun also works (`bun install -g`).
- **Auto-patching `settings.local.json` is fragile** — If the user has custom hooks with non-standard formatting, the merge may fail. Mitigated by: read-parse-merge-write approach (not blind append), and a `--manual` flag that prints the configuration for manual copy-paste.
- **Global install may conflict with other versions** — If a user has Impulse v1 globally and a project needs v2, commands may mismatch. Mitigated by: `impulse --version` check at init time, and local `npx impulse-memory` as alternative.
- **API key management adds friction** — Users must obtain and configure an API key for extraction. Mitigated by: fall back to `ANTHROPIC_API_KEY` which many Claude Code users already have set.

---

## Alternatives Considered

### Alternative 1: Shell Scripts Only (No npm Package)

Rejected because:
- Users must manually copy 4 shell scripts to a PATH-accessible location
- No `init` wizard — manual `.impulse/` creation and `settings.local.json` editing
- No `status` command for monitoring
- Version management is manual (no `npm update`)
- The convenience gap is too large for adoption

### Alternative 2: Claude Code Plugin (Built-in)

Deferred because:
- Claude Code does not currently support third-party plugins (only hooks and MCP servers)
- If/when a plugin system is added, Impulse could be distributed as a native plugin
- Hooks are the current extensibility mechanism and they work

### Alternative 3: Homebrew / Cargo / pip Distribution

Deferred because:
- npm reaches the largest audience of developers who use AI coding tools
- Adding Homebrew formula, Cargo crate, or PyPI package increases maintenance burden
- Can be added later as alternative distribution channels
- `npx impulse-memory init` works without global install for one-time setup

### Alternative 4: Docker-Based Distribution

Rejected because:
- Docker adds startup latency (~500ms+) to every hook invocation
- Hooks fire frequently (PostToolUse on every file write) — latency compounds
- Docker daemon dependency is heavier than npm
- Violates "zero infrastructure" principle

---

## References

- AGENT-HARNESS-ANALYSIS.md §2.1: Claude Code hook configuration via `.claude/settings.local.json`
- AGENT-HARNESS-ANALYSIS.md §5: Recommended platform strategy (shell scripts invoking Bun CLIs)
- TERMINAL-LAYER-ANALYSIS.md §3.2: mise `enter` hook for auto-init (`MISE_PROJECT_ROOT` available)
- LLM-CODING-PROBLEMS.md §8: Manual prompt engineering is a pain point — zero-config after install
