//! `photon`: a disposable read-only subagent
//! (`docs/superpowers/specs/2026-10-02-ion-photon-subagent-design.md`).
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
//! (one per REPL session) allows at most [`PHOTON_SESSION_LIMIT`] runs. Its
//! answer is model text derived from file contents and reaches the parent
//! model through the same untrusted tool-output envelope as every tool result.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use min_agent::agent::{run, Budget, RunOptions, RunReport};
use min_agent::model::{HttpModelClient, ModelClient};
use min_agent::tools::Workspace;
use min_agent::trace::Trace;
use serde_json::{json, Value};

use super::tools::{ReplTool, ToolOutcome};
use super::ReplContext;
use crate::loop_contract::ION_DEFAULT_WALL_CLOCK;
use crate::model_endpoint::{
    min_agent_bridge, EndpointRole, ModelEndpoint, ModelEndpointConfig, TrustedModelHosts,
    TRUSTED_MODEL_HOSTS_ENV,
};

/// Photon runs one registry (one REPL session) may start.
pub const PHOTON_SESSION_LIMIT: usize = 5;
/// Model used when `ION_PHOTON_MODEL` is unset or blank.
pub const PHOTON_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
/// Environment variable that overrides the photon model.
pub const PHOTON_MODEL_ENV: &str = "ION_PHOTON_MODEL";
/// Output-token ceiling per photon turn (Anthropic Messages requires one).
pub const PHOTON_MAX_OUTPUT_TOKENS: u32 = 4096;
/// Longest question accepted, in bytes.
pub const PHOTON_MAX_QUESTION_BYTES: usize = 8192;

/// Builds a fresh model client for one photon run against `endpoint`.
/// Called on the blocking worker thread, once per run, so no client state is
/// shared between runs.
pub type PhotonClientFactory =
    Arc<dyn Fn(&ModelEndpoint) -> Result<Box<dyn ModelClient>> + Send + Sync>;

/// Chooses the endpoint for one photon run from the session context.
pub type PhotonEndpointResolver = Arc<dyn Fn(&ReplContext) -> Result<ModelEndpoint> + Send + Sync>;

/// Environment variable naming Ion's provider; consulted only to refuse a
/// silent cross-vendor default (ADR-0022 rule 6).
const PROVIDER_ENV: &str = "IMPULSE_PROVIDER";

/// The photon's budget: `min-agent`'s defaults with the wall clock halved
/// relative to Ion's own tool-loop budget, so one photon cannot consume the
/// whole parent exchange. Per-request time is capped to fit inside it.
pub fn photon_budget() -> Budget {
    let wall_clock = ION_DEFAULT_WALL_CLOCK / 2;
    let defaults = Budget::default();
    Budget {
        wall_clock,
        request_timeout: defaults.request_timeout.min(wall_clock / 2),
        ..defaults
    }
}

/// The endpoint for one photon run (ADR-0022 rule 6): the `roles.photon`
/// profile when assigned; otherwise Anthropic Messages at `anthropic_origin`
/// with the `ION_PHOTON_MODEL` override or the default model. When Ion runs on
/// another provider and no photon profile exists, this refuses instead of
/// guessing a model on a vendor the user did not choose.
///
/// A configured profile comes from the project's `config.json`, which a
/// cloned repository controls, so its credential may only go to the
/// protocol's vendor host, a numeric loopback address, or a host the user
/// lists in `IMPULSE_TRUSTED_MODEL_HOSTS`.
pub fn resolve_photon_endpoint(
    config: &ModelEndpointConfig,
    env: &dyn Fn(&str) -> Option<String>,
    anthropic_origin: &str,
) -> Result<ModelEndpoint> {
    if let Some(endpoint) = config.endpoint_for(EndpointRole::Photon)? {
        endpoint
            .validate()
            .context("photon: the configured photon profile is invalid")?;
        let trusted = TrustedModelHosts::from_list(env(TRUSTED_MODEL_HOSTS_ENV).as_deref());
        endpoint
            .check_credential_destination(&trusted)
            .context("photon: the configured photon profile is refused")?;
        return Ok(endpoint.clone());
    }
    let provider = env(PROVIDER_ENV)
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty());
    if let Some(provider) = provider.filter(|p| p != "anthropic" && p != "claude") {
        bail!(
            "photon: {PROVIDER_ENV} is '{provider}' and no photon endpoint is configured; \
             add a profile under model_endpoints.profiles in .impulse/config.json and set \
             model_endpoints.roles.photon to its name"
        );
    }
    let model = env(PHOTON_MODEL_ENV)
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| PHOTON_DEFAULT_MODEL.to_string());
    Ok(ModelEndpoint::anthropic_messages(
        anthropic_origin,
        &model,
        PHOTON_MAX_OUTPUT_TOKENS,
    ))
}

