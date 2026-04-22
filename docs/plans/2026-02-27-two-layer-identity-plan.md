# Two-Layer Identity Architecture — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give the Impulse agent its own identity, scope terminal panes to user-selected project directories, and wire init context injection at spawn.

**Architecture:** Three identity layers — application (`~/.impulse/`), project (`<target>/.impulse/`), developer (existing `CLAUDE.md`). Project selector dialog before pane spawn. Auto-scaffold on first target. Init injection after startup delay.

**Tech Stack:** Rust, egui 0.31, rfd (native file dialogs), impulse-term, serde_json

---

### Task 1: Add `rfd` dependency to impulse-gui

**Files:**
- Modify: `impulse-gui/Cargo.toml`

**Step 1: Add the dependency**

In `impulse-gui/Cargo.toml`, add `rfd` to `[dependencies]`:

```toml
rfd = "0.15"
```

This gives us native folder picker dialogs on macOS/Linux/Windows.

**Step 2: Verify it compiles**

Run: `cd impulse-gui && cargo check`
Expected: Clean compile with rfd resolved.

**Step 3: Commit**

```bash
git add impulse-gui/Cargo.toml
git commit -m "deps(gui): add rfd for native file dialogs"
```

---

### Task 2: Create global config module (`~/.impulse/`)

**Files:**
- Create: `impulse-gui/src/global_config.rs`
- Modify: `impulse-gui/src/main.rs` (add `mod global_config;`)

**Step 1: Write the test**

At the bottom of `impulse-gui/src/global_config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = GlobalConfig::default();
        assert!(config.recent_projects.is_empty());
    }

    #[test]
    fn test_add_recent_project() {
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/project-a".into());
        config.add_recent_project("/tmp/project-b".into());
        assert_eq!(config.recent_projects.len(), 2);
        // Most recent first
        assert_eq!(config.recent_projects[0], std::path::PathBuf::from("/tmp/project-b"));
    }

    #[test]
    fn test_add_recent_project_deduplicates() {
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/project-a".into());
        config.add_recent_project("/tmp/project-b".into());
        config.add_recent_project("/tmp/project-a".into());
        assert_eq!(config.recent_projects.len(), 2);
        // Re-added project moves to front
        assert_eq!(config.recent_projects[0], std::path::PathBuf::from("/tmp/project-a"));
    }

    #[test]
    fn test_max_recent_projects() {
        let mut config = GlobalConfig::default();
        for i in 0..15 {
            config.add_recent_project(format!("/tmp/project-{}", i).into());
        }
        assert_eq!(config.recent_projects.len(), 10);
    }

    #[test]
    fn test_load_save_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut config = GlobalConfig::default();
        config.add_recent_project("/tmp/test-proj".into());
        config.save(dir.path()).unwrap();

        let loaded = GlobalConfig::load(dir.path()).unwrap();
        assert_eq!(loaded.recent_projects.len(), 1);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let config = GlobalConfig::load(dir.path()).unwrap();
        assert!(config.recent_projects.is_empty());
    }
}
```

**Step 2: Run tests — verify they fail**

Run: `cd impulse-gui && cargo test global_config`
Expected: Compilation error (module doesn't exist yet).

**Step 3: Write the implementation**

```rust
//! Global Impulse configuration — stored at ~/.impulse/config.json.
//!
//! Tracks recent projects and application-level preferences.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT_PROJECTS: usize = 10;

/// Application-level configuration stored at `~/.impulse/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            recent_projects: Vec::new(),
        }
    }
}

impl GlobalConfig {
    /// Add a project to the recent list (MRU order, deduplicating).
    pub fn add_recent_project(&mut self, path: PathBuf) {
        self.recent_projects.retain(|p| p != &path);
        self.recent_projects.insert(0, path);
        self.recent_projects.truncate(MAX_RECENT_PROJECTS);
    }

