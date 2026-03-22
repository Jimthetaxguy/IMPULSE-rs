---
status: active
phase: all
audience: builder
tags: [research, agent, harness]
last_updated: 2026-03-17
---

# Agent Harness Analysis: OpenCode, Claude Code, OneContext, Desloppify

> **Version:** 2.0 | **Status:** Research Complete | **Updated:** 2026-03-17
> **Purpose:** Deep analysis of agent integration points for Impulse
> **Builds on:** docs/archive/OPENCODE-INTEGRATION.md, docs/research/TOOL-STACK-ANALYSIS.md

---

## Executive Summary

This document is the single most critical research artifact in the Impulse project. It determines whether the MVP stays as an OpenCode plugin, pivots to Claude Code hooks, adopts OneContext, or pursues a multi-platform strategy.

**Key findings:**

1. **The SDK Gap is real and severe.** The impulse-plugin code assumes 4 hooks and a registration model that does not match the actual OpenCode Plugin SDK. There is no `session.end` hook. There is no `session.start` hook. There is no `sdk.on()` registration model. The impulse-plugin `PluginSDK` interface is entirely fabricated -- it matches nothing in the real SDK.

2. **OpenCode's actual plugin model is powerful but different.** Plugins are functions that receive a `PluginInput` and return a `Hooks` object. The hook model is an input/output mutation pattern, not an event listener pattern. System prompt injection works via `experimental.chat.system.transform`, not `appendSystemPrompt()`.

3. **Claude Code has a complete session lifecycle** with `SessionStart`, `SessionEnd`, `PreCompact`, `Stop`, and 12+ other hook events. It is more mature for the Impulse use case than OpenCode's plugin SDK. Claude Code hooks can inject context at session start, perform cleanup at session end, and preserve context through compaction.

4. **OneContext is complementary, not competitive.** It provides a UI and sharing layer that Impulse does not attempt. However, its dual Node+Python dependency (Node 16+, Python 3.11+) and v0.x status make it unsuitable as a foundation. It could consume Impulse's files as a data source in the future.

5. **Recommendation: Claude Code as primary target, OpenCode as secondary.** Claude Code hooks map 1:1 to Impulse's requirements with zero fabricated APIs. OpenCode support is achievable but requires a complete rewrite of the plugin registration and hook model.

---

## 1. OpenCode Plugin SDK -- Deep Analysis

### 1.1 Actual Hook Interface (from source)

**Source file:** `cloned-repos/opencode/packages/plugin/src/index.ts` (lines 148-234)

The OpenCode Plugin SDK defines a `Hooks` interface as a plain TypeScript object type. Each hook is a named method on this object. Here is the **complete** hook inventory with exact signatures:

```typescript
export interface Hooks {
  // === General Events ===
  event?: (input: { event: Event }) => Promise<void>
  config?: (input: Config) => Promise<void>

  // === Custom Tools ===
  tool?: { [key: string]: ToolDefinition }

  // === Auth ===
  auth?: AuthHook

  // === Chat Lifecycle ===
  "chat.message"?: (
    input: {
      sessionID: string;
      agent?: string;
      model?: { providerID: string; modelID: string };
      messageID?: string;
      variant?: string;
    },
    output: { message: UserMessage; parts: Part[] },
  ) => Promise<void>

  "chat.params"?: (
    input: {
      sessionID: string;
      agent: string;
      model: Model;
      provider: ProviderContext;
      message: UserMessage;
    },
    output: {
      temperature: number;
      topP: number;
      topK: number;
      options: Record<string, any>;
    },
  ) => Promise<void>

  "chat.headers"?: (
    input: {
      sessionID: string;
      agent: string;
      model: Model;
      provider: ProviderContext;
      message: UserMessage;
    },
    output: { headers: Record<string, string> },
  ) => Promise<void>

  // === Permission ===
  "permission.ask"?: (
    input: Permission,
    output: { status: "ask" | "deny" | "allow" },
  ) => Promise<void>

  // === Command ===
  "command.execute.before"?: (
    input: { command: string; sessionID: string; arguments: string },
    output: { parts: Part[] },
  ) => Promise<void>

  // === Tool Lifecycle ===
  "tool.execute.before"?: (
    input: { tool: string; sessionID: string; callID: string },
    output: { args: any },
  ) => Promise<void>

  "tool.execute.after"?: (
    input: { tool: string; sessionID: string; callID: string; args: any },
    output: { title: string; output: string; metadata: any },
  ) => Promise<void>

  // === Shell ===
  "shell.env"?: (
    input: { cwd: string; sessionID?: string; callID?: string },
    output: { env: Record<string, string> },
  ) => Promise<void>

  // === Experimental ===
  "experimental.chat.messages.transform"?: (
    input: {},
    output: {
      messages: { info: Message; parts: Part[] }[];
    },
  ) => Promise<void>

  "experimental.chat.system.transform"?: (
    input: { sessionID?: string; model: Model },
    output: { system: string[] },
  ) => Promise<void>

  "experimental.session.compacting"?: (
    input: { sessionID: string },
    output: { context: string[]; prompt?: string },
  ) => Promise<void>

  "experimental.text.complete"?: (
    input: { sessionID: string; messageID: string; partID: string },
    output: { text: string },
  ) => Promise<void>

  // === Tool Definition Modification ===
  "tool.definition"?: (
    input: { toolID: string },
    output: { description: string; parameters: any },
  ) => Promise<void>
}
```

**Total hooks:** 15 named hooks + `event` (generic bus subscription) + `config` + `tool` + `auth`

**Critical observation:** The hook model is an **input/output mutation pattern**, not an event listener pattern. Each hook receives an `input` (read-only context) and an `output` (mutable object that the plugin can modify). The calling code reads the mutated `output` after all plugins have run.

### 1.2 Plugin Registration Model

**Source file:** `cloned-repos/opencode/packages/opencode/src/plugin/index.ts`

