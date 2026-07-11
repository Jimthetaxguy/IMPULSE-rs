//! Ion REPL core (TUI_SPEC.md T6): readline loop -> route -> render.
//!
//! [`ReplSession::run`] owns the interactive loop: read a line via
//! `rustyline`, route it through [`router::route`], and print the rendered
//! response. Chat turns (T8) and tool-calling (T7/T9) are intentionally
//! stubbed here — this module wires the deterministic slash-command surface
//! (`/help`, `/quit`, `/clear`, unknown-command) plus history persistence at
//! `.impulse/ion_history` (`history.rs`, `IMPULSE_HOME`-aware).

pub mod history;
pub mod router;
pub mod tools;

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use router::{RouterOutcome, SlashCommand};

const PROMPT: &str = "ion \u{276f} ";

/// Per-session state handed to `ReplTool::run` (T7) and, later, the chat
/// backend (T8). Only `repo_root` is populated in T6; kept as a struct (not
/// inlined args) so future fields (tool registry, `ChatSession`, transcript)
/// are additive per TUI_SPEC.md section 2.3.
#[derive(Debug, Default, Clone)]
pub struct ReplContext {
    /// Directory the REPL was launched from. Future `ReplTool`s (T7) use
    /// this as the default `--repo` for gate calls.
    pub repo_root: std::path::PathBuf,
}

/// Owns the readline editor, history path, and REPL context for one
/// interactive session.
pub struct ReplSession {
    editor: DefaultEditor,
    history_path: std::path::PathBuf,
    #[allow(dead_code)] // dead_code: consumed by T7 (ReplTool dispatch) / T8 (ChatSession)
    context: ReplContext,
}

impl ReplSession {
    /// Creates a session, loading persisted history if present. A history
    /// load failure is a warning, not a hard error — the REPL still starts
    /// with empty history.
    pub fn new() -> Result<Self> {
        let mut editor = DefaultEditor::new()?;
        let history_path = history::history_path();
        if let Err(err) = history::load(&mut editor, &history_path) {
            eprintln!(
                "Note: could not load ion history from {} ({err})",
                history_path.display()
            );
        }
        let repo_root = std::env::current_dir().unwrap_or_default();
        Ok(Self {
            editor,
            history_path,
            context: ReplContext { repo_root },
        })
    }

    /// Runs the loop until `/quit` or Ctrl-D (EOF). Ctrl-C cancels the
    /// current line only (TUI_SPEC.md section 2.1) and re-prompts. Always
    /// attempts a history save on the way out, even after an editor error,
    /// so a session isn't lost by one bad readline call.
    pub fn run(&mut self) -> Result<()> {
        loop {
            match self.editor.readline(PROMPT) {
                Ok(line) => {
                    let _ = self.editor.add_history_entry(line.as_str());
                    if self.handle_line(&line) {
                        break;
                    }
                }
                Err(ReadlineError::Eof) => {
                    println!("Goodbye.");
                    break;
                }
                Err(ReadlineError::Interrupted) => {
                    continue;
                }
                Err(err) => {
                    eprintln!("ion repl error: {err}");
                    break;
                }
            }
        }

        if let Err(err) = history::save(&mut self.editor, &self.history_path) {
            eprintln!(
                "Note: could not save ion history to {} ({err})",
                self.history_path.display()
            );
        }
        Ok(())
    }

    /// Routes and renders one line of input. Returns `true` if the session
    /// should exit.
    fn handle_line(&mut self, line: &str) -> bool {
        let (text, should_exit) = respond(router::route(line));
        if !text.is_empty() {
            println!("{text}");
        }
        should_exit
    }
}