    /// Load from `<dir>/config.json`. Returns default if file doesn't exist.
    pub fn load(impulse_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let path = impulse_dir.join("config.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save to `<dir>/config.json` using atomic write (temp + rename).
    pub fn save(&self, impulse_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(impulse_dir)?;
        let path = impulse_dir.join("config.json");
        let content = serde_json::to_string_pretty(self)?;

        // Atomic write: temp file + rename.
        let tmp_path = impulse_dir.join(format!(
            ".config.json.tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    /// The global impulse directory path.
    pub fn impulse_home() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".impulse")
    }
}
```

**Step 4: Add `dirs` and `tempfile` dependencies**

In `impulse-gui/Cargo.toml`:
```toml
dirs = "6"
tempfile = { version = "3", optional = false }

[dev-dependencies]
tempfile = "3"
```

Note: `dirs` is needed for `dirs::home_dir()`. `tempfile` only needed in tests.

**Step 5: Add module declaration to main.rs**

In `impulse-gui/src/main.rs`, add:
```rust
mod global_config;
```

**Step 6: Run tests — verify they pass**

Run: `cd impulse-gui && cargo test global_config -- --nocapture`
Expected: All 6 tests pass.

**Step 7: Commit**

```bash
git add impulse-gui/Cargo.toml impulse-gui/src/global_config.rs impulse-gui/src/main.rs
git commit -m "feat(gui): add global config module for ~/.impulse/"
```

---

### Task 3: Create project scaffold module

**Files:**
- Create: `impulse-gui/src/project_scaffold.rs`
- Modify: `impulse-gui/src/main.rs` (add `mod project_scaffold;`)

**Step 1: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scaffold_creates_impulse_dir() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        assert!(dir.path().join(".impulse").exists());
        assert!(dir.path().join(".impulse/GENOME.md").exists());
        assert!(dir.path().join(".impulse/config.json").exists());
    }

    #[test]
    fn test_scaffold_idempotent() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        scaffold_impulse_dir(dir.path()).unwrap(); // Second call should not error
        assert!(dir.path().join(".impulse").exists());
    }

    #[test]
    fn test_needs_scaffold_true() {
        let dir = TempDir::new().unwrap();
        assert!(needs_scaffold(dir.path()));
    }

    #[test]
    fn test_needs_scaffold_false_after_scaffold() {
        let dir = TempDir::new().unwrap();
        scaffold_impulse_dir(dir.path()).unwrap();
        assert!(!needs_scaffold(dir.path()));
    }
}
```

**Step 2: Run tests — verify they fail**

Run: `cd impulse-gui && cargo test project_scaffold`
Expected: Compilation error.

**Step 3: Write the implementation**

```rust
//! Auto-scaffold `.impulse/` directory for new project targets.

use std::path::Path;

const GENOME_TEMPLATE: &str = r#"{
  "decisions": [],
  "preferences": [],
  "constraints": [],
  "last_updated": null
}"#;

const CONFIG_TEMPLATE: &str = r#"{}"#;

/// Check if a target directory needs scaffolding.
pub fn needs_scaffold(target: &Path) -> bool {
    !target.join(".impulse").exists()
}

/// Create `.impulse/` with starter files in a target project directory.
pub fn scaffold_impulse_dir(target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let impulse_dir = target.join(".impulse");
    std::fs::create_dir_all(&impulse_dir)?;

    let genome_path = impulse_dir.join("GENOME.md");
    if !genome_path.exists() {
        atomic_write(&genome_path, GENOME_TEMPLATE)?;
    }

    let config_path = impulse_dir.join("config.json");
    if !config_path.exists() {
        atomic_write(&config_path, CONFIG_TEMPLATE)?;
    }

    let history_path = impulse_dir.join("HISTORY.jsonl");
    if !history_path.exists() {
        atomic_write(&history_path, "")?;
    }

    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().ok_or("no parent directory")?;
    let tmp_path = parent.join(format!(
        ".tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
```

**Step 4: Add module declaration**

In `impulse-gui/src/main.rs`, add:
```rust
mod project_scaffold;
```

**Step 5: Run tests — verify they pass**

Run: `cd impulse-gui && cargo test project_scaffold`
Expected: All 4 tests pass.

**Step 6: Commit**

```bash
git add impulse-gui/src/project_scaffold.rs impulse-gui/src/main.rs
git commit -m "feat(gui): add project scaffold for auto-creating .impulse/"
```

---

### Task 4: Build project selector dialog

**Files:**
- Create: `impulse-gui/src/widgets/project_selector.rs`
- Modify: `impulse-gui/src/widgets/mod.rs` (add `pub mod project_selector;`)

**Step 1: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_initial_state() {
        let selector = ProjectSelector::new(vec![]);
        assert!(!selector.is_open());
        assert!(selector.selected_path().is_none());
    }

    #[test]
    fn test_selector_with_recent_projects() {
        let recents = vec![
            std::path::PathBuf::from("/tmp/proj-a"),
            std::path::PathBuf::from("/tmp/proj-b"),
        ];
        let selector = ProjectSelector::new(recents.clone());
        assert_eq!(selector.recent_projects().len(), 2);
    }

    #[test]
    fn test_selector_open_close() {
        let mut selector = ProjectSelector::new(vec![]);
        selector.open(None);
        assert!(selector.is_open());
        selector.close();
        assert!(!selector.is_open());
    }

    #[test]
    fn test_selector_select_recent() {
        let recents = vec![std::path::PathBuf::from("/tmp/proj-a")];
        let mut selector = ProjectSelector::new(recents);
        selector.open(None);
        selector.select(std::path::PathBuf::from("/tmp/proj-a"));
        assert_eq!(
            selector.selected_path(),
            Some(&std::path::PathBuf::from("/tmp/proj-a"))
        );
    }

    #[test]
    fn test_selector_default_to_home() {
        let mut selector = ProjectSelector::new(vec![]);
        selector.open(None);
        selector.select_default();
        assert!(selector.selected_path().is_some());
    }
}
```

**Step 2: Write the implementation**

```rust
//! Project selector dialog — pick a target directory before spawning a pane.

use std::path::PathBuf;

use eframe::egui;

use crate::theme::colors;

/// Project selector state — tracks whether dialog is open and what's selected.
pub struct ProjectSelector {
    open: bool,
    recent: Vec<PathBuf>,
    selected: Option<PathBuf>,
    /// Agent info passed through for spawn after selection.
    pending_agent: Option<String>,
}

impl ProjectSelector {
    pub fn new(recent_projects: Vec<PathBuf>) -> Self {
        Self {
            open: false,
            recent: recent_projects,
            selected: None,
            pending_agent: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, agent_name: Option<String>) {
        self.open = true;
        self.selected = None;
        self.pending_agent = agent_name;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.pending_agent = None;
    }

    pub fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
    }

    pub fn select_default(&mut self) {
        self.selected = Some(
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        );
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.selected.as_ref()
    }

    pub fn pending_agent(&self) -> Option<&str> {
        self.pending_agent.as_deref()
    }

    pub fn recent_projects(&self) -> &[PathBuf] {
        &self.recent
    }

    pub fn update_recents(&mut self, recents: Vec<PathBuf>) {
        self.recent = recents;
    }

    /// Render the selector dialog. Returns Some(selected_path) when user confirms.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        if !self.open {
            return None;
        }

        let mut result = None;

        egui::Window::new("Select Project Directory")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Choose a project folder for this terminal")
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(8.0);

                // Recent projects list.
                if !self.recent.is_empty() {
                    ui.label(
                        egui::RichText::new("Recent Projects")
                            .strong()
                            .color(colors::TEXT),
                    );
                    ui.add_space(4.0);

                    let mut clicked_path = None;
                    for path in &self.recent {
                        let display = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.display().to_string());

                        let is_selected = self.selected.as_ref() == Some(path);
                        let resp = ui.selectable_label(
                            is_selected,
                            egui::RichText::new(&display).color(colors::ACCENT),
                        );
                        if resp.clicked() {
                            clicked_path = Some(path.clone());
                        }
                        resp.on_hover_text(path.display().to_string());
                    }
                    if let Some(p) = clicked_path {
                        self.selected = Some(p);
                    }
                    ui.add_space(8.0);
                }