The actual registration model is fundamentally different from what the impulse-plugin assumes.

**How plugins actually register:**

```typescript
// A plugin is a function that receives PluginInput and returns Hooks
export type Plugin = (input: PluginInput) => Promise<Hooks>

export type PluginInput = {
  client: ReturnType<typeof createOpencodeClient>  // SDK client for API calls
  project: Project                                   // Project metadata
  directory: string                                  // Project directory
  worktree: string                                   // Git worktree root
  serverUrl: URL                                     // OpenCode server URL
  $: BunShell                                        // Bun shell for commands
}
```

**How plugins are loaded** (from `Plugin` namespace, lines 24-98):

1. Internal built-in plugins are loaded directly: `CodexAuthPlugin`, `CopilotAuthPlugin`, `GitlabAuthPlugin`
2. External plugins are specified in `config.plugin` as npm package names (e.g., `"opencode-anthropic-auth@0.0.13"`)
3. npm packages are installed via `BunProc.install(pkg, version)`
4. The module is imported: `const mod = await import(plugin)`
5. All exported functions are called with `PluginInput` and the returned `Hooks` objects are collected
6. When a hook fires, `Plugin.trigger()` iterates all hooks and calls each matching handler

**How hooks are triggered** (from `Plugin.trigger()`, lines 101-116):

```typescript
export async function trigger<Name extends Exclude<keyof Required<Hooks>, "auth" | "event" | "tool">>(
  name: Name,
  input: Input,
  output: Output,
): Promise<Output> {
  if (!name) return output
  for (const hook of await state().then((x) => x.hooks)) {
    const fn = hook[name]
    if (!fn) continue
    await fn(input, output)  // Plugin mutates `output` in place
  }
  return output
}
```

**The event bus** (from `Plugin.init()`, lines 122-137):

```typescript
export async function init() {
  // ...
  Bus.subscribeAll(async (input) => {
    const hooks = await state().then((x) => x.hooks)
    for (const hook of hooks) {
      hook["event"]?.({ event: input })
    }
  })
}
```

This means every `Bus.publish()` call in the system is forwarded to all plugins via the `event` hook. The bus events include `session.created`, `session.updated`, `session.deleted`, `session.error`, `session.compacted`, and all message events.

### 1.3 SDK Gap Analysis (Assumed vs Actual)

This is the core finding. The impulse-plugin code makes assumptions that diverge fundamentally from the real SDK.

#### Gap 1: Plugin Registration Model (CRITICAL)

**Impulse-plugin assumes** (`impulse-plugin/src/index.ts`, lines 25-28):
```typescript
export interface PluginSDK {
  on: (event: string, handler: (...args: unknown[]) => Promise<void>) => void;
  getProjectRoot: () => string;
}
```

**Reality:** There is no `PluginSDK` interface. There is no `on()` method. There is no `getProjectRoot()`. Plugins are factory functions that return a `Hooks` object. The project root is available as `PluginInput.directory` or `PluginInput.worktree`.

**Impact:** The entire `register()` function in `impulse-plugin/src/index.ts` is incompatible. It must be rewritten from scratch.

#### Gap 2: `session.start` Hook (CRITICAL -- DOES NOT EXIST)

**Impulse-plugin assumes** (`impulse-plugin/src/index.ts`, line 39):
```typescript
sdk.on('session.start', async (ctx: unknown) => { ... });
```

**Reality:** There is no `session.start` hook in the OpenCode Plugin SDK. The closest alternatives are:
- `event` hook: receives all bus events, including `session.created` (fires when a session is created in the database, not when it starts prompting)
- `experimental.chat.system.transform`: fires before each LLM call, can modify the system prompt
- `chat.message`: fires when a new user message is submitted

**Workaround:** Use `experimental.chat.system.transform` to inject GENOME.md content into every system prompt. This is actually invoked on every LLM call (see `llm.ts` line 83-87), making it a reasonable substitute for session start injection.

#### Gap 3: `session.end` Hook (CRITICAL -- DOES NOT EXIST)

**Impulse-plugin assumes** (`impulse-plugin/src/index.ts`, line 47):
```typescript
sdk.on('session.end', async (ctx: unknown) => { ... });
```

**Reality:** There is no `session.end` hook. Sessions in OpenCode don't have a formal "end" event. A session is a database record that persists indefinitely. The bus events are:
- `session.created` -- when created
- `session.updated` -- when touched
- `session.deleted` -- when explicitly deleted (not the same as "ended")

There is no event for "user closed the terminal" or "agent finished responding."

**Workaround options:**
1. **Use the `event` hook** to listen for `session.deleted` events (partial -- only fires on explicit deletion)
2. **Use a heuristic** in `tool.execute.after`: if no tool calls for N minutes, trigger extraction (unreliable)
3. **Use a separate process** that watches for session staleness and triggers extraction
4. **Accept degraded session.end**: Run extraction on *next* session start instead of previous session end. When `experimental.chat.system.transform` fires, check if there's unprocessed history from the previous session and extract then.

#### Gap 4: SessionContext Interface (CRITICAL -- ENTIRELY FABRICATED)

**Impulse-plugin assumes** (`impulse-plugin/src/types.ts`, lines 42-52):
```typescript
export interface SessionContext {
  projectRoot: string;
  sessionId: string;
  agentId: string;
  appendSystemPrompt: (content: string) => void;
  getSessionTranscript: () => string;
  getModifiedFiles: () => string[];
  llm: {
    complete: (prompt: string) => Promise<string>;
  };
}
```

**Reality:** None of these methods exist in the OpenCode SDK. Search across the entire OpenCode codebase for `appendSystemPrompt`, `getSessionTranscript`, `getModifiedFiles`, and `injectPreCompaction` returned **zero results**.

