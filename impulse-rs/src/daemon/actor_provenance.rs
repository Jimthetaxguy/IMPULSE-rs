//! Socket actor provenance for the daemon boundary (ADR-0018).
//!
//! Before this module, every request on the Unix socket was anonymous: the
//! daemon could not tell its own operator surface from a launched Builder that
//! had inherited `IMPULSE_SOCKET_PATH`, so a governed runtime could send
//! `RecordOperatorDecision` and mint `accepted` for its own task. The actor
//! check in the state layer is kind-only, and a kind is a field a client
//! fills in.
//!
//! Two independent facts now classify a connection:
//!
//! 1. **Peer credentials.** `SO_PEERCRED`/`LOCAL_PEERCRED`, read through
//!    [`tokio::net::UnixStream::peer_cred`], prove the connecting process's uid
//!    to the kernel. A peer whose uid differs from the daemon's own can never
//!    reach operator class.
//! 2. **A per-daemon-run operator capability.** 32 random bytes, hex encoded,
//!    written mode 0600 beside the socket at startup and removed at shutdown.
//!    A client presents it once per connection.
//!
//! Peer credentials alone are not sufficient: a launched Builder runs as the
//! same user and passes that check. The capability is what separates the
//! operator surface (which reads the file, or receives
//! `IMPULSE_OPERATOR_CAPABILITY`) from a launched runtime, which is never given
//! either and whose environment is scrubbed of every `IMPULSE_*` key before the
//! PTY spawns.
//!
//! **Boundary this does not cross.** The capability file is owned by the
//! daemon's uid in a mode 0700 directory; a same-uid process that deliberately
//! goes looking for it can read it. This is a structural boundary — a launched
//! runtime is never *handed* the capability — not a cryptographic one against a
//! same-uid adversary. ADR-0018 states the limit explicitly rather than
//! implying stronger authentication than the socket can carry.

use std::path::{Path, PathBuf};

use impulse_ops::governed_task::{GovernedTaskMutation, OperatorAuthentication};

// Where the capability lives, what a well-formed token looks like, and how a
// client discovers one are shared with `impulse-desktop`'s own socket client
// through `impulse_ops::operator_capability`. Minting, publishing, connection
// classification, and the constant-time comparison stay here, with the daemon.
pub use impulse_ops::operator_capability::{
    OPERATOR_CAPABILITY_ENV, OPERATOR_CAPABILITY_EXTENSION,
};

/// Random bytes behind one capability token. 32 bytes is the same width as the
/// SHA-256 digests used elsewhere in the governed record chain.
const CAPABILITY_BYTES: usize = 32;

/// Hex characters in a well-formed capability token.
const CAPABILITY_HEX_LEN: usize = impulse_ops::operator_capability::OPERATOR_CAPABILITY_HEX_LEN;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActorProvenanceError {
    #[error(
        "request `{request}` requires an operator-class connection: present this daemon run's \
         operator capability (`{OPERATOR_CAPABILITY_ENV}`, or the 0600 file beside the socket) \
         with PresentOperatorCapability before sending it"
    )]
    OperatorClassRequired { request: &'static str },
    #[error(
        "operator capability rejected: connection peer uid {peer_uid} does not match daemon uid \
         {daemon_uid}"
    )]
    PeerUidMismatch { peer_uid: u32, daemon_uid: u32 },
    #[error("operator capability rejected: peer credentials are unavailable for this connection")]
    PeerCredentialsUnavailable,
    #[error("operator capability rejected: the presented token does not match this daemon run")]
    CapabilityRejected,
    #[error(
        "operator capability unavailable: this daemon run has no capability to compare against"
    )]
    CapabilityUnavailable,
    #[error("malformed operator capability: expected {CAPABILITY_HEX_LEN} lowercase hexadecimal characters")]
    MalformedCapability,
}

/// What a connection is allowed to do, decided per connection and never
/// inferred from request payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionClass {
    /// The default for every accepted connection, including a launched
    /// governed runtime holding `IMPULSE_SOCKET_PATH`.
    #[default]
    NonOperator,
    /// Presented this daemon run's capability from a peer whose uid matches
    /// the daemon's own.
    Operator,
}

impl ConnectionClass {
    pub fn is_operator(self) -> bool {
        matches!(self, Self::Operator)
    }

    /// Provenance stamped onto an operator decision recorded by this class.
    pub fn operator_authentication(self) -> OperatorAuthentication {
        match self {
            Self::Operator => OperatorAuthentication::CapabilityAuthenticated,
            Self::NonOperator => OperatorAuthentication::Declared,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::NonOperator => "non_operator",
        }
    }
}

