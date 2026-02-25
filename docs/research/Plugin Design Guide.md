# Designing Effective Plugins for Coding Agents: A Technical Instruction Guide

This guide provides concrete, implementation-ready patterns for building plugins that extend coding agents — specifically Claude Code and OpenCode. It covers architecture decisions, skill and hook design, context management, security, testing, distribution, and how to prevent plugin bloat.

## Choosing an Architecture Pattern

Every plugin should map to one of three structural patterns. The choice determines how complex the plugin can grow before becoming unmaintainable.[^1][^2]

| Pattern | Structure | When to Use | Example |
|---------|-----------|-------------|---------|
| **Single skill/tool** | One `SKILL.md` or one exported tool | Focused capability with no branching logic | A linter that runs after file edits |
| **Orchestrator-worker** | A coordinator skill that spawns sub-agents in parallel | Tasks that decompose into independent subtasks | A code reviewer that spawns per-subsystem reviewers |
| **Pipeline** | Sequential stages where each output feeds the next | Structured multi-step workflows | Analyze → plan → edit → validate |

Default to the single skill/tool pattern. Upgrade to orchestrator-worker or pipeline only when the task genuinely requires decomposition or sequencing. Premature complexity is the top cause of plugin bloat.[^3]

## Plugin Directory Structure

Both Claude Code and OpenCode enforce specific directory conventions. Getting the layout wrong is the most common reason a plugin fails to load.[^4]

### Claude Code Layout

```
my-plugin/
├── .claude-plugin/
│   └── plugin.json          # Manifest (only file in this dir)
├── skills/
│   └── review/
│       ├── SKILL.md          # Required entry point
│       ├── reference.md      # Optional supporting docs
│       └── scripts/
│           └── validate.sh   # Optional helper scripts
├── agents/
│   └── security-reviewer.md  # Custom sub-agent definitions
├── commands/
│   └── deploy.md             # User-invoked slash commands
├── hooks/
│   └── hooks.json            # Event handlers
├── scripts/
│   └── lint-check.sh         # Hook scripts
├── settings.json             # Default config (only `agent` key supported)
├── .mcp.json                 # MCP server definitions
├── .lsp.json                 # LSP server configurations
├── README.md
└── CHANGELOG.md
```

The `.claude-plugin/` directory must contain only `plugin.json`. Placing `skills/`, `agents/`, or `hooks/` inside `.claude-plugin/` is the single most common structural mistake.[^4]

### OpenCode Layout

```
.opencode/
├── plugins/
│   └── my-plugin.ts          # Plugin module (auto-loaded)
├── package.json              # Optional: external npm dependencies
└── ...
```

Or via `opencode.json` for npm-based plugins:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["opencode-wakatime", "@my-org/custom-plugin"]
}
```

OpenCode plugins are JavaScript/TypeScript modules exporting async functions. Local files in `.opencode/plugins/` (project) or `~/.config/opencode/plugins/` (global) are loaded automatically at startup.[^5]

## Writing the Plugin Manifest

The manifest defines identity and metadata. In Claude Code, it lives at `.claude-plugin/plugin.json`:[^4]

```json
{
  "name": "code-quality",
  "description": "Automated linting and formatting after every file edit",
  "version": "1.0.0",
  "author": {
    "name": "Your Name",
    "url": "https://github.com/yourname"
  },
  "repository": "https://github.com/yourname/code-quality",
  "license": "MIT",
  "keywords": ["lint", "format", "quality"]
}
```

The `name` field is the only required field if a manifest is present. It doubles as the skill namespace — skills are invoked as `/code-quality:lint`, `/code-quality:format`, etc. Use kebab-case, no spaces, max 64 characters.[^4]

If the manifest is omitted entirely, Claude Code auto-discovers components in default locations and derives the name from the directory name.[^4]

## Designing Skills

Skills are the primary way to teach a coding agent new capabilities. A well-designed skill gives the model enough context to act correctly without overwhelming the token budget.[^1]

### Anatomy of a SKILL.md

Every skill needs a `SKILL.md` file with YAML frontmatter and markdown instructions:

```markdown
---
name: security-audit
description: >
  Scans code for security vulnerabilities including injection attacks,
  authentication flaws, and sensitive data exposure. Use when reviewing
  code for security, checking PRs for vulnerabilities, or when asked
  to audit security.
