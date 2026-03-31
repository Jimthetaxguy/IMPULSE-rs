# Supervisor-as-Terminal with Two-Tier Memory — Design Spec

**Goal:** Transform the supervisor from a separate chat widget into a real PTY terminal pane that lives at the root `~/.impulse/` level, sees across all projects, and operates at a higher memory tier than per-project agent terminals.

**Architecture:** Option C (Hybrid) — the supervisor is a real agent running in a terminal (Claude Code, OpenCode, or Codex), promoted to supervisor status through context injection. It is NOT a special UI widget — it's a terminal pane like any other, but with cross-project awareness injected by Impulse.

---

## Core Concept

```
~/.impulse/                          <-- Supervisor lives HERE
  supervisor/
    GENOME.md                        <-- Cross-project decisions
    HISTORY.jsonl                    <-- Supervisor session log
    LIVE_STATE.json                  <-- Active supervisor state
    config.json                      <-- Supervisor-specific config
    insights/                        <-- Extracted cross-project insights
      project-a.jsonl
      project-b.jsonl

<project-a>/.impulse/                <-- Project agent terminals live HERE
  GENOME.md                          <-- Project-scoped decisions
  HISTORY.jsonl                      <-- Project session log

<project-b>/.impulse/                <-- Another project
  GENOME.md
  HISTORY.jsonl
```

**The key distinction:** Project terminals remember what happened *in that project*. The supervisor remembers what happened *across all projects* — patterns, conflicts, recurring decisions, cross-project dependencies.

---

## Two-Tier Memory Architecture

### Tier 1: Project Memory (existing — per-project agents)

What it is today. Each project has its own `.impulse/` directory with:
- `GENOME.md` — project-specific decisions, preferences, constraints
- `HISTORY.jsonl` — session log for that project
- `LIVE_STATE.json` — active sessions in that project
- `retrieval.db` — search index for that project

**Agents that use it:** Claude Code, OpenCode, Codex — spawned in the project's terminal tab, scoped to that project directory.

**Context injection:** Session start injects project GENOME.md + recent history. Threshold-based re-injection at 45/60/80%.

### Tier 2: Supervisor Memory (new — cross-project)

Lives at `~/.impulse/supervisor/`. Stores:
- **Cross-project decisions** — "I always use X pattern for auth" / "Team prefers Y over Z"
- **Project summaries** — condensed view of each project's state (auto-extracted)
- **Recurring patterns** — things the supervisor notices across projects (builds on existing `stewardship/cross_project.rs`)
- **Conflict history** — when two projects made conflicting decisions
- **Routing intelligence** — which projects/agents to suggest for different tasks

**The supervisor terminal is injected with:**
1. Its own `~/.impulse/supervisor/GENOME.md` (cross-project decisions)
2. Summaries of all active projects (condensed from per-project GENOMEs)
3. Current activity across all open terminal tabs (via existing insight extraction)
4. The cross-project stewardship patterns (already computed by `stewardship/cross_project.rs`)

---

## Supervisor Terminal Lifecycle

### Spawning

The supervisor tab appears in the terminal multiplexer like any other tab, but:
- Labeled as "Supervisor" with a distinct accent color (purple, matching current agent panel)
- Spawns the user's preferred agent (Claude Code by default, configurable)
- Working directory: the user's home or a meta-project directory
- Gets a **system prompt injection** on spawn (via context lifecycle) that tells it:
  - "You are the Impulse supervisor. You see across all active projects."
  - Current project summaries
  - Cross-project decisions from `~/.impulse/supervisor/GENOME.md`
  - Active terminal tabs and their recent activity

### Context Injection

Uses the same context lifecycle as regular terminals (`ContextBridge`), but with a **different injection template**:

| Regular Terminal | Supervisor Terminal |
|-----------------|-------------------|
| Project GENOME.md | Supervisor GENOME.md + all project summaries |
| Project session history | Cross-project activity feed |
| Threshold: project-scoped | Threshold: global-scoped |
| Extracts: file mods, errors, decisions | Extracts: cross-project patterns, conflicts, meta-decisions |

### Extraction

When the supervisor outputs text, Impulse extracts:
- **Cross-project decisions** → saved to `~/.impulse/supervisor/GENOME.md`
- **Routing suggestions** → which project/agent should handle a task
- **Conflict resolutions** → when the supervisor resolves a cross-project conflict
- **Meta-insights** → patterns observed across projects

