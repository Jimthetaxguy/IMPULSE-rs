//! Stale-basis predicate — fail-closed freshness check for settlement.
//!
//! A run records the *basis* it planned against: the set of sources it actually
//! read, each captured with a cheap version handle. At settlement the predicate
//! re-reads every handle and compares. Settlement holds only when every pair
//! still matches; any mismatch fails closed and names what moved, so the retry
//! can re-plan against current state instead of guessing.
//!
//! The predicate checks the basis, not the world. Sources with no version
//! handle (a web page, a human's intention) are declared
//! [`BasisDeclaration::Unverifiable`]; they never block settlement but appear
//! in every verdict as assumptions, which keeps the honest gap visible rather
//! than laundering it into a green check.
//!
//! This module is deliberately self-contained: it owns its own types and does
//! not depend on the WorkItem/WorkGraph types, whose ADR is not yet ratified.
//! Wiring the predicate into settlement is a separate change.
//!
//! ```no_run
//! use impulse_rs::basis::{BasisDeclaration, BasisSet};
//!
//! let basis = BasisSet::capture(vec![
//!     BasisDeclaration::file("/etc/hosts"),
//!     BasisDeclaration::unverifiable("operator confirmed the window is open"),
//! ])?;
//! // ... plan and execute ...
//! if !basis.verify().settles() {
//!     // fail closed: re-plan against current state
//! }
//! # Ok::<(), impulse_rs::basis::BasisError>(())
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Default cap on basis size. A run that reads three files and one registry
/// entry carries four pairs, not a snapshot of the machine.
pub const DEFAULT_BASIS_CAP: usize = 32;

/// Number of trailing lines hashed for a [`BasisSource::LedgerSection`].
///
/// Fixed rather than per-source so the tail hash is reproducible from the
/// path alone at settlement time.
pub const LEDGER_TAIL_LINES: usize = 20;

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum BasisError {
    /// Reading nothing before acting is not a valid plan.
    #[error("empty basis: a run that reads nothing before acting has no plan to settle")]
    EmptyBasis,

    /// Over-cap is a design smell in the WorkItem, and the report says so.
    #[error(
        "basis of {count} sources exceeds the cap of {cap}: a WorkItem that reads this much is \
         carrying a snapshot of the machine instead of a plan — split it into narrower items"
    )]
    OverCap { count: usize, cap: usize },

    /// The basis is a set; the same handle cannot be recorded at two versions.
    #[error("duplicate basis source `{name}`: the basis is a set, one version handle per source")]
    DuplicateSource { name: String },

    #[error("cannot read basis source `{name}`: {reason}")]
    Unreadable { name: String, reason: String },
}

// ============================================================================
// Declarations — a source's identity, before its version is read
// ============================================================================

/// A source a run intends to read, named without its version.
///
/// [`BasisSet::capture`] turns each declaration into a [`BasisSource`] by
/// reading the current version handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BasisDeclaration {
    /// A file, versioned by the SHA-256 of its contents.
    File(PathBuf),
    /// A git ref in a repository, versioned by the commit it resolves to.
    Git { repo: PathBuf, refname: String },
    /// A JSON registry entry, versioned by the value at a JSON pointer.
    Registry { path: PathBuf, json_pointer: String },
    /// An append-only ledger, versioned by line count plus a tail hash.
    Ledger(PathBuf),
    /// A source with no version handle. Recorded as an assumption.
    Unverifiable(String),
}

impl BasisDeclaration {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    pub fn git(repo: impl Into<PathBuf>, refname: impl Into<String>) -> Self {
        Self::Git {
            repo: repo.into(),
            refname: refname.into(),
        }
    }

    pub fn registry(path: impl Into<PathBuf>, json_pointer: impl Into<String>) -> Self {
        Self::Registry {
            path: path.into(),
            json_pointer: json_pointer.into(),
        }
    }

