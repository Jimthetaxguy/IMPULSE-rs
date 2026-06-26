use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::state::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingFailureKind {
    MissingScript,
    SpawnFailed,
    StdinWriteFailed,
    Timeout,
    ProcessFailed,
    InvalidOutput,
    CountMismatch,
    DimMismatch,
}

#[derive(Debug, Clone)]
pub struct EmbeddingError {
    pub kind: EmbeddingFailureKind,
    pub message: String,
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for EmbeddingError {}

/// A text embedding vector produced by an [`EmbeddingProvider`].
pub type Embedding = Vec<f32>;

/// Stable boundary for Impulse's semantic-search embedding backend.
///
/// Retrieval depends on this trait, never on a concrete embedder, so the engine
/// can change — the real `memory-pipeline/retrieval_embed.py` script today; a
/// future in-process Rust embedder or an Ollama HTTP backend tomorrow — without
/// touching retrieval internals. (Interface boundary = control plane.)
pub trait EmbeddingProvider: Send + Sync {
    /// Stable identifier for provenance/audit (e.g. the embedding model name).
    fn model_id(&self) -> &str;

    /// Embed a batch of texts, returning exactly one vector per input.
    /// `timeout_secs` bounds the call and must be > 0 for non-empty input.
    fn embed(
        &self,
        texts: &[String],
        timeout_secs: u64,
    ) -> std::result::Result<Vec<Embedding>, EmbeddingError>;
}

/// Production embedder: shells out to the real Python embedding script
/// (`memory-pipeline/retrieval_embed.py`, resolved via `IMPULSE_EMBED_SCRIPT`
/// or a repo-relative path). This is the shipped backend — not a mock.
pub struct ScriptEmbedder {
    python_cmd: String,
    model: String,
}

impl ScriptEmbedder {
    pub fn new(python_cmd: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            python_cmd: python_cmd.into(),
            model: model.into(),
        }
    }
}

impl EmbeddingProvider for ScriptEmbedder {
    fn model_id(&self) -> &str {
        &self.model
    }

    fn embed(
        &self,
        texts: &[String],
        timeout_secs: u64,
    ) -> std::result::Result<Vec<Embedding>, EmbeddingError> {
        embed_texts_with(&self.python_cmd, &self.model, texts, timeout_secs)
    }
}

/// Build the configured production [`EmbeddingProvider`] from runtime config.
///
/// This is the single place the concrete embedder is chosen; swapping engines
/// (e.g. to a future Ollama or in-process Rust embedder) changes only this
/// function, not retrieval.
pub fn provider_for(config: &Config) -> ScriptEmbedder {
    ScriptEmbedder::new(
        config.retrieval_python_cmd.clone(),
        config.embedding_model.clone(),
    )
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    texts: &'a [String],
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    vectors: Vec<Vec<f32>>,
    dim: usize,
}

