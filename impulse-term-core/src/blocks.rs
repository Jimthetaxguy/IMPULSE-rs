//! Command-block model — Warp-style addressable units of "one command +
//! its output" inside a terminal stream.
//!
//! # The block insight
//!
//! A traditional terminal is a stream of characters. Tools like Warp
//! (~$1B-valuation insight) realized that for *most* terminal use, the
//! meaningful unit is not a character or a row but a **block**: one shell
//! prompt + the command typed + the output produced + the exit code.
//!
//! This shape matches Impulse's data model — `HISTORY.jsonl` is already
//! block-structured. Surfacing it in the UI unlocks:
//!
//! - **Click to copy** an entire command's output
//! - **Click to rerun** the same command
//! - **Hover for exit status** (✓ green / ✗ red gutter mark)
//! - **Collapse old blocks** to keep the visible window compact
//! - **"Ask AI about this block"** — pass the typed unit to the agent
//!
//! # Block lifecycle (OSC 133 mapped)
//!
//! Blocks transition through these states, driven by the OSC 133 escape
//! sequences emitted by shell-integration scripts (L170 wires the
//! per-shell `PROMPT_COMMAND`/`precmd`/`fish_prompt` injectors):
//!
//! ```text
//!   PromptShown ── OSC 133;A ──> [prompt rendered]
//!   AwaitingCommand ── OSC 133;B ──> [user typing]
//!   Streaming ── OSC 133;C ──> [command output flowing]
//!   Finished(exit) ── OSC 133;D;<exit> ──> [command done]
//! ```
//!
//! The OSC 133 protocol is the same one VS Code uses for shell integration
//! (its "command decorations" in the terminal UI). Choosing this protocol
//! means we get instant compatibility with users' existing shell setups
//! that already emit it.
//!
//! # Status (L168)
//!
//! This module defines the data model and a hand-driven `BlockStore`. The
//! actual escape-sequence parser that drives state transitions from the
//! PTY byte stream lands at L169.

use serde::{Deserialize, Serialize};

/// A block's lifecycle state. Drives UI affordances: a `Streaming` block
/// shows an inline spinner; a `Finished(Some(0))` block shows ✓; a
/// `Finished(Some(n))` with `n != 0` shows ✗ + exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockState {
    /// OSC 133;A received — the prompt is rendered, no command yet.
    PromptShown,
    /// OSC 133;B received — user is typing a command.
    AwaitingCommand,
    /// OSC 133;C received — Enter pressed, output is streaming.
    Streaming,
    /// OSC 133;D;<exit> received — command finished. `None` exit code
    /// means the shell didn't report one (some integrations omit it).
    Finished(Option<i32>),
}

impl BlockState {
    /// Whether the block is in a terminal (non-changing) state.
    pub fn is_finished(self) -> bool {
        matches!(self, BlockState::Finished(_))
    }

    /// Status icon hint for the renderer. Toolkit-neutral string.
    pub fn status_icon(self) -> &'static str {
        match self {
            BlockState::PromptShown | BlockState::AwaitingCommand => "·",
            BlockState::Streaming => "⟳",
            BlockState::Finished(Some(0)) => "✓",
            BlockState::Finished(Some(_)) => "✗",
            BlockState::Finished(None) => "?",
        }
    }
}

/// One command+output block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// Monotonic ID assigned by `BlockStore`. Stable for the store's lifetime.
    pub id: u64,
    /// Text of the prompt (between OSC 133;A and OSC 133;B). Often empty
    /// if the shell only emits the boundary markers without sending the
    /// prompt text through the OSC payload.
    pub prompt: Option<String>,
    /// Text of the command the user typed (between OSC 133;B and OSC 133;C).
    pub command: Option<String>,
    /// Output the command produced (after OSC 133;C, until OSC 133;D).
    pub output: String,
    /// Current lifecycle state.
    pub state: BlockState,
}

impl Block {
    fn new(id: u64) -> Self {
        Self {
            id,
            prompt: None,
            command: None,
            output: String::new(),
            state: BlockState::PromptShown,
        }
    }

