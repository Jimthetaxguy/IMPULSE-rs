// Fetch module - retrieve latest model information from AI providers
// Uses provider APIs to get current model lists

use super::{known_providers, ModelInfo};
use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

/// Fetch models from OpenAI API
pub async fn fetch_openai_models(api_key: &str) -> Result<Vec<ModelInfo>> {
    let client = Client::new();

    let response = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("OpenAI API error: {}", response.status()));
    }

    #[derive(Deserialize)]
    struct OpenAiModelsResponse {
        data: Vec<OpenAiModel>,
    }

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct OpenAiModel {
        id: String,
        object: String,
        created: u64,
        owned_by: String,
    }

    let resp: OpenAiModelsResponse = response.json().await?;

    let models: Vec<ModelInfo> = resp
        .data
        .into_iter()
        .filter(|m| m.id.starts_with("gpt-") || m.id.starts_with("o1") || m.id.starts_with("o3"))
        .map(|m| {
            let (supports_vision, supports_function_calling) =
                if m.id.contains("vision") || m.id.contains("4o") {
                    (true, true)
                } else {
                    (false, m.id.contains("4o") || m.id.contains("function"))
                };

            ModelInfo {
                id: m.id.clone(),
                name: m.id.clone(),
                provider: "openai".to_string(),
                description: Some(format!("Created: {}, Owned by: {}", m.created, m.owned_by)),
                context_window: Some(if m.id.contains("32k") { 32000 } else { 128000 }),
                input_cost_per_mtok: Some(if m.id.contains("4o") { 2.50 } else { 10.00 }),
                output_cost_per_mtok: Some(if m.id.contains("4o") { 10.00 } else { 30.00 }),
                supports_vision,
                supports_function_calling,
                supports_json: true,
                is_latest: true,
            }
        })
        .collect();

    Ok(models)
}

/// Fetch models from Anthropic - uses static list since API requires special handling
pub fn fetch_anthropic_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-opus-4-5-20250514".to_string(),
            name: "Claude Opus 4.5".to_string(),
            provider: "anthropic".to_string(),
            description: Some(
                "Most capable model for complex reasoning and creative writing".to_string(),
            ),
            context_window: Some(200000),
            input_cost_per_mtok: Some(15.00),
            output_cost_per_mtok: Some(75.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "claude-sonnet-4-20250514".to_string(),
            name: "Claude Sonnet 4.5".to_string(),
            provider: "anthropic".to_string(),
            description: Some("Balanced model for most tasks".to_string()),
            context_window: Some(200000),
            input_cost_per_mtok: Some(3.00),
            output_cost_per_mtok: Some(15.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "claude-haiku-3-5-20250514".to_string(),
            name: "Claude Haiku 3.5".to_string(),
            provider: "anthropic".to_string(),
            description: Some("Fast, affordable model for simple tasks".to_string()),
            context_window: Some(200000),
            input_cost_per_mtok: Some(0.80),
            output_cost_per_mtok: Some(4.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "claude-3-5-sonnet-20241022".to_string(),
            name: "Claude 3.5 Sonnet".to_string(),
            provider: "anthropic".to_string(),
            description: Some("Previous generation Sonnet".to_string()),
            context_window: Some(200000),
            input_cost_per_mtok: Some(3.00),
            output_cost_per_mtok: Some(15.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: false,
        },
        ModelInfo {
            id: "claude-3-opus-20240229".to_string(),
            name: "Claude 3 Opus".to_string(),
            provider: "anthropic".to_string(),
            description: Some("Previous generation Opus".to_string()),
            context_window: Some(200000),
            input_cost_per_mtok: Some(15.00),
            output_cost_per_mtok: Some(75.00),
            supports_vision: true,
            supports_function_calling: false,
            supports_json: true,
            is_latest: false,
        },
    ]
}

/// Fetch models from Google (Gemini)
pub fn fetch_google_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-2.0-flash".to_string(),
            name: "Gemini 2.0 Flash".to_string(),
            provider: "google".to_string(),
            description: Some("Latest fast model with native tool use".to_string()),
            context_window: Some(1000000),
            input_cost_per_mtok: Some(0.00), // Free tier
            output_cost_per_mtok: Some(0.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "gemini-2.0-flash-exp".to_string(),
            name: "Gemini 2.0 Flash Experimental".to_string(),
            provider: "google".to_string(),
            description: Some("Experimental version of Flash 2.0".to_string()),
            context_window: Some(1000000),
            input_cost_per_mtok: None,
            output_cost_per_mtok: None,
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "gemini-1.5-pro".to_string(),
            name: "Gemini 1.5 Pro".to_string(),
            provider: "google".to_string(),
            description: Some("Previous generation pro model".to_string()),
            context_window: Some(2000000),
            input_cost_per_mtok: Some(1.25),
            output_cost_per_mtok: Some(5.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: false,
        },
        ModelInfo {
            id: "gemini-1.5-flash".to_string(),
            name: "Gemini 1.5 Flash".to_string(),
            provider: "google".to_string(),
            description: Some("Previous generation flash model".to_string()),
            context_window: Some(1000000),
            input_cost_per_mtok: Some(0.075),
            output_cost_per_mtok: Some(0.30),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: false,
        },
    ]
}

