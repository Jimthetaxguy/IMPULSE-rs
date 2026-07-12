#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use security_framework::base::{Error as NativeKeychainError, Result as NativeKeychainResult};
#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::SecKeychain;
#[cfg(target_os = "macos")]
use security_framework::passwords::{
    delete_internet_password, get_internet_password, set_internet_password,
};
#[cfg(target_os = "macos")]
use security_framework_sys::base::errSecItemNotFound;
#[cfg(target_os = "macos")]
use security_framework_sys::keychain::{SecAuthenticationType, SecProtocolType};

use crate::credentials::{CredentialError, CredentialProviderType, CredentialStatus, SecretEntry};

#[cfg(target_os = "macos")]
static KEYCHAIN_INTERACTION_LOCK: Mutex<()> = Mutex::new(());

pub struct KeychainProvider {
    service_name: String,
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
struct InternetPasswordIdentity<'a> {
    server: &'a str,
    security_domain: Option<&'a str>,
    account: &'a str,
    path: &'a str,
    port: Option<u16>,
    protocol: SecProtocolType,
    authentication_type: SecAuthenticationType,
}

#[cfg(target_os = "macos")]
fn native_credential_error(key: &str, error: NativeKeychainError) -> CredentialError {
    if error.code() == errSecItemNotFound {
        CredentialError::KeyNotFound {
            key: key.into(),
            provider: "keychain".into(),
        }
    } else {
        native_command_error(error)
    }
}

#[cfg(target_os = "macos")]
fn native_command_error(error: NativeKeychainError) -> CredentialError {
    CredentialError::CommandFailed {
        provider: "keychain".into(),
        message: format!("{error} (OSStatus {})", error.code()),
    }
}

#[cfg(target_os = "macos")]
const fn interaction_guard_needed(interaction_was_allowed: bool) -> bool {
    interaction_was_allowed
}

#[cfg(target_os = "macos")]
fn with_noninteractive_keychain<T>(
    operation: impl FnOnce() -> NativeKeychainResult<T>,
) -> NativeKeychainResult<T> {
    // Keychain user-interaction policy is process-global. Serialize every
    // native operation in this module so one call cannot restore UI while
    // another call still expects it to be disabled. This mutex protects no
    // data, so recovering its guard after a panic is safe.
    let _serialized = KEYCHAIN_INTERACTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // The crate's RAII guard always enables interaction on Drop. Only create
    // it when interaction was enabled beforehand; if another component had
    // already disabled UI, leaving this as None preserves that prior state.
    let interaction_was_allowed = SecKeychain::user_interaction_allowed()?;
    let _interaction_guard = if interaction_guard_needed(interaction_was_allowed) {
        Some(SecKeychain::disable_user_interaction()?)
    } else {
        None
    };

    operation()
}

impl KeychainProvider {
    pub fn new() -> Self {
        Self {
            service_name: "com.impulse-rs".to_string(),
        }
    }

    pub fn is_available() -> bool {
        cfg!(target_os = "macos")
    }

