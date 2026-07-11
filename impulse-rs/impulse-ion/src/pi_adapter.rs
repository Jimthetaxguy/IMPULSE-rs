//! Rust-side Ion adapter for harness #2 (TS Pi on MiniMax), promoted per
//! `~/.ai-memory/docs/ion-harness/spec-b-pi-gate-bringup.md` (status: B5 COMPLETE).
//!
//! This adapter does not re-implement Pi bring-up — it drives the already-promoted
//! `launch-gate.sh` entrypoint in `--mode rpc` and speaks the spec-a
//! `HarnessRequest`/`HarnessResponse` contract over its stdin/stdout JSONL framing.
//!
//! Pi has no `--json-schema` enforcement (spec-b status note 3), so a verdict can
//! arrive with raw, unescaped newlines inside a JSON string value. [`read_response_lenient`]
//! handles that by accumulating physical lines until one parses as valid JSON, rather
//! than trusting the first line to be a complete, valid JSON document.

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::{HarnessRequest, HarnessResponse};

/// Ceiling on physical lines accumulated while hunting for a parseable JSON
/// response. Bounds the lenient-parse loop against a hung or garbage-emitting
/// child process instead of buffering forever.
const MAX_RESPONSE_LINES: usize = 500;

/// Environment variable that overrides the launcher path at runtime, per the
/// repo's real-systems rule ("paths come from config/env, not hardcoded").
/// See [`PiAdapter::new`] for the full override precedence.
pub const ION_GATE_LAUNCHER_ENV: &str = "ION_GATE_LAUNCHER";

/// Default ceiling on how long a single gate round trip may run before the
/// child is killed. Generous enough for a real MiniMax verify call (model
/// latency + Node 22 startup) while still bounding a hung child. Override via
/// [`PiAdapter::with_timeout`].
pub const DEFAULT_GATE_TIMEOUT: Duration = Duration::from_secs(300);

/// How often the timeout watchdog polls the child's exit status while
/// waiting. Small enough to keep timeout tests fast, large enough to avoid
/// busy-looping.
const POLL_INTERVAL: Duration = Duration::from_millis(15);

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("failed to launch gate process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("failed to serialize HarnessRequest: {0}")]
    Serialize(serde_json::Error),
    #[error("gate process produced no parseable HarnessResponse within {0} lines")]
    UnparseableResponse(usize),
    #[error("gate process exited with status {code} (stderr: {stderr})")]
    NonZeroExit { code: i32, stderr: String },
    #[error(
        "gate process timed out after {timeout_secs}s and was killed (stderr so far: {stderr})"
    )]
    TimedOut { timeout_secs: u64, stderr: String },
}

/// Locates the promoted Pi gate launcher shipped alongside the Ion harness specs.
///
/// Launcher path resolution precedence (highest to lowest):
/// 1. Explicit constructor argument — [`PiAdapter::with_launch_script`] (test override).
/// 2. `ION_GATE_LAUNCHER` environment variable — [`ION_GATE_LAUNCHER_ENV`], per the
///    real-systems rule that paths come from config/env, not hardcoded constants.
/// 3. Default — `~/.ai-memory/docs/ion-harness/pi-gate/launch-gate.sh`, the single
///    launcher shared by both Ion harnesses (spec-b: "one gate implementation, two
///    harnesses, zero drift").
pub struct PiAdapter {
    launch_script: PathBuf,
    timeout: Duration,
}

