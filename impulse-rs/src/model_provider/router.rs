//! Role-based model selection.
//!
//! A role (ion, photon, orchestrator) names its own profile in
//! `model_endpoints.roles` and may list more in `model_endpoints.fallbacks`.
//! The router builds one provider per profile and wraps them in a
//! [`PolicyProvider`] under `model_endpoints.policy`. Nothing here picks a
//! model by quality tier or by name: what each role runs is configuration.

use std::net::IpAddr;
use std::sync::Arc;

use super::anthropic::AnthropicProvider;
use super::openai_chat::OpenAiChatProvider;
use super::policy::{Candidate, PolicyProvider};
use super::{ModelCapabilities, ModelProvider, ProviderError};
use crate::model_endpoint::{
    EndpointError, EndpointRole, ModelEndpoint, ModelEndpointConfig, WireProtocol,
};

/// Why a role could not be given a provider.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error(transparent)]
    Config(#[from] EndpointError),
    #[error("profile '{profile}' cannot be used: {source}")]
    Provider {
        profile: String,
        #[source]
        source: ProviderError,
    },
}

/// Whether `endpoint` is on this machine: a numeric loopback host, the same
/// test ADR-0022 uses to allow plain HTTP.
pub fn is_local(endpoint: &ModelEndpoint) -> bool {
    reqwest::Url::parse(&endpoint.base_url)
        .ok()
        .and_then(|url| {
            url.host_str().map(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string()
            })
        })
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback())
}

/// The capabilities an endpoint declares, or its protocol's defaults.
pub fn capabilities_of(endpoint: &ModelEndpoint) -> ModelCapabilities {
    endpoint
        .capabilities
        .unwrap_or_else(|| ModelCapabilities::for_protocol(endpoint.protocol))
}

/// Builds the provider for one validated endpoint.
pub fn provider_for_endpoint(
    profile: &str,
    endpoint: &ModelEndpoint,
) -> Result<Arc<dyn ModelProvider>, ProviderError> {
    let capabilities = capabilities_of(endpoint);
    match endpoint.protocol {
        WireProtocol::AnthropicMessages => Ok(Arc::new(AnthropicProvider::new(
            profile,
            endpoint.clone(),
            capabilities,
        )?)),
        WireProtocol::OpenaiChat => Ok(Arc::new(OpenAiChatProvider::new(
            profile,
            endpoint.clone(),
            capabilities,
        )?)),
        // Photon reaches the Responses API through min-agent; a native
        // provider for it is follow-up work, refused here rather than guessed.
        WireProtocol::OpenaiResponses => Err(ProviderError::UnsupportedProtocol {
            protocol: endpoint.protocol,
        }),
    }
}

/// Role → provider, from a validated `model_endpoints` section.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    config: ModelEndpointConfig,
}

impl ModelRouter {
    /// Validates `config` once; every later lookup can trust it.
    pub fn new(config: ModelEndpointConfig) -> Result<Self, RouterError> {
        config.validate()?;
        Ok(Self { config })
    }

    /// A router for one role, validating only that role's profiles and the
    /// policy (see [`ModelEndpointConfig::validate_role`]).
    pub fn for_role(config: ModelEndpointConfig, role: EndpointRole) -> Result<Self, RouterError> {
        config.validate_role(role)?;
        Ok(Self { config })
    }

    /// The profile names a role may use, in configured order: its own
    /// profile, then its fallbacks, without duplicates.
    pub fn profiles_for(&self, role: EndpointRole) -> Result<Vec<String>, RouterError> {
        let mut names = Vec::new();
        if self.config.endpoint_for(role)?.is_some() {
            let own = match role {
                EndpointRole::Ion => self.config.roles.ion.as_deref(),
                EndpointRole::Photon => self.config.roles.photon.as_deref(),
                EndpointRole::Orchestrator => self.config.roles.orchestrator.as_deref(),
            };
            if let Some(own) = own {
                names.push(own.trim().to_string());
            }
        }
        for fallback in self.config.fallbacks.for_role(role) {
            let fallback = fallback.trim().to_string();
            if !names.contains(&fallback) {
                names.push(fallback);
            }
        }
        Ok(names)
    }

    /// The policy provider for `role`, or `None` when the role has no
    /// profile and no fallbacks (the caller keeps its legacy path).
    pub fn provider_for(&self, role: EndpointRole) -> Result<Option<PolicyProvider>, RouterError> {
        let names = self.profiles_for(role)?;
        if names.is_empty() {
            return Ok(None);
        }
        let mut candidates = Vec::with_capacity(names.len());
        for name in names {
            // Validated in `new`: every listed profile exists.
            let Some(endpoint) = self.config.profiles.get(&name) else {
                continue;
            };
            let provider =
                provider_for_endpoint(&name, endpoint).map_err(|source| RouterError::Provider {
                    profile: name.clone(),
                    source,
                })?;
            candidates.push(Candidate {
                provider,
                local: is_local(endpoint),
            });
        }
        Ok(Some(PolicyProvider::new(
            role.to_string(),
            self.config.policy,
            candidates,
        )))
    }