What actually exists:
- **System prompt injection:** Modify the `output.system` array in `experimental.chat.system.transform`
- **Session transcript:** Use the SDK client (`PluginInput.client`) to call `client.session.messages({ sessionID })` via the REST API
- **Modified files:** Not directly available. Must be tracked manually via `tool.execute.after` (monitor file-editing tools)
- **LLM completion:** Use the SDK client to make API calls, or use an external LLM client

#### Gap 5: CompactionContext Interface (FABRICATED)

**Impulse-plugin assumes** (`impulse-plugin/src/types.ts`, lines 63-68):
```typescript
export interface CompactionContext {
  projectRoot: string;
  sessionId: string;
  agentId: string;
  injectPreCompaction: (content: string) => void;
}
```

**Reality:** The `experimental.session.compacting` hook receives:
```typescript
input: { sessionID: string }
output: { context: string[]; prompt?: string }
```
There is no `injectPreCompaction()`. Instead, plugins push strings into `output.context[]` or replace the entire compaction prompt via `output.prompt`.

#### Gap 6: ToolContext Interface (PARTIALLY WRONG)

**Impulse-plugin assumes** (`impulse-plugin/src/types.ts`, lines 54-61):
```typescript
export interface ToolContext {
  projectRoot: string;
  sessionId: string;
  agentId: string;
  toolName: string;
  toolArgs: Record<string, unknown>;
  lastMessage: string | undefined;
}
```

**Reality:** The `tool.execute.after` hook receives:
```typescript
input: { tool: string; sessionID: string; callID: string; args: any }
output: { title: string; output: string; metadata: any }
```
There is no `agentId`, no `projectRoot`, no `lastMessage`. The project root is available from the plugin's closure over `PluginInput.directory`. The agent name is not passed to tool hooks.

### 1.4 session.end Workarounds

Since OpenCode has no session.end hook, here are the viable strategies ranked by reliability:

#### Strategy A: "Extract on Next Start" (RECOMMENDED)

Run knowledge extraction when the *next* session's first `experimental.chat.system.transform` fires. Store a flag in LIVE_STATE.json indicating "extraction pending."

```
Session N ends (no hook fires)
Session N+1 starts:
  1. experimental.chat.system.transform fires
  2. Check LIVE_STATE.json for pending extraction
  3. Use SDK client to fetch Session N's messages
  4. Run LLM extraction
  5. Append to GENOME.md and HISTORY_INDEX.md
  6. Load fresh context for Session N+1
```

**Pros:** Reliable, no polling, no external process.
**Cons:** 1 session delay before knowledge is extracted. First session after restart is slower (extraction + context loading).

#### Strategy B: Bus Event Listener

Subscribe to all bus events via the `event` hook. Watch for `session.deleted` events or long gaps between `session.updated` events.

**Pros:** Catches explicit session deletions.
**Cons:** Sessions are rarely deleted. Terminal closure generates no event.

#### Strategy C: Process Signal Handler

Since the plugin runs inside the Bun process, register a `process.on('beforeExit')` handler.

**Pros:** Catches clean exits.
**Cons:** Doesn't catch `kill -9`, terminal closure, or crashes. Not part of the plugin API contract.

#### Strategy D: Periodic Extraction

Run extraction every N tool calls or every M minutes of inactivity detected in `tool.execute.after`.

**Pros:** Doesn't depend on session lifecycle.
**Cons:** May extract mid-session (partial/noisy data). Complex heuristic.

### 1.5 Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Plugin SDK is "experimental" -- hooks may change | HIGH | Pin to specific OpenCode version, test on upgrade |
| No session.end hook | HIGH | Strategy A: extract-on-next-start |
| Input/output mutation model may confuse developers | MEDIUM | Document clearly, add helper functions |
| `experimental.chat.system.transform` may be removed | MEDIUM | It's the only injection point; OpenCode must keep it or provide alternative |
| Plugin must be npm-published for non-local use | LOW | Can use `file://` path for local dev |
| Bun-only runtime (no Node.js) | LOW | Impulse is already Bun-only |
| SDK types use `any` extensively (e.g., tool args) | LOW | Add runtime validation with Zod |

---

## 2. Claude Code Hooks -- Deep Analysis

### 2.1 Hook Types and Configuration

Claude Code implements a comprehensive hook system with **16 hook events**, configured via JSON settings files.

**Configuration locations (in priority order):**
1. Managed policy settings (enterprise, highest priority)
2. `~/.claude/settings.json` (user-global)
3. `.claude/settings.json` (project, committed)
4. `.claude/settings.local.json` (project, gitignored)
5. Plugin `hooks/hooks.json` (when plugin enabled)
6. Skill/agent frontmatter (component lifecycle)

**Configuration format:**
```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/script.sh",
            "timeout": 600
          }
        ]
      }
    ]
  }
}
```

**Three hook handler types:**
1. **Command hooks** (`type: "command"`): Shell commands. Receive JSON on stdin, return decisions via stdout/exit codes.
2. **Prompt hooks** (`type: "prompt"`): Single-turn LLM evaluation. The model returns `{ "ok": true/false, "reason": "..." }`.
3. **Agent hooks** (`type: "agent"`): Multi-turn subagent with tool access (Read, Grep, Glob). Up to 50 turns.

**Complete hook event inventory:**

| Event | When | Can Block? | Matcher Target |
|-------|------|------------|----------------|
| `SessionStart` | Session begins/resumes | No | How started: `startup`, `resume`, `clear`, `compact` |
| `UserPromptSubmit` | User submits prompt | Yes | N/A (always fires) |
| `PreToolUse` | Before tool executes | Yes (allow/deny/ask) | Tool name: `Bash`, `Edit`, `Write`, etc. |
| `PermissionRequest` | Permission dialog appears | Yes | Tool name |
| `PostToolUse` | After tool succeeds | No (feedback only) | Tool name |
| `PostToolUseFailure` | After tool fails | No (feedback only) | Tool name |
| `Notification` | Notification sent | No | Type: `permission_prompt`, `idle_prompt`, etc. |
| `SubagentStart` | Subagent spawned | No (inject context) | Agent type |
| `SubagentStop` | Subagent finished | Yes (block stop) | Agent type |
| `Stop` | Main agent finished | Yes (block stop) | N/A (always fires) |
| `TeammateIdle` | Team member going idle | Yes (exit code 2) | N/A |
| `TaskCompleted` | Task marked complete | Yes (exit code 2) | N/A |
| `ConfigChange` | Config file changes | Yes (except policy) | Config source |
| `PreCompact` | Before compaction | No | Trigger: `manual`, `auto` |
| `SessionEnd` | Session terminates | No (cleanup only) | Reason: `clear`, `logout`, `prompt_input_exit`, etc. |

