//! Subprocess runner for the `sem` CLI tool.
//!
//! All interactions with `sem` go through this module. It spawns `sem` as a child
//! process, captures JSON output, and parses it into our types.

use anyhow::{bail, Context, Result};
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process_util::{run_with_limits, run_with_timeout, MAX_STDERR_BYTES};
use crate::storage::sanitize_filename;

use super::types::*;

/// Hard timeout for any `sem` subprocess. A stuck `sem` (e.g. on a pathological
/// repo) must never hang the caller indefinitely.
const SEM_TIMEOUT: Duration = Duration::from_secs(30);

/// `sem --version` answers at once; waiting longer only stalls `sem-status`.
const SEM_VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// Most output `sem diff` may print. Its JSON carries every changed entity's
/// full source on both sides, and a nested entity repeats its parent's text,
/// so a session that adds a large generated file can pass the default cap
/// with a complete answer. The parser skips that text.
const SEM_DIFF_MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    /// The program a test on this thread runs in place of `sem` on PATH, and
    /// the timeout every call then uses. Per thread, so tests don't need to
    /// change PATH for the whole process.
    static TEST_SEM: std::cell::RefCell<Option<(std::path::PathBuf, Duration)>> =
        const { std::cell::RefCell::new(None) };
}

fn sem_program() -> OsString {
    #[cfg(test)]
    if let Some((program, _)) = TEST_SEM.with(|sem| sem.borrow().clone()) {
        return program.into_os_string();
    }
    OsString::from("sem")
}

fn sem_timeout(default: Duration) -> Duration {
    #[cfg(test)]
    if let Some((_, timeout)) = TEST_SEM.with(|sem| sem.borrow().clone()) {
        return timeout;
    }
    default
}

/// Runs `f` with `program` standing in for `sem`, and `timeout` for every
/// sem timeout, on this thread only. A nested call restores the outer
/// setting when it returns.
#[cfg(test)]
pub(crate) fn with_test_sem<T>(program: &Path, timeout: Duration, f: impl FnOnce() -> T) -> T {
    type Setting = Option<(std::path::PathBuf, Duration)>;
    struct Restore(Setting);
    impl Drop for Restore {
        fn drop(&mut self) {
            let outer = self.0.take();
            TEST_SEM.with(|sem| *sem.borrow_mut() = outer);
        }
    }
    let outer = TEST_SEM.with(|sem| sem.replace(Some((program.to_path_buf(), timeout))));
    let _restore = Restore(outer);
    f()
}

/// A `sem` command that answers from this machine only.
///
/// With a sem cloud login and consent, `sem diff` prints its answer and then
/// goes on to a relations pass of minutes before exiting, which the timeout
/// would end, throwing the complete answer away. `SEM_LOCAL=1` is sem's own
/// switch for keeping a run local.
fn sem_command() -> Command {
    let mut cmd = Command::new(sem_program());
    cmd.env("SEM_LOCAL", "1");
    cmd
}

/// Check whether the `sem` CLI is available on PATH.
pub fn sem_available() -> bool {
    which::which(sem_program()).is_ok()
}

fn require_sem() -> Result<()> {
    if !sem_available() {
        bail!("sem CLI not found on PATH. Install from https://github.com/Ataraxy-Labs/sem");
    }
    Ok(())
}