allowed-tools: Read, Grep, Glob, Bash(npm audit *)
context: fork
agent: Explore
---

# Security Audit

Scan the specified files or directories for security issues:

1. Check for injection vulnerabilities (SQL, XSS, command injection)
2. Verify authentication and authorization patterns
3. Look for hardcoded secrets or credentials
4. Review dependency vulnerabilities with `npm audit`
5. Report findings with severity, file path, and line number

Focus on actionable findings. Skip style issues.
```

### Description Engineering

The `description` field is the most consequential line in any skill. It controls when the model auto-invokes the skill.[^1]

**Effective descriptions follow three rules:**

1. **State what the skill does** — Lead with the action: "Scans code for security vulnerabilities" not "A skill for security."
2. **State when to use it** — Include explicit trigger conditions: "Use when reviewing PRs, checking code quality, or when asked to audit security."
3. **Avoid overlap** — If two skills have similar descriptions, the model will route unpredictably. Each skill's trigger conditions must be distinct.

**Warning:** Skill descriptions consume context budget. Claude Code allocates 2% of the context window (fallback: 16,000 characters) for all skill descriptions combined. If too many skills are loaded, some will be silently excluded. Run `/context` to check for warnings about excluded skills.[^1]

### Controlling Invocation

| Frontmatter Setting | Who Can Invoke | When Loaded |
|---------------------|----------------|-------------|
| *(default)* | You + Claude | Description always in context; full skill loads on invocation |
| `disable-model-invocation: true` | You only | Description not in context; loads when you invoke `/skill-name` |
| `user-invocable: false` | Claude only | Description always in context; not in `/` menu |

Use `disable-model-invocation: true` for any skill with side effects — deployments, git operations, Slack messages, database writes. The model should never autonomously trigger these.[^1]

### Restricting Tool Access

Use `allowed-tools` to limit what the model can do when a skill is active:

```yaml
---
name: safe-reader
description: Read and analyze files without making changes
allowed-tools: Read, Grep, Glob
---
```

This creates a read-only mode. The model can explore files but cannot edit, write, or run shell commands while this skill is active.[^1]

For shell commands, use pattern matching: `Bash(npm test *)` allows only commands starting with `npm test`.[^1]

### Using Arguments and Dynamic Context

Skills accept user input via `$ARGUMENTS` (full input) or `$ARGUMENTS[N]` / `$N` (positional):

```markdown
---
name: migrate-component
description: Migrate a UI component between frameworks
---

Migrate the $0 component from $1 to $2.
Preserve all existing behavior and tests.
```

Invoked as `/migrate-component SearchBar React Vue`.[^1]

For dynamic data injection, use `!` backtick syntax to run shell commands before the skill content reaches the model:

```markdown
## PR Context
- PR diff: !`gh pr diff`
- Changed files: !`gh pr diff --name-only`
```

The command output replaces the placeholder, so the model receives actual data rather than the command itself.[^1]

### Running Skills in Isolation

Add `context: fork` to run a skill in a sandboxed sub-agent that cannot see conversation history:

```yaml
---
name: deep-research
description: Research a topic thoroughly across the codebase
context: fork
agent: Explore
---
```

The `agent` field selects which sub-agent type executes (built-in options: `Explore`, `Plan`, `general-purpose`, or any custom agent from `agents/`). Forked skills return a summary to the main conversation.[^1]

## Designing Hooks

Hooks are event handlers that fire at specific points during a coding agent session. They automate validation, enforce guardrails, and inject context without manual intervention.

### Claude Code Hook Architecture

Hooks are defined in JSON with three levels of nesting:[^4]

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/lint-check.sh"
          }
        ]
      }
    ]
  }
}
```