/// One daemon run's operator capability.
///
/// The token never reaches `Debug` output, logs, or a daemon response; only
/// [`OperatorCapability::expose`] can read it, and only the file writer and the
/// constant-time comparison call it.
#[derive(Clone, PartialEq, Eq)]
pub struct OperatorCapability {
    token: String,
}

impl std::fmt::Debug for OperatorCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorCapability")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl OperatorCapability {
    /// Mint a fresh capability for one daemon run: 32 bytes straight from the
    /// operating system's CSPRNG.
    ///
    /// `/dev/urandom` rather than a crate: `rand` and `getrandom` are only
    /// transitive dependencies here, and this workspace's manifest is not this
    /// lane's to change. Reading the device is the same kernel CSPRNG those
    /// crates draw from on this platform. An earlier draft concatenated two
    /// `Uuid::new_v4` values, which is 244 bits with six fixed nibbles, not 256
    /// — a real difference from what the ADR claims, so the claim now matches
    /// the code. Failure is returned rather than absorbed: a daemon that cannot
    /// read randomness must not fall back to something weaker.
    pub fn generate() -> anyhow::Result<Self> {
        use anyhow::Context as _;
        use std::io::Read as _;

        let mut bytes = [0u8; CAPABILITY_BYTES];
        std::fs::File::open("/dev/urandom")
            .context("Failed to open the system CSPRNG for the operator capability")?
            .read_exact(&mut bytes)
            .context("Failed to read operator capability entropy")?;
        let mut token = String::with_capacity(CAPABILITY_HEX_LEN);
        for byte in bytes {
            use std::fmt::Write as _;
            // Writing to a String is infallible; the Result exists only to
            // satisfy the fmt::Write signature.
            let _ = write!(token, "{byte:02x}");
        }
        Ok(Self { token })
    }

    /// Parse a token read from a file or the environment.
    pub fn parse(token: &str) -> Result<Self, ActorProvenanceError> {
        impulse_ops::operator_capability::parse_token(token)
            .map(|token| Self { token })
            .ok_or(ActorProvenanceError::MalformedCapability)
    }

    /// The raw token. Only the capability file writer and the comparison below
    /// call this; nothing else may put the value on a wire or in a log.
    pub fn expose(&self) -> &str {
        &self.token
    }

    /// Compare in time independent of how many leading characters match, so a
    /// local caller cannot recover the token one character at a time.
    pub fn matches(&self, presented: &str) -> bool {
        let expected = self.token.as_bytes();
        let presented = presented.trim().as_bytes();
        if expected.len() != presented.len() {
            return false;
        }
        let mut difference = 0u8;
        for (left, right) in expected.iter().zip(presented.iter()) {
            difference |= left ^ right;
        }
        difference == 0
    }

    /// Where the capability for `socket_path` lives.
    pub fn path_for_socket(socket_path: &Path) -> PathBuf {
        impulse_ops::operator_capability::path_for_socket(socket_path)
    }

    /// Resolve a capability the way a client does: an explicit environment
    /// override, else the file the daemon published beside `socket_path`.
    pub fn resolve_for_socket(socket_path: &Path) -> Option<Self> {
        impulse_ops::operator_capability::resolve_for_socket(socket_path)
            .map(|token| Self { token })
    }
}

/// Peer-credential and capability state for one accepted connection.
#[derive(Debug, Clone)]
pub struct ConnectionProvenance {
    peer_uid: Option<u32>,
    daemon_uid: u32,
    class: ConnectionClass,
}

impl ConnectionProvenance {
    pub fn new(peer_uid: Option<u32>, daemon_uid: u32) -> Self {
        Self {
            peer_uid,
            daemon_uid,
            class: ConnectionClass::NonOperator,
        }
    }

    pub fn class(&self) -> ConnectionClass {
        self.class
    }

    pub fn peer_uid(&self) -> Option<u32> {
        self.peer_uid
    }