These are extracted using the same `OutputExtractor` + `parser.rs` infrastructure, but with a supervisor-specific insight type classification.

### Per-Project Insight Sync

When the supervisor analyzes a specific project, its findings flow **down** to that project:
- Supervisor decides "Project A should use async-trait 0.1" → gets appended to `<project-a>/.impulse/GENOME.md`
- This uses the existing `DelegationSpec` pattern — the supervisor "delegates" a decision to a project

When a project terminal makes a decision, it flows **up** to the supervisor:
- Project terminal decides "We're using SQLite for this" → summarized into supervisor's project insight file
- This uses the existing `ExtractedInsight` pipeline

---

## GUI Integration

### Where the Supervisor Tab Lives

The supervisor tab lives in the **same tab bar** as other terminals in the Agents view. It is:
- Pinned (cannot be closed accidentally — requires Ctrl+Shift+W or explicit close)
- Always the first tab (leftmost position)
- Visually distinguished: purple accent, "Supervisor" label with cross-project icon
- Uses the same `TerminalPanel` widget as other tabs

### Replacing the Agent Panel

The current agent panel (`impulse-gui/src/agent_panel/`) is replaced by the supervisor terminal tab. The side panel goes away. Benefits:
- Consistent UX — everything is a terminal
- Full PTY features — scrollback, copy, search, resize
- Context lifecycle — extraction, injection, compaction detection
- Keyboard shortcuts — same as any other terminal tab

### Activity Feed

The current activity feed (cross-pane insights) becomes an **overlay on the supervisor tab**, triggered by a keyboard shortcut (Ctrl+Shift+A). This shows the same data but rendered inside the terminal context.

Alternatively, the activity feed becomes a **context injection** — when new activity happens in other tabs, it's injected into the supervisor terminal as a formatted text block that the supervisor agent can read and respond to.

---

## Implementation Phases

### Phase A: Supervisor Tab Foundation
- Add `SupervisorTab` variant to the tab system
- Spawn supervisor as a pinned first tab using Claude Code
- Inject supervisor system prompt on spawn
- Store supervisor state at `~/.impulse/supervisor/`

### Phase B: Two-Tier Memory Wiring
- Create `~/.impulse/supervisor/GENOME.md` and `HISTORY.jsonl`
- Extract cross-project insights from supervisor output
- Inject project summaries into supervisor context
- Sync supervisor decisions down to project GENOMEs

### Phase C: Activity Injection
- Route cross-pane activity to supervisor terminal as context injections
- Supervisor sees what other agents are doing in real-time
- Replace agent panel with supervisor tab

### Phase D: Agent Panel Deprecation
- Remove `impulse-gui/src/agent_panel/` module
- Migrate slash commands (/clear, /help, /status) to supervisor injection prompts
- Migrate proposal system to delegation tracking (already exists from PR #8)

---

## What This Changes

| Before | After |
|--------|-------|
| Supervisor is a custom chat widget | Supervisor is a real terminal tab |
| Supervisor has its own UI (message bubbles) | Supervisor uses the same PTY renderer as other agents |
| Supervisor state lives nowhere persistent | Supervisor state lives at `~/.impulse/supervisor/` |
| Project agents don't know about each other | Supervisor bridges projects via context injection |
| Agent panel is a SidePanel | No side panel — supervisor is a tab |
| Activity feed is rendered in agent panel | Activity is injected into supervisor terminal |

## What This Preserves

- All existing terminal infrastructure (PTY, vt100, context bridge, renderer)
- All existing context lifecycle (extraction, injection, compaction detection)
- All existing delegation tracking (from PR #8)
- Per-project `.impulse/` isolation
- Stewardship cross-project patterns

## Key Design Decisions

1. **The supervisor IS an agent** — not a custom UI. It's Claude Code (or OpenCode/Codex) running in a PTY with a privileged context injection.
2. **Two-tier memory** — supervisor at `~/.impulse/supervisor/`, projects at `<project>/.impulse/`. Insights flow bidirectionally.
3. **Same widget, different context** — uses `TerminalPanel` like everything else. The difference is what gets injected, not how it renders.
4. **Graceful degradation** — if the supervisor terminal isn't spawned, everything works as it does today. The supervisor is additive, not required.