**Level 1 — Event**: The lifecycle point (`PreToolUse`, `PostToolUse`, `Stop`, etc.).
**Level 2 — Matcher**: A regex filter for when the hook fires (`"Write|Edit"` matches either tool).
**Level 3 — Handler**: The shell command, prompt, or agent that runs.

### Hook Event Reference

| Event | When It Fires | Can Block? |
|-------|---------------|------------|
| `SessionStart` | Session begins or resumes | No |
| `UserPromptSubmit` | User submits a prompt, before processing | Yes |
| `PreToolUse` | Before a tool call executes | Yes |
| `PermissionRequest` | Permission dialog appears | Yes |
| `PostToolUse` | After a tool call succeeds | No (feedback only) |
| `PostToolUseFailure` | After a tool call fails | No (feedback only) |
| `Stop` | Claude finishes responding | Yes |
| `SubagentStart` / `SubagentStop` | Sub-agent lifecycle | Start: No, Stop: Yes |
| `TaskCompleted` | Task marked as completed | Yes |
| `PreCompact` | Before context compaction | No |
| `SessionEnd` | Session terminates | No |

### Hook Handler Types

| Type | Purpose | Key Fields |
|------|---------|------------|
| `command` | Run a shell script | `command`, `async`, `timeout` (default 600s) |
| `prompt` | Single-turn LLM evaluation | `prompt`, `model`, `timeout` (default 30s) |
| `agent` | Spawn a mini-agent with tools | `prompt`, `model`, `timeout` (default 60s) |

### Exit Code Protocol

Hook scripts communicate results through exit codes:[^4]

- **Exit 0**: Success. Parse stdout for JSON output.
- **Exit 2**: Blocking error. stderr is fed to Claude as an error message. Blocks the action for events that support it.
- **Any other code**: Non-blocking error. stderr shown in verbose mode only.

### Decision Control Patterns

For `PreToolUse`, return structured JSON to allow, deny, or escalate:

```bash
#!/bin/bash
COMMAND=$(jq -r '.tool_input.command' < /dev/stdin)

if [[ "$COMMAND" == *"rm -rf"* ]]; then
  jq -n '{
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: "Destructive command blocked"
    }
  }'
else
  exit 0
fi
```

For `PostToolUse`, `Stop`, and `UserPromptSubmit`, use top-level `decision`:

```json
{
  "decision": "block",
  "reason": "Tests must pass before stopping"
}
```

### OpenCode Hook Patterns

OpenCode hooks are event subscriptions returned from the plugin function:[^5]

```typescript
import type { Plugin } from "@opencode-ai/plugin"

export const QualityPlugin: Plugin = async ({ $, directory }) => {
  return {
    "tool.execute.before": async (input, output) => {
      if (input.tool === "read" && output.args.filePath.includes(".env")) {
        throw new Error("Reading .env files is not permitted")
      }
    },
    "tool.execute.after": async (input, output) => {
      if (input.tool === "write") {
        await $`npx eslint --fix ${output.args.filePath}`
      }
    },
    event: async ({ event }) => {
      if (event.type === "session.idle") {
        await $`osascript -e 'display notification "Done!" with title "opencode"'`
      }
    },
  }
}
```

Key OpenCode events include `tool.execute.before`, `tool.execute.after`, `session.idle`, `session.compacted`, `shell.env`, `message.updated`, and `lsp.client.diagnostics`.[^5]

## Building Custom Tools

Both platforms let plugins register tools that the LLM can invoke directly, alongside built-in tools.

### OpenCode Custom Tools

Use the `tool` helper with Zod schema validation:[^5]

