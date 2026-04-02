---
status: superseded
phase: 1
audience: builder
tags: [phase, checklist, mvp, hooks]
last_updated: 2026-02-21
---

# Phase 1 Implementation Checklist

> **⚠️ Superseded for implementation authority.**
> This checklist targets the TypeScript/Bun implementation path and is preserved as historical context.
> Use [`../spec/RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md) for the active implementation contract.
>
> **Version:** 2.1 | **Status:** Active | **Updated:** 2026-02-21
> **Supersedes:** v1.0 (OpenCode plugin architecture — see `archive/`)
> **Spec:** [`RUST-CANONICAL-CONTRACT.md`](../spec/RUST-CANONICAL-CONTRACT.md) — Current contract reference for the live workspace
> **Limitations:** [`HONEST-ROADMAP.md`](../HONEST-ROADMAP.md) — Read this second
> **Target:** TypeScript/Bun ONLY. Zero Python. Zero Rust. Zero databases.

---

## Priority 0: Pre-Phase Spike (NON-OPTIONAL — Complete Before Any TypeScript)

**These 5 tests validate the critical path assumptions. If any fail, revise the spec before building.**

- [ ] **Hook injection test:** Write a 10-line bash `session-start.sh`:
  ```bash
  #!/bin/bash
  echo "GENOME: TEST IMPULSE INJECTION $(date)"
  ```
  Register it as a Claude Code SessionStart hook. Start a new session. Verify the output appears in Claude Code's system context. Document exactly how it appears.

- [ ] **PreCompact test:** Write a 10-line bash `pre-compact.sh`:
  ```bash
  #!/bin/bash
  echo "MUST SURVIVE COMPACTION: critical-test-content"
  ```
  Trigger a compaction (long session or manual). Verify `critical-test-content` appears post-compaction.

- [ ] **JSONL format inspection:** Open 3 real Claude Code JSONL transcript files (~/.claude/projects/*/). Document the exact structure of: user messages, assistant messages, tool calls, and tool results. Note the content block format `[{type:'text',text:'...'}]`.

- [ ] **type: agent hook test:** Create a SessionEnd hook with `"type": "agent"`. Write a 2-turn agent that reads `transcript_path` and writes 3 fake "decisions" to a test file. Measure: turns used, latency, output quality. Compare to a direct SDK call.

- [ ] **Extraction hypothesis test:** Manually create `.impulse/GENOME.md` with 10 real architectural decisions for a real project. Use Claude Code normally for 3-5 days. Observe: does Claude Code reference these decisions? Does it avoid re-debating settled choices?

**Go/No-go gates:**
- Hook injection PASSES → proceed with SessionStart implementation
- PreCompact PASSES → proceed with PreCompact implementation
- If type: agent wins quality test → use type: agent for SessionEnd (no API key required)
- Extraction hypothesis PASSES → the core product is viable; proceed with full Phase 1

---

## Pre-Implementation (Read First)

### Required Reading

- [ ] `docs/HONEST-ROADMAP.md` — **READ THIS FIRST** — limitations and corrections
- [ ] `docs/spec/PRODUCT-SPEC-v2.md` — Full product specification
- [ ] `docs/decisions/0001-claude-code-primary.md` — Why Claude Code hooks, not OpenCode
- [ ] `docs/decisions/0002-file-first-memory.md` — Why files instead of a database
- [ ] `docs/decisions/0004-extraction-strategy.md` — How LLM extraction works
- [ ] `docs/decisions/0005-distribution-model.md` — npm distribution model

### Environment Setup

- [ ] Install Bun (`bun --version` — need ≥1.0)
- [ ] Install Node.js 20+ (for npm publishing)
- [ ] Install Claude Code (for hook testing)
- [ ] Have an Anthropic API key (for SessionEnd LLM call)
- [ ] Create `impulse/` directory at project root

### Project Setup

- [ ] `cd impulse && bun init`
- [ ] Set `"type": "module"` in package.json
- [ ] Add `"bin"` entries:
  ```json
  "bin": {
    "impulse": "./dist/cli.js",
    "impulse-session-start": "./dist/hooks/session-start.js",
    "impulse-post-tool-use": "./dist/hooks/post-tool-use.js",
    "impulse-session-end": "./dist/hooks/session-end.js",
    "impulse-pre-compact": "./dist/hooks/pre-compact.js"
  }
  ```
- [ ] Configure TypeScript strict mode (`strict: true`)
- [ ] Set up Vitest for testing
- [ ] Verify type-check: `bun run tsc --noEmit`

---

## Priority 1: Core File Operations

### Target Files

- [ ] Create `src/files.ts` — All `.impulse/` file read/write logic

### Acceptance Criteria for files.ts

- [ ] `ensureImpulseDir(cwd: string): void` — Create `.impulse/` if not exists
- [ ] `readGenome(cwd: string): string` — Read GENOME.md, return empty string if missing
- [ ] `appendGenome(cwd: string, entries: string[]): void` — Atomic append to GENOME.md
- [ ] `readLiveState(cwd: string): LiveState` — Parse LIVE_STATE.json, return default if missing
- [ ] `writeLiveState(cwd: string, state: LiveState): void` — Atomic write LIVE_STATE.json
- [ ] `readHistoryIndex(cwd: string, limit?: number): string` — Read last N entries from HISTORY_INDEX.md
- [ ] `appendHistoryIndex(cwd: string, entry: string): void` — Atomic prepend to HISTORY_INDEX.md

### Atomicity Requirement (CRITICAL)

All writes MUST use temp file + rename pattern:
```typescript
import { writeFileSync, renameSync } from 'fs';
const tmpPath = `${filePath}.tmp`;
writeFileSync(tmpPath, content, 'utf-8');
renameSync(tmpPath, filePath);
```
Never use `writeFileSync(finalPath, ...)` directly — corrupts on crash.

### Tests for files.ts

- [ ] `files.test.ts` — All tests passing (use `tmp` directory)
  - [ ] `readGenome` returns empty string when file missing
  - [ ] `appendGenome` creates file if not exists
  - [ ] `appendGenome` is idempotent on duplicate entries
  - [ ] `writeLiveState` creates valid JSON
  - [ ] `readLiveState` returns default state when file missing
  - [ ] `readHistoryIndex` returns last N entries in reverse-chron order
  - [ ] Atomic write verified (temp file removed after success)
  - [ ] Concurrent write test (two processes writing simultaneously)

**Acceptance Criteria:**
- [ ] All file operations are atomic (temp + rename)
- [ ] All operations have error handling (never throw up to hook level)
- [ ] Tests use real temp directories (not mocked fs)
- [ ] >90% coverage

---

## Priority 2: Type Definitions

### Target Files

- [ ] Create `src/types.ts` — All shared types + Zod schemas

### Required Types

```typescript
// LiveState (LIVE_STATE.json schema)
const LiveStateSchema = z.object({
  agents: z.array(AgentEntrySchema),
  extractionPending: z.boolean().default(false),
  lastUpdated: z.string().datetime(),
});

// AgentEntry (one agent's status)
const AgentEntrySchema = z.object({
  id: z.string(),                       // session_id from Claude Code
  startedAt: z.string().datetime(),
  lastActivity: z.string().datetime(),
  activeFiles: z.array(z.string()),
  recentTools: z.array(z.string()),
  transcriptPath: z.string().optional(), // set by SessionEnd for deferred extraction
});

// SessionStartInput (from Claude Code stdin)
const SessionStartInputSchema = z.object({
  session_id: z.string(),
  transcript_path: z.string(),
  cwd: z.string(),
  hook_event_name: z.literal('SessionStart'),
  source: z.string().optional(),
  model: z.string().optional(),
});

// PostToolUseInput (from Claude Code stdin)
const PostToolUseInputSchema = z.object({
  session_id: z.string(),
  tool: z.string(),
  input: z.string(),   // stringified JSON of tool input
  output: z.string(),
  hook_event_name: z.literal('PostToolUse'),
});

// SessionEndInput (from Claude Code stdin)
const SessionEndInputSchema = z.object({
  session_id: z.string(),
  transcript_path: z.string(),
  cwd: z.string(),
  hook_event_name: z.literal('SessionEnd'),
  reason: z.string().optional(),
});

// PreCompactInput (from Claude Code stdin)
const PreCompactInputSchema = z.object({
  session_id: z.string(),
  hook_event_name: z.literal('PreCompact'),
});

// ExtractionResult (LLM response schema)
const ExtractionResultSchema = z.object({
  decisions: z.array(z.string()),
  summary: z.string(),
  contradictions: z.array(z.string()).optional(),
});
```

- [ ] All schemas defined with Zod
- [ ] All types exported
- [ ] Helper functions: `parseStdin<T>(schema: ZodSchema<T>): T`

---

## Priority 3: Hook 1 — SessionStart

### Target Files

- [ ] Create `src/hooks/session-start.ts`

### Behavior to Implement

1. Read stdin JSON → parse `SessionStartInputSchema`
2. Extract `cwd` as project root
3. Check `LIVE_STATE.json` for `extractionPending: true`
   - If true: run deferred extraction (call extraction logic from SessionEnd)
4. Read `.impulse/GENOME.md` — full content
5. Read `.impulse/LIVE_STATE.json` — filter other active agents
6. Read `.impulse/HISTORY_INDEX.md` — last 3 entries
7. Register this agent in `LIVE_STATE.json`
8. Format context and print to stdout

### Output Format (stdout)

```
## Project Context (Impulse)

### Architectural Decisions
[GENOME.md content]

### Other Active Agents
[List or "None"]

### Recent Sessions
[Last 3 HISTORY_INDEX.md entries]
```

### Performance Target

- [ ] < 30ms for file reads when no deferred extraction
- [ ] < 10s when deferred extraction runs

### Tests for session-start.ts

- [ ] `session-start.test.ts` — All tests passing
  - [ ] Normal start: reads and outputs all 3 files
  - [ ] First run (no `.impulse/`): creates dir, outputs empty context
  - [ ] With other active agents: shows agents section
  - [ ] With extractionPending: runs deferred extraction before loading context
  - [ ] With missing GENOME.md: outputs empty decisions section
  - [ ] Invalid stdin JSON: exits 0 (graceful degradation)
  - [ ] Registers agent in LIVE_STATE.json after success

---

## Priority 4: Hook 2 — PostToolUse

### Target Files

- [ ] Create `src/hooks/post-tool-use.ts`

### Behavior to Implement

1. Read stdin JSON → parse `PostToolUseInputSchema`
2. Extract file paths from `input` JSON string:
   - For `Write`: extract `file_path`
   - For `Edit`: extract `file_path`
   - For `Bash`: extract file paths mentioned in `command` (regex: `/\b[\w./]+\.\w+\b/g`)
3. Read current `LIVE_STATE.json`
4. Update this agent's `activeFiles` and `lastActivity`
5. Write updated `LIVE_STATE.json`
6. Exit with code 0 (no stdout)

### Performance Target

- [ ] < 100ms total (JSON parse + file read + file write)

### Tests for post-tool-use.ts

- [ ] `post-tool-use.test.ts` — All tests passing
  - [ ] Write tool: extracts and stores file_path
  - [ ] Edit tool: extracts and stores file_path
  - [ ] Bash tool: extracts file paths from command
  - [ ] First call: creates agent entry if missing
  - [ ] Subsequent calls: updates lastActivity timestamp
  - [ ] Invalid input: exits 0 (graceful degradation)
  - [ ] Performance: < 100ms in 10 consecutive runs

---

## Priority 5: Hook 3 — SessionEnd

### Target Files

- [ ] Create `src/hooks/session-end.ts`
- [ ] Create `src/extraction.ts` — JSONL parsing + LLM extraction

### Behavior to Implement

1. Parse stdin → `SessionEndInputSchema`
2. Read JSONL transcript from `transcript_path`
3. Parse JSONL: extract `human` and `assistant` messages, skip tool result messages
4. Filter: remove messages with `<function_calls>` content (tool noise, ~75% of transcript)
5. Sample: beginning (30%) + end (70%) of remaining text
6. Read existing `GENOME.md` for contradiction awareness
7. Build extraction prompt (see PRODUCT-SPEC-v2.md Section 7)
8. Call LLM API (model from `IMPULSE_MODEL` env var, default: `claude-haiku-4-5-20251001`)
9. Parse JSON response: `{ decisions, summary, contradictions }`
10. Deduplicate: skip decisions already in GENOME.md (fuzzy string match)
11. Append decisions to GENOME.md
12. Prepend session summary to HISTORY_INDEX.md
13. Remove this agent from LIVE_STATE.json
14. On any failure: set `extractionPending: true` in LIVE_STATE.json

### Performance Target

- [ ] < 10s total (JSONL parse + filter + 1 LLM call + file writes)

### Tests for session-end.ts

- [ ] `session-end.test.ts` — All tests passing
  - [ ] Parses JSONL transcript correctly (human + assistant messages)
  - [ ] Filters tool noise (skips messages with tool calls)
  - [ ] Deduplicates against existing GENOME.md
  - [ ] Appends to GENOME.md (does not overwrite)
  - [ ] Prepends to HISTORY_INDEX.md
  - [ ] Removes agent from LIVE_STATE.json
  - [ ] Sets extractionPending on LLM API failure
  - [ ] Sets extractionPending on transcript parse failure
  - [ ] Graceful on empty transcript (first session)

### LLM Extraction Tests (mock the API)

- [ ] `extraction.test.ts` — All tests passing
  - [ ] Extraction prompt includes GENOME context
  - [ ] Extraction prompt has few-shot examples
  - [ ] Parses valid JSON response
  - [ ] Handles malformed JSON response (retry or empty result)
  - [ ] Handles API timeout (sets extractionPending)

---

## Priority 6: Hook 4 — PreCompact

### Target Files

- [ ] Create `src/hooks/pre-compact.ts`

### Behavior to Implement

1. Parse stdin → `PreCompactInputSchema`
2. Read `.impulse/GENOME.md`
3. Extract first 50 lines
4. Format with header
5. Print to stdout

### Performance Target

- [ ] < 100ms total (file read + format + stdout)

### Tests for pre-compact.ts

- [ ] `pre-compact.test.ts` — All tests passing
  - [ ] Returns first 50 lines of GENOME.md
  - [ ] Returns empty output when GENOME.md missing
  - [ ] Output includes "must survive compaction" header
  - [ ] No stdout when GENOME.md is empty
  - [ ] Performance: < 100ms

---

## Priority 7: CLI (impulse init)

### Target Files

- [ ] Create `src/cli.ts` — `impulse init` command

### Behavior to Implement

1. `impulse init`:
   - Create `.impulse/` directory
   - Create `.impulse/GENOME.md` (empty template)
   - Create `.impulse/HISTORY_INDEX.md` (empty template)
   - Add `.impulse/LIVE_STATE.json` to `.gitignore`
   - Write hook config to `.claude/settings.local.json`
   - Print success message with next steps

2. `impulse status`:
   - Print current LIVE_STATE.json (active agents)
   - Print GENOME.md line count and last decision date
   - Print HISTORY_INDEX.md entry count

### Hook Config Written by `impulse init`

```json
{
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "impulse-session-start", "timeout": 30 }] }],
    "PostToolUse": [{ "matcher": "Write|Edit|Bash", "hooks": [{ "type": "command", "command": "impulse-post-tool-use", "timeout": 5 }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "impulse-session-end", "timeout": 60 }] }],
    "PreCompact": [{ "hooks": [{ "type": "command", "command": "impulse-pre-compact", "timeout": 10 }] }]
  }
}
```

### Tests for cli.ts

- [ ] `cli.test.ts` — All tests passing
  - [ ] `init` creates `.impulse/` directory
  - [ ] `init` writes hook config to `.claude/settings.local.json`
  - [ ] `init` is idempotent (safe to run twice)
  - [ ] `init` adds LIVE_STATE.json to .gitignore
  - [ ] `status` reads and formats current state

---

## Priority 8: Integration Testing

### End-to-End Tests

- [ ] `integration.test.ts` — All tests passing
  - [ ] Full lifecycle: init → session-start → post-tool-use × 3 → session-end
  - [ ] Context persists across sessions (GENOME.md)
  - [ ] Multi-agent: two sessions register/deregister correctly
  - [ ] Deferred extraction: set extractionPending, next session-start picks it up
  - [ ] All hooks exit 0 even on internal errors

### Manual Testing Steps

- [ ] Run `bun run build` and verify `dist/` is populated
- [ ] Run `npx impulse init` in a test project
- [ ] Verify hook config written to `.claude/settings.local.json`
- [ ] Open Claude Code in the test project
- [ ] Verify session-start output appears (check Claude's system context)
- [ ] Make 3 file edits and verify LIVE_STATE.json updates
- [ ] End session and verify GENOME.md + HISTORY_INDEX.md updated

---

## Code Quality Gates

### Type Safety

- [ ] `bun run tsc --noEmit` passes (strict mode)
- [ ] No `any` types
- [ ] All hooks have explicit return types
- [ ] All Zod schemas exported and tested

### Error Handling (CRITICAL)

Every hook script MUST:
- [ ] Be wrapped in a top-level try-catch
- [ ] Log errors to stderr (NOT stdout — stdout is reserved for context injection)
- [ ] Exit with code 0 even on error (graceful degradation — never block agent)
- [ ] Never throw from file operations to hook level

### Testing

- [ ] `bun test` — all tests passing
- [ ] >85% overall coverage
- [ ] >90% coverage on `files.ts` (core file operations)
- [ ] >85% coverage on each hook

### Linting

- [ ] `bun run lint` passes
- [ ] No `console.log` in production code (use stderr: `console.error`)
- [ ] No hardcoded paths

---

## Distribution (Phase 1 Complete Criterion)

- [ ] `bun run build` produces `dist/` with all 5 executables
- [ ] `npm publish --dry-run` succeeds without errors
- [ ] `npx impulse init` works from a fresh install
- [ ] All 4 hook commands are on PATH after `npm install -g impulse`
- [ ] `README.md` covers 5-minute quickstart

---

## Size Monitoring

GENOME.md size should be monitored:

| Lines | Action |
|-------|--------|
| < 200 | Normal operation |
| 200-500 | `console.warn` in session-end (suggest review) |
| > 500 | Phase 2 trigger: add LLM-assisted pruning |

---

## Timeline Estimate

| Phase | Focus | Deliverable |
|-------|-------|-------------|
| Day 1 | Priorities 1-2 | File ops + types (tested) |
| Day 2 | Priorities 3-4 | SessionStart + PostToolUse (tested) |
| Day 3 | Priority 5 | SessionEnd + extraction (tested) |
| Day 4 | Priorities 6-7 | PreCompact + CLI (tested) |
| Day 5 | Priority 8 | Integration tests + manual testing |
| Day 6 | Quality gates | Type-check, lint, coverage, npm dry-run |

**Total:** ~1 week for one developer

---

_v2.0 created: 2026-02-21 | Rewritten for Claude Code hooks architecture_
_Supersedes: v1.0 (OpenCode plugin architecture, in archive/)_