impl PiAdapter {
    /// Resolves the launcher path via env var (`ION_GATE_LAUNCHER`) or the
    /// default path under `~/.ai-memory`. See the type-level doc comment for
    /// full precedence (explicit arg via `with_launch_script` wins over both).
    pub fn new() -> Self {
        let launch_script = std::env::var(ION_GATE_LAUNCHER_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
                home.join(".ai-memory/docs/ion-harness/pi-gate/launch-gate.sh")
            });
        Self {
            launch_script,
            timeout: DEFAULT_GATE_TIMEOUT,
        }
    }

    /// Explicit launcher override (highest precedence — beats the env var and
    /// the default path). Primarily for tests driving a stub gate script.
    pub fn with_launch_script(launch_script: PathBuf) -> Self {
        Self {
            launch_script,
            timeout: DEFAULT_GATE_TIMEOUT,
        }
    }

    /// Overrides the child-process timeout (default [`DEFAULT_GATE_TIMEOUT`]).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Drive one verify round-trip against the live gate process. This spawns a
    /// real child process (bash -> node22 -> pi --mode rpc) and requires the
    /// MiniMax key + Node 22 to be present on the host, matching launch-gate.sh's
    /// own preconditions.
    ///
    /// The child's stderr is piped (never inherited — inherited stderr would
    /// corrupt a future TUI's screen) and surfaced via `tracing::warn!` when
    /// non-empty, plus folded into `NonZeroExit`/`TimedOut` errors so it is
    /// never silently dropped. The whole round trip is bounded by `self.timeout`;
    /// a child that never produces output is killed and reported as `TimedOut`
    /// instead of hanging the caller forever.
    pub fn verify(&self, request: &HarnessRequest) -> Result<HarnessResponse, AdapterError> {
        let payload = serde_json::to_string(request).map_err(AdapterError::Serialize)?;

        let mut command = Command::new("bash");
        command
            .arg(&self.launch_script)
            .arg("--mode")
            .arg("rpc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // `launch-gate.sh` forks descendants (node, the model process); on a
        // timeout we must kill the whole process group, not just the direct
        // `bash` child, or an orphaned grandchild keeps the stdout/stderr
        // pipes open and `read_line` never sees EOF. Put the child in its own
        // group (pgid == its own pid) so `kill_process_group` below can
        // target it precisely.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;

        {
            let stdin = child.stdin.as_mut().expect("stdin was piped");
            stdin.write_all(payload.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
        // Dropping the handle (via scope end) closes stdin so a stateless
        // (`--no-session`) gate process sees EOF and can finish its one turn.
        child.stdin = None;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        // Drain stderr on a background thread so a chatty child can't fill
        // the pipe buffer and deadlock against the stdout reader below.
        let stderr_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            let mut stderr = stderr;
            let _ = stderr.read_to_string(&mut buf);
            buf
        });

        // Read the response on a background thread so the main thread is free
        // to poll for a timeout instead of blocking forever on `read_line`.
        let (response_tx, response_rx) = mpsc::channel();
        let response_handle = std::thread::spawn(move || {
            let result = read_response_lenient(std::io::BufReader::new(stdout));
            let _ = response_tx.send(());
            result
        });

        let start = Instant::now();
        let mut timed_out = false;
        let mut response_ready = false;
        loop {
            if let Ok(Some(_status)) = child.try_wait() {
                break;
            }
            if start.elapsed() >= self.timeout {
                timed_out = true;
                kill_process_group(&mut child);
                let _ = child.wait();
                break;
            }
            if response_ready {
                // Response bytes are already parsed; keep polling try_wait
                // (still bounded by the timeout above) without re-touching a
                // closed channel.
                std::thread::sleep(POLL_INTERVAL);
            } else if response_rx.recv_timeout(POLL_INTERVAL).is_ok() {
                response_ready = true;
            }
        }

        let response_result = response_handle
            .join()
            .unwrap_or(Err(AdapterError::UnparseableResponse(MAX_RESPONSE_LINES)));
        let stderr_output = stderr_handle.join().unwrap_or_default();

        if !stderr_output.trim().is_empty() {
            tracing::warn!(stderr = %stderr_output, "pi gate child process wrote to stderr");
        }

        if timed_out {
            return Err(AdapterError::TimedOut {
                timeout_secs: self.timeout.as_secs(),
                stderr: stderr_output,
            });
        }

        let status = child.wait()?;
        if !status.success() {
            return Err(AdapterError::NonZeroExit {
                code: status.code().unwrap_or(-1),
                stderr: stderr_output,
            });
        }

        response_result
    }
}

impl Default for PiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Kill `child` and, on unix, its entire process group (see the
/// `process_group(0)` note at spawn time) so an orphaned grandchild (e.g. a
/// `node` process forked by `launch-gate.sh`) can't keep the stdout/stderr
/// pipes open past the timeout. Best-effort: a kill that races the child's
/// own natural exit is not an error.
fn kill_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // `child.id()` is also the process group id because the child was
        // spawned with `process_group(0)`. Shell out to `kill` rather than
        // pulling in `libc` just for one syscall.
        let pgid = child.id();
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{pgid}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    {
        // No process-group kill primitive without libc on non-unix targets;
        // fall back to killing the direct child only (may still leave
        // orphaned grandchildren holding the pipes open).
        let _ = child.kill();
    }
}

