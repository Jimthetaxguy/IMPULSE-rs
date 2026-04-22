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

    let request = EmbedRequest {
        texts,
        model: &config.embedding_model,
    };
    let payload = serde_json::to_vec(&request).map_err(|e| EmbeddingError {
        kind: EmbeddingFailureKind::InvalidOutput,
        message: format!("failed to serialize embedding request: {}", e),
    })?;

    let mut child = Command::new(&config.retrieval_python_cmd)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EmbeddingError {
            kind: EmbeddingFailureKind::SpawnFailed,
            message: format!(
                "failed to start embedding subprocess '{}': {}",
                config.retrieval_python_cmd, e
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
