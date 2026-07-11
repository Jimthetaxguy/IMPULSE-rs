//! Input routing for the ion REPL (TUI_SPEC.md T6, section 2.3).
//!
//! Splits one raw line of user input into a [`RouterOutcome`]: a recognized
//! slash command, an unrecognized slash command, chat text (stubbed until
//! T8), or the empty-line no-op. Deliberately synchronous and stdin-free —
//! `route` takes a `&str` and returns an enum, so it is unit-testable in
//! isolation from the interactive readline loop (`ion_repl::mod`), which is
//! awkward to drive in tests since it reads real stdin.

/// One parsed slash command recognized by the T6 REPL.
///
/// `Verify` carries its raw argument tokens (post shell-like splitting) so a
/// future `/verify [--repo P] [--diff-ref R] [description...]` (TUI_SPEC.md
/// §2.3) can be parsed without changing the router's shape; T6 only renders
/// a stub message for it (T7 wires it to `run_ion_verify` as a `ReplTool`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Quit,
    Clear,
    Verify(Vec<String>),
    /// `/tools` -- list the tools registered in the REPL's `ReplToolRegistry`
    /// (TUI_SPEC.md T7).
    Tools,
}

/// The result of routing one line of input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterOutcome {
    /// Blank/whitespace-only input — re-prompt, no output.
    Empty,
    /// A recognized slash command.
    Command(SlashCommand),
    /// `/foo` with no matching command. Carries the unrecognized name
    /// (without the leading `/`).
    UnknownCommand(String),
    /// Free text — a future chat turn (T8 wires this to `ChatSession`). T6
    /// renders a "chat isn't wired up yet" stub for every value here.
    ChatTurn(String),
}

/// Slash commands recognized by [`route`], used to render `/help` and the
/// unknown-command message so the two never drift out of sync.
pub const KNOWN_COMMANDS: &[&str] = &["/help", "/quit", "/clear", "/verify", "/tools"];

/// Route one line of raw input. Never reads stdin itself — the caller owns
/// the readline loop.
pub fn route(line: &str) -> RouterOutcome {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return RouterOutcome::Empty;
    }

    if let Some(rest) = trimmed.strip_prefix('/') {
        let tokens = split_args(rest);
        let name = tokens.first().cloned().unwrap_or_default();
        let args = if tokens.len() > 1 {
            tokens[1..].to_vec()
        } else {
            Vec::new()
        };
        return match name.as_str() {
            "help" => RouterOutcome::Command(SlashCommand::Help),
            "quit" => RouterOutcome::Command(SlashCommand::Quit),
            "clear" => RouterOutcome::Command(SlashCommand::Clear),
            "verify" => RouterOutcome::Command(SlashCommand::Verify(args)),
            "tools" => RouterOutcome::Command(SlashCommand::Tools),
            other => RouterOutcome::UnknownCommand(other.to_string()),
        };
    }

    RouterOutcome::ChatTurn(trimmed.to_string())
}

/// Shell-like whitespace splitting with single- and double-quote support, so
/// `/verify --description "fix the thing"` yields a single
/// `"fix the thing"` token instead of two. Deliberately minimal (no escape
/// sequences) — good enough for REPL slash-command args, not a full shlex.
fn split_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut in_single = false;
    let mut in_double = false;

    for c in input.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                has_current = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_current = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_current {
                    args.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            c => {
                current.push(c);
                has_current = true;
            }
        }
    }
    if has_current {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_empty_line_returns_empty() {
        assert_eq!(route(""), RouterOutcome::Empty);
    }

    #[test]
    fn test_route_whitespace_only_line_returns_empty() {
        assert_eq!(route("   \t  "), RouterOutcome::Empty);
    }

    #[test]
    fn test_route_help_returns_help_command() {
        assert_eq!(route("/help"), RouterOutcome::Command(SlashCommand::Help));
    }

    #[test]
    fn test_route_help_trims_surrounding_whitespace() {
        assert_eq!(
            route("  /help  "),
            RouterOutcome::Command(SlashCommand::Help)
        );
    }

    #[test]
    fn test_route_quit_returns_quit_command() {
        assert_eq!(route("/quit"), RouterOutcome::Command(SlashCommand::Quit));
    }

    #[test]
    fn test_route_clear_returns_clear_command() {
        assert_eq!(route("/clear"), RouterOutcome::Command(SlashCommand::Clear));
    }

    #[test]
    fn test_route_verify_with_no_args_returns_empty_args() {
        assert_eq!(
            route("/verify"),
            RouterOutcome::Command(SlashCommand::Verify(Vec::new()))
        );
    }

    #[test]
    fn test_route_verify_splits_unquoted_args() {
        assert_eq!(
            route("/verify --repo /tmp/foo --diff-ref HEAD~1..HEAD"),
            RouterOutcome::Command(SlashCommand::Verify(vec![
                "--repo".to_string(),
                "/tmp/foo".to_string(),
                "--diff-ref".to_string(),
                "HEAD~1..HEAD".to_string(),
            ]))
        );
    }

    #[test]
    fn test_route_verify_keeps_double_quoted_arg_as_one_token() {
        assert_eq!(
            route(r#"/verify --description "fix the thing""#),
            RouterOutcome::Command(SlashCommand::Verify(vec![
                "--description".to_string(),
                "fix the thing".to_string(),
            ]))
        );
    }

    #[test]
    fn test_route_verify_keeps_single_quoted_arg_as_one_token() {
        assert_eq!(
            route("/verify --description 'fix the thing'"),
            RouterOutcome::Command(SlashCommand::Verify(vec![
                "--description".to_string(),
                "fix the thing".to_string(),
            ]))
        );
    }

    #[test]
    fn test_route_unknown_command_returns_name_without_slash() {
        assert_eq!(
            route("/frobnicate"),
            RouterOutcome::UnknownCommand("frobnicate".to_string())
        );
    }

    #[test]
    fn test_route_bare_slash_returns_unknown_command_empty_name() {
        assert_eq!(route("/"), RouterOutcome::UnknownCommand(String::new()));
    }

    #[test]
    fn test_route_free_text_returns_chat_turn() {
        assert_eq!(
            route("what changed in this repo?"),
            RouterOutcome::ChatTurn("what changed in this repo?".to_string())
        );
    }

    #[test]
    fn test_route_free_text_trims_whitespace() {
        assert_eq!(
            route("  hello  "),
            RouterOutcome::ChatTurn("hello".to_string())
        );
    }

    #[test]
    fn test_known_commands_lists_all_slash_commands() {
        assert_eq!(
            KNOWN_COMMANDS,
            &["/help", "/quit", "/clear", "/verify", "/tools"]
        );
    }

    #[test]
    fn test_route_tools_returns_tools_command() {
        assert_eq!(route("/tools"), RouterOutcome::Command(SlashCommand::Tools));
    }
}