### 2.2 SessionStart and SessionEnd Capabilities

#### SessionStart

This is the most important hook for Impulse. When it fires:
- New session startup
- Session resume (`--resume`, `--continue`, `/resume`)
- After `/clear`
- After compaction

**Input received on stdin:**
```json
{
  "session_id": "abc123",
  "transcript_path": "/Users/.../.claude/projects/.../uuid.jsonl",
  "cwd": "/Users/.../my-project",
  "permission_mode": "default",
  "hook_event_name": "SessionStart",
  "source": "startup",
  "model": "claude-sonnet-4-6"
}
```

**Context injection:** Any text printed to stdout on exit 0 is added as context for Claude. Additionally, JSON output can include `additionalContext` field. This is the equivalent of `appendSystemPrompt()` -- and it actually works.

**Environment variable persistence:** SessionStart hooks have access to `CLAUDE_ENV_FILE` for writing environment variables that persist through the session.

**Impulse mapping:** Read GENOME.md, LIVE_STATE.json, and HISTORY_INDEX.md, format them, and print to stdout. Claude will see this context for the entire session.

#### SessionEnd

Fires when the session terminates. Supports matchers on exit reason:
- `clear` -- `/clear` command
- `logout` -- user logged out
- `prompt_input_exit` -- user exited from prompt
- `bypass_permissions_disabled`
- `other`

**Input received:**
```json
{
  "session_id": "abc123",
  "transcript_path": "/Users/.../.claude/projects/.../uuid.jsonl",
  "cwd": "/Users/...",
  "hook_event_name": "SessionEnd",
  "reason": "prompt_input_exit"
}
```

**Cannot block.** SessionEnd hooks are fire-and-forget. But they CAN:
- Read the transcript via `transcript_path`
- Run LLM extraction (call an API within the script)
- Write to GENOME.md and HISTORY_INDEX.md
- Clean up LIVE_STATE.json
- Log session statistics

**This is exactly what impulse-plugin's `onSessionEnd` needs.** The transcript path is provided directly -- no need for a fabricated `getSessionTranscript()` method.

### 2.3 JSONL History Format

Claude Code stores session data in JSONL format at predictable locations.

**Global history index:** `~/.claude/history.jsonl`
- Each line is a JSON object with prompt text, timestamp, project path, and session ID
- Serves as a log of every input across all projects

**Session files:** `~/.claude/projects/<encoded-path>/<session-uuid>.jsonl`
- Path encoding: forward slashes replaced with hyphens
- Full conversation history per session
- Contains: session metadata, user messages, Claude responses, tool uses, system summaries

**Summary files:** `~/.claude/projects/<encoded-path>/<summary-uuid>.jsonl`
- Compacted conversation summaries

**Impulse relevance:** The `transcript_path` provided to hooks points to the session JSONL file. A SessionEnd hook can read this file, parse the JSONL, extract decisions, and write to GENOME.md without needing any fabricated API.

### 2.4 System Prompt Injection (CLAUDE.md)

Claude Code has a built-in system prompt injection mechanism: **CLAUDE.md files**.

**Load order (all are injected into the system prompt):**
1. `~/.claude/CLAUDE.md` -- user-global instructions
2. `CLAUDE.md` in project root -- project instructions (committed)
3. `.claude/CLAUDE.md` -- project instructions (alternative location)
4. `CLAUDE.md` files in parent directories up to git root

**Impulse implication:** GENOME.md could potentially be loaded as a CLAUDE.md variant. However, CLAUDE.md is static (loaded at session start), while GENOME.md needs to be dynamically updated. A SessionStart hook is the better approach for injecting dynamic content from GENOME.md.

### 2.5 MCP Server Integration

Claude Code supports MCP (Model Context Protocol) servers, which provide tools to Claude.

**Adding an MCP server:**
```bash
claude mcp add <server-name> -- <command>
```

**Impulse as MCP server:** Instead of hooks, Impulse could expose its functionality via MCP tools:
- `impulse_read_genome` -- Read GENOME.md content
- `impulse_update_genome` -- Append a decision
- `impulse_read_history` -- Read recent session summaries
- `impulse_agent_status` -- Check active agents

**MCP tools appear in tool hooks:** `mcp__impulse__read_genome` would be matchable in PreToolUse/PostToolUse hooks.

**Assessment:** MCP is a complementary approach, not a replacement for hooks. Hooks provide lifecycle events (start/end); MCP provides on-demand tools. An ideal implementation uses both.

### 2.6 Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Hooks are shell commands (not in-process) | MEDIUM | Performance overhead per hook invocation. Keep scripts fast. |
| SessionEnd cannot block -- extraction may not complete | MEDIUM | Use async extraction. Write state to temp file; process on next start if incomplete. |
| JSON parsing from stdin can fail | LOW | Validate with `jq` or a parser; graceful degradation on parse failure |
| Hook configuration is snapshot-based (changes need review) | LOW | This is a feature, not a bug -- prevents mid-session tampering |
| No direct LLM access from hooks | MEDIUM | Must use external API call (curl to Anthropic/OpenAI) for extraction |
| Claude Code is Anthropic-specific | MEDIUM | Impulse-on-Claude-Code only works with Claude Code users |
| Hook timeout defaults (600s for command) | LOW | Plenty of time for file I/O operations |

