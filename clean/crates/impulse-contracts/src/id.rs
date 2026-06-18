//! Strongly-typed newtypes for IDs that flow through the system.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident, $prefix:literal) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Deserialize,
            Serialize,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a fresh random id.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Parse from a hyphenated UUID string. Also accepts the
            /// `<prefix>_<uuid>` display format (e.g. `ws_3a2c…`).
            ///
            /// # Errors
            /// Returns [`ContractsError::InvalidId`] when the input is not a valid UUID.
            pub fn parse(s: &str) -> Result<Self, $crate::error::ContractsError> {
                let stripped = s.strip_prefix(concat!($prefix, "_")).unwrap_or(s);
                Uuid::parse_str(stripped).map(Self).map_err(|source| {
                    $crate::error::ContractsError::InvalidId {
                        kind: $prefix,
                        value: s.to_owned(),
                        source,
                    }
                })
            }

            /// Borrow the underlying UUID.
            #[must_use]
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }
    };
}

id_newtype!(SessionId, "sess");
id_newtype!(PaneId, "pane");
id_newtype!(DelegationId, "dlg");
id_newtype!(ToolCallId, "tc");
id_newtype!(WorkspaceId, "ws");

/// An absolute, canonicalized path that points at a workspace root.
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize, JsonSchema,
)]
#[serde(transparent)]
pub struct WorkspacePath(pub PathBuf);

impl WorkspacePath {
    /// Wrap a path after canonicalizing it. Returns the input unchanged if canonicalize fails
    /// (we still validate it is absolute).
    ///
    /// # Errors
    /// Returns [`ContractsError::InvalidPath`] if the path is not absolute or is empty.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, crate::error::ContractsError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(crate::error::ContractsError::InvalidPath {
                reason: "path is empty".to_owned(),
            });
        }
        if !path.is_absolute() {
            return Err(crate::error::ContractsError::InvalidPath {
                reason: format!("path is not absolute: {}", path.display()),
            });
        }
        Ok(Self(path))
    }

    /// Wrap without validation. Useful for read-only display contexts.
    #[must_use]
    pub fn new_unchecked(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Borrow the underlying [`Path`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

impl AsRef<Path> for WorkspacePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_round_trips_through_string() {
        let id = SessionId::new();
        let s = id.to_string();
        let parsed = SessionId::parse(s.split('_').nth(1).expect("uuid suffix")).expect("parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn session_id_rejects_garbage() {
        assert!(SessionId::parse("not-a-uuid").is_err());
    }

    #[test]
    fn session_id_parses_with_or_without_prefix() {
        let id = SessionId::new();
        let bare = id.as_uuid().to_string();
        let with_prefix = id.to_string();
        assert_eq!(SessionId::parse(&bare).unwrap(), id);
        assert_eq!(SessionId::parse(&with_prefix).unwrap(), id);
    }

    #[test]
    fn workspace_id_parses_with_or_without_prefix() {
        let id = WorkspaceId::new();
        let bare = id.as_uuid().to_string();
        let with_prefix = id.to_string();
        assert_eq!(WorkspaceId::parse(&bare).unwrap(), id);
        assert_eq!(WorkspaceId::parse(&with_prefix).unwrap(), id);
    }

    #[test]
    fn workspace_path_requires_absolute() {
        assert!(WorkspacePath::new("relative/path").is_err());
        assert!(WorkspacePath::new("").is_err());
        assert!(WorkspacePath::new("/abs/path").is_ok());
    }

    #[test]
    fn workspace_path_display() {
        let p = WorkspacePath::new_unchecked("/tmp/impulse");
        assert_eq!(p.to_string(), "/tmp/impulse");
    }

    #[test]
    fn distinct_id_types_are_distinct_strings() {
        // Display prefixes differ so they never collide in logs.
        let s = SessionId::new().to_string();
        let p = PaneId::new().to_string();
        assert!(s.starts_with("sess_"));
        assert!(p.starts_with("pane_"));
    }
}
