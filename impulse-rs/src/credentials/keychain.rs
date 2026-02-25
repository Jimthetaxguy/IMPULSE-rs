use std::process::Command;

use crate::credentials::{CredentialProviderType, CredentialStatus, SecretEntry};

pub struct KeychainProvider {
    service_name: String,
}

impl KeychainProvider {
    pub fn new() -> Self {
        Self {
            service_name: "com.impulse-rs".to_string(),
        }
    }

    pub fn is_available() -> bool {
        // Use help which always works
        Command::new("security")
            .arg("help")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Default for KeychainProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::credentials::CredentialProvider for KeychainProvider {
    fn name(&self) -> &str {
        "keychain"
    }

    fn provider_type(&self) -> CredentialProviderType {
        CredentialProviderType::Keychain
    }

    fn get(&self, key: &str) -> Result<String, String> {
        let output = Command::new("security")
            .args([
                "find-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
                "-w",
            ])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(format!(
                "Key not found: {} (error: {})",
                key,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let output = Command::new("security")
            .args([
                "add-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
                "-w",
                value,
                "-U",
            ])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Failed to store key: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let output = Command::new("security")
            .args([
                "delete-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
            ])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("could not find")
        {
            Ok(())
        } else {
            Err(format!(
                "Failed to delete key: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn list(&self) -> Result<Vec<SecretEntry>, String> {
        let output = Command::new("security")
            .args(["find-internet-password", "-s", &self.service_name])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut secrets = Vec::new();

        for line in output_str.lines() {
            if let Some(account) = line.strip_prefix("account: ") {
                secrets.push(SecretEntry {
                    key: account.to_string(),
                    provider: "keychain".to_string(),
                    created_at: chrono::Utc::now(),
                    last_accessed: None,
                });
            }
        }

        Ok(secrets)
    }

    fn status(&self) -> CredentialStatus {
        CredentialStatus {
            provider: "keychain".to_string(),
            available: Self::is_available(),
            secrets_count: self.list().map(|l| l.len()).unwrap_or(0),
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::CredentialProvider;

    #[test]
    #[cfg(target_os = "macos")]
    fn test_keychain_available() {
        let available = KeychainProvider::is_available();
        assert!(available, "Keychain should be available on macOS");
    }

    #[test]
    fn test_provider_name() {
        let provider = KeychainProvider::new();
        assert_eq!(provider.name(), "keychain");
    }
}