/// Reads the `model_endpoints` section of `<impulse_dir>/config.json`. A
/// missing file is the empty default; an unreadable or malformed one is an
/// error, so a typo never silently falls back to another endpoint.
pub fn load_endpoint_config(impulse_dir: &Path) -> Result<ModelEndpointConfig> {
    #[derive(serde::Deserialize, Default)]
    struct Section {
        #[serde(default)]
        model_endpoints: ModelEndpointConfig,
    }
    let path = impulse_dir.join("config.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(e) => return Err(e).with_context(|| format!("photon: cannot read {}", path.display())),
    };
    let section: Section = serde_json::from_str(&raw)
        .with_context(|| format!("photon: cannot parse model_endpoints in {}", path.display()))?;
    Ok(section.model_endpoints)
}

/// Validated `photon` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotonRequest {
    pub question: String,
    pub path: Option<String>,
}

/// Parses `{question, path?}`; the question must be non-blank and bounded.
pub fn parse_photon_args(args: &Value) -> Result<PhotonRequest> {
    let question = args
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    ensure!(
        !question.is_empty(),
        "photon: 'question' must be a non-empty string"
    );
    ensure!(
        question.len() <= PHOTON_MAX_QUESTION_BYTES,
        "photon: 'question' is {} bytes; the limit is {PHOTON_MAX_QUESTION_BYTES}",
        question.len()
    );
    let path = match args.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) if p.trim().is_empty() => None,
        Some(Value::String(p)) => Some(p.trim().to_string()),
        Some(_) => bail!("photon: 'path' must be a string"),
    };
    Ok(PhotonRequest {
        question: question.to_string(),
        path,
    })
}

/// Resolves the photon root: `repo_root` by default, otherwise `raw` relative
/// to it. The canonical directory must lie inside the session's read
/// sandbox; symlinks are resolved before the check, and that same canonical
/// path is the one handed to `Workspace::open`.
pub fn resolve_photon_root(raw: Option<&str>, ctx: &ReplContext) -> Result<PathBuf> {
    let repo_root = ctx.effective_repo_root();
    let label = raw.unwrap_or(".");
    let joined = match raw {
        None => repo_root,
        Some(p) if PathBuf::from(p).is_absolute() => PathBuf::from(p),
        Some(p) => repo_root.join(p),
    };
    let canonical = joined
        .canonicalize()
        .with_context(|| format!("photon: cannot resolve path '{label}'"))?;
    if !ctx
        .sandbox_tool_context()
        .is_path_allowed(&canonical, false)
    {
        bail!(
            "photon: '{label}' resolves outside the session's read sandbox \
             (repo root plus any /allow grants); use /allow to grant access first"
        );
    }
    ensure!(canonical.is_dir(), "photon: '{label}' is not a directory");
    check_photon_root_policy(&canonical, ctx, label)?;
    Ok(canonical)
}

/// Applies min-agent's own path policy to the photon root. `Workspace::open`
/// checks only paths below the root it is given, so without this a photon
/// could be rooted at `.git`, `.kube`, `.docker`, `node_modules`, or another
/// excluded directory and read its contents freely. The root is checked
/// relative to the read root that grants it; a `/allow` root is also checked
/// by its own name, since granting Ion a sensitive directory does not mean
/// handing it to a sub-agent that may talk to another vendor.
fn check_photon_root_policy(canonical: &Path, ctx: &ReplContext, label: &str) -> Result<()> {
    let refuse = |error: &dyn std::fmt::Display| {
        anyhow::anyhow!("photon: '{label}' is a path photon may not read ({error})")
    };
    let repo_root = ctx.effective_repo_root().canonicalize().ok();
    let granting = ctx
        .sandbox_tool_context()
        .allowed_read_roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .filter(|root| canonical.starts_with(root))
        .max_by_key(|root| root.components().count())
        .with_context(|| format!("photon: no read root grants '{label}'"))?;
    let below = canonical
        .strip_prefix(&granting)
        .context("photon: read root does not contain the photon root")?;
    if !below.as_os_str().is_empty() {
        let relative = below
            .to_str()
            .with_context(|| format!("photon: '{label}' is not valid UTF-8"))?;
        Workspace::open(&granting)
            .context("photon: cannot open the granting read root")?
            .prepare("list_files", json!({"path": relative, "limit": 1}))
            .map_err(|e| refuse(&e))?;
    }
    if repo_root.as_deref() != Some(granting.as_path()) {
        if let (Some(parent), Some(name)) = (granting.parent(), granting.file_name()) {
            let name = name
                .to_str()
                .with_context(|| format!("photon: '{label}' is not valid UTF-8"))?;
            Workspace::open(parent)
                .context("photon: cannot open the read root's parent")?
                .prepare("list_files", json!({"path": name, "limit": 1}))
                .map_err(|e| refuse(&e))?;
        }
    }
    Ok(())
}

