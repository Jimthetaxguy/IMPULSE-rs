//! Ion REPL core (TUI_SPEC.md T6-T9): readline loop -> route -> render.
//!
//! [`ReplSession::run`] owns the interactive loop: read a line via
//! `rustyline`, route it through [`router::route`], dispatch through the
//! [`registry::ReplToolRegistry`] when the command names a tool, and print
//! the rendered response. This module wires the deterministic slash-command
//! surface (`/help`, `/quit`, `/clear`, `/verify`, `/tools`,
//! unknown-command), history persistence at `.impulse/ion_history`
//! (`history.rs`, `IMPULSE_HOME`-aware), and free-text chat turns via
//! [`chat::ChatState`] (T8: `/clear` really clears its history; T9: every
//! chat turn exposes the session's `ReplToolRegistry` to the model as
//! Anthropic tool-use schemas, so free text like "verify my diff" can
//! trigger `ion_verify` -- or `file_write`/`bash_exec` -- conversationally
//! instead of only via `/verify`).

pub mod chat;
pub mod history;
pub mod registry;
pub mod router;
pub mod tool_bridge;
pub mod tool_claim;
#[cfg(feature = "office-support")]
pub mod tool_document;
pub mod tool_verify;
pub mod tools;

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use chat::ChatState;
use registry::ReplToolRegistry;
use router::{RouterOutcome, SlashCommand};

const PROMPT: &str = "ion \u{276f} ";

/// Per-session state handed to `ReplTool::run` (T7) and the chat backend
/// (T8/T9). Kept as a struct (not inlined args) so additional fields stay
/// additive per TUI_SPEC.md section 2.3.
///
/// **Tool sandbox roots (Stage 1,
/// `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`):**
/// before this, `tool_bridge::DynamicToolBridge::run` gave every bridged
/// tool an unrestricted `ToolContext::with_all_capabilities()` -- once a
/// user typed `y` once, `file_write`/`bash_exec` could touch anywhere on
/// the host. `repo_root` now doubles as the session's fixed write root
/// (never overridable, not even by `/allow` or a `CONFIRM`);
/// `allowed_read_roots` is the session's *extension* list, grown one path
/// at a time via `/allow <path>` (`apply_allow`, below). See
/// [`ReplContext::sandbox_tool_context`] for how the two combine into the
/// `ToolContext` every bridged tool actually runs under.
#[derive(Debug, Default, Clone)]
pub struct ReplContext {
    /// Directory the REPL was launched from. `ReplTool`s (T7, e.g.
    /// `ion_verify`) use this as the default `--repo` for gate calls, and
    /// it is this session's fixed filesystem write root.
    pub repo_root: std::path::PathBuf,
    /// Additional read-only roots granted this session via `/allow <path>`.
    /// Never consulted for writes -- see the struct doc comment.
    pub allowed_read_roots: Vec<std::path::PathBuf>,
}

impl ReplContext {
    /// The write/read root to use when `repo_root` is unset (only
    /// `ReplContext::default()`, i.e. tests -- `ReplSession::new` always
    /// populates `repo_root` from `std::env::current_dir()`). Falls back to
    /// the process's own current directory rather than an empty path, which
    /// would otherwise sandbox every tool call to a path that can't exist.
    pub fn effective_repo_root(&self) -> std::path::PathBuf {
        if self.repo_root.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        } else {
            self.repo_root.clone()
        }
    }

    /// The sandboxed `ToolContext` every bridged tool
    /// (`tool_bridge::DynamicToolBridge`) actually executes under: writes
    /// limited to [`ReplContext::effective_repo_root`]; reads limited to
    /// that same root plus every `/allow`-granted path. All capabilities
    /// are still granted (`ion` is a CLI-launched coding agent, matching
    /// `ToolContext::with_all_capabilities`'s existing precedent) -- only
    /// the filesystem roots are narrowed, which is the piece that was
    /// previously unrestricted.
    pub fn sandbox_tool_context(&self) -> crate::tooling::ToolContext {
        let repo_root = self.effective_repo_root();
        let mut read_roots = vec![repo_root.clone()];
        read_roots.extend(self.allowed_read_roots.iter().cloned());
        crate::tooling::ToolContext {
            execution_origin: crate::tooling::ExecutionOrigin::Cli,
            allowed_read_roots: read_roots,
            allowed_write_roots: vec![repo_root],
            ..crate::tooling::ToolContext::with_all_capabilities()
        }
    }
}

