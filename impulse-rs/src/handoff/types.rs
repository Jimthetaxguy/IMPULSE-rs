//! Typed findings handed from one agent to another.
//!
//! A [`Claim`] always carries a [`ClaimSource`]. There is no absent-source
//! variant and no `Default`: deserializing a claim without `source` fails,
//! so an unsourced claim cannot be stored or accepted.

use std::fmt;
use std::path::{Component, Path};

use chrono::{DateTime, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

/// Line a cited file must contain, in its first 64 KiB, recording what the
/// file is about. The gate reads this marker; the sender does not get to
/// restate it beside the path.
pub const FILE_SUBJECT_PREFIX: &str = "impulse-subject:";

/// Commit-message trailer the gate reads for a cited commit's subject.
pub const COMMIT_SUBJECT_TRAILER: &str = "Impulse-Subject:";

/// Ledger file under the impulse directory.
pub const HANDOFF_GATE_FILE: &str = "HANDOFF_GATE.json";

const MAX_SUBJECT_LEN: usize = 253;
const MAX_ID_LEN: usize = 128;
const MAX_STATEMENT_BYTES: usize = 8 * 1024;
pub(crate) const MAX_CLAIMS: usize = 32;
pub(crate) const MAX_ARTIFACT_BYTES: usize = 256 * 1024;
pub(crate) const SUBJECT_SCAN_BYTES: usize = 64 * 1024;

/// What a check failed on. Serialized in the refusal so a caller can branch
/// without parsing prose, while [`GateFailure::fix`] names what to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// The request itself is not a well-formed handoff call.
    Request,
    /// The report carried no claims.
    ClaimsPresent,
    /// File missing, commit missing, or URL fetch not recorded by the sender.
    SourceResolves,
    /// The source's recorded subject or host is not the claim's subject.
    SubjectMatches,
    /// A shared-artifact write was based on a generation the writer had not
    /// just read.
    StaleRead,
    /// The handoff id is not in the ledger.
    HandoffExists,
    /// The shared artifact is not in the ledger.
    ArtifactExists,
    /// An accepted handoff was submitted again.
    AlreadyAccepted,
}

impl CheckKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::ClaimsPresent => "claims_present",
            Self::SourceResolves => "source_resolves",
            Self::SubjectMatches => "subject_matches",
            Self::StaleRead => "stale_read",
            Self::HandoffExists => "handoff_exists",
            Self::ArtifactExists => "artifact_exists",
            Self::AlreadyAccepted => "already_accepted",
        }
    }
}

impl fmt::Display for CheckKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reason a handoff or shared write was refused. `fix` names the change
/// that would make the same call pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateFailure {
    pub target: String,
    pub check: CheckKind,
    pub fix: String,
}

impl fmt::Display for GateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.target, self.check, self.fix)
    }
}

/// Lowercase subject key. For a URL source the gate compares this to the
/// URL host. For a file or commit it compares this to the subject recorded
/// in the file or commit message. The sender declares the claim's key and
/// cannot supply a second copy on the source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SubjectKey(String);

impl SubjectKey {
    pub fn parse(raw: &str) -> Result<Self, HandoffError> {
        let value = raw.trim();
        if value.is_empty() || value.len() > MAX_SUBJECT_LEN {
            return Err(HandoffError::Invalid(format!(
                "subject key `{raw}` must be 1..={MAX_SUBJECT_LEN} characters"
            )));
        }
        if value.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(HandoffError::Invalid(format!(
                "subject key `{value}` must be lowercase"
            )));
        }
        let bytes = value.as_bytes();
        let alphabet_ok = bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'.' || *b == b'-');
        let edges_ok = bytes.first().is_some_and(|b| b.is_ascii_alphanumeric())
            && bytes.last().is_some_and(|b| b.is_ascii_alphanumeric());
        if !alphabet_ok || !edges_ok || value.contains("..") {
            return Err(HandoffError::Invalid(format!(
                "subject key `{value}` must be a lowercase domain or token (letters, digits, dots, and hyphens; no leading, trailing, or doubled dots or hyphens)"
            )));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SubjectKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(de::Error::custom)
    }
}

/// Where a claim's evidence lives. Each variant is a locator the gate can
/// resolve without a model: a workspace-relative file, a git commit, or a
/// URL the sender has already recorded a fetch for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaimSource {
    File { path: String },
    Commit { hash: String },
    Url { url: String },
}

