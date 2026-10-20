//! Typed model endpoints (ADR-0022): a protocol, not a vendor.
//!
//! A [`ModelEndpoint`] says how to reach one model: the wire protocol, the API
//! base URL the protocol path is appended to, how to authenticate (an
//! environment variable *name*, never a secret), the model id, and an optional
//! output-token limit. Named endpoints live in `config.json` under
//! `model_endpoints.profiles`; `model_endpoints.roles` maps a harness role
//! (ion, photon, orchestrator) to a profile name, and
//! `model_endpoints.fallbacks` lists further profiles per role that the
//! stage-2 endpoint policy (`model_provider::policy`) may try, under
//! `model_endpoints.policy`.
//!
//! Everything here is pure data plus validation. Only [`min_agent_bridge`]
//! knows another crate's config shapes.

#[cfg(feature = "photon-subagent")]
pub mod min_agent_bridge;

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Longest accepted profile, env-var, or header name.
const MAX_NAME_LEN: usize = 64;

/// Environment variable listing extra hosts (comma-separated) that a
/// configured profile may send a credential to.
pub const TRUSTED_MODEL_HOSTS_ENV: &str = "IMPULSE_TRUSTED_MODEL_HOSTS";

/// The request format an endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireProtocol {
    /// `{base_url}/messages` (Anthropic Messages API).
    AnthropicMessages,
    /// `{base_url}/chat/completions` (OpenAI Chat Completions and compatible
    /// servers: OpenRouter, LiteLLM, Ollama, vLLM, ...).
    OpenaiChat,
    /// `{base_url}/responses` (OpenAI Responses API).
    OpenaiResponses,
}

impl fmt::Display for WireProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AnthropicMessages => "anthropic_messages",
            Self::OpenaiChat => "openai_chat",
            Self::OpenaiResponses => "openai_responses",
        })
    }
}

/// How an endpoint authenticates. Only environment-variable names are stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EndpointAuth {
    /// No credential (for example a local Ollama server).
    None,
    /// `Authorization: Bearer <value of env>`.
    BearerEnv { env: String },
    /// `<header>: <value of env>`, for example Anthropic's `x-api-key`.
    HeaderEnv { header: String, env: String },
}

/// One reachable model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpoint {
    pub protocol: WireProtocol,
    /// API base including its path prefix, e.g. `https://api.openai.com/v1`
    /// or `https://openrouter.ai/api/v1`; the protocol path is appended.
    pub base_url: String,
    pub auth: EndpointAuth,
    pub model: String,
    /// Output-token ceiling per turn. Required for `anthropic_messages`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// What this endpoint's model can do. Defaults to what the protocol
    /// supports; set it to narrow a model, for example a small local model
    /// without tool calling. Never inferred from the model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<crate::model_provider::ModelCapabilities>,
}

/// Harness roles that select an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointRole {
    Ion,
    Photon,
    Orchestrator,
}

impl EndpointRole {
    pub const ALL: [Self; 3] = [Self::Ion, Self::Photon, Self::Orchestrator];
}

impl fmt::Display for EndpointRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ion => "ion",
            Self::Photon => "photon",
            Self::Orchestrator => "orchestrator",
        })
    }
}

/// Role → profile name assignments.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointRoles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<String>,
}

/// Role → further profiles the endpoint policy may try after the role's own
/// profile, in configured order (local-first routing may reorder them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointFallbacks {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ion: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photon: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orchestrator: Vec<String>,
}

impl EndpointFallbacks {
    pub fn for_role(&self, role: EndpointRole) -> &[String] {
        match role {
            EndpointRole::Ion => &self.ion,
            EndpointRole::Photon => &self.photon,
            EndpointRole::Orchestrator => &self.orchestrator,
        }
    }
}