/// Pure rendering step: given a routed outcome, returns the text to print
/// and whether the session should exit. Kept separate from
/// `ReplSession::handle_line` so it is unit-testable without constructing a
/// `rustyline::Editor`.
fn respond(outcome: RouterOutcome) -> (String, bool) {
    match outcome {
        RouterOutcome::Empty => (String::new(), false),
        RouterOutcome::Command(SlashCommand::Help) => (help_text(), false),
        RouterOutcome::Command(SlashCommand::Quit) => ("Goodbye.".to_string(), true),
        RouterOutcome::Command(SlashCommand::Clear) => (
            "Nothing to clear yet -- chat history isn't wired up (TUI_SPEC.md T8 adds it)."
                .to_string(),
            false,
        ),
        RouterOutcome::Command(SlashCommand::Verify(_args)) => (
            "`/verify` isn't wired up in the REPL yet -- run `ion verify` from the shell for now \
             (TUI_SPEC.md T7 wires it in as a ReplTool)."
                .to_string(),
            false,
        ),
        RouterOutcome::UnknownCommand(name) => (
            format!(
                "Unknown command: /{name}. Available: {}",
                router::KNOWN_COMMANDS.join(", ")
            ),
            false,
        ),
        RouterOutcome::ChatTurn(_text) => (
            "Chat isn't wired up yet -- try /verify or /help (TUI_SPEC.md T8 adds chat)."
                .to_string(),
            false,
        ),
    }
}

fn help_text() -> String {
    [
        "Available commands:",
        "  /help    Show this message",
        "  /verify  (stub) run the Ion verification gate -- wired up in T7",
        "  /clear   (stub) clear chat history -- wired up in T8",
        "  /quit    Exit the REPL (Ctrl-D also works)",
    ]
    .join("\n")
}

/// Prints the startup banner, then hands off to the readline loop. Entry
/// point called from `src/bin/ion.rs` for a bare `ion` invocation.
pub fn run() -> Result<()> {
    println!(
        "ion {} \u{2014} Ion interactive harness. Type /help for commands, /quit or Ctrl-D to exit.",
        env!("CARGO_PKG_VERSION")
    );
    ReplSession::new()?.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_respond_empty_returns_no_text_and_does_not_exit() {
        let (text, should_exit) = respond(RouterOutcome::Empty);
        assert_eq!(text, "");
        assert!(!should_exit);
    }

    #[test]
    fn test_respond_help_lists_all_known_commands() {
        let (text, should_exit) = respond(RouterOutcome::Command(SlashCommand::Help));
        assert!(!should_exit);
        for cmd in router::KNOWN_COMMANDS {
            assert!(text.contains(cmd), "help text missing {cmd}: {text}");
        }
    }

    #[test]
    fn test_respond_quit_says_goodbye_and_exits() {
        let (text, should_exit) = respond(RouterOutcome::Command(SlashCommand::Quit));
        assert_eq!(text, "Goodbye.");
        assert!(should_exit);
    }

    #[test]
    fn test_respond_clear_is_a_placeholder_and_does_not_exit() {
        let (text, should_exit) = respond(RouterOutcome::Command(SlashCommand::Clear));
        assert!(text.to_lowercase().contains("nothing to clear"));
        assert!(!should_exit);
    }

    #[test]
    fn test_respond_verify_is_a_stub_and_does_not_exit() {
        let (text, should_exit) = respond(RouterOutcome::Command(SlashCommand::Verify(vec![
            "--repo".to_string(),
            ".".to_string(),
        ])));
        assert!(text.contains("/verify"));
        assert!(text.to_lowercase().contains("isn't wired up"));
        assert!(!should_exit);
    }

    #[test]
    fn test_respond_unknown_command_lists_known_commands() {
        let (text, should_exit) = respond(RouterOutcome::UnknownCommand("frobnicate".to_string()));
        assert!(text.contains("/frobnicate"));
        for cmd in router::KNOWN_COMMANDS {
            assert!(
                text.contains(cmd),
                "unknown-command text missing {cmd}: {text}"
            );
        }
        assert!(!should_exit);
    }

    #[test]
    fn test_respond_chat_turn_is_a_stub_and_does_not_exit() {
        let (text, should_exit) = respond(RouterOutcome::ChatTurn("hello".to_string()));
        assert!(text.to_lowercase().contains("chat isn't wired up"));
        assert!(!should_exit);
    }

    #[test]
    fn test_repl_context_default_has_empty_repo_root() {
        let ctx = ReplContext::default();
        assert_eq!(ctx.repo_root, std::path::PathBuf::new());
    }

    #[test]
    fn test_repl_session_new_constructs_without_a_tty() {
        // DefaultEditor::new() must not require an interactive terminal to
        // construct (only `readline()` needs one); this proves the REPL can
        // be built headlessly, matching how it will be exercised by the
        // `tests/ion_binary.rs` integration test with piped stdin.
        let session = ReplSession::new();
        assert!(session.is_ok(), "{:?}", session.err());
    }
}
