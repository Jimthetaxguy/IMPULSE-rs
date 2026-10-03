//! `scout`: a disposable read-only subagent
//! (`docs/superpowers/specs/2026-10-02-ion-scout-subagent-design.md`).
//!
//! Ion's model hands one question about a workspace subtree to a fresh
//! `min-agent` run, which answers it with three read-only tools (`list_files`,
//! `read_file`, `search_text`) under a `cap-std` directory capability and then
//! is dropped. Nothing outlives the call except the returned report: no
//! transcript, trace file, memory, or session.
//!
//! The host, never the model, picks the model, connection, budget, and root
//! (ADR-0015). The tool has no side effects, so it stays outside
//! `chat::CONFIRMATION_REQUIRED_TOOLS`; it does spend tokens, so each registry
//! (one per REPL session) allows at most [`SCOUT_SESSION_LIMIT`] runs. Its
//! answer is model text derived from file contents and reaches the parent
//! model through the same untrusted tool-output envelope as every tool result.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use min_agent::agent::{run, Budget, RunOptions, RunReport};
use min_agent::config::{Auth, Connection, ModelProfile, Protocol};
use min_agent::model::{HttpModelClient, ModelClient};
use min_agent::tools::Workspace;
use min_agent::trace::Trace;
use serde_json::{json, Value};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;
use crate::loop_contract::ION_DEFAULT_WALL_CLOCK;

/// Scout runs one registry (one REPL session) may start.
pub const SCOUT_SESSION_LIMIT: usize = 5;
/// Model used when `ION_SCOUT_MODEL` is unset or blank.
pub const SCOUT_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
/// Environment variable that overrides the scout model.
pub const SCOUT_MODEL_ENV: &str = "ION_SCOUT_MODEL";
/// Credential variable the scout connection reads by name.
pub const SCOUT_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
/// Output-token ceiling per scout turn (Anthropic Messages requires one).
pub const SCOUT_MAX_OUTPUT_TOKENS: u32 = 4096;
/// Longest question accepted, in bytes.
pub const SCOUT_MAX_QUESTION_BYTES: usize = 8192;

/// Builds a fresh model client for one scout run. Called on the blocking
/// worker thread, once per run, so no client state is shared between runs.
pub type ScoutClientFactory = Arc<dyn Fn() -> Result<Box<dyn ModelClient>> + Send + Sync>;

/// The scout's budget: `min-agent`'s defaults with the wall clock halved
/// relative to Ion's own tool-loop budget, so one scout cannot consume the
/// whole parent exchange. Per-request time is capped to fit inside it.
pub fn scout_budget() -> Budget {
    let wall_clock = ION_DEFAULT_WALL_CLOCK / 2;
    let defaults = Budget::default();
    Budget {
        wall_clock,
        request_timeout: defaults.request_timeout.min(wall_clock / 2),
        ..defaults
    }
}

/// Anthropic Messages connection at `origin` (scheme + host + optional port,
/// as `llm_backends::anthropic::anthropic_api_origin` returns it).
pub fn scout_connection(origin: &str) -> Connection {
    Connection {
        protocol: Protocol::AnthropicMessages,
        base_url: format!("{}/v1", origin.trim_end_matches('/')),
        auth: Auth::HeaderEnv {
            header: "x-api-key".into(),
            env: SCOUT_API_KEY_ENV.into(),
        },
        proxy: None,
    }
}

/// Model profile for a scout run; a blank override falls back to the default.
pub fn scout_profile(model_override: Option<&str>) -> ModelProfile {
    let model = model_override
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(SCOUT_DEFAULT_MODEL);
    ModelProfile {
        connection: "ion-scout".into(),
        model: model.into(),
        native_tools: true,
        max_output_tokens: Some(SCOUT_MAX_OUTPUT_TOKENS),
        output_limit_parameter: None,
    }
}

/// Validated `scout` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoutRequest {
    pub question: String,
    pub path: Option<String>,
}

