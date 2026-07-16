//! Load ElevenLabs credentials from process env or Infisical CLI.
//!
//! Never logs secret values. Infisical secret name in the James vault is
//! `ElevenLabs_API_Key` (project at `~/code/.infisical.json`); we also accept
//! the conventional `ELEVENLABS_API_KEY` env var.

use std::process::Command;

/// Resolved API key source (for status output — not the key itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    Env,
    Infisical { secret_name: String },
    Missing,
}

/// Load ElevenLabs API key without printing it.
///
/// Order:
/// 1. `ELEVENLABS_API_KEY` env
/// 2. Infisical `secrets get ElevenLabs_API_Key --env=dev --plain` (cwd walk
///    for `.infisical.json` handled by the CLI)
pub fn load_elevenlabs_api_key() -> (Option<String>, SecretSource) {
    if let Ok(key) = std::env::var("ELEVENLABS_API_KEY") {
        let trimmed = key.trim().to_string();
        if !trimmed.is_empty() {
            return (Some(trimmed), SecretSource::Env);
        }
    }

    // Prefer exact vault name used on this machine.
    for secret_name in ["ElevenLabs_API_Key", "ELEVENLABS_API_KEY"] {
        if let Some(key) = infisical_get_plain(secret_name) {
            return (
                Some(key),
                SecretSource::Infisical {
                    secret_name: secret_name.to_string(),
                },
            );
        }
    }

    (None, SecretSource::Missing)
}

fn infisical_get_plain(secret_name: &str) -> Option<String> {
    // Run from home code root when present so project mapping resolves.
    let code_root = dirs::home_dir().map(|h| h.join("code"));
    let mut cmd = Command::new("infisical");
    cmd.args([
        "secrets",
        "get",
        secret_name,
        "--env=dev",
        "--plain",
        "--silent",
    ]);
    if let Some(root) = code_root.as_ref().filter(|p| p.join(".infisical.json").is_file()) {
        cmd.current_dir(root);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Inject key into process env for child tools (ElevenLabs CLI, reqwest clients).
/// Returns whether a key was applied and its source label.
pub fn ensure_elevenlabs_env() -> (bool, SecretSource) {
    let (key, source) = load_elevenlabs_api_key();
    match key {
        Some(k) => {
            // Only set if not already present, so explicit env wins.
            if std::env::var_os("ELEVENLABS_API_KEY").is_none() {
                // SAFETY: single-threaded init path for CLI; value is not logged.
                unsafe { std::env::set_var("ELEVENLABS_API_KEY", k) };
            }
            (true, source)
        }
        None => (false, source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_when_unset_and_no_infisical_output() {
        // Does not assert Infisical offline — only that the API does not panic.
        let (_key, source) = load_elevenlabs_api_key();
        let _ = source;
    }
}
