use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use crate::credentials::{
    CredentialError, CredentialProvider, CredentialProviderType, CredentialStatus, SecretEntry,
};

pub struct SocketProvider {
    socket_path: String,
}

impl SocketProvider {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    fn send_request(
        &self,
        command: &str,
        key: &str,
        value: Option<&str>,
    ) -> Result<String, CredentialError> {
        // Validate key doesn't contain whitespace or newlines (protocol injection prevention)
        if key.contains(|c: char| c.is_whitespace() || c == '\n' || c == '\r') {
            return Err(CredentialError::ProtocolError {
                provider: "socket".into(),
                message: "Key must not contain whitespace or newline characters".into(),
            });
        }
        // Validate value doesn't contain newlines (command injection prevention)
        if let Some(v) = value {
            if v.contains('\n') || v.contains('\r') {
                return Err(CredentialError::ProtocolError {
                    provider: "socket".into(),
                    message: "Value must not contain newline characters".into(),
                });
            }
        }

        let mut stream = UnixStream::connect(&self.socket_path).map_err(|_| {
            CredentialError::ProviderUnavailable(format!(
                "Failed to connect to credential agent at {}",
                self.socket_path
            ))
        })?;

        let request = match value {
            Some(v) => format!("{} {} {}\n", command, key, v),
            None => format!("{} {}\n", command, key),
        };

        stream.write_all(request.as_bytes())?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;

        if response.starts_with("ERROR:") {
            Err(CredentialError::CommandFailed {
                provider: "socket".into(),
                message: response.trim_start_matches("ERROR:").trim().to_string(),
            })
        } else {
            Ok(response.trim().to_string())
        }
    }
}

impl CredentialProvider for SocketProvider {
    fn name(&self) -> &str {
        "socket"
    }

    fn provider_type(&self) -> CredentialProviderType {
        CredentialProviderType::Socket
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.send_request("GET", key, None)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), CredentialError> {
        self.send_request("SET", key, Some(value)).map(|_| ())
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.send_request("DELETE", key, None).map(|_| ())
    }

    fn list(&self) -> Result<Vec<SecretEntry>, CredentialError> {
        let response = self.send_request("LIST", "", None)?;
        let mut secrets = Vec::new();

        for line in response.lines() {
            if !line.is_empty() {
                secrets.push(SecretEntry {
                    key: line.to_string(),
                    provider: "socket".to_string(),
                    created_at: chrono::Utc::now(),
                    last_accessed: None,
                });
            }
        }

        Ok(secrets)
    }

    fn status(&self) -> CredentialStatus {
        let available = UnixStream::connect(&self.socket_path).is_ok();
        CredentialStatus {
            provider: "socket".to_string(),
            available,
            secrets_count: 0,
            last_error: if !available {
                Some("Socket not connected".to_string())
            } else {
                None
            },
        }
    }

