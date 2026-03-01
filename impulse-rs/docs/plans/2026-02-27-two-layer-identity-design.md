# Two-Layer Identity Architecture

> Approved 2026-02-27. Gives the Impulse agent its own identity and scopes terminal panes to user-selected project directories.

---

## Problem

When an AI coding agent is launched inside Impulse, it has no idea it's being managed by Impulse. The CLAUDE.md describes Impulse from a developer perspective ("here's the architecture") but never tells the agent "you are being managed by Impulse." The GENOME.md is full of test data. Terminal panes all inherit the GUI's working directory with no per-project targeting.

## Solution: Three Identity Layers

### Layer 1: Application-Level (`~/.impulse/`)

The Impulse agent (GUI agent panel) reads from here.

| File | Purpose |
|------|---------|
| `~/.impulse/CLAUDE.md` | "You ARE Impulse — context manager, memory keeper, process manager" |
| `~/.impulse/AGENTS.md` | How other AI agents interact with Impulse CLI |
| `~/.impulse/config.json` | Global config: `recent_projects`, global preferences |

**Role definition:** Impulse is a context manager, memory keeper, and process manager for AI coding agents. It does not write code itself. It coordinates the agents that do — tracking sessions, managing decisions (GENOME), surfacing cross-pane conflicts, and providing session continuity.

### Layer 2: Project-Level (`<target>/.impulse/`)

Coding agents in terminal panes read from here, scoped to the target project.

| File | Purpose |
|------|---------|
| `<target>/CLAUDE.md` | Project conventions (untouched if exists) |
| `<target>/.impulse/GENOME.md` | Project decisions and preferences |
| `<target>/.impulse/HISTORY.jsonl` | Session log for this project |
| `<target>/.impulse/config.json` | Project-specific Impulse config |

Auto-scaffolded on first target selection if `.impulse/` doesn't exist.

### Layer 3: Developer-Level (`impulse-rs/CLAUDE.md`)

Unchanged. Existing architecture docs, code style, build instructions for contributors to the Impulse codebase itself.

---

## Pane Spawn Flow

```
User clicks "New Terminal" or agent button
        │
        ▼
┌─────────────────────────────┐
│  Project Selector Dialog    │
│                             │
│  Recent Projects:           │
│   ▸ ~/projects/my-app      │
│   ▸ ~/projects/api-server  │
│                             │
│  [Browse...]  [Use ~/]      │
│                             │
│  ℹ Select a project folder  │
│    for this terminal        │
└─────────────────────────────┘
        │
        ▼ (folder selected)
   .impulse/ exists?
   ├── Yes → spawn pane with working_dir = target
   └── No  → auto-scaffold .impulse/ → spawn pane
        │
        ▼
   Wait for agent startup (Claude 3s, OpenCode/Codex 2s, Shell 0.5s)
        │
        ▼
   Inject init context (build_init_message v2)
```

### Project Selector

- **Recent projects list:** Stored in `~/.impulse/config.json` under `recent_projects: Vec<PathBuf>`. Max 10, MRU order.
- **Browse button:** `rfd::FileDialog` native folder picker.
- **Default fallback:** `~/` (user home). User is encouraged to select a project but not hard-blocked.
- **Tab labels show project:** `[Claude: my-app]` instead of `[Claude Code]`.

---

## Context Injection

### Init Injection (Coding Agent Pane)

Injected after agent startup delay. Combines application + project layers.

```xml
<impulse-context type="init" version="2">
## Identity
You are a coding agent running inside Impulse, a terminal multiplexer
and memory system for AI coding agents. Impulse tracks your work across
sessions — file changes, decisions, errors — and provides continuity.

## Your Project
Working directory: ~/projects/my-app
Session: abc-1234 | Pane: Claude Code

## Standing Decisions (from GENOME)
- Use TypeScript strict mode (2026-02-15)
- API responses follow JSON:API spec (2026-02-10)

## Last Session Summary
- Modified: src/auth.ts, src/middleware.ts
- Decision: Switched from JWT to session tokens
- Error resolved: CORS issue on /api/users

## Available Tools
Run `impulse-rs sync-context` to refresh context at any time.
Run `impulse-rs add-decision "description" --rationale "why"` to record decisions.
</impulse-context>
```

**Sources:**