    #[cfg(target_os = "macos")]
    fn internet_password_identity<'a>(&'a self, key: &'a str) -> InternetPasswordIdentity<'a> {
        InternetPasswordIdentity {
            server: &self.service_name,
            security_domain: None,
            account: key,
            path: "",
            port: None,
            protocol: SecProtocolType::Any,
            authentication_type: SecAuthenticationType::Default,
        }
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
        #[cfg(target_os = "macos")]
        {
            let identity = self.internet_password_identity(key);
            let bytes = with_noninteractive_keychain(|| {
                get_internet_password(
                    identity.server,
                    identity.security_domain,
                    identity.account,
                    identity.path,
                    identity.port,
                    identity.protocol,
                    identity.authentication_type,
                )
            })
            .map_err(|error| native_credential_error(key, error))?;

            String::from_utf8(bytes).map_err(|_| CredentialError::ProtocolError {
                provider: "keychain".into(),
                message: "stored credential is not valid UTF-8".into(),
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            Err(CredentialError::ProviderUnavailable(
                "macOS Keychain is only available on macOS".into(),
            ))
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), CredentialError> {
        #[cfg(target_os = "macos")]
        {
            let identity = self.internet_password_identity(key);
            with_noninteractive_keychain(|| {
                set_internet_password(
                    identity.server,
                    identity.security_domain,
                    identity.account,
                    identity.path,
                    identity.port,
                    identity.protocol,
                    identity.authentication_type,
                    value.as_bytes(),
                )
            })
            .map_err(native_command_error)
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (key, value);
            Err(CredentialError::ProviderUnavailable(
                "macOS Keychain is only available on macOS".into(),
            ))
        }
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        #[cfg(target_os = "macos")]
        {
            let identity = self.internet_password_identity(key);
            match with_noninteractive_keychain(|| {
                delete_internet_password(
                    identity.server,
                    identity.security_domain,
                    identity.account,
                    identity.path,
                    identity.port,
                    identity.protocol,
                    identity.authentication_type,
                )
            }) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == errSecItemNotFound => Ok(()),
                Err(error) => Err(native_command_error(error)),
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = key;
            Err(CredentialError::ProviderUnavailable(
                "macOS Keychain is only available on macOS".into(),
            ))
        }
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        #[cfg(target_os = "macos")]
        {
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

        #[cfg(not(target_os = "macos"))]
        {
            Err(CredentialError::ProviderUnavailable(
                "macOS Keychain is only available on macOS".into(),
            ))
        }
    }

    fn status(&self) -> CredentialStatus {
        CredentialStatus {
            provider: "keychain".to_string(),
            available: Self::is_available(),
            secrets_count: self.list().map(|l| l.len()).unwrap_or(0),
            last_error: None,
        }
    }

    fn is_available(&self) -> bool {
        Self::is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::CredentialProvider;

    #[cfg(target_os = "macos")]
    struct LiveKeychainCleanup<'a> {
        provider: &'a KeychainProvider,
        key: String,
    }

    #[cfg(target_os = "macos")]
    impl Drop for LiveKeychainCleanup<'_> {
        fn drop(&mut self) {
            let _ = self.provider.delete(&self.key);
        }
    }

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
    fn test_native_identity_matches_legacy_cli_item() {
        let provider = KeychainProvider::new();
        let identity = provider.internet_password_identity("OPENAI");

        assert_eq!(identity.server, "com.impulse-rs");
        assert_eq!(identity.security_domain, None);
        assert_eq!(identity.account, "OPENAI");
        assert_eq!(identity.path, "");
        assert_eq!(identity.port, None);
        assert_eq!(identity.protocol, SecProtocolType::Any);
        assert_eq!(identity.authentication_type, SecAuthenticationType::Default);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_native_not_found_maps_to_credential_contract() {
        let error = native_credential_error(
            "OPENAI",
            NativeKeychainError::from_code(security_framework_sys::base::errSecItemNotFound),
        );

        match error {
            CredentialError::KeyNotFound { key, provider } => {
                assert_eq!(key, "OPENAI");
                assert_eq!(provider, "keychain");
            }
            other => panic!("expected KeyNotFound, got {other:?}"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_native_failure_keeps_provider_and_security_status() {
        let error = native_credential_error(
            "OPENAI",
            NativeKeychainError::from_code(security_framework_sys::base::errSecAuthFailed),
        );

        match error {
            CredentialError::CommandFailed { provider, message } => {
                assert_eq!(provider, "keychain");
                assert!(message.contains("-25293"), "message was {message:?}");
            }
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_interaction_guard_is_only_created_when_ui_was_enabled() {
        assert!(interaction_guard_needed(true));
        assert!(!interaction_guard_needed(false));
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "wire-gated integration: mutates the signed-in user's macOS Keychain; run only with explicit approval"]
    fn integration_native_keychain_round_trip_requires_wire_gate() {
        assert_eq!(
            std::env::var("IMPULSE_RUN_LIVE_KEYCHAIN_TEST").as_deref(),
            Ok("1"),
            "explicit approval requires IMPULSE_RUN_LIVE_KEYCHAIN_TEST=1"
        );

        let provider = KeychainProvider::new();
        let key = format!(
            "impulse-native-keychain-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        let cleanup = LiveKeychainCleanup {
            provider: &provider,
            key,
        };
        let value = "test-secret-value-via-native-api";

        provider
            .set(&cleanup.key, value)
            .expect("set should succeed");
        let fetched = provider
            .get(&cleanup.key)
            .expect("get should find the value just set");
        assert_eq!(fetched, value);

        provider
            .delete(&cleanup.key)
            .expect("delete should succeed");
        match provider.get(&cleanup.key) {
            Err(CredentialError::KeyNotFound { key, provider }) => {
                assert_eq!(key, cleanup.key);
                assert_eq!(provider, "keychain");
            }
            other => panic!("expected KeyNotFound after delete, got {other:?}"),
        }
    }
}