    fn is_available(&self) -> bool {
        UnixStream::connect(&self.socket_path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for socket credential provider.
    //!
    //! Key strategy: `send_request` validates keys/values BEFORE attempting socket
    //! connection, so tests using a non-existent socket path still exercise the
    //! validation paths and return `ProtocolError` rather than `ProviderUnavailable`.
    //! This lets us test injection prevention without spinning up a real server.
    use super::*;
    use crate::credentials::CredentialError;
    use proptest::prelude::*;

    fn provider() -> SocketProvider {
        // Deliberately non-existent socket — validation fires before connect.
        SocketProvider::new("/nonexistent/impulse-test-socket".to_string())
    }

    fn is_protocol_error(result: &Result<String, CredentialError>) -> bool {
        matches!(result, Err(CredentialError::ProtocolError { .. }))
    }

    fn is_protocol_error_unit(result: &Result<(), CredentialError>) -> bool {
        matches!(result, Err(CredentialError::ProtocolError { .. }))
    }

    #[test]
    fn test_get_rejects_key_with_space() {
        let p = provider();
        let result = p.get("has space");
        assert!(
            is_protocol_error(&result),
            "expected ProtocolError, got {:?}",
            result
        );
    }

    #[test]
    fn test_get_rejects_key_with_newline() {
        let p = provider();
        let result = p.get("key\nwith-newline");
        assert!(is_protocol_error(&result));
    }

    #[test]
    fn test_get_rejects_key_with_carriage_return() {
        let p = provider();
        let result = p.get("key\rwith-cr");
        assert!(is_protocol_error(&result));
    }

    #[test]
    fn test_get_rejects_key_with_tab() {
        let p = provider();
        let result = p.get("key\twith-tab");
        assert!(is_protocol_error(&result));
    }

    #[test]
    fn test_set_rejects_value_with_newline() {
        let p = provider();
        let result = p.set("valid_key", "value\nwith-newline");
        assert!(is_protocol_error_unit(&result));
    }

    #[test]
    fn test_set_rejects_value_with_carriage_return() {
        let p = provider();
        let result = p.set("valid_key", "value\rwith-cr");
        assert!(is_protocol_error_unit(&result));
    }

    #[test]
    fn test_delete_rejects_whitespace_key() {
        let p = provider();
        let result = p.delete(" key_with_leading_space");
        assert!(is_protocol_error_unit(&result));
    }

    #[test]
    fn test_error_message_identifies_key_injection() {
        let p = provider();
        match p.get("bad\nkey") {
            Err(CredentialError::ProtocolError { provider, message }) => {
                assert_eq!(provider, "socket");
                assert!(
                    message.contains("Key must not contain"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected ProtocolError, got {:?}", other),
        }
    }

    #[test]
    fn test_error_message_identifies_value_injection() {
        let p = provider();
        match p.set("good_key", "bad\nvalue") {
            Err(CredentialError::ProtocolError { provider, message }) => {
                assert_eq!(provider, "socket");
                assert!(
                    message.contains("Value must not contain"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected ProtocolError, got {:?}", other),
        }
    }

    #[test]
    fn test_valid_key_attempts_connection_and_fails_with_unavailable() {
        // Clean ASCII key + no socket → should reach connect step and fail with
        // ProviderUnavailable, confirming validation passed.
        let p = provider();
        let result = p.get("valid_key_123");
        assert!(
            matches!(result, Err(CredentialError::ProviderUnavailable(_))),
            "expected ProviderUnavailable (validation passed, connect failed), got {:?}",
            result
        );
    }

    #[test]
    fn test_provider_name_and_type() {
        let p = provider();
        assert_eq!(p.name(), "socket");
        assert_eq!(p.provider_type(), CredentialProviderType::Socket);
    }

    #[test]
    fn test_status_when_socket_unavailable() {
        let p = provider();
        let status = p.status();
        assert_eq!(status.provider, "socket");
        assert!(!status.available);
        assert!(status.last_error.is_some());
        assert_eq!(status.last_error.as_deref(), Some("Socket not connected"));
    }

    #[test]
    fn test_is_available_false_when_no_socket() {
        let p = provider();
        assert!(!p.is_available());
    }

    #[test]
    fn test_null_byte_in_key_passes_validation_but_fails_connect() {
        // Null byte is not whitespace/newline/CR per the filter — validation passes,
        // connect fails. This documents current behavior; if null-byte filtering is
        // added later, update this test.
        let p = provider();
        let result = p.get("key\0with-null");
        assert!(matches!(
            result,
            Err(CredentialError::ProviderUnavailable(_))
        ));
    }

    proptest! {
        // Property 1: any key containing whitespace/newline/CR is rejected.
        #[test]
        fn prop_keys_with_whitespace_or_newline_rejected(
            prefix in "[a-zA-Z0-9_]{0,20}",
            bad in prop::sample::select(vec![" ", "\t", "\n", "\r", "\r\n", "  "]),
            suffix in "[a-zA-Z0-9_]{0,20}",
        ) {
            let key = format!("{prefix}{bad}{suffix}");
            let p = provider();
            let result = p.get(&key);
            prop_assert!(
                is_protocol_error(&result),
                "key {:?} should be rejected but got {:?}",
                key, result
            );
        }

        // Property 2: any value containing newline or CR is rejected on set.
        #[test]
        fn prop_values_with_newline_or_cr_rejected(
            prefix in "[a-zA-Z0-9 _!@#$%^&*()=-]{0,50}",
            bad in prop::sample::select(vec!["\n", "\r", "\r\n"]),
            suffix in "[a-zA-Z0-9 _!@#$%^&*()=-]{0,50}",
        ) {
            let value = format!("{prefix}{bad}{suffix}");
            let p = provider();
            let result = p.set("valid_key", &value);
            prop_assert!(
                is_protocol_error_unit(&result),
                "value {:?} should be rejected but got {:?}",
                value, result
            );
        }

        // Property 3: clean ASCII keys (no whitespace) pass validation and hit connect path.
        #[test]
        fn prop_clean_keys_reach_connect_stage(
            key in "[a-zA-Z0-9_.\\-]{1,64}",
        ) {
            let p = provider();
            let result = p.get(&key);
            // Clean keys should NOT hit ProtocolError — should reach connect, which fails
            // with ProviderUnavailable since socket is fake.
            prop_assert!(
                !is_protocol_error(&result),
                "clean key {:?} incorrectly rejected: {:?}",
                key, result
            );
        }

        // Property 4: round-trip — validation decision depends solely on content.
        // Same key goes through GET, SET, DELETE consistently.
        #[test]
        fn prop_validation_consistent_across_operations(
            key in "[a-zA-Z0-9_]{1,30}",
        ) {
            let p = provider();
            let get_ok = !is_protocol_error(&p.get(&key));
            let set_ok = !is_protocol_error_unit(&p.set(&key, "value"));
            let del_ok = !is_protocol_error_unit(&p.delete(&key));
            prop_assert_eq!(get_ok, set_ok);
            prop_assert_eq!(set_ok, del_ok);
        }
    }
}