impl ClaimSource {
    pub fn display_label(&self) -> String {
        match self {
            Self::File { path } => format!("file `{path}`"),
            Self::Commit { hash } => format!("commit `{hash}`"),
            Self::Url { url } => format!("URL `{url}`"),
        }
    }
}

/// One sourced statement in a findings handoff.
///
/// Fields are private so the only constructors are [`Claim::try_new`] and
/// deserialization, both of which require a source and a subject key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Claim {
    id: String,
    statement: String,
    subject: SubjectKey,
    source: ClaimSource,
}

impl Claim {
    pub fn try_new(
        id: impl Into<String>,
        statement: impl Into<String>,
        subject: &str,
        source: ClaimSource,
    ) -> Result<Self, HandoffError> {
        let id = validate_id("claim id", &id.into())?;
        let statement = validate_statement(&statement.into())?;
        let subject = SubjectKey::parse(subject)?;
        let source = normalize_source(source)?;
        Ok(Self {
            id,
            statement,
            subject,
            source,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn statement(&self) -> &str {
        &self.statement
    }

    pub fn subject(&self) -> &SubjectKey {
        &self.subject
    }

    pub fn source(&self) -> &ClaimSource {
        &self.source
    }
}

impl<'de> Deserialize<'de> for Claim {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            statement: String,
            subject: String,
            source: ClaimSource,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::try_new(raw.id, raw.statement, &raw.subject, raw.source).map_err(de::Error::custom)
    }
}

/// Outcome of one `handoff` tool call.
///
/// `accepted` is true only when a findings handoff was accepted or a shared
/// write was committed. A refusal uses [`WorkflowState::Pending`] and a
/// non-empty `failures` list. There is no warning state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub state: WorkflowState,
    pub accepted: bool,
    pub snapshot: serde_json::Value,
    pub failures: Vec<GateFailure>,
    pub next_step: NextStep,
}

impl WorkflowResult {
    pub fn pending(
        snapshot: serde_json::Value,
        failures: Vec<GateFailure>,
        next_step: NextStep,
    ) -> Result<Self, HandoffError> {
        if failures.is_empty() {
            return Err(HandoffError::Invalid(
                "a pending handoff result requires at least one failure; refusing to report a silent pass"
                    .into(),
            ));
        }
        Ok(Self {
            state: WorkflowState::Pending,
            accepted: false,
            snapshot,
            failures,
            next_step,
        })
    }

    pub fn accepted(snapshot: serde_json::Value, next_step: NextStep) -> Self {
        Self {
            state: WorkflowState::Accepted,
            accepted: true,
            snapshot,
            failures: Vec::new(),
            next_step,
        }
    }

    pub fn recorded(snapshot: serde_json::Value, next_step: NextStep) -> Self {
        Self {
            state: WorkflowState::Recorded,
            accepted: false,
            snapshot,
            failures: Vec::new(),
            next_step,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    Pending,
    Accepted,
    Recorded,
}

impl WorkflowState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Recorded => "recorded",
        }
    }
}

impl fmt::Display for WorkflowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single action the caller should take next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextStep {
    pub action: String,
    pub detail: String,
}

#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    #[error("invalid handoff input: {0}")]
    Invalid(String),
    #[error("failed to {action} the handoff store: {source}")]
    Store {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("handoff store is unreadable: {0}")]
    Corrupt(String),
}

pub(crate) fn validate_id(field: &str, raw: &str) -> Result<String, HandoffError> {
    let value = raw.trim();
    let ok = !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !ok {
        return Err(HandoffError::Invalid(format!(
            "{field} `{raw}` must be 1..={MAX_ID_LEN} characters of letters, digits, '.', '_', or '-'"
        )));
    }
    Ok(value.to_string())
}

fn validate_statement(raw: &str) -> Result<String, HandoffError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(HandoffError::Invalid(
            "claim statement must not be empty".into(),
        ));
    }
    if value.len() > MAX_STATEMENT_BYTES {
        return Err(HandoffError::Invalid(format!(
            "claim statement is {} bytes; the maximum is {MAX_STATEMENT_BYTES}",
            value.len()
        )));
    }
    Ok(value.to_string())
}