/// The `model_endpoints` section of `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointConfig {
    #[serde(default)]
    pub profiles: BTreeMap<String, ModelEndpoint>,
    #[serde(default)]
    pub roles: EndpointRoles,
    #[serde(default)]
    pub fallbacks: EndpointFallbacks,
    #[serde(default)]
    pub policy: crate::model_provider::policy::EndpointPolicy,
}

/// Why an endpoint or its configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EndpointError {
    #[error("model id must not be blank")]
    BlankModel,
    /// The URL itself is not echoed: one that fails to parse may still carry
    /// userinfo, and endpoint errors reach logs and the model.
    #[error("base_url is not a valid URL: {reason}")]
    InvalidBaseUrl { reason: String },
    #[error("base_url '{url}' must use https, or http to a numeric loopback address")]
    InsecureBaseUrl { url: String },
    #[error("base_url must not carry credentials, a query, or a fragment")]
    BaseUrlCarriesExtras,
    #[error("'{name}' is not a valid environment variable name")]
    InvalidEnvName { name: String },
    #[error("'{name}' is not a valid HTTP header name")]
    InvalidHeaderName { name: String },
    #[error("{protocol} endpoints require max_output_tokens")]
    MissingOutputLimit { protocol: WireProtocol },
    #[error("max_output_tokens must be positive")]
    ZeroOutputLimit,
    #[error("'{name}' is not a valid profile name (1-64 of A-Z a-z 0-9 _ -)")]
    InvalidProfileName { name: String },
    #[error("profile '{name}' is invalid: {reason}")]
    InvalidProfile { name: String, reason: String },
    #[error(
        "role '{role}' names profile '{profile}', which is not defined in model_endpoints.profiles"
    )]
    UnknownProfile { role: String, profile: String },
    #[error(
        "this profile would send {env} to '{host}', which is neither the {protocol} vendor host \
         ({vendor}) nor a numeric loopback address; if you trust it, add it to \
         IMPULSE_TRUSTED_MODEL_HOSTS in your environment (config.json cannot grant this)"
    )]
    UntrustedCredentialHost {
        host: String,
        env: String,
        protocol: WireProtocol,
        vendor: &'static str,
    },
    #[error("fallback for role '{role}' names profile '{profile}', which is not defined")]
    UnknownFallback { role: String, profile: String },
    #[error("invalid model_endpoints.policy: {reason}")]
    InvalidPolicy { reason: String },
}

/// Hosts a configured profile may send a credential to (ADR-0022, credential
/// destinations). `config.json` is project data that a cloned repository
/// controls, so it may choose endpoints but not where a secret goes: beyond
/// each protocol's vendor host and numeric loopback, only hosts the user
/// lists in [`TRUSTED_MODEL_HOSTS_ENV`] qualify.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedModelHosts {
    extra: Vec<String>,
}

impl TrustedModelHosts {
    /// Parses a comma-separated host list, ignoring blanks and case.
    pub fn from_list(value: Option<&str>) -> Self {
        let extra = value
            .unwrap_or("")
            .split(',')
            .map(|host| host.trim().trim_end_matches('.').to_ascii_lowercase())
            .filter(|host| !host.is_empty())
            .collect();
        Self { extra }
    }

    /// The list from [`TRUSTED_MODEL_HOSTS_ENV`].
    pub fn from_env() -> Self {
        Self::from_list(std::env::var(TRUSTED_MODEL_HOSTS_ENV).ok().as_deref())
    }

    fn allows(&self, host: &str) -> bool {
        self.extra.iter().any(|trusted| trusted == host)
    }
}

impl WireProtocol {
    /// The vendor's own API host, which may always receive the credential.
    pub fn vendor_host(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "api.anthropic.com",
            Self::OpenaiChat | Self::OpenaiResponses => "api.openai.com",
        }
    }
}

fn valid_name(name: &str, extra: impl Fn(u8) -> bool) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || extra(b))
}

fn validate_env_name(name: &str) -> Result<(), EndpointError> {
    let ok = valid_name(name, |b| b == b'_') && !name.as_bytes()[0].is_ascii_digit();
    ok.then_some(())
        .ok_or_else(|| EndpointError::InvalidEnvName {
            name: name.to_string(),
        })
}