/// Turns a finished run into the parent's tool outcome. Only a completed run
/// is `ok`; any other stop is reported with its partial text labeled as not
/// an answer.
pub fn outcome_from_report(report: &RunReport, endpoint: &ModelEndpoint) -> Result<ToolOutcome> {
    let mut payload =
        serde_json::to_value(report).context("photon: cannot serialize run report")?;
    payload["endpoint"] = json!({
        "protocol": endpoint.protocol.to_string(),
        "model": endpoint.model,
    });
    let reads: Vec<String> = report
        .calls
        .iter()
        .map(|c| match &c.path {
            Some(p) => format!("{} {p}", c.tool),
            None => c.tool.clone(),
        })
        .collect();
    let evidence = format!(
        "[photon ({} via {}): {} rounds, {} tool calls{}]",
        endpoint.model,
        endpoint.protocol,
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
            format!("photon stopped: {}{partial}\n\n{evidence}", report.stop)
        }
    };
    Ok(ToolOutcome {
        rendered,
        payload,
        ok,
    })
}

/// Ion's disposable read-only subagent tool.
pub struct PhotonTool {
    resolver: PhotonEndpointResolver,
    factory: PhotonClientFactory,
    session_limit: usize,
    /// Shared with the worker thread so a run whose client cannot be built
    /// gives its slot back.
    used: Arc<AtomicUsize>,
}

impl PhotonTool {
    /// A photon with an injected endpoint resolver, client factory, and run cap.
    pub fn new(
        resolver: PhotonEndpointResolver,
        factory: PhotonClientFactory,
        session_limit: usize,
    ) -> Self {
        Self {
            resolver,
            factory,
            session_limit,
            used: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// The production photon. The endpoint is resolved per run from the
    /// project's `config.json` (`model_endpoints`) and the environment
    /// (ADR-0022); the credential is read by env-var name when the client is
    /// built, so a missing key fails that run rather than the REPL launch.
    pub fn from_env() -> Self {
        let resolver: PhotonEndpointResolver = Arc::new(|ctx: &ReplContext| {
            let config = load_endpoint_config(&ctx.sandbox_tool_context().impulse_dir)?;
            resolve_photon_endpoint(
                &config,
                &|key| std::env::var(key).ok(),
                &crate::llm_backends::anthropic::anthropic_api_origin(),
            )
        });
        let factory: PhotonClientFactory = Arc::new(|endpoint: &ModelEndpoint| {
            let (connection, profile) = min_agent_bridge::to_min_agent(endpoint)
                .context("photon: invalid model endpoint")?;
            let client = HttpModelClient::new(&connection, &profile)
                .context("photon: cannot build the model client")?;
            Ok(Box::new(client) as Box<dyn ModelClient>)
        });
        Self::new(resolver, factory, PHOTON_SESSION_LIMIT)
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
                    "photon: this session already used its {limit} photon runs; \
                     answer from what you have or use file_read"
                )
            })
    }
}

