//! Unified credential abstraction over 5 backends.
//!
//! Provides the [`CredentialProvider`] trait with implementations for
//! keychain (macOS), Unix socket agent, CLI proxy (Infisical/Doppler/Vault),
//! environment variables, and in-memory session storage.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

pub mod cli_proxy;
pub mod keychain;
pub mod socket;

/// Structured error type for credential operations.
#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Key not found: {key} (provider: {provider})")]
    KeyNotFound { key: String, provider: String },

    #[error("Provider unavailable: {0}")]
    ProviderUnavailable(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),

    #[error("Lock poisoned in {provider}")]
    PoisonedLock { provider: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Command failed ({provider}): {message}")]
    CommandFailed { provider: String, message: String },

    #[error("Protocol error ({provider}): {message}")]
    ProtocolError { provider: String, message: String },
}

/// Unified trait for all credential providers
pub trait CredentialProvider: Send + Sync {
    /// Provider name (e.g., "keychain", "infisical")
    fn name(&self) -> &str;

    /// Provider type
    fn provider_type(&self) -> CredentialProviderType;

    /// Get a secret by key
    fn get(&self, key: &str) -> Result<String, CredentialError>;

    /// Set a secret
    fn set(&self, key: &str, value: &str) -> Result<(), CredentialError>;

    /// Delete a secret
    fn delete(&self, key: &str) -> Result<(), CredentialError>;

    /// List all secrets (without values)
    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError>;

    /// Get provider status
    fn status(&self) -> CredentialStatus;

    /// Check if provider supports OAuth/refresh tokens
    fn supports_oauth(&self) -> bool {
        false
    }

    /// Check if provider is available (installed, configured, etc.)
    fn is_available(&self) -> bool {
        true
    }
}