```typescript
import { type Plugin, tool } from "@opencode-ai/plugin"

export const DatabasePlugin: Plugin = async (ctx) => {
  return {
    tool: {
      query_schema: tool({
        description: "Query the database schema for table and column information",
        args: {
          table_name: tool.schema.string(),
          include_indexes: tool.schema.boolean().optional(),
        },
        async execute(args, context) {
          const schema = await fetchSchema(args.table_name)
          return JSON.stringify(schema)
        },
      }),
    },
  }
}
```

**Important**: If a plugin tool shares a name with a built-in tool, the plugin tool takes precedence. Use unique, descriptive names to avoid accidental overrides.[^5]

### Claude Code Custom Tools

In Claude Code, custom tools are provided through MCP servers defined in `.mcp.json`:[^4]

```json
{
  "mcpServers": {
    "plugin-database": {
      "command": "${CLAUDE_PLUGIN_ROOT}/servers/db-server",
      "args": ["--config", "${CLAUDE_PLUGIN_ROOT}/config.json"],
      "env": {
        "DB_PATH": "${CLAUDE_PLUGIN_ROOT}/data"
      }
    }
  }
}
```

Plugin MCP servers start automatically when the plugin is enabled and appear as standard tools in the model's toolkit.[^4]

## Managing Context Efficiently

Token budget is the single most constrained resource in any agent session. Every plugin author must design with this constraint in mind.

### Token Budget Rules

1. **Provide file paths, not file contents** — Let the agent read what it needs on demand. Never dump entire files into skill instructions.
2. **Use targeted retrieval** — Grep for symbol usage first, then open only the 3–5 most relevant files.[^6]
3. **Batch operations in groups of 5–20 files** — Large enough for meaningful progress, small enough to fit in context.[^6]
4. **Keep skill instructions hierarchical** — Most critical guidance first. Add detail only where ambiguity would cause errors.

### Compaction Hooks

Long sessions eventually trigger context compaction. Without intervention, compaction discards context the plugin depends on.

**OpenCode** provides the `experimental.session.compacting` hook to inject persistent state:[^5]

```typescript
"experimental.session.compacting": async (input, output) => {
  output.context.push(`
## Persistent Plugin State

- Current task: migrating auth module to v2
- Modified files: src/auth/tokens.ts, src/auth/middleware.ts
- Tests passing: 47/52 (5 pending new assertions)
  `)
}
```

To replace the compaction prompt entirely, set `output.prompt` — this overrides both the default prompt and the `output.context` array.[^5]

**Claude Code** uses `PreCompact` hooks, which fire before compaction occurs. These cannot modify the compaction behavior but can log state or trigger side effects.[^4]

### Skill Description Budget

Claude Code allocates 2% of the context window for skill descriptions. With a 200K context window, that's ~4,000 tokens. Each installed plugin's skills consume part of this budget. If the total exceeds the limit, skills are silently excluded.[^1]

**Design implication**: Write concise descriptions (1–3 sentences max). Load detailed instructions in the skill body, not the description.

## Security and Permissions

### Principle of Least Privilege

Every plugin should start with the minimum permissions needed, then expand only after validation:[^6]

1. **Default to read-only** — Use `allowed-tools: Read, Grep, Glob` in skills. Expand to `Write`, `Edit`, `Bash` only after the agent demonstrates understanding of the codebase.
2. **Gate destructive operations** — Block `rm -rf`, `DROP TABLE`, force pushes, and similar commands via `PreToolUse` hooks.
3. **Protect secrets** — Block reads of `.env`, `.pem`, `credentials.json` and similar files.
4. **Scope shell access** — Use `Bash(npm test *)` patterns to allow only specific command prefixes rather than unrestricted shell access.

