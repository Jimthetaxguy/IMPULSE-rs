---
title: 'OpenCode Plugin SDK -- Pattern Extraction'
status: active
phase: '1.5'
audience: builder
tags: [opencode, plugin-sdk, tool-definition, event-system, reference-pattern]
source_repo: https://github.com/anomalyco/opencode
source_version: '~1.2.9 (@opencode-ai/plugin)'
extracted: 2026-02-21
---

# OpenCode Plugin SDK -- Pattern Extraction

> This document captures the architecture, patterns, and interfaces from the
> OpenCode plugin SDK so we can build a Impulse Phase 1.5 adapter without
> needing the repo on disk. Every code snippet is verbatim from the source.

---

## 1. Architecture Overview

### Monorepo Structure

OpenCode is a Bun-based TypeScript monorepo managed with Turbo. Key packages:

```
opencode/
  packages/
    plugin/          <-- @opencode-ai/plugin  (the SDK -- what third parties import)
    opencode/        <-- The core CLI/server (consumes the plugin SDK)
    sdk/             <-- @opencode-ai/sdk  (client SDK for API access)
    app/             <-- Web frontend (SolidJS)
    desktop/         <-- Tauri desktop app
    console/         <-- Console UI assets
    ui/              <-- Shared UI components
    extensions/      <-- Extension system
    ...
```

The `plugin` package is tiny (4 files, ~235 lines total) and deliberately
minimal. It defines **types only** -- no runtime logic beyond a `tool()`
factory function. The heavy lifting happens in `packages/opencode/src/plugin/`
which loads, initializes, and triggers plugin hooks.

### Client-Server Architecture

OpenCode runs a local HTTP server (Hono-based) and the TUI/web/desktop are all
clients. Plugins operate server-side and receive a `client` (SDK) instance that
hits the local server via internal fetch.

### Runtime

- **Bun** (>= 1.3.9) is the only supported runtime
- `Bun.$` shell is passed directly to plugins as `$`
- All validation uses **Zod 4** (catalog version: 4.1.8)

---

## 2. Plugin Interface

### The Plugin Function Signature

A plugin is an async function that receives context and returns a `Hooks` object:

```typescript
// packages/plugin/src/index.ts

export type PluginInput = {
  client: ReturnType<typeof createOpencodeClient>;
  project: Project;
  directory: string;
  worktree: string;
  serverUrl: URL;
  $: BunShell;
};

export type Plugin = (input: PluginInput) => Promise<Hooks>;
```

**Key context provided to every plugin:**