    /// The model id the role's first-choice profile names, for display.
    pub fn primary_model(&self, role: EndpointRole) -> Result<Option<String>, RouterError> {
        Ok(self
            .config
            .endpoint_for(role)?
            .map(|endpoint| endpoint.model.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_endpoint::{EndpointAuth, EndpointFallbacks, EndpointRoles};
    use std::collections::BTreeMap;

    fn openai(base_url: &str, model: &str) -> ModelEndpoint {
        ModelEndpoint {
            protocol: WireProtocol::OpenaiChat,
            base_url: base_url.into(),
            auth: EndpointAuth::None,
            model: model.into(),
            max_output_tokens: None,
            capabilities: None,
        }
    }

    fn config() -> ModelEndpointConfig {
        let anthropic =
            ModelEndpoint::anthropic_messages("https://api.anthropic.com", "sonnet", 4096);
        ModelEndpointConfig {
            profiles: BTreeMap::from([
                ("cloud".to_string(), anthropic),
                (
                    "ollama".to_string(),
                    openai("http://127.0.0.1:11434/v1", "qwen3"),
                ),
                (
                    "router".to_string(),
                    openai("https://openrouter.ai/api/v1", "x/y"),
                ),
            ]),
            roles: EndpointRoles {
                ion: Some("cloud".into()),
                photon: None,
                orchestrator: Some("router".into()),
            },
            fallbacks: EndpointFallbacks {
                ion: vec!["ollama".into(), "cloud".into()],
                photon: vec!["ollama".into()],
                orchestrator: Vec::new(),
            },
            policy: Default::default(),
        }
    }

    #[test]
    fn test_is_local_accepts_numeric_loopback_only() {
        assert!(is_local(&openai("http://127.0.0.1:11434/v1", "m")));
        assert!(is_local(&openai("http://[::1]:8000/v1", "m")));
        assert!(!is_local(&openai("http://localhost:11434/v1", "m")));
        assert!(!is_local(&openai("https://api.openai.com/v1", "m")));
    }

    #[test]
    fn test_role_candidates_are_own_profile_then_fallbacks_without_duplicates() {
        let router = ModelRouter::new(config()).expect("valid");
        assert_eq!(
            router.profiles_for(EndpointRole::Ion).unwrap(),
            ["cloud", "ollama"]
        );
        assert_eq!(
            router.profiles_for(EndpointRole::Photon).unwrap(),
            ["ollama"]
        );
        assert_eq!(
            router.profiles_for(EndpointRole::Orchestrator).unwrap(),
            ["router"]
        );
    }

    #[test]
    fn test_provider_for_orders_local_first_by_default() {
        let router = ModelRouter::new(config()).expect("valid");
        let ion = router
            .provider_for(EndpointRole::Ion)
            .unwrap()
            .expect("ion configured");
        assert_eq!(ion.order(), ["ollama", "cloud"]);
        assert_eq!(ion.name(), "ion");
        assert_eq!(
            router.primary_model(EndpointRole::Ion).unwrap().as_deref(),
            Some("sonnet")
        );
    }

    #[test]
    fn test_unassigned_role_without_fallbacks_is_none() {
        let mut config = config();
        config.fallbacks.photon.clear();
        let router = ModelRouter::new(config).unwrap();
        assert!(router.provider_for(EndpointRole::Photon).unwrap().is_none());
    }

    #[test]
    fn test_unknown_fallback_is_a_config_error_not_a_silent_skip() {
        let mut config = config();
        config.fallbacks.ion.push("missing".into());
        let err = ModelRouter::new(config).unwrap_err();
        assert!(err.to_string().contains("missing"));
    }

    /// Review P1: a broken profile used only by photon must not stop Ion.
    #[test]
    fn test_for_role_ignores_another_roles_broken_profile() {
        let mut config = config();
        config
            .profiles
            .insert("bad".into(), openai("http://example.com/v1", "m"));
        config.roles.photon = Some("bad".into());
        config.fallbacks.photon.clear();
        assert!(
            ModelRouter::new(config.clone()).is_err(),
            "the whole config is invalid"
        );
        let ion = ModelRouter::for_role(config.clone(), EndpointRole::Ion).expect("ion is fine");
        assert!(ion.provider_for(EndpointRole::Ion).unwrap().is_some());
        assert!(ModelRouter::for_role(config, EndpointRole::Photon).is_err());
    }

    #[test]
    fn test_responses_protocol_is_refused_by_name() {
        let mut config = config();
        config.profiles.insert(
            "responses".into(),
            ModelEndpoint {
                protocol: WireProtocol::OpenaiResponses,
                ..openai("https://api.openai.com/v1", "gpt")
            },
        );
        config.roles.photon = Some("responses".into());
        let router = ModelRouter::new(config).unwrap();
        let err = router
            .provider_for(EndpointRole::Photon)
            .err()
            .expect("refused");
        assert!(err.to_string().contains("responses"));
    }

    #[test]
    fn test_declared_capabilities_override_protocol_defaults() {
        let mut endpoint = openai("http://127.0.0.1:11434/v1", "tiny");
        assert!(capabilities_of(&endpoint).tool_calling);
        endpoint.capabilities = Some(ModelCapabilities {
            tool_calling: false,
            vision: false,
            streaming: true,
            structured_output: false,
            context_window: Some(8_192),
        });
        assert!(!capabilities_of(&endpoint).tool_calling);
        endpoint
            .validate()
            .expect("capabilities do not affect validation");
    }
}