/// Parses `{question, path?}`; the question must be non-blank and bounded.
pub fn parse_scout_args(args: &Value) -> Result<ScoutRequest> {
    let question = args
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    ensure!(
        !question.is_empty(),
        "scout: 'question' must be a non-empty string"
    );
    ensure!(
        question.len() <= SCOUT_MAX_QUESTION_BYTES,
        "scout: 'question' is {} bytes; the limit is {SCOUT_MAX_QUESTION_BYTES}",
        question.len()
    );
    let path = match args.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) if p.trim().is_empty() => None,
        Some(Value::String(p)) => Some(p.trim().to_string()),
        Some(_) => bail!("scout: 'path' must be a string"),
    };
    Ok(ScoutRequest {
        question: question.to_string(),
        path,
    })
}

/// Resolves the scout root: `repo_root` by default, otherwise `raw` relative
/// to it. The canonical directory must lie inside the session's read
/// sandbox; symlinks are resolved before the check, and that same canonical
/// path is the one handed to `Workspace::open`.
pub fn resolve_scout_root(raw: Option<&str>, ctx: &ReplContext) -> Result<PathBuf> {
    let repo_root = ctx.effective_repo_root();
    let label = raw.unwrap_or(".");
    let joined = match raw {
        None => repo_root,
        Some(p) if PathBuf::from(p).is_absolute() => PathBuf::from(p),
        Some(p) => repo_root.join(p),
    };
    let canonical = joined
        .canonicalize()
        .with_context(|| format!("scout: cannot resolve path '{label}'"))?;
    if !ctx
        .sandbox_tool_context()
        .is_path_allowed(&canonical, false)
    {
        bail!(
            "scout: '{label}' resolves outside the session's read sandbox \
             (repo root plus any /allow grants); use /allow to grant access first"
        );
    }
    ensure!(canonical.is_dir(), "scout: '{label}' is not a directory");
    Ok(canonical)
}

/// Turns a finished run into the parent's tool outcome. Only a completed run
/// is `ok`; any other stop is reported with its partial text labeled as not
/// an answer.
pub fn outcome_from_report(report: &RunReport) -> Result<ToolOutcome> {
    let payload = serde_json::to_value(report).context("scout: cannot serialize run report")?;
    let reads: Vec<String> = report
        .calls
        .iter()
        .map(|c| match &c.path {
            Some(p) => format!("{} {p}", c.tool),
            None => c.tool.clone(),
        })
        .collect();
    let evidence = format!(
        "[scout: {} rounds, {} tool calls{}]",
        report.rounds,
        report.tool_calls,
        if reads.is_empty() {
            String::new()
        } else {
            format!("; {}", reads.join(", "))
        }
    );
    let ok = report.stop.is_completed();
    let rendered = match (&report.answer, ok) {
        (Some(answer), true) => format!("{answer}\n\n{evidence}"),
        _ => {
            let partial = report
                .last_text
                .as_deref()
                .map(|t| format!("\nPartial text (not an answer): {t}"))
                .unwrap_or_default();
            format!("scout stopped: {}{partial}\n\n{evidence}", report.stop)
        }
    };
    Ok(ToolOutcome {
        rendered,
        payload,
        ok,
    })
}

/// Ion's disposable read-only subagent tool.
pub struct ScoutTool {
    factory: ScoutClientFactory,
    session_limit: usize,
    used: AtomicUsize,
}

impl ScoutTool {
    /// A scout with an injected client factory and run cap.
    pub fn new(factory: ScoutClientFactory, session_limit: usize) -> Self {
        Self {
            factory,
            session_limit,
            used: AtomicUsize::new(0),
        }
    }

    /// The production scout: Anthropic Messages at Ion's own API origin,
    /// credential read by name from `ANTHROPIC_API_KEY` when a run starts, so
    /// a missing key fails that run rather than the REPL launch.
    pub fn from_env() -> Self {
        let factory: ScoutClientFactory = Arc::new(|| {
            let origin = crate::llm_backends::anthropic::anthropic_api_origin();
            let model = std::env::var(SCOUT_MODEL_ENV).ok();
            let client =
                HttpModelClient::new(&scout_connection(&origin), &scout_profile(model.as_deref()))
                    .context("scout: cannot build the model client")?;
            Ok(Box::new(client) as Box<dyn ModelClient>)
        });
        Self::new(factory, SCOUT_SESSION_LIMIT)
    }