/// Create a provider based on config
pub fn create_provider(config: &CredentialConfig) -> Box<dyn CredentialProvider> {
    match config.provider {
        CredentialProviderType::Keychain => {
            Box::new(crate::credentials::keychain::KeychainProvider::new())
        }
        CredentialProviderType::Socket => {
            let socket = config.socket_path.clone().unwrap_or_else(|| {
                let new_path = "/tmp/impulse-credentials.sock";
                // DEPRECATED(2026-04-01): Remove cockpit-credentials.sock fallback.
                // The Cockpit→Impulse rename shipped 2026-02-24. After 2026-04-01,
                // remove this block and use only impulse-credentials.sock.
                let old_path = "/tmp/cockpit-credentials.sock";
                if std::path::Path::new(old_path).exists()
                    && !std::path::Path::new(new_path).exists()
                {
                    eprintln!("Warning: using legacy cockpit-credentials.sock. Rename to impulse-credentials.sock to silence this warning.");
                    old_path.to_string()
                } else {
                    new_path.to_string()
                }
            });
            Box::new(crate::credentials::socket::SocketProvider::new(socket))
        }
        CredentialProviderType::CliProxy => {
            Box::new(crate::credentials::cli_proxy::CliProxyProvider::new(
                config
                    .cli_tool
                    .clone()
                    .unwrap_or_else(|| "infisical".to_string()),
                config.provider_url.clone(),
            ))
        }
        CredentialProviderType::Env => Box::new(EnvProvider::new()),
        CredentialProviderType::Memory => Box::new(MemoryProvider::new()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CredentialProviderType {
    Keychain,
    Socket,
    CliProxy,
    Env,
    Memory,
}

impl CredentialProviderType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "keychain" => Some(Self::Keychain),
            "socket" | "agent" => Some(Self::Socket),
            "cliproxy" | "cli" | "infisical" | "doppler" | "vault" => Some(Self::CliProxy),
            "env" | "environment" => Some(Self::Env),
            "memory" | "session" => Some(Self::Memory),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::Socket => "socket",
            Self::CliProxy => "cliproxy",
            Self::Env => "env",
            Self::Memory => "memory",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialConfig {
    pub provider: CredentialProviderType,
    pub cli_tool: Option<String>,
    pub socket_path: Option<String>,
    pub provider_url: Option<String>,
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            provider: CredentialProviderType::Env,
            cli_tool: None,
            socket_path: None,
            provider_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretEntry {
    pub key: String,
    pub provider: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_accessed: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStatus {
    pub provider: String,
    pub available: bool,
    pub secrets_count: usize,
    pub last_error: Option<String>,
}

/// Environment variable-based provider (legacy fallback)
pub struct EnvProvider;

impl EnvProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for EnvProvider {
    fn name(&self) -> &str {
        "env"
    }

    fn provider_type(&self) -> CredentialProviderType {
        CredentialProviderType::Env
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        let env_key = format!("{}_API_KEY", key.to_uppercase());
        std::env::var(&env_key).map_err(|_| CredentialError::KeyNotFound {
            key: env_key,
            provider: "env".into(),
        })
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), CredentialError> {
        Err(CredentialError::NotSupported(
            "Cannot set environment variables at runtime".into(),
        ))
    }

    fn delete(&self, _key: &str) -> Result<(), CredentialError> {
        Err(CredentialError::NotSupported(
            "Cannot delete environment variables".into(),
        ))
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        let keys = [
            "ANTHROPIC",
            "OPENAI",
            "MINIMAX",
            "GOOGLE",
            "MISTRAL",
            "COHERE",
        ];
        let mut secrets = Vec::new();

        for key in keys {
            let env_key = format!("{}_API_KEY", key);
            if std::env::var(&env_key).is_ok() {
                secrets.push(SecretEntry {
                    key: key.to_string(),
                    provider: "env".to_string(),
                    created_at: chrono::Utc::now(),
                    last_accessed: None,
                });
            }
        }

        Ok(secrets)
    }

    fn status(&self) -> CredentialStatus {
        let secrets = self.list().unwrap_or_default();
        CredentialStatus {
            provider: "env".to_string(),
            available: true,
            secrets_count: secrets.len(),
            last_error: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// Memory-only provider (session-based, most air-gapped)
pub struct MemoryProvider {
    secrets: std::sync::RwLock<HashMap<String, String>>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self {
            secrets: std::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for MemoryProvider {
    fn name(&self) -> &str {
        "memory"
    }

    fn provider_type(&self) -> CredentialProviderType {
        CredentialProviderType::Memory
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.secrets
            .read()
            .map_err(|_| CredentialError::PoisonedLock {
                provider: "memory".into(),
            })?
            .get(key)
            .cloned()
            .ok_or_else(|| CredentialError::KeyNotFound {
                key: key.into(),
                provider: "memory".into(),
            })
    }

    fn set(&self, key: &str, value: &str) -> Result<(), CredentialError> {
        self.secrets
            .write()
            .map_err(|_| CredentialError::PoisonedLock {
                provider: "memory".into(),
            })?
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.secrets
            .write()
            .map_err(|_| CredentialError::PoisonedLock {
                provider: "memory".into(),
            })?
            .remove(key);
        Ok(())
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        let secrets = self
            .secrets
            .read()
            .map_err(|_| CredentialError::PoisonedLock {
                provider: "memory".into(),
            })?;
        Ok(secrets
            .keys()
            .map(|k| SecretEntry {
                key: k.clone(),
                provider: "memory".to_string(),
                created_at: chrono::Utc::now(),
                last_accessed: None,
            })
            .collect())
    }

    fn status(&self) -> CredentialStatus {
        let secrets = self.list().unwrap_or_default();
        CredentialStatus {
            provider: "memory".to_string(),
            available: true,
            secrets_count: secrets.len(),
            last_error: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_provider_crud_cycle() {
        let provider = MemoryProvider::new();
        assert!(provider.get("key1").is_err());

        provider.set("key1", "value1").unwrap();
        assert_eq!(provider.get("key1").unwrap(), "value1");

        provider.set("key1", "value2").unwrap();
        assert_eq!(provider.get("key1").unwrap(), "value2");

        provider.delete("key1").unwrap();
        assert!(provider.get("key1").is_err());
    }

    #[test]
    fn test_memory_provider_list() {
        let provider = MemoryProvider::new();
        provider.set("a", "1").unwrap();
        provider.set("b", "2").unwrap();
        let entries = provider.list().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_memory_provider_status() {
        let provider = MemoryProvider::new();
        provider.set("x", "y").unwrap();
        let status = provider.status();
        assert!(status.available);
        assert_eq!(status.secrets_count, 1);
        assert_eq!(status.provider, "memory");
    }

    #[test]
    fn test_env_provider_get_missing_key() {
        let provider = EnvProvider::new();
        let result = provider.get("nonexistent_test_key_xyz");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::KeyNotFound { .. }
        ));
    }

    #[test]
    fn test_env_provider_set_not_supported() {
        let provider = EnvProvider::new();
        let result = provider.set("key", "value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CredentialError::NotSupported(_)
        ));
    }

    #[test]
    fn test_credential_provider_type_parse_all_aliases() {
        assert_eq!(
            CredentialProviderType::parse("keychain"),
            Some(CredentialProviderType::Keychain)
        );
        assert_eq!(
            CredentialProviderType::parse("socket"),
            Some(CredentialProviderType::Socket)
        );
        assert_eq!(
            CredentialProviderType::parse("agent"),
            Some(CredentialProviderType::Socket)
        );
        assert_eq!(
            CredentialProviderType::parse("env"),
            Some(CredentialProviderType::Env)
        );
        assert_eq!(
            CredentialProviderType::parse("environment"),
            Some(CredentialProviderType::Env)
        );
        assert_eq!(
            CredentialProviderType::parse("memory"),
            Some(CredentialProviderType::Memory)
        );
        assert_eq!(
            CredentialProviderType::parse("infisical"),
            Some(CredentialProviderType::CliProxy)
        );
        assert_eq!(CredentialProviderType::parse("unknown"), None);
    }

    #[test]
    fn test_credential_provider_type_as_str_roundtrip() {
        for provider_type in [
            CredentialProviderType::Keychain,
            CredentialProviderType::Socket,
            CredentialProviderType::CliProxy,
            CredentialProviderType::Env,
            CredentialProviderType::Memory,
        ] {
            let s = provider_type.as_str();
            assert_eq!(CredentialProviderType::parse(s), Some(provider_type));
        }
    }

    #[test]
    fn test_create_provider_routes_correctly() {
        let config = CredentialConfig {
            provider: CredentialProviderType::Memory,
            cli_tool: None,
            socket_path: None,
            provider_url: None,
        };
        let provider = create_provider(&config);
        assert_eq!(provider.name(), "memory");

        let config = CredentialConfig {
            provider: CredentialProviderType::Env,
            cli_tool: None,
            socket_path: None,
            provider_url: None,
        };
        let provider = create_provider(&config);
        assert_eq!(provider.name(), "env");
    }

    #[test]
    fn test_credential_config_default() {
        let config = CredentialConfig::default();
        assert_eq!(config.provider, CredentialProviderType::Env);
        assert!(config.cli_tool.is_none());
    }
}
