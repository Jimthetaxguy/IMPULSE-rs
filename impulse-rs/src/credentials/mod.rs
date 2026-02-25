use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod cli_proxy;
pub mod keychain;
pub mod socket;

/// Unified trait for all credential providers
pub trait CredentialProvider: Send + Sync {
    /// Provider name (e.g., "keychain", "infisical")
    fn name(&self) -> &str;

    /// Provider type
    fn provider_type(&self) -> CredentialProviderType;

    /// Get a secret by key
    fn get(&self, key: &str) -> Result<String, String>;

    /// Set a secret
    fn set(&self, key: &str, value: &str) -> Result<(), String>;

    /// Delete a secret
    fn delete(&self, key: &str) -> Result<(), String>;

    /// List all secrets (without values)
    fn list(&self) -> Result<Vec<SecretEntry>, String>;

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

    fn get(&self, key: &str) -> Result<String, String> {
        let env_key = format!("{}_API_KEY", key.to_uppercase());
        std::env::var(&env_key).map_err(|_| format!("Environment variable {} not set", env_key))
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Err("Cannot set environment variables at runtime".to_string())
    }

    fn delete(&self, _key: &str) -> Result<(), String> {
        Err("Cannot delete environment variables".to_string())
    }

    fn list(&self) -> Result<Vec<SecretEntry>, String> {
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

    fn get(&self, key: &str) -> Result<String, String> {
        self.secrets
            .read()
            .map_err(|e| e.to_string())?
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Key not found in memory: {}", key))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.secrets
            .write()
            .map_err(|e| e.to_string())?
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.secrets.write().map_err(|e| e.to_string())?.remove(key);
        Ok(())
    }

    fn list(&self) -> Result<Vec<SecretEntry>, String> {
        let secrets = self.secrets.read().map_err(|e| e.to_string())?;
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