                // Browse and default buttons.
                ui.horizontal(|ui| {
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(dirs::home_dir().unwrap_or_default())
                            .pick_folder()
                        {
                            self.selected = Some(path);
                        }
                    }
                    if ui.button("Use ~/").clicked() {
                        self.selected =
                            Some(dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")));
                    }
                });

                // Show selected path.
                if let Some(ref path) = self.selected {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("Selected: {}", path.display()))
                            .color(colors::GREEN),
                    );
                }

                ui.add_space(12.0);

                // Confirm / Cancel buttons.
                ui.horizontal(|ui| {
                    let can_confirm = self.selected.is_some();
                    if ui
                        .add_enabled(can_confirm, egui::Button::new("Open Terminal"))
                        .clicked()
                    {
                        result = self.selected.clone();
                        self.open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.close();
                    }
                });
            });

        result
    }
}
```

**Step 3: Add module declaration**

In `impulse-gui/src/widgets/mod.rs`, add:
```rust
pub mod project_selector;
```

**Step 4: Run tests — verify they pass**

Run: `cd impulse-gui && cargo test project_selector`
Expected: All 5 tests pass.

**Step 5: Commit**

```bash
git add impulse-gui/src/widgets/project_selector.rs impulse-gui/src/widgets/mod.rs
git commit -m "feat(gui): add project selector dialog widget"
```

---

### Task 5: Wire project selector into spawn flow

**Files:**
- Modify: `impulse-gui/src/views/terminals.rs` (Tab struct, spawn_tab, UI)
- Modify: `impulse-gui/src/app.rs` (hold selector state, integrate into update loop)

**Step 1: Update Tab struct**

In `impulse-gui/src/views/terminals.rs`, add `target_dir` field to `Tab`:

```rust
use std::path::PathBuf;

