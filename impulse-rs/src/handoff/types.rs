//! Typed findings handed from one agent to another.
//!
//! A [`Claim`] always carries a [`ClaimSource`]. There is no absent-source
//! variant and no `Default`: deserializing a claim without `source` fails,
//! so an unsourced claim cannot be stored or accepted.

use std::fmt;
use std::path::{Component, Path, PathBuf};

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

/// Default maximum age of a recorded URL fetch that may count as evidence.
///
/// An age equal to this limit is still evidence. One second past it is not.
/// Callers may pass a different positive duration; this is the default.
pub const DEFAULT_FETCH_MAX_AGE_SECS: i64 = 24 * 60 * 60;

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
    /// A fetch row exists and is not usable evidence: status is not 2xx,
    /// `fetched_at` is in the future or older than the max age, or the
    /// recording session is not this submit's session.
    FetchUnusable,
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
            Self::FetchUnusable => "fetch_unusable",
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
/// that would make the same call pass. `field` is the payload field to
/// change on the next attempt, when one field is at fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateFailure {
    pub target: String,
    pub check: CheckKind,
    /// JSON field the caller should correct. Older ledger rows omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub fix: String,
}

impl fmt::Display for GateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(
                f,
                "{} [{}] field `{field}`: {}",
                self.target, self.check, self.fix
            ),
            None => write!(f, "{} [{}]: {}", self.target, self.check, self.fix),
        }
    }
}

/// Who is calling the gate, stamped by the caller from tool or session
/// context. The gate does not read these values from the JSON payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionContext {
    pub execution_origin: String,
    pub session_id: Option<String>,
    pub fetch_max_age: chrono::Duration,
}

impl AdmissionContext {
    /// Library entry with no session. URL fetches recorded this way resolve
    /// only for another call that also has no session.
    pub fn direct() -> Self {
        Self {
            execution_origin: "direct".to_string(),
            session_id: None,
            fetch_max_age: default_fetch_max_age(),
        }
    }

    /// Build a stamp from the tool context's origin and session.
    ///
    /// Empty strings become "direct" and `None`. This function has no
    /// payload argument: a caller cannot pass a JSON field here.
    pub fn from_tool(execution_origin: &str, session_id: Option<&str>) -> Self {
        let origin = execution_origin.trim();
        Self {
            execution_origin: if origin.is_empty() {
                "direct".to_string()
            } else {
                origin.to_string()
            },
            session_id: blank_to_none(session_id),
            fetch_max_age: default_fetch_max_age(),
        }
    }

    pub fn with_fetch_max_age(mut self, fetch_max_age: chrono::Duration) -> Self {
        self.fetch_max_age = fetch_max_age;
        self
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = blank_to_none(Some(session_id));
        self
    }
}

pub fn default_fetch_max_age() -> chrono::Duration {
    chrono::Duration::seconds(DEFAULT_FETCH_MAX_AGE_SECS)
}

/// Parse a caller-supplied max age in seconds.
///
/// Blank, non-numeric, zero, negative, and overflowing values keep
/// [`default_fetch_max_age`]. The gate receives the resulting duration; it
/// does not read the environment itself.
pub fn parse_fetch_max_age_secs(raw: Option<&str>) -> chrono::Duration {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return default_fetch_max_age();
    };
    match raw.parse::<i64>() {
        Ok(seconds) if seconds > 0 => {
            chrono::Duration::try_seconds(seconds).unwrap_or_else(default_fetch_max_age)
        }
        _ => default_fetch_max_age(),
    }
}

fn blank_to_none(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|trimmed| !trimmed.is_empty())
        .map(str::to_string)
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
    /// A payload field is missing or unusable. `message` is the refusal text.
    /// Dispatch turns this into a [`GateFailure`] whose `field` is `field`.
    #[error("{message}")]
    Request { field: String, message: String },
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
        return Err(HandoffError::Request {
            field: field.to_string(),
            message: format!(
                "{field} `{raw}` must be 1..={MAX_ID_LEN} characters of letters, digits, '.', '_', or '-'"
            ),
        });
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