impl EndpointAuth {
    /// Checks the env-var and header names.
    pub fn validate(&self) -> Result<(), EndpointError> {
        match self {
            Self::None => Ok(()),
            Self::BearerEnv { env } => validate_env_name(env),
            Self::HeaderEnv { header, env } => {
                if !valid_name(header, |b| b == b'-' || b == b'_') {
                    return Err(EndpointError::InvalidHeaderName {
                        name: header.clone(),
                    });
                }
                validate_env_name(env)
            }
        }
    }
}

impl ModelEndpoint {
    /// An Anthropic Messages endpoint at `origin` (scheme, host, optional
    /// port) authenticated by `ANTHROPIC_API_KEY`.
    pub fn anthropic_messages(origin: &str, model: &str, max_output_tokens: u32) -> Self {
        Self {
            protocol: WireProtocol::AnthropicMessages,
            base_url: format!("{}/v1", origin.trim_end_matches('/')),
            auth: EndpointAuth::HeaderEnv {
                header: "x-api-key".into(),
                env: "ANTHROPIC_API_KEY".into(),
            },
            model: model.to_string(),
            max_output_tokens: Some(max_output_tokens),
            capabilities: None,
        }
    }

    /// Validates every field. Plain HTTP is accepted only to a numeric
    /// loopback address, so a typo cannot send a prompt in cleartext.
    pub fn validate(&self) -> Result<(), EndpointError> {
        if self.model.trim().is_empty() {
            return Err(EndpointError::BlankModel);
        }
        let url =
            reqwest::Url::parse(&self.base_url).map_err(|e| EndpointError::InvalidBaseUrl {
                reason: e.to_string(),
            })?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(EndpointError::BaseUrlCarriesExtras);
        }
        // Numeric loopback only: a hostname such as `localhost` can resolve
        // anywhere, so it does not qualify for plain HTTP.
        let loopback = url
            .host_str()
            .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
            .and_then(|h| h.parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        match url.scheme() {
            "https" => {}
            "http" if loopback => {}
            _ => {
                return Err(EndpointError::InsecureBaseUrl {
                    url: self.base_url.clone(),
                })
            }
        }
        self.auth.validate()?;
        match self.max_output_tokens {
            Some(0) => Err(EndpointError::ZeroOutputLimit),
            None if self.protocol == WireProtocol::AnthropicMessages => {
                Err(EndpointError::MissingOutputLimit {
                    protocol: self.protocol,
                })
            }
            _ => Ok(()),
        }
    }
}

impl ModelEndpoint {
    /// Refuses a credential-bearing endpoint whose host neither belongs to
    /// the protocol's vendor, nor is a numeric loopback address, nor is listed
    /// in `trusted`. Endpoints built from the user's own environment (Ion's
    /// `ANTHROPIC_BASE_URL` default) are not subject to this; it guards
    /// profiles read from `config.json`.
    pub fn check_credential_destination(
        &self,
        trusted: &TrustedModelHosts,
    ) -> Result<(), EndpointError> {
        let env = match &self.auth {
            EndpointAuth::None => return Ok(()),
            EndpointAuth::BearerEnv { env } | EndpointAuth::HeaderEnv { env, .. } => env,
        };
        let url =
            reqwest::Url::parse(&self.base_url).map_err(|e| EndpointError::InvalidBaseUrl {
                reason: e.to_string(),
            })?;
        let host = url
            .host_str()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let loopback = host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        let vendor = self.protocol.vendor_host();
        if loopback || host == vendor || trusted.allows(&host) {
            return Ok(());
        }
        Err(EndpointError::UntrustedCredentialHost {
            host,
            env: env.clone(),
            protocol: self.protocol,
            vendor,
        })
    }
}

