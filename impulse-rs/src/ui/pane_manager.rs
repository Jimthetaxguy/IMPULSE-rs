use std::path::Path;

use portable_pty::PtySize;

use super::terminal_pane::{TerminalPane, TerminalSpawnRequest, DEFAULT_SCROLLBACK_LINES};

pub struct PaneManager {
    pub panes: Vec<TerminalPane>,
    pub active_pane_index: usize,
    next_id: usize,
    /// Default scrollback lines for new panes
    default_scrollback: usize,
}

pub struct PaneCreateRequest<'a> {
    pub name: String,
    pub command: &'a str,
    pub args: &'a [&'a str],
    pub working_dir: Option<&'a Path>,
    pub size: PtySize,
    pub project_index: usize,
    pub impulse_home: Option<&'a Path>,
    pub scrollback_lines: Option<usize>,
    pub session_id: Option<&'a str>,
    pub platform: Option<&'a str>,
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

    pub fn create_pane(&mut self, request: PaneCreateRequest<'_>) -> anyhow::Result<usize> {
        let PaneCreateRequest {
            name,
            command,
            args,
            working_dir,
            size,
            project_index,
            impulse_home,
            scrollback_lines,
            session_id,
            platform,
        } = request;

        let id = self.next_id;
        self.next_id += 1;

        let pane = TerminalPane::spawn(TerminalSpawnRequest {
            id,
            name: name.clone(),
            command,
            args,
            working_dir,
            size,
            project_index,
            impulse_home,
            scrollback_lines: scrollback_lines.or(Some(self.default_scrollback)),
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// A real PTY pane: `alive` keeps a child running, otherwise the child
    /// exits at once so the pane goes dead.
    fn spawn(manager: &mut PaneManager, name: &str, alive: bool) -> usize {
        let script = if alive { "sleep 60" } else { "true" };
        manager
            .create_pane(PaneCreateRequest {
                name: name.to_string(),
                command: "sh",
                args: &["-c", script],
                working_dir: None,
                size: size(),
                project_index: 0,
                impulse_home: None,
                scrollback_lines: None,
                session_id: None,
                platform: None,
            })
            .expect("spawn a PTY pane")
    }

    fn wait_until_dead(manager: &PaneManager, index: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while manager.get_pane(index).is_some_and(TerminalPane::is_alive) {
            assert!(
                std::time::Instant::now() < deadline,
                "pane {index} never exited"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn test_pane_manager_new_is_empty_with_no_active_pane() {
        let manager = PaneManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
        assert!(manager.active_pane().is_none());
    }

    #[test]
    fn test_create_pane_assigns_increasing_ids_and_activates_the_new_pane() {
        let mut manager = PaneManager::new();
        let first = spawn(&mut manager, "a", true);
        let second = spawn(&mut manager, "b", true);
        assert!(second > first);
        assert_eq!(manager.len(), 2);
        assert_eq!(manager.active_pane_index, 1);
        assert_eq!(manager.active_pane().map(|p| p.id), Some(second));
        assert_eq!(manager.find_by_id(first).map(|p| p.id), Some(first));
        assert!(manager.find_by_id(second + 100).is_none());
    }

    #[test]
    fn test_next_and_prev_pane_wrap_around() {
        let mut manager = PaneManager::new();
        for name in ["a", "b", "c"] {
            spawn(&mut manager, name, true);
        }
        manager.select_pane(2);
        manager.next_pane();
        assert_eq!(manager.active_pane_index, 0, "next wraps to the first pane");
        manager.prev_pane();
        assert_eq!(manager.active_pane_index, 2, "prev wraps to the last pane");
        manager.prev_pane();
        assert_eq!(manager.active_pane_index, 1);
    }

    #[test]
    fn test_navigation_on_an_empty_manager_is_a_no_op() {
        let mut manager = PaneManager::new();
        manager.next_pane();
        manager.prev_pane();
        manager.select_pane(3);
        assert_eq!(manager.active_pane_index, 0);
        assert!(manager.inject_to_active("ignored").is_ok());
    }

    #[test]
    fn test_select_pane_ignores_an_out_of_range_index() {
        let mut manager = PaneManager::new();
        spawn(&mut manager, "a", true);
        spawn(&mut manager, "b", true);
        manager.select_pane(0);
        manager.select_pane(5);
        assert_eq!(manager.active_pane_index, 0);
    }

    #[test]
    fn test_close_pane_out_of_range_returns_error() {
        let mut manager = PaneManager::new();
        spawn(&mut manager, "a", true);
        let err = manager.close_pane(1).unwrap_err();
        assert!(err.to_string().contains("out of range"), "{err}");
        assert_eq!(manager.len(), 1);
    }

    #[test]
    fn test_close_pane_clamps_the_active_index() {
        let mut manager = PaneManager::new();
        for name in ["a", "b", "c"] {
            spawn(&mut manager, name, true);
        }
        assert_eq!(manager.active_pane_index, 2);
        manager.close_pane(2).unwrap();
        assert_eq!(manager.len(), 2);
        assert_eq!(
            manager.active_pane_index, 1,
            "active moves to the new last pane"
        );
        manager.close_pane(0).unwrap();
        manager.close_pane(0).unwrap();
        assert!(manager.is_empty());
        assert_eq!(manager.active_pane_index, 0);
    }

    #[test]
    fn test_cleanup_dead_keeps_the_active_pane_by_id() {
        let mut manager = PaneManager::new();
        spawn(&mut manager, "dead", false);
        let keep = spawn(&mut manager, "alive", true);
        wait_until_dead(&manager, 0);
        assert_eq!(manager.dead_indices(), vec![0]);
        assert_eq!((manager.active_count(), manager.dead_count()), (1, 1));

        // The live pane is active at index 1; removing the dead pane before it
        // must keep it active at its new index.
        manager.cleanup_dead();
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.active_pane_index, 0);
        assert_eq!(manager.active_pane().map(|p| p.id), Some(keep));
        assert!(manager.dead_indices().is_empty());
    }

    #[test]
    fn test_cleanup_dead_clamps_when_the_active_pane_died() {
        let mut manager = PaneManager::new();
        let survivor = spawn(&mut manager, "alive", true);
        spawn(&mut manager, "dead", false);
        assert_eq!(manager.active_pane_index, 1);
        wait_until_dead(&manager, 1);
        manager.cleanup_dead();
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.active_pane_index, 0);
        assert_eq!(manager.active_pane().map(|p| p.id), Some(survivor));
    }

    #[test]
    fn test_set_default_scrollback_applies_to_new_panes() {
        let mut manager = PaneManager::new();
        manager.set_default_scrollback(123);
        assert_eq!(manager.default_scrollback, 123);
        spawn(&mut manager, "a", true);
        assert_eq!(manager.len(), 1);
        assert!(manager.resize_all(size()).is_ok());
    }
}