/// Fetch models from Mistral AI
pub fn fetch_mistral_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "mistral-large-2411".to_string(),
            name: "Mistral Large 2".to_string(),
            provider: "mistral".to_string(),
            description: Some("Mistral's flagship model".to_string()),
            context_window: Some(128000),
            input_cost_per_mtok: Some(2.00),
            output_cost_per_mtok: Some(6.00),
            supports_vision: false,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "mistral-small-2409".to_string(),
            name: "Mistral Small".to_string(),
            provider: "mistral".to_string(),
            description: Some("Efficient model for most tasks".to_string()),
            context_window: Some(128000),
            input_cost_per_mtok: Some(0.20),
            output_cost_per_mtok: Some(0.60),
            supports_vision: false,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
        ModelInfo {
            id: "pixtral-large-2411".to_string(),
            name: "Pixtral Large".to_string(),
            provider: "mistral".to_string(),
            description: Some("Mistral's vision model".to_string()),
            context_window: Some(128000),
            input_cost_per_mtok: Some(2.00),
            output_cost_per_mtok: Some(6.00),
            supports_vision: true,
            supports_function_calling: true,
            supports_json: true,
            is_latest: true,
        },
    ]
}

/// Fetch all known models (combines static lists + API calls where available)
pub async fn fetch_all_models(openai_api_key: Option<&str>) -> Result<Vec<ModelInfo>> {
    let mut all_models = Vec::new();

    // Anthropic models (static list - comprehensive)
    all_models.extend(fetch_anthropic_models());

    // Google models (static list)
    all_models.extend(fetch_google_models());

    // Mistral models (static list)
    all_models.extend(fetch_mistral_models());

    // OpenAI models (API call if key provided)
    if let Some(key) = openai_api_key {
        if let Ok(models) = fetch_openai_models(key).await {
            all_models.extend(models);
        }
    } else {
        // Add known models without API key
        all_models.extend(vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                name: "GPT-4o".to_string(),
                provider: "openai".to_string(),
                description: Some("OpenAI's flagship model".to_string()),
                context_window: Some(128000),
                input_cost_per_mtok: Some(2.50),
                output_cost_per_mtok: Some(10.00),
                supports_vision: true,
                supports_function_calling: true,
                supports_json: true,
                is_latest: true,
            },
            ModelInfo {
                id: "gpt-4o-mini".to_string(),
                name: "GPT-4o Mini".to_string(),
                provider: "openai".to_string(),
                description: Some("Smaller, faster GPT-4o".to_string()),
                context_window: Some(128000),
                input_cost_per_mtok: Some(0.15),
                output_cost_per_mtok: Some(0.60),
                supports_vision: true,
                supports_function_calling: true,
                supports_json: true,
                is_latest: true,
            },
            ModelInfo {
                id: "o1".to_string(),
                name: "OpenAI o1".to_string(),
                provider: "openai".to_string(),
                description: Some("Reasoning model".to_string()),
                context_window: Some(200000),
                input_cost_per_mtok: Some(15.00),
                output_cost_per_mtok: Some(60.00),
                supports_vision: false,
                supports_function_calling: false,
                supports_json: false,
                is_latest: true,
            },
            ModelInfo {
                id: "o1-mini".to_string(),
                name: "OpenAI o1-mini".to_string(),
                provider: "openai".to_string(),
                description: Some("Smaller reasoning model".to_string()),
                context_window: Some(128000),
                input_cost_per_mtok: Some(3.00),
                output_cost_per_mtok: Some(12.00),
                supports_vision: false,
                supports_function_calling: false,
                supports_json: false,
                is_latest: true,
            },
        ]);
    }

    Ok(all_models)
}

/// Fetch docs URL for a specific provider
pub fn fetch_docs_url(provider_id: &str) -> Option<String> {
    known_providers()
        .into_iter()
        .find(|p| p.id == provider_id)
        .map(|p| p.docs_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_all_models() {
        let models = fetch_all_models(None).await.unwrap();
        assert!(!models.is_empty());
    }

    #[test]
    fn test_fetch_anthropic_models() {
        let models = fetch_anthropic_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id.contains("opus")));
    }

    #[test]
    fn test_fetch_docs_url() {
        assert_eq!(
            fetch_docs_url("openai"),
            Some("https://platform.openai.com/docs".to_string())
        );
        assert_eq!(
            fetch_docs_url("anthropic"),
            Some("https://docs.anthropic.com/en/docs".to_string())
        );
        assert_eq!(fetch_docs_url("unknown"), None);
    }
}