    /// Raise this connection to operator class if, and only if, the peer uid
    /// matches the daemon's own uid *and* the presented token matches this
    /// daemon run's capability.
    ///
    /// A rejected presentation leaves the class untouched: a caller cannot
    /// downgrade or corrupt an already-authenticated connection by presenting
    /// garbage afterwards.
    pub fn present_capability(
        &mut self,
        presented: &str,
        expected: Option<&OperatorCapability>,
    ) -> Result<ConnectionClass, ActorProvenanceError> {
        let peer_uid = self
            .peer_uid
            .ok_or(ActorProvenanceError::PeerCredentialsUnavailable)?;
        if peer_uid != self.daemon_uid {
            return Err(ActorProvenanceError::PeerUidMismatch {
                peer_uid,
                daemon_uid: self.daemon_uid,
            });
        }
        let expected = expected.ok_or(ActorProvenanceError::CapabilityUnavailable)?;
        if !expected.matches(presented) {
            return Err(ActorProvenanceError::CapabilityRejected);
        }
        self.class = ConnectionClass::Operator;
        Ok(self.class)
    }
}

/// The uid this daemon process runs as.
pub fn daemon_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments, reads only the calling process's
    // own credentials, cannot fail, and is defined by POSIX to always succeed
    // (no errno path). There is no pointer, no allocation, and no lifetime to
    // uphold, so the call has no preconditions to validate beforehand.
    unsafe { libc::geteuid() }
}

/// Peer uid for an accepted connection, or `None` when the platform refuses to
/// report it (treated as "cannot be operator class").
pub fn peer_uid(stream: &tokio::net::UnixStream) -> Option<u32> {
    match stream.peer_cred() {
        Ok(credentials) => Some(credentials.uid()),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "peer credentials unavailable; connection stays non-operator"
            );
            None
        }
    }
}

/// Whether a governed mutation may be applied by `class`.
///
/// `RecordOperatorDecision` is the acceptance gate itself and always requires
/// operator class. For a *profiled* task the launch-lifecycle marks are also
/// operator-only: a Builder submits claims through the daemon-owned producer
/// requests, it does not narrate its own lifecycle. Caller-composed
/// (non-profiled) tasks keep their existing lifecycle behavior, since no
/// daemon-owned producer chain governs them. The claim, verification, and
/// Supervisor mutations are not classified here because a profiled task already
/// refuses them outright in favour of the daemon-owned producer requests, and an
/// unprofiled task's evidence is caller-composed by contract.
///
/// **The match below is deliberately exhaustive and must stay that way.** An
/// earlier draft ended in `_ => Ok(())`, which fails *open*: a mutation variant
/// added later — ADR-0019's promotion is the obvious one — would silently become
/// reachable from a launched Builder. Adding a variant to
/// [`GovernedTaskMutation`] must break this function's compilation so the new
/// transition is classified on purpose rather than by omission.
pub fn authorize_governed_mutation(
    mutation: &GovernedTaskMutation,
    task_is_profiled: bool,
    class: ConnectionClass,
) -> Result<(), ActorProvenanceError> {
    if class.is_operator() {
        return Ok(());
    }
    let gated: Option<&'static str> = match mutation {
        GovernedTaskMutation::RecordOperatorDecision { .. } => Some("RecordOperatorDecision"),
        GovernedTaskMutation::MarkRunning { .. } => task_is_profiled.then_some("MarkRunning"),
        GovernedTaskMutation::MarkLaunchFailed { .. } => {
            task_is_profiled.then_some("MarkLaunchFailed")
        }
        GovernedTaskMutation::MarkRuntimeExited { .. } => {
            task_is_profiled.then_some("MarkRuntimeExited")
        }
        // ADR-0012's reservation journal (PR #45) is reconciled in-process at
        // ledger load; nothing legitimately submits it over the socket. Gating
        // it operator-only therefore costs nothing and fails closed: a launched
        // Builder cannot forge an "your reservation was interrupted" note to
        // move its own task out of a stuck reservation.
        GovernedTaskMutation::NoteProducerReservationInterrupted { .. } => {
            Some("NoteProducerReservationInterrupted")
        }
        GovernedTaskMutation::SubmitClaim { .. }
        | GovernedTaskMutation::RecordVerification { .. }
        | GovernedTaskMutation::RecordSupervisorVerdict { .. } => None,
    };
    match gated {
        Some(request) => Err(ActorProvenanceError::OperatorClassRequired { request }),
        None => Ok(()),
    }
}

/// True when authorizing `mutation` needs the task's verification profile, and
/// therefore a ledger lookup.
pub fn mutation_authorization_needs_profile(mutation: &GovernedTaskMutation) -> bool {
    matches!(
        mutation,
        GovernedTaskMutation::MarkRunning { .. }
            | GovernedTaskMutation::MarkLaunchFailed { .. }
            | GovernedTaskMutation::MarkRuntimeExited { .. }
    )
}