struct Tab {
    #[allow(dead_code)]
    id: u64,
    label: String,
    agent_name: &'static str,
    panel: TerminalPanel,
    target_dir: PathBuf,
}
```

**Step 2: Update spawn_tab to accept a target directory**

Change `spawn_tab` signature and body:

```rust
pub fn spawn_tab(&mut self, agent: &AgentInfo, target_dir: &Path, _ctx: &egui::Context) {
    if self.tabs.len() >= self.max_tabs {
        log::warn!("Max tabs reached ({})", self.max_tabs);
        return;
    }

    let id = self.next_id;
    self.next_id += 1;

    let args: Vec<String> = agent.args.iter().map(|s| s.to_string()).collect();

    match TerminalPanel::spawn(
        agent.command,
        &args,
        Some(target_dir),
        agent.name,
        id as usize,
    ) {
        Ok(panel) => {
            // Tab label: "AgentName: project-folder"
            let project_name = target_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "~".to_string());
            let label = format!("{}: {}", agent.name, project_name);

            let tab = Tab {
                id,
                label,
                agent_name: agent.name,
                panel,
                target_dir: target_dir.to_path_buf(),
            };
            self.tabs.insert(id, tab);
            self.active_tab = Some(id);
            log::info!("Spawned tab {} for {} in {}", id, agent.name, target_dir.display());
        }
        Err(e) => {
            log::error!("Failed to spawn {}: {}", agent.name, e);
        }
    }
}
```

**Step 3: Update all spawn_tab call sites to go through the project selector**

In the tab bar spawn buttons and welcome screen quick-launch buttons, replace direct `self.spawn_tab(agent, _ctx)` with opening the project selector:

In the tab bar inline buttons (around line 442):
```rust
if resp.clicked() {
    // Open project selector instead of spawning directly
    self.pending_spawn_agent = Some(agent.name.to_string());
}
```

Add a `pending_spawn_agent: Option<String>` field to `TerminalsView` for the project selector to pick up.

**Step 4: Integrate project selector in `app.rs`**

In `ImpulseApp`, add:
```rust
project_selector: ProjectSelector,
global_config: GlobalConfig,
```

In `update()`, after handling global shortcuts, add:
```rust
// Project selector dialog
if let Some(selected_dir) = self.project_selector.show(ctx) {
    // Auto-scaffold if needed
    if project_scaffold::needs_scaffold(&selected_dir) {
        if let Err(e) = project_scaffold::scaffold_impulse_dir(&selected_dir) {
            log::error!("Failed to scaffold .impulse/: {}", e);
        }
    }

    // Update recent projects
    self.global_config.add_recent_project(selected_dir.clone());
    let _ = self.global_config.save(&GlobalConfig::impulse_home());

    // Find the agent and spawn
    if let Some(agent_name) = self.project_selector.pending_agent() {
        if let Some(agent) = self.terminals.agents.iter().find(|a| a.name == agent_name) {
            let agent = agent.clone();
            self.terminals.spawn_tab(&agent, &selected_dir, ctx);
        }
    }
}

