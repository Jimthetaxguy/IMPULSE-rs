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

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{HarnessRequest, HarnessResponse};

/// Ceiling on physical lines accumulated while hunting for a parseable JSON
/// response. Bounds the lenient-parse loop against a hung or garbage-emitting
/// child process instead of buffering forever.
const MAX_RESPONSE_LINES: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("failed to launch gate process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("failed to serialize HarnessRequest: {0}")]
    Serialize(serde_json::Error),
    #[error("gate process produced no parseable HarnessResponse within {0} lines")]
    UnparseableResponse(usize),
    #[error("gate process exited with status {0}")]
    NonZeroExit(i32),
}

/// Locates the promoted Pi gate launcher shipped alongside the Ion harness specs.
/// Defaults to `~/.ai-memory/docs/ion-harness/pi-gate/launch-gate.sh` — the single
/// launcher shared by both Ion harnesses (spec-b: "one gate implementation, two
/// harnesses, zero drift"). Override via `PiAdapter::with_launch_script` for tests.
pub struct PiAdapter {
    launch_script: PathBuf,
}

impl PiAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            launch_script: home.join(".ai-memory/docs/ion-harness/pi-gate/launch-gate.sh"),
        }
    }

    pub fn with_launch_script(launch_script: PathBuf) -> Self {
        Self { launch_script }
    }

    /// Drive one verify round-trip against the live gate process. This spawns a
    /// real child process (bash -> node22 -> pi --mode rpc) and requires the
    /// MiniMax key + Node 22 to be present on the host, matching launch-gate.sh's
    /// own preconditions.
    pub fn verify(&self, request: &HarnessRequest) -> Result<HarnessResponse, AdapterError> {
        let payload = serde_json::to_string(request).map_err(AdapterError::Serialize)?;

        let mut child = Command::new("bash")
            .arg(&self.launch_script)
            .arg("--mode")
            .arg("rpc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        {
            let stdin = child.stdin.as_mut().expect("stdin was piped");
            stdin.write_all(payload.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
        // Dropping the handle (via scope end) closes stdin so a stateless
        // (`--no-session`) gate process sees EOF and can finish its one turn.
        child.stdin = None;

        let stdout = child.stdout.take().expect("stdout was piped");
        let response = read_response_lenient(std::io::BufReader::new(stdout))?;

        let status = child.wait()?;
        if !status.success() {
            return Err(AdapterError::NonZeroExit(status.code().unwrap_or(-1)));
        }

        Ok(response)
    }
}

impl Default for PiAdapter {
    fn default() -> Self {
        Self::new()
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
        let adapter = PiAdapter::new();
        assert!(adapter
            .launch_script
            .ends_with(".ai-memory/docs/ion-harness/pi-gate/launch-gate.sh"));
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
        let request = HarnessRequest {
            contract_version: crate::CONTRACT_VERSION.to_string(),
            request_id: "req-smoke-test".to_string(),
            intent: crate::Intent::Verify,
            repo: crate::RepoRef {
                path: env!("CARGO_MANIFEST_DIR").to_string(),
                diff_ref: Some("HEAD~1..HEAD".to_string()),
                diff_inline: None,
            },
            task: crate::Task {
                description: "Smoke-test the Ion Pi adapter round trip.".to_string(),
                verdict_priority: vec!["correctness".into()],
            },
            capability_allowlist: vec!["read", "grep", "find", "ls", "build", "test"]
                .into_iter()
                .map(String::from)
                .collect(),
            model_role: "verifier-cheap".to_string(),
            context: crate::Context {
                read_only: true,
                payload: vec![],
            },
        };
        let response = adapter.verify(&request).expect("live gate round trip");
        assert_eq!(response.request_id, "req-smoke-test");
    }
}