### Hook-Based Guardrails

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{
          "type": "command",
          "command": "${CLAUDE_PLUGIN_ROOT}/scripts/block-destructive.sh"
        }]
      },
      {
        "matcher": "Read",
        "hooks": [{
          "type": "command",
          "command": "${CLAUDE_PLUGIN_ROOT}/scripts/block-secrets.sh"
        }]
      }
    ]
  }
}
```

### Permission Scoping in Teams

| Scope | Settings File | Use Case |
|-------|---------------|----------|
| `user` | `~/.claude/settings.json` | Personal plugins across all projects |
| `project` | `.claude/settings.json` | Team plugins committed to version control |
| `local` | `.claude/settings.local.json` | Project-specific, gitignored |
| `managed` | Managed policy settings | Organization-wide, admin-controlled |

Enterprise administrators can set `allowManagedHooksOnly` to block user, project, and plugin hooks entirely.[^4]

## Automated Validation

Wire validation into every edit cycle so problems are caught immediately, not at review time.[^6]

### Post-Edit Validation Pattern

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path' | xargs npx eslint --fix"
          },
          {
            "type": "command",
            "command": "jq -r '.tool_input.file_path' | xargs npx prettier --write"
          }
        ]
      }
    ]
  }
}
```

### Task Completion Gate

Use `TaskCompleted` hooks to prevent the agent from marking work as done until tests pass:

```json
{
  "hooks": {
    "TaskCompleted": [
      {
        "hooks": [{
          "type": "command",
          "command": "npm test || exit 2"
        }]
      }
    ]
  }
}
```

Exit code 2 prevents the task from being marked complete. The stderr message is fed back to the agent as feedback.[^4]

### Stop Gate

Prevent the agent from finishing a response if validation fails:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [{
          "type": "agent",
          "prompt": "Check if all modified files pass linting and type checking. $ARGUMENTS. If any checks fail, return {\"decision\": \"block\", \"reason\": \"Fix lint/type errors before stopping\"}.",
          "timeout": 60
        }]
      }
    ]
  }
}
```

Agent-type hooks can use `Read`, `Grep`, and `Glob` tools to verify conditions before returning a decision.[^4]

## Preventing Plugin Bloat

Plugin bloat — accumulated overhead from too many plugins, overly broad hooks, and wasteful context consumption — is the primary way plugins go from productivity multiplier to productivity drag.

### Sources of Bloat

| Source | Mechanism | Impact |
|--------|-----------|--------|
| **Too many skills** | Skill descriptions exceed 2% context budget | Skills silently excluded; model loses awareness of capabilities |
| **Overlapping hooks** | Multiple plugins register for same events | All matching hooks run in parallel — more hooks = more latency per tool call |
| **Broad matchers** | Using `"*"` or omitting matcher | Hook fires on every tool call, including irrelevant ones |
| **Large skill bodies** | Dumping reference docs into `SKILL.md` | Full skill content loads into context on invocation, displacing working memory |
| **Duplicate plugins** | Local plugin + npm plugin with similar names | Both loaded separately, double the overhead[^5] |
| **Tool name collisions** | Plugin tool overrides built-in tool | Unexpected behavior; difficult to debug[^5] |

### Mitigation Strategies

**1. Audit your skill budget regularly.**
Run `/context` in Claude Code to check for warnings about excluded skills. If skills are being dropped, either reduce the number of installed plugins or shorten skill descriptions.[^1]

**2. Use precise matchers.**
Never use `"*"` or omit the matcher unless the hook genuinely needs to fire on every tool call. Prefer specific patterns:

```json
// ❌ Fires on every tool call
{ "matcher": "*" }

// ✅ Fires only on file modifications
{ "matcher": "Write|Edit" }

// ✅ Fires only on specific MCP tools
{ "matcher": "mcp__memory__.*" }
```

**3. Keep SKILL.md focused; use supporting files for detail.**
Put essential instructions in `SKILL.md` (loaded on invocation). Put reference docs, API specs, and examples in separate files (`reference.md`, `examples/`) that the model reads only when needed:[^1]

```
my-skill/
├── SKILL.md          # Core instructions (loaded on invocation)
├── reference.md      # Detailed API docs (loaded when needed)
├── examples.md       # Usage examples (loaded when needed)
└── scripts/
    └── validate.sh   # Executed, never loaded into context