// Check if terminals requested a spawn (pending_spawn_agent)
if let Some(agent_name) = self.terminals.take_pending_spawn() {
    self.project_selector.open(Some(agent_name));
}
```

**Step 5: Add `take_pending_spawn` to TerminalsView**

```rust
pub fn take_pending_spawn(&mut self) -> Option<String> {
    self.pending_spawn_agent.take()
}
```

**Step 6: Verify it compiles**

Run: `cd impulse-gui && cargo check`
Expected: Clean compile.

**Step 7: Run all GUI tests**

Run: `cd impulse-gui && cargo test`
Expected: All tests pass.

**Step 8: Commit**

```bash
git add impulse-gui/src/views/terminals.rs impulse-gui/src/app.rs
git commit -m "feat(gui): wire project selector into terminal spawn flow"
```

---

### Task 6: Create application-level identity files

**Files:**
- Create: content for `~/.impulse/CLAUDE.md` (template in source)
- Create: content for `~/.impulse/AGENTS.md` (template in source)
- Create: `impulse-gui/src/identity.rs` (manages identity file creation + reading)

**Step 1: Write the test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_ensure_identity_creates_files() {
        let dir = TempDir::new().unwrap();
        ensure_identity_files(dir.path()).unwrap();
        assert!(dir.path().join("CLAUDE.md").exists());
        assert!(dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn test_ensure_identity_does_not_overwrite() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "custom content").unwrap();
        ensure_identity_files(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert_eq!(content, "custom content");
    }

    #[test]
    fn test_load_identity_reads_claude_md() {
        let dir = TempDir::new().unwrap();
        ensure_identity_files(dir.path()).unwrap();
        let identity = load_identity(dir.path()).unwrap();
        assert!(identity.contains("Impulse"));
        assert!(identity.contains("context manager"));
    }
}
```

**Step 2: Write the implementation**