/// Write `capability` to `path` atomically with owner-only permissions.
///
/// Temp file name carries the PID plus nanoseconds so two daemons racing on the
/// same directory cannot collide, matching `storage::atomic_write_path`.
pub fn write_capability_file(path: &Path, capability: &OperatorCapability) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path
        .parent()
        .context("operator capability path has no parent directory")?;
    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "Failed to create operator capability directory {}",
            parent.display()
        )
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("operator-cap"),
        std::process::id(),
        nanos
    ));
    // A leftover temp file from a crashed run could have any owner-visible
    // mode, and `OpenOptions::mode` only applies to a file this call creates.
    // `create_new` refuses to open an existing path at all, so the secret is
    // only ever written into a file this process just created at 0600.
    match std::fs::remove_file(&temp) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(anyhow::Error::new(error).context(format!(
                "Failed to clear stale operator capability temp file {}",
                temp.display()
            )))
        }
    }
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .with_context(|| {
                format!(
                    "Failed to create operator capability temp file {}",
                    temp.display()
                )
            })?;
        // Assert the mode on the open descriptor, not the path: a path-based
        // chmod races against anything that could swap the path in between.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .context("Failed to restrict operator capability permissions")?;
        file.write_all(capability.expose().as_bytes())
            .context("Failed to write operator capability")?;
        file.write_all(b"\n")
            .context("Failed to terminate operator capability file")?;
        file.sync_all()
            .context("Failed to flush operator capability file")?;
    }
    std::fs::rename(&temp, path).with_context(|| {
        format!(
            "Failed to install operator capability at {}",
            path.display()
        )
    })?;
    Ok(())
}

/// Read a capability token from `path`.
pub fn read_capability_file(path: &Path) -> anyhow::Result<OperatorCapability> {
    use anyhow::Context as _;

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read operator capability {}", path.display()))?;
    OperatorCapability::parse(&contents)
        .with_context(|| format!("Invalid operator capability in {}", path.display()))
}

