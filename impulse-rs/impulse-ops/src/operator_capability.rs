//! Shared discovery of the daemon run's operator capability (ADR-0018).
//!
//! The daemon publishes a per-run capability beside its socket. Two independent
//! clients need to find and validate it — the CLI/TUI `DaemonClient` in the root
//! crate and the Dioxus cockpit's own socket client in `impulse-desktop` — and
//! neither depends on the other. This module is the one place that knows where
//! the file lives, what a well-formed token looks like, and that an explicit
//! environment override wins over the file.
//!
//! Minting, writing, connection classification, and the constant-time
//! comparison stay with the daemon (`impulse-rs/src/daemon/actor_provenance.rs`);
//! only the client-side half lives here.

use std::path::{Path, PathBuf};

/// Environment variable a client may use to present the capability instead of
/// reading the file. Governed panes never receive it: every inherited
/// `IMPULSE_*` key is scrubbed before a runtime is spawned.
pub const OPERATOR_CAPABILITY_ENV: &str = "IMPULSE_OPERATOR_CAPABILITY";

/// Extension applied to the socket path to locate the capability file, matching
/// how the daemon's PID file is placed (`impulse.sock` -> `impulse.operator-cap`).
pub const OPERATOR_CAPABILITY_EXTENSION: &str = "operator-cap";

/// Hex characters in a well-formed capability token (32 random bytes).
pub const OPERATOR_CAPABILITY_HEX_LEN: usize = 64;

/// Where the daemon listening on `socket_path` publishes its capability.
pub fn path_for_socket(socket_path: &Path) -> PathBuf {
    socket_path.with_extension(OPERATOR_CAPABILITY_EXTENSION)
}

/// Validate and normalize a token read from a file or the environment.
///
/// Returns `None` for anything that is not exactly
/// [`OPERATOR_CAPABILITY_HEX_LEN`] lowercase hexadecimal characters after
/// trimming, so a truncated or half-written file is never presented as if it
/// were a capability.
pub fn parse_token(raw: &str) -> Option<String> {
    let token = raw.trim();
    if token.len() != OPERATOR_CAPABILITY_HEX_LEN
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some(token.to_string())
}

/// Resolve a capability for the daemon listening on `socket_path`, preferring an
/// explicit environment override over the published file.
///
/// `None` means no capability is reachable — a launched governed pane, whose
/// environment is scrubbed of every `IMPULSE_*` key, is exactly that case. A
/// caller should still send its request so the daemon's own typed authorization
/// error is what surfaces, rather than a client-side guess.
pub fn resolve_for_socket(socket_path: &Path) -> Option<String> {
    if let Ok(value) = std::env::var(OPERATOR_CAPABILITY_ENV) {
        if let Some(token) = parse_token(&value) {
            return Some(token);
        }
    }
    parse_token(&std::fs::read_to_string(path_for_socket(socket_path)).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_for_socket_sits_beside_the_socket() {
        assert_eq!(
            path_for_socket(Path::new("/tmp/sockets/impulse.sock")),
            PathBuf::from("/tmp/sockets/impulse.operator-cap")
        );
    }

    #[test]
    fn parse_token_accepts_only_full_width_lowercase_hex() {
        let valid = "a1b2c3d4".repeat(8);
        assert_eq!(parse_token(&valid).as_deref(), Some(valid.as_str()));
        assert_eq!(
            parse_token(&format!("  {valid}\n")).as_deref(),
            Some(valid.as_str())
        );

        for invalid in [
            String::new(),
            "not-hex".to_string(),
            "a".repeat(OPERATOR_CAPABILITY_HEX_LEN - 1),
            "a".repeat(OPERATOR_CAPABILITY_HEX_LEN + 1),
            "A".repeat(OPERATOR_CAPABILITY_HEX_LEN),
        ] {
            assert!(
                parse_token(&invalid).is_none(),
                "expected `{invalid}` to be rejected"
            );
        }
    }

    #[test]
    fn resolve_for_socket_reads_the_published_file_and_rejects_junk() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("impulse.sock");
        assert!(
            resolve_for_socket(&socket).is_none(),
            "an unpublished capability resolves to None, not an error"
        );

        let token = "b".repeat(OPERATOR_CAPABILITY_HEX_LEN);
        std::fs::write(path_for_socket(&socket), format!("{token}\n")).unwrap();
        assert_eq!(resolve_for_socket(&socket).as_deref(), Some(token.as_str()));

        std::fs::write(path_for_socket(&socket), "half-written").unwrap();
        assert!(resolve_for_socket(&socket).is_none());
    }
}