| Section | Source | Layer |
|---------|--------|-------|
| Identity preamble | `~/.impulse/CLAUDE.md` | Application |
| Project info + working dir | Spawn target + `<target>/.impulse/config.json` | Project |
| Standing decisions | `<target>/.impulse/GENOME.md` (last N decisions) | Project |
| Last session summary | `<target>/.impulse/HISTORY.jsonl` (most recent) | Project |
| Cross-pane insights | Other open panes targeting same project | Runtime |

### Agent Panel Context (Impulse Agent)

The agent panel reads from the application layer, not a project:

```xml
<impulse-context type="agent-panel" version="2">
## Identity
You are Impulse — a context manager, memory keeper, and process manager
for AI coding agents. You coordinate work across terminal panes, track
decisions and file changes, and provide session continuity.

You do NOT write code yourself. You manage the agents that do.

## Active Panes
- Pane 1: Claude Code → ~/projects/my-app (active, 45% context)
- Pane 2: OpenCode → ~/projects/api-server (idle)

## Cross-Pane Activity
- [Claude: my-app] FileModified: src/auth.ts
- [OpenCode: api-server] ErrorEncountered: type mismatch in handler

## Your Capabilities
- Track sessions, file changes, decisions, errors
- Surface cross-pane conflicts (same file edited in multiple panes)
- Manage GENOME (permanent decisions) per project
- Inject context at threshold boundaries (45/60/80%)
- Evaluate guardrail rules before dangerous actions
</impulse-context>
```

### Refresh Tiers (Unchanged)

| Tier | Trigger | Content |
|------|---------|---------|
| Full | Spawn | Identity + project + genome + last session + tools |
| Essential | 45% usage | Tools + active files + key decisions |
| Critical | 60% usage | Tools + current task summary |
| Minimal | 80% usage | Tool list + refresh command |
| PostCompaction | After compaction | Identity + tools + current state |

---

## GENOME Cleanup

1. **Purge test data** from `.impulse/GENOME.md` — 140+ identical "Test decision for integration" entries → empty
2. **Fix integration tests** — redirect via `IMPULSE_HOME` or `--impulse-dir` to `tempfile::TempDir`
3. **Add dedup guard** in `State::add_decision()` — skip if last N decisions have same description
4. **Starter template** for auto-scaffolded projects:
   ```json
   { "decisions": [], "preferences": [], "constraints": [], "last_updated": null }
   ```

---

## Data Model Changes

### Tab struct (impulse-gui)

```rust
struct Tab {
    id: u64,
    label: String,
    agent_name: String,
    panel: TerminalPanel,
    target_dir: PathBuf,        // NEW
    session_id: Option<String>, // NEW
}
```

### TerminalPanel::spawn (impulse-term)

```rust
pub fn spawn(
    command: &str,
    args: &[String],
    working_dir: &Path,         // Now required
    agent_name: &'static str,
    pane_id: usize,
    session_id: Option<&str>,   // NEW
) -> Result<Self, Box<dyn std::error::Error>>
```

### Global config (`~/.impulse/config.json`)

```json
{
  "recent_projects": [
    "~/projects/my-app",
    "~/projects/api-server"
  ]
}
```

---

## Summary of Changes

| Component | Change |
|-----------|--------|
| `~/.impulse/CLAUDE.md` | NEW — Application-level Impulse agent identity |
| `~/.impulse/AGENTS.md` | NEW — Agent interaction guide |
| `~/.impulse/config.json` | NEW — Global config with recent_projects |
| `impulse-gui` spawn flow | MODIFIED — Project selector dialog before pane spawn |
| `TerminalPanel::spawn` | MODIFIED — Required working_dir, optional session_id |
| `Tab` struct | MODIFIED — Add target_dir and session_id fields |
| Init injection | WIRED — Call build_init_message() after spawn delay |
| Agent panel context | NEW — Application-level identity + cross-pane status |
| `.impulse/GENOME.md` | CLEANED — Purge test data |
| Integration tests | FIXED — Use IMPULSE_HOME isolation |
| `State::add_decision` | IMPROVED — Dedup guard |
| Auto-scaffold | NEW — Create .impulse/ on first project target |
| Tab bar labels | IMPROVED — Show [Agent: project-name] |
| `rfd` dependency | NEW — Native file dialog for project selector |