/// The version `sem --version` reports.
pub fn sem_version() -> Result<String> {
    require_sem()?;
    let mut cmd = sem_command();
    cmd.arg("--version");
    let output = run_with_timeout(cmd, sem_timeout(SEM_VERSION_TIMEOUT))
        .context("failed to run `sem --version`")?;
    if !output.status.success() {
        bail!(
            "sem --version failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The revision argument for `sem diff`.
///
/// sem reads its positional arguments as refs or files and anything after
/// `--` as pathspecs, so `--` can't keep a ref from being read as an option.
/// A ref starting with `-` is refused instead; no git ref name or revision
/// expression starts with one.
fn diff_range(base_ref: &str, head_ref: &str) -> Result<String> {
    for (label, value) in [("base ref", base_ref), ("head ref", head_ref)] {
        if value.starts_with('-') {
            bail!("{label} `{value}` starts with `-`, which sem would read as an option");
        }
    }
    Ok(if head_ref.is_empty() {
        base_ref.to_string()
    } else {
        format!("{base_ref}..{head_ref}")
    })
}

/// Run `sem diff` between two Git refs and return parsed entity changes.
///
/// # Arguments
/// * `repo_path` — path to the Git repository
/// * `base_ref` — base Git ref (commit, branch, tag)
/// * `head_ref` — head Git ref (commit, branch, tag, or empty for working tree)
pub fn run_semantic_diff(
    repo_path: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<EntityChange>> {
    require_sem()?;
    let range = diff_range(base_ref, head_ref)?;

    let mut cmd = sem_command();
    cmd.arg("diff")
        .arg(&range)
        .arg("--format")
        .arg("json")
        .current_dir(repo_path);
    let output = run_with_limits(
        cmd,
        sem_timeout(SEM_TIMEOUT),
        SEM_DIFF_MAX_OUTPUT_BYTES,
        MAX_STDERR_BYTES,
    )
    .context("failed to run `sem diff`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // sem returns non-zero when there are no changes in some versions
        if stderr.contains("no changes") || stderr.contains("No changes") {
            return Ok(Vec::new());
        }
        anyhow::bail!("sem diff failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    parse_sem_diff_output(&stdout)
}

/// Run `sem blame` on a file and return entity-level blame entries.
pub fn run_semantic_blame(repo_path: &Path, file_path: &str) -> Result<Vec<SemanticBlameEntry>> {
    require_sem()?;

    // `--` ends sem's options, so a file name starting with `-` stays a file.
    let mut cmd = sem_command();
    cmd.arg("blame")
        .arg("--format")
        .arg("json")
        .arg("--")
        .arg(file_path)
        .current_dir(repo_path);
    let output =
        run_with_timeout(cmd, sem_timeout(SEM_TIMEOUT)).context("failed to run `sem blame`")?;

    if !output.status.success() {
        anyhow::bail!(
            "sem blame failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    parse_sem_blame_output(&stdout, file_path)
}

/// Run `sem impact` for a given entity and return its blast radius.
pub fn run_semantic_impact(repo_path: &Path, entity_name: &str) -> Result<ImpactResult> {
    require_sem()?;

    // `--` ends sem's options, so the entity name is never read as one.
    let mut cmd = sem_command();
    cmd.arg("impact")
        .arg("--format")
        .arg("json")
        .arg("--")
        .arg(entity_name)
        .current_dir(repo_path);
    let output =
        run_with_timeout(cmd, sem_timeout(SEM_TIMEOUT)).context("failed to run `sem impact`")?;

    if !output.status.success() {
        anyhow::bail!(
            "sem impact failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    parse_sem_impact_output(&String::from_utf8_lossy(&output.stdout))
}

/// Capture semantic diff at session end and store it.
///
/// Called during session-end to record what semantically changed during the session.
/// The result is stored in `.impulse/semantic_diffs/<session_id>.json`.
pub fn capture_semantic_diff(
    impulse_dir: &Path,
    repo_path: &Path,
    session_id: &str,
    base_ref: &str,
    head_ref: &str,
) -> Result<SemanticDiffReport> {
    let changes = run_semantic_diff(repo_path, base_ref, head_ref)?;

    let report = SemanticDiffReport::new(
        session_id.to_string(),
        base_ref.to_string(),
        head_ref.to_string(),
        changes,
    );

    // Store the report — sanitize session_id to prevent path traversal
    let safe_id = sanitize_filename(session_id);
    let diff_dir = impulse_dir.join("semantic_diffs");
    std::fs::create_dir_all(&diff_dir).context("failed to create semantic_diffs directory")?;

    let report_path = diff_dir.join(format!("{}.json", safe_id));
    let json = serde_json::to_string_pretty(&report)
        .context("failed to serialize semantic diff report")?;

    // Atomic write: temp file + rename
    let tmp_path = diff_dir.join(format!(".{}.{}.tmp", safe_id, std::process::id()));
    std::fs::write(&tmp_path, json.as_bytes())
        .context("failed to write semantic diff temp file")?;
    std::fs::rename(&tmp_path, &report_path)
        .context("failed to rename semantic diff report into place")?;

    Ok(report)
}

/// Load a previously stored semantic diff report for a session.
#[cfg(test)]
pub fn load_semantic_diff(
    impulse_dir: &Path,
    session_id: &str,
) -> Result<Option<SemanticDiffReport>> {
    let safe_id = sanitize_filename(session_id);
    let report_path = impulse_dir
        .join("semantic_diffs")
        .join(format!("{}.json", safe_id));

    if !report_path.exists() {
        return Ok(None);
    }

    let content =
        std::fs::read_to_string(&report_path).context("failed to read semantic diff report")?;
    let report: SemanticDiffReport =
        serde_json::from_str(&content).context("failed to parse semantic diff report")?;
    Ok(Some(report))
}

/// List all stored semantic diff session IDs.
#[cfg(test)]
pub fn list_semantic_diffs(impulse_dir: &Path) -> Result<Vec<String>> {
    let diff_dir = impulse_dir.join("semantic_diffs");
    if !diff_dir.exists() {
        return Ok(Vec::new());
    }

    let mut session_ids = Vec::new();
    for entry in std::fs::read_dir(&diff_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if !stem.starts_with('.') {
                    session_ids.push(stem.to_string());
                }
            }
        }
    }
    session_ids.sort();
    Ok(session_ids)
}

// ============================================================================
// JSON parsing helpers
// ============================================================================

/// Parse the JSON output from `sem diff --format json`.
///
/// Current sem prints an object whose `changes` list holds camelCase records
/// (`changeType`, `entityName`, `entityType`, `filePath`, ...). Earlier
/// shapes are still read: a bare list, an `entities` list, snake_case names,
/// and a nested `entity` object. Anything else is an error rather than a
/// guess: null, a number or an `{"error": ...}` object is not a list of
/// changes, and a change without a type, name, entity type or file is not
/// one to report.
///
/// The output is read straight into typed records, so fields the report
/// doesn't use (each change's full source text, for one) are skipped
/// instead of held in memory.
fn parse_sem_diff_output(json_str: &str) -> Result<Vec<EntityChange>> {
    let changes: SemDiffChanges =
        serde_json::from_str(json_str).context("failed to parse sem diff JSON")?;
    changes
        .0
        .into_iter()
        .enumerate()
        .map(|(index, change)| {
            change
                .into_change()
                .with_context(|| format!("sem diff change #{} is incomplete", index + 1))
        })
        .collect()
}

/// The change records of a `sem diff --format json` document.
struct SemDiffChanges(Vec<RawChange>);

impl<'de> Deserialize<'de> for SemDiffChanges {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SemDiffChangesVisitor)
    }
}

struct SemDiffChangesVisitor;

impl<'de> Visitor<'de> for SemDiffChangesVisitor {
    type Value = SemDiffChanges;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a list of changes, or an object with a `changes` list")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        Vec::deserialize(de::value::SeqAccessDeserializer::new(seq)).map(SemDiffChanges)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut changes = None;
        let mut error: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "changes" | "entities" if changes.is_none() => {
                    changes = Some(map.next_value::<Vec<RawChange>>()?);
                }
                "error" => error = Some(map.next_value()?),
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        match (error, changes) {
            (Some(error), _) => Err(de::Error::custom(format_args!(
                "sem reported an error: {error:.300}"
            ))),
            (None, Some(changes)) => Ok(SemDiffChanges(changes)),
            (None, None) => Err(de::Error::custom(
                "expected a `changes` list in the sem diff output",
            )),
        }
    }
}

/// One change as sem prints it, in any of the shapes it has used.
#[derive(Deserialize)]
struct RawChange {
    #[serde(alias = "changeType", alias = "kind", alias = "status")]
    change_type: Option<String>,
    #[serde(alias = "entityName", alias = "identifier")]
    name: Option<String>,
    #[serde(alias = "entityType", alias = "type")]
    entity_type: Option<String>,
    #[serde(alias = "filePath", alias = "file", alias = "path")]
    file_path: Option<String>,
    #[serde(alias = "startLine", alias = "line")]
    start_line: Option<u32>,
    #[serde(alias = "endLine")]
    end_line: Option<u32>,
    #[serde(alias = "parentId", alias = "parent_id")]
    parent: Option<String>,
    /// Earlier shape: the entity in its own object.
    entity: Option<RawEntity>,
    /// Earlier shape: the entity before a rename or move.
    #[serde(alias = "old")]
    previous: Option<RawEntity>,
    /// Current sem: the entity's name before a rename, otherwise null.
    #[serde(alias = "oldEntityName")]
    old_entity_name: Option<String>,
    /// Current sem: the entity's file before a move, otherwise null.
    #[serde(alias = "oldFilePath")]
    old_file_path: Option<String>,
    #[serde(alias = "oldStartLine")]
    old_start_line: Option<u32>,
    #[serde(alias = "oldEndLine")]
    old_end_line: Option<u32>,
    #[serde(alias = "oldParentId")]
    old_parent_id: Option<String>,
}

#[derive(Deserialize)]
struct RawEntity {
    #[serde(alias = "entityName", alias = "identifier")]
    name: Option<String>,
    #[serde(alias = "entityType", alias = "type")]
    entity_type: Option<String>,
    #[serde(alias = "filePath", alias = "file", alias = "path")]
    file_path: Option<String>,
    #[serde(alias = "startLine", alias = "line")]
    start_line: Option<u32>,
    #[serde(alias = "endLine")]
    end_line: Option<u32>,
    #[serde(alias = "parentId", alias = "parent_id")]
    parent: Option<String>,
}

impl RawChange {
    fn into_change(self) -> Result<EntityChange> {
        let kind = parse_change_kind(self.change_type.as_deref())?;
        let entity = match self.entity {
            Some(entity) => entity.into_info()?,
            None => RawEntity {
                name: self.name,
                entity_type: self.entity_type,
                file_path: self.file_path,
                start_line: self.start_line,
                end_line: self.end_line,
                parent: self.parent,
            }
            .into_info()?,
        };
        let previous = match self.previous {
            Some(previous) => Some(previous.into_info()?),
            None if self.old_entity_name.is_some() || self.old_file_path.is_some() => {
                Some(EntityInfo {
                    name: self.old_entity_name.unwrap_or_else(|| entity.name.clone()),
                    entity_type: entity.entity_type.clone(),
                    file_path: self
                        .old_file_path
                        .unwrap_or_else(|| entity.file_path.clone()),
                    start_line: self.old_start_line,
                    end_line: self.old_end_line,
                    parent: self.old_parent_id,
                })
            }
            None => None,
        };
        Ok(EntityChange {
            kind,
            entity,
            previous,
        })
    }
}

impl RawEntity {
    fn into_info(self) -> Result<EntityInfo> {
        // A name may be empty: sem names a JSON entity by its key, and an npm
        // lockfile's root package is keyed "" (an empty Markdown heading is
        // another). It must still be there.
        let Some(name) = self.name else {
            bail!("it has no entity name");
        };
        Ok(EntityInfo {
            name,
            entity_type: required(self.entity_type, "entity type")?,
            file_path: required(self.file_path, "file path")?,
            start_line: self.start_line,
            end_line: self.end_line,
            parent: self.parent,
        })
    }
}

fn required(value: Option<String>, what: &str) -> Result<String> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => bail!("it has no {what}"),
    }
}

fn parse_change_kind(value: Option<&str>) -> Result<ChangeKind> {
    let Some(value) = value else {
        bail!("it has no change type");
    };
    Ok(match value.to_ascii_lowercase().as_str() {
        "added" | "add" | "new" => ChangeKind::Added,
        "modified" | "modify" | "changed" | "change" => ChangeKind::Modified,
        "deleted" | "delete" | "removed" | "remove" => ChangeKind::Deleted,
        // sem reports an entity that only changed position in its file as
        // reordered; the report has no bucket of its own for that.
        "moved" | "move" | "reordered" => ChangeKind::Moved,
        "renamed" | "rename" => ChangeKind::Renamed,
        _ => bail!("its change type `{value:.40}` is not one sem documents"),
    })
}

/// One entity as `sem blame` and `sem impact` print it.
#[derive(Deserialize)]
struct RawSemEntity {
    name: String,
    #[serde(rename = "type")]
    entity_type: String,
    /// `sem impact` names each entity's file; `sem blame` entries are all in
    /// the blamed file.
    #[serde(default)]
    file: Option<String>,
    /// `[start, end]`.
    #[serde(default)]
    lines: Option<[u32; 2]>,
}

impl RawSemEntity {
    fn into_info(self, blamed_file: Option<&str>) -> Result<EntityInfo> {
        let file_path = self.file.or_else(|| blamed_file.map(str::to_string));
        Ok(EntityInfo {
            name: self.name,
            entity_type: required(Some(self.entity_type), "entity type")?,
            file_path: required(file_path, "file path")?,
            start_line: self.lines.map(|[start, _]| start),
            end_line: self.lines.map(|[_, end]| end),
            parent: None,
        })
    }
}

/// One `sem blame --format json` entry: an entity with its fields flat
/// beside the blame, and `commit` null for lines not yet committed.
#[derive(Deserialize)]
struct RawBlameEntry {
    name: String,
    #[serde(rename = "type")]
    entity_type: String,
    #[serde(default)]
    lines: Option<[u32; 2]>,
    author: String,
    date: String,
    commit: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

/// Parse the list `sem blame --format json` prints for `file_path`.
fn parse_sem_blame_output(json_str: &str, file_path: &str) -> Result<Vec<SemanticBlameEntry>> {
    let entries: Vec<RawBlameEntry> =
        serde_json::from_str(json_str).context("failed to parse sem blame JSON output")?;
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let entity = RawSemEntity {
                name: entry.name,
                entity_type: entry.entity_type,
                file: None,
                lines: entry.lines,
            }
            .into_info(Some(file_path))
            .with_context(|| format!("sem blame entry #{} is incomplete", index + 1))?;
            Ok(SemanticBlameEntry {
                entity,
                author: entry.author,
                commit: entry.commit,
                date: entry.date,
                message: entry.summary,
            })
        })
        .collect()
}

/// The parts of `sem impact --format json` the report uses: the entity, its
/// direct dependents, and how many entities the change reaches in all.
#[derive(Deserialize)]
struct RawImpact {
    entity: RawSemEntity,
    dependents: Vec<RawSemEntity>,
    #[serde(default)]
    impact: Option<RawImpactReach>,
}

#[derive(Deserialize)]
struct RawImpactReach {
    total: usize,
}

/// Parse what `sem impact --format json` prints for one entity.
fn parse_sem_impact_output(json_str: &str) -> Result<ImpactResult> {
    let impact: RawImpact =
        serde_json::from_str(json_str).context("failed to parse sem impact JSON output")?;
    let target = impact
        .entity
        .into_info(None)
        .context("sem impact's entity is incomplete")?;
    let dependents = impact
        .dependents
        .into_iter()
        .map(|entity| entity.into_info(None))
        .collect::<Result<Vec<_>>>()
        .context("a dependent in sem impact's output is incomplete")?;
    let blast_radius = impact.impact.map_or(dependents.len(), |reach| reach.total);
    Ok(ImpactResult {
        target,
        dependents,
        blast_radius,
    })
}

/// Fake `sem` scripts for tests here and in the sem command handlers.
#[cfg(test)]
pub(crate) mod fake_sem {
    use std::path::{Path, PathBuf};

    /// A shell script standing in for `sem`. Each run appends its arguments,
    /// one per line followed by a `--end--` line, to the returned log, and
    /// the `SEM_LOCAL` it saw to `env.log` beside it; then it runs `body`.
    pub(crate) fn fake_sem(body: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let log = dir.path().join("argv.log");
        let env_log = dir.path().join("env.log");
        let script = dir.path().join("sem");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 [ \"$1\" = --impulse-test-warm-up ] && exit 0\n\
                 printf '%s\\n' \"$@\" --end-- >> '{}'\n\
                 echo \"SEM_LOCAL=$SEM_LOCAL\" >> '{}'\n\
                 {body}\n",
                log.display(),
                env_log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // On Linux a process another test forks while the script is open for
        // writing keeps that descriptor until it execs, and running the
        // script fails with ETXTBSY until then. Wait that out here, once.
        for _ in 0..200 {
            match std::process::Command::new(&script)
                .arg("--impulse-test-warm-up")
                .status()
            {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                _ => break,
            }
        }
        (dir, script, log)
    }

    /// Each sem run's arguments, from a [`fake_sem`] log.
    pub(crate) fn logged_runs(log: &Path) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let mut runs = Vec::new();
        let mut current = Vec::new();
        for line in text.lines() {
            if line == "--end--" {
                runs.push(std::mem::take(&mut current));
            } else {
                current.push(line.to_string());
            }
        }
        runs
    }

    /// The `SEM_LOCAL=...` line each run logged, from a [`fake_sem`] log.
    pub(crate) fn logged_sem_local(log: &Path) -> Vec<String> {
        let env_log = log.with_file_name("env.log");
        std::fs::read_to_string(env_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fake_sem::{fake_sem, logged_runs, logged_sem_local};
    use super::*;

    #[test]
    fn test_sem_available_returns_bool() {
        // Just verify it doesn't panic — sem may or may not be installed
        let _ = sem_available();
    }

    #[test]
    fn test_parse_sem_diff_array_format() {
        let json = r#"[
            {
                "change_type": "added",
                "name": "new_function",
                "type": "function",
                "file_path": "src/lib.rs",
                "start_line": 10,
                "end_line": 20
            },
            {
                "change_type": "modified",
                "name": "existing_fn",
                "type": "function",
                "file_path": "src/lib.rs",
                "start_line": 30,
                "end_line": 45
            }
        ]"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].entity.name, "new_function");
        assert_eq!(changes[1].kind, ChangeKind::Modified);
        assert_eq!(changes[1].entity.name, "existing_fn");
    }

    #[test]
    fn test_parse_sem_diff_object_format() {
        let json = r#"{
            "changes": [
                {
                    "kind": "deleted",
                    "entity": {
                        "name": "old_fn",
                        "entity_type": "function",
                        "file_path": "src/old.rs"
                    }
                }
            ]
        }"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Deleted);
        assert_eq!(changes[0].entity.name, "old_fn");
    }

    #[test]
    fn test_parse_rename_with_previous() {
        let json = r#"[
            {
                "change_type": "renamed",
                "name": "new_name",
                "type": "function",
                "file_path": "src/lib.rs",
                "previous": {
                    "name": "old_name",
                    "type": "function",
                    "file_path": "src/lib.rs"
                }
            }
        ]"#;

        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Renamed);
        assert_eq!(changes[0].entity.name, "new_name");
        assert!(changes[0].previous.is_some());
        assert_eq!(changes[0].previous.as_ref().unwrap().name, "old_name");
    }

    #[test]
    fn test_parse_empty_output() {
        let changes = parse_sem_diff_output("[]").unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_list_semantic_diffs_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = list_semantic_diffs(dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_nonexistent_diff() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = load_semantic_diff(dir.path(), "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_roundtrip_storage() {
        let dir = tempfile::TempDir::new().unwrap();
        let changes = vec![EntityChange {
            kind: ChangeKind::Added,
            entity: EntityInfo {
                name: "test_fn".to_string(),
                entity_type: "function".to_string(),
                file_path: "src/test.rs".to_string(),
                start_line: Some(1),
                end_line: Some(10),
                parent: None,
            },
            previous: None,
        }];

        let report = SemanticDiffReport::new(
            "test-session".to_string(),
            "aaa".to_string(),
            "bbb".to_string(),
            changes,
        );

        // Store
        let diff_dir = dir.path().join("semantic_diffs");
        std::fs::create_dir_all(&diff_dir).unwrap();
        let path = diff_dir.join("test-session.json");
        let json = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(&path, json).unwrap();

        // Load
        let loaded = load_semantic_diff(dir.path(), "test-session")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.session_id, "test-session");
        assert_eq!(loaded.changes.len(), 1);
        assert_eq!(loaded.changes[0].entity.name, "test_fn");
        assert_eq!(loaded.summary.added, 1);

        // List
        let ids = list_semantic_diffs(dir.path()).unwrap();
        assert_eq!(ids, vec!["test-session"]);
    }

    #[test]
    fn test_load_semantic_diff_path_traversal_sanitized() {
        let dir = tempfile::TempDir::new().unwrap();
        // A traversal ID like "../../etc/passwd" should be sanitized to a flat filename
        let result = load_semantic_diff(dir.path(), "../../etc/passwd").unwrap();
        assert!(result.is_none());

        // Verify the sanitized path stays inside semantic_diffs/
        let safe = crate::storage::sanitize_filename("../../etc/passwd");
        assert!(!safe.contains('/'));
        assert!(!safe.contains('\\'));
    }

    /// `sem diff --format json` output as the sem README prints it (one
    /// modified Python function), trimmed to one line of source per side.
    const CURRENT_SEM_OUTPUT: &str = r#"{
        "summary": {"fileCount": 1, "added": 0, "modified": 1, "deleted": 0, "moved": 0,
                    "renamed": 0, "reordered": 0, "binary": 0, "orphan": 0, "total": 1},
        "changes": [{
            "entityId": "auth.py::function::authenticate_user",
            "changeType": "modified",
            "entityType": "function",
            "entityName": "authenticate_user",
            "startLine": 1,
            "endLine": 6,
            "oldStartLine": 1,
            "oldEndLine": 4,
            "oldEntityName": null,
            "filePath": "auth.py",
            "oldFilePath": null,
            "oldParentId": null,
            "beforeContent": "def authenticate_user(username, password):\n    return False",
            "afterContent": "def authenticate_user(username, password):\n    return True",
            "commitSha": null,
            "author": null,
            "structuralChange": true
        }],
        "binaryChanges": []
    }"#;

    /// Review finding: current sem names its fields in camelCase, which the
    /// parser didn't read, so every real change came out as "modified
    /// unknown (unknown) in unknown".
    #[test]
    fn test_parse_sem_diff_reads_current_sem_output() {
        let changes = parse_sem_diff_output(CURRENT_SEM_OUTPUT).unwrap();
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.kind, ChangeKind::Modified);
        assert_eq!(change.entity.name, "authenticate_user");
        assert_eq!(change.entity.entity_type, "function");
        assert_eq!(change.entity.file_path, "auth.py");
        assert_eq!(change.entity.start_line, Some(1));
        assert_eq!(change.entity.end_line, Some(6));
        assert!(
            change.previous.is_none(),
            "an in-place edit has no previous"
        );
    }

    #[test]
    fn test_parse_sem_diff_reads_current_sem_renames_moves_and_reorders() {
        let json = r#"{"changes": [
            {"changeType": "renamed", "entityType": "function", "entityName": "verify",
             "filePath": "src/auth.ts", "oldEntityName": "check", "oldFilePath": null,
             "oldStartLine": 3, "oldEndLine": 9},
            {"changeType": "moved", "entityType": "class", "entityName": "Token",
             "filePath": "src/token.ts", "oldEntityName": null, "oldFilePath": "src/auth.ts"},
            {"changeType": "reordered", "entityType": "function", "entityName": "load",
             "filePath": "src/config.ts", "oldEntityName": null, "oldFilePath": null}
        ]}"#;
        let changes = parse_sem_diff_output(json).unwrap();
        assert_eq!(changes.len(), 3);

        assert_eq!(changes[0].kind, ChangeKind::Renamed);
        let renamed_from = changes[0].previous.as_ref().unwrap();
        assert_eq!(renamed_from.name, "check");
        assert_eq!(renamed_from.file_path, "src/auth.ts");
        assert_eq!(renamed_from.start_line, Some(3));

        assert_eq!(changes[1].kind, ChangeKind::Moved);
        let moved_from = changes[1].previous.as_ref().unwrap();
        assert_eq!(moved_from.name, "Token");
        assert_eq!(moved_from.file_path, "src/auth.ts");

        assert_eq!(changes[2].kind, ChangeKind::Moved);
        assert!(changes[2].previous.is_none());
    }

    /// Review finding: any JSON sem printed that wasn't a recognised list
    /// (null, a number, an error object) became one fabricated change.
    #[test]
    fn test_parse_sem_diff_refuses_output_that_is_not_a_change_list() {
        for input in [
            "null",
            "42",
            r#""fatal: bad revision""#,
            r#"{"summary": {"total": 0}}"#,
            r#"{"changes": null}"#,
            "[0, 0, 0]",
            r#"[{"changeType": "added", "entityName": "f", "entityType": "function",
                 "filePath": "a.rs"}, 7]"#,
        ] {
            let result = parse_sem_diff_output(input);
            assert!(result.is_err(), "{input} parsed as {result:?}");
        }

        let err =
            parse_sem_diff_output(r#"{"error": "fatal: bad revision 'abc..HEAD'"}"#).unwrap_err();
        assert!(
            format!("{err:#}").contains("sem reported an error: fatal: bad revision"),
            "{err:#}"
        );
    }

    #[test]
    fn test_parse_sem_diff_refuses_a_change_missing_its_name() {
        let err = parse_sem_diff_output(
            r#"{"changes": [
                {"changeType": "added", "entityName": "f", "entityType": "function", "filePath": "a.rs"},
                {"changeType": "added", "entityType": "function", "filePath": "a.rs"}
            ]}"#,
        )
        .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("change #2"), "{message}");
        assert!(message.contains("entity name"), "{message}");
    }

    #[test]
    fn test_parse_sem_diff_refuses_an_unknown_change_type() {
        let err = parse_sem_diff_output(
            r#"[{"changeType": "teleported", "entityName": "f", "entityType": "function",
                 "filePath": "a.rs"}]"#,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("teleported"), "{err:#}");

        let missing = parse_sem_diff_output(
            r#"[{"entityName": "f", "entityType": "function", "filePath": "a.rs"}]"#,
        )
        .unwrap_err();
        assert!(
            format!("{missing:#}").contains("no change type"),
            "{missing:#}"
        );
    }

    #[test]
    fn test_diff_range_joins_refs_and_refuses_option_shaped_ones() {
        assert_eq!(diff_range("abc", "HEAD").unwrap(), "abc..HEAD");
        assert_eq!(diff_range("main", "").unwrap(), "main");
        assert!(diff_range("--output=/tmp/x", "HEAD").is_err());
        assert!(diff_range("main", "-p").is_err());
    }

    #[test]
    fn test_run_semantic_diff_passes_one_range_and_reads_current_output() {
        let (_dir, sem, log) = fake_sem(&format!("cat <<'JSON'\n{CURRENT_SEM_OUTPUT}\nJSON"));
        let repo = tempfile::TempDir::new().unwrap();
        let changes = with_test_sem(&sem, Duration::from_secs(10), || {
            run_semantic_diff(repo.path(), "abc", "HEAD")
        })
        .unwrap();
        assert_eq!(
            logged_runs(&log),
            [["diff", "abc..HEAD", "--format", "json"]]
        );
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity.name, "authenticate_user");
    }

    /// Review finding: refs went to sem unguarded, so one starting with `-`
    /// was read as an option.
    #[test]
    fn test_run_semantic_diff_refuses_an_option_shaped_ref_without_running_sem() {
        let (_dir, sem, log) = fake_sem("echo '{\"changes\": []}'");
        let repo = tempfile::TempDir::new().unwrap();
        let err = with_test_sem(&sem, Duration::from_secs(10), || {
            run_semantic_diff(repo.path(), "--output=/tmp/x", "HEAD")
        })
        .unwrap_err();
        assert!(err.to_string().contains("starts with `-`"), "{err}");
        assert!(
            logged_runs(&log).is_empty(),
            "sem ran: {:?}",
            logged_runs(&log)
        );
    }

    /// Review finding: file and entity names went to sem unguarded, so one
    /// starting with `-` was read as an option.
    #[test]
    fn test_run_semantic_blame_and_impact_end_options_before_the_name() {
        let (_dir, sem, log) = fake_sem(&format!(
            "if [ \"$1\" = blame ]; then echo '[]'; else cat <<'JSON'\n{SEM_IMPACT_OUTPUT}\nJSON\nfi"
        ));
        let repo = tempfile::TempDir::new().unwrap();
        with_test_sem(&sem, Duration::from_secs(10), || {
            run_semantic_blame(repo.path(), "-weird.rs").unwrap();
            run_semantic_impact(repo.path(), "--format=text").unwrap();
        });
        assert_eq!(
            logged_runs(&log),
            [
                ["blame", "--format", "json", "--", "-weird.rs"],
                ["impact", "--format", "json", "--", "--format=text"],
            ]
        );
    }

    /// Review finding: an error object from sem was stored as a session's
    /// semantic diff, with one fabricated change.
    #[test]
    fn test_capture_semantic_diff_stores_nothing_when_sem_reports_an_error() {
        let (_dir, sem, _log) = fake_sem("echo '{\"error\": \"fatal: bad revision\"}'");
        let impulse = tempfile::TempDir::new().unwrap();
        let repo = tempfile::TempDir::new().unwrap();
        let result = with_test_sem(&sem, Duration::from_secs(10), || {
            capture_semantic_diff(impulse.path(), repo.path(), "sess-1", "abc", "HEAD")
        });
        let err = result.expect_err("an error object is not a diff");
        assert!(
            format!("{err:#}").contains("fatal: bad revision"),
            "{err:#}"
        );
        assert!(list_semantic_diffs(impulse.path()).unwrap().is_empty());
    }

    /// Review finding: sem leaving a background process that held its
    /// stdout kept `run_semantic_diff` waiting past `SEM_TIMEOUT`.
    #[test]
    fn test_run_semantic_diff_returns_by_the_timeout_when_sem_leaves_output_open() {
        let marker = crate::process_util::test_sleep::unique_duration(5);
        let (_dir, sem, _log) = fake_sem(&format!("sleep {marker} &\necho '{{\"changes\": []}}'"));
        let repo = tempfile::TempDir::new().unwrap();
        let start = std::time::Instant::now();
        let result = with_test_sem(&sem, Duration::from_millis(300), || {
            run_semantic_diff(repo.path(), "abc", "HEAD")
        });
        let elapsed = start.elapsed();
        crate::process_util::test_sleep::stop(&marker);
        assert!(result.is_err(), "{result:?}");
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    }

    #[test]
    fn test_sem_version_reports_the_version_and_times_out_when_it_hangs() {
        let (_dir, sem, _log) = fake_sem("echo 'sem 9.9.9'");
        let version = with_test_sem(&sem, Duration::from_secs(10), sem_version).unwrap();
        assert_eq!(version, "sem 9.9.9");

        let marker = crate::process_util::test_sleep::unique_duration(5);
        let (_dir, hung, _log) = fake_sem(&format!("sleep {marker}"));
        let start = std::time::Instant::now();
        let result = with_test_sem(&hung, Duration::from_millis(300), sem_version);
        let elapsed = start.elapsed();
        crate::process_util::test_sleep::stop(&marker);
        assert!(result.is_err(), "{result:?}");
        assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    }

    /// `sem impact --format json` output in the shape sem's `impact.rs`
    /// prints (full mode, no `--deps`/`--dependents`/`--tests`).
    const SEM_IMPACT_OUTPUT: &str = r#"{
        "entity": {"entityId": "src/config.ts::function::parseConfig", "name": "parseConfig",
                   "type": "function", "file": "src/config.ts", "lines": [3, 9]},
        "dependencies": [],
        "dependents": [
            {"entityId": "src/load.ts::function::load", "name": "load", "type": "function",
             "file": "src/load.ts", "lines": [1, 4]}
        ],
        "impact": {"depth": 2, "total": 3, "entities": []},
        "tests": [],
        "noTestReaches": true
    }"#;

    /// Verification finding: sem names a JSON entity by its key, and an npm
    /// lockfile's root package is keyed "", so adding a lockfile failed the
    /// whole diff.
    #[test]
    fn test_parse_sem_diff_keeps_an_entity_with_an_empty_name() {
        let changes = parse_sem_diff_output(
            r#"{"changes": [{"changeType": "added", "entityName": "", "entityType": "object",
                             "filePath": "package-lock.json"}]}"#,
        )
        .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity.name, "");
        assert_eq!(changes[0].entity.file_path, "package-lock.json");
    }

    /// Verification finding: sem's blame entries are flat (`name`, `type`,
    /// `lines`, a nullable `commit`), so every non-empty blame failed to
    /// parse.
    #[test]
    fn test_parse_sem_blame_output_reads_sem_entries() {
        let entries = parse_sem_blame_output(
            r#"[
                {"name": "parse", "type": "function", "lines": [3, 9], "author": "Ada",
                 "date": "2026-10-01", "commit": "abc1234", "summary": "fix parse"},
                {"name": "draft", "type": "function", "lines": [11, 12], "author": "unknown",
                 "date": "", "commit": null, "summary": ""}
            ]"#,
            "src/config.ts",
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entity.name, "parse");
        assert_eq!(entries[0].entity.entity_type, "function");
        assert_eq!(entries[0].entity.file_path, "src/config.ts");
        assert_eq!(entries[0].entity.start_line, Some(3));
        assert_eq!(entries[0].entity.end_line, Some(9));
        assert_eq!(entries[0].commit.as_deref(), Some("abc1234"));
        assert_eq!(entries[0].message.as_deref(), Some("fix parse"));
        assert_eq!(entries[1].commit, None);

        assert!(parse_sem_blame_output(r#"[{"name": "x"}]"#, "a.rs").is_err());
    }

    /// Verification finding: sem's impact output has an `entity` object and
    /// an `impact.total`, not `target` and `blast_radius`, so every impact
    /// failed to parse.
    #[test]
    fn test_parse_sem_impact_output_reads_sem_output() {
        let result = parse_sem_impact_output(SEM_IMPACT_OUTPUT).unwrap();
        assert_eq!(result.target.name, "parseConfig");
        assert_eq!(result.target.file_path, "src/config.ts");
        assert_eq!(result.target.start_line, Some(3));
        assert_eq!(result.dependents.len(), 1);
        assert_eq!(result.dependents[0].file_path, "src/load.ts");
        assert_eq!(result.blast_radius, 3);

        let without_reach = parse_sem_impact_output(
            r#"{"entity": {"name": "f", "type": "function", "file": "a.rs"},
                "dependents": [{"name": "g", "type": "function", "file": "b.rs"}]}"#,
        )
        .unwrap();
        assert_eq!(without_reach.blast_radius, 1);

        assert!(parse_sem_impact_output(r#"{"target": {}, "blast_radius": 0}"#).is_err());
        assert!(parse_sem_impact_output(
            r#"{"entity": {"name": "f", "type": "function"}, "dependents": []}"#
        )
        .is_err());
    }

    /// Verification finding: with a sem cloud login, `sem diff` runs on for
    /// minutes after printing its answer, past the timeout. Every sem command
    /// now runs with `SEM_LOCAL=1`.
    #[test]
    fn test_sem_commands_run_with_sem_local_set() {
        let (_dir, sem, log) = fake_sem(&format!(
            "case \"$1\" in \
               diff) echo '{{\"changes\": []}}' ;; \
               blame) echo '[]' ;; \
               impact) cat <<'JSON'\n{SEM_IMPACT_OUTPUT}\nJSON\n;; \
               *) echo 'sem 9.9.9' ;; \
             esac"
        ));
        let repo = tempfile::TempDir::new().unwrap();
        with_test_sem(&sem, Duration::from_secs(10), || {
            run_semantic_diff(repo.path(), "abc", "HEAD").unwrap();
            run_semantic_blame(repo.path(), "a.rs").unwrap();
            run_semantic_impact(repo.path(), "f").unwrap();
            sem_version().unwrap();
        });
        assert_eq!(logged_sem_local(&log), ["SEM_LOCAL=1"; 4]);
    }

    /// Verification finding: sem's JSON carries each change's full source,
    /// so a complete answer can pass the 32 MiB default cap; `sem diff`
    /// gets a larger one.
    #[test]
    fn test_run_semantic_diff_reads_an_answer_larger_than_the_default_cap() {
        let source_bytes = crate::process_util::MAX_STDOUT_BYTES + 1024 * 1024;
        let (_dir, sem, _log) = fake_sem(&format!(
            "printf '{{\"changes\": [{{\"changeType\": \"added\", \"entityName\": \"f\", \
             \"entityType\": \"function\", \"filePath\": \"gen.rs\", \"afterContent\": \"'\n\
             head -c {source_bytes} /dev/zero | tr '\\0' a\n\
             printf '\"}}]}}'"
        ));
        let repo = tempfile::TempDir::new().unwrap();
        let changes = with_test_sem(&sem, Duration::from_secs(30), || {
            run_semantic_diff(repo.path(), "abc", "HEAD")
        })
        .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].entity.file_path, "gen.rs");
    }

    /// Verification finding: a nested `with_test_sem` reset the outer
    /// setting to none, so the next call ran the real `sem`.
    #[test]
    fn test_nested_test_sem_restores_the_outer_program() {
        let outer = Path::new("/outer/sem");
        let inner = Path::new("/inner/sem");
        with_test_sem(outer, Duration::from_secs(1), || {
            with_test_sem(inner, Duration::from_secs(2), || {
                assert_eq!(sem_program(), inner.as_os_str());
            });
            assert_eq!(sem_program(), outer.as_os_str());
            assert_eq!(sem_timeout(SEM_TIMEOUT), Duration::from_secs(1));
        });
        assert_eq!(sem_program(), "sem");
    }
}