```

**4. One purpose per plugin.**
If a plugin does linting and deployment and memory and notifications, it should be four plugins. Each plugin should be installable and removable independently without side effects.[^3]

**5. Prefer `async: true` for non-blocking hooks.**
Long-running hooks (logging, notifications, telemetry) should run in the background so they don't block the agent's workflow:

```json
{
  "type": "command",
  "command": "send-notification.sh",
  "async": true
}
```

**6. Use `once: true` for setup hooks in skills.**
If a hook only needs to run once per session (e.g., environment setup), set `once: true` to prevent it from firing repeatedly.[^4]

**7. Deduplicate before adding.**
Before creating a new plugin, check if an existing plugin already covers the use case. Check the Claude Code marketplace (`/plugin marketplace`), OpenCode ecosystem, awesome-opencode, and opencode.cafe.[^7][^8]

**8. Measure before expanding autonomy.**
Track PR cycle time, failure rates, and reverts. Increase plugin scope only when metrics stay stable for 4–6 weeks. If adding a new plugin causes regressions, remove it immediately.[^6]

## Testing and Debugging

### Local Testing

Both platforms support loading plugins from local directories during development:

```bash
# Claude Code
claude --plugin-dir ./my-plugin

# Multiple plugins
claude --plugin-dir ./plugin-one --plugin-dir ./plugin-two
```

For OpenCode, place files in `.opencode/plugins/` and restart.[^5]

### Debugging Commands

```bash
# Claude Code: full plugin loading details
claude --debug

# Inside Claude Code TUI
/debug         # Toggle debug mode
/hooks         # Interactive hook manager
/context       # Check context budget, skill loading
/plugin validate  # Validate manifest
```

Debug output shows which plugins loaded, any manifest errors, command/agent/hook registration, and MCP server initialization.[^4]

### Common Issues

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Plugin not loading | Invalid `plugin.json` | Run `claude plugin validate` or `/plugin validate` |
| Commands/skills not appearing | Components inside `.claude-plugin/` | Move to plugin root level |
| Hooks not firing | Script not executable | `chmod +x script.sh` |
| Path errors in hooks | Absolute paths used | Use `${CLAUDE_PLUGIN_ROOT}` or `$CLAUDE_PROJECT_DIR` |
| MCP server fails | Missing path variable | Use `${CLAUDE_PLUGIN_ROOT}` for all plugin paths |
| Skill not triggering | Description doesn't match user intent | Rewrite with explicit trigger conditions |
| Skill triggers too often | Description too broad | Narrow description or set `disable-model-invocation: true` |
| JSON parsing fails in hooks | Shell profile prints text on startup | Ensure stdout contains only JSON |

### Hook Script Debugging Checklist

1. Verify shebang line: `#!/bin/bash` or `#!/usr/bin/env bash`
2. Ensure script is executable: `chmod +x ./scripts/hook.sh`
3. Check path uses `${CLAUDE_PLUGIN_ROOT}`: `"command": "${CLAUDE_PLUGIN_ROOT}/scripts/hook.sh"`
4. Test manually: `echo '{"tool_name":"Bash","tool_input":{"command":"npm test"}}' | ./scripts/hook.sh`
5. Verify event name is case-sensitive: `PostToolUse`, not `postToolUse`
6. Confirm matcher regex matches your target tools[^4]

## Distribution

### Semantic Versioning

Follow `MAJOR.MINOR.PATCH`:[^4]

- **MAJOR**: Breaking changes (renamed skills, changed hook behavior)
- **MINOR**: New features (added skills, new hooks) that are backward-compatible
- **PATCH**: Bug fixes