impl ModelEndpointConfig {
    /// Validates every profile name, every profile, and every role reference,
    /// with credential destinations checked against
    /// [`TrustedModelHosts::from_env`].
    pub fn validate(&self) -> Result<(), EndpointError> {
        self.validate_with(&TrustedModelHosts::from_env())
    }

    /// [`Self::validate`] against an explicit trusted-host list.
    pub fn validate_with(&self, trusted: &TrustedModelHosts) -> Result<(), EndpointError> {
        for (name, endpoint) in &self.profiles {
            if !valid_name(name, |b| b == b'_' || b == b'-') {
                return Err(EndpointError::InvalidProfileName { name: name.clone() });
            }
            endpoint
                .validate()
                .and_then(|()| endpoint.check_credential_destination(trusted))
                .map_err(|e| EndpointError::InvalidProfile {
                    name: name.clone(),
                    reason: e.to_string(),
                })?;
        }
        for role in EndpointRole::ALL {
            self.endpoint_for(role)?;
            for profile in self.fallbacks.for_role(role) {
                if !self.profiles.contains_key(profile.trim()) {
                    return Err(EndpointError::UnknownFallback {
                        role: role.to_string(),
                        profile: profile.clone(),
                    });
                }
            }
        }
        self.policy
            .validate()
            .map_err(|reason| EndpointError::InvalidPolicy { reason })
    }

    /// Validates only what `role` uses: its own profile, its fallbacks, and
    /// the policy. A broken profile assigned to another role does not stop
    /// this one from running. Credential destinations are checked against
    /// [`TrustedModelHosts::from_env`].
    pub fn validate_role(&self, role: EndpointRole) -> Result<(), EndpointError> {
        self.validate_role_with(role, &TrustedModelHosts::from_env())
    }

    /// [`Self::validate_role`] against an explicit trusted-host list.
    pub fn validate_role_with(
        &self,
        role: EndpointRole,
        trusted: &TrustedModelHosts,
    ) -> Result<(), EndpointError> {
        let own = self.endpoint_for(role)?.map(|_| ());
        let mut names: Vec<&str> = Vec::new();
        if own.is_some() {
            if let Some(name) = match role {
                EndpointRole::Ion => self.roles.ion.as_deref(),
                EndpointRole::Photon => self.roles.photon.as_deref(),
                EndpointRole::Orchestrator => self.roles.orchestrator.as_deref(),
            } {
                names.push(name.trim());
            }
        }
        for fallback in self.fallbacks.for_role(role) {
            if !self.profiles.contains_key(fallback.trim()) {
                return Err(EndpointError::UnknownFallback {
                    role: role.to_string(),
                    profile: fallback.clone(),
                });
            }
            names.push(fallback.trim());
        }
        for name in names {
            if let Some(endpoint) = self.profiles.get(name) {
                endpoint
                    .validate()
                    .and_then(|()| endpoint.check_credential_destination(trusted))
                    .map_err(|e| EndpointError::InvalidProfile {
                        name: name.to_string(),
                        reason: e.to_string(),
                    })?;
            }
        }
        self.policy
            .validate()
            .map_err(|reason| EndpointError::InvalidPolicy { reason })
    }

    /// Whether `role` has an own profile or any fallback configured.
    pub fn role_is_configured(&self, role: EndpointRole) -> bool {
        let own = match role {
            EndpointRole::Ion => self.roles.ion.as_deref(),
            EndpointRole::Photon => self.roles.photon.as_deref(),
            EndpointRole::Orchestrator => self.roles.orchestrator.as_deref(),
        };
        own.is_some_and(|name| !name.trim().is_empty()) || !self.fallbacks.for_role(role).is_empty()
    }