---

## 3. OneContext -- Evaluation

### 3.1 Architecture and Capabilities

**What it is:** An "Agent Self-Managed Context layer" that provides a unified context for AI agents. Released February 7, 2026 (v0.1.0), latest release v0.8.3 (February 14, 2026).

**Installation:**
```bash
npm i -g onecontext-ai
```

**Dependencies:**
- Node.js >= 16
- Python >= 3.11 (with `uv`, `pipx`, `pip3`, or `pip`)
- The npm installer deploys a Python package backend

**Core capabilities:**
1. **Trajectory recording:** Records agent actions as they work
2. **Context sharing:** Share via Slack link; recipients can continue from same point
3. **Session import:** (v0.8.3) Import past Codex/Claude sessions as reusable context
4. **Multi-agent context:** Multiple agents share the same context state
5. **Session management:** Create, resume, close, archive sessions under named contexts

**Commands:** Available as `onecontext-ai`, `onecontext`, or `oc`

### 3.2 Overlap with Impulse

| Feature | Impulse | OneContext | Overlap? |
|---------|---------|-----------|----------|
| Cross-session memory | GENOME.md (permanent decisions) | Session continuity (full transcript replay) | Partial -- different approaches |
| Session summaries | HISTORY_INDEX.md (extracted summaries) | Auto-generated session summaries | Yes -- direct overlap |
| Multi-agent awareness | LIVE_STATE.json (file locks, activity) | Shared context (all agents see same state) | Partial -- different granularity |
| Context sharing | Git commit (GENOME.md) | Slack link sharing | No -- different distribution |
| Session import | N/A | Import Codex/Claude sessions | No overlap |
| UI/visualization | None (files only) | Web UI for context management | No overlap |
| LLM extraction | Session-end extraction to GENOME.md | No extraction (raw trajectory) | No overlap |
| Compaction survival | Compaction hook injects critical context | Not addressed | No overlap |

### 3.3 Complement vs Conflict Analysis

**Complement areas:**
- OneContext's session import could consume HISTORY_INDEX.md entries
- OneContext's sharing UI could distribute GENOME.md to team members
- OneContext's trajectory recording could feed into Impulse's extraction pipeline
- OneContext could provide the "Phase 3 UI" that Impulse defers

**Conflict areas:**
- Both maintain session state, but in different formats
- OneContext's "unified context" could override Impulse's curated GENOME.md
- Dual dependency (Node + Python) conflicts with Impulse's "Bun-only" constraint

### 3.4 Verdict

**Do not adopt OneContext as a foundation.** The reasons:

1. **Dual dependency** (Node + Python) violates Impulse's "Bun-only, single dep" constraint
2. **v0.x status** (2 weeks old, 12 commits) -- too immature for foundation use
3. **Philosophical mismatch**: OneContext replays full trajectories; Impulse extracts and curates knowledge. These are different approaches to memory.
4. **No hook system**: OneContext doesn't provide lifecycle hooks; it IS the lifecycle layer. Impulse needs to hook into existing agents, not replace them.

**Future integration path** (Phase 3+): OneContext could import from HISTORY_INDEX.md and provide a team collaboration UI. This is additive, not foundational.

---

## 4. Desloppify -- Evaluation

### 4.1 Architecture and Purpose

**What it is:** A multi-language codebase health scanner and cleanup orchestrator (v0.9.10). Python-first (99.5%), MIT license. Designed to systematically improve code quality across 29 languages by combining mechanical detection (dead code, duplication, complexity) with LLM-driven subjective analysis (naming, abstractions, module boundaries).

**Key design principle:** "The primary user is an AI coding agent, not a human." This drives all architecture decisions.

### 4.2 Layered Architecture

Desloppify uses a strict 5-layer stack with enforced import direction (higher depends on lower, never reverse):

```
Layer 4: Interface          (app/ - thin CLI entry points)
Layer 3: Language Plugins   (python/, typescript/, rust/, etc.)
Layer 2: Framework          (languages/_framework/ - standardized contracts)
Layer 1: Algorithms         (engine/detectors/ - language-agnostic)
Layer 0: Foundation         (base/ - paths, config, enums, utilities)
```

**Key components:**

| Component | Purpose |
|-----------|---------|
| **Engine/Detectors** | Language-agnostic analysis algorithms |
| **Scoring** | Quality rating with anti-gaming safeguards |
| **Living Plan** | State machine driving 5-phase workflow (Scan → Review → Workflow → Triage → Execute) |
| **Policy** | Rules and constraints for decision-making |
| **State** | Persistent storage managing queue lifecycle |

### 4.3 The Living Plan Engine

Desloppify enforces a **5-phase lifecycle** via state machine:

1. **Scan** -- Identify issues, populate queue
2. **Review** -- Subjective LLM-driven assessment
3. **Workflow** -- Score communication, import findings
4. **Triage** -- Expose priority stages
5. **Execute** -- Apply objective fixes, re-scan

**Safety net pattern:** Even though lifecycle phase is persisted in `plan.refresh_state.lifecycle_phase`, the system re-resolves the correct phase from visible items on load. Persisted state is never trusted alone -- it's validated against source of truth.

### 4.4 Anti-Gaming Mechanism

The scoring system resists manipulation:
- Subjective findings weighted at **75%**, objective at **25%**
- Scores only improve through genuine code improvement
- Cross-checking prevents score anchoring
- Agents don't see target scores during review ("blind packet" system)

### 4.5 Agent Integration Model

Desloppify is a **CLI-based orchestrator**, not an SDK:

```bash
desloppify scan --path .           # Identify issues
desloppify next                    # Get next priority item
desloppify plan triage --stage ... # Manage workflow
desloppify resolve                 # Mark work complete
```