fn resolve_script_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("IMPULSE_EMBED_SCRIPT") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    let candidates = [
        PathBuf::from("memory-pipeline/retrieval_embed.py"),
        PathBuf::from("../memory-pipeline/retrieval_embed.py"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

pub fn embed_texts(
    config: &Config,
    texts: &[String],
    timeout_secs: u64,
) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
    // Route the production path through the EmbeddingProvider boundary so the
    // trait is exercised on every embed, not just in tests.
    provider_for(config).embed(texts, timeout_secs)
}

/// Core embedding implementation: spawn the embedding script with the given
/// python command + model. Callers should go through [`embed_texts`] or an
/// [`EmbeddingProvider`] rather than calling this directly.
fn embed_texts_with(
    python_cmd: &str,
    model: &str,
    texts: &[String],
    timeout_secs: u64,
) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if timeout_secs == 0 {
        return Err(EmbeddingError {
            kind: EmbeddingFailureKind::Timeout,
            message: "embedding timeout must be > 0".to_string(),
        });
    }

    let script = resolve_script_path().ok_or_else(|| EmbeddingError {
        kind: EmbeddingFailureKind::MissingScript,
        message:
            "embedding script not found; set IMPULSE_EMBED_SCRIPT or add memory-pipeline/retrieval_embed.py"
                .to_string(),
    })?;

    let request = EmbedRequest { texts, model };
    let payload = serde_json::to_vec(&request).map_err(|e| EmbeddingError {
        kind: EmbeddingFailureKind::InvalidOutput,
        message: format!("failed to serialize embedding request: {}", e),
    })?;

    let mut child = Command::new(python_cmd)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EmbeddingError {
            kind: EmbeddingFailureKind::SpawnFailed,
            message: format!(
                "failed to start embedding subprocess '{}': {}",
                python_cmd, e
            ),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload).map_err(|e| EmbeddingError {
            kind: EmbeddingFailureKind::StdinWriteFailed,
            message: format!(
                "failed to write embedding request to subprocess stdin: {}",
                e
            ),
        })?;
    }

    let timeout = Duration::from_secs(timeout_secs);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(EmbeddingError {
                        kind: EmbeddingFailureKind::Timeout,
                        message: format!(
                            "embedding subprocess timed out after {}s and was terminated",
                            timeout_secs
                        ),
                    });
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                return Err(EmbeddingError {
                    kind: EmbeddingFailureKind::ProcessFailed,
                    message: format!("failed while waiting for embedding subprocess: {}", e),
                });
            }
        }
    }

    let output = child.wait_with_output().map_err(|e| EmbeddingError {
        kind: EmbeddingFailureKind::ProcessFailed,
        message: format!("failed reading embedding subprocess output: {}", e),
    })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(EmbeddingError {
            kind: EmbeddingFailureKind::ProcessFailed,
            message: format!("embedding subprocess failed: {}", err.trim()),
        });
    }

    let response: EmbedResponse =
        serde_json::from_slice(&output.stdout).map_err(|e| EmbeddingError {
            kind: EmbeddingFailureKind::InvalidOutput,
            message: format!("failed to parse embedding subprocess JSON output: {}", e),
        })?;

    if response.vectors.len() != texts.len() {
        return Err(EmbeddingError {
            kind: EmbeddingFailureKind::CountMismatch,
            message: format!(
                "embedding output count mismatch: expected {}, got {}",
                texts.len(),
                response.vectors.len()
            ),
        });
    }

    if response.dim == 0 {
        return Err(EmbeddingError {
            kind: EmbeddingFailureKind::DimMismatch,
            message: "embedding output dim is zero".to_string(),
        });
    }

    for vec in &response.vectors {
        if vec.len() != response.dim {
            return Err(EmbeddingError {
                kind: EmbeddingFailureKind::DimMismatch,
                message: "embedding vector dimension mismatch".to_string(),
            });
        }
    }

    Ok(response.vectors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Config;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    /// Deterministic, hermetic embedder for exercising the [`EmbeddingProvider`]
    /// contract offline (test-only — the real backend is [`ScriptEmbedder`]).
    struct FakeEmbedder {
        dims: usize,
        model: String,
    }

    impl EmbeddingProvider for FakeEmbedder {
        fn model_id(&self) -> &str {
            &self.model
        }

        fn embed(
            &self,
            texts: &[String],
            _timeout_secs: u64,
        ) -> std::result::Result<Vec<Embedding>, EmbeddingError> {
            if texts.is_empty() {
                return Ok(Vec::new());
            }
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0f32; self.dims];
                    for (i, b) in t.bytes().enumerate() {
                        v[i % self.dims] += b as f32;
                    }
                    v
                })
                .collect())
        }
    }

    /// Behavioral contract every `EmbeddingProvider` must satisfy.
    fn assert_provider_contract<P: EmbeddingProvider>(provider: &P) {
        // Empty input yields empty output (and must not spawn anything).
        assert!(
            provider.embed(&[], 5).unwrap().is_empty(),
            "empty input must yield empty output"
        );

        let inputs = vec!["a".to_string(), "bb".to_string(), "ccc".to_string()];
        let out = provider.embed(&inputs, 5).unwrap();
        assert_eq!(out.len(), inputs.len(), "one vector per input");

        let dim = out[0].len();
        assert!(dim > 0, "vectors must be non-empty");
        assert!(
            out.iter().all(|v| v.len() == dim),
            "all vectors share one dimension"
        );

        // Deterministic for the same inputs.
        let again = provider.embed(&inputs, 5).unwrap();
        assert_eq!(out, again, "embedding must be deterministic");

        assert!(!provider.model_id().is_empty(), "model_id must be set");
    }

    #[test]
    fn test_fake_embedder_satisfies_provider_contract() {
        assert_provider_contract(&FakeEmbedder {
            dims: 8,
            model: "fake:hash".to_string(),
        });
    }

    #[test]
    fn test_script_embedder_empty_input_short_circuits() {
        // Empty input must return Ok(empty) without ever spawning the script,
        // so this is hermetic even where no python/script exists.
        let provider = ScriptEmbedder::new("python3", "test-model");
        assert!(provider.embed(&[], 5).unwrap().is_empty());
        assert_eq!(provider.model_id(), "test-model");
    }

    #[test]
    fn test_provider_for_uses_configured_model() {
        let config = Config::default();
        let provider = provider_for(&config);
        assert_eq!(
            provider.model_id(),
            config.embedding_model,
            "provider_for must carry the configured embedding model"
        );
    }

    #[test]
    fn test_embed_texts_times_out_and_kills_process() {
        let _guard = env_lock().lock().unwrap();
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }

        let temp = TempDir::new().unwrap();
        let script_path = temp.path().join("slow_embed.py");
        fs::write(
            &script_path,
            r#"#!/usr/bin/env python3
import json
import time
import sys

_ = sys.stdin.read()
time.sleep(2)
print(json.dumps({"vectors": [[1.0]], "dim": 1}))
"#,
        )
        .unwrap();

        std::env::set_var("IMPULSE_EMBED_SCRIPT", &script_path);
        let cfg = Config::default();
        let result = embed_texts(&cfg, &[String::from("timeout-case")], 1);
        std::env::remove_var("IMPULSE_EMBED_SCRIPT");

        assert!(result.is_err());
        let err = result.err().unwrap();
        assert_eq!(err.kind, EmbeddingFailureKind::Timeout);
        assert!(
            err.message.contains("timed out"),
            "expected timeout message, got: {}",
            err.message
        );
    }
}