#[async_trait]
impl ReplTool for PhotonTool {
    fn name(&self) -> &'static str {
        "photon"
    }

    fn usage(&self) -> &'static str {
        "photon {\"question\": \"...\", \"path\": \"...\"} -- ask a disposable read-only \
         subagent one question about the repo (or a subdirectory)"
    }

    fn json_schema(&self) -> Value {
        json!({
            "name": "photon",
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
                        "description": "The single question the photon should answer"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to scope the photon to (defaults to the repo root)"
                    }
                },
                "required": ["question"]
            }
        })
    }

    async fn run(&self, args: Value, ctx: &ReplContext) -> Result<ToolOutcome> {
        let request = parse_photon_args(&args)?;
        let root = resolve_photon_root(request.path.as_deref(), ctx)?;
        let endpoint = (self.resolver)(ctx)?;
        // Refuse a malformed endpoint before it can cost a run slot.
        endpoint
            .validate()
            .context("photon: invalid model endpoint")?;
        self.reserve_slot()?;
        let factory = Arc::clone(&self.factory);
        let used = Arc::clone(&self.used);
        let run_endpoint = endpoint.clone();
        let report = tokio::task::spawn_blocking(move || -> Result<RunReport> {
            // No request has been sent if the client cannot be built (for
            // example a missing credential), so the slot is returned.
            let client = factory(&run_endpoint).inspect_err(|_| {
                used.fetch_sub(1, Ordering::SeqCst);
            })?;
            let workspace = Workspace::open(&root).context("photon: cannot open workspace")?;
            let options = RunOptions {
                text_only: false,
                budget: photon_budget(),
                meta: json!({"host": "ion", "tool": "photon"}),
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
        .context("photon: worker thread failed")??;
        outcome_from_report(&report, &endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::WireProtocol;
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

    /// Always resolves the same Anthropic endpoint; the client is scripted.
    fn fixed_resolver() -> PhotonEndpointResolver {
        Arc::new(|_: &ReplContext| {
            Ok(ModelEndpoint::anthropic_messages(
                "https://api.anthropic.com",
                "test-model",
                256,
            ))
        })
    }

    /// Reads NOTES.md, then answers with the planted fact.
    fn reading_factory() -> PhotonClientFactory {
        Arc::new(|_: &ModelEndpoint| {
            Ok(Box::new(ScriptedClient {
                turns: Mutex::new(VecDeque::from([
                    call_turn("c1", "read_file", json!({"path": "NOTES.md"})),
                    text_turn("The planted fact is 42."),
                ])),
            }) as Box<dyn ModelClient>)
        })
    }

    /// Lists the root forever with fresh call ids, tripping the repeated-batch breaker.
    fn looping_factory() -> PhotonClientFactory {
        Arc::new(|_: &ModelEndpoint| {
            let turns = (0..20)
                .map(|i| call_turn(&format!("c{i}"), "list_files", json!({"path": "."})))
                .collect();
            Ok(Box::new(ScriptedClient {
                turns: Mutex::new(turns),
            }) as Box<dyn ModelClient>)
        })
    }

    /// Records whether it was ever called, then fails.
    fn tripwire_factory(called: Arc<AtomicBool>) -> PhotonClientFactory {
        Arc::new(move |_: &ModelEndpoint| {
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
    async fn test_photon_run_reads_file_and_returns_completed_answer() {
        let (_dir, ctx) = repo_with_notes();
        let tool = PhotonTool::new(fixed_resolver(), reading_factory(), 5);
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
            "the transcript must not leave the photon"
        );
    }

    #[tokio::test]
    async fn test_photon_run_budget_stop_is_not_success() {
        let (_dir, ctx) = repo_with_notes();
        let tool = PhotonTool::new(fixed_resolver(), looping_factory(), 5);
        let outcome = tool
            .run(json!({"question": "Loop forever"}), &ctx)
            .await
            .unwrap();
        assert!(!outcome.ok);
        assert!(outcome.payload["answer"].is_null());
        assert_eq!(outcome.payload["stop"]["kind"], "budget_exceeded");
        assert!(outcome.rendered.starts_with("photon stopped:"));
    }

    #[tokio::test]
    async fn test_photon_run_sandbox_escape_refused_without_consuming_slot() {
        let (_dir, ctx) = repo_with_notes();
        let outside = tempfile::tempdir().unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let tool = PhotonTool::new(fixed_resolver(), tripwire_factory(Arc::clone(&called)), 1);
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
    async fn test_photon_run_allows_granted_read_root() {
        let (_dir, mut ctx) = repo_with_notes();
        let granted = tempfile::tempdir().unwrap();
        std::fs::write(granted.path().join("NOTES.md"), "planted fact: 42\n").unwrap();
        ctx.allowed_read_roots.push(granted.path().to_path_buf());
        let tool = PhotonTool::new(fixed_resolver(), reading_factory(), 5);
        let path = granted.path().display().to_string();
        let outcome = tool
            .run(json!({"question": "fact?", "path": path}), &ctx)
            .await
            .unwrap();
        assert!(outcome.ok);
    }

    #[tokio::test]
    async fn test_photon_run_session_limit_refuses_after_cap() {
        let (_dir, ctx) = repo_with_notes();
        let tool = PhotonTool::new(fixed_resolver(), reading_factory(), 2);
        for _ in 0..2 {
            let outcome = tool.run(json!({"question": "fact?"}), &ctx).await.unwrap();
            assert!(outcome.ok);
        }
        let err = tool
            .run(json!({"question": "fact?"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("already used its 2 photon runs"));
    }

    #[tokio::test]
    async fn test_photon_run_factory_error_propagates_and_returns_the_slot() {
        let (_dir, ctx) = repo_with_notes();
        let called = Arc::new(AtomicBool::new(false));
        let tool = PhotonTool::new(fixed_resolver(), tripwire_factory(Arc::clone(&called)), 1);
        let err = tool
            .run(json!({"question": "fact?"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("tripwire"));
        assert!(called.load(Ordering::SeqCst));
        assert_eq!(tool.used.load(Ordering::SeqCst), 0, "no request was sent");
    }

    /// Review P3: an endpoint that fails validation (here `localhost`, which
    /// is not numeric loopback) used to cost a slot inside the factory.
    #[tokio::test]
    async fn test_photon_run_invalid_endpoint_does_not_consume_slot() {
        let (_dir, ctx) = repo_with_notes();
        let called = Arc::new(AtomicBool::new(false));
        let resolver: PhotonEndpointResolver = Arc::new(|_: &ReplContext| {
            Ok(ModelEndpoint::anthropic_messages(
                "http://localhost:4010",
                "m",
                256,
            ))
        });
        let tool = PhotonTool::new(resolver, tripwire_factory(Arc::clone(&called)), 1);
        for _ in 0..2 {
            let err = tool
                .run(json!({"question": "fact?"}), &ctx)
                .await
                .unwrap_err();
            assert!(format!("{err:#}").contains("must use https"), "{err:#}");
        }
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(tool.used.load(Ordering::SeqCst), 0);
    }

    /// Review P2: min-agent's excluded names apply only below the workspace
    /// root, so the root itself must not be one of them.
    #[test]
    fn test_resolve_photon_root_refuses_excluded_directories() {
        let (dir, ctx) = repo_with_notes();
        std::fs::create_dir_all(dir.path().join(".git").join("refs")).unwrap();
        std::fs::write(dir.path().join(".git").join("config"), "[core]\n").unwrap();
        std::fs::create_dir_all(dir.path().join("sub").join("node_modules")).unwrap();
        for raw in [".git", ".git/refs", "sub/node_modules"] {
            let err = resolve_photon_root(Some(raw), &ctx).unwrap_err();
            assert!(
                format!("{err}").contains("is a path photon may not read"),
                "{raw}: {err}"
            );
        }
        assert!(resolve_photon_root(Some("sub"), &ctx).is_ok());
        assert!(resolve_photon_root(None, &ctx).is_ok());
    }

    #[test]
    fn test_resolve_photon_root_refuses_a_granted_root_with_an_excluded_name() {
        let (_dir, mut ctx) = repo_with_notes();
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        ctx.allowed_read_roots.push(ssh.clone());
        let err = resolve_photon_root(Some(ssh.to_str().unwrap()), &ctx).unwrap_err();
        assert!(
            format!("{err}").contains("is a path photon may not read"),
            "{err}"
        );
    }

    #[test]
    fn test_parse_photon_args_valid_trims_and_defaults_path() {
        let parsed = parse_photon_args(&json!({"question": "  where?  ", "path": " "})).unwrap();
        assert_eq!(
            parsed,
            PhotonRequest {
                question: "where?".into(),
                path: None
            }
        );
        let parsed = parse_photon_args(&json!({"question": "q", "path": "src"})).unwrap();
        assert_eq!(parsed.path.as_deref(), Some("src"));
    }

    #[test]
    fn test_parse_photon_args_rejects_blank_missing_oversize_and_bad_path() {
        assert!(parse_photon_args(&json!({})).is_err());
        assert!(parse_photon_args(&json!({"question": "   "})).is_err());
        assert!(parse_photon_args(&json!({"question": 7})).is_err());
        let big = "x".repeat(PHOTON_MAX_QUESTION_BYTES + 1);
        assert!(parse_photon_args(&json!({ "question": big })).is_err());
        let err = parse_photon_args(&json!({"question": "q", "path": 3})).unwrap_err();
        assert!(format!("{err}").contains("'path' must be a string"));
    }

    #[test]
    fn test_resolve_photon_root_rejects_file_and_missing_paths() {
        let (_dir, ctx) = repo_with_notes();
        let err = resolve_photon_root(Some("NOTES.md"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("is not a directory"));
        let err = resolve_photon_root(Some("missing"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("cannot resolve path 'missing'"));
        let root = resolve_photon_root(Some("sub"), &ctx).unwrap();
        assert!(root.ends_with("sub"));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_photon_root_rejects_symlink_out_of_sandbox() {
        let (dir, ctx) = repo_with_notes();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let err = resolve_photon_root(Some("link"), &ctx).unwrap_err();
        assert!(format!("{err}").contains("outside the session's read sandbox"));
    }

    #[test]
    fn test_photon_budget_fits_inside_parent_loop_and_validates() {
        let budget = photon_budget();
        assert!(budget.validate().is_ok());
        assert!(budget.wall_clock < ION_DEFAULT_WALL_CLOCK);
        assert!(budget.request_timeout <= budget.wall_clock);
    }

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn test_resolve_photon_endpoint_defaults_to_anthropic_with_model_override() {
        let config = ModelEndpointConfig::default();
        let endpoint =
            resolve_photon_endpoint(&config, &env_of(&[]), "https://api.anthropic.com").unwrap();
        assert_eq!(endpoint.protocol, WireProtocol::AnthropicMessages);
        assert_eq!(endpoint.base_url, "https://api.anthropic.com/v1");
        assert_eq!(endpoint.model, PHOTON_DEFAULT_MODEL);
        assert_eq!(endpoint.max_output_tokens, Some(PHOTON_MAX_OUTPUT_TOKENS));
        let endpoint = resolve_photon_endpoint(
            &config,
            &env_of(&[
                (PHOTON_MODEL_ENV, " claude-sonnet-5-5 "),
                (PROVIDER_ENV, "Claude"),
            ]),
            "http://127.0.0.1:4010",
        )
        .unwrap();
        assert_eq!(endpoint.model, "claude-sonnet-5-5");
        assert_eq!(endpoint.base_url, "http://127.0.0.1:4010/v1");
        let endpoint = resolve_photon_endpoint(
            &config,
            &env_of(&[(PHOTON_MODEL_ENV, "  ")]),
            "https://a.test",
        )
        .unwrap();
        assert_eq!(endpoint.model, PHOTON_DEFAULT_MODEL);
    }

    #[test]
    fn test_resolve_photon_endpoint_uses_assigned_profile_over_env() {
        let config: ModelEndpointConfig = serde_json::from_value(json!({
            "profiles": {"local": {
                "protocol": "openai_chat",
                "base_url": "http://127.0.0.1:11434/v1",
                "auth": {"kind": "none"},
                "model": "qwen2.5-coder:7b"
            }},
            "roles": {"photon": "local"}
        }))
        .unwrap();
        let endpoint = resolve_photon_endpoint(
            &config,
            &env_of(&[(PHOTON_MODEL_ENV, "ignored"), (PROVIDER_ENV, "openai")]),
            "https://api.anthropic.com",
        )
        .unwrap();
        assert_eq!(endpoint.protocol, WireProtocol::OpenaiChat);
        assert_eq!(endpoint.model, "qwen2.5-coder:7b");
    }

    #[test]
    fn test_resolve_photon_endpoint_refuses_cross_vendor_default_and_bad_profiles() {
        let config = ModelEndpointConfig::default();
        let err = resolve_photon_endpoint(
            &config,
            &env_of(&[(PROVIDER_ENV, "minimax")]),
            "https://api.anthropic.com",
        )
        .unwrap_err();
        assert!(format!("{err}").contains("IMPULSE_PROVIDER is 'minimax'"));
        let missing: ModelEndpointConfig =
            serde_json::from_value(json!({"roles": {"photon": "ghost"}})).unwrap();
        let err = resolve_photon_endpoint(&missing, &env_of(&[]), "https://api.anthropic.com")
            .unwrap_err();
        assert!(format!("{err}").contains("'ghost'"));
        let insecure: ModelEndpointConfig = serde_json::from_value(json!({
            "profiles": {"p": {
                "protocol": "openai_chat", "base_url": "http://remote.test/v1",
                "auth": {"kind": "none"}, "model": "m"
            }},
            "roles": {"photon": "p"}
        }))
        .unwrap();
        let err = resolve_photon_endpoint(&insecure, &env_of(&[]), "https://api.anthropic.com")
            .unwrap_err();
        assert!(format!("{err:#}").contains("must use https"));
    }

    /// Review P1: a cloned repository's `.impulse/config.json` must not be
    /// able to send the user's key to a host it chose.
    #[test]
    fn test_resolve_photon_endpoint_refuses_a_credential_to_an_untrusted_host() {
        let hostile: ModelEndpointConfig = serde_json::from_value(json!({
            "profiles": {"p": {
                "protocol": "anthropic_messages",
                "base_url": "https://collector.example/v1",
                "auth": {"kind": "header_env", "header": "x-api-key", "env": "ANTHROPIC_API_KEY"},
                "model": "m",
                "max_output_tokens": 256
            }},
            "roles": {"photon": "p"}
        }))
        .unwrap();
        let err = resolve_photon_endpoint(&hostile, &env_of(&[]), "https://api.anthropic.com")
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("collector.example"), "{message}");
        assert!(message.contains(TRUSTED_MODEL_HOSTS_ENV), "{message}");
        let endpoint = resolve_photon_endpoint(
            &hostile,
            &env_of(&[(TRUSTED_MODEL_HOSTS_ENV, "collector.example")]),
            "https://api.anthropic.com",
        )
        .unwrap();
        assert_eq!(endpoint.base_url, "https://collector.example/v1");
    }

    #[test]
    fn test_load_endpoint_config_missing_file_is_default_and_malformed_is_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load_endpoint_config(dir.path()).unwrap(),
            ModelEndpointConfig::default()
        );
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"log_level": "info", "model_endpoints": {"roles": {"photon": "p"}}}"#,
        )
        .unwrap();
        let loaded = load_endpoint_config(dir.path()).unwrap();
        assert_eq!(loaded.roles.photon.as_deref(), Some("p"));
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"model_endpoints": {"rolez": {}}}"#,
        )
        .unwrap();
        let err = load_endpoint_config(dir.path()).unwrap_err();
        assert!(format!("{err}").contains("cannot parse model_endpoints"));
    }

    #[tokio::test]
    async fn test_photon_run_resolver_error_does_not_consume_slot() {
        let (_dir, ctx) = repo_with_notes();
        let resolver: PhotonEndpointResolver =
            Arc::new(|_: &ReplContext| bail!("no endpoint configured"));
        let tool = PhotonTool::new(resolver, reading_factory(), 1);
        let err = tool.run(json!({"question": "q"}), &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("no endpoint configured"));
        assert_eq!(tool.used.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_photon_run_reports_resolved_endpoint() {
        let (_dir, ctx) = repo_with_notes();
        let tool = PhotonTool::new(fixed_resolver(), reading_factory(), 5);
        let outcome = tool.run(json!({"question": "fact?"}), &ctx).await.unwrap();
        assert_eq!(outcome.payload["endpoint"]["model"], "test-model");
        assert_eq!(
            outcome.payload["endpoint"]["protocol"],
            "anthropic_messages"
        );
        assert!(outcome
            .rendered
            .contains("test-model via anthropic_messages"));
    }

    /// Live graduation check against the real provider. Opt-in:
    /// `ANTHROPIC_API_KEY=... cargo test --lib tool_photon -- --ignored`.
    /// Sends only a synthetic temp workspace.
    #[tokio::test]
    #[ignore = "live provider call; requires ANTHROPIC_API_KEY"]
    async fn test_photon_from_env_live_round_trip_finds_planted_fact() {
        let (_dir, ctx) = repo_with_notes();
        let tool = PhotonTool::from_env();
        let outcome = tool
            .run(
                json!({"question": "Read NOTES.md and state the planted fact number."}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(outcome.ok, "photon did not complete: {}", outcome.rendered);
        assert!(outcome.rendered.contains("42"), "{}", outcome.rendered);
    }

    #[test]
    fn test_photon_schema_name_matches_tool_name_and_requires_question() {
        let tool = PhotonTool::new(fixed_resolver(), reading_factory(), 1);
        let schema = tool.json_schema();
        assert_eq!(schema["name"], tool.name());
        assert_eq!(schema["input_schema"]["required"], json!(["question"]));
    }
}
