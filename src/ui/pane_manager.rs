use std::path::Path;

use portable_pty::PtySize;

use super::terminal_pane::{TerminalPane, DEFAULT_SCROLLBACK_LINES};

pub struct PaneManager {
    pub panes: Vec<TerminalPane>,
    pub active_pane_index: usize,
    next_id: usize,
    /// Default scrollback lines for new panes
    default_scrollback: usize,
}

impl Default for PaneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneManager {
    pub fn new() -> Self {
        Self {
            panes: Vec::new(),
            active_pane_index: 0,
            next_id: 1,
            default_scrollback: DEFAULT_SCROLLBACK_LINES,
        }
    }

    /// Set the default scrollback lines for new panes
    pub fn set_default_scrollback(&mut self, lines: usize) {
        self.default_scrollback = lines;
    }

    // TODO(refactor): extract params into struct
    #[allow(clippy::too_many_arguments)]
    pub fn create_pane(
        &mut self,
        name: String,
        command: &str,
        args: &[&str],
        working_dir: Option<&Path>,
        size: PtySize,
        project_index: usize,
        impulse_home: Option<&Path>,
        scrollback_lines: Option<usize>,
        session_id: Option<&str>,
        platform: Option<&str>,
    ) -> anyhow::Result<usize> {
        let id = self.next_id;
        self.next_id += 1;

        let pane = TerminalPane::spawn(
            id,
            name.clone(),
            command,
            args,
            working_dir,
            size,
            project_index,
            impulse_home,
            scrollback_lines.or(Some(self.default_scrollback)),
        )?;

        // Send startup message to inform the agent about Impulse
        if let (Some(sid), Some(plat)) = (session_id, platform) {
            pane.send_startup_message(sid, plat).ok();
        }

        self.panes.push(pane);
        // Switch to the newly created pane
        self.active_pane_index = self.panes.len() - 1;
        Ok(id)
    }

    pub fn close_pane(&mut self, index: usize) -> anyhow::Result<()> {
        if index >= self.panes.len() {
            return Err(anyhow::anyhow!("pane index {} out of range", index));
        }

        let pane = &self.panes[index];
        if pane.is_alive() {
            pane.kill().ok(); // Best-effort kill
        }
        self.panes.remove(index);

        // Adjust active index
        if self.panes.is_empty() {
            self.active_pane_index = 0;
        } else if self.active_pane_index >= self.panes.len() {
            self.active_pane_index = self.panes.len() - 1;
        }

        Ok(())
    }

    pub fn active_pane(&self) -> Option<&TerminalPane> {
        self.panes.get(self.active_pane_index)
    }

    pub fn next_pane(&mut self) {
        if !self.panes.is_empty() {
            self.active_pane_index = (self.active_pane_index + 1) % self.panes.len();
        }
    }

    pub fn prev_pane(&mut self) {
        if !self.panes.is_empty() {
            self.active_pane_index = if self.active_pane_index == 0 {
                self.panes.len() - 1
            } else {
                self.active_pane_index - 1
            };
        }
    }

    pub fn select_pane(&mut self, index: usize) {
        if index < self.panes.len() {
            self.active_pane_index = index;
        }
    }

    pub fn cleanup_dead(&mut self) {
        let active_id = self.panes.get(self.active_pane_index).map(|p| p.id);

        self.panes.retain(|p| p.is_alive());

        // Restore active index by ID if possible
        if let Some(id) = active_id {
            if let Some(pos) = self.panes.iter().position(|p| p.id == id) {
                self.active_pane_index = pos;
            } else if self.panes.is_empty() {
                self.active_pane_index = 0;
            } else {
                self.active_pane_index = self.active_pane_index.min(self.panes.len() - 1);
            }
        } else {
            self.active_pane_index = 0;
        }
    }

    pub fn resize_all(&mut self, size: PtySize) -> anyhow::Result<()> {
        for pane in &self.panes {
            pane.resize(size).ok(); // Best-effort — don't fail all for one
        }
        Ok(())
    }

    pub fn total_output_bytes(&self) -> u64 {
        self.panes
            .iter()
            .map(TerminalPane::output_bytes)
            .fold(0u64, u64::saturating_add)
    }

    /// Get the number of active (alive) panes
    pub fn active_count(&self) -> usize {
        self.panes.iter().filter(|p| p.is_alive()).count()
    }

    /// Get the number of dead panes
    pub fn dead_count(&self) -> usize {
        self.panes.iter().filter(|p| !p.is_alive()).count()
    }

    /// Get pane by index, returning None if out of bounds
    pub fn get_pane(&self, index: usize) -> Option<&TerminalPane> {
        self.panes.get(index)
    }

    /// Get mutable pane by index
    pub fn get_pane_mut(&mut self, index: usize) -> Option<&mut TerminalPane> {
        self.panes.get_mut(index)
    }

    /// Inject text into the active pane (sends to PTY)
    pub fn inject_to_active(&mut self, text: &str) -> anyhow::Result<()> {
        if let Some(pane) = self.panes.get_mut(self.active_pane_index) {
            pane.write_input(text.as_bytes())?;
        }
        Ok(())
    }

    /// Find pane by ID
    pub fn find_by_id(&self, id: usize) -> Option<&TerminalPane> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// Get indices of all dead panes
    pub fn dead_indices(&self) -> Vec<usize> {
        self.panes
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_alive())
            .map(|(i, _)| i)
            .collect()
    }

    /// Check if there are any panes
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// Get the total number of panes
    #[must_use]
    pub fn len(&self) -> usize {
        self.panes.len()
    }
}

impl Drop for PaneManager {
    fn drop(&mut self) {
        for pane in &self.panes {
            pane.kill().ok();
        }
    }
}