/// Remove the capability file, ignoring an already-absent file.
pub fn remove_capability_file(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            path = %path.display(),
            error = %error,
            "failed to remove operator capability file"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use impulse_ops::governed_task::{
        GovernedActor, GovernedActorKind, GovernedRecordId, OperatorDecisionInput,
        OperatorDecisionKind,
    };
    use std::os::unix::fs::PermissionsExt;

    fn operator_decision_mutation() -> GovernedTaskMutation {
        GovernedTaskMutation::RecordOperatorDecision {
            decision: OperatorDecisionInput {
                actor: GovernedActor {
                    kind: GovernedActorKind::Operator,
                    id: "operator-a".to_string(),
                },
                supervisor_verdict_id: GovernedRecordId::try_new("verdict-a").unwrap(),
                decision: OperatorDecisionKind::Approve,
                rationale: "looks right".to_string(),
            },
        }
    }

    fn system_actor() -> GovernedActor {
        GovernedActor {
            kind: GovernedActorKind::System,
            id: "desktop".to_string(),
        }
    }

    #[test]
    fn test_generate_produces_distinct_full_width_lowercase_hex_tokens() {
        let first = OperatorCapability::generate().unwrap();
        let second = OperatorCapability::generate().unwrap();
        assert_eq!(first.expose().len(), CAPABILITY_HEX_LEN);
        assert!(first
            .expose()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert_ne!(
            first.expose(),
            second.expose(),
            "each daemon run must mint a distinct capability"
        );
    }

    #[test]
    fn test_debug_never_renders_the_token() {
        let capability = OperatorCapability::generate().unwrap();
        let rendered = format!("{capability:?}");
        assert!(rendered.contains("redacted"));
        assert!(!rendered.contains(capability.expose()));
    }

    #[test]
    fn test_parse_rejects_malformed_tokens() {
        for candidate in [
            "",
            "not-hex",
            &"a".repeat(CAPABILITY_HEX_LEN - 1),
            &"A".repeat(CAPABILITY_HEX_LEN),
        ] {
            assert_eq!(
                OperatorCapability::parse(candidate).unwrap_err(),
                ActorProvenanceError::MalformedCapability,
                "expected `{candidate}` to be rejected"
            );
        }
    }

    #[test]
    fn test_parse_accepts_a_generated_token_with_surrounding_whitespace() {
        let capability = OperatorCapability::generate().unwrap();
        let parsed = OperatorCapability::parse(&format!("  {}\n", capability.expose())).unwrap();
        assert_eq!(parsed, capability);
    }

    #[test]
    fn test_matches_rejects_wrong_length_and_wrong_value() {
        let capability = OperatorCapability::generate().unwrap();
        assert!(capability.matches(capability.expose()));
        assert!(!capability.matches(""));
        assert!(!capability.matches(&capability.expose()[..CAPABILITY_HEX_LEN - 1]));
        let mut mutated = capability.expose().to_string();
        mutated.replace_range(0..1, if mutated.starts_with('a') { "b" } else { "a" });
        assert!(!capability.matches(&mutated));
    }

    #[test]
    fn test_present_capability_raises_only_a_same_uid_peer_with_the_right_token() {
        let capability = OperatorCapability::generate().unwrap();
        let mut provenance = ConnectionProvenance::new(Some(501), 501);
        assert_eq!(provenance.class(), ConnectionClass::NonOperator);
        assert_eq!(
            provenance
                .present_capability(capability.expose(), Some(&capability))
                .unwrap(),
            ConnectionClass::Operator
        );
        assert!(provenance.class().is_operator());
    }

    #[test]
    fn test_present_capability_rejects_a_different_peer_uid() {
        let capability = OperatorCapability::generate().unwrap();
        let mut provenance = ConnectionProvenance::new(Some(502), 501);
        assert_eq!(
            provenance
                .present_capability(capability.expose(), Some(&capability))
                .unwrap_err(),
            ActorProvenanceError::PeerUidMismatch {
                peer_uid: 502,
                daemon_uid: 501
            }
        );
        assert_eq!(provenance.class(), ConnectionClass::NonOperator);
    }

    #[test]
    fn test_present_capability_rejects_missing_peer_credentials_and_wrong_tokens() {
        let capability = OperatorCapability::generate().unwrap();

        let mut unknown_peer = ConnectionProvenance::new(None, 501);
        assert_eq!(
            unknown_peer
                .present_capability(capability.expose(), Some(&capability))
                .unwrap_err(),
            ActorProvenanceError::PeerCredentialsUnavailable
        );

        let mut no_capability = ConnectionProvenance::new(Some(501), 501);
        assert_eq!(
            no_capability
                .present_capability(capability.expose(), None)
                .unwrap_err(),
            ActorProvenanceError::CapabilityUnavailable
        );

        let mut wrong_token = ConnectionProvenance::new(Some(501), 501);
        assert_eq!(
            wrong_token
                .present_capability(
                    OperatorCapability::generate().unwrap().expose(),
                    Some(&capability)
                )
                .unwrap_err(),
            ActorProvenanceError::CapabilityRejected
        );
        assert_eq!(wrong_token.class(), ConnectionClass::NonOperator);
    }

    #[test]
    fn test_a_rejected_presentation_cannot_downgrade_an_operator_connection() {
        let capability = OperatorCapability::generate().unwrap();
        let mut provenance = ConnectionProvenance::new(Some(501), 501);
        provenance
            .present_capability(capability.expose(), Some(&capability))
            .unwrap();
        assert!(provenance
            .present_capability("nonsense", Some(&capability))
            .is_err());
        assert!(provenance.class().is_operator());
    }

    #[test]
    fn test_authorize_rejects_operator_decision_from_a_non_operator_connection() {
        let error = authorize_governed_mutation(
            &operator_decision_mutation(),
            true,
            ConnectionClass::NonOperator,
        )
        .unwrap_err();
        assert_eq!(
            error,
            ActorProvenanceError::OperatorClassRequired {
                request: "RecordOperatorDecision"
            }
        );
        assert!(format!("{error}").contains("operator-class connection"));
        assert!(format!("{error}").contains(OPERATOR_CAPABILITY_ENV));
    }

    #[test]
    fn test_authorize_rejects_operator_decision_for_caller_composed_tasks_too() {
        assert!(authorize_governed_mutation(
            &operator_decision_mutation(),
            false,
            ConnectionClass::NonOperator
        )
        .is_err());
    }

    #[test]
    fn test_authorize_allows_every_mutation_on_an_operator_connection() {
        for mutation in [
            operator_decision_mutation(),
            GovernedTaskMutation::MarkRunning {
                actor: system_actor(),
            },
        ] {
            assert!(
                authorize_governed_mutation(&mutation, true, ConnectionClass::Operator).is_ok()
            );
        }
    }

    #[test]
    fn test_authorize_gates_profiled_lifecycle_marks_only() {
        let marks = [
            GovernedTaskMutation::MarkRunning {
                actor: system_actor(),
            },
            GovernedTaskMutation::MarkLaunchFailed {
                actor: system_actor(),
                reason: "spawn failed".to_string(),
            },
            GovernedTaskMutation::MarkRuntimeExited {
                actor: system_actor(),
                reason: None,
            },
        ];
        for mark in marks {
            assert!(
                authorize_governed_mutation(&mark, true, ConnectionClass::NonOperator).is_err(),
                "profiled lifecycle marks are operator-only"
            );
            assert!(
                authorize_governed_mutation(&mark, false, ConnectionClass::NonOperator).is_ok(),
                "caller-composed tasks keep their existing lifecycle behavior"
            );
            assert!(mutation_authorization_needs_profile(&mark));
        }
        assert!(!mutation_authorization_needs_profile(
            &operator_decision_mutation()
        ));
    }

    #[test]
    fn test_authorize_gates_the_reservation_interrupted_note_operator_only() {
        let note = GovernedTaskMutation::NoteProducerReservationInterrupted {
            actor: system_actor(),
            reason: "a producer reservation was left open by a dead process".to_string(),
        };
        for profiled in [true, false] {
            let error = authorize_governed_mutation(&note, profiled, ConnectionClass::NonOperator)
                .unwrap_err();
            assert_eq!(
                error,
                ActorProvenanceError::OperatorClassRequired {
                    request: "NoteProducerReservationInterrupted"
                },
                "the reconcile-only note is never submittable from a non-operator connection"
            );
        }
        assert!(authorize_governed_mutation(&note, true, ConnectionClass::Operator).is_ok());
        assert!(
            !mutation_authorization_needs_profile(&note),
            "the note is gated regardless of profile, so no ledger lookup is needed"
        );
    }

    #[test]
    fn test_connection_class_maps_to_operator_authentication() {
        assert_eq!(
            ConnectionClass::Operator.operator_authentication(),
            OperatorAuthentication::CapabilityAuthenticated
        );
        assert_eq!(
            ConnectionClass::NonOperator.operator_authentication(),
            OperatorAuthentication::Declared
        );
        assert_eq!(ConnectionClass::default(), ConnectionClass::NonOperator);
        assert_eq!(ConnectionClass::Operator.as_str(), "operator");
        assert_eq!(ConnectionClass::NonOperator.as_str(), "non_operator");
    }

    #[test]
    fn test_capability_file_round_trips_at_owner_only_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("impulse.sock");
        let path = OperatorCapability::path_for_socket(&socket);
        assert_eq!(path.file_name().unwrap(), "impulse.operator-cap");

        let capability = OperatorCapability::generate().unwrap();
        write_capability_file(&path, &capability).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "capability must be owner-read-write only");
        assert_eq!(read_capability_file(&path).unwrap(), capability);

        // Rewriting over an existing capability keeps the mode and the value.
        let replacement = OperatorCapability::generate().unwrap();
        write_capability_file(&path, &replacement).unwrap();
        assert_eq!(read_capability_file(&path).unwrap(), replacement);

        remove_capability_file(&path);
        assert!(!path.exists());
        // Removing an absent file is not an error.
        remove_capability_file(&path);
    }

    #[test]
    fn test_read_capability_file_errors_on_missing_and_malformed_files() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("absent.operator-cap");
        assert!(read_capability_file(&missing).is_err());

        let malformed = directory.path().join("bad.operator-cap");
        std::fs::write(&malformed, "not-a-capability").unwrap();
        let error = read_capability_file(&malformed).unwrap_err();
        assert!(format!("{error:#}").contains("Invalid operator capability"));
    }

    #[test]
    fn test_write_capability_file_errors_when_the_parent_cannot_be_created() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, "file").unwrap();
        let path = blocker.join("nested").join("impulse.operator-cap");
        assert!(write_capability_file(&path, &OperatorCapability::generate().unwrap()).is_err());
    }

    #[test]
    fn test_daemon_uid_is_stable_and_matches_the_owner_of_a_file_it_creates() {
        // Exercises the one `unsafe` call: the uid it reports must equal the
        // owner recorded by the kernel for a file this process just created.
        use std::os::unix::fs::MetadataExt as _;

        let first = daemon_uid();
        assert_eq!(first, daemon_uid(), "geteuid must be stable within a run");

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owned");
        std::fs::write(&path, b"owned").unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().uid(), first);
    }
}
