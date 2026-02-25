// Models module - manage model configurations and selections

use super::ModelInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Provider-specific model settings
    pub providers: HashMap<String, ProviderModelConfig>,
    /// Default provider
    pub default_provider: String,
    /// Default model (if not using provider default)
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelConfig {
    /// The default model for this provider
    pub default_model: String,
    /// Whether this provider is enabled
    pub enabled: bool,
    /// API key (should be loaded from env, not stored here)
    #[serde(skip)]
    pub api_key: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        let mut providers = HashMap::new();

        providers.insert(
            "anthropic".to_string(),
            ProviderModelConfig {
                default_model: "claude-sonnet-4-20250514".to_string(),
                enabled: true,
                api_key: None,
            },
        );

        providers.insert(
            "openai".to_string(),
            ProviderModelConfig {
                default_model: "gpt-4o".to_string(),
                enabled: true,
                api_key: None,
            },
        );

        providers.insert(
            "google".to_string(),
            ProviderModelConfig {
                default_model: "gemini-2.0-flash".to_string(),
                enabled: true,
                api_key: None,
            },
        );

        providers.insert(
            "mistral".to_string(),
            ProviderModelConfig {
                default_model: "mistral-small-2409".to_string(),
                enabled: true,
                api_key: None,
            },
        );

        Self {
            providers,
            default_provider: "anthropic".to_string(),
            default_model: None,
        }
    }
}

impl ModelConfig {
    /// Get the effective default model for a provider
    pub fn get_default_model(&self, provider: &str) -> Option<String> {
        self.providers
            .get(provider)
            .map(|c| c.default_model.clone())
            .or_else(|| self.default_model.clone())
    }

    /// Set default model for a provider
    pub fn set_default_model(&mut self, provider: &str, model: &str) -> Result<(), String> {
        if let Some(config) = self.providers.get_mut(provider) {
            config.default_model = model.to_string();
            Ok(())
        } else {
            Err(format!("Unknown provider: {}", provider))
        }
    }

    /// Enable or disable a provider
    pub fn set_provider_enabled(&mut self, provider: &str, enabled: bool) -> Result<(), String> {
        if let Some(config) = self.providers.get_mut(provider) {
            config.enabled = enabled;
            Ok(())
        } else {
            Err(format!("Unknown provider: {}", provider))
        }
    }

    /// Get list of enabled providers
    pub fn enabled_providers(&self) -> Vec<String> {
        self.providers
            .iter()
            .filter(|(_, c)| c.enabled)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// Filter options for model listing
#[derive(Debug, Default)]
pub struct ModelFilter {
    pub provider: Option<String>,
    pub supports_vision: Option<bool>,
    pub supports_function_calling: Option<bool>,
    pub latest_only: bool,
}

impl ModelFilter {
    pub fn apply(&self, models: &[ModelInfo]) -> Vec<ModelInfo> {
        models
            .iter()
            .filter(|m| {
                if let Some(ref p) = self.provider {
                    if &m.provider != p {
                        return false;
                    }
                }
                if let Some(vision) = self.supports_vision {
                    if m.supports_vision != vision {
                        return false;
                    }
                }
                if let Some(functions) = self.supports_function_calling {
                    if m.supports_function_calling != functions {
                        return false;
                    }
                }
                if self.latest_only && !m.is_latest {
                    return false;
                }
                true
            })
            .cloned()
            .collect()
    }
}

/// Format models for display
pub fn format_models(models: &[ModelInfo], verbose: bool) -> String {
    if verbose {
        let mut output = String::new();
        for model in models {
            output.push_str(&format!("{} ({})\n", model.name, model.id));
            output.push_str(&format!("  Provider: {}\n", model.provider));
            if let Some(desc) = &model.description {
                output.push_str(&format!("  Description: {}\n", desc));
            }
            if let Some(ctx) = model.context_window {
                output.push_str(&format!("  Context: {} tokens\n", ctx));
            }
            if let Some(input) = model.input_cost_per_mtok {
                if let Some(output_cost) = model.output_cost_per_mtok {
                    output.push_str(&format!(
                        "  Cost: ${}/1M in, ${}/1M out\n",
                        input, output_cost
                    ));
                }
            }
            output.push_str(&format!(
                "  Features: vision={}, functions={}, json={}\n",
                model.supports_vision, model.supports_function_calling, model.supports_json
            ));
            output.push('\n');
        }
        output
    } else {
        // Brief table format
        let mut output = format!(
            "{:<35} {:<15} {:<12} {}\n",
            "Model", "Provider", "Context", "Features"
        );
        output.push_str(&"=".repeat(80));
        output.push('\n');

        for model in models {
            let features: Vec<&str> = [
                if model.supports_vision { "V" } else { "" },
                if model.supports_function_calling {
                    "F"
                } else {
                    ""
                },
                if model.supports_json { "J" } else { "" },
            ]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect();

            let features_str = features.join(",");

            let context = model
                .context_window
                .map(|c| format!("{}k", c / 1000))
                .unwrap_or_else(|| "-".to_string());

            output.push_str(&format!(
                "{:<35} {:<15} {:<12} {}\n",
                model.id, model.provider, context, features_str
            ));
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_config_default() {
        let config = ModelConfig::default();
        assert_eq!(config.default_provider, "anthropic");
        assert!(config.providers.contains_key("openai"));
    }

    #[test]
    fn test_set_default_model() {
        let mut config = ModelConfig::default();
        config
            .set_default_model("anthropic", "claude-opus-4-5-20250514")
            .unwrap();
        assert_eq!(
            config.get_default_model("anthropic").unwrap(),
            "claude-opus-4-5-20250514"
        );
    }

    #[test]
    fn test_model_filter() {
        let models = vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "openai".to_string(),
                description: None,
                context_window: Some(128000),
                input_cost_per_mtok: None,
                output_cost_per_mtok: None,
                supports_vision: true,
                supports_function_calling: true,
                supports_json: true,
                is_latest: true,
            },
            ModelInfo {
                id: "claude-haiku".to_string(),
                name: "Claude Haiku".to_string(),
                provider: "anthropic".to_string(),
                description: None,
                context_window: Some(200000),
                input_cost_per_mtok: None,
                output_cost_per_mtok: None,
                supports_vision: true,
                supports_function_calling: false,
                supports_json: true,
                is_latest: true,
            },
        ];

        let filter = ModelFilter {
            provider: Some("openai".to_string()),
            ..Default::default()
        };

        let filtered = filter.apply(&models);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "gpt-4o");
    }
}