| Field       | Type       | Purpose                                        |
| ----------- | ---------- | ---------------------------------------------- |
| `client`    | SDK client | Full API access to sessions, auth, etc.        |
| `project`   | `Project`  | Current project metadata                       |
| `directory` | `string`   | Project directory (cwd)                        |
| `worktree`  | `string`   | Git worktree root                              |
| `serverUrl` | `URL`      | Local server URL (e.g., http://localhost:4096) |
| `$`         | `BunShell` | Bun's tagged template shell for subprocesses   |

### Plugin Registration

Plugins are specified in `opencode.json(c)` as an array of strings:

```jsonc
{
  "plugin": [
    "my-npm-plugin@1.0.0", // npm package
    "file:///path/to/local-plugin.ts", // local file
  ],
}
```

Plugins can also live as `.ts`/`.js` files in `.opencode/plugins/` directory
and are auto-discovered.

### Plugin Loading (Core Side)

From `packages/opencode/src/plugin/index.ts`:

```typescript
export namespace Plugin {
  const BUILTIN = ["opencode-anthropic-auth@0.0.13"]

  // Built-in plugins that are directly imported (not installed from npm)
  const INTERNAL_PLUGINS: PluginInstance[] = [
    CodexAuthPlugin,
    CopilotAuthPlugin,
    GitlabAuthPlugin,
  ]

  // ... initialization:
  // 1. Load INTERNAL_PLUGINS first
  // 2. Wait for npm dependencies if config has plugins
  // 3. For each plugin string:
  //    - If not file://, resolve as npm package via BunProc.install()
  //    - Dynamic import the resolved path
  //    - Deduplicate by function reference (handles default + named export)
  //    - Call fn(input) to get Hooks
  //    - Push to hooks array
```

**Deduplication is important:** If a module exports the same function as both
`export default` and `export const MyPlugin`, it only gets initialized once.

### Example Plugin (from the repo)

```typescript
// packages/plugin/src/example.ts
import { Plugin } from './index';
import { tool } from './tool';

export const ExamplePlugin: Plugin = async (ctx) => {
  return {
    tool: {
      mytool: tool({
        description: 'This is a custom tool',
        args: {
          foo: tool.schema.string().describe('foo'),
        },
        async execute(args) {
          return `Hello ${args.foo}!`;
        },
      }),
    },
  };
};
```

---

## 3. Tool Definition Pattern

### The `tool()` Factory

Tools are defined with a minimal factory that wraps Zod schemas:

```typescript
// packages/plugin/src/tool.ts
import { z } from 'zod';

export type ToolContext = {
  sessionID: string;
  messageID: string;
  agent: string;
  directory: string; // Project directory
  worktree: string; // Worktree root
  abort: AbortSignal;
  metadata(input: { title?: string; metadata?: { [key: string]: any } }): void;
  ask(input: AskInput): Promise<void>;
};

type AskInput = {
  permission: string;
  patterns: string[];
  always: string[];
  metadata: { [key: string]: any };
};

export function tool<Args extends z.ZodRawShape>(input: {
  description: string;
  args: Args;
  execute(args: z.infer<z.ZodObject<Args>>, context: ToolContext): Promise<string>;
}) {
  return input;
}
tool.schema = z; // Exposes Zod as tool.schema for convenience

export type ToolDefinition = ReturnType<typeof tool>;
```

**Key observations:**

1. **Args are Zod raw shapes** -- not `z.object()`, just the shape object.
   The wrapping into `z.object()` happens at registration time in the core.
2. **Return type is `string`** -- tools return plain text, not structured data.
3. **`tool.schema`** is just `z` re-exported for convenience so plugins can
   do `tool.schema.string()` without importing Zod separately.
4. **Permission system**: Tools can call `context.ask()` to request user
   permission before performing sensitive operations.

### Core Tool Registration (how plugins' tools become LLM tools)

From `packages/opencode/src/tool/registry.ts`:

```typescript
function fromPlugin(id: string, def: ToolDefinition): Tool.Info {
  return {
    id,
    init: async (initCtx) => ({
      parameters: z.object(def.args), // <-- wraps the raw shape
      description: def.description,
      execute: async (args, ctx) => {
        const pluginCtx = {
          ...ctx,
          directory: Instance.directory,
          worktree: Instance.worktree,
        } as unknown as PluginToolContext;
        const result = await def.execute(args as any, pluginCtx);
        const out = await Truncate.output(result, {}, initCtx?.agent);
        return {
          title: '',
          output: out.truncated ? out.content : result,
          metadata: {
            truncated: out.truncated,
            outputPath: out.truncated ? out.outputPath : undefined,
          },
        };
      },
    }),
  };
}
```

### Real-World Custom Tool (GitHub Triage)

Located at `.opencode/tool/github-triage.ts`:

```typescript
import { tool } from "@opencode-ai/plugin"
import DESCRIPTION from "./github-triage.txt"  // External description file

export default tool({
  description: DESCRIPTION,
  args: {
    assignee: tool.schema
      .enum(ASSIGNEES as [string, ...string[]])
      .describe("The username of the assignee")
      .default("rekram1-node"),
    labels: tool.schema
      .array(tool.schema.enum([...]))
      .describe("The labels(s) to add to the issue")
      .default([]),
  },
  async execute(args) {
    // ... GitHub API calls ...
    return results.join("\n")
  },
})
```

### Tool Discovery from `.opencode/` Directory

Custom tools are discovered by scanning config directories:

```typescript
const matches = await Config.directories().then((dirs) =>
  dirs.flatMap((dir) =>
    Glob.scanSync('{tool,tools}/*.{js,ts}', {
      cwd: dir,
      absolute: true,
      dot: true,
      symlink: true,
    })
  )
);
```

The tool file's basename becomes the namespace. If the export is `default`,
the tool ID is just the namespace. Otherwise it's `namespace_exportName`.

---

## 4. Shell Integration

### BunShell Type (passed to plugins)

```typescript
// packages/plugin/src/shell.ts
export interface BunShell {
  (strings: TemplateStringsArray, ...expressions: ShellExpression[]): BunShellPromise;

  braces(pattern: string): string[];
  escape(input: string): string;
  env(newEnv?: Record<string, string | undefined>): BunShell;
  cwd(newCwd?: string): BunShell;
  nothrow(): BunShell;
  throws(shouldThrow: boolean): BunShell;
}

export interface BunShellPromise extends Promise<BunShellOutput> {
  readonly stdin: WritableStream;
  cwd(newCwd: string): this;
  env(newEnv: Record<string, string> | undefined): this;
  quiet(): this;
  lines(): AsyncIterable<string>;
  text(encoding?: BufferEncoding): Promise<string>;
  json(): Promise<any>;
  // ...
}
```

Plugins get the real `Bun.$` instance. Usage example:

```typescript
const plugin: Plugin = async ({ $ }) => {
  const result = await $`git log --oneline -5`.quiet().text();
  // ...
};
```

### Shell Environment Hook

Plugins can modify the shell environment via `shell.env`:

```typescript
export interface Hooks {
  'shell.env'?: (
    input: { cwd: string; sessionID?: string; callID?: string },
    output: { env: Record<string, string> }
  ) => Promise<void>;
  // ...
}
```

### Native Shell Detection

The core uses a multi-strategy approach for shell selection:

```typescript
// packages/opencode/src/shell/shell.ts
const BLACKLIST = new Set(['fish', 'nu']); // Unsupported shells

function fallback() {
  if (process.platform === 'win32') {
    // Try git bash, then COMSPEC, then cmd.exe
  }
  if (process.platform === 'darwin') return '/bin/zsh';
  const bash = Bun.which('bash');
  if (bash) return bash;
  return '/bin/sh';
}
```

---

## 5. Event/Hook System

### The Hooks Interface (Complete)

This is the core extensibility surface. Plugins return a `Hooks` object with
optional handlers for each named hook:

```typescript
// packages/plugin/src/index.ts
export interface Hooks {
  // ── Global event stream ──
  event?: (input: { event: Event }) => Promise<void>;

  // ── Configuration ──
  config?: (input: Config) => Promise<void>;

  // ── Custom tools ──
  tool?: { [key: string]: ToolDefinition };

  // ── Auth provider ──
  auth?: AuthHook;

  // ── Chat lifecycle ──
  'chat.message'?: (
    input: { sessionID; agent?; model?; messageID?; variant? },
    output: { message: UserMessage; parts: Part[] }
  ) => Promise<void>;

  'chat.params'?: (
    input: { sessionID; agent; model; provider; message },
    output: { temperature; topP; topK; options }
  ) => Promise<void>;

  'chat.headers'?: (
    input: { sessionID; agent; model; provider; message },
    output: { headers: Record<string, string> }
  ) => Promise<void>;

  // ── Permission system ──
  'permission.ask'?: (
    input: Permission,
    output: { status: 'ask' | 'deny' | 'allow' }
  ) => Promise<void>;

  // ── Command lifecycle ──
  'command.execute.before'?: (
    input: { command; sessionID; arguments },
    output: { parts: Part[] }
  ) => Promise<void>;

  // ── Tool lifecycle ──
  'tool.execute.before'?: (
    input: { tool; sessionID; callID },
    output: { args: any }
  ) => Promise<void>;

  'tool.execute.after'?: (
    input: { tool; sessionID; callID; args },
    output: { title; output; metadata }
  ) => Promise<void>;

  'tool.definition'?: (
    input: { toolID: string },
    output: { description: string; parameters: any }
  ) => Promise<void>;

  // ── Shell environment ──
  'shell.env'?: (
    input: { cwd; sessionID?; callID? },
    output: { env: Record<string, string> }
  ) => Promise<void>;

  // ── Experimental hooks ──
  'experimental.chat.messages.transform'?: (
    input: {},
    output: { messages: { info: Message; parts: Part[] }[] }
  ) => Promise<void>;

  'experimental.chat.system.transform'?: (
    input: { sessionID?; model },
    output: { system: string[] }
  ) => Promise<void>;

  'experimental.session.compacting'?: (
    input: { sessionID },
    output: { context: string[]; prompt?: string }
  ) => Promise<void>;

  'experimental.text.complete'?: (
    input: { sessionID; messageID; partID },
    output: { text: string }
  ) => Promise<void>;
}
```

### Hook Trigger Pattern (input/output mutation)

All hooks follow an **input + mutable output** pattern:

```typescript
export async function trigger<Name extends ...>(
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

**Key insight:** Hooks don't return values -- they mutate the `output`
parameter. Multiple plugins can chain modifications to the same output object.
This is a **pipeline pattern** (similar to middleware).

### Bus Event System (Internal)

The core has a separate pub/sub bus (`Bus`) that plugins receive via the
`event` hook:

```typescript
// packages/opencode/src/bus/bus-event.ts
export namespace BusEvent {
  export function define<Type extends string, Properties extends ZodType>(
    type: Type,
    properties: Properties,
  ) {
    const result = { type, properties }
    registry.set(type, result)
    return result
  }
}

// packages/opencode/src/bus/index.ts
export namespace Bus {
  export async function publish<Definition>(def, properties) {
    const payload = { type: def.type, properties }
    // Notify type-specific subscribers
    // Notify wildcard (*) subscribers
    // Emit to GlobalBus
  }

  export function subscribe<Definition>(def, callback) { ... }
  export function subscribeAll(callback) { ... }  // Wildcard
}
```

Events are Zod-validated, type-safe, and registered in a global registry.
Plugins receive all events through their `event` hook after `Plugin.init()`
calls `Bus.subscribeAll()`.

---

## 6. Configuration Format

### Config File: `opencode.json` / `opencode.jsonc`

Located at project root or in `.opencode/` directory. Supports JSONC (comments,
trailing commas).

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-20250514",
  "plugin": ["my-plugin@1.0.0"],
  "provider": {
    "opencode": { "options": {} },
  },
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["node", "server.js"],
    },
  },
  "agent": {
    "build": { "model": "...", "prompt": "..." },
    "plan": { "model": "..." },
  },
  "permission": {
    "edit": "allow",
    "bash": { "*": "ask", "git *": "allow" },
  },
}
```

### Config Precedence (low to high)

1. Remote `.well-known/opencode` (org defaults)
2. Global config (`~/.config/opencode/opencode.json`)
3. Custom config (`OPENCODE_CONFIG` env var)
4. Project config (`opencode.json` at project root)
5. `.opencode/` directories (scanned up from cwd to worktree root)
6. Inline config (`OPENCODE_CONFIG_CONTENT` env var)
7. Managed config directory (enterprise, highest priority)

### `.opencode/` Directory Structure

```
.opencode/
  opencode.jsonc      # Project-level config overrides
  agent/              # Custom agents (Markdown with YAML frontmatter)
    triage.md
    docs.md
  command/            # Custom slash commands (Markdown)
    deploy.md
  tool/               # Custom tools (TypeScript/JavaScript)
    github-triage.ts
    github-triage.txt # Tool description (imported by the tool)
  plugins/            # Local plugin files (auto-discovered)
    my-plugin.ts
  themes/             # Custom themes
```

### Agent Definition (Markdown + Frontmatter)

```markdown
---
mode: primary
hidden: true
model: opencode/minimax-m2.5
color: '#44BA81'
tools:
  '*': false
  'github-triage': true
---

You are a triage agent responsible for triaging github issues.
Use your github-triage tool to triage issues.
```

---

## 7. Application to Impulse -- Phase 1.5 Plugin Adapter

### What Impulse Can Learn

| OpenCode Pattern                  | Impulse Application                                                    | Priority      |
| --------------------------------- | ---------------------------------------------------------------------- | ------------- |
| `Plugin` function signature       | Impulse adapter returns hooks object                                   | High          |
| `tool()` factory with Zod args    | If Impulse adds custom tools for agents, reuse this pattern            | Medium        |
| Input/output mutation hooks       | SessionStart/SessionEnd could be hooks with mutable context            | High          |
| `.opencode/` directory convention | `.impulse/` already follows this; tool/agent dirs are validated        | Existing      |
| npm + file:// plugin resolution   | `npx impulse init` installs, local files for development               | High          |
| `BunShell` passthrough            | Impulse hooks already spawn Bun CLIs; no shell needed in Phase 1       | Low           |
| Bus event system                  | Impulse's 4 hooks map cleanly to 4 events                              | Medium        |
| Config precedence chain           | Impulse is simpler (project-only), but global config pattern is useful | Low (Phase 2) |

### Proposed Adapter Shape

If Impulse builds a Phase 1.5 adapter for OpenCode compatibility, the mapping
would be:

```typescript
// impulse-opencode-adapter.ts (conceptual)
import type { Plugin, Hooks } from '@opencode-ai/plugin';

export const ImpulsePlugin: Plugin = async (input) => {
  const hooks: Hooks = {
    // ── Inject memory at session start ──
    'experimental.chat.system.transform': async (_input, output) => {
      const genome = await readGenome(input.directory);
      const history = await readHistoryIndex(input.directory);
      if (genome) output.system.push(genome);
      if (history) output.system.push(history);
    },

    // ── Preserve memory during compaction ──
    'experimental.session.compacting': async (_input, output) => {
      const genome = await readGenomeTop50(input.directory);
      if (genome) output.context.push(genome);
    },

    // ── Track tool usage (replaces PostToolUse hook) ──
    'tool.execute.after': async (toolInput, _output) => {
      await updateLiveState(input.directory, {
        tool: toolInput.tool,
        sessionID: toolInput.sessionID,
        timestamp: Date.now(),
      });
    },

    // ── Extract knowledge at session end ──
    event: async ({ event }) => {
      if (event.type === 'session.completed' || event.type === 'session.quit') {
        await extractAndAppend(input.directory, event);
      }
    },
  };

  return hooks;
};
```

### Mapping Impulse's 4 Hooks to OpenCode's Hook System

| Impulse Hook   | OpenCode Hook                           | Notes                                            |
| -------------- | --------------------------------------- | ------------------------------------------------ |
| `SessionStart` | `experimental.chat.system.transform`    | Inject GENOME + HISTORY as system context        |
| `PostToolUse`  | `tool.execute.after`                    | Update LIVE_STATE.json                           |
| `PreCompact`   | `experimental.session.compacting`       | Push GENOME top-50 lines into compaction context |
| `SessionEnd`   | `event` (listen for session completion) | Trigger LLM extraction                           |

### Key Differences to Account For

1. **Impulse hooks are out-of-process shell scripts; OpenCode hooks are in-process functions.**
   The adapter would be in-process, which is actually simpler and faster (no
   spawn overhead).

2. **Impulse writes 3 files; OpenCode has no file-memory equivalent.**
   The adapter would manage `.impulse/` files from within the plugin process.

3. **OpenCode hooks mutate output objects; Impulse hooks write to stdout.**
   The adapter uses the mutation pattern directly -- no stdout parsing needed.

4. **OpenCode plugins get a full SDK client; Impulse doesn't need one.**
   The adapter can ignore `input.client` and just use `input.directory` for
   file operations.

### Implementation Notes

- The `@opencode-ai/plugin` package is published to npm (v1.2.9 at time of
  extraction). Impulse can depend on it for types only.
- OpenCode auto-installs `@opencode-ai/plugin` into `.opencode/node_modules/`
  when it detects local plugins. Impulse's adapter would be a proper npm
  package, not a local file.
- The `experimental.*` hooks are explicitly marked experimental in the type
  definitions. They are the most relevant for Impulse's use case
  (system prompt injection, compaction context). Their stability is not
  guaranteed.
- The `event` hook receives ALL bus events. Filtering by event type is the
  plugin's responsibility.

---

## Appendix: File Inventory

| File                                      | Lines | Purpose                                         |
| ----------------------------------------- | ----- | ----------------------------------------------- |
| `packages/plugin/src/index.ts`            | 235   | Plugin type, Hooks interface, AuthHook types    |
| `packages/plugin/src/tool.ts`             | 38    | `tool()` factory, ToolContext, ToolDefinition   |
| `packages/plugin/src/shell.ts`            | 137   | BunShell type definitions                       |
| `packages/plugin/src/example.ts`          | 18    | Example plugin (minimal)                        |
| `packages/plugin/package.json`            | 28    | Package config (deps: @opencode-ai/sdk, zod)    |
| `packages/opencode/src/plugin/index.ts`   | 138   | Plugin loader, trigger(), init()                |
| `packages/opencode/src/plugin/copilot.ts` | 327   | GitHub Copilot auth plugin (real-world example) |
| `packages/opencode/src/plugin/codex.ts`   | 624   | OpenAI Codex auth plugin (OAuth flow example)   |
| `packages/opencode/src/tool/registry.ts`  | 171   | Tool registration, plugin tool bridging         |
| `packages/opencode/src/tool/tool.ts`      | 89    | Core Tool.Info type, Tool.define()              |
| `packages/opencode/src/config/config.ts`  | 1491  | Full config schema, loading, precedence         |
| `packages/opencode/src/bus/index.ts`      | 105   | Event bus (pub/sub, type-safe)                  |
| `packages/opencode/src/bus/bus-event.ts`  | 43    | BusEvent.define() factory                       |
| `packages/opencode/src/bus/global.ts`     | 10    | Global EventEmitter bridge                      |
| `packages/opencode/src/shell/shell.ts`    | 68    | Shell detection and process management          |
| `.opencode/tool/github-triage.ts`         | 113   | Real-world custom tool example                  |
| `.opencode/agent/triage.md`               | 141   | Real-world custom agent definition              |

---

_Extracted from `cloned-repos/opencode/` on 2026-02-21. Source: github.com/anomalyco/opencode._