    /// Combined display text — prompt + command + output. Used by the
    /// "copy whole block" affordance.
    pub fn full_text(&self) -> String {
        let mut s = String::new();
        if let Some(p) = &self.prompt {
            s.push_str(p);
        }
        if let Some(c) = &self.command {
            s.push_str(c);
            s.push('\n');
        }
        s.push_str(&self.output);
        s
    }
}

/// Append-only collection of blocks.
///
/// Blocks are created by `open_prompt` and transition through state via
/// `open_command`, `open_output`, `close_with_exit`. Output text streams
/// in through `append_output`. `current_mut` returns the active (last)
/// block for the parser at L169 to feed into.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockStore {
    pub blocks: Vec<Block>,
    next_id: u64,
}

impl BlockStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new block at `PromptShown`. Returns the new block's ID.
    pub fn open_prompt(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.blocks.push(Block::new(id));
        id
    }

    /// Transition the current block to `AwaitingCommand`. No-op if there
    /// is no current block (parser desync). Returns true if the
    /// transition happened.
    pub fn open_command(&mut self) -> bool {
        if let Some(b) = self.blocks.last_mut() {
            b.state = BlockState::AwaitingCommand;
            true
        } else {
            false
        }
    }

    /// Transition the current block to `Streaming`. No-op if there is no
    /// current block. Returns true if the transition happened.
    pub fn open_output(&mut self) -> bool {
        if let Some(b) = self.blocks.last_mut() {
            b.state = BlockState::Streaming;
            true
        } else {
            false
        }
    }

    /// Transition the current block to `Finished(exit_code)`. No-op if
    /// there is no current block. Returns true if the transition happened.
    pub fn close_with_exit(&mut self, exit_code: Option<i32>) -> bool {
        if let Some(b) = self.blocks.last_mut() {
            b.state = BlockState::Finished(exit_code);
            true
        } else {
            false
        }
    }

    /// Append output to the current block. No-op if there is no current
    /// block. Returns the byte count appended (0 if no-op).
    pub fn append_output(&mut self, text: &str) -> usize {
        if let Some(b) = self.blocks.last_mut() {
            b.output.push_str(text);
            text.len()
        } else {
            0
        }
    }

    /// Set the prompt text on the current block. Replaces any existing.
    pub fn set_prompt(&mut self, text: impl Into<String>) -> bool {
        if let Some(b) = self.blocks.last_mut() {
            b.prompt = Some(text.into());
            true
        } else {
            false
        }
    }

    /// Set the command text on the current block. Replaces any existing.
    pub fn set_command(&mut self, text: impl Into<String>) -> bool {
        if let Some(b) = self.blocks.last_mut() {
            b.command = Some(text.into());
            true
        } else {
            false
        }
    }

    /// Borrow the current (last) block, if any.
    pub fn current(&self) -> Option<&Block> {
        self.blocks.last()
    }

    /// Mutable borrow of the current (last) block, if any.
    pub fn current_mut(&mut self) -> Option<&mut Block> {
        self.blocks.last_mut()
    }

    /// Total number of blocks tracked.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Find a block by its ID. Linear scan; block counts are typically
    /// small (~hundreds, not millions).
    pub fn get(&self, id: u64) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_store_is_empty() {
        let store = BlockStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert!(store.current().is_none());
    }

    #[test]
    fn test_open_prompt_creates_block_with_prompt_state() {
        let mut store = BlockStore::new();
        let id = store.open_prompt();
        assert_eq!(id, 0);
        assert_eq!(store.len(), 1);
        let block = store.current().expect("block exists");
        assert_eq!(block.id, 0);
        assert_eq!(block.state, BlockState::PromptShown);
        assert!(block.prompt.is_none());
        assert!(block.command.is_none());
        assert!(block.output.is_empty());
    }

    #[test]
    fn test_block_ids_are_monotonic() {
        let mut store = BlockStore::new();
        let id0 = store.open_prompt();
        let id1 = store.open_prompt();
        let id2 = store.open_prompt();
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_state_transitions_full_lifecycle() {
        let mut store = BlockStore::new();
        store.open_prompt();
        assert_eq!(store.current().unwrap().state, BlockState::PromptShown);

        assert!(store.open_command());
        assert_eq!(store.current().unwrap().state, BlockState::AwaitingCommand);

        assert!(store.open_output());
        assert_eq!(store.current().unwrap().state, BlockState::Streaming);

        assert!(store.close_with_exit(Some(0)));
        assert_eq!(
            store.current().unwrap().state,
            BlockState::Finished(Some(0))
        );
        assert!(store.current().unwrap().state.is_finished());
    }

    #[test]
    fn test_transitions_without_current_block_are_noop() {
        let mut store = BlockStore::new();
        assert!(!store.open_command());
        assert!(!store.open_output());
        assert!(!store.close_with_exit(Some(0)));
        assert!(!store.set_prompt("$ "));
        assert!(!store.set_command("ls"));
        assert_eq!(store.append_output("hello"), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn test_append_output_concatenates() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.open_output();
        assert_eq!(store.append_output("line 1\n"), 7);
        assert_eq!(store.append_output("line 2\n"), 7);
        let block = store.current().unwrap();
        assert_eq!(block.output, "line 1\nline 2\n");
    }

    #[test]
    fn test_set_prompt_and_command() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_prompt("$ ");
        store.open_command();
        store.set_command("echo hi");
        store.open_output();
        store.append_output("hi\n");
        store.close_with_exit(Some(0));

        let b = store.current().unwrap();
        assert_eq!(b.prompt.as_deref(), Some("$ "));
        assert_eq!(b.command.as_deref(), Some("echo hi"));
        assert_eq!(b.output, "hi\n");
        assert_eq!(b.state, BlockState::Finished(Some(0)));
    }

    #[test]
    fn test_full_text_concatenates_components() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_prompt("$ ");
        store.set_command("ls");
        store.append_output("file1 file2\n");
        let b = store.current().unwrap();
        assert_eq!(b.full_text(), "$ ls\nfile1 file2\n");
    }

    #[test]
    fn test_status_icon_for_each_state() {
        assert_eq!(BlockState::PromptShown.status_icon(), "·");
        assert_eq!(BlockState::AwaitingCommand.status_icon(), "·");
        assert_eq!(BlockState::Streaming.status_icon(), "⟳");
        assert_eq!(BlockState::Finished(Some(0)).status_icon(), "✓");
        assert_eq!(BlockState::Finished(Some(1)).status_icon(), "✗");
        assert_eq!(BlockState::Finished(Some(127)).status_icon(), "✗");
        assert_eq!(BlockState::Finished(None).status_icon(), "?");
    }

    #[test]
    fn test_get_by_id_returns_correct_block() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_command("first");
        store.open_prompt();
        store.set_command("second");
        store.open_prompt();
        store.set_command("third");

        assert_eq!(store.get(0).unwrap().command.as_deref(), Some("first"));
        assert_eq!(store.get(1).unwrap().command.as_deref(), Some("second"));
        assert_eq!(store.get(2).unwrap().command.as_deref(), Some("third"));
        assert!(store.get(99).is_none());
    }

    #[test]
    fn test_block_serde_round_trip() {
        let mut store = BlockStore::new();
        store.open_prompt();
        store.set_prompt("> ");
        store.open_command();
        store.set_command("pwd");
        store.open_output();
        store.append_output("/home/user\n");
        store.close_with_exit(Some(0));

        let json = serde_json::to_string(&store).unwrap();
        let recovered: BlockStore = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.len(), 1);
        let b = recovered.current().unwrap();
        assert_eq!(b.command.as_deref(), Some("pwd"));
        assert_eq!(b.output, "/home/user\n");
        assert_eq!(b.state, BlockState::Finished(Some(0)));
    }

    #[test]
    fn test_finished_state_with_nonzero_exit_is_distinguishable() {
        assert!(BlockState::Finished(Some(0)).is_finished());
        assert!(BlockState::Finished(Some(127)).is_finished());
        assert!(BlockState::Finished(None).is_finished());
        assert!(!BlockState::PromptShown.is_finished());
        assert!(!BlockState::Streaming.is_finished());
    }
}
