use std::io::Write;
use std::process::{Command, Stdio};

use crate::credentials::{CredentialError, CredentialProviderType, CredentialStatus, SecretEntry};

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

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        let output = Command::new("security")
            .args([
                "find-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
                "-w",
            ])
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(CredentialError::KeyNotFound {
                key: key.into(),
                provider: "keychain".into(),
            })
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), CredentialError> {
        // `-w value` (the secret as a bare CLI argument) is visible to any
        // other local process via `ps aux`/`ps -ef` for the brief window
        // this child runs -- `security add-internet-password -h`'s own
        // usage text warns "Use of the -p or -w options is insecure.
        // Specify -w as the last option to be prompted." Prompted mode
        // reads the password from stdin (via getpass) instead of argv, and
        // asks for it twice (entry + confirmation) -- verified this works
        // non-interactively by piping both copies, newline-separated, and
        // closing stdin before reading output.
        let mut child = Command::new("security")
            .args([
                "add-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
                "-U",
                "-w", // no value follows -- this is what triggers the prompt
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // `-w` alone prompts twice: once to enter the password, once to
        // confirm it. Feeding the same value both times non-interactively
        // satisfies both prompts without ever putting the secret in argv.
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{value}")?;
            writeln!(stdin, "{value}")?;
            // Drop closes stdin, matching EOF for a real interactive
            // session that finished typing.
        }

        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err(CredentialError::CommandFailed {
                provider: "keychain".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        let output = Command::new("security")
            .args([
                "delete-internet-password",
                "-s",
                &self.service_name,
                "-a",
                key,
            ])
            .output()?;

        if output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("could not find")
        {
            Ok(())
        } else {
            Err(CredentialError::CommandFailed {
                provider: "keychain".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        let output = Command::new("security")
            .args(["find-internet-password", "-s", &self.service_name])
            .output()?;

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

    #[test]
    #[cfg(target_os = "macos")]
    fn test_set_get_delete_round_trips_via_stdin_prompt_not_argv() {
        // Regression test for the secret-as-argv leak fix: `set()` now
        // pipes the value via stdin to `security`'s interactive `-w`
        // prompt (entered twice) instead of passing it as `-w <value>`.
        // This proves the new mechanism actually works end-to-end (set,
        // then get returns the same value, then delete removes it) --
        // proving the *absence* of the value in argv from inside a unit
        // test isn't practical (would need to race a `ps` snapshot against
        // the child's lifetime), so this asserts the replacement mechanism
        // is functionally correct instead.
        let provider = KeychainProvider::new();
        let key = format!("test-stdin-prompt-{}", std::process::id());
        let value = "test-secret-value-via-stdin-prompt";

        // Clean up any stale entry from a prior interrupted run first.
        let _ = provider.delete(&key);

        provider.set(&key, value).expect("set should succeed");
        let fetched = provider
            .get(&key)
            .expect("get should find the value just set");
        assert_eq!(fetched, value);

        provider.delete(&key).expect("delete should succeed");
        assert!(provider.get(&key).is_err(), "get should fail after delete");
    }
}