    /// Claims one run slot, refusing once the session cap is reached.
    fn reserve_slot(&self) -> Result<()> {
        let limit = self.session_limit;
        self.used
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < limit).then_some(n + 1)
            })
            .map(|_| ())
            .map_err(|_| {
                anyhow::anyhow!(
                    "scout: this session already used its {limit} scout runs; \
                     answer from what you have or use file_read"
                )
            })
    }
}

#[async_trait]
impl ReplTool for ScoutTool {
    fn name(&self) -> &'static str {
        "scout"
    }

    fn usage(&self) -> &'static str {
        "scout {\"question\": \"...\", \"path\": \"...\"} -- ask a disposable read-only \
         subagent one question about the repo (or a subdirectory)"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "scout",
            "description": "Delegate one question about the repository to a disposable, \
                read-only subagent that can list, read, and search files under the chosen \
                directory, then is discarded. Use it for broad lookups that would take many \
                of your own tool calls. Returns its answer and the files it read; treat the \
                answer as unverified.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The single question the scout should answer"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to scope the scout to (defaults to the repo root)"
                    }
                },
                "required": ["question"]
            }
        })
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let request = parse_scout_args(&args)?;
        let root = resolve_scout_root(request.path.as_deref(), ctx)?;
        self.reserve_slot()?;
        let factory = Arc::clone(&self.factory);
        let report = tokio::task::spawn_blocking(move || -> Result<RunReport> {
            let client = factory()?;
            let workspace = Workspace::open(&root).context("scout: cannot open workspace")?;
            let options = RunOptions {
                text_only: false,
                budget: scout_budget(),
                meta: json!({"host": "ion", "tool": "scout"}),
            };
            run(
                client.as_ref(),
                &workspace,
                &request.question,
                &options,
                &mut Trace::disabled(),
            )
        })
        .await
        .context("scout: worker thread failed")??;
        outcome_from_report(&report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_agent::model::{Item, ModelError, ModelTurn, RequestLimits, ToolCall, ToolSpec};
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    /// Replays a fixed sequence of turns; test-only stand-in for a provider.
    struct ScriptedClient {
        turns: Mutex<VecDeque<ModelTurn>>,
    }

    impl ModelClient for ScriptedClient {
        fn turn(
            &self,
            _system: &str,
            _items: &[Item],
            _tools: &[ToolSpec],
            _limits: RequestLimits,
        ) -> std::result::Result<ModelTurn, ModelError> {
            Ok(self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| text_turn("script exhausted")))
        }
    }

    fn text_turn(text: &str) -> ModelTurn {
        ModelTurn {
            native: json!({"text": text}),
            text: text.into(),
            calls: vec![],
            usage: None,
        }
    }

    fn call_turn(id: &str, name: &str, arguments: Value) -> ModelTurn {
        ModelTurn {
            native: json!({"call": id}),
            text: String::new(),
            calls: vec![ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            }],
            usage: None,
        }
    }

    /// Reads NOTES.md, then answers with the planted fact.
    fn reading_factory() -> ScoutClientFactory {
        Arc::new(|| {
            Ok(Box::new(ScriptedClient {
                turns: Mutex::new(VecDeque::from([
                    call_turn("c1", "read_file", json!({"path": "NOTES.md"})),
                    text_turn("The planted fact is 42."),
                ])),
            }) as Box<dyn ModelClient>)
        })
    }

    /// Lists the root forever with fresh call ids, tripping the repeated-batch breaker.
    fn looping_factory() -> ScoutClientFactory {
        Arc::new(|| {
            let turns = (0..20)
                .map(|i| call_turn(&format!("c{i}"), "list_files", json!({"path": "."})))
                .collect();
            Ok(Box::new(ScriptedClient {
                turns: Mutex::new(turns),
            }) as Box<dyn ModelClient>)
        })
    }

    /// Records whether it was ever called, then fails.
    fn tripwire_factory(called: Arc<AtomicBool>) -> ScoutClientFactory {
        Arc::new(move || {
            called.store(true, Ordering::SeqCst);
            bail!("tripwire factory should not run")
        })
    }

    fn repo_with_notes() -> (tempfile::TempDir, ReplContext) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("NOTES.md"), "planted fact: 42\n").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let ctx = ReplContext {
            repo_root: dir.path().to_path_buf(),
            allowed_read_roots: Vec::new(),
        };
        (dir, ctx)
    }

    #[tokio::test]
    async fn test_scout_run_reads_file_and_returns_completed_answer() {
        let (_dir, ctx) = repo_with_notes();
        let tool = ScoutTool::new(reading_factory(), 5);
        let outcome = tool
            .run(json!({"question": "What is the planted fact?"}), &ctx)
            .await
            .unwrap();
        assert!(outcome.ok);
        assert!(outcome.rendered.starts_with("The planted fact is 42."));
        assert!(outcome.rendered.contains("read_file NOTES.md"));
        assert_eq!(outcome.payload["answer"], "The planted fact is 42.");
        assert_eq!(outcome.payload["stop"]["kind"], "completed");
        assert_eq!(outcome.payload["calls"][0]["tool"], "read_file");
        assert!(
            outcome.payload.get("transcript").is_none(),
            "the transcript must not leave the scout"
        );
    }

    #[tokio::test]
    async fn test_scout_run_budget_stop_is_not_success() {
        let (_dir, ctx) = repo_with_notes();
        let tool = ScoutTool::new(looping_factory(), 5);
        let outcome = tool
            .run(json!({"question": "Loop forever"}), &ctx)
            .await
            .unwrap();
        assert!(!outcome.ok);
        assert!(outcome.payload["answer"].is_null());
        assert_eq!(outcome.payload["stop"]["kind"], "budget_exceeded");
        assert!(outcome.rendered.starts_with("scout stopped:"));
    }

    #[tokio::test]
    async fn test_scout_run_sandbox_escape_refused_without_consuming_slot() {
        let (_dir, ctx) = repo_with_notes();
        let outside = tempfile::tempdir().unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let tool = ScoutTool::new(tripwire_factory(Arc::clone(&called)), 1);
        let escape = outside.path().display().to_string();
        let err = tool
            .run(json!({"question": "q", "path": escape}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("outside the session's read sandbox"));
        let err = tool
            .run(json!({"question": "q", "path": "../"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("outside the session's read sandbox"));
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(tool.used.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_scout_run_allows_granted_read_root() {
        let (_dir, mut ctx) = repo_with_notes();
        let granted = tempfile::tempdir().unwrap();
        std::fs::write(granted.path().join("NOTES.md"), "planted fact: 42\n").unwrap();
        ctx.allowed_read_roots.push(granted.path().to_path_buf());
        let tool = ScoutTool::new(reading_factory(), 5);
        let path = granted.path().display().to_string();
        let outcome = tool
            .run(json!({"question": "fact?", "path": path}), &ctx)
            .await
            .unwrap();
        assert!(outcome.ok);
    }

    #[tokio::test]
    async fn test_scout_run_session_limit_refuses_after_cap() {
        let (_dir, ctx) = repo_with_notes();
        let tool = ScoutTool::new(reading_factory(), 2);
        for _ in 0..2 {
            let outcome = tool.run(json!({"question": "fact?"}), &ctx).await.unwrap();
            assert!(outcome.ok);
        }
        let err = tool
            .run(json!({"question": "fact?"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("already used its 2 scout runs"));
    }

    #[tokio::test]
    async fn test_scout_run_factory_error_propagates() {
        let (_dir, ctx) = repo_with_notes();
        let called = Arc::new(AtomicBool::new(false));
        let tool = ScoutTool::new(tripwire_factory(Arc::clone(&called)), 1);
        let err = tool
            .run(json!({"question": "fact?"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("tripwire"));
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_parse_scout_args_valid_trims_and_defaults_path() {
        let parsed = parse_scout_args(&json!({"question": "  where?  ", "path": " "})).unwrap();
        assert_eq!(
            parsed,
            ScoutRequest {
                question: "where?".into(),
                path: None
            }
        );
        let parsed = parse_scout_args(&json!({"question": "q", "path": "src"})).unwrap();
        assert_eq!(parsed.path.as_deref(), Some("src"));
    }

    #[test]
    fn test_parse_scout_args_rejects_blank_missing_oversize_and_bad_path() {
        assert!(parse_scout_args(&json!({})).is_err());
        assert!(parse_scout_args(&json!({"question": "   "})).is_err());
        assert!(parse_scout_args(&json!({"question": 7})).is_err());
        let big = "x".repeat(SCOUT_MAX_QUESTION_BYTES + 1);
        assert!(parse_scout_args(&json!({ "question": big })).is_err());
        let err = parse_scout_args(&json!({"question": "q", "path": 3})).unwrap_err();
        assert!(format!("{err}").contains("'path' must be a string"));
    }

    #[test]
    fn test_resolve_scout_root_rejects_file_and_missing_paths() {
        let (_dir, ctx) = repo_with_notes();
        let err = resolve_scout_root(Some("NOTES.md"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("is not a directory"));
        let err = resolve_scout_root(Some("missing"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("cannot resolve path 'missing'"));
        let root = resolve_scout_root(Some("sub"), &ctx).unwrap();
        assert!(root.ends_with("sub"));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_scout_root_rejects_symlink_out_of_sandbox() {
        let (dir, ctx) = repo_with_notes();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let err = resolve_scout_root(Some("link"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("outside the session's read sandbox"));
    }

    #[test]
    fn test_scout_budget_fits_inside_parent_loop_and_validates() {
        let budget = scout_budget();
        assert!(budget.validate().is_ok());
        assert!(budget.wall_clock < ION_DEFAULT_WALL_CLOCK);
        assert!(budget.request_timeout <= budget.wall_clock);
    }

    #[test]
    fn test_scout_connection_uses_anthropic_messages_and_named_key() {
        let conn = scout_connection("https://api.anthropic.com/");
        assert!(conn.validate().is_ok());
        assert_eq!(
            conn.endpoint().unwrap().as_str(),
            "https://api.anthropic.com/v1/messages"
        );
        assert!(matches!(
            &conn.auth,
            Auth::HeaderEnv { header, env } if header == "x-api-key" && env == SCOUT_API_KEY_ENV
        ));
        assert!(scout_connection("http://127.0.0.1:4010").validate().is_ok());
    }

    #[test]
    fn test_scout_profile_override_and_blank_fallback() {
        assert_eq!(scout_profile(None).model, SCOUT_DEFAULT_MODEL);
        assert_eq!(scout_profile(Some("  ")).model, SCOUT_DEFAULT_MODEL);
        let profile = scout_profile(Some("claude-sonnet-5-5"));
        assert_eq!(profile.model, "claude-sonnet-5-5");
        assert_eq!(profile.max_output_tokens, Some(SCOUT_MAX_OUTPUT_TOKENS));
        assert!(profile.native_tools);
    }

    /// Live graduation check against the real provider. Opt-in:
    /// `ANTHROPIC_API_KEY=... cargo test --lib tool_scout -- --ignored`.
    /// Sends only a synthetic temp workspace.
    #[tokio::test]
    #[ignore = "live provider call; requires ANTHROPIC_API_KEY"]
    async fn test_scout_from_env_live_round_trip_finds_planted_fact() {
        let (_dir, ctx) = repo_with_notes();
        let tool = ScoutTool::from_env();
        let outcome = tool
            .run(
                json!({"question": "Read NOTES.md and state the planted fact number."}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(outcome.ok, "scout did not complete: {}", outcome.rendered);
        assert!(outcome.rendered.contains("42"), "{}", outcome.rendered);
    }

    #[test]
    fn test_scout_schema_name_matches_tool_name_and_requires_question() {
        let tool = ScoutTool::new(reading_factory(), 1);
        let schema = tool.json_schema();
        assert_eq!(schema["name"], tool.name());
        assert_eq!(schema["input_schema"]["required"], json!(["question"]));
    }
}