    /// The profile assigned to `role`, or `None` when the role is unassigned.
    /// A role that names a missing profile is an error, never a fallback.
    pub fn endpoint_for(
        &self,
        role: EndpointRole,
    ) -> Result<Option<&ModelEndpoint>, EndpointError> {
        let assigned = match role {
            EndpointRole::Ion => self.roles.ion.as_deref(),
            EndpointRole::Photon => self.roles.photon.as_deref(),
            EndpointRole::Orchestrator => self.roles.orchestrator.as_deref(),
        };
        let Some(name) = assigned.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        self.profiles
            .get(name)
            .map(Some)
            .ok_or_else(|| EndpointError::UnknownProfile {
                role: role.to_string(),
                profile: name.to_string(),
            })
    }
}

/// Reads the `model_endpoints` section of `<impulse_dir>/config.json`. A
/// missing file or section is the empty default; an unreadable or malformed
/// one is an error, so a typo never silently falls back to another endpoint.
pub fn load_config(impulse_dir: &std::path::Path) -> anyhow::Result<ModelEndpointConfig> {
    use anyhow::Context as _;
    #[derive(Deserialize, Default)]
    struct Section {
        #[serde(default)]
        model_endpoints: ModelEndpointConfig,
    }
    let path = impulse_dir.join("config.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", path.display())),
    };
    let section: Section = serde_json::from_str(&raw)
        .with_context(|| format!("cannot parse model_endpoints in {}", path.display()))?;
    Ok(section.model_endpoints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn openrouter() -> ModelEndpoint {
        ModelEndpoint {
            protocol: WireProtocol::OpenaiChat,
            base_url: "https://openrouter.ai/api/v1".into(),
            auth: EndpointAuth::BearerEnv {
                env: "OPENROUTER_API_KEY".into(),
            },
            model: "qwen/qwen3-coder".into(),
            max_output_tokens: None,
            capabilities: None,
        }
    }

    fn config_with(endpoint: ModelEndpoint, photon: Option<&str>) -> ModelEndpointConfig {
        ModelEndpointConfig {
            profiles: BTreeMap::from([("router".to_string(), endpoint)]),
            roles: EndpointRoles {
                ion: None,
                photon: photon.map(str::to_string),
                orchestrator: None,
            },
            ..ModelEndpointConfig::default()
        }
    }

    #[test]
    fn test_model_endpoint_config_round_trips_through_json() {
        let original = config_with(openrouter(), Some("router"));
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ModelEndpointConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
        let anthropic = ModelEndpoint::anthropic_messages("https://api.anthropic.com", "m", 10);
        let recovered: ModelEndpoint =
            serde_json::from_str(&serde_json::to_string(&anthropic).unwrap()).unwrap();
        assert_eq!(anthropic, recovered);
    }

    #[test]
    fn test_model_endpoint_config_parses_documented_shape() {
        let parsed: ModelEndpointConfig = serde_json::from_value(json!({
            "profiles": {
                "local-qwen": {
                    "protocol": "openai_chat",
                    "base_url": "http://127.0.0.1:11434/v1",
                    "auth": {"kind": "none"},
                    "model": "qwen2.5-coder:7b"
                }
            },
            "roles": {"photon": "local-qwen"}
        }))
        .unwrap();
        assert!(parsed.validate().is_ok());
        let endpoint = parsed.endpoint_for(EndpointRole::Photon).unwrap().unwrap();
        assert_eq!(endpoint.protocol, WireProtocol::OpenaiChat);
        assert_eq!(endpoint.auth, EndpointAuth::None);
    }

    #[test]
    fn test_model_endpoint_config_rejects_unknown_fields() {
        let err = serde_json::from_value::<ModelEndpointConfig>(json!({"profile": {}}));
        assert!(err.is_err());
        let err = serde_json::from_value::<ModelEndpoint>(json!({
            "protocol": "openai_chat", "base_url": "https://x.test/v1",
            "auth": {"kind": "none"}, "model": "m", "api_key": "sk-nope"
        }));
        assert!(err.is_err(), "a secret field must not deserialize");
        let err = serde_json::from_value::<WireProtocol>(json!("grpc"));
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_accepts_https_and_loopback_http() {
        assert!(openrouter().validate().is_ok());
        let mut local = openrouter();
        local.base_url = "http://[::1]:8080/v1".into();
        assert!(local.validate().is_ok());
        assert!(
            ModelEndpoint::anthropic_messages("http://127.0.0.1:4010", "m", 1)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn test_validate_rejects_insecure_and_malformed_urls() {
        let mut e = openrouter();
        e.base_url = "http://openrouter.ai/api/v1".into();
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InsecureBaseUrl { .. })
        ));
        e.base_url = "http://localhost:11434/v1".into();
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InsecureBaseUrl { .. })
        ));
        e.base_url = "not a url".into();
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InvalidBaseUrl { .. })
        ));
        e.base_url = "https://user:pw@openrouter.ai/v1".into();
        assert_eq!(e.validate(), Err(EndpointError::BaseUrlCarriesExtras));
        e.base_url = "https://openrouter.ai/v1?key=x".into();
        assert_eq!(e.validate(), Err(EndpointError::BaseUrlCarriesExtras));
        e.base_url = "ftp://openrouter.ai/v1".into();
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InsecureBaseUrl { .. })
        ));
    }

    #[test]
    fn test_validate_rejects_blank_model_bad_auth_and_output_limits() {
        let mut e = openrouter();
        e.model = "  ".into();
        assert_eq!(e.validate(), Err(EndpointError::BlankModel));
        let mut e = openrouter();
        e.auth = EndpointAuth::BearerEnv { env: "1BAD".into() };
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InvalidEnvName { .. })
        ));
        e.auth = EndpointAuth::HeaderEnv {
            header: "x api key".into(),
            env: "KEY".into(),
        };
        assert!(matches!(
            e.validate(),
            Err(EndpointError::InvalidHeaderName { .. })
        ));
        let mut a = ModelEndpoint::anthropic_messages("https://api.anthropic.com", "m", 1);
        a.max_output_tokens = None;
        assert_eq!(
            a.validate(),
            Err(EndpointError::MissingOutputLimit {
                protocol: WireProtocol::AnthropicMessages
            })
        );
        a.max_output_tokens = Some(0);
        assert_eq!(a.validate(), Err(EndpointError::ZeroOutputLimit));
    }

    #[test]
    fn test_endpoint_for_unassigned_role_is_none_and_missing_profile_is_error() {
        let config = config_with(openrouter(), None);
        assert_eq!(config.endpoint_for(EndpointRole::Photon), Ok(None));
        let config = config_with(openrouter(), Some("  "));
        assert_eq!(config.endpoint_for(EndpointRole::Photon), Ok(None));
        let config = config_with(openrouter(), Some("ghost"));
        let err = config.endpoint_for(EndpointRole::Photon).unwrap_err();
        assert_eq!(
            err,
            EndpointError::UnknownProfile {
                role: "photon".into(),
                profile: "ghost".into()
            }
        );
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_names_the_invalid_profile() {
        let mut bad = openrouter();
        bad.base_url = "http://evil.test/v1".into();
        let err = config_with(bad, None).validate().unwrap_err();
        assert!(matches!(&err, EndpointError::InvalidProfile { name, .. } if name == "router"));
        let mut config = config_with(openrouter(), None);
        config.profiles.insert("bad name".into(), openrouter());
        assert!(matches!(
            config.validate(),
            Err(EndpointError::InvalidProfileName { .. })
        ));
    }

    /// Review P1: `config.json` is repository data, so a cloned repo could
    /// otherwise point a credential-bearing profile at its own host and
    /// receive the user's key.
    #[test]
    fn test_credential_destination_allows_vendor_loopback_and_user_trusted_hosts() {
        let none = TrustedModelHosts::default();
        assert_eq!(
            openrouter().check_credential_destination(&none),
            Err(EndpointError::UntrustedCredentialHost {
                host: "openrouter.ai".into(),
                env: "OPENROUTER_API_KEY".into(),
                protocol: WireProtocol::OpenaiChat,
                vendor: "api.openai.com",
            })
        );
        let trusted = TrustedModelHosts::from_list(Some(" OpenRouter.ai. , ,other.test"));
        assert_eq!(openrouter().check_credential_destination(&trusted), Ok(()));

        let vendor = ModelEndpoint::anthropic_messages("https://api.anthropic.com", "m", 1);
        assert_eq!(vendor.check_credential_destination(&none), Ok(()));
        let collector = ModelEndpoint::anthropic_messages("https://collector.example", "m", 1);
        assert!(matches!(
            collector.check_credential_destination(&none),
            Err(EndpointError::UntrustedCredentialHost { .. })
        ));
        let local = ModelEndpoint::anthropic_messages("http://127.0.0.1:4010", "m", 1);
        assert_eq!(local.check_credential_destination(&none), Ok(()));
        let mut anonymous = openrouter();
        anonymous.auth = EndpointAuth::None;
        assert_eq!(anonymous.check_credential_destination(&none), Ok(()));
    }

    #[test]
    fn test_config_validate_with_refuses_an_untrusted_credential_host() {
        let config = config_with(openrouter(), Some("router"));
        let err = config
            .validate_with(&TrustedModelHosts::default())
            .unwrap_err();
        assert!(
            matches!(&err, EndpointError::InvalidProfile { name, reason }
                if name == "router" && reason.contains(TRUSTED_MODEL_HOSTS_ENV)),
            "{err}"
        );
        assert_eq!(
            config.validate_with(&TrustedModelHosts::from_list(Some("openrouter.ai"))),
            Ok(())
        );
    }

    /// Review P1 (stage 2): Ion resolves its endpoint through
    /// `validate_role`, so the credential-destination rule must hold there
    /// too, not only in `validate`.
    #[test]
    fn test_validate_role_refuses_an_untrusted_credential_host_for_ion() {
        let mut config = config_with(openrouter(), None);
        config.roles.ion = Some("router".into());
        let err = config
            .validate_role_with(EndpointRole::Ion, &TrustedModelHosts::default())
            .unwrap_err();
        assert!(
            matches!(&err, EndpointError::InvalidProfile { reason, .. }
                if reason.contains(TRUSTED_MODEL_HOSTS_ENV)),
            "{err}"
        );
        assert_eq!(
            config.validate_role_with(
                EndpointRole::Ion,
                &TrustedModelHosts::from_list(Some("openrouter.ai"))
            ),
            Ok(())
        );
    }

    /// Review P3: a URL that fails to parse can still carry userinfo.
    #[test]
    fn test_invalid_base_url_error_does_not_echo_the_url() {
        let mut e = openrouter();
        e.base_url = "https://user:secret@host:notaport/v1".into();
        let message = e.validate().unwrap_err().to_string();
        assert!(message.contains("not a valid URL"), "{message}");
        assert!(!message.contains("secret"), "{message}");
    }

    #[test]
    fn test_endpoint_error_display_carries_context() {
        let err = EndpointError::UnknownProfile {
            role: "ion".into(),
            profile: "p".into(),
        };
        assert!(err.to_string().contains("role 'ion' names profile 'p'"));
        let err = EndpointError::MissingOutputLimit {
            protocol: WireProtocol::AnthropicMessages,
        };
        assert!(err.to_string().contains("anthropic_messages"));
        assert!(EndpointError::InsecureBaseUrl { url: "u".into() }
            .to_string()
            .contains("https"));
        let err = EndpointError::UntrustedCredentialHost {
            host: "collector.example".into(),
            env: "ANTHROPIC_API_KEY".into(),
            protocol: WireProtocol::AnthropicMessages,
            vendor: "api.anthropic.com",
        }
        .to_string();
        for part in [
            "ANTHROPIC_API_KEY",
            "collector.example",
            TRUSTED_MODEL_HOSTS_ENV,
        ] {
            assert!(err.contains(part), "{err}");
        }
    }
}
