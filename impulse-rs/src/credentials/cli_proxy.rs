use std::process::Command;
use std::time::Duration;

use crate::credentials::{
    CredentialError, CredentialProvider, CredentialProviderType, CredentialStatus, SecretEntry,
};
use crate::process_util::run_with_timeout;

/// Hard timeout for a secrets-manager CLI call. These fetches are network-backed
/// (Infisical/Doppler/Vault/1Password), so a hung backend must not block the
/// caller — or the daemon — indefinitely.
const CLI_PROXY_TIMEOUT: Duration = Duration::from_secs(20);

pub struct CliProxyProvider {
    cli_tool: String,
    provider_url: Option<String>,
}

impl CliProxyProvider {
    pub fn new(cli_tool: String, provider_url: Option<String>) -> Self {
        Self {
            cli_tool,
            provider_url,
        }
    }

    fn run_cli(&self, args: &[&str]) -> Result<String, CredentialError> {
        let mut command = Command::new(&self.cli_tool);
        command.args(args);
        let output = run_with_timeout(command, CLI_PROXY_TIMEOUT)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(CredentialError::CommandFailed {
                provider: self.cli_tool.clone(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    /// A key is passed to the CLI as an argument, so one that starts with
    /// `-` would be parsed as a flag (`--help`, or an output option) rather
    /// than a secret name. No secrets manager names a secret that way.
    fn validate_key(&self, key: &str) -> Result<(), CredentialError> {
        let reason = if key.trim().is_empty() {
            "must not be empty"
        } else if key.starts_with('-') {
            "must not start with '-', which the CLI would read as a flag"
        } else if key.chars().any(char::is_control) {
            "must not contain control characters"
        } else {
            return Ok(());
        };
        Err(CredentialError::InvalidKey {
            provider: self.cli_tool.clone(),
            reason: reason.to_string(),
        })
    }

    pub fn is_tool_available(&self) -> bool {
        let mut command = Command::new(&self.cli_tool);
        command.arg("--version");
        run_with_timeout(command, CLI_PROXY_TIMEOUT)
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl CredentialProvider for CliProxyProvider {
    fn name(&self) -> &str {
        &self.cli_tool
    }

    fn provider_type(&self) -> CredentialProviderType {
        CredentialProviderType::CliProxy
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.validate_key(key)?;
        match self.cli_tool.as_str() {
            "infisical" => {
                let mut args = vec!["secrets", "get", key];
                if let Some(project) = &self.provider_url {
                    args.push("--project");
                    args.push(project);
                }
                let output = self.run_cli(&args)?;
                Ok(output
                    .lines()
                    .find(|l| !l.is_empty())
                    .unwrap_or(&output)
                    .to_string())
            }
            "doppler" => {
                let output = self.run_cli(&["secrets", "get", key, "--json"])?;
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
                    json.get("value")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| CredentialError::CommandFailed {
                            provider: "doppler".into(),
                            message: "Invalid Doppler response: missing 'value' field".into(),
                        })
                } else {
                    Ok(output)
                }
            }
            "vault" => {
                let base_path = self
                    .provider_url
                    .as_deref()
                    .unwrap_or("secret/data/impulse");
                let field_flag = format!("-field={}", key);
                let output = self.run_cli(&["kv", "get", &field_flag, base_path])?;
                Ok(output.trim().to_string())
            }
            "op" | "1password" => {
                let output = self.run_cli(&["get", "item", key])?;
                Ok(output)
            }
            _ => Err(CredentialError::NotSupported(format!(
                "Unsupported CLI tool: {}",
                self.cli_tool
            ))),
        }
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), CredentialError> {
        Err(CredentialError::NotSupported(format!(
            "Setting secrets via {} not supported. Use the CLI directly.",
            self.cli_tool
        )))
    }

    fn delete(&self, _key: &str) -> Result<(), CredentialError> {
        Err(CredentialError::NotSupported(format!(
            "Deleting secrets via {} not supported. Use the CLI directly.",
            self.cli_tool
        )))
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        match self.cli_tool.as_str() {
            "infisical" => {
                let mut args = vec!["secrets", "list"];
                if let Some(project) = &self.provider_url {
                    args.push("--project");
                    args.push(project);
                }
                let output = self.run_cli(&args)?;
                Ok(output
                    .lines()
                    .skip(1)
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split('\t').collect();
                        parts.first().map(|k| SecretEntry {
                            key: k.to_string(),
                            provider: "infisical".to_string(),
                            created_at: chrono::Utc::now(),
                            last_accessed: None,
                        })
                    })
                    .collect())
            }
            "doppler" => {
                let output = self.run_cli(&["secrets", "list", "--json"])?;
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
                    if let Some(arr) = json.as_array() {
                        return Ok(arr
                            .iter()
                            .filter_map(|v| v.get("key").and_then(|k| k.as_str()))
                            .map(|k| SecretEntry {
                                key: k.to_string(),
                                provider: "doppler".to_string(),
                                created_at: chrono::Utc::now(),
                                last_accessed: None,
                            })
                            .collect());
                    }
                }
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn status(&self) -> CredentialStatus {
        let available = self.is_tool_available();
        let secrets = if available {
            self.list()
        } else {
            Ok(Vec::new())
        };

        CredentialStatus {
            provider: self.cli_tool.clone(),
            available,
            secrets_count: secrets.map(|s| s.len()).unwrap_or(0),
            last_error: if !available {
                Some(format!("{} CLI not found", self.cli_tool))
            } else {
                None
            },
        }
    }

    fn supports_oauth(&self) -> bool {
        matches!(self.cli_tool.as_str(), "infisical" | "doppler")
    }

    fn is_available(&self) -> bool {
        self.is_tool_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_refuses_a_key_the_cli_would_read_as_a_flag() {
        // The tool name never runs: validation comes first.
        let provider = CliProxyProvider::new("infisical".into(), None);
        for key in ["--help", "-o", "", "   ", "bad\nkey"] {
            match provider.get(key) {
                Err(CredentialError::InvalidKey { provider, reason }) => {
                    assert_eq!(provider, "infisical");
                    assert!(!reason.is_empty());
                }
                other => panic!("key {key:?} must be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_invalid_key_display_names_the_provider_and_reason() {
        let err = CredentialError::InvalidKey {
            provider: "doppler".into(),
            reason: "must not be empty".into(),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("doppler"), "{rendered}");
        assert!(rendered.contains("must not be empty"), "{rendered}");
    }

    #[test]
    fn test_get_with_an_unsupported_tool_is_not_supported() {
        let provider = CliProxyProvider::new("not-a-secrets-cli".into(), None);
        assert!(matches!(
            provider.get("API_TOKEN"),
            Err(CredentialError::NotSupported(_))
        ));
    }

    #[test]
    fn test_set_and_delete_are_not_supported() {
        let provider = CliProxyProvider::new("vault".into(), None);
        assert!(matches!(
            provider.set("k", "v"),
            Err(CredentialError::NotSupported(_))
        ));
        assert!(matches!(
            provider.delete("k"),
            Err(CredentialError::NotSupported(_))
        ));
        assert_eq!(provider.name(), "vault");
        assert_eq!(provider.provider_type(), CredentialProviderType::CliProxy);
    }

    #[test]
    fn test_a_missing_cli_binary_is_unavailable_and_errors_instead_of_panicking() {
        let missing = CliProxyProvider::new("/nonexistent/impulse-test/op".into(), None);
        assert!(!missing.is_tool_available());
        assert!(missing.run_cli(&["get", "item", "x"]).is_err());
    }
}