**Delegation pattern** (Hermes integration):
- Spawns **isolated child agents** (up to 3 concurrent via ThreadPoolExecutor)
- Each child receives: dedicated prompt file, blind packet (context without score targets), JSON output file
- Children don't inherit parent context -- explicit information boundaries
- MAX_DEPTH = 2 (prevents grandchild recursion)

**Execution philosophy -- "THE LOOP":**
```bash
desloppify next   # Get ONE priority item
# Fix it
desloppify resolve
# Repeat
```

Queue is execution-focused (one item at a time), not backlog-focused.

### 4.6 State Management

State lives in `.desloppify/`:
- `plan.json` -- Active workflow state
- `review_packet_blind.json` -- Blinded review data (no score targets)
- `query.json` -- Dimension definitions for scoring
- `results/` -- Agent outputs

**Key constraint:** No automatic completion. Work completion requires explicit `resolve`, `skip --permanent`, or `reopen` commands. Automated scans can only "add new work, reopen previously completed work, or corroborate existing resolutions."

### 4.7 Patterns Relevant to Impulse

#### Pattern 1: Agent-First Design
Desloppify's "primary user is an AI agent" principle aligns with Impulse's sidecar model. Prioritize structured output and reliable state over human UX.

#### Pattern 2: State Re-Validation on Load
Never trust persisted state alone. Re-resolve from visible items on load. Impulse should apply this to `LIVE_STATE.json` -- validate against actual `.impulse/` contents on session resume.

#### Pattern 3: Anti-Anchoring (Blind Packets)
When replaying context to agents, separate evidence (what was done) from guidance (what should be done). Don't show agents their previous decisions for the same action. Impulse should surface file changes and tool usage without anchoring agents to prior conclusions.

#### Pattern 4: Explicit State Transitions
Work completion requires explicit `resolve` -- no silent completions. Impulse should require agents to explicitly mark actions complete rather than inferring completion from file modifications.

#### Pattern 5: Phase-Enforced Workflow
The 5-phase lifecycle ensures work progresses through assessment before improvement. Impulse could enforce: Record → Execute → Verify → Persist as an auditable contract between agent and sidecar.

#### Pattern 6: Strict Layer Boundaries
The 5-layer import-direction enforcement prevents coupling. Impulse's 4-crate workspace (CLI, ops, terminal, GUI) should enforce similar boundaries with clear contracts between layers.

#### Pattern 7: Execution Queue (One Item at a Time)
`desloppify next` returns exactly one priority item. This prevents agent context overflow and ensures focused execution. Impulse's context injection should follow the same principle -- inject the most relevant context, not everything.

### 4.8 Overlap Analysis

| Feature | Impulse | Desloppify | Overlap? |
|---------|---------|------------|----------|
| Cross-session memory | GENOME.md (permanent decisions) | plan.json (workflow state) | Partial -- different scopes |
| Quality tracking | Not primary focus | Core feature (scoring + anti-gaming) | No overlap |
| Agent orchestration | Sidecar (observes agents) | Orchestrator (directs agents) | Complementary |
| State persistence | `.impulse/` directory | `.desloppify/` directory | Pattern overlap |
| LLM integration | Session extraction | Subjective review scoring | Different purposes |
| File change tracking | LIVE_STATE.json | Scan-based detection | Different mechanisms |
| Multi-agent support | Session tracking | Isolated child delegation | Different models |

### 4.9 Verdict

**Do not adopt desloppify as a foundation.** The reasons:

1. **Different problem domain**: Desloppify orchestrates code quality improvements; Impulse records agent actions. Desloppify is directive; Impulse is observational.
2. **Python-only**: 99.5% Python, no Rust integration path.
3. **Orchestrator vs Sidecar**: Desloppify tells agents what to do; Impulse remembers what agents did. These are fundamentally different roles.

**Patterns to borrow:**
- State re-validation on load (safety net pattern)
- Anti-anchoring via blind packets (context injection design)
- Explicit state transitions (no silent completions)
- Phase-enforced workflow (auditable lifecycle)
- Agent-first design philosophy
- Execution queue discipline (one priority item, not backlog dump)

**Future integration path:** Desloppify could consume Impulse's `HISTORY.jsonl` to identify which code areas agents frequently touch (hotspot analysis), or Impulse could record desloppify scan results as session context.

---

## 5. Comparison Matrix

| Dimension | OpenCode Plugin SDK | Claude Code Hooks | OneContext | Desloppify |
|-----------|-------------------|-------------------|-----------|------------|
| **Session start hook** | NO (use `experimental.chat.system.transform`) | YES (`SessionStart`) | N/A | N/A (CLI orchestrator) |
| **Session end hook** | NO (no lifecycle event) | YES (`SessionEnd`) | N/A | N/A |
| **Tool-after hook** | YES (`tool.execute.after`) | YES (`PostToolUse`) | N/A | N/A |
| **Compaction hook** | YES (`experimental.session.compacting`) | YES (`PreCompact`) | N/A | N/A |
| **System prompt injection** | YES (modify `output.system[]`) | YES (stdout from SessionStart) | N/A | YES (prompt files per task) |
| **Transcript access** | YES (via SDK client REST API) | YES (`transcript_path` in input) | YES (trajectory recording) | NO (agent output via JSON files) |
| **LLM access from plugin** | YES (via SDK client or external) | NO (must call external API) | N/A | YES (delegated subagent calls) |
| **Block tool execution** | YES (`tool.execute.before` modifies args) | YES (`PreToolUse` can deny) | N/A | NO |
| **Read/write file access** | YES (in-process, full filesystem) | YES (shell command, full filesystem) | YES | YES (CLI, full filesystem) |
| **Plugin registration** | npm package or `file://` path | JSON settings file | npm global install | pip install |
| **Execution model** | In-process async functions | Out-of-process shell commands | Standalone process | CLI commands (THE LOOP) |
| **Latency** | Sub-millisecond (in-process) | ~10-50ms (process spawn) | N/A | N/A (batch-oriented) |
| **Language** | TypeScript (Bun) | Any (shell commands) | Node + Python | Python (99.5%) |
| **Maturity** | Experimental (hooks may change) | Stable (documented, 16 events) | v0.x (2 weeks old) | v0.9.10 (active development) |
| **User base** | Growing (open source) | Large (Anthropic-backed) | Small (1.1k stars) | Growing (agent-focused) |
| **Multi-agent support** | Plugins run per-instance | Hooks run per-session | Context shared across agents | Isolated child delegation (max 3) |
| **Stop/Continue control** | NO | YES (`Stop` hook can block) | NO | YES (explicit resolve/skip) |
| **State re-validation** | NO | NO | NO | YES (re-resolve phase from items) |
| **Anti-gaming** | NO | NO | NO | YES (blind packets, cross-check) |