/// Handles `/allow <path>`: grants an additional read root for the rest of
/// this session (never a write root -- see [`ReplContext`]'s doc comment).
/// A relative path resolves against the process's current working
/// directory, matching how `ToolContext::resolve_path` treats relative tool
/// arguments elsewhere. Returns the text to print; an empty argument list
/// is a usage error, not a panic or a silent no-op.
fn apply_allow(ctx: &mut ReplContext, args: &[String]) -> String {
    let Some(path_arg) = args.first() else {
        return "Usage: /allow <path> -- grant read access to an additional directory \
                or file for this session."
            .to_string();
    };
    let path = std::path::PathBuf::from(path_arg);
    let resolved = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    };
    ctx.allowed_read_roots.push(resolved.clone());
    format!(
        "Granted read access to {} for this session.",
        resolved.display()
    )
}

/// Owns the readline editor, history path, tool registry, chat state, and
/// REPL context for one interactive session. `chat` (T8) is built once here
/// and held mutably for the session's lifetime — routing a `ChatTurn`
/// through a freshly-constructed `ChatState` on every line would silently
/// drop conversation history after each turn.
pub struct ReplSession {
    editor: DefaultEditor,
    history_path: std::path::PathBuf,
    context: ReplContext,
    tools: ReplToolRegistry,
    chat: ChatState,
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
            context: ReplContext {
                repo_root,
                ..ReplContext::default()
            },
            tools: ReplToolRegistry::with_defaults(),
            chat: ChatState::from_env(),
        })
    }

    /// Runs the loop until `/quit` or Ctrl-D (EOF). Ctrl-C cancels the
    /// current line only (TUI_SPEC.md section 2.1) and re-prompts. Always
    /// attempts a history save on the way out, even after an editor error,
    /// so a session isn't lost by one bad readline call.
    pub async fn run(&mut self) -> Result<()> {
        loop {
            match self.editor.readline(PROMPT) {
                Ok(line) => {
                    let _ = self.editor.add_history_entry(line.as_str());
                    if self.handle_line(&line).await {
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
    async fn handle_line(&mut self, line: &str) -> bool {
        let (text, should_exit) = respond(
            router::route(line),
            &self.tools,
            &mut self.context,
            &mut self.chat,
        )
        .await;
        if !text.is_empty() {
            println!("{text}");
        }
        should_exit
    }
}

/// One-line notice shown when a chat turn is attempted with no usable API
/// key configured (`AgentError::MissingApiKey`). Slash commands (`/verify`,
/// `/tools`, `/help`, `/clear`) are unaffected — this only fires on the
/// `ChatTurn` branch below.
const MISSING_API_KEY_NOTICE: &str =
    "No ANTHROPIC_API_KEY set -- chat is unavailable, but /verify and /tools still work.";

/// Pure-ish rendering step: given a routed outcome, returns the text to
/// print and whether the session should exit. Kept separate from
/// `ReplSession::handle_line` so it is unit-testable without constructing a
/// `rustyline::Editor` (tools/registry/chat are passed in explicitly).
async fn respond(
    outcome: RouterOutcome,
    tools: &ReplToolRegistry,
    ctx: &mut ReplContext,
    chat: &mut ChatState,
) -> (String, bool) {
    match outcome {
        RouterOutcome::Empty => (String::new(), false),
        RouterOutcome::Command(SlashCommand::Help) => (help_text(tools), false),
        RouterOutcome::Command(SlashCommand::Quit) => ("Goodbye.".to_string(), true),
        RouterOutcome::Command(SlashCommand::Clear) => {
            chat.clear();
            ("Chat history cleared.".to_string(), false)
        }
        RouterOutcome::Command(SlashCommand::Verify(args)) => (
            run_tool_command(tools, "ion_verify", verify_args_to_json(&args), ctx).await,
            false,
        ),
        RouterOutcome::Command(SlashCommand::Tools) => (tools_text(tools), false),
        RouterOutcome::Command(SlashCommand::Allow(args)) => (apply_allow(ctx, &args), false),
        RouterOutcome::Command(SlashCommand::Loop) => (loop_report_text(chat), false),
        RouterOutcome::UnknownCommand(name) => (
            format!(
                "Unknown command: /{name}. Available: {}",
                router::KNOWN_COMMANDS.join(", ")
            ),
            false,
        ),
        RouterOutcome::ChatTurn(text) => {
            let reply = match chat.turn(&text, tools, ctx).await {
                Ok(reply) => reply,
                Err(crate::error::AgentError::MissingApiKey { .. }) => {
                    MISSING_API_KEY_NOTICE.to_string()
                }
                // AgentError::ToolLoopTimedOut (Opus adversarial-review
                // follow-up to T9, finding S2) falls through to the generic
                // branch below, same as its sibling ToolLoopLimitExceeded:
                // the Display message ("Tool-use loop exceeded its Ns
                // wall-clock budget without a final reply") is already
                // clear on its own, unlike MissingApiKey which needs a
                // pointer to /verify and /tools still working.
                Err(err) => format!("Chat failed: {err}"),
            };
            // ADR-0017 loop evidence: a trip is a fact worth surfacing
            // without a dedicated `/loop` call every time (which still
            // exists for the full report). Only a genuine trip is appended
            // here -- a normal `Completed` turn stays exactly as before.
            let reply = match chat.last_loop_report() {
                Some(report)
                    if matches!(
                        report.termination,
                        crate::loop_contract::LoopTermination::Tripped { .. }
                    ) =>
                {
                    format!("{reply}\n{}", loop_trip_summary(report))
                }
                _ => reply,
            };
            (reply, false)
        }
    }
}

/// One-line loop-evidence summary appended to a chat reply when
/// `ChatState::last_loop_report` shows the turn tripped (ADR-0017): what
/// tripped, plus the round/tool-call/error counters a human needs to judge
/// whether the model was actually stuck versus doing legitimate work that
/// happened to hit a limit.
fn loop_trip_summary(report: &crate::loop_contract::LoopReport) -> String {
    let crate::loop_contract::LoopTermination::Tripped { trip } = &report.termination else {
        // Unreachable from this module's only call site (guarded by the
        // same match arm above), but stay total rather than panic/unwrap on
        // a future caller that forgets the guard.
        return format!("[loop] {} ended without tripping.", report.contract);
    };
    format!(
        "[loop] {} tripped: {trip} (rounds={}, tool_calls={}, tool_errors={}, elapsed={}ms)",
        report.contract,
        report.rounds_used,
        report.tool_calls,
        report.tool_errors,
        report.elapsed_ms
    )
}

/// `/loop` -- full report from the most recent chat turn, or a notice when
/// no turn has run yet this session. Pretty JSON: it's the same typed
/// `LoopReport` a future harness-diagnosis loop would consume (ADR-0017),
/// so showing its real shape is more useful here than a hand-formatted
/// table.
fn loop_report_text(chat: &ChatState) -> String {
    match chat.last_loop_report() {
        Some(report) => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("failed to render loop report: {err}")),
        None => "No loop report yet -- run a chat turn first.".to_string(),
    }
}

/// Dispatches `tool_name` through `tools` with `args`, rendering either the
/// tool's own `ToolOutcome::rendered` text or an error message. Shared by
/// every slash command that maps 1:1 onto a `ReplTool` (currently only
/// `/verify` -> `ion_verify`; future gate/tool commands reuse this).
async fn run_tool_command(
    tools: &ReplToolRegistry,
    tool_name: &str,
    args: serde_json::Value,
    ctx: &ReplContext,
) -> String {
    match tools.get(tool_name) {
        Some(tool) => match tool.run(args, ctx).await {
            Ok(outcome) => outcome.rendered,
            Err(err) => format!("{tool_name} failed: {err:#}"),
        },
        None => format!("Tool '{tool_name}' is not registered."),
    }
}

/// Parses `/verify [--repo P] [--diff-ref R] [description...]` tokens
/// (already shell-split by `router::split_args`) into the `ion_verify`
/// ReplTool's JSON args shape.
fn verify_args_to_json(args: &[String]) -> serde_json::Value {
    let mut repo = None;
    let mut diff_ref = None;
    let mut description_parts = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" if i + 1 < args.len() => {
                repo = Some(args[i + 1].clone());
                i += 2;
            }
            "--diff-ref" if i + 1 < args.len() => {
                diff_ref = Some(args[i + 1].clone());
                i += 2;
            }
            other => {
                description_parts.push(other.to_string());
                i += 1;
            }
        }
    }

    let mut obj = serde_json::Map::new();
    if let Some(repo) = repo {
        obj.insert("repo".to_string(), serde_json::Value::String(repo));
    }
    if let Some(diff_ref) = diff_ref {
        obj.insert("diff_ref".to_string(), serde_json::Value::String(diff_ref));
    }
    if !description_parts.is_empty() {
        obj.insert(
            "description".to_string(),
            serde_json::Value::String(description_parts.join(" ")),
        );
    }
    serde_json::Value::Object(obj)
}