/// One workspace root for both the tool sandbox and claim resolution.
///
/// Trims surrounding whitespace, rejects an empty value, a NUL byte, and any
/// `..` segment, then canonicalizes. Callers must use this `PathBuf` for the
/// read-root check and for every later file read and `git -C`. A second trim
/// of the original string is what let `" /etc"` pass the sandbox and then
/// escape.
pub(crate) fn normalize_workspace(raw: &str) -> Result<PathBuf, HandoffError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(HandoffError::Invalid("workspace must not be empty".into()));
    }
    if trimmed.contains('\0') {
        return Err(HandoffError::Invalid(
            "workspace must not contain a NUL byte".into(),
        ));
    }
    let path = Path::new(trimmed);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(HandoffError::Invalid(
            "workspace must not contain `..`".into(),
        ));
    }
    let canonical = path.canonicalize().map_err(|_| {
        HandoffError::Invalid(format!(
            "workspace `{trimmed}` does not exist or could not be resolved"
        ))
    })?;
    if !canonical.is_dir() {
        return Err(HandoffError::Invalid(format!(
            "workspace `{}` must be a directory",
            canonical.display()
        )));
    }
    Ok(canonical)
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
        let request = HandoffError::Request {
            field: "from_agent".into(),
            message: "payload.from_agent missing".into(),
        };
        assert!(format!("{request}").contains("payload.from_agent missing"));
        let corrupt = HandoffError::Corrupt("schema 9".into());
        assert!(format!("{corrupt}").contains("unreadable"));
        assert!(format!("{corrupt}").contains("schema 9"));
    }

    #[test]
    fn test_gate_failure_display_includes_the_fix() {
        let failure = GateFailure {
            target: "refund-cap".into(),
            check: CheckKind::SubjectMatches,
            field: None,
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
    fn test_normalize_workspace_rejects_empty_nul_parent_and_missing() {
        let empty = normalize_workspace("   ").unwrap_err();
        assert!(format!("{empty}").contains("must not be empty"));
        let nul = normalize_workspace("/tmp/ws\0").unwrap_err();
        assert!(format!("{nul}").contains("NUL"));
        let parent = normalize_workspace("/tmp/project/../etc").unwrap_err();
        assert!(format!("{parent}").contains(".."));
        let temp = tempfile::tempdir().unwrap();
        let missing_path = temp.path().join("missing-workspace");
        let missing = normalize_workspace(&missing_path.to_string_lossy()).unwrap_err();
        assert!(format!("{missing}").contains("does not exist"));
    }

    #[test]
    fn test_normalize_workspace_trims_and_canonicalizes_an_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().canonicalize().unwrap();
        let padded = format!("  {}  ", temp.path().display());
        assert_eq!(normalize_workspace(&padded).unwrap(), canonical);
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

    #[test]
    fn test_gate_failure_round_trip_keeps_the_field() {
        let original = GateFailure {
            target: "refund-report".into(),
            check: CheckKind::Request,
            field: Some("from_agent".into()),
            fix: "payload.from_agent missing".into(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: GateFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
        assert_eq!(recovered.field.as_deref(), Some("from_agent"));
        assert!(recovered.to_string().contains("from_agent"));
        assert!(recovered.to_string().contains("payload.from_agent missing"));

        let legacy =
            r#"{"target":"refund-cap","check":"subject_matches","fix":"cite billing.example.com"}"#;
        let without_field: GateFailure = serde_json::from_str(legacy).unwrap();
        assert_eq!(without_field.field, None);
        assert!(without_field.to_string().contains("subject_matches"));
    }

    #[test]
    fn test_fetch_unusable_check_serializes_snake_case() {
        let kind = CheckKind::FetchUnusable;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"fetch_unusable\"");
        let recovered: CheckKind = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered, kind);
        assert_eq!(kind.to_string(), "fetch_unusable");
    }

    #[test]
    fn test_parse_fetch_max_age_secs_defaults_and_accepts_a_positive_window() {
        assert_eq!(parse_fetch_max_age_secs(None), default_fetch_max_age());
        assert_eq!(
            parse_fetch_max_age_secs(Some("  ")),
            default_fetch_max_age()
        );
        assert_eq!(parse_fetch_max_age_secs(Some("0")), default_fetch_max_age());
        assert_eq!(
            parse_fetch_max_age_secs(Some("-5")),
            default_fetch_max_age()
        );
        assert_eq!(
            parse_fetch_max_age_secs(Some("nope")),
            default_fetch_max_age()
        );
        assert_eq!(
            parse_fetch_max_age_secs(Some("3600")),
            chrono::Duration::seconds(3600)
        );
        assert_eq!(
            default_fetch_max_age(),
            chrono::Duration::seconds(DEFAULT_FETCH_MAX_AGE_SECS)
        );
        assert_eq!(DEFAULT_FETCH_MAX_AGE_SECS, 24 * 60 * 60);
    }

    #[test]
    fn test_admission_context_from_tool_ignores_blank_session() {
        let stamped = AdmissionContext::from_tool(" daemon ", Some("  "));
        assert_eq!(stamped.execution_origin, "daemon");
        assert_eq!(stamped.session_id, None);
        let with_session = stamped.with_session(" sess-9 ");
        assert_eq!(with_session.session_id.as_deref(), Some("sess-9"));
        let direct = AdmissionContext::direct();
        assert_eq!(direct.execution_origin, "direct");
        assert_eq!(direct.session_id, None);
        assert_eq!(direct.fetch_max_age, default_fetch_max_age());
    }
}
