use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use crate::credentials::{
    CredentialProvider, CredentialProviderType, CredentialStatus, SecretEntry,
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
    ) -> Result<String, String> {
        // Validate key doesn't contain whitespace or newlines (protocol injection prevention)
        if key.contains(|c: char| c.is_whitespace() || c == '\n' || c == '\r') {
            return Err("Key must not contain whitespace or newline characters".to_string());
        }
        // Validate value doesn't contain newlines (command injection prevention)
        if let Some(v) = value {
            if v.contains('\n') || v.contains('\r') {
                return Err("Value must not contain newline characters".to_string());
            }
        }

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Failed to connect to credential agent: {}", e))?;

        let request = match value {
            Some(v) => format!("{} {} {}\n", command, key, v),
            None => format!("{} {}\n", command, key),
        };

        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("Failed to send request: {}", e))?;

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if response.starts_with("ERROR:") {
            Err(response.trim_start_matches("ERROR:").to_string())
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

    fn get(&self, key: &str) -> Result<String, String> {
        self.send_request("GET", key, None)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.send_request("SET", key, Some(value)).map(|_| ())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.send_request("DELETE", key, None).map(|_| ())
    }

    fn list(&self) -> Result<Vec<SecretEntry>, String> {
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