Start at `1.0.0` for the first stable release. Use `0.x.y` while iterating. Use pre-release tags like `2.0.0-beta.1` for testing.[^4]

### README Requirements

Every distributed plugin needs a README with:

1. **What it does** — One paragraph maximum.
2. **How to install** — Exact commands for marketplace install or `--plugin-dir`.
3. **How to configure** — Any required environment variables, MCP servers, or settings.
4. **What it changes** — List of skills, hooks, and tools added. Users need to know what they're giving the agent access to.

### Claude Code Marketplace Distribution

Plugins are distributed via GitHub repositories with a `.claude-plugin/marketplace.json` catalog:[^9]

```bash
# Add a marketplace
/plugin marketplace add username/repo-name

# Install from marketplace
/plugin install my-plugin@marketplace-name

# Install to project scope (shared via git)
claude plugin install my-plugin@marketplace-name --scope project
```

### OpenCode npm Distribution

Publish to npm and reference in `opencode.json`:[^5]

```json
{
  "plugin": ["your-published-plugin"]
}
```

npm plugins are installed automatically via Bun at startup and cached in `~/.cache/opencode/node_modules/`.[^5]

## Iteration Workflow

The most reliable path from idea to distributed plugin follows a concrete progression:[^6]

1. **Start standalone** — Build skills and hooks directly in `.claude/` (Claude Code) or `.opencode/plugins/` (OpenCode). Test in read-only mode. Wire up linting and test gates.
2. **Prove it works** — Run for 1–2 weeks. Confirm the skill triggers correctly, hooks don't cause false positives, and context consumption stays reasonable.
3. **Convert to plugin** — Move files into a plugin directory structure. Add `plugin.json`. Test with `--plugin-dir`. Verify everything still works with namespaced skill names.
4. **Distribute** — Publish to a marketplace or npm. Add a README. Tag `1.0.0`. Monitor for issues from other users.
5. **Expand cautiously** — Add new skills or hooks only when metrics (cycle time, error rate, token usage) are stable. Remove anything that isn't actively useful.

---

## References

1. [Plugin Creation Guidelines | Claude Code Skill Architecture](https://mcpmarket.com/tools/skills/plugin-creation-guidelines)

2. [Building Claude Code Plugins: Architecture, Best Practices, and ...](https://aiskill.market/blog/claude-code-plugin-development)

3. [create-opencode-plugin | Skills Mark... - LobeHub](https://lobehub.com/skills/igorwarzocha-opencode-workflows-create-opencode-plugin) - Comprehensive guide for building AI workflows, agents, and workforce systems with AgenticFlow. Use w...

4. [Create plugins - Claude Code Docs](https://code.claude.com/docs/en/plugins) - Create custom plugins to extend Claude Code with skills, agents, hooks, and MCP servers.

5. [Plugins - OpenCode](https://opencode.ai/docs/plugins/) - Custom tools​​ The tool helper creates a custom tool that opencode can call. It takes a Zod schema f...

6. [Claude Code Plugin Best Practices for Large Codebases (2025)](https://skywork.ai/blog/claude-code-plugin-best-practices-large-codebases-2025/) - Pro-level guide to using the Claude Code Plugin in large codebases: setup, context engineering, safe...

7. [Ecosystem](https://opencode.ai/docs/ecosystem/) - Plugins ;  opencode-md-table-formatter , Clean up markdown tables produced by LLMs ; opencode-morph-...

8. [Discover and install prebuilt plugins through marketplacescode.claude.com › docs › discover-plugins](https://code.claude.com/docs/en/discover-plugins) - Find and install plugins from marketplaces to extend Claude Code with new commands, agents, and capa...

9. [Create and distribute a plugin marketplace - Claude Code Docs](https://code.claude.com/docs/en/plugin-marketplaces) - Build and host plugin marketplaces to distribute Claude Code extensions across teams and communities...

