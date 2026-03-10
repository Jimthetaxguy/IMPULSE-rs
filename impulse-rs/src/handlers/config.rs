use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::{branding, credentials, memory, orchestration, state};

use super::{
    build_tool_registry, print_config, refresh_capabilities_manifest, tool_resolution_root,
};

pub fn handle_config(
    state: &Arc<state::State>,
    key: Option<String>,
    value: Option<String>,
    list: bool,
) -> Result<()> {
    if list {
        let config = state.list_config()?;
        print_config(config);
    } else if let Some(key) = key {
        if let Some(value) = value {
            match state.set_config(&key, &value) {
                Ok(true) => println!("Set {} = {}", key, value),
                Ok(false) => {
                    eprintln!("Error: Invalid value '{}' for key '{}'", value, key)
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        } else {
            match state.get_config(&key)? {
                Some(v) => println!("{} = {}", key, v),
                None => println!("Unknown config key: {}", key),
            }
        }
    } else {
        let config = state.list_config()?;
        print_config(config);
        println!("\nUse 'config <key>' to get a value");
        println!("Use 'config <key> --value <value>' to set a value");
        println!("Use 'config --list' to list all values");
    }
    Ok(())
}

pub fn handle_init(state: &Arc<state::State>, impulse_dir: &Path) -> Result<()> {
    state
        .storage()
        .write_json("LIVE_STATE.json", &state::LiveState::new())?;
    state
        .storage()
        .write_json("GENOME.md", &memory::Genome::new())?;
    state
        .storage()
        .write_json("config.json", &state::Config::default())?;
    let _ = orchestration::ensure_context_dirs(state.storage().base_path())?;
    let config = state.config_snapshot()?;
    let external_tools_dir =
        config.resolved_external_tools_dir_from(tool_resolution_root(impulse_dir));
    std::fs::create_dir_all(&external_tools_dir)?;
    let registry = build_tool_registry(impulse_dir, &config)?;
    let manifest_path = refresh_capabilities_manifest(state.storage().base_path(), &registry)?;
    branding::print_banner();
    println!("Initialized at {:?}", state.storage().base_path());
    println!("External tools dir: {}", external_tools_dir.display());
    println!("Capabilities manifest: {}", manifest_path.display());
    Ok(())
}

pub async fn handle_status(state: &Arc<state::State>) -> Result<()> {
    branding::print_banner();
    let sessions = state.list_sessions().await?;
    println!("Active sessions: {}", sessions.len());
    for s in &sessions {
        println!("  - {} ({}) [{:?}]", s.name, s.id, s.status);
    }
    Ok(())
}

pub fn handle_list_providers() -> Result<()> {
    use crate::agent::{AnthropicProvider, LlmProvider, MinimaxProvider, OpenAiProvider};

    println!("Available LLM Providers:\n");

    let anthropic = AnthropicProvider::new(String::new());
    let openai = OpenAiProvider::new(String::new());
    let minimax = MinimaxProvider::new(String::new());

    println!(
        "  {} (default: {})\n    Models: {}",
        anthropic.name(),
        anthropic.default_model(),
        anthropic.supported_models().join(", ")
    );
    println!(
        "\n  {} (default: {})\n    Models: {}",
        openai.name(),
        openai.default_model(),
        openai.supported_models().join(", ")
    );
    println!(
        "\n  {} (default: {})\n    Models: {}",
        minimax.name(),
        minimax.default_model(),
        minimax.supported_models().join(", ")
    );
    Ok(())
}

pub fn handle_model(
    state: &Arc<state::State>,
    _impulse_dir: &Path,
    verbose: bool,
    subcommand: String,
    provider: Option<String>,
    model: Option<String>,
) -> Result<()> {
    use crate::docs::cache;
    use crate::docs::models as model_mgr;

    match subcommand.as_str() {
        "list" | "ls" => {
            let cache = cache::create_cache(state.storage().base_path())?;
            let models = cache.load_models().unwrap_or_else(|_| Vec::new());

            let filter = model_mgr::ModelFilter {
                provider: provider.clone(),
                ..Default::default()
            };

            let filtered = filter.apply(&models);
            println!("{}", model_mgr::format_models(&filtered, verbose));
        }
        "set" => {
            let provider = provider.ok_or_else(|| anyhow::anyhow!("--provider required"))?;
            let model = model.ok_or_else(|| anyhow::anyhow!("--model required"))?;
            state.set_config(&format!("model.{}", provider), &model)?;
            println!("Set default model for {} to {}", provider, model);
        }
        "get" => {
            let provider = provider.ok_or_else(|| anyhow::anyhow!("--provider required"))?;
            let model = state.get_config(&format!("model.{}", provider))?;
            if let Some(m) = model {
                println!("{}: {}", provider, m);
            } else {
                let defaults = model_mgr::ModelConfig::default();
                if let Some(def) = defaults.get_default_model(&provider) {
                    println!("{} (default): {}", provider, def);
                } else {
                    println!("No model configured for {}", provider);
                }
            }
        }
        _ => {
            eprintln!(
                "Unknown model subcommand: {}. Use: list, set, get",
                subcommand
            );
        }
    }
    Ok(())
}

pub fn handle_credentials(
    subcommand: String,
    provider: Option<String>,
    key: Option<String>,
    value: Option<String>,
    socket_path: Option<String>,
    tool: Option<String>,
) -> Result<()> {
    use credentials::{create_provider, CredentialConfig, CredentialProviderType};

    let provider_type = provider
        .as_ref()
        .and_then(|p| CredentialProviderType::parse(p))
        .unwrap_or(CredentialProviderType::Env);

    let config = CredentialConfig {
        provider: provider_type,
        cli_tool: tool.clone(),
        socket_path: socket_path.clone(),
        provider_url: None,
    };

    let cred_provider = create_provider(&config);

    match subcommand.as_str() {
        "list" | "ls" => {
            let status = cred_provider.status();
            println!(
                "Provider: {} (available: {})",
                status.provider, status.available
            );

            match cred_provider.list() {
                Ok(secrets) => {
                    if secrets.is_empty() {
                        println!("No secrets stored.");
                    } else {
                        println!("\nStored secrets:");
                        for secret in secrets {
                            println!("  - {}", secret.key);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error listing secrets: {}", e);
                }
            }
        }
        "get" => {
            let key = key
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--key required"))?;
            match cred_provider.get(key) {
                Ok(val) => {
                    println!("{}", val);
                }
                Err(e) => {
                    eprintln!("Error getting secret: {}", e);
                }
            }
        }
        "set" => {
            let key = key
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--key required"))?;
            let value = value
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--value required"))?;
            match cred_provider.set(key, value) {
                Ok(_) => {
                    println!("Secret '{}' stored successfully.", key);
                }
                Err(e) => {
                    eprintln!("Error setting secret: {}", e);
                }
            }
        }
        "delete" | "rm" => {
            let key = key
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--key required"))?;
            match cred_provider.delete(key) {
                Ok(_) => {
                    println!("Secret '{}' deleted.", key);
                }
                Err(e) => {
                    eprintln!("Error deleting secret: {}", e);
                }
            }
        }
        "status" => {
            let status = cred_provider.status();
            println!("Provider: {}", status.provider);
            println!("Available: {}", status.available);
            println!("Secrets count: {}", status.secrets_count);
            if let Some(err) = status.last_error {
                println!("Last error: {}", err);
            }
        }
        _ => {
            eprintln!(
                "Unknown credentials subcommand: {}. Use: list, get, set, delete, status",
                subcommand
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    // ── handle_config: get/set/list ─────────────────────────────────────

    #[test]
    fn config_list_succeeds() {
        let (_tmp, st) = test_state();
        let result = handle_config(&st, None, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn config_get_known_key() {
        let (_tmp, st) = test_state();
        let result = handle_config(&st, Some("log_level".to_string()), None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn config_get_unknown_key() {
        let (_tmp, st) = test_state();
        let result = handle_config(&st, Some("nonexistent".to_string()), None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn config_set_valid() {
        let (_tmp, st) = test_state();
        let result = handle_config(
            &st,
            Some("log_level".to_string()),
            Some("debug".to_string()),
            false,
        );
        assert!(result.is_ok());
        let val = st.get_config("log_level").unwrap();
        assert_eq!(val, Some("debug".to_string()));
    }

    #[test]
    fn config_set_invalid_value() {
        let (_tmp, st) = test_state();
        let result = handle_config(
            &st,
            Some("log_level".to_string()),
            Some("verbose_mode".to_string()),
            false,
        );
        assert!(result.is_ok());
        let val = st.get_config("log_level").unwrap();
        assert_eq!(val, Some("info".to_string()));
    }

    #[test]
    fn config_no_args_shows_help() {
        let (_tmp, st) = test_state();
        let result = handle_config(&st, None, None, false);
        assert!(result.is_ok());
    }

    // ── handle_list_providers ───────────────────────────────────────────

    #[test]
    fn list_providers_succeeds() {
        let result = handle_list_providers();
        assert!(result.is_ok());
    }

    // ── handle_init ─────────────────────────────────────────────────────

    #[test]
    fn init_creates_files() {
        let (tmp, st) = test_state();
        let impulse_dir = tmp.path();
        let result = handle_init(&st, impulse_dir);
        assert!(result.is_ok());
        assert!(impulse_dir.join("LIVE_STATE.json").exists());
        assert!(impulse_dir.join("GENOME.md").exists());
        assert!(impulse_dir.join("config.json").exists());
    }

    // ── handle_status ───────────────────────────────────────────────────

    #[tokio::test]
    async fn status_shows_no_sessions() {
        let (_tmp, st) = test_state();
        let result = handle_status(&st).await;
        assert!(result.is_ok());
    }

    // ── handle_model ────────────────────────────────────────────────────

    #[test]
    fn model_set_and_get() {
        let (tmp, st) = test_state();
        let result = handle_model(
            &st,
            tmp.path(),
            false,
            "set".to_string(),
            Some("anthropic".to_string()),
            Some("claude-3-5-sonnet".to_string()),
        );
        assert!(result.is_ok());

        let result = handle_model(
            &st,
            tmp.path(),
            false,
            "get".to_string(),
            Some("anthropic".to_string()),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn model_set_missing_provider_fails() {
        let (tmp, st) = test_state();
        let result = handle_model(
            &st,
            tmp.path(),
            false,
            "set".to_string(),
            None,
            Some("gpt-4".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn model_unknown_subcommand() {
        let (tmp, st) = test_state();
        let result = handle_model(&st, tmp.path(), false, "invalid".to_string(), None, None);
        assert!(result.is_ok());
    }
}
