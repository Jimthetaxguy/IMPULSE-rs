//! Standardized JSON envelope for agent-friendly CLI output (ATCC v1).
//!
//! Every structured response wraps data in an [`Envelope`] with explicit error
//! typing, timing metadata, and a consistent `ok` flag so agents can branch on
//! a single field.

use clap::ValueEnum;
use serde::Serialize;
use std::io::{self, Write};
use std::time::Instant;

// ─── Output format ──────────────────────────────────────────────────────────

/// Global output format, selectable via `--format`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Compact JSON (one object, one line).
    #[default]
    Json,
    /// Human-readable text — pretty-prints data, errors to stderr.
    Text,
    /// Newline-delimited JSON — one envelope per line for streaming.
    Ndjson,
}

// ─── Envelope ───────────────────────────────────────────────────────────────

/// Top-level response wrapper.
///
/// ```json
/// { "ok": true, "command": "session-start", "data": { ... }, "meta": { "took_ms": 12 } }
/// ```
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub ok: bool,
    pub command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<EnvelopeError>,
    pub meta: Meta,
}

/// Structured error payload inside an envelope.
#[derive(Debug, Serialize)]
pub struct EnvelopeError {
    pub kind: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Timing / request metadata.
#[derive(Debug, Serialize)]
pub struct Meta {
    pub took_ms: u128,
    pub version: &'static str,
}

// ─── Builder ────────────────────────────────────────────────────────────────

/// Convenience builder to reduce boilerplate in handlers.
pub struct EnvelopeBuilder {
    command: &'static str,
    start: Instant,
}

impl EnvelopeBuilder {
    pub fn new(command: &'static str) -> Self {
        Self {
            command,
            start: Instant::now(),
        }
    }

    /// Build a success envelope.
    pub fn ok<T: Serialize>(self, data: T) -> Envelope<T> {
        let meta = self.meta();
        Envelope {
            ok: true,
            command: self.command,
            data: Some(data),
            error: None,
            meta,
        }
    }

    /// Build an error envelope.
    pub fn err<T: Serialize>(self, kind: &str, message: &str, retryable: bool) -> Envelope<T> {
        let meta = self.meta();
        Envelope {
            ok: false,
            command: self.command,
            data: None,
            error: Some(EnvelopeError {
                kind: kind.to_string(),
                message: message.to_string(),
                retryable,
                details: None,
            }),
            meta,
        }
    }

    /// Build an error envelope with extra details.
    pub fn err_details<T: Serialize>(
        self,
        kind: &str,
        message: &str,
        retryable: bool,
        details: serde_json::Value,
    ) -> Envelope<T> {
        let meta = self.meta();
        Envelope {
            ok: false,
            command: self.command,
            data: None,
            error: Some(EnvelopeError {
                kind: kind.to_string(),
                message: message.to_string(),
                retryable,
                details: Some(details),
            }),
            meta,
        }
    }

    fn meta(&self) -> Meta {
        Meta {
            took_ms: self.start.elapsed().as_millis(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

// ─── Writer ─────────────────────────────────────────────────────────────────

/// Write an envelope to stdout respecting the chosen format.
pub fn write_envelope<T: Serialize>(format: OutputFormat, envelope: &Envelope<T>) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    match format {
        OutputFormat::Json | OutputFormat::Ndjson => {
            serde_json::to_writer(&mut out, envelope)?;
            writeln!(out)?;
        }
        OutputFormat::Text => {
            // Text mode: for success, pretty-print data; for errors, print to stderr.
            if envelope.ok {
                if let Some(ref data) = envelope.data {
                    serde_json::to_writer_pretty(&mut out, data)?;
                    writeln!(out)?;
                }
            } else if let Some(ref err) = envelope.error {
                eprintln!("Error [{}]: {}", err.kind, err.message);
            }
        }
    }
    out.flush()?;
    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_ok_serializes() {
        let env = EnvelopeBuilder::new("test-cmd").ok(serde_json::json!({"key": "val"}));
        assert!(env.ok);
        assert_eq!(env.command, "test-cmd" as &str);
        assert!(env.data.is_some());
        assert!(env.error.is_none());

        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"version\""));
    }

    #[test]
    fn envelope_err_serializes() {
        let env: Envelope<()> =
            EnvelopeBuilder::new("bad-cmd").err("invalid_input", "bad path", false);
        assert!(!env.ok);
        assert!(env.data.is_none());
        let err = env.error.as_ref().unwrap();
        assert_eq!(err.kind, "invalid_input");
        assert!(!err.retryable);
    }

    #[test]
    fn envelope_err_with_details() {
        let env: Envelope<()> = EnvelopeBuilder::new("x").err_details(
            "validation",
            "bad",
            true,
            serde_json::json!({"field": "name"}),
        );
        let err = env.error.as_ref().unwrap();
        assert!(err.retryable);
        assert!(err.details.is_some());
    }
}