    pub fn ledger(path: impl Into<PathBuf>) -> Self {
        Self::Ledger(path.into())
    }

    pub fn unverifiable(description: impl Into<String>) -> Self {
        Self::Unverifiable(description.into())
    }
}

// ============================================================================
// Sources — identity plus the version captured at read time
// ============================================================================

/// A source paired with the version the run planned against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BasisSource {
    FileHash {
        path: PathBuf,
        sha256: String,
    },
    GitRef {
        repo: PathBuf,
        refname: String,
        commit: String,
    },
    RegistryEntry {
        path: PathBuf,
        json_pointer: String,
        updated_at: String,
    },
    LedgerSection {
        path: PathBuf,
        line_count: usize,
        tail_sha256: String,
    },
    Unverifiable {
        description: String,
    },
}

impl BasisSource {
    /// Read the current version handle for this source's identity.
    ///
    /// `Ok(None)` means the source has no handle to read (unverifiable).
    fn read_current(&self) -> Result<Option<String>, BasisError> {
        let current = match self {
            Self::FileHash { path, .. } => file_sha256(path)?,
            Self::GitRef { repo, refname, .. } => git_commit(repo, refname)?,
            Self::RegistryEntry {
                path, json_pointer, ..
            } => registry_value(path, json_pointer)?,
            Self::LedgerSection { path, .. } => {
                let (line_count, tail) = ledger_handle(path)?;
                encode_ledger(line_count, &tail)
            }
            Self::Unverifiable { .. } => return Ok(None),
        };
        Ok(Some(current))
    }

    /// The version this source was captured at, or `None` if unverifiable.
    pub fn planned_version(&self) -> Option<String> {
        match self {
            Self::FileHash { sha256, .. } => Some(sha256.clone()),
            Self::GitRef { commit, .. } => Some(commit.clone()),
            Self::RegistryEntry { updated_at, .. } => Some(updated_at.clone()),
            Self::LedgerSection {
                line_count,
                tail_sha256,
                ..
            } => Some(encode_ledger(*line_count, tail_sha256)),
            Self::Unverifiable { .. } => None,
        }
    }

    /// Stable identity used to name this source in mismatches and reports.
    pub fn id(&self) -> String {
        match self {
            Self::FileHash { path, .. } => format!("file:{}", path.display()),
            Self::GitRef { repo, refname, .. } => {
                format!("git:{}#{}", repo.display(), refname)
            }
            Self::RegistryEntry {
                path, json_pointer, ..
            } => format!("registry:{}{}", path.display(), json_pointer),
            Self::LedgerSection { path, .. } => format!("ledger:{}", path.display()),
            Self::Unverifiable { description } => format!("unverifiable:{description}"),
        }
    }

    fn check(&self) -> Freshness {
        let planned = match self.planned_version() {
            Some(planned) => planned,
            None => return Freshness::Unverifiable,
        };
        // Fail closed: a source that cannot be re-read is a mismatch, not a pass.
        let found = match self.read_current() {
            Ok(Some(found)) => found,
            Ok(None) => return Freshness::Unverifiable,
            Err(err) => format!("<unreadable: {err}>"),
        };
        if found == planned {
            Freshness::Match
        } else {
            Freshness::Mismatch(Mismatch {
                source: self.id(),
                planned,
                found,
            })
        }
    }
}

impl fmt::Display for BasisSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id())
    }
}

enum Freshness {
    Match,
    Mismatch(Mismatch),
    Unverifiable,
}

// ============================================================================
// Verdict
// ============================================================================

/// A source whose version moved between plan and settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub source: String,
    pub planned: String,
    pub found: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} planned={} found={}",
            self.source, self.planned, self.found
        )
    }
}

/// The result of re-verifying a basis at settlement time.
///
/// Both variants carry the unverifiable sources: assumptions surface in every
/// verdict, including the fresh one, and can never be silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum BasisVerdict {
    Fresh {
        unverifiable: Vec<String>,
    },
    Stale {
        mismatches: Vec<Mismatch>,
        unverifiable: Vec<String>,
    },
}