```rust
//! Application-level identity files for the Impulse agent.
//!
//! Creates and reads ~/.impulse/CLAUDE.md and AGENTS.md.
//! These files tell the Impulse agent WHO it is.

use std::path::Path;

/// The Impulse agent's identity (CLAUDE.md content).
const IMPULSE_CLAUDE_MD: &str = r#"# Impulse — AI Agent Coordinator

You are **Impulse**, a context manager, memory keeper, and process manager for AI coding agents.

## Your Role

You coordinate AI coding agents running in terminal panes. You do NOT write code yourself. You manage the agents that do.

### What You Do

- **Track sessions** — Record which files changed, which tools were used, what decisions were made
- **Manage the GENOME** — Permanent project decisions and preferences that persist across sessions
- **Surface cross-pane conflicts** — Alert when two agents in different panes modify the same file
- **Inject context** — Provide relevant history and decisions when agents start or their context runs low
- **Evaluate guardrails** — Block dangerous actions (force-push, rm -rf, DROP TABLE) before they execute
- **Provide session continuity** — When a new session starts, surface what happened last time

### What You Don't Do

- You don't write or modify code directly
- You don't make implementation decisions for the user
- You don't override the coding agent's work

## Your Data

Each project you manage has a `.impulse/` directory containing:
- `GENOME.md` — Permanent decisions and preferences (committed to git)
- `HISTORY.jsonl` — Append-only session log (committed to git)
- `LIVE_STATE.json` — Active session state (ephemeral)
- `config.json` — Runtime configuration
- `retrieval.db` — Search index (rebuildable)

## Your Tools

- `impulse-rs session-start` — Begin tracking a new session
- `impulse-rs session-end` — End and summarize a session
- `impulse-rs track-write` — Record a file modification
- `impulse-rs track-tool` — Record a tool invocation
- `impulse-rs add-decision` — Record a permanent decision to the GENOME
- `impulse-rs guard --action "cmd"` — Evaluate an action against guardrail rules
- `impulse-rs search-history` — Search session history
- `impulse-rs search-genome` — Search project decisions
- `impulse-rs sync-context` — Refresh context in a terminal pane

## Behavioral Guidelines

- **Speak up** when you detect cross-pane conflicts, repeated errors, or stale context
- **Stay quiet** when agents are working normally — don't interrupt productive flow
- **Be concise** — agents have limited context windows. Every token you inject costs them capacity
- **Prioritize recency** — recent sessions and decisions matter more than old ones
"#;

const IMPULSE_AGENTS_MD: &str = r#"# Impulse — Agent Integration Guide

## For AI Coding Agents (Claude Code, OpenCode, Codex)

You are running inside Impulse, a terminal multiplexer and memory system.
Impulse tracks your work across sessions and provides context continuity.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `IMPULSE_PANE_ID` | Your pane identifier |
| `IMPULSE_PANE_NAME` | Your agent name |
| `IMPULSE_SESSION_ID` | Current session UUID |
| `IMPULSE_TERM_PROGRAM` | Always `impulse-gui` |
| `IMPULSE_VERSION` | Impulse version |

### Reporting Back to Impulse

Record decisions: `impulse-rs add-decision "description" --rationale "why"`
Refresh context: `impulse-rs sync-context`
Check guardrails: `impulse-rs guard --action "your command" --target bash`

### Context Injection

Impulse may inject context into your session at these thresholds:
- **Spawn** — Full context: identity, project info, recent decisions, last session summary
- **45% usage** — Essential: tools + active files + key decisions
- **60% usage** — Critical: tools + current task summary
- **80% usage** — Minimal: tool list + refresh command

These injections appear as system messages. They are from Impulse, not from the user.
"#;

/// Ensure identity files exist in the given directory.
/// Does NOT overwrite existing files (user may have customized them).
pub fn ensure_identity_files(impulse_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(impulse_dir)?;

    let claude_md = impulse_dir.join("CLAUDE.md");
    if !claude_md.exists() {
        std::fs::write(&claude_md, IMPULSE_CLAUDE_MD)?;
    }

    let agents_md = impulse_dir.join("AGENTS.md");
    if !agents_md.exists() {
        std::fs::write(&agents_md, IMPULSE_AGENTS_MD)?;
    }

    Ok(())
}

/// Load the Impulse agent identity from CLAUDE.md.
pub fn load_identity(impulse_dir: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let claude_md = impulse_dir.join("CLAUDE.md");
    if claude_md.exists() {
        Ok(std::fs::read_to_string(&claude_md)?)
    } else {
        Ok(IMPULSE_CLAUDE_MD.to_string())
    }
}
```

**Step 3: Add module declaration and wire into startup**

In `impulse-gui/src/main.rs`:
```rust
mod identity;
```

In `ImpulseApp::new()` (app.rs), add:
```rust
// Ensure application-level identity files exist
let impulse_home = global_config::GlobalConfig::impulse_home();
if let Err(e) = identity::ensure_identity_files(&impulse_home) {
    log::warn!("Failed to create identity files: {}", e);
}
```

**Step 4: Run tests — verify they pass**

Run: `cd impulse-gui && cargo test identity`
Expected: All 3 tests pass.

**Step 5: Commit**

```bash
git add impulse-gui/src/identity.rs impulse-gui/src/main.rs impulse-gui/src/app.rs
git commit -m "feat(gui): create application-level identity files for Impulse agent"
```

---

### Task 7: Wire init context injection at pane spawn

**Files:**
- Modify: `impulse-gui/src/views/terminals.rs` (add delayed init injection)
- Modify: `impulse-gui/src/app.rs` (track pending injections)

This is the key wiring task. After a pane spawns, we wait for the agent startup delay, then inject context.

**Step 1: Add pending injection tracking to TerminalsView**

```rust
use std::time::{Duration, Instant};
use std::path::PathBuf;

/// A pending context injection — waiting for agent startup.
struct PendingInjection {
    tab_id: u64,
    inject_at: Instant,
    target_dir: PathBuf,
}

// Add to TerminalsView:
pending_injections: Vec<PendingInjection>,
```

**Step 2: Schedule injection in spawn_tab**

After successfully spawning, add:

```rust
// Schedule init context injection after startup delay.
let delay = match agent.name {
    "Claude Code" => Duration::from_secs(3),
    "OpenCode" | "Codex" => Duration::from_secs(2),
    _ => Duration::from_millis(500),
};
self.pending_injections.push(PendingInjection {
    tab_id: id,
    inject_at: Instant::now() + delay,
    target_dir: target_dir.to_path_buf(),
});
```

**Step 3: Add injection tick method**

```rust
/// Process pending init injections (called from app.rs update loop).
pub fn process_pending_injections(&mut self, impulse_home: &Path) {
    let now = Instant::now();
    let ready: Vec<PendingInjection> = self
        .pending_injections
        .drain_filter_compat(|p| now >= p.inject_at);

    for pending in ready {
        if let Some(tab) = self.tabs.get_mut(&pending.tab_id) {
            let identity = crate::identity::load_identity(impulse_home)
                .unwrap_or_default();

            // Build init context with project-specific info.
            let project_name = pending.target_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let context = format!(
                "<impulse-context type=\"init\" version=\"2\">\n\
                 {}\n\n\
                 ## Your Project\n\
                 Working directory: {}\n\
                 Pane: {}\n\
                 </impulse-context>",
                identity.trim(),
                pending.target_dir.display(),
                tab.agent_name,
            );

            match tab.panel.context_bridge().inject_context(&context) {
                Ok(()) => log::info!(
                    "Injected init context into tab {} ({})",
                    pending.tab_id,
                    project_name
                ),
                Err(e) => log::warn!(
                    "Failed to inject init context into tab {}: {}",
                    pending.tab_id,
                    e
                ),
            }
        }
    }
}
```

Note: Since `drain_filter` is nightly-only, implement a compat version:

```rust
// Compat helper for drain_filter (not stable yet)
trait DrainFilterCompat<T> {
    fn drain_filter_compat<F: FnMut(&T) -> bool>(&mut self, pred: F) -> Vec<T>;
}

impl<T> DrainFilterCompat<T> for Vec<T> {
    fn drain_filter_compat<F: FnMut(&T) -> bool>(&mut self, mut pred: F) -> Vec<T> {
        let mut drained = Vec::new();
        let mut i = 0;
        while i < self.len() {
            if pred(&self[i]) {
                drained.push(self.remove(i));
            } else {
                i += 1;
            }
        }
        drained
    }
}
```

**Step 4: Call from app.rs update loop**

In `ImpulseApp::update()`, after the context tick block:

```rust
// Process pending init injections.
let impulse_home = global_config::GlobalConfig::impulse_home();
self.terminals.process_pending_injections(&impulse_home);
```

**Step 5: Verify it compiles**

Run: `cd impulse-gui && cargo check`
Expected: Clean compile.

**Step 6: Run all tests**

Run: `cd impulse-gui && cargo test`
Expected: All tests pass.

**Step 7: Commit**

```bash
git add impulse-gui/src/views/terminals.rs impulse-gui/src/app.rs
git commit -m "feat(gui): wire init context injection at pane spawn"
```

---

### Task 8: Add dedup guard to Genome.add_decision

**Files:**
- Modify: `impulse-rs/src/memory/mod.rs`

**Step 1: Write the failing test**

Add to `src/memory/mod.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_decision_dedup() {
        let mut genome = Genome::new();
        genome.add_decision("Test decision".into(), None, vec![]);
        genome.add_decision("Test decision".into(), None, vec![]);
        genome.add_decision("Test decision".into(), None, vec![]);
        // Should only keep one — dedup blocks consecutive identical descriptions
        assert_eq!(genome.decisions.len(), 1);
    }

    #[test]
    fn test_add_decision_different_descriptions_allowed() {
        let mut genome = Genome::new();
        genome.add_decision("Decision A".into(), None, vec![]);
        genome.add_decision("Decision B".into(), None, vec![]);
        assert_eq!(genome.decisions.len(), 2);
    }
}
```

**Step 2: Run test — verify it fails**

Run: `cargo test memory::tests::test_add_decision_dedup`
Expected: FAIL — assertion `left == right` (left: 3, right: 1).

**Step 3: Add dedup guard**

Modify `add_decision` in `src/memory/mod.rs`:

```rust
pub fn add_decision(
    &mut self,
    description: String,
    rationale: Option<String>,
    tags: Vec<String>,
) {
    // Dedup guard: skip if the last decision has the same description.
    if let Some(last) = self.decisions.last() {
        if last.description == description {
            return;
        }
    }

    self.decisions.push(Decision {
        date: Utc::now(),
        description,
        rationale,
        tags,
    });
    self.last_updated = Utc::now();
}
```

**Step 4: Run tests — verify they pass**

Run: `cargo test memory::tests`
Expected: Both tests pass.

**Step 5: Commit**

```bash
git add src/memory/mod.rs
git commit -m "fix(memory): add dedup guard to prevent duplicate genome decisions"
```

---

### Task 9: Purge test data from GENOME.md

**Files:**
- Modify: `.impulse/GENOME.md` (reset to empty)
- Modify: `src/integration_tests.rs` (fix test_add_decision to use isolated dir)

**Step 1: Reset GENOME.md to empty state**

Overwrite `.impulse/GENOME.md` with:

```json
{
  "decisions": [],
  "preferences": [],
  "constraints": [],
  "last_updated": "2026-02-27T00:00:00Z"
}
```

**Step 2: Fix integration test to use isolated directory**

In `src/integration_tests.rs`, change `test_add_decision` (line ~309) to use `run_impulse_with_impulse_dir`:

```rust
#[test]
fn test_add_decision() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Initialize the temp impulse dir with a valid genome
    let genome_path = tmp.path().join("GENOME.md");
    std::fs::write(&genome_path, r#"{"decisions":[],"preferences":[],"constraints":[],"last_updated":"2026-01-01T00:00:00Z"}"#).unwrap();

    let output = run_impulse_with_impulse_dir(tmp.path(), &[
        "add-decision",
        "-d",
        "Test decision for integration",
        "-r",
        "Testing integration flow",
    ]);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "add-decision failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("Decision added") || stdout.contains("decision"),
        "Expected confirmation, got: {}",
        stdout
    );

    // Verify it was written to the temp dir, not the real one
    let genome_content = std::fs::read_to_string(&genome_path).unwrap();
    assert!(genome_content.contains("Test decision for integration"));
}
```

**Step 3: Run integration tests**

Run: `cargo test integration_tests::tests::test_add_decision`
Expected: PASS — and no pollution of real `.impulse/GENOME.md`.

**Step 4: Verify genome is clean**

Run: `cargo run -- genome`
Expected: Empty genome output (no decisions).

**Step 5: Commit**

```bash
git add .impulse/GENOME.md src/integration_tests.rs
git commit -m "fix(genome): purge test data and isolate integration tests"
```

---

### Task 10: Final verification

**Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: Clean build, zero warnings.

**Step 2: Clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Zero warnings.

**Step 3: Format check**

Run: `cargo fmt --check --all`
Expected: No formatting issues.

**Step 4: Full test suite**

Run: `cargo test --workspace`
Expected: All tests pass (existing + new).

**Step 5: Verify GENOME is clean**

Run: `cargo run -- genome`
Expected: Empty genome (no test data).

**Step 6: Verify application identity files would be created**

Run: `ls ~/.impulse/CLAUDE.md ~/.impulse/AGENTS.md 2>/dev/null || echo "Will be created on first GUI launch"`
Expected: Either shows the files or the message.

**Step 7: Commit any final adjustments**

```bash
git add -A
git commit -m "chore: final verification pass for two-layer identity"
```

---

## Summary

| Task | What | Files |
|------|------|-------|
| 1 | Add `rfd` dependency | `Cargo.toml` |
| 2 | Global config module | `global_config.rs` |
| 3 | Project scaffold module | `project_scaffold.rs` |
| 4 | Project selector dialog | `widgets/project_selector.rs` |
| 5 | Wire selector into spawn flow | `terminals.rs`, `app.rs` |
| 6 | Application-level identity files | `identity.rs` |
| 7 | Init context injection at spawn | `terminals.rs`, `app.rs` |
| 8 | Genome dedup guard | `memory/mod.rs` |
| 9 | Purge test data + fix integration test | `GENOME.md`, `integration_tests.rs` |
| 10 | Final verification | All |