fn tools_text(tools: &ReplToolRegistry) -> String {
    let mut lines = vec!["Available tools:".to_string()];
    for tool in tools.list() {
        lines.push(format!("  {}: {}", tool.name(), tool.usage()));
    }
    lines.join("\n")
}

fn help_text(tools: &ReplToolRegistry) -> String {
    let mut lines = vec![
        "Available commands:".to_string(),
        "  /help    Show this message".to_string(),
        "  /verify  Run the Ion verification gate (ion_verify ReplTool)".to_string(),
        "  /tools   List available ReplTools".to_string(),
        "  /allow   Grant an additional read root for this session: /allow <path>".to_string(),
        "  /loop    Show the full loop report from the last chat turn".to_string(),
        "  /clear   Clear chat history".to_string(),
        "  /quit    Exit the REPL (Ctrl-D also works)".to_string(),
    ];
    if !tools.is_empty() {
        lines.push(String::new());
        lines.push(tools_text(tools));
    }
    lines.join("\n")
}

/// Prints the startup banner, then hands off to the readline loop. Entry
/// point called from `src/bin/ion.rs` for a bare `ion` invocation.
pub async fn run() -> Result<()> {
    println!(
        "ion {} \u{2014} Ion interactive harness. Type /help for commands, /quit or Ctrl-D to exit.",
        env!("CARGO_PKG_VERSION")
    );
    ReplSession::new()?.run().await
}