impl BasisVerdict {
    /// Whether settlement may proceed. Unverifiable sources never block.
    pub fn settles(&self) -> bool {
        matches!(self, Self::Fresh { .. })
    }

    pub fn mismatches(&self) -> &[Mismatch] {
        match self {
            Self::Fresh { .. } => &[],
            Self::Stale { mismatches, .. } => mismatches,
        }
    }

    /// Assumptions recorded in this basis, present in every verdict.
    pub fn unverifiable(&self) -> &[String] {
        match self {
            Self::Fresh { unverifiable } | Self::Stale { unverifiable, .. } => unverifiable,
        }
    }

    /// Human-readable settlement report.
    pub fn report(&self) -> String {
        let mut out = match self {
            Self::Fresh { .. } => String::from("basis fresh: settlement may proceed\n"),
            Self::Stale { mismatches, .. } => {
                let mut s = format!(
                    "basis stale: settlement held, {} source(s) moved since plan\n",
                    mismatches.len()
                );
                for m in mismatches {
                    s.push_str(&format!("  moved: {m}\n"));
                }
                s
            }
        };
        for assumption in self.unverifiable() {
            out.push_str(&format!("  assumption (unverifiable): {assumption}\n"));
        }
        out
    }
}

// ============================================================================
// BasisSet
// ============================================================================

/// The bounded set of (source, version) pairs a run planned against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasisSet {
    sources: Vec<BasisSource>,
    cap: usize,
}

impl BasisSet {
    /// Read the current version of each declared source and record the basis.
    pub fn capture(declarations: Vec<BasisDeclaration>) -> Result<Self, BasisError> {
        Self::capture_with_cap(declarations, DEFAULT_BASIS_CAP)
    }

    pub fn capture_with_cap(
        declarations: Vec<BasisDeclaration>,
        cap: usize,
    ) -> Result<Self, BasisError> {
        let mut sources = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            sources.push(capture_one(declaration)?);
        }
        Self::from_sources_with_cap(sources, cap)
    }

    /// Rebuild a basis from already-captured pairs (e.g. rehydrated from disk).
    pub fn from_sources(sources: Vec<BasisSource>) -> Result<Self, BasisError> {
        Self::from_sources_with_cap(sources, DEFAULT_BASIS_CAP)
    }

    pub fn from_sources_with_cap(
        sources: Vec<BasisSource>,
        cap: usize,
    ) -> Result<Self, BasisError> {
        if sources.is_empty() {
            return Err(BasisError::EmptyBasis);
        }
        if sources.len() > cap {
            return Err(BasisError::OverCap {
                count: sources.len(),
                cap,
            });
        }
        let mut seen = HashSet::with_capacity(sources.len());
        for source in &sources {
            let id = source.id();
            if !seen.insert(id.clone()) {
                return Err(BasisError::DuplicateSource { name: id });
            }
        }
        Ok(Self { sources, cap })
    }

    pub fn sources(&self) -> &[BasisSource] {
        &self.sources
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Always false — an empty basis cannot be constructed.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Re-read every version handle and compare against the planned versions.
    pub fn verify(&self) -> BasisVerdict {
        let mut mismatches = Vec::new();
        let mut unverifiable = Vec::new();
        for source in &self.sources {
            match source.check() {
                Freshness::Match => {}
                Freshness::Mismatch(mismatch) => mismatches.push(mismatch),
                Freshness::Unverifiable => unverifiable.push(source.id()),
            }
        }
        if mismatches.is_empty() {
            BasisVerdict::Fresh { unverifiable }
        } else {
            BasisVerdict::Stale {
                mismatches,
                unverifiable,
            }
        }
    }
}

// ============================================================================
// Version handle readers
// ============================================================================

