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
        match self.cli_tool.as_str() {
            "infisical" => {
                let mut args = vec!["secrets", "get", key];
                if let Some(project) = &self.provider_url {
                    args.push("--project");
                    args.push(project);
                }
                let output = self.run_cli(&args.to_vec())?;
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
                let output = self.run_cli(&args.to_vec())?;
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
