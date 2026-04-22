//! Documentation fetching and display for AI providers.
//!
//! Fetches and caches model lists from OpenAI, Anthropic, and other providers.
//! Supports offline mode via local cache in `.impulse/docs_cache/`.

pub mod cache;
pub mod fetch;
pub mod models;

use serde::{Deserialize, Serialize};

/// Represents an AI provider (OpenAI, Anthropic, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api_url: String,
    pub docs_url: String,
    pub models_url: String,
}

/// Represents a model from an AI provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub input_cost_per_mtok: Option<f64>,
    pub output_cost_per_mtok: Option<f64>,
    pub supports_vision: bool,
    pub supports_function_calling: bool,
    pub supports_json: bool,
    pub is_latest: bool,
}

impl ModelInfo {
    pub fn new(id: &str, name: &str, provider: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            provider: provider.to_string(),
            description: None,
            context_window: None,
            input_cost_per_mtok: None,
            output_cost_per_mtok: None,
            supports_vision: false,
            supports_function_calling: false,
            supports_json: false,
            is_latest: true,
        }
    }
}

/// Known AI providers
pub fn known_providers() -> Vec<Provider> {
    vec![
        Provider {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            api_url: "https://api.openai.com".to_string(),
            docs_url: "https://platform.openai.com/docs".to_string(),
            models_url: "https://api.openai.com/v1/models".to_string(),
        },
        Provider {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            api_url: "https://api.anthropic.com".to_string(),
            docs_url: "https://docs.anthropic.com/en/docs".to_string(),
            models_url: "https://docs.anthropic.com/en/docs/models".to_string(),
        },
        Provider {
            id: "google".to_string(),
            name: "Google".to_string(),
            api_url: "https://generativelanguage.googleapis.com".to_string(),
            docs_url: "https://ai.google.dev/docs".to_string(),
            models_url: "https://ai.google.dev/models".to_string(),
        },
        Provider {
            id: "mistral".to_string(),
            name: "Mistral AI".to_string(),
            api_url: "https://api.mistral.ai".to_string(),
            docs_url: "https://docs.mistral.ai".to_string(),
            models_url: "https://docs.mistral.ai/models".to_string(),
        },
        Provider {
            id: "cohere".to_string(),
            name: "Cohere".to_string(),
            api_url: "https://api.cohere.ai".to_string(),
            docs_url: "https://docs.cohere.com/docs".to_string(),
            models_url: "https://docs.cohere.com/docs/models".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_providers() {
        let providers = known_providers();
        assert!(!providers.is_empty());
        assert!(providers.iter().any(|p| p.id == "openai"));
        assert!(providers.iter().any(|p| p.id == "anthropic"));
    }

    #[test]
    fn test_model_info_new() {
        let model = ModelInfo::new("gpt-4o", "GPT-4o", "openai");
        assert_eq!(model.id, "gpt-4o");
        assert_eq!(model.provider, "openai");
        assert!(model.is_latest);
    }
}