fn capture_one(declaration: BasisDeclaration) -> Result<BasisSource, BasisError> {
    Ok(match declaration {
        BasisDeclaration::File(path) => {
            let sha256 = file_sha256(&path)?;
            BasisSource::FileHash { path, sha256 }
        }
        BasisDeclaration::Git { repo, refname } => {
            let commit = git_commit(&repo, &refname)?;
            BasisSource::GitRef {
                repo,
                refname,
                commit,
            }
        }
        BasisDeclaration::Registry { path, json_pointer } => {
            let updated_at = registry_value(&path, &json_pointer)?;
            BasisSource::RegistryEntry {
                path,
                json_pointer,
                updated_at,
            }
        }
        BasisDeclaration::Ledger(path) => {
            let (line_count, tail_sha256) = ledger_handle(&path)?;
            BasisSource::LedgerSection {
                path,
                line_count,
                tail_sha256,
            }
        }
        BasisDeclaration::Unverifiable(description) => BasisSource::Unverifiable { description },
    })
}

fn unreadable(name: impl fmt::Display, reason: impl fmt::Display) -> BasisError {
    BasisError::Unreadable {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn file_sha256(path: &Path) -> Result<String, BasisError> {
    let bytes = std::fs::read(path).map_err(|e| unreadable(path.display(), e))?;
    Ok(sha256_hex(&bytes))
}

fn git_commit(repo: &Path, refname: &str) -> Result<String, BasisError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("rev-parse")
        .arg("--verify")
        .arg(format!("{refname}^{{commit}}"))
        .output()
        .map_err(|e| unreadable(format!("{}#{refname}", repo.display()), e))?;
    if !output.status.success() {
        return Err(unreadable(
            format!("{}#{refname}", repo.display()),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn registry_value(path: &Path, json_pointer: &str) -> Result<String, BasisError> {
    let text = std::fs::read_to_string(path).map_err(|e| unreadable(path.display(), e))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| unreadable(path.display(), e))?;
    let found = value.pointer(json_pointer).ok_or_else(|| {
        unreadable(
            format!("{}{json_pointer}", path.display()),
            "json pointer not found",
        )
    })?;
    Ok(match found {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Line count plus the SHA-256 of the last [`LEDGER_TAIL_LINES`] lines.
fn ledger_handle(path: &Path) -> Result<(usize, String), BasisError> {
    let text = std::fs::read_to_string(path).map_err(|e| unreadable(path.display(), e))?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(LEDGER_TAIL_LINES);
    let tail = lines[start..].join("\n");
    Ok((lines.len(), sha256_hex(tail.as_bytes())))
}

fn encode_ledger(line_count: usize, tail_sha256: &str) -> String {
    format!("{line_count}:{tail_sha256}")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("write fixture");
        path
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Real git repo with one commit on `main`.
    fn init_repo(dir: &Path) {
        git(dir, &["init", "--quiet", "--initial-branch=main"]);
        git(dir, &["config", "user.email", "basis@test.local"]);
        git(dir, &["config", "user.name", "Basis Test"]);
        fs::write(dir.join("tracked.txt"), "one\n").expect("write tracked file");
        git(dir, &["add", "tracked.txt"]);
        git(dir, &["commit", "--quiet", "-m", "one"]);
    }

    // --- Acceptance check 1: modified basis fails settlement, named ---------

    #[test]
    fn test_verify_modified_basis_file_returns_stale_with_named_mismatch() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(&dir, "plan-input.md", "planned contents\n");
        let basis = BasisSet::capture(vec![BasisDeclaration::file(&path)]).expect("capture");

        assert!(basis.verify().settles(), "basis is fresh before the change");

        fs::write(&path, "someone else moved this\n").expect("mutate");
        let verdict = basis.verify();

        assert!(!verdict.settles(), "modified basis must fail closed");
        let mismatches = verdict.mismatches();
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].source, format!("file:{}", path.display()));
        assert_ne!(mismatches[0].planned, mismatches[0].found);
        assert!(verdict.report().contains(&path.display().to_string()));
    }

    #[test]
    fn test_verify_deleted_basis_file_fails_closed_as_mismatch() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(&dir, "gone.md", "here for now\n");
        let basis = BasisSet::capture(vec![BasisDeclaration::file(&path)]).expect("capture");

        fs::remove_file(&path).expect("remove");
        let verdict = basis.verify();

        assert!(!verdict.settles(), "unreadable source must not settle");
        assert!(verdict.mismatches()[0].found.contains("unreadable"));
    }

    #[test]
    fn test_verify_moved_git_ref_returns_stale() {
        let dir = TempDir::new().expect("tempdir");
        init_repo(dir.path());
        let basis =
            BasisSet::capture(vec![BasisDeclaration::git(dir.path(), "main")]).expect("capture");
        assert!(basis.verify().settles());

        fs::write(dir.path().join("tracked.txt"), "two\n").expect("write");
        git(dir.path(), &["add", "tracked.txt"]);
        git(dir.path(), &["commit", "--quiet", "-m", "two"]);

        let verdict = basis.verify();
        assert!(!verdict.settles(), "moved ref must fail closed");
        assert!(verdict.mismatches()[0].source.contains("#main"));
    }

    #[test]
    fn test_verify_changed_registry_entry_returns_stale() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(
            &dir,
            "registry.json",
            r#"{"agents":{"sidecar":{"updated_at":"2026-08-09T10:00:00Z"}}}"#,
        );
        let basis = BasisSet::capture(vec![BasisDeclaration::registry(
            &path,
            "/agents/sidecar/updated_at",
        )])
        .expect("capture");
        assert!(basis.verify().settles());

        fs::write(
            &path,
            r#"{"agents":{"sidecar":{"updated_at":"2026-08-10T09:00:00Z"}}}"#,
        )
        .expect("mutate");

        let verdict = basis.verify();
        assert!(!verdict.settles());
        assert_eq!(verdict.mismatches()[0].planned, "2026-08-09T10:00:00Z");
        assert_eq!(verdict.mismatches()[0].found, "2026-08-10T09:00:00Z");
    }

    /// The ledger-liveness lesson: an activity-ledger entry is a basis source
    /// like any other, so a plan built on the ledger inherits freshness checks.
    #[test]
    fn test_verify_appended_ledger_section_returns_stale() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(
            &dir,
            "ledger.md",
            "- agent: sidecar-fixer status: running\n",
        );
        let basis = BasisSet::capture(vec![BasisDeclaration::ledger(&path)]).expect("capture");
        assert!(basis.verify().settles());

        fs::write(
            &path,
            "- agent: sidecar-fixer status: running\n- correction: died a day ago\n",
        )
        .expect("append");

        let verdict = basis.verify();
        assert!(!verdict.settles(), "ledger drift must fail closed");
        assert!(verdict.mismatches()[0].source.starts_with("ledger:"));
    }

    #[test]
    fn test_verify_ledger_tail_edit_same_line_count_returns_stale() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(&dir, "ledger.md", "line a\nline b\n");
        let basis = BasisSet::capture(vec![BasisDeclaration::ledger(&path)]).expect("capture");

        fs::write(&path, "line a\nline B EDITED\n").expect("edit in place");
        assert!(
            !basis.verify().settles(),
            "same line count with edited tail must still fail closed"
        );
    }

    // --- Acceptance check 2: empty basis fails registration ----------------

    #[test]
    fn test_capture_empty_declarations_returns_empty_basis_error() {
        let err = BasisSet::capture(vec![]).expect_err("empty basis must not construct");
        assert!(matches!(err, BasisError::EmptyBasis));
        assert!(err.to_string().contains("empty basis"));

        let err = BasisSet::from_sources(vec![]).expect_err("empty basis must not construct");
        assert!(matches!(err, BasisError::EmptyBasis));
    }

    // --- Acceptance check 3: unverifiable surfaces, never blocks -----------

    #[test]
    fn test_verify_unverifiable_source_never_blocks_but_always_surfaces() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(&dir, "input.md", "stable\n");
        let assumption = "operator said the maintenance window is open";
        let basis = BasisSet::capture(vec![
            BasisDeclaration::file(&path),
            BasisDeclaration::unverifiable(assumption),
        ])
        .expect("capture");

        // Fresh verdict: settles, and still lists the assumption.
        let fresh = basis.verify();
        assert!(fresh.settles(), "an unverifiable source must never block");
        assert_eq!(fresh.unverifiable().len(), 1);
        assert!(fresh.unverifiable()[0].contains(assumption));
        assert!(fresh.report().contains(assumption));

        // Stale verdict: the assumption is still carried, not dropped.
        fs::write(&path, "moved\n").expect("mutate");
        let stale = basis.verify();
        assert!(!stale.settles());
        assert_eq!(stale.unverifiable().len(), 1);
        assert!(stale.unverifiable()[0].contains(assumption));
        assert!(stale.report().contains(assumption));
    }

    #[test]
    fn test_verify_all_unverifiable_basis_settles_and_reports_every_assumption() {
        let basis = BasisSet::capture(vec![
            BasisDeclaration::unverifiable("a web page said so"),
            BasisDeclaration::unverifiable("the requester's intention"),
        ])
        .expect("capture");

        let verdict = basis.verify();
        assert!(verdict.settles());
        assert_eq!(
            verdict.unverifiable().len(),
            2,
            "no assumption may be silently dropped"
        );
        assert_eq!(verdict.report().matches("assumption").count(), 2);
    }

    // --- Acceptance check 4: ten-pair basis verifies in under a second ------

    #[test]
    fn test_verify_ten_pair_basis_completes_in_under_one_second() {
        let dir = TempDir::new().expect("tempdir");
        let repo = dir.path().join("repo");
        fs::create_dir(&repo).expect("mkdir repo");
        init_repo(&repo);

        let mut declarations = vec![
            BasisDeclaration::git(&repo, "main"),
            BasisDeclaration::registry(
                write(&dir, "registry.json", r#"{"entry":{"updated_at":"t0"}}"#),
                "/entry/updated_at",
            ),
            BasisDeclaration::ledger(write(&dir, "ledger.md", "a\nb\nc\n")),
        ];
        for i in 0..7 {
            declarations.push(BasisDeclaration::file(write(
                &dir,
                &format!("input-{i}.md"),
                &format!("contents {i}\n").repeat(64),
            )));
        }
        assert_eq!(declarations.len(), 10);

        let basis = BasisSet::capture(declarations).expect("capture");
        let started = Instant::now();
        let verdict = basis.verify();
        let elapsed = started.elapsed();

        assert!(verdict.settles());
        assert!(
            elapsed.as_secs_f64() < 1.0,
            "ten-pair verify took {elapsed:?}; freshness checking that is slow gets skipped"
        );
    }

    // --- Bounds ------------------------------------------------------------

    #[test]
    fn test_capture_with_cap_over_cap_returns_error_naming_the_workitem_smell() {
        let dir = TempDir::new().expect("tempdir");
        let declarations: Vec<_> = (0..4)
            .map(|i| BasisDeclaration::file(write(&dir, &format!("f{i}.md"), "x")))
            .collect();

        let err = BasisSet::capture_with_cap(declarations, 3).expect_err("over cap must not build");
        assert!(matches!(err, BasisError::OverCap { count: 4, cap: 3 }));
        let message = err.to_string();
        assert!(
            message.contains("WorkItem"),
            "must name the smell: {message}"
        );
        assert!(message.contains("snapshot of the machine"));
    }

    #[test]
    fn test_capture_default_cap_is_thirty_two_and_recorded_on_the_set() {
        let dir = TempDir::new().expect("tempdir");
        let basis = BasisSet::capture(vec![BasisDeclaration::file(write(&dir, "f.md", "x"))])
            .expect("capture");
        assert_eq!(DEFAULT_BASIS_CAP, 32);
        assert_eq!(basis.cap(), 32);
        assert_eq!(basis.len(), 1);
        assert!(!basis.is_empty());
    }

    #[test]
    fn test_capture_duplicate_source_returns_duplicate_source_error() {
        let dir = TempDir::new().expect("tempdir");
        let path = write(&dir, "same.md", "x");
        let err = BasisSet::capture(vec![
            BasisDeclaration::file(&path),
            BasisDeclaration::file(&path),
        ])
        .expect_err("a set holds one version per source");
        assert!(matches!(err, BasisError::DuplicateSource { .. }));
    }

    #[test]
    fn test_capture_unreadable_declared_source_returns_unreadable_error() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("never-existed.md");
        let err = BasisSet::capture(vec![BasisDeclaration::file(missing)])
            .expect_err("cannot plan against a source you could not read");
        assert!(matches!(err, BasisError::Unreadable { .. }));
    }

    #[test]
    fn test_basis_error_display_names_the_problem_for_every_variant() {
        assert!(BasisError::EmptyBasis.to_string().contains("empty basis"));

        let over_cap = BasisError::OverCap { count: 40, cap: 32 }.to_string();
        assert!(over_cap.contains("40") && over_cap.contains("32"));
        assert!(over_cap.contains("WorkItem"));

        let duplicate = BasisError::DuplicateSource {
            name: "file:/tmp/a".into(),
        }
        .to_string();
        assert!(duplicate.contains("file:/tmp/a"));
        assert!(duplicate.contains("duplicate basis source"));

        let unreadable = BasisError::Unreadable {
            name: "file:/tmp/b".into(),
            reason: "no such file".into(),
        }
        .to_string();
        assert!(unreadable.contains("file:/tmp/b"));
        assert!(unreadable.contains("no such file"));
    }

    #[test]
    fn test_basis_verdict_round_trips_through_json() {
        let stale = BasisVerdict::Stale {
            mismatches: vec![Mismatch {
                source: "ledger:/tmp/ledger.md".into(),
                planned: "3:aaa".into(),
                found: "4:bbb".into(),
            }],
            unverifiable: vec!["unverifiable:a human's intention".into()],
        };
        let recovered: BasisVerdict =
            serde_json::from_str(&serde_json::to_string(&stale).expect("serialize"))
                .expect("deserialize");
        assert_eq!(stale, recovered);

        let fresh = BasisVerdict::Fresh {
            unverifiable: Vec::new(),
        };
        let recovered: BasisVerdict =
            serde_json::from_str(&serde_json::to_string(&fresh).expect("serialize"))
                .expect("deserialize");
        assert_eq!(fresh, recovered);
    }

    #[test]
    fn test_mismatch_display_names_source_planned_and_found() {
        let mismatch = Mismatch {
            source: "file:/tmp/a".into(),
            planned: "aaa".into(),
            found: "bbb".into(),
        };
        let rendered = mismatch.to_string();
        assert!(rendered.contains("file:/tmp/a"));
        assert!(rendered.contains("planned=aaa"));
        assert!(rendered.contains("found=bbb"));
    }

    #[test]
    fn test_basis_set_round_trips_through_json() {
        let dir = TempDir::new().expect("tempdir");
        let basis = BasisSet::capture(vec![
            BasisDeclaration::file(write(&dir, "f.md", "x")),
            BasisDeclaration::unverifiable("a human's intention"),
        ])
        .expect("capture");

        let json = serde_json::to_string(&basis).expect("serialize");
        let restored: BasisSet = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(basis, restored);
        assert!(restored.verify().settles());
    }
}