---

## 6. The SDK Gap -- Detailed Analysis

This section maps every assumption in the impulse-plugin code to reality.

### 6.1 File-by-File Gap Map

#### `impulse-plugin/src/index.ts` -- Plugin Entry

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 25-28 | `PluginSDK` with `on()` and `getProjectRoot()` | Plugin is `(input: PluginInput) => Promise<Hooks>` | CRITICAL |
| 33 | `register(sdk, userConfig)` function | Must export a function matching `Plugin` type | CRITICAL |
| 39 | `sdk.on('session.start', ...)` | No session.start hook exists | CRITICAL |
| 43 | `sdk.on('tool.execute.after', ...)` | Return `{ "tool.execute.after": fn }` in Hooks object | HIGH |
| 47 | `sdk.on('session.end', ...)` | No session.end hook exists | CRITICAL |
| 50 | `sdk.on('experimental.session.compacting', ...)` | Return `{ "experimental.session.compacting": fn }` in Hooks object | HIGH |

#### `impulse-plugin/src/types.ts` -- Context Interfaces

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 42-52 | `SessionContext` with `appendSystemPrompt()`, `getSessionTranscript()`, `getModifiedFiles()`, `llm.complete()` | None of these exist. System prompt via `output.system[]` mutation. Transcript via REST API. No built-in LLM access. | CRITICAL |
| 54-61 | `ToolContext` with `agentId`, `lastMessage` | `tool.execute.after` input has `tool`, `sessionID`, `callID`, `args` | HIGH |
| 63-68 | `CompactionContext` with `injectPreCompaction()` | Output is `{ context: string[]; prompt?: string }` -- push to context array | HIGH |

#### `impulse-plugin/src/hooks/session-start.ts` -- Context Loading

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 20-22 | Receives `SessionContext` with `projectRoot` | No session.start hook. Must use `experimental.chat.system.transform` which receives `{ sessionID?: string; model: Model }` | CRITICAL |
| 89 | `ctx.appendSystemPrompt(injection)` | Push string to `output.system[]` | HIGH |

#### `impulse-plugin/src/hooks/session-end.ts` -- Knowledge Extraction

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 42-45 | Receives `SessionContext` at session end | No session.end hook exists | CRITICAL |
| 52 | `ctx.getSessionTranscript()` | Must use SDK client: `client.session.messages({ sessionID })` | HIGH |
| 53 | `ctx.getModifiedFiles()` | Must track manually via `tool.execute.after` | MEDIUM |
| 61 | `ctx.llm.complete(prompt)` | Must use external LLM client (not provided by SDK) | HIGH |

#### `impulse-plugin/src/hooks/tool-after.ts` -- Live State Update

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 27-30 | Receives `ToolContext` with `toolArgs`, `agentId`, `lastMessage` | Receives `input: { tool, sessionID, callID, args }` and `output: { title, output, metadata }` | HIGH |
| 32 | `extractFilePaths(ctx.toolArgs)` | Must use `input.args` (the actual tool arguments) | MEDIUM |

#### `impulse-plugin/src/hooks/compaction.ts` -- Context Preservation

| Line | Assumed | Actual | Severity |
|------|---------|--------|----------|
| 22-25 | Receives `CompactionContext` with `projectRoot`, `injectPreCompaction()` | Receives `input: { sessionID }`, `output: { context: string[], prompt?: string }` | HIGH |
| 45-49 | `ctx.injectPreCompaction(text)` | `output.context.push(text)` | MEDIUM |

### 6.2 Summary of Required Changes for OpenCode Target

To make impulse-plugin work with OpenCode's actual SDK:

1. **Rewrite `index.ts` completely.** Replace `register()` with a default-exported factory function matching `Plugin` type.
2. **Replace `SessionContext`, `ToolContext`, `CompactionContext`** with types matching actual hook signatures.
3. **Replace `session.start`** with `experimental.chat.system.transform` (fires on every LLM call, not just session start -- needs deduplication logic).
4. **Replace `session.end`** with "extract on next start" strategy.
5. **Add modified-file tracking** to `tool.execute.after` (accumulate file paths across calls).
6. **Add external LLM client** for extraction (the SDK does not provide one).
7. **Get project root from closure** over `PluginInput.directory` instead of `ctx.projectRoot`.

**Estimated effort:** 3-5 days of focused work. The hook logic (file I/O, extraction, formatting) is reusable; only the integration layer needs rewriting.

---

## Key Findings

1. **The impulse-plugin code is a prototype against a fabricated SDK.** Every interface (`PluginSDK`, `SessionContext`, `ToolContext`, `CompactionContext`) and 2 of 4 hook names (`session.start`, `session.end`) do not exist in OpenCode's actual Plugin SDK. This is not a minor mismatch -- it's a complete impedance mismatch.

2. **OpenCode's plugin model is an input/output mutation pattern.** Hooks don't receive events; they receive mutable output objects and modify them. This is fundamentally different from the event-listener pattern the impulse-plugin assumes.

3. **Claude Code's hook system maps 1:1 to Impulse's needs.** `SessionStart` = load context. `PostToolUse` = track file activity. `SessionEnd` = extract knowledge. `PreCompact` = preserve context. All four impulse hooks have direct Claude Code equivalents with stable, documented APIs.