#[cfg(test)]
// See handlers::ion's test module for why holding env_lock() across .await
// here is intentional (must span the whole gate-launcher round trip) and
// safe (test-only std::sync::Mutex<()>, never contended by production code).
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the process-global `ION_GATE_LAUNCHER`
    /// env var, shared with `handlers::ion` and `tool_verify` via
    /// `crate::test_support` (see that module's doc comment for why a
    /// per-file lock is insufficient).
    use crate::test_support::init_git_repo;
    use crate::test_support::ion_gate_launcher_env_lock as env_lock;
    use chat::test_support::{EchoProvider, MissingKeyProvider};

    /// A `ChatState` that always fails with `AgentError::MissingApiKey`,
    /// deterministic regardless of the test process's ambient
    /// `ANTHROPIC_API_KEY` env state. Used by every `respond()` test that
    /// doesn't specifically exercise the chat-turn success path.
    fn test_chat() -> ChatState {
        ChatState::with_provider(
            Box::new(MissingKeyProvider),
            "missing-key-fake-model".into(),
        )
    }

    #[tokio::test]
    async fn test_respond_empty_returns_no_text_and_does_not_exit() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(RouterOutcome::Empty, &tools, &mut ctx, &mut chat).await;
        assert_eq!(text, "");
        assert!(!should_exit);
    }

    #[tokio::test]
    async fn test_respond_help_lists_all_known_commands_and_tools() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Help),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        for cmd in router::KNOWN_COMMANDS {
            assert!(text.contains(cmd), "help text missing {cmd}: {text}");
        }
        for tool in tools.list() {
            assert!(
                text.contains(tool.name()),
                "help text missing tool {}: {text}",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn test_respond_quit_says_goodbye_and_exits() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Quit),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert_eq!(text, "Goodbye.");
        assert!(should_exit);
    }

    #[tokio::test]
    async fn test_respond_clear_clears_chat_history_and_does_not_exit() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".into(),
        );
        // Build up history first, so /clear has something real to clear.
        chat.turn("hi", &tools, &ctx)
            .await
            .expect("fake provider succeeds");
        assert_eq!(chat.history_len(), 2);

        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Clear),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(text.to_lowercase().contains("cleared"));
        assert!(!should_exit);
        assert_eq!(
            chat.history_len(),
            0,
            "/clear must actually clear ChatState history, not just print a message"
        );
    }

    #[tokio::test]
    async fn test_respond_tools_lists_registered_tools() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Tools),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        assert!(text.contains("ion_verify"));
        assert!(text.contains("file_read"));
        assert!(text.contains("file_write"));
        assert!(text.contains("bash_exec"));
        #[cfg(feature = "office-support")]
        assert!(text.contains("document_read"));
    }

    #[tokio::test]
    async fn test_respond_verify_runs_ion_verify_tool_against_stub_gate() {
        let _guard = env_lock();
        let repo = init_git_repo();
        let stub_gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fakes/ion-verify-stub-gate.sh");
        std::env::set_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV, &stub_gate);

        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext {
            repo_root: repo.path().to_path_buf(),
            ..ReplContext::default()
        };
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Verify(vec![
                "--diff-ref".to_string(),
                "HEAD".to_string(),
            ])),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;

        std::env::remove_var(impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV);

        assert!(!should_exit);
        assert!(
            text.contains("Approve"),
            "unexpected /verify output: {text}"
        );
    }

    #[tokio::test]
    async fn test_respond_verify_reports_error_for_unregistered_tool() {
        let tools = ReplToolRegistry::new(); // deliberately empty
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Verify(Vec::new())),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        assert!(text.contains("not registered"));
    }

    #[test]
    fn test_verify_args_to_json_parses_repo_diff_ref_and_description() {
        let json = verify_args_to_json(&[
            "--repo".to_string(),
            "/tmp/foo".to_string(),
            "--diff-ref".to_string(),
            "HEAD~1..HEAD".to_string(),
            "fix".to_string(),
            "the".to_string(),
            "thing".to_string(),
        ]);
        assert_eq!(json["repo"], "/tmp/foo");
        assert_eq!(json["diff_ref"], "HEAD~1..HEAD");
        assert_eq!(json["description"], "fix the thing");
    }

    #[test]
    fn test_verify_args_to_json_empty_args_yields_empty_object() {
        let json = verify_args_to_json(&[]);
        assert_eq!(json, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_respond_unknown_command_lists_known_commands() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::UnknownCommand("frobnicate".to_string()),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(text.contains("/frobnicate"));
        for cmd in router::KNOWN_COMMANDS {
            assert!(
                text.contains(cmd),
                "unknown-command text missing {cmd}: {text}"
            );
        }
        assert!(!should_exit);
    }

    #[tokio::test]
    async fn test_respond_chat_turn_sends_text_to_chat_session_and_returns_reply() {
        // Proves a ChatTurn actually reaches ChatState::turn (and therefore
        // the underlying Agent/LlmProvider), not just a hardcoded stub
        // string, by asserting the fake provider's echoed reply comes back.
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".into(),
        );
        let (text, should_exit) = respond(
            RouterOutcome::ChatTurn("hello".to_string()),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert_eq!(text, "echo:hello");
        assert!(!should_exit);
    }

    #[tokio::test]
    async fn test_respond_chat_turn_missing_api_key_prints_graceful_notice_not_panic() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat(); // MissingKeyProvider
        let (text, should_exit) = respond(
            RouterOutcome::ChatTurn("hello".to_string()),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert_eq!(text, MISSING_API_KEY_NOTICE);
        assert!(!should_exit);
        // Slash commands must still work after a missing-key chat turn.
        let (help_text, _) = respond(
            RouterOutcome::Command(SlashCommand::Help),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(help_text.contains("/verify"));
    }

    #[test]
    fn test_repl_context_default_has_empty_repo_root() {
        let ctx = ReplContext::default();
        assert_eq!(ctx.repo_root, std::path::PathBuf::new());
        assert!(ctx.allowed_read_roots.is_empty());
    }

    #[test]
    fn test_effective_repo_root_falls_back_to_current_dir_when_unset() {
        let ctx = ReplContext::default();
        let expected = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        assert_eq!(ctx.effective_repo_root(), expected);
    }

    #[test]
    fn test_effective_repo_root_uses_repo_root_when_set() {
        let ctx = ReplContext {
            repo_root: std::path::PathBuf::from("/tmp/some-repo"),
            ..ReplContext::default()
        };
        assert_eq!(
            ctx.effective_repo_root(),
            std::path::PathBuf::from("/tmp/some-repo")
        );
    }

    #[test]
    fn test_sandbox_tool_context_limits_write_to_repo_root_and_extends_reads_with_allow_grants() {
        let ctx = ReplContext {
            repo_root: std::path::PathBuf::from("/tmp/some-repo"),
            allowed_read_roots: vec![std::path::PathBuf::from("/tmp/granted")],
        };
        let tool_ctx = ctx.sandbox_tool_context();
        assert_eq!(
            tool_ctx.allowed_write_roots,
            vec![std::path::PathBuf::from("/tmp/some-repo")]
        );
        assert_eq!(
            tool_ctx.allowed_read_roots,
            vec![
                std::path::PathBuf::from("/tmp/some-repo"),
                std::path::PathBuf::from("/tmp/granted"),
            ]
        );
    }

    #[test]
    fn test_apply_allow_with_no_args_returns_usage_and_does_not_mutate_context() {
        let mut ctx = ReplContext::default();
        let text = apply_allow(&mut ctx, &[]);
        assert!(text.to_lowercase().contains("usage"));
        assert!(ctx.allowed_read_roots.is_empty());
    }

    #[test]
    fn test_apply_allow_absolute_path_grants_it_verbatim() {
        let mut ctx = ReplContext::default();
        let text = apply_allow(&mut ctx, &["/tmp/some/dir".to_string()]);
        assert!(text.contains("/tmp/some/dir"));
        assert_eq!(
            ctx.allowed_read_roots,
            vec![std::path::PathBuf::from("/tmp/some/dir")]
        );
    }

    #[test]
    fn test_apply_allow_can_be_called_more_than_once_and_accumulates_grants() {
        let mut ctx = ReplContext::default();
        apply_allow(&mut ctx, &["/tmp/a".to_string()]);
        apply_allow(&mut ctx, &["/tmp/b".to_string()]);
        assert_eq!(
            ctx.allowed_read_roots,
            vec![
                std::path::PathBuf::from("/tmp/a"),
                std::path::PathBuf::from("/tmp/b"),
            ]
        );
    }

    #[tokio::test]
    async fn test_respond_allow_grants_a_read_root_via_the_router() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Allow(vec![
                "/tmp/granted-via-router".to_string()
            ])),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        assert!(text.contains("/tmp/granted-via-router"));
        assert_eq!(
            ctx.allowed_read_roots,
            vec![std::path::PathBuf::from("/tmp/granted-via-router")]
        );
    }

    #[tokio::test]
    async fn test_respond_loop_with_no_prior_turn_reports_no_report_yet() {
        let tools = ReplToolRegistry::with_defaults();
        let mut ctx = ReplContext::default();
        let mut chat = test_chat();
        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Loop),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        assert!(text.to_lowercase().contains("no loop report"));
    }

    #[tokio::test]
    async fn test_respond_loop_after_a_chat_turn_prints_the_completed_report() {
        let tools = ReplToolRegistry::new();
        let mut ctx = ReplContext::default();
        let mut chat = ChatState::with_provider(
            Box::new(EchoProvider { prefix: "echo:" }),
            "echo-fake-model".into(),
        );
        chat.turn("hi", &tools, &ctx)
            .await
            .expect("fake provider succeeds");

        let (text, should_exit) = respond(
            RouterOutcome::Command(SlashCommand::Loop),
            &tools,
            &mut ctx,
            &mut chat,
        )
        .await;
        assert!(!should_exit);
        assert!(text.contains("\"outcome\""), "expected pretty JSON: {text}");
        assert!(
            text.contains("completed") || text.contains("Completed"),
            "{text}"
        );
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