pub(crate) fn validate_relative_path(field: &str, raw: &str) -> Result<String, HandoffError> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(HandoffError::Invalid(format!("{field} must not be empty")));
    }
    if path.len() > 256 {
        return Err(HandoffError::Invalid(format!(
            "{field} `{path}` is longer than 256 characters"
        )));
    }
    if path.contains('\\') || path.contains('\0') {
        return Err(HandoffError::Invalid(format!(
            "{field} `{path}` must be a relative path using `/`"
        )));
    }
    let mut parts = Vec::new();
    for comp in Path::new(path).components() {
        match comp {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(HandoffError::Invalid(format!(
                    "{field} `{path}` must not contain `..`"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(HandoffError::Invalid(format!(
                    "{field} `{path}` must be a relative path inside the workspace"
                )));
            }
        }
    }
    if parts.is_empty() {
        return Err(HandoffError::Invalid(format!(
            "{field} `{path}` must name a file"
        )));
    }
    Ok(parts.join("/"))
}

fn normalize_source(source: ClaimSource) -> Result<ClaimSource, HandoffError> {
    match source {
        ClaimSource::File { path } => Ok(ClaimSource::File {
            path: validate_relative_path("file source", &path)?,
        }),
        ClaimSource::Commit { hash } => Ok(ClaimSource::Commit {
            hash: validate_commit_hash(&hash)?,
        }),
        ClaimSource::Url { url } => {
            let (normalized, _) = normalize_url(&url)?;
            Ok(ClaimSource::Url { url: normalized })
        }
    }
}

pub(crate) fn validate_commit_hash(raw: &str) -> Result<String, HandoffError> {
    let hash = raw.trim();
    let ok = (7..=40).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if !ok || hash.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(HandoffError::Invalid(format!(
            "commit hash `{raw}` must be 7..=40 lowercase hex characters"
        )));
    }
    Ok(hash.to_string())
}

/// Canonical URL plus the host used as the source's subject.
///
/// Userinfo is rejected so a credential cannot land in the handoff ledger.
/// The fragment is dropped and default ports are removed so a fetch and a
/// claim that name the same resource match.
pub(crate) fn normalize_url(raw: &str) -> Result<(String, SubjectKey), HandoffError> {
    let raw = raw.trim();
    let Some(separator) = raw.find("://") else {
        return Err(HandoffError::Invalid(format!(
            "URL `{raw}` must start with http:// or https://"
        )));
    };
    let scheme = raw[..separator].to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return Err(HandoffError::Invalid(format!(
            "URL `{raw}` must start with http:// or https://"
        )));
    }
    let rest = &raw[separator + 3..];
    if rest.is_empty() || rest.contains(' ') {
        return Err(HandoffError::Invalid(format!(
            "URL `{raw}` is not a usable http(s) URL"
        )));
    }
    let (authority, after) = match rest.find(['/', '?', '#']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(HandoffError::Invalid(format!("URL `{raw}` has no host")));
    }
    if authority.contains('@') {
        return Err(HandoffError::Invalid(
            "URL must not include userinfo (a username or password). Cite the URL without credentials."
                .into(),
        ));
    }
    let (host, port) = split_host_port(authority)?;
    let subject = SubjectKey::parse(&host.to_ascii_lowercase()).map_err(|_| {
        HandoffError::Invalid(format!(
            "URL host `{host}` is not a subject key. Use a lowercase domain name."
        ))
    })?;
    let mut path_and_query = after;
    if let Some(index) = path_and_query.find('#') {
        path_and_query = &path_and_query[..index];
    }
    if path_and_query.is_empty() {
        path_and_query = "/";
    }
    let port_suffix = match (scheme.as_str(), port) {
        ("https", Some(443)) | ("http", Some(80)) | (_, None) => String::new(),
        (_, Some(port)) => format!(":{port}"),
    };
    let normalized = format!(
        "{scheme}://{}{port_suffix}{path_and_query}",
        subject.as_str()
    );
    Ok((normalized, subject))
}

fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), HandoffError> {
    if hostport.starts_with('[') {
        return Err(HandoffError::Invalid(
            "URL host must be a domain name, not an IP literal".into(),
        ));
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) => {
            if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(HandoffError::Invalid(format!(
                    "URL host `{hostport}` has no numeric port"
                )));
            }
            let port: u16 = port
                .parse()
                .map_err(|_| HandoffError::Invalid(format!("URL port `{port}` is out of range")))?;
            Ok((host.to_string(), Some(port)))
        }
        None => Ok((hostport.to_string(), None)),
    }
}

