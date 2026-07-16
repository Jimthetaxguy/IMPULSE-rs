# Horizon Goals Framework — Specification

> **NONCANONICAL AND OUTDATED DRAFT.** This point-in-time proposal predates the current Impulse
> product framing and desktop implementation. Its memory-sidecar description, egui migration state,
> crate/test counts, dependencies, UI inventory, and implementation roadmap must not be treated as
> current truth or an approved build plan. Use [`../VISION.md`](../VISION.md),
> [`../docs/spec/RUST-CANONICAL-CONTRACT.md`](../docs/spec/RUST-CANONICAL-CONTRACT.md),
> [`../docs/spec/USER-STORY-MAP.md`](../docs/spec/USER-STORY-MAP.md), and
> [`../docs/spec/TEST-TRACEABILITY.md`](../docs/spec/TEST-TRACEABILITY.md) for current authority.

> **Version:** 0.1.0-draft
> **Author:** James Pustorino
> **Date:** 2026-06-25
> **Status:** Draft
> **Crate:** `impulse-rs` (primary), `impulse-ops` (shared types), `impulse-desktop` (desktop rendering)
> **Dependencies:** ratatui 0.28, chrono, serde, uuid

---

## Table of Contents

1. [Current State Assessment](#1-current-state-assessment)
2. [Longer-Horizon Goals Framework](#2-longer-horizon-goals-framework)
3. [UI/UX Design](#3-uiux-design)
4. [Visual Elements](#4-visual-elements)
5. [Integration Points](#5-integration-points)
6. [Implementation Roadmap](#6-implementation-roadmap)

---

## 1. Current State Assessment

### What Impulse Does Today

Impulse is a terminal-native sidecar for AI coding agents. It runs alongside Claude Code and Codex, recording session history, file changes, tool usage, and decisions. Its core value proposition is *session continuity* — when an agent spins up tomorrow, Impulse can inject yesterday's context.

The system operates in three modes: **direct** (stateless CLI commands), **daemon** (long-running Unix socket IPC), and **desktop** (Dioxus shell, in migration from frozen egui).

### Architecture Summary

```
impulse-rs/              Main binary — CLI + daemon + ratatui TUI (1,326 tests)
impulse-rs/impulse-ops/  Shared types across crate boundaries (4 tests)
impulse-rs/impulse-term/ PTY + vt100 + session core (114 tests)
impulse-rs/impulse-desktop/ Dioxus desktop shell scaffold (116 tests)
```

**Execution model:** The ratatui TUI runs a 200ms poll loop with a 5-second MIER (Monitor, Inject, Extract, Refine) pipeline tick. The daemon serves JSON-line IPC over a Unix socket. Direct-mode commands are fire-and-forget.

### Existing Data Model (Relevant Structures)

**What exists for state:**

| Structure | Location | Purpose |
|-----------|----------|---------|
| `Session` | `state/session.rs` | Per-agent session with status, files, tools, role, delegation |
| `HistoryEntry` | `state/persistence.rs` | Append-only session log (HISTORY.jsonl) |
| `Genome` | `memory/mod.rs` | Persistent decisions, preferences, constraints |
| `Config` | `state/config.rs` | 69 runtime configuration keys |
| `TrackedDelegation` | `delegation/types.rs` | Task delegation between agents with state machine |
| `ExtractedInsight` | `context_lifecycle/types.rs` | Auto-extracted observations from PTY output |
| `AgentIntent` | `context_lifecycle/intent.rs` | Classified agent activity with `goal` field |
| `CrossProjectMemory` | `stewardship/cross_project.rs` | Patterns and learnings across projects |
| `ConflictEntry` | `state/persistence.rs` | File conflict tracking with resolution |

**What does NOT exist:**

- No goal, objective, milestone, or target data structures
- No task tracking (delegations exist but are agent-scoped, not user-scoped)
- No temporal planning (daily/weekly/monthly/quarterly horizons)
- No progress measurement or completion tracking
- No dependency mapping between work items
- No horizon-aware views in the TUI

The closest existing concept is `AgentIntent.goal` — a string auto-classified from PTY output (e.g., "refactoring the auth module"). It is transient, unstructured, and disconnected from any persistence layer.

### Existing UI State

The TUI has 10 tabs: Dashboard, Sessions, Timeline, History, Genome, Search, Analytics, Chat, Config, Stewardship. Navigation uses tab/arrow keys with Alt+1-9 for project switching and Ctrl+1-9 for terminal panes.

All rendering follows a uniform pattern: build `Vec<Line>` of styled `Span` elements, wrap in a bordered `Paragraph` block. Custom visualization helpers provide `sparkline()`, `horizontal_bar()`, `gauge()` functions returning Unicode strings — no ratatui `StatefulWidget` implementations exist.

**Design system gap:** The TUI uses a basic cyan-accent palette (`Color::Rgb(20,25,35)` panels, `Color::Cyan` accent). The desktop shell has a full Phosphor/Aperture design system with amber CRT theming (`#ffb01a` amber, `#ffe39a` amber-hot, scanline overlays, vignette). These are completely disconnected. This spec addresses that gap for the TUI.

### What This Spec Adds

A multi-horizon goal framework that gives Impulse a *forward-looking* dimension. Today, Impulse looks backward (what happened). Horizon Goals look forward (what should happen, and how does today's work connect to longer arcs). This transforms Impulse from a memory system into a *purposeful memory system* — one that can answer "why are we doing this?" and "what's next?"

---

## 2. Longer-Horizon Goals Framework

### Design Philosophy

Goals are not tasks. A task is "implement OAuth login." A goal is "ship user authentication so we can launch the beta." Goals have horizons (when), decomposition (how), and purpose (why). The framework must:

1. **Feel lightweight** — Adding a goal should take one command or one modal. If it feels like JIRA, we failed.
2. **Connect upward** — Every daily task should be traceable to a longer-horizon goal, but this connection can be loose. Not every commit needs a parent goal.
3. **Auto-enrich** — The MIER pipeline already extracts insights. Goal progress should be partially inferred from what agents actually do, not solely from manual check-offs.
4. **Survive across sessions** — Goals persist in `.impulse/` and are available to context injection. When an agent starts, it can know "this project's current quarterly goal is X."

### 2.1 Horizon Levels

```
Horizon::Day      ─── What I'm doing today (auto-populated from sessions + manual)
Horizon::Week     ─── Sprint-scale arcs (3-7 day clusters)
Horizon::Month    ─── Monthly milestones and deliverables
Horizon::Quarter  ─── 90-day strategic objectives
Horizon::Year     ─── Annual north-star goals
Horizon::Someday  ─── Parking lot for ideas without a deadline
```

```rust
/// src/goals/types.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Horizon {
    Day,
    Week,
    Month,
    Quarter,
    Year,
    Someday,
}

impl Horizon {
    /// Typical duration for progress calculation
    pub fn typical_days(&self) -> Option<u32> {
        match self {
            Horizon::Day => Some(1),
            Horizon::Week => Some(7),
            Horizon::Month => Some(30),
            Horizon::Quarter => Some(90),
            Horizon::Year => Some(365),
            Horizon::Someday => None,
        }
    }

    /// Display label for TUI rendering
    pub fn label(&self) -> &'static str {
        match self {
            Horizon::Day => "TODAY",
            Horizon::Week => "THIS WEEK",
            Horizon::Month => "THIS MONTH",
            Horizon::Quarter => "THIS QUARTER",
            Horizon::Year => "THIS YEAR",
            Horizon::Someday => "SOMEDAY",
        }
    }
}
```

### 2.2 Core Data Model

```rust
/// src/goals/types.rs

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A goal at any horizon level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    /// Unique identifier. Format: "goal-{uuid-v4-short}" (first 8 chars of UUID).
    pub id: String,

    /// Human-readable title. Imperative mood preferred: "Ship beta auth" not "Beta auth."
    pub title: String,

    /// Optional longer description. Markdown-compatible.
    pub description: Option<String>,

    /// Which time horizon this goal lives at.
    pub horizon: Horizon,

    /// Current status.
    pub status: GoalStatus,

    /// Optional parent goal ID. Day goals often parent to Week goals,
    /// Week to Month, etc. Cross-horizon parenting is allowed but not required.
    pub parent_id: Option<String>,

    /// Child goal IDs. Maintained bidirectionally with parent_id.
    pub children: Vec<String>,

    /// IDs of goals that must complete before this one can start.
    pub blocked_by: Vec<String>,

    /// IDs of goals that this one blocks.
    pub blocks: Vec<String>,

    /// Optional target date. For Day goals, this is the date itself.
    /// For Week/Month/Quarter/Year, this is the end of the period.
    pub target_date: Option<NaiveDate>,

    /// When this goal was created.
    pub created_at: DateTime<Utc>,

    /// When this goal was last modified (status change, progress update, etc.)
    pub updated_at: DateTime<Utc>,

    /// When this goal was completed or abandoned.
    pub closed_at: Option<DateTime<Utc>>,

    /// Progress tracking. 0.0 to 1.0.
    /// For leaf goals: manually set or inferred from linked sessions.
    /// For parent goals: computed from children's weighted progress.
    pub progress: f32,

    /// How progress is calculated for this goal.
    pub progress_mode: ProgressMode,

    /// Tags for filtering and grouping.
    pub tags: Vec<String>,

    /// Key results / acceptance criteria. Each has its own completion state.
    pub milestones: Vec<Milestone>,

    /// Links to Impulse sessions that contributed to this goal.
    /// Auto-populated when sessions are tagged or when MIER detects relevant work.
    pub linked_sessions: Vec<LinkedSession>,

    /// Links to external resources (GitHub issues, PRs, docs).
    pub external_links: Vec<ExternalLink>,

    /// Arbitrary metadata for extensibility.
    pub metadata: HashMap<String, String>,

    /// Project scope. None = cross-project / personal.
    pub project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    /// Not started yet.
    NotStarted,
    /// Active work in progress.
    Active,
    /// Temporarily paused (context switch, blocked, deprioritized).
    Paused,
    /// All milestones met, goal achieved.
    Completed,
    /// Explicitly abandoned (with reason in metadata).
    Abandoned,
    /// Deferred to a future horizon (with new target in metadata).
    Deferred,
}

impl GoalStatus {
    pub fn is_open(&self) -> bool {
        matches!(self, GoalStatus::NotStarted | GoalStatus::Active | GoalStatus::Paused)
    }

    pub fn is_closed(&self) -> bool {
        matches!(self, GoalStatus::Completed | GoalStatus::Abandoned | GoalStatus::Deferred)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressMode {
    /// Progress = average of children's progress, weighted by child count.
    FromChildren,
    /// Progress = count(completed milestones) / count(total milestones).
    FromMilestones,
    /// Progress is set manually (0.0 - 1.0).
    Manual,
    /// Progress is inferred from linked session activity.
    /// Uses file-change velocity + tool-usage density as proxy.
    Inferred,
}

/// A concrete, verifiable sub-outcome of a goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Milestone {
    /// Short identifier within the goal. "ms-1", "ms-2", etc.
    pub id: String,
    /// What needs to be true for this milestone to be complete.
    pub description: String,
    /// Is this milestone done?
    pub completed: bool,
    /// When was it completed?
    pub completed_at: Option<DateTime<Utc>>,
    /// Optional verification command (e.g., "cargo test --lib").
    pub verification: Option<String>,
}

/// A reference to an Impulse session that contributed work toward a goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkedSession {
    /// Impulse session ID.
    pub session_id: String,
    /// Session name at time of linking.
    pub session_name: String,
    /// When the link was established.
    pub linked_at: DateTime<Utc>,
    /// How the link was established.
    pub link_source: LinkSource,
    /// Files touched in this session that are relevant to the goal.
    pub relevant_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkSource {
    /// User explicitly tagged the session to this goal.
    Manual,
    /// MIER pipeline detected overlap (file paths, intent keywords).
    Inferred,
    /// Session was started via a goal-scoped delegation.
    Delegation,
}

/// A link to an external resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExternalLink {
    pub url: String,
    pub title: Option<String>,
    pub link_type: ExternalLinkType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalLinkType {
    GitHubIssue,
    GitHubPR,
    Document,
    Url,
}
```

### 2.3 Goal Store

Goals persist in `.impulse/GOALS.json` using the same atomic write pattern as all other Impulse state files.

```rust
/// src/goals/store.rs

/// The on-disk representation of all goals for a project.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GoalStore {
    /// Schema version for forward compatibility.
    pub version: u32,  // starts at 1

    /// All goals, keyed by goal ID.
    pub goals: HashMap<String, Goal>,

    /// Last modification timestamp.
    pub updated_at: DateTime<Utc>,
}

impl GoalStore {
    pub const FILENAME: &'static str = "GOALS.json";

    /// Load from .impulse/GOALS.json. Returns empty store if file doesn't exist.
    pub fn load(storage: &Storage) -> Result<Self>;

    /// Atomic write to .impulse/GOALS.json.
    pub fn save(&self, storage: &Storage) -> Result<()>;

    /// Add a goal. Maintains parent/child bidirectional links.
    pub fn add_goal(&mut self, goal: Goal) -> Result<&Goal>;

    /// Update a goal by ID. Recalculates parent progress if needed.
    pub fn update_goal(&mut self, id: &str, f: impl FnOnce(&mut Goal)) -> Result<()>;

    /// Get goals at a specific horizon, optionally filtered by status.
    pub fn by_horizon(&self, horizon: Horizon, status: Option<GoalStatus>) -> Vec<&Goal>;

    /// Get the full ancestor chain for a goal (child → parent → grandparent → ...).
    pub fn ancestor_chain(&self, id: &str) -> Vec<&Goal>;

    /// Get the full descendant tree for a goal.
    pub fn descendant_tree(&self, id: &str) -> Vec<&Goal>;

    /// Recalculate progress for a goal and all its ancestors.
    pub fn recalculate_progress(&mut self, id: &str);

    /// Find goals whose target_date has passed without completion.
    pub fn overdue(&self) -> Vec<&Goal>;

    /// Find goals with no activity in the last N days.
    pub fn stale(&self, days: u32) -> Vec<&Goal>;

    /// Goals that are blocked (all blockers incomplete).
    pub fn blocked(&self) -> Vec<&Goal>;

    /// Daily digest: today's goals + overdue + unblocked.
    pub fn daily_surface(&self, date: NaiveDate) -> DailySurface;
}

/// What shows up for a given day — the "what should I work on?" answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySurface {
    pub date: NaiveDate,
    /// Day-horizon goals targeted at this date.
    pub today_goals: Vec<Goal>,
    /// Goals past their target date.
    pub overdue: Vec<Goal>,
    /// Goals that just became unblocked.
    pub newly_unblocked: Vec<Goal>,
    /// Active goals at Week+ horizons for context.
    pub active_arcs: Vec<GoalArc>,
}

/// A compressed view of a longer-horizon goal for daily display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalArc {
    pub goal_id: String,
    pub title: String,
    pub horizon: Horizon,
    pub progress: f32,
    pub days_remaining: Option<i64>,
    pub child_count: usize,
    pub completed_children: usize,
}
```

### 2.4 Dependency Mapping

Dependencies use a simple directed graph stored within the `Goal` structs (`blocked_by` / `blocks` fields). Cycle detection is enforced on write.

```rust
/// src/goals/deps.rs

/// Validates that adding a dependency does not create a cycle.
pub fn validate_dependency(
    store: &GoalStore,
    from_id: &str,  // the goal being blocked
    by_id: &str,    // the blocker
) -> Result<(), GoalError>;

/// Returns a topological ordering of goals within a horizon.
/// Goals with no blockers come first.
pub fn topological_sort(goals: &[&Goal]) -> Result<Vec<&Goal>, GoalError>;

/// Finds the critical path through a set of goals
/// (longest chain of dependencies to the final goal).
pub fn critical_path(store: &GoalStore, root_id: &str) -> Vec<&Goal>;

#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("Goal not found: {id}")]
    NotFound { id: String },

    #[error("Dependency cycle detected: {chain}")]
    CycleDetected { chain: String },

    #[error("Cannot modify closed goal: {id} (status: {status})")]
    GoalClosed { id: String, status: String },

    #[error("Invalid progress value: {value} (must be 0.0-1.0)")]
    InvalidProgress { value: f32 },

    #[error("Goal store I/O error: {0}")]
    Storage(#[from] anyhow::Error),
}
```

### 2.5 Cross-Project Goals

Goals with `project: None` are cross-project / personal goals. They live in the supervisor-level store at `~/.impulse/supervisor/GOALS.json` (alongside the existing `CrossProjectMemory`). Project-scoped goals live in the project's `.impulse/GOALS.json`.

The `DailySurface` computation merges both stores, giving a unified view of "everything I should think about today."

---

## 3. UI/UX Design

### 3.1 Goals Tab (Tab 10 or replaces Stewardship)

The Goals view is a new TUI tab. Given that the current 10-tab layout is already crowded, the recommendation is to **replace the Stewardship tab (index 9)** with a Goals tab that subsumes stewardship's cleanup-proposal function as a sub-pane. Alternative: add as tab 10 with `Shift+G` shortcut.

**Primary layout (3-pane horizontal split):**

```
┌─────────────────────────────────────────────────────────────────────┐
│ ◈ HORIZON GOALS                                           [G]oals │
├──────────────┬──────────────────────────────┬────────────────────────┤
│  HORIZONS    │  GOAL LIST                   │  DETAIL / TREE        │
│              │                              │                       │
│ ▸ TODAY    3 │  ● Ship OAuth login     78%  │  ◈ Ship beta auth     │
│   THIS WK  2 │  ○ Write migration tests 0%  │    ├─ Ship OAuth  78% │
│   THIS MO  5 │  ◐ Refactor DB layer   45%  │    │  ├─ ms-1 ✓ model │
│   THIS QTR 3 │  ● Review PR #142      90%  │    │  ├─ ms-2 ✓ route │
│   THIS YR  2 │                              │    │  └─ ms-3 … tests │
│   SOMEDAY  8 │  ── Overdue ──               │    ├─ Migration   0%  │
│              │  ⚠ Fix CI pipeline    [3d]  │    └─ DB refactor 45% │
│              │                              │                       │
│              │  ── Blocked ──               │  PROGRESS ████░░ 41%  │
│              │  ◌ Deploy staging  ←OAuth    │  TARGET   2026-07-15  │
│              │                              │  SESSIONS 12 linked   │
├──────────────┴──────────────────────────────┴────────────────────────┤
│ [a]dd  [e]dit  [m]ilestone  [c]omplete  [p]arent  [/]search  [?]   │
└─────────────────────────────────────────────────────────────────────┘
```

**Left pane — Horizon selector:** Vertical list of horizon levels with active goal counts. The selected horizon filters the center pane. Visual indicator: filled dot for horizons with overdue items.

**Center pane — Goal list:** Goals at the selected horizon, grouped by status (Active first, then Overdue, then Blocked, then NotStarted). Each row shows: status icon, title (truncated), progress percentage. Selected row highlighted with accent color.

**Right pane — Detail / Tree view:** When a goal is selected, shows either:
- **Detail view** (default): Full title, description, milestones with checkmarks, progress bar, target date, linked sessions count, tags.
- **Tree view** (toggle with `t`): Ancestor/descendant tree showing how this goal connects to other horizons. Uses box-drawing characters for tree lines.

### 3.2 Dashboard Integration

The existing Dashboard tab (index 0) gains a new panel: **"Active Arcs"** — a compact summary of the current horizon context.

```
┌─ ACTIVE ARCS ──────────────────────────────────────┐
│                                                     │
│  QTR  Ship beta auth          ████████░░░░░  62%   │
│  MO   Complete OAuth module   ██████████░░░  78%   │
│  WK   Close out PR backlog    █████░░░░░░░░  38%   │
│                                                     │
│  TODAY  3 goals  ·  1 overdue  ·  2 blocked        │
│                                                     │
└─────────────────────────────────────────────────────┘
```

This panel occupies 8 lines in the Dashboard layout, positioned below the engine indicator and above the stats panel.

### 3.3 Timeline Enhancement

The existing Timeline tab (index 2) gains goal markers. When a session is linked to a goal, its timeline entry shows the goal's horizon badge:

```
  14:32  [QTR] claude-code  "implementing OAuth callback handler"
  14:28  [DAY] codex        "fixing test assertions in auth_test.rs"
  14:15        claude-code  "exploring options for rate limiting"
```

### 3.4 Modal Interactions

**Add Goal modal** (`a` key in Goals tab):

```
┌─ NEW GOAL ──────────────────────────────────────────┐
│                                                      │
│  Title:    [Ship OAuth login________________________]│
│  Horizon:  < THIS WEEK >    (←/→ to change)          │
│  Target:   [2026-07-01]     (optional)               │
│  Parent:   [Ship beta auth] (Tab to search)          │
│  Tags:     [auth, backend]                           │
│                                                      │
│  Milestones (Enter to add, Backspace to remove):     │
│    1. [Define OAuth model structs___________________]│
│    2. [Implement callback route____________________] │
│    3. [Write integration tests_____________________] │
│    4. [_____________________________________________]│
│                                                      │
│              [Save]  [Cancel]                        │
└──────────────────────────────────────────────────────┘
```

**Quick-link modal** (when in Sessions/Timeline tab, press `g` on a session):

```
┌─ LINK TO GOAL ──────────────────────────┐
│  Session: claude-code-2026-06-25-14h    │
│                                          │
│  Search: [oauth_____]                   │
│                                          │
│  ▸ Ship OAuth login          [WK] 78%   │
│    Complete auth module      [MO] 62%   │
│    Ship beta auth            [QTR] 41%  │
│                                          │
│  Enter to link  ·  Esc to cancel        │
└──────────────────────────────────────────┘
```

### 3.5 Navigation & Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `G` | Any tab | Jump to Goals tab |
| `a` | Goals tab | Add new goal modal |
| `e` | Goals tab, goal selected | Edit goal modal |
| `m` | Goals tab, goal selected | Toggle milestone completion |
| `c` | Goals tab, goal selected | Mark goal completed |
| `p` | Goals tab, goal selected | Set/change parent goal |
| `t` | Goals tab, goal selected | Toggle detail/tree view in right pane |
| `1-6` | Goals tab | Switch horizon (1=Day, 2=Week, ... 6=Someday) |
| `/` | Goals tab | Search goals by title/tag |
| `g` | Sessions/Timeline tab | Link session to goal |
| `Enter` | Goals tab, goal selected | Expand goal (show children in center pane) |
| `Backspace` | Goals tab, drilled into children | Go back to parent level |

---

## 4. Visual Elements

### 4.1 Aperture Design System — TUI Port

The desktop shell's Phosphor palette needs a TUI equivalent. These constants bridge the gap between the desktop's CSS custom properties and ratatui's `Color::Rgb`.

```rust
/// src/ui/aperture.rs — Aperture design tokens for ratatui
///
/// Port of the Phosphor palette from impulse-desktop/src/theme.rs
/// and impulse-desktop/assets/impulse_crt.css to ratatui Color values.
///
/// Design principle: "Chrome stays calm; bloom is reserved for the
/// brand lockup and one pending signal."

use ratatui::style::Color;

// ── Phosphor palette ────────────────────────────────────────────────
pub const P_AMBER:      Color = Color::Rgb(0xFF, 0xB0, 0x1A);  // #ffb01a — primary warm accent
pub const P_AMBER_HOT:  Color = Color::Rgb(0xFF, 0xE3, 0x9A);  // #ffe39a — brand glow, highlights
pub const P_ORANGE:     Color = Color::Rgb(0xFF, 0x6A, 0x00);  // #ff6a00 — warning, overdue
pub const P_RED:        Color = Color::Rgb(0xFF, 0x3B, 0x1F);  // #ff3b1f — error, blocked, abandoned
pub const P_BLUE:       Color = Color::Rgb(0x5B, 0x63, 0xFF);  // #5b63ff — info, external links
pub const P_CYAN:       Color = Color::Rgb(0x2F, 0xD0, 0xFF);  // #2fd0ff — active data, sessions
pub const P_TEAL:       Color = Color::Rgb(0x2F, 0xD6, 0xA8);  // #2fd6a8 — metadata, tags
pub const P_LIME:       Color = Color::Rgb(0xB6, 0xF0, 0x3C);  // #b6f03c — success, completed
pub const P_MAGENTA:    Color = Color::Rgb(0xFF, 0x3D, 0x81);  // #ff3d81 — reserved
pub const P_YELLOW:     Color = Color::Rgb(0xFF, 0xD2, 0x3F);  // #ffd23f — paused, pending

// ── Foreground ramp ─────────────────────────────────────────────────
pub const FG_PRIMARY:   Color = Color::Rgb(0xD6, 0xF3, 0xFF);  // #d6f3ff — main text
pub const FG_SECONDARY: Color = Color::Rgb(0x8F, 0xB8, 0xC8);  // #8fb8c8 — dimmed text
pub const FG_LABEL:     Color = Color::Rgb(0x5D, 0x80, 0x90);  // #5d8090 — section headings
pub const FG_FAINT:     Color = Color::Rgb(0x3A, 0x55, 0x62);  // #3a5562 — borders, idle elements

// ── Chrome (backgrounds) ────────────────────────────────────────────
pub const BG_PANEL:     Color = Color::Rgb(0x0A, 0x0E, 0x14);  // #0a0e14 — main panel bg (darker than current)
pub const BG_SURFACE:   Color = Color::Rgb(0x11, 0x18, 0x22);  // #111822 — raised surface
pub const BG_SELECTED:  Color = Color::Rgb(0x1A, 0x24, 0x32);  // #1a2432 — selected row highlight
pub const BG_MODAL:     Color = Color::Rgb(0x0D, 0x12, 0x1C);  // #0d121c — modal overlay bg

// ── Semantic mappings for Goals ─────────────────────────────────────
pub const GOAL_ACTIVE:     Color = P_AMBER;      // Active work
pub const GOAL_COMPLETED:  Color = P_LIME;       // Done
pub const GOAL_BLOCKED:    Color = P_RED;         // Blocked by dependency
pub const GOAL_PAUSED:     Color = P_YELLOW;      // On hold
pub const GOAL_OVERDUE:    Color = P_ORANGE;      // Past target date
pub const GOAL_NOT_STARTED:Color = FG_LABEL;      // Not yet begun
pub const GOAL_DEFERRED:   Color = FG_FAINT;      // Pushed out
pub const GOAL_ABANDONED:  Color = FG_FAINT;      // Explicitly dropped

// ── Progress bar colors ─────────────────────────────────────────────
pub const PROGRESS_FILL:   Color = P_AMBER;       // Filled portion
pub const PROGRESS_EMPTY:  Color = FG_FAINT;      // Empty portion
pub const PROGRESS_FULL:   Color = P_LIME;        // 100% complete

// ── Horizon badges ──────────────────────────────────────────────────
pub const HORIZON_DAY:     Color = P_AMBER_HOT;   // Brightest — most immediate
pub const HORIZON_WEEK:    Color = P_AMBER;
pub const HORIZON_MONTH:   Color = P_CYAN;
pub const HORIZON_QUARTER: Color = P_BLUE;
pub const HORIZON_YEAR:    Color = P_TEAL;
pub const HORIZON_SOMEDAY: Color = FG_LABEL;      // Dimmest — least urgent
```

### 4.2 Status Icons

Unicode characters for goal status indicators, chosen for terminal compatibility:

```rust
/// src/ui/goal_icons.rs

/// Status icons — single-character indicators
pub const ICON_ACTIVE:      &str = "●";  // U+25CF BLACK CIRCLE
pub const ICON_NOT_STARTED: &str = "○";  // U+25CB WHITE CIRCLE
pub const ICON_HALF:        &str = "◐";  // U+25D0 CIRCLE LEFT HALF BLACK
pub const ICON_BLOCKED:     &str = "◌";  // U+25CC DOTTED CIRCLE
pub const ICON_COMPLETED:   &str = "✓";  // U+2713 CHECK MARK
pub const ICON_ABANDONED:   &str = "✕";  // U+2715 MULTIPLICATION X
pub const ICON_PAUSED:      &str = "⏸";  // U+23F8 DOUBLE VERT BAR (pause)
pub const ICON_OVERDUE:     &str = "⚠";  // U+26A0 WARNING SIGN
pub const ICON_DEFERRED:    &str = "→";  // U+2192 RIGHTWARDS ARROW

/// Milestone icons
pub const MS_DONE:          &str = "✓";  // Completed milestone
pub const MS_PENDING:       &str = "…";  // Pending milestone

/// Tree connectors (box-drawing)
pub const TREE_BRANCH:      &str = "├─";  // Mid-level child
pub const TREE_LAST:        &str = "└─";  // Last child
pub const TREE_PIPE:        &str = "│ ";   // Continuation line
pub const TREE_SPACE:       &str = "  ";   // No continuation

/// Horizon badges (compact, for inline use)
pub fn horizon_badge(h: Horizon) -> &'static str {
    match h {
        Horizon::Day     => "[DAY]",
        Horizon::Week    => "[WK]",
        Horizon::Month   => "[MO]",
        Horizon::Quarter => "[QTR]",
        Horizon::Year    => "[YR]",
        Horizon::Someday => "[∞]",
    }
}
```

### 4.3 Progress Visualization Widget

A custom progress bar that renders in the Aperture amber style with CRT-inspired segment characters:

```rust
/// src/ui/widgets/progress_bar.rs

use ratatui::prelude::*;
use ratatui::widgets::Widget;

/// Aperture-styled progress bar.
///
/// Renders as: ████████░░░░░ 78%
/// Uses block characters for the filled portion and light shade for empty.
/// At 100%, the entire bar renders in P_LIME.
pub struct ApertureProgressBar {
    /// Progress value, 0.0 to 1.0.
    progress: f32,
    /// Total width in columns (including percentage label).
    width: u16,
    /// Whether to show the percentage label.
    show_label: bool,
    /// Override fill color (default: P_AMBER, at 100%: P_LIME).
    fill_color: Option<Color>,
}

impl ApertureProgressBar {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            width: 20,
            show_label: true,
            fill_color: None,
        }
    }

    pub fn width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }
}

impl Widget for ApertureProgressBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let label = if self.show_label {
            format!(" {:>3}%", (self.progress * 100.0) as u32)
        } else {
            String::new()
        };

        let bar_width = (area.width as usize).saturating_sub(label.len());
        let filled = ((bar_width as f32) * self.progress) as usize;
        let empty = bar_width.saturating_sub(filled);

        let fill_color = if self.progress >= 1.0 {
            super::aperture::PROGRESS_FULL
        } else {
            self.fill_color.unwrap_or(super::aperture::PROGRESS_FILL)
        };

        // Render filled portion: █ characters in amber/lime
        let filled_str: String = "█".repeat(filled);
        // Render empty portion: ░ characters in faint
        let empty_str: String = "░".repeat(empty);

        let line = Line::from(vec![
            Span::styled(filled_str, Style::default().fg(fill_color)),
            Span::styled(empty_str, Style::default().fg(super::aperture::PROGRESS_EMPTY)),
            Span::styled(label, Style::default().fg(super::aperture::FG_SECONDARY)),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
```

### 4.4 Goal Tree Widget

Renders the hierarchical relationship between goals using box-drawing characters:

```rust
/// src/ui/widgets/goal_tree.rs

use ratatui::prelude::*;
use ratatui::widgets::Widget;

/// Renders a goal decomposition tree.
///
/// Example output:
///   ◈ Ship beta auth              [QTR] ████████░░ 62%
///   ├─ ● Ship OAuth login         [WK]  ██████████ 78%
///   │  ├─ ✓ ms-1: Define models
///   │  ├─ ✓ ms-2: Implement route
///   │  └─ … ms-3: Write tests
///   ├─ ○ Write migration tests    [WK]  ░░░░░░░░░░  0%
///   └─ ◐ Refactor DB layer        [MO]  ████░░░░░░ 45%
pub struct GoalTree<'a> {
    store: &'a GoalStore,
    root_id: &'a str,
    /// How many levels deep to render.
    max_depth: usize,
    /// Whether to show milestones for leaf goals.
    show_milestones: bool,
    /// Currently selected goal ID (for highlighting).
    selected: Option<&'a str>,
}

// Implementation builds Vec<Line> with proper indentation,
// tree connectors, status icons, horizon badges, and inline progress bars.
```

### 4.5 Horizon Sparkline

A compact multi-horizon progress overview using the existing `sparkline()` pattern but with horizon-aware coloring:

```rust
/// Renders a compact sparkline showing progress across all horizons.
///
/// Output:  DAY ██░  WK ████░  MO ██░░░  QTR █░░░░░  YR ░░░░░░░
///
/// Each horizon segment uses its badge color. Width is proportional
/// to the horizon's typical duration (Day=3, Week=7, Month=10, etc.)
pub fn horizon_sparkline(store: &GoalStore) -> Vec<Span<'static>>;
```

### 4.6 Daily Briefing Panel

A compact "what to work on today" panel for the Dashboard:

```rust
/// Renders the daily briefing panel.
///
/// ┌─ TODAY ── Wed Jun 25 ────────────────────────────┐
/// │                                                   │
/// │  ● Ship OAuth login         ██████████ 78%  [WK] │
/// │  ○ Write migration tests    ░░░░░░░░░░  0%  [WK] │
/// │  ● Review PR #142           █████████░ 90%  [DAY]│
/// │                                                   │
/// │  ⚠ 1 overdue  ·  ◌ 1 blocked  ·  3 active       │
/// │                                                   │
/// │  ARC  Ship beta auth        ████████░░ 62%  [QTR]│
/// └──────────────────────────────────────────────────┘
pub fn render_daily_briefing(
    f: &mut Frame,
    area: Rect,
    surface: &DailySurface,
    store: &GoalStore,
);
```

---

## 5. Integration Points

### 5.1 MIER Pipeline → Goal Progress

The existing MIER pipeline extracts `InsightType::TaskCompleted` and `InsightType::FileModified` from PTY output. These can feed goal progress inference:

```rust
/// src/goals/inference.rs

/// Called on each MIER extraction cycle (every 30s per pane).
/// Checks if any extracted insights match active goals and updates progress.
pub fn process_insights_for_goals(
    insights: &[ExtractedInsight],
    store: &mut GoalStore,
    sessions: &HashMap<String, Session>,
) -> Vec<GoalProgressEvent> {
    // 1. For each FileModified insight, check if the file path
    //    appears in any active goal's linked_sessions[].relevant_files.
    //    If so, bump that goal's inferred activity score.
    //
    // 2. For each TaskCompleted insight, fuzzy-match the task description
    //    against milestone descriptions in active goals.
    //    If match confidence > 0.7, mark the milestone completed.
    //
    // 3. For each DecisionMade insight, check if the decision content
    //    matches any goal's description or tags.
    //    If so, auto-link the current session to that goal.
}
```

**Conservative defaults:** Auto-inference is opt-in via config key `goals_auto_inference = false`. When enabled, inferred progress changes are surfaced as MIER recommendations (like `FileConflict` and `ErrorAssist` today) rather than applied silently. The user confirms via the recommendation panel.

### 5.2 Context Injection → Goal Awareness

When injecting context into a new agent session, the injection engine adds the relevant goal context:

```xml
<!-- Added to the <impulse-context> XML block injected into Claude Code sessions -->
<active-goals>
  <goal horizon="quarter" progress="0.62" target="2026-09-30">
    Ship beta auth — complete OAuth, migrations, and DB refactor
  </goal>
  <goal horizon="week" progress="0.78" target="2026-06-28">
    Ship OAuth login — 2/3 milestones complete, tests remaining
  </goal>
</active-goals>
```

This gives the coding agent awareness of what the project is working toward, enabling it to make better prioritization decisions and connect its work to the larger arc.

### 5.3 Session Lifecycle → Goal Linking

When a session starts, Impulse checks if the session's `working_directory` or initial file set overlaps with any active goal's `linked_sessions[].relevant_files`. If so, the session is auto-linked (with `LinkSource::Inferred`).

When a session ends, its `HistoryEntry` is checked against active goals for file overlap, and the goal's `linked_sessions` list is updated.

### 5.4 Delegation → Goal Scoping

The existing delegation system (`DelegationSpec`) gains an optional `goal_id` field:

```rust
// Extension to existing DelegationSpec in delegation/types.rs
pub struct DelegationSpec {
    pub task: String,
    pub target_files: Vec<String>,
    pub constraints: Option<String>,
    pub max_depth: u8,
    pub restricted_tools: Vec<String>,
    pub goal_id: Option<String>,  // NEW: link delegation to a goal
}
```

When a delegation completes, its `DiffSummary` and `ToolInvocationRecord` are used to update goal progress. The delegation's session is auto-linked to the goal.

### 5.5 CLI Commands

New direct-mode commands following existing patterns in `handlers/`:

| Command | Description |
|---------|-------------|
| `impulse goal add <title> --horizon <H> [--parent <id>] [--target <date>]` | Add a new goal |
| `impulse goal list [--horizon <H>] [--status <S>]` | List goals with filters |
| `impulse goal show <id>` | Show goal detail with tree |
| `impulse goal update <id> --progress <0.0-1.0>` | Set manual progress |
| `impulse goal complete <id>` | Mark goal completed |
| `impulse goal link <goal-id> <session-id>` | Link a session to a goal |
| `impulse goal milestone <goal-id> add <description>` | Add a milestone |
| `impulse goal milestone <goal-id> check <ms-id>` | Check off a milestone |
| `impulse goal today` | Show daily surface |
| `impulse goal tree <id>` | Print goal decomposition tree |

### 5.6 IPC Protocol Extension

New daemon request/response variants (Protocol Version 3):

```rust
// New variants in DaemonRequest
AddGoal { goal: Goal },
UpdateGoal { id: String, updates: GoalUpdate },
ListGoals { horizon: Option<Horizon>, status: Option<GoalStatus> },
GetGoal { id: String },
GetDailySurface { date: Option<NaiveDate> },
LinkSessionToGoal { goal_id: String, session_id: String },
GetGoalTree { id: String, max_depth: Option<usize> },

// New variants in DaemonResponse
GoalResult { success: bool, goal: Option<Goal>, error: Option<String> },
GoalListResult { goals: Vec<Goal> },
DailySurfaceResult { surface: DailySurface },
GoalTreeResult { tree: Vec<GoalTreeNode> },
```

### 5.7 Genome Integration

When a goal is completed, a `Decision` is automatically added to the Genome:

```rust
// Auto-generated decision on goal completion
Decision {
    date: Utc::now(),
    description: format!("Completed goal: {} ({})", goal.title, goal.horizon.label()),
    rationale: Some(format!(
        "{} milestones completed, {} sessions linked",
        goal.milestones.iter().filter(|m| m.completed).count(),
        goal.linked_sessions.len()
    )),
    tags: goal.tags.clone(),
}
```

This creates a permanent record of achievements in the project's memory, searchable via the retrieval system.

### 5.8 ROSA Bridge (Future)

The horizon goals framework is designed to extend beyond coding projects. When ROSA (the personal agent ecosystem) materializes, the goal framework can serve as its planning backbone:

- **Personal goals** use `project: None` and live in `~/.impulse/supervisor/GOALS.json`
- **Cross-domain horizons** (fitness, finance, learning) use tags for domain scoping
- **Agent routing** uses goal context to decide which agent/tool handles a task
- The `DailySurface` becomes the input to ROSA's daily briefing

The data model intentionally avoids coding-specific assumptions. `GoalStatus`, `Milestone`, `Horizon` are domain-agnostic. The only coding-specific integration is the MIER inference pipeline, which is behind a feature flag.

---

## 6. Implementation Roadmap

### Phase 0: Foundation (Week 1-2)

**Goal:** Core data model compiles, persists, and round-trips.

| Task | File | Output |
|------|------|--------|
| Create `src/goals/` module | `mod.rs`, `types.rs`, `store.rs`, `deps.rs` | All types from §2 with Serialize/Deserialize |
| Implement `GoalStore` persistence | `store.rs` | Load/save with atomic writes, CRUD operations |
| Dependency validation | `deps.rs` | Cycle detection, topological sort |
| Progress calculation | `store.rs` | FromChildren, FromMilestones, Manual modes |
| Tests: round-trip, error paths, cycle detection | `tests/` + `mod tests` | Target: 30+ tests at 3.0/KLOC |

**Verification gate:**
```bash
cargo test -p impulse-rs -- goals && cargo clippy -- -D warnings
```

**Exit criteria:** `GoalStore` can be created, populated with a multi-horizon goal tree, persisted to disk, reloaded, and all progress calculations are correct.

### Phase 1: CLI + Direct Mode (Week 3)

**Goal:** Goals are usable from the command line.

| Task | File | Output |
|------|------|--------|
| Add `goal` subcommand to CLI parser | `src/cli.rs` | All commands from §5.5 |
| Implement goal handlers | `src/handlers/goal_handlers.rs` | Direct-mode dispatch |
| Wire into `direct_dispatch.rs` | `src/handlers/direct_dispatch.rs` | Routing for `impulse goal *` |
| `DailySurface` computation | `src/goals/store.rs` | `daily_surface()` with overdue/blocked/unblocked logic |
| Integration tests | `tests/goals_integration.rs` | CLI end-to-end tests |

**Exit criteria:** `impulse goal add "Ship OAuth" --horizon week --target 2026-07-01` works. `impulse goal today` shows a daily briefing.

### Phase 2: TUI — Aperture Port + Goals Tab (Week 4-5)

**Goal:** Goals are visible and interactive in the TUI.

| Task | File | Output |
|------|------|--------|
| Port Aperture design tokens to TUI | `src/ui/aperture.rs` | Constants from §4.1 |
| Implement `ApertureProgressBar` widget | `src/ui/widgets/progress_bar.rs` | StatefulWidget impl |
| Implement `GoalTree` widget | `src/ui/widgets/goal_tree.rs` | Tree rendering with box-drawing |
| Goals tab layout (3-pane) | `src/ui/render_goals.rs` | Horizon selector + goal list + detail |
| Dashboard "Active Arcs" panel | `src/ui/render_dashboard.rs` | New panel in dashboard layout |
| Navigation and keybindings | `src/ui/runner.rs` | All keybindings from §3.5 |
| Add/Edit goal modal | `src/ui/render_goals.rs` | Modal overlay with form input |
| Session-goal linking modal | `src/ui/render_goals.rs` | Quick-link from Sessions tab |

**Exit criteria:** Full Goals tab with all three panes rendering. Can add, edit, complete goals. Dashboard shows active arcs. Sessions can be linked to goals.

### Phase 3: Intelligence — MIER Integration (Week 6-7)

**Goal:** Goals are enriched by agent activity.

| Task | File | Output |
|------|------|--------|
| Goal inference engine | `src/goals/inference.rs` | Insight → goal progress mapping |
| MIER pipeline hook | `src/context_lifecycle/extractor.rs` | Call `process_insights_for_goals()` on extract cycle |
| Recommendation type for goal progress | `src/agent/coordinator.rs` | New `GoalProgress` recommendation variant |
| Context injection extension | `src/context_lifecycle/templates.rs` | `<active-goals>` block in injected context |
| Session lifecycle hooks | `src/state/persistence.rs` | Auto-link on session start/end |
| Config keys for inference | `src/state/config.rs` | `goals_auto_inference`, `goals_injection_enabled` |

**Exit criteria:** When a coding agent touches files linked to a goal, the MIER pipeline surfaces a recommendation like "Session may have progressed goal 'Ship OAuth' — mark milestone 'Implement route' complete?" Context injection includes active goals.

### Phase 4: IPC + Desktop Bridge (Week 8-9)

**Goal:** Goals are accessible via the daemon protocol and ready for the desktop shell.

| Task | File | Output |
|------|------|--------|
| IPC protocol extension (v3) | `src/daemon/protocol.rs` | New request/response variants from §5.6 |
| Daemon handlers | `src/daemon/handler.rs` | Goal CRUD over Unix socket |
| `impulse-ops` shared types | `impulse-ops/src/lib.rs` | `GoalSummary`, `DailySurface` exports |
| Delegation `goal_id` extension | `src/delegation/types.rs` | Link delegations to goals |
| Genome auto-decision on completion | `src/goals/store.rs` | Decision write on `GoalStatus::Completed` |

**Exit criteria:** A Dioxus desktop shell (or any IPC client) can CRUD goals via the daemon socket. Completing a goal creates a Genome decision.

### Phase 5: Cross-Project + Supervisor (Week 10+)

**Goal:** Goals span projects and inform the supervisor.

| Task | File | Output |
|------|------|--------|
| Supervisor-level `GoalStore` | `src/goals/store.rs` | Load from `~/.impulse/supervisor/GOALS.json` |
| Merged `DailySurface` | `src/goals/store.rs` | Cross-project daily briefing |
| Cross-project goal views in TUI | `src/ui/render_goals.rs` | Project switcher in goals tab |
| Goal-aware context injection per project | `src/context_lifecycle/injector.rs` | Filter goals by project on injection |

**Exit criteria:** Goals can exist at both project and personal level. `impulse goal today` shows a unified daily briefing across all projects.

---

## Appendix A: Design Decisions

### Why not use the existing Delegation system for goals?

Delegations are agent-scoped, transient, and operational. A delegation is "ask Codex to write these tests" — it has a coordinator pane, a worker pane, and a short lifecycle. Goals are user-scoped, persistent, and strategic. A goal is "ship the authentication system this quarter." They operate at different time scales and abstraction levels. Goals may *contain* delegations, but they are not delegations.

### Why a separate GOALS.json instead of extending GENOME.md?

The Genome is append-mostly (decisions accumulate) and represents *past* decisions. Goals represent *future* intent and are frequently mutated (progress updates, status changes, re-parenting). Different access patterns warrant different storage. Goals also need indexed lookup by ID, horizon, and status — operations poorly suited to the Genome's linear structure.

### Why not embed task tracking (à la Todoist)?

Impulse is a sidecar, not a project management tool. The goal framework provides *horizons and arcs* — the strategic context that makes daily tasks meaningful. For granular task tracking, users have Linear, GitHub Issues, or whatever they already use. Goals connect to those systems via `ExternalLink`, not by replacing them.

### Why port Aperture to the TUI instead of waiting for the desktop shell?

The desktop shell (Dioxus) is in early migration (Phase 2 of 4). The TUI is the production surface today and will remain so for months. Users shouldn't wait for the desktop shell to get a coherent visual language. Porting the Phosphor palette to ratatui constants is low-effort and immediately improves the TUI's visual quality, with zero risk to the desktop migration.

---

## Appendix B: Test Requirements

Per CLAUDE.md testing standards, the goals module must ship with:

| Requirement | Target |
|-------------|--------|
| Test density | 3.0 tests/KLOC (core module) |
| Serde round-trip | Every type with `Serialize + Deserialize` |
| Error path coverage | Every `Result`-returning function |
| Display tests | Every `GoalError` variant |
| Cycle detection | Property-based tests with `proptest` |
| Progress calculation | Boundary cases (0 children, 1 child, deep nesting) |
| Dependency validation | Cycle, self-reference, missing ID |
| Store persistence | Load empty, load corrupt, atomic write verification |

Estimated test count for Phase 0: 30-40 tests across `types.rs`, `store.rs`, and `deps.rs`.

---

## Appendix C: File Layout

```
impulse-rs/
├── src/
│   ├── goals/
│   │   ├── mod.rs          — Module exports
│   │   ├── types.rs        — Goal, Horizon, GoalStatus, Milestone, etc.
│   │   ├── store.rs        — GoalStore, DailySurface, persistence
│   │   ├── deps.rs         — Dependency graph validation
│   │   └── inference.rs    — MIER → goal progress inference
│   ├── ui/
│   │   ├── aperture.rs     — Aperture design tokens for ratatui
│   │   ├── widgets/
│   │   │   ├── mod.rs
│   │   │   ├── progress_bar.rs  — ApertureProgressBar
│   │   │   └── goal_tree.rs     — GoalTree renderer
│   │   ├── render_goals.rs      — Goals tab + modals
│   │   └── goal_icons.rs        — Unicode status/tree icons
│   ├── handlers/
│   │   └── goal_handlers.rs     — CLI command handlers
│   └── daemon/
│       └── protocol.rs          — IPC v3 extensions
├── .impulse/
│   └── GOALS.json               — Per-project goal store
└── ~/.impulse/supervisor/
    └── GOALS.json               — Cross-project / personal goals
```