4. **The `session.end` gap in OpenCode is the single biggest architectural risk.** Without it, knowledge extraction must be deferred to the next session or implemented via heuristics. Neither is ideal.

5. **Claude Code provides `transcript_path` directly to hooks.** This eliminates the need for `getSessionTranscript()` -- the hook script can simply read the JSONL file.

6. **OneContext solves different problems than Impulse.** It's a context replay and sharing tool, not a knowledge extraction and curation tool. They are complementary, not competitive.

7. **Desloppify demonstrates agent-first design principles.** Its "blind packet" anti-anchoring, state re-validation safety net, explicit state transitions (no silent completions), and phase-enforced workflow are patterns worth borrowing. However, it's an orchestrator (tells agents what to do), not a sidecar (remembers what agents did), making it complementary rather than foundational.

---

## Implications for Impulse

### Architecture Decision Required

The impulse-plugin code must be significantly reworked regardless of platform choice:

- **For OpenCode:** Rewrite registration model, replace 2 missing hooks with workarounds, replace 4 fabricated interfaces, add external LLM client. Estimated 3-5 days.
- **For Claude Code:** Rewrite as shell scripts or a Node/Bun CLI that reads stdin JSON and writes stdout. The hook logic (file ops, extraction, formatting) from the existing code is largely reusable. Estimated 2-3 days.
- **For both (recommended):** Create a shared core library for file operations, extraction, and formatting. Write thin platform adapters for each target.

### The Session End Problem

This is the most consequential finding. OpenCode lacks session.end; Claude Code has it.

- If Impulse targets only OpenCode: Must implement "extract on next start" (1 session delay, slower first prompt)
- If Impulse targets only Claude Code: Clean session.end extraction, no delay
- If Impulse targets both: Must implement both strategies. The core extraction logic is shared, but the trigger mechanism differs.

### LLM Access for Extraction

- OpenCode: Plugin runs in-process (Bun). Can use any npm LLM client (e.g., `@ai-sdk/openai`, `@anthropic-ai/sdk`). Or use the SDK client's REST API.
- Claude Code: Hooks are shell commands. Must use `curl` or a CLI tool to call LLM APIs. Alternatively, use a `type: "agent"` hook (spawns a subagent with tool access) to perform extraction using Claude itself.

---

## Recommended Platform Strategy

### Phase 1: Claude Code Primary

Build Impulse as **Claude Code hooks first**. Rationale:
- 1:1 hook mapping (no workarounds needed)
- SessionStart + SessionEnd cover the full lifecycle
- `transcript_path` eliminates transcript access fabrication
- Stable, documented API with 16 hook events
- Larger user base (Claude Code is more widely used than OpenCode)
- Shell-command model means the core logic can be written in any language

**Implementation approach:**
```
.claude/hooks/
  impulse-session-start.sh    # Read 3 files, print context to stdout
  impulse-post-tool-use.sh    # Update LIVE_STATE.json
  impulse-session-end.sh      # Read transcript, extract decisions, update files
  impulse-pre-compact.sh      # Inject GENOME.md critical lines

.claude/settings.json or .claude/settings.local.json:
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "..." }] }],
    "PostToolUse": [{ "matcher": "Write|Edit|Bash", "hooks": [{ "type": "command", "command": "..." }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "..." }] }],
    "PreCompact": [{ "hooks": [{ "type": "command", "command": "..." }] }]
  }
}
```

The shell scripts can invoke a compiled Bun binary or `bun run` to reuse the TypeScript logic from impulse-plugin.

### Phase 1.5: OpenCode Adapter

After Claude Code hooks are proven, build the OpenCode adapter:
- Rewrite `index.ts` as a proper `Plugin` factory function
- Map `experimental.chat.system.transform` to session-start logic
- Map `tool.execute.after` to live-state tracking
- Map `experimental.session.compacting` to compaction logic
- Implement "extract on next start" for the session.end gap

### Phase 2+: Shared Core, Platform Adapters

```
impulse-core/              # Shared TypeScript library
  src/file-ops.ts          # File I/O (reused from current code)
  src/extraction.ts        # LLM extraction (reused)
  src/formatting.ts        # GENOME/HISTORY formatting (reused)
  src/types.ts             # Core types (rewritten)

impulse-claude-code/       # Claude Code hook scripts
  hooks/session-start.sh
  hooks/post-tool-use.sh
  hooks/session-end.sh
  hooks/pre-compact.sh

impulse-opencode/          # OpenCode plugin
  src/index.ts             # Plugin factory (new)
  src/adapter.ts           # Maps OpenCode hooks to core
```

### Decision: Do NOT adopt OneContext now

OneContext is too immature and its dual-dependency model conflicts with Impulse's constraints. Revisit in Phase 3 when a collaboration UI is needed.

---

*This document was produced by analyzing the actual source code of OpenCode's Plugin SDK (`packages/plugin/src/index.ts`), OpenCode's plugin loader (`packages/opencode/src/plugin/index.ts`), OpenCode's session management (`packages/opencode/src/session/`), the impulse-plugin implementation (`impulse-plugin/src/`), Claude Code's official hook documentation, OneContext's README and documentation, and desloppify's architecture and release notes.*

*Sources consulted:*
- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [OneContext GitHub Repository](https://github.com/TheAgentContextLab/OneContext)
- [OneContext Documentation](https://github.com/TheAgentContextLab/OneContext/blob/main/Documentation.md)
- [Claude Code JSONL History](https://kentgigger.com/posts/claude-code-conversation-history)
- [Claude Code Hook Mastery](https://github.com/disler/claude-code-hooks-mastery)
- [Desloppify v0.9.10 Release](https://github.com/peteromallet/desloppify/releases/tag/v0.9.10)
- [Desloppify GitHub Repository](https://github.com/peteromallet/desloppify)