pub(crate) fn parse_digest(raw: &str) -> Result<String, HandoffError> {
    let hex = raw.trim().strip_prefix("sha256:").unwrap_or(raw.trim());
    let ok = hex.len() == 64
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
        && !hex.chars().any(|c| c.is_ascii_uppercase());
    if !ok {
        return Err(HandoffError::Invalid(
            "body_digest must be 64 lowercase hex characters, with an optional `sha256:` prefix"
                .into(),
        ));
    }
    Ok(format!("sha256:{hex}"))
}

/// `valid_from`-style timestamp used when a caller omits `fetched_at`.
pub(crate) fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>, HandoffError> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|stamp| stamp.with_timezone(&Utc))
        .map_err(|_| {
            HandoffError::Invalid(format!("timestamp `{raw}` must be an RFC 3339 timestamp"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_without_source_does_not_deserialize() {
        let json = r#"{"id":"refund-cap","statement":"Refunds over $50 need a second approver.","subject":"billing.example.com"}"#;
        let error = serde_json::from_str::<Claim>(json).unwrap_err();
        assert!(
            error.to_string().contains("source"),
            "missing source must be named, got {error}"
        );
    }

    #[test]
    fn test_claim_round_trip_preserves_normalized_source() {
        let original = Claim::try_new(
            "refund-cap",
            "Refunds over $50 need a second approver.",
            "billing.example.com",
            ClaimSource::Url {
                url: "HTTPS://Billing.Example.com:443/docs/refunds#cap".into(),
            },
        )
        .expect("normalized claim");
        assert_eq!(
            original.source(),
            &ClaimSource::Url {
                url: "https://billing.example.com/docs/refunds".into(),
            }
        );
        let json = serde_json::to_string(&original).unwrap();
        let recovered: Claim = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_subject_key_rejects_uppercase_and_empty() {
        let upper = SubjectKey::parse("Billing.Example.com").unwrap_err();
        assert!(format!("{upper}").contains("lowercase"));
        let empty = SubjectKey::parse("").unwrap_err();
        assert!(format!("{empty}").contains("subject key"));
    }

    #[test]
    fn test_pending_result_rejects_an_empty_failure_list() {
        let error = WorkflowResult::pending(
            serde_json::json!({}),
            Vec::new(),
            NextStep {
                action: "submit".into(),
                detail: "unused".into(),
            },
        )
        .unwrap_err();
        assert!(format!("{error}").contains("silent pass"));
    }

    #[test]
    fn test_handoff_error_display_names_the_problem() {
        let invalid = HandoffError::Invalid("claim is missing source".into());
        assert!(format!("{invalid}").contains("missing source"));
        let corrupt = HandoffError::Corrupt("schema 9".into());
        assert!(format!("{corrupt}").contains("unreadable"));
        assert!(format!("{corrupt}").contains("schema 9"));
    }

    #[test]
    fn test_gate_failure_display_includes_the_fix() {
        let failure = GateFailure {
            target: "refund-cap".into(),
            check: CheckKind::SubjectMatches,
            fix: "cite billing.example.com".into(),
        };
        let text = failure.to_string();
        assert!(text.contains("refund-cap"));
        assert!(text.contains("subject_matches"));
        assert!(text.contains("cite billing.example.com"));
    }

    #[test]
    fn test_normalize_url_strips_default_port_and_fragment() {
        let (url, host) =
            normalize_url("https://Auth.Example.com:443/docs/refunds#section").unwrap();
        assert_eq!(url, "https://auth.example.com/docs/refunds");
        assert_eq!(host.as_str(), "auth.example.com");
    }

    #[test]
    fn test_normalize_url_rejects_userinfo() {
        let error = normalize_url("https://user:secret@billing.example.com/docs").unwrap_err();
        assert!(format!("{error}").contains("userinfo"));
        assert!(!format!("{error}").contains("secret"));
    }

    #[test]
    fn test_workflow_result_round_trip() {
        let original = WorkflowResult::accepted(
            serde_json::json!({"kind": "handoff", "id": "h1"}),
            NextStep {
                action: "read".into(),
                detail: "Handoff `h1` is accepted.".into(),
            },
        );
        let json = serde_json::to_string(&original).unwrap();
        let recovered: WorkflowResult = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
        assert!(recovered.accepted);
        assert!(recovered.failures.is_empty());
    }
}