/// Accumulate physical lines from `reader` until the buffer parses as a valid
/// `HarnessResponse`, or `MAX_RESPONSE_LINES` is exceeded. Handles two distinct
/// ways Pi's lack of constrained decoding can fragment the JSON across reads:
/// the document itself spanning multiple physical lines (harmless — whitespace
/// between tokens), and a raw, unescaped control character left inside a
/// `output_logs` string value (invalid per RFC 8259 — `sanitize_control_chars_in_strings`
/// repairs it before the retry).
fn read_response_lenient(mut reader: impl BufRead) -> Result<HarnessResponse, AdapterError> {
    let mut buffer = String::new();
    for _ in 0..MAX_RESPONSE_LINES {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break; // EOF
        }
        buffer.push_str(&line);
        if let Ok(response) = serde_json::from_str::<HarnessResponse>(&buffer) {
            return Ok(response);
        }
        let sanitized = sanitize_control_chars_in_strings(&buffer);
        if let Ok(response) = serde_json::from_str::<HarnessResponse>(&sanitized) {
            return Ok(response);
        }
    }
    Err(AdapterError::UnparseableResponse(MAX_RESPONSE_LINES))
}

/// Escape raw newline/carriage-return bytes that appear *inside* a JSON string,
/// leaving whitespace outside strings untouched. A single left-to-right pass
/// tracking string/escape state, since JSON strings never nest.
fn sanitize_control_chars_in_strings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in input.chars() {
        if in_string {
            if escaped {
                out.push(c);
                escaped = false;
            } else if c == '\\' {
                out.push(c);
                escaped = true;
            } else if c == '"' {
                out.push(c);
                in_string = false;
            } else if c == '\n' {
                out.push_str("\\n");
            } else if c == '\r' {
                out.push_str("\\r");
            } else {
                out.push(c);
            }
        } else if c == '"' {
            out.push(c);
            in_string = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_response_lenient_parses_single_line_response() {
        let json = r#"{"contract_version":"0","request_id":"r1","verdict":"APPROVE","findings":[],"commands_run":[{"command":"cargo test","exit_code":0,"output_ref":"log-1"}],"output_logs":{},"metrics":{"tokens_in":0,"tokens_out":0,"latency_ms":0}}"#;
        let cursor = Cursor::new(format!("{json}\n"));
        let response = read_response_lenient(cursor).expect("should parse");
        assert!(response.passed());
    }

    #[test]
    fn read_response_lenient_recovers_from_raw_embedded_newline() {
        // A model with no constrained decoding can leave a literal newline inside
        // a JSON string value (here, inside output_logs["log-1"]). A naive
        // single-read_line parse would see line 1 alone and fail; this must
        // accumulate through line 2 and succeed.
        let raw = "{\"contract_version\":\"0\",\"request_id\":\"r1\",\"verdict\":\"APPROVE\",\"findings\":[],\"commands_run\":[{\"command\":\"cargo test\",\"exit_code\":0,\"output_ref\":\"log-1\"}],\"output_logs\":{\"log-1\":\"line one\nline two\"},\"metrics\":{\"tokens_in\":0,\"tokens_out\":0,\"latency_ms\":0}}\n";
        let cursor = Cursor::new(raw.to_string());
        let response =
            read_response_lenient(cursor).expect("should recover across the embedded newline");
        assert_eq!(
            response.output_logs.get("log-1").map(String::as_str),
            Some("line one\nline two")
        );
    }

    #[test]
    fn sanitize_control_chars_in_strings_escapes_only_inside_strings() {
        let input = "{\"a\":\"line one\nline two\"}\n{\"b\":1}";
        let sanitized = sanitize_control_chars_in_strings(input);
        assert_eq!(sanitized, "{\"a\":\"line one\\nline two\"}\n{\"b\":1}");
    }

    #[test]
    fn sanitize_control_chars_in_strings_respects_escaped_quotes() {
        // The escaped quote inside the string must not be mistaken for the
        // closing quote, or the newline right after it would be treated as
        // outside a string and left unescaped.
        let input = "{\"a\":\"she said \\\"hi\\\"\nthen left\"}";
        let sanitized = sanitize_control_chars_in_strings(input);
        assert!(sanitized.contains("hi\\\"\\nthen left"));
    }

    #[test]
    fn read_response_lenient_gives_up_on_pure_garbage() {
        let cursor = Cursor::new("not json\nstill not json\n".to_string());
        let err = read_response_lenient(cursor).unwrap_err();
        assert!(matches!(err, AdapterError::UnparseableResponse(_)));
    }

    #[test]
    fn default_launch_script_points_at_promoted_pi_gate() {
        let _guard = env_lock();
        std::env::remove_var(ION_GATE_LAUNCHER_ENV);
        let adapter = PiAdapter::new();
        assert!(adapter
            .launch_script
            .ends_with(".ai-memory/docs/ion-harness/pi-gate/launch-gate.sh"));
    }

    /// Serializes tests that mutate the process-global `ION_GATE_LAUNCHER`
    /// env var, since `cargo test` runs unit tests in the same process on
    /// multiple threads by default.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn new_respects_ion_gate_launcher_env_override() {
        let _guard = env_lock();
        std::env::set_var(ION_GATE_LAUNCHER_ENV, "/tmp/custom-ion-gate.sh");
        let adapter = PiAdapter::new();
        std::env::remove_var(ION_GATE_LAUNCHER_ENV);
        assert_eq!(
            adapter.launch_script,
            PathBuf::from("/tmp/custom-ion-gate.sh")
        );
    }

    #[test]
    fn with_launch_script_beats_env_override() {
        // Explicit constructor arg is documented as the top-precedence
        // override (beats the env var) — verify that holds even when the
        // env var is set to a different path.
        let _guard = env_lock();
        std::env::set_var(ION_GATE_LAUNCHER_ENV, "/tmp/should-be-ignored.sh");
        let adapter = PiAdapter::with_launch_script(PathBuf::from("/tmp/explicit-gate.sh"));
        std::env::remove_var(ION_GATE_LAUNCHER_ENV);
        assert_eq!(
            adapter.launch_script,
            PathBuf::from("/tmp/explicit-gate.sh")
        );
    }

    #[test]
    fn adapter_error_timed_out_display_includes_seconds_and_stderr() {
        let err = AdapterError::TimedOut {
            timeout_secs: 5,
            stderr: "gate appears wedged".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("5"));
        assert!(msg.contains("timed out"));
        assert!(msg.contains("gate appears wedged"));
    }

    #[test]
    fn adapter_error_non_zero_exit_display_includes_code_and_stderr() {
        let err = AdapterError::NonZeroExit {
            code: 7,
            stderr: "boom".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains('7'));
        assert!(msg.contains("boom"));
    }

    fn sample_request() -> HarnessRequest {
        HarnessRequest::verify(
            env!("CARGO_MANIFEST_DIR"),
            "HEAD~1..HEAD",
            "Smoke-test the Ion Pi adapter round trip.",
        )
    }

    /// Fake-gate test (T2/G3): a child that never writes output and never
    /// exits must not hang `verify` forever. `hang-gate.sh` under
    /// `tests/fakes/` sleeps well past the configured timeout; `verify` must
    /// kill it and return `TimedOut` promptly.
    #[test]
    fn verify_kills_hung_gate_and_returns_timed_out() {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fakes/hang-gate.sh");
        let adapter =
            PiAdapter::with_launch_script(script).with_timeout(Duration::from_millis(200));
        let request = sample_request();

        let start = Instant::now();
        let err = adapter
            .verify(&request)
            .expect_err("hung gate must time out");
        let elapsed = start.elapsed();

        assert!(matches!(err, AdapterError::TimedOut { .. }));
        assert!(
            elapsed < Duration::from_secs(5),
            "verify should return promptly after the configured timeout, took {elapsed:?}"
        );
    }

    /// Live smoke test against the real, already-promoted Pi gate process.
    /// Ignored by default: requires Node 22, the MiniMax key in
    /// ~/.local/share/opencode/auth.json, and network access — not something an
    /// ambient CI/loop run should depend on every pass. Run explicitly with
    /// `cargo test -p impulse-ion -- --ignored live_gate_round_trip`.
    #[test]
    #[ignore]
    fn live_gate_round_trip() {
        let adapter = PiAdapter::new();
        let request = sample_request();
        let sent_request_id = request.request_id.clone();
        let response = adapter.verify(&request).expect("live gate round trip");
        assert_eq!(response.request_id, sent_request_id);
    }
}
