//! Off-context blackboard: a durable SQLite key/value store that lets agents
//! park large results outside the model's context window and hand back a
//! small reference instead (ADR-0023).
//!
//! One table, `blackboard`, in `<impulse_dir>/blackboard.db`. Rows are keyed
//! by `task_id`, carry an opaque `payload` BLOB with a `content_type`, a
//! creation time, an optional TTL, and a JSON-object `metadata` column.
//!
//! The store is deliberately small:
//!
//! - **Insert-only for live keys.** [`Blackboard::put`] refuses a key whose
//!   row is still live, so one agent cannot silently overwrite another's
//!   entry. A key whose TTL has elapsed may be reused; the replacement is a
//!   single atomic upsert guarded on expiry.
//! - **Expired means absent.** Reads ignore expired rows even before
//!   [`Blackboard::purge_expired`] deletes them, so purge cadence is a disk
//!   concern, never a correctness one.
//! - **Connection per handle, file shared across processes.** Ion, the
//!   daemon, and any other agent open the same file; WAL plus a busy timeout
//!   lets them coexist. Nothing is cached in memory, so a crashed writer
//!   leaves only what SQLite committed.
//!
//! Paging and projection over a stored payload live in [`projection`].

pub mod projection;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// File name of the blackboard database inside an `.impulse` directory.
pub const BLACKBOARD_DB_FILE: &str = "blackboard.db";
/// Schema version recorded in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 1;
/// Longest accepted `task_id`, in bytes.
pub const MAX_KEY_BYTES: usize = 160;
/// Largest payload one row may hold. Spilled tool output above this is
/// stored truncated, with `truncated: true` in its metadata.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
/// Largest serialized `metadata` object.
pub const MAX_METADATA_BYTES: usize = 16 * 1024;
/// Longest accepted `content_type`.
pub const MAX_CONTENT_TYPE_BYTES: usize = 128;
/// Longest accepted TTL: one year.
pub const MAX_TTL_SECONDS: u64 = 365 * 24 * 60 * 60;
/// Default content type when a writer does not name one.
pub const DEFAULT_CONTENT_TYPE: &str = "text/plain";

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Expiry predicate shared by every query, so the expression index below is
/// usable and reads and purges can never disagree about what has expired.
const EXPIRED_AT: &str = "(ttl_seconds IS NOT NULL AND created_at + ttl_seconds <= ?)";

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS blackboard (
    task_id      TEXT PRIMARY KEY NOT NULL,
    payload      BLOB NOT NULL,
    content_type TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    ttl_seconds  INTEGER NULL,
    metadata     TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS blackboard_expiry
    ON blackboard (created_at + ttl_seconds)
    WHERE ttl_seconds IS NOT NULL;
";

/// Errors from the blackboard store.
#[derive(Debug, thiserror::Error)]
pub enum BlackboardError {
    #[error("invalid blackboard task_id {key:?}: {reason}")]
    InvalidKey { key: String, reason: &'static str },
    #[error("blackboard payload is {bytes} bytes, over the {max}-byte limit")]
    PayloadTooLarge { bytes: usize, max: usize },
    #[error("invalid blackboard metadata: {reason}")]
    InvalidMetadata { reason: String },
    #[error("invalid blackboard content_type {content_type:?}: {reason}")]
    InvalidContentType {
        content_type: String,
        reason: &'static str,
    },
    #[error("invalid blackboard ttl_seconds {ttl}: must be between 1 and {max}")]
    InvalidTtl { ttl: u64, max: u64 },
    #[error("blackboard task_id {key:?} already holds a live entry; choose another key")]
    KeyExists { key: String },
    #[error(
        "blackboard database {path} has schema version {found}, newer than the supported {supported}"
    )]
    NewerSchema {
        path: PathBuf,
        found: i64,
        supported: i64,
    },
    #[error("blackboard I/O failed while {action} ({path}): {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("blackboard SQLite operation failed while {action}: {source}")]
    Sqlite {
        action: &'static str,
        source: rusqlite::Error,
    },
}

fn sqlite(action: &'static str) -> impl FnOnce(rusqlite::Error) -> BlackboardError {
    move |source| BlackboardError::Sqlite { action, source }
}

/// The `blackboard` section of `config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlackboardConfig {
    /// A tool result larger than this many bytes is stored in the blackboard
    /// and replaced in context by a reference with a short preview.
    pub spill_threshold_bytes: usize,
    /// TTL applied to spilled tool results. `None` keeps them until deleted.
    pub spill_ttl_seconds: Option<u64>,
    /// How often the daemon purges expired rows.
    pub purge_interval_secs: u64,
}

/// Default spill threshold: 4 KiB.
pub const DEFAULT_SPILL_THRESHOLD_BYTES: usize = 4 * 1024;
/// Smallest accepted spill threshold. The reference that replaces a spilled
/// result carries about 600 bytes of fixed text plus its envelope, so below
/// this a spill would send the model more than it saved.
pub const MIN_SPILL_THRESHOLD_BYTES: usize = 1024;
/// Largest accepted spill threshold.
pub const MAX_SPILL_THRESHOLD_BYTES: usize = 1024 * 1024;
/// Default TTL for spilled tool results: seven days.
pub const DEFAULT_SPILL_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Default daemon purge cadence.
pub const DEFAULT_PURGE_INTERVAL_SECS: u64 = 300;
/// Shortest accepted purge cadence.
pub const MIN_PURGE_INTERVAL_SECS: u64 = 10;

impl Default for BlackboardConfig {
    fn default() -> Self {
        Self {
            spill_threshold_bytes: DEFAULT_SPILL_THRESHOLD_BYTES,
            spill_ttl_seconds: Some(DEFAULT_SPILL_TTL_SECONDS),
            purge_interval_secs: DEFAULT_PURGE_INTERVAL_SECS,
        }
    }
}

impl BlackboardConfig {
    /// Checks every field against its documented bounds.
    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_SPILL_THRESHOLD_BYTES..=MAX_SPILL_THRESHOLD_BYTES)
            .contains(&self.spill_threshold_bytes)
        {
            return Err(format!(
                "blackboard.spill_threshold_bytes must be between {MIN_SPILL_THRESHOLD_BYTES} \
                 and {MAX_SPILL_THRESHOLD_BYTES}, got {}",
                self.spill_threshold_bytes
            ));
        }
        if let Some(ttl) = self.spill_ttl_seconds {
            validate_ttl(ttl).map_err(|err| format!("blackboard.spill_ttl_seconds: {err}"))?;
        }
        if self.purge_interval_secs < MIN_PURGE_INTERVAL_SECS {
            return Err(format!(
                "blackboard.purge_interval_secs must be at least {MIN_PURGE_INTERVAL_SECS}, got {}",
                self.purge_interval_secs
            ));
        }
        Ok(())
    }
}

impl BlackboardConfig {
    /// Parses and validates a raw `blackboard` section. `Null` (section
    /// absent) is the default.
    pub fn from_section(section: &serde_json::Value) -> Result<Self, String> {
        if section.is_null() {
            return Ok(Self::default());
        }
        let config: Self = serde_json::from_value(section.clone())
            .map_err(|err| format!("invalid blackboard section: {err}"))?;
        config.validate()?;
        Ok(config)
    }
}

/// Reads and validates the `blackboard` section of `<impulse_dir>/config.json`.
/// A missing file or section is the default; a malformed or out-of-range one
/// is an error naming the problem.
pub fn load_config(impulse_dir: &Path) -> anyhow::Result<BlackboardConfig> {
    use anyhow::Context as _;
    #[derive(Deserialize, Default)]
    struct Section {
        #[serde(default)]
        blackboard: serde_json::Value,
    }
    let path = impulse_dir.join("config.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BlackboardConfig::default())
        }
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", path.display()));
        }
    };
    let section: Section =
        serde_json::from_str(&raw).with_context(|| format!("cannot parse {}", path.display()))?;
    BlackboardConfig::from_section(&section.blackboard)
        .map_err(|reason| anyhow::anyhow!("{reason} (in {})", path.display()))
}

/// A row to write.
#[derive(Debug, Clone)]
pub struct NewEntry<'a> {
    pub task_id: &'a str,
    pub payload: &'a [u8],
    pub content_type: &'a str,
    pub ttl_seconds: Option<u64>,
    /// Must be a JSON object; `Value::Null` is stored as `{}`.
    pub metadata: serde_json::Value,
}

/// A stored row, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub task_id: String,
    pub payload: Vec<u8>,
    pub content_type: String,
    pub created_at: i64,
    pub ttl_seconds: Option<u64>,
    pub metadata: serde_json::Value,
}

impl Entry {
    /// Unix time after which the row reads as absent, if it expires.
    pub fn expires_at(&self) -> Option<i64> {
        self.ttl_seconds
            .map(|ttl| self.created_at.saturating_add(ttl as i64))
    }
}

/// What a writer gets back, and what goes into context in place of a payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryRef {
    pub task_id: String,
    pub content_type: String,
    pub bytes: usize,
    /// Lowercase hex SHA-256 of the payload, so a reader can tell whether
    /// what it fetched is what was referenced.
    pub sha256: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
}

impl EntryRef {
    /// The artifact-id form used in governed records: `blackboard:<task_id>`.
    pub fn artifact_id(&self) -> String {
        format!("{ARTIFACT_ID_PREFIX}{}", self.task_id)
    }
}

/// Prefix for blackboard references carried in governed `artifact_ids`.
pub const ARTIFACT_ID_PREFIX: &str = "blackboard:";

/// Handle on one blackboard database file.
pub struct Blackboard {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Blackboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blackboard")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Path of the blackboard database inside `impulse_dir`.
pub fn db_path(impulse_dir: &Path) -> PathBuf {
    impulse_dir.join(BLACKBOARD_DB_FILE)
}

impl Blackboard {
    /// Opens (creating if needed) `<impulse_dir>/blackboard.db`, ensures the
    /// schema, and purges expired rows.
    pub fn open(impulse_dir: &Path) -> Result<Self, BlackboardError> {
        std::fs::create_dir_all(impulse_dir).map_err(|source| BlackboardError::Io {
            action: "creating the .impulse directory",
            path: impulse_dir.to_path_buf(),
            source,
        })?;
        Self::open_at(&db_path(impulse_dir))
    }

    /// Opens the blackboard in `impulse_dir` only if its file already exists.
    /// Used by the daemon so that starting it never creates a database in a
    /// project that has never written to one.
    pub fn open_existing(impulse_dir: &Path) -> Result<Option<Self>, BlackboardError> {
        let path = db_path(impulse_dir);
        if !path.exists() {
            return Ok(None);
        }
        Self::open_at(&path).map(Some)
    }

    /// Opens a blackboard at an explicit file path.
    pub fn open_at(path: &Path) -> Result<Self, BlackboardError> {
        let existed = path.exists();
        let conn = Connection::open(path).map_err(sqlite("opening the database"))?;
        if !existed {
            restrict_permissions(path)?;
        }
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(sqlite("setting the busy timeout"))?;
        // WAL lets readers in other processes proceed while one writes. A
        // filesystem that cannot do WAL still works in rollback mode.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(sqlite("reading the schema version"))?;
        if version > SCHEMA_VERSION {
            return Err(BlackboardError::NewerSchema {
                path: path.to_path_buf(),
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        conn.execute_batch(SCHEMA_SQL)
            .map_err(sqlite("creating the schema"))?;
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(sqlite("recording the schema version"))?;
        }

        let board = Self {
            conn,
            path: path.to_path_buf(),
        };
        board.purge_expired()?;
        Ok(board)
    }

    /// Path of the underlying database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes a new row. Fails with [`BlackboardError::KeyExists`] when the
    /// key holds a live row.
    pub fn put(&self, entry: NewEntry<'_>) -> Result<EntryRef, BlackboardError> {
        self.put_at(entry, unix_now())
    }

    /// [`Blackboard::put`] at an explicit clock, for tests.
    pub fn put_at(&self, entry: NewEntry<'_>, now: i64) -> Result<EntryRef, BlackboardError> {
        validate_key(entry.task_id)?;
        validate_content_type(entry.content_type)?;
        if entry.payload.len() > MAX_PAYLOAD_BYTES {
            return Err(BlackboardError::PayloadTooLarge {
                bytes: entry.payload.len(),
                max: MAX_PAYLOAD_BYTES,
            });
        }
        if let Some(ttl) = entry.ttl_seconds {
            validate_ttl(ttl)?;
        }
        let metadata = metadata_text(entry.metadata)?;

        // Insert, or replace only a row that has already expired. A live row
        // makes the upsert's WHERE false, so nothing changes and the count
        // below reports the conflict -- atomically, with no read-then-write
        // window for another process to slip through.
        let sql = format!(
            "INSERT INTO blackboard (task_id, payload, content_type, created_at, ttl_seconds, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(task_id) DO UPDATE SET
                payload = excluded.payload,
                content_type = excluded.content_type,
                created_at = excluded.created_at,
                ttl_seconds = excluded.ttl_seconds,
                metadata = excluded.metadata
             WHERE {}",
            EXPIRED_AT.replace('?', "?7")
        );
        let changed = self
            .conn
            .execute(
                &sql,
                params![
                    entry.task_id,
                    entry.payload,
                    entry.content_type,
                    now,
                    entry.ttl_seconds.map(|ttl| ttl as i64),
                    metadata,
                    now,
                ],
            )
            .map_err(sqlite("writing an entry"))?;
        if changed == 0 {
            return Err(BlackboardError::KeyExists {
                key: entry.task_id.to_string(),
            });
        }
        Ok(EntryRef {
            task_id: entry.task_id.to_string(),
            content_type: entry.content_type.to_string(),
            bytes: entry.payload.len(),
            sha256: sha256_hex(entry.payload),
            created_at: now,
            expires_at: entry.ttl_seconds.map(|ttl| now.saturating_add(ttl as i64)),
        })
    }

    /// Reads a live row. An expired or missing key is `None`.
    pub fn get(&self, task_id: &str) -> Result<Option<Entry>, BlackboardError> {
        self.get_at(task_id, unix_now())
    }

    /// [`Blackboard::get`] at an explicit clock, for tests.
    pub fn get_at(&self, task_id: &str, now: i64) -> Result<Option<Entry>, BlackboardError> {
        validate_key(task_id)?;
        let sql = format!(
            "SELECT task_id, payload, content_type, created_at, ttl_seconds, metadata
             FROM blackboard WHERE task_id = ?1 AND NOT {}",
            EXPIRED_AT.replace('?', "?2")
        );
        let row = self
            .conn
            .query_row(&sql, params![task_id, now], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .optional()
            .map_err(sqlite("reading an entry"))?;
        let Some((task_id, payload, content_type, created_at, ttl, metadata)) = row else {
            return Ok(None);
        };
        // A row written by another tool with unparsable metadata still reads;
        // its metadata is reported as the raw string rather than dropped.
        let metadata = match serde_json::from_str(&metadata) {
            Ok(parsed) => parsed,
            Err(_) => serde_json::Value::String(metadata),
        };
        Ok(Some(Entry {
            task_id,
            payload,
            content_type,
            created_at,
            ttl_seconds: ttl.and_then(|ttl| u64::try_from(ttl).ok()),
            metadata,
        }))
    }

    /// Deletes one row, live or expired. Returns whether a row was removed.
    /// Used to undo a write whose purpose failed (a claim the daemon did not
    /// record), never by a model-facing tool.
    pub fn delete(&self, task_id: &str) -> Result<bool, BlackboardError> {
        validate_key(task_id)?;
        let removed = self
            .conn
            .execute(
                "DELETE FROM blackboard WHERE task_id = ?1",
                params![task_id],
            )
            .map_err(sqlite("deleting an entry"))?;
        Ok(removed > 0)
    }

    /// Deletes every expired row and returns how many went.
    pub fn purge_expired(&self) -> Result<usize, BlackboardError> {
        self.purge_expired_at(unix_now())
    }

    /// [`Blackboard::purge_expired`] at an explicit clock, for tests.
    pub fn purge_expired_at(&self, now: i64) -> Result<usize, BlackboardError> {
        let sql = format!("DELETE FROM blackboard WHERE {EXPIRED_AT}");
        self.conn
            .execute(&sql, params![now])
            .map_err(sqlite("purging expired entries"))
    }

    /// Number of live rows.
    pub fn live_count(&self) -> Result<usize, BlackboardError> {
        self.live_count_at(unix_now())
    }

    /// [`Blackboard::live_count`] at an explicit clock, for tests.
    pub fn live_count_at(&self, now: i64) -> Result<usize, BlackboardError> {
        let sql = format!("SELECT COUNT(*) FROM blackboard WHERE NOT {EXPIRED_AT}");
        let count: i64 = self
            .conn
            .query_row(&sql, params![now], |row| row.get(0))
            .map_err(sqlite("counting live entries"))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }
}

/// What one maintenance pass found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeReport {
    /// Expired rows deleted by this pass.
    pub purged: usize,
    /// Live rows left after it.
    pub live: usize,
}

/// One maintenance pass over the blackboard in `impulse_dir`: purge expired
/// rows and count what is left. `None` when the project has no blackboard;
/// this never creates one.
pub fn maintain(impulse_dir: &Path) -> Result<Option<PurgeReport>, BlackboardError> {
    let Some(board) = Blackboard::open_existing(impulse_dir)? else {
        return Ok(None);
    };
    // `open_at` already purged once; this second call reports the count for
    // a database that stayed open in another process while rows expired.
    let purged = board.purge_expired()?;
    Ok(Some(PurgeReport {
        purged,
        live: board.live_count()?,
    }))
}

/// Current Unix time in whole seconds.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Keys are 1..=160 bytes of `[A-Za-z0-9._:-]`, starting with an
/// alphanumeric. They are opaque to SQLite (always bound as parameters); the
/// charset keeps them safe to echo into prompts, logs, and artifact ids.
pub fn validate_key(key: &str) -> Result<(), BlackboardError> {
    let invalid = |reason| BlackboardError::InvalidKey {
        key: key.chars().take(MAX_KEY_BYTES).collect(),
        reason,
    };
    if key.is_empty() {
        return Err(invalid("must not be empty"));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(invalid("longer than 160 bytes"));
    }
    if !key
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(invalid("must start with a letter or digit"));
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(invalid(
            "may contain only letters, digits, '.', '_', ':' and '-'",
        ));
    }
    Ok(())
}

fn validate_content_type(content_type: &str) -> Result<(), BlackboardError> {
    let invalid = |reason| BlackboardError::InvalidContentType {
        content_type: content_type.chars().take(MAX_CONTENT_TYPE_BYTES).collect(),
        reason,
    };
    if content_type.trim().is_empty() {
        return Err(invalid("must not be blank"));
    }
    if content_type.len() > MAX_CONTENT_TYPE_BYTES {
        return Err(invalid("longer than 128 bytes"));
    }
    if content_type.chars().any(char::is_control) {
        return Err(invalid("must not contain control characters"));
    }
    Ok(())
}

fn validate_ttl(ttl: u64) -> Result<(), BlackboardError> {
    if ttl == 0 || ttl > MAX_TTL_SECONDS {
        return Err(BlackboardError::InvalidTtl {
            ttl,
            max: MAX_TTL_SECONDS,
        });
    }
    Ok(())
}

fn metadata_text(metadata: serde_json::Value) -> Result<String, BlackboardError> {
    let metadata = match metadata {
        serde_json::Value::Null => serde_json::Value::Object(serde_json::Map::new()),
        object @ serde_json::Value::Object(_) => object,
        _ => {
            return Err(BlackboardError::InvalidMetadata {
                reason: "must be a JSON object".to_string(),
            })
        }
    };
    let text = metadata.to_string();
    if text.len() > MAX_METADATA_BYTES {
        return Err(BlackboardError::InvalidMetadata {
            reason: format!(
                "serialized size {} exceeds {MAX_METADATA_BYTES} bytes",
                text.len()
            ),
        });
    }
    Ok(text)
}

/// Tool output can carry anything a command printed, so a new database file
/// is readable by its owner only. SQLite gives the WAL and shared-memory
/// sidecars the same mode as the main file.
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), BlackboardError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|source| {
        BlackboardError::Io {
            action: "restricting database permissions",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), BlackboardError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn board() -> (tempfile::TempDir, Blackboard) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let board = Blackboard::open(dir.path()).expect("open blackboard");
        (dir, board)
    }

    fn entry<'a>(key: &'a str, payload: &'a [u8], ttl: Option<u64>) -> NewEntry<'a> {
        NewEntry {
            task_id: key,
            payload,
            content_type: DEFAULT_CONTENT_TYPE,
            ttl_seconds: ttl,
            metadata: json!({"tool": "test"}),
        }
    }

    #[test]
    fn test_put_then_get_round_trips_every_column() {
        let (_dir, board) = board();
        let stored = board
            .put_at(entry("task-1", b"hello", Some(60)), 1_000)
            .expect("put");
        assert_eq!(stored.bytes, 5);
        assert_eq!(stored.expires_at, Some(1_060));
        assert_eq!(stored.sha256, sha256_hex(b"hello"));
        let read = board.get_at("task-1", 1_010).expect("get").expect("live");
        assert_eq!(read.payload, b"hello");
        assert_eq!(read.content_type, DEFAULT_CONTENT_TYPE);
        assert_eq!(read.created_at, 1_000);
        assert_eq!(read.ttl_seconds, Some(60));
        assert_eq!(read.metadata, json!({"tool": "test"}));
        assert_eq!(read.expires_at(), Some(1_060));
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let (_dir, board) = board();
        assert!(board.get("absent").expect("get").is_none());
    }

    #[test]
    fn test_get_expired_entry_returns_none_before_purge() {
        let (_dir, board) = board();
        board
            .put_at(entry("short", b"x", Some(10)), 1_000)
            .expect("put");
        assert!(board.get_at("short", 1_009).expect("get").is_some());
        assert!(board.get_at("short", 1_010).expect("get").is_none());
    }

    #[test]
    fn test_put_live_key_returns_key_exists() {
        let (_dir, board) = board();
        board
            .put_at(entry("k", b"first", None), 1_000)
            .expect("put");
        let err = board
            .put_at(entry("k", b"second", None), 2_000)
            .expect_err("live key must not be overwritten");
        assert!(matches!(err, BlackboardError::KeyExists { .. }));
        let kept = board.get_at("k", 2_000).expect("get").expect("live");
        assert_eq!(kept.payload, b"first");
    }

    #[test]
    fn test_put_expired_key_replaces_it() {
        let (_dir, board) = board();
        board
            .put_at(entry("k", b"old", Some(5)), 1_000)
            .expect("put");
        board
            .put_at(entry("k", b"new", None), 1_005)
            .expect("expired key is reusable");
        let read = board.get_at("k", 5_000).expect("get").expect("live");
        assert_eq!(read.payload, b"new");
        assert_eq!(read.ttl_seconds, None);
    }

    #[test]
    fn test_purge_expired_deletes_only_expired_rows() {
        let (_dir, board) = board();
        board.put_at(entry("a", b"1", Some(10)), 1_000).expect("a");
        board.put_at(entry("b", b"2", Some(100)), 1_000).expect("b");
        board.put_at(entry("c", b"3", None), 1_000).expect("c");
        assert_eq!(board.purge_expired_at(1_050).expect("purge"), 1);
        assert_eq!(board.live_count_at(1_050).expect("count"), 2);
        assert_eq!(board.purge_expired_at(1_050).expect("purge again"), 0);
    }

    #[test]
    fn test_open_purges_expired_rows_on_startup() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        {
            let board = Blackboard::open(dir.path()).expect("open");
            board
                .put_at(entry("stale", b"x", Some(1)), 1_000)
                .expect("put");
            board.put_at(entry("kept", b"y", None), 1_000).expect("put");
        }
        let reopened = Blackboard::open(dir.path()).expect("reopen");
        let total: i64 = reopened
            .conn
            .query_row("SELECT COUNT(*) FROM blackboard", [], |row| row.get(0))
            .expect("count");
        assert_eq!(total, 1, "the expired row is gone after reopen");
        assert!(reopened.get("kept").expect("get").is_some());
    }

    #[test]
    fn test_entries_survive_reopen_like_a_daemon_restart() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let stored = {
            let board = Blackboard::open(dir.path()).expect("open");
            board.put(entry("durable", b"survives", None)).expect("put")
        };
        let reopened = Blackboard::open(dir.path()).expect("reopen");
        let read = reopened.get("durable").expect("get").expect("live");
        assert_eq!(sha256_hex(&read.payload), stored.sha256);
    }

    #[test]
    fn test_two_handles_on_one_file_see_each_others_writes() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let writer = Blackboard::open(dir.path()).expect("writer");
        let reader = Blackboard::open(dir.path()).expect("reader");
        writer.put(entry("shared", b"cross", None)).expect("put");
        assert_eq!(
            reader.get("shared").expect("get").expect("live").payload,
            b"cross"
        );
        let err = reader
            .put(entry("shared", b"steal", None))
            .expect_err("second agent cannot overwrite");
        assert!(matches!(err, BlackboardError::KeyExists { .. }));
    }

    #[test]
    fn test_open_existing_does_not_create_a_database() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert!(Blackboard::open_existing(dir.path())
            .expect("open_existing")
            .is_none());
        assert!(!db_path(dir.path()).exists());
        Blackboard::open(dir.path()).expect("create");
        assert!(Blackboard::open_existing(dir.path())
            .expect("open_existing")
            .is_some());
    }

    #[test]
    fn test_maintain_without_a_database_creates_nothing() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert_eq!(maintain(dir.path()).expect("maintain"), None);
        assert!(!db_path(dir.path()).exists());
    }

    #[test]
    fn test_maintain_reports_live_rows_after_a_restart() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        {
            let board = Blackboard::open(dir.path()).expect("open");
            board.put(entry("a", b"1", None)).expect("a");
            board.put(entry("b", b"2", Some(3_600))).expect("b");
        }
        let report = maintain(dir.path()).expect("maintain").expect("present");
        assert_eq!(report.live, 2);
        assert_eq!(report.purged, 0);
    }

    #[test]
    fn test_open_rejects_newer_schema_version() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = db_path(dir.path());
        let conn = Connection::open(&path).expect("raw open");
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("bump version");
        drop(conn);
        let err = Blackboard::open(dir.path()).expect_err("newer schema");
        assert!(matches!(err, BlackboardError::NewerSchema { .. }));
        assert!(err.to_string().contains("newer than the supported"));
    }

    #[cfg(unix)]
    #[test]
    fn test_new_database_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let (dir, _board) = board();
        let mode = std::fs::metadata(db_path(dir.path()))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_validate_key_accepts_documented_charset() {
        validate_key("spill:bash_exec:0a1b2c3d").expect("spill key");
        validate_key("claim.task-7").expect("dotted key");
        validate_key(&"a".repeat(MAX_KEY_BYTES)).expect("max length");
    }

    #[test]
    fn test_validate_key_rejects_bad_keys() {
        for key in [
            "",
            ":lead",
            "has space",
            "slash/key",
            "quote'key",
            "ünïcode",
        ] {
            assert!(validate_key(key).is_err(), "{key:?} should be rejected");
        }
        assert!(validate_key(&"a".repeat(MAX_KEY_BYTES + 1)).is_err());
    }

    #[test]
    fn test_put_rejects_oversized_payload() {
        let (_dir, board) = board();
        let big = vec![b'x'; MAX_PAYLOAD_BYTES + 1];
        let err = board.put(entry("big", &big, None)).expect_err("too large");
        assert!(matches!(err, BlackboardError::PayloadTooLarge { .. }));
    }

    #[test]
    fn test_put_rejects_zero_and_excessive_ttl() {
        let (_dir, board) = board();
        assert!(matches!(
            board.put(entry("t0", b"x", Some(0))),
            Err(BlackboardError::InvalidTtl { .. })
        ));
        assert!(matches!(
            board.put(entry("t1", b"x", Some(MAX_TTL_SECONDS + 1))),
            Err(BlackboardError::InvalidTtl { .. })
        ));
    }

    #[test]
    fn test_put_rejects_non_object_metadata_and_accepts_null() {
        let (_dir, board) = board();
        let mut bad = entry("m1", b"x", None);
        bad.metadata = json!(["not", "an", "object"]);
        assert!(matches!(
            board.put(bad),
            Err(BlackboardError::InvalidMetadata { .. })
        ));
        let mut null = entry("m2", b"x", None);
        null.metadata = serde_json::Value::Null;
        board.put(null).expect("null metadata stored as {}");
        assert_eq!(
            board.get("m2").expect("get").expect("live").metadata,
            json!({})
        );
    }

    #[test]
    fn test_put_rejects_blank_or_control_content_type() {
        let (_dir, board) = board();
        let mut blank = entry("c1", b"x", None);
        blank.content_type = "  ";
        assert!(matches!(
            board.put(blank),
            Err(BlackboardError::InvalidContentType { .. })
        ));
        let mut control = entry("c2", b"x", None);
        control.content_type = "text/plain\n";
        assert!(matches!(
            board.put(control),
            Err(BlackboardError::InvalidContentType { .. })
        ));
    }

    #[test]
    fn test_entry_ref_artifact_id_uses_prefix() {
        let reference = EntryRef {
            task_id: "claim:7".to_string(),
            content_type: DEFAULT_CONTENT_TYPE.to_string(),
            bytes: 1,
            sha256: sha256_hex(b"x"),
            created_at: 0,
            expires_at: None,
        };
        assert_eq!(reference.artifact_id(), "blackboard:claim:7");
    }

    #[test]
    fn test_entry_ref_round_trips_through_json() {
        let original = EntryRef {
            task_id: "k".to_string(),
            content_type: "application/json".to_string(),
            bytes: 42,
            sha256: sha256_hex(b"payload"),
            created_at: 1_000,
            expires_at: Some(2_000),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EntryRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_config_round_trips_and_defaults() {
        let original = BlackboardConfig::default();
        assert_eq!(original.spill_threshold_bytes, 4096);
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: BlackboardConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
        original.validate().expect("defaults are valid");
    }

    #[test]
    fn test_config_validate_rejects_out_of_range_values() {
        let low = BlackboardConfig {
            spill_threshold_bytes: MIN_SPILL_THRESHOLD_BYTES - 1,
            ..BlackboardConfig::default()
        };
        assert!(low
            .validate()
            .unwrap_err()
            .contains("spill_threshold_bytes"));
        let fast = BlackboardConfig {
            purge_interval_secs: 1,
            ..BlackboardConfig::default()
        };
        assert!(fast.validate().unwrap_err().contains("purge_interval_secs"));
        let ttl = BlackboardConfig {
            spill_ttl_seconds: Some(0),
            ..BlackboardConfig::default()
        };
        assert!(ttl.validate().unwrap_err().contains("spill_ttl_seconds"));
    }

    #[test]
    fn test_delete_removes_a_row_and_reports_absence() {
        let (_dir, board) = board();
        board.put(entry("gone", b"x", None)).expect("put");
        assert!(board.delete("gone").expect("delete"));
        assert!(board.get("gone").expect("get").is_none());
        assert!(!board.delete("gone").expect("delete again"));
    }

    #[test]
    fn test_from_section_null_is_default_and_typo_is_error() {
        assert_eq!(
            BlackboardConfig::from_section(&serde_json::Value::Null).expect("null"),
            BlackboardConfig::default()
        );
        let err =
            BlackboardConfig::from_section(&json!({"spill_threshold": 2048})).expect_err("typo");
        assert!(err.contains("spill_threshold"));
        let err = BlackboardConfig::from_section(&json!({"spill_threshold_bytes": 0}))
            .expect_err("out of range");
        assert!(err.contains("spill_threshold_bytes"));
    }

    /// Review P1: a bad `blackboard` section must not stop the rest of
    /// Impulse's configuration from loading. `Config` keeps the section raw
    /// and round-trips it untouched; only blackboard consumers parse it.
    #[test]
    fn test_config_with_a_bad_blackboard_section_still_loads_and_round_trips() {
        let raw = r#"{"log_level":"info","blackboard":{"spill_threshold":2048,"purge_interval_secs":-1}}"#;
        let config: crate::state::Config = serde_json::from_str(raw).expect("config loads");
        assert_eq!(config.blackboard["spill_threshold"], 2048);
        let saved = serde_json::to_value(&config).expect("serialize");
        assert_eq!(
            saved["blackboard"],
            json!({"spill_threshold": 2048, "purge_interval_secs": -1})
        );
        assert!(BlackboardConfig::from_section(&config.blackboard).is_err());
    }

    #[test]
    fn test_load_config_missing_file_is_default() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        assert_eq!(
            load_config(dir.path()).expect("load"),
            BlackboardConfig::default()
        );
    }

    #[test]
    fn test_load_config_reads_section_and_rejects_unknown_fields() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"log_level":"info","blackboard":{"spill_threshold_bytes":8192}}"#,
        )
        .expect("write config");
        let loaded = load_config(dir.path()).expect("load");
        assert_eq!(loaded.spill_threshold_bytes, 8192);
        assert_eq!(loaded.purge_interval_secs, DEFAULT_PURGE_INTERVAL_SECS);

        std::fs::write(
            dir.path().join("config.json"),
            r#"{"blackboard":{"spill_treshold_bytes":8192}}"#,
        )
        .expect("write typo config");
        assert!(load_config(dir.path()).is_err(), "a typo is an error");
    }

    #[test]
    fn test_load_config_rejects_out_of_range_threshold() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"blackboard":{"spill_threshold_bytes":1}}"#,
        )
        .expect("write config");
        let err = load_config(dir.path()).expect_err("out of range");
        assert!(format!("{err:#}").contains("spill_threshold_bytes"));
    }

    #[test]
    fn test_error_display_names_the_problem() {
        let err = BlackboardError::KeyExists {
            key: "k".to_string(),
        };
        assert!(err.to_string().contains("already holds a live entry"));
        let err = BlackboardError::PayloadTooLarge { bytes: 9, max: 8 };
        assert!(err.to_string().contains("9 bytes"));
        let err = BlackboardError::InvalidTtl { ttl: 0, max: 1 };
        assert!(err.to_string().contains("ttl_seconds 0"));
        let err = BlackboardError::InvalidKey {
            key: "x y".to_string(),
            reason: "bad",
        };
        assert!(err.to_string().contains("x y"));
        let err = BlackboardError::InvalidMetadata {
            reason: "nope".to_string(),
        };
        assert!(err.to_string().contains("nope"));
        let err = BlackboardError::InvalidContentType {
            content_type: "t".to_string(),
            reason: "blank",
        };
        assert!(err.to_string().contains("content_type"));
        let err = BlackboardError::Io {
            action: "testing",
            path: PathBuf::from("/x"),
            source: std::io::Error::other("boom"),
        };
        assert!(err.to_string().contains("testing"));
        let err = BlackboardError::Sqlite {
            action: "testing",
            source: rusqlite::Error::InvalidQuery,
        };
        assert!(err.to_string().contains("SQLite"));
    }

    proptest::proptest! {
        #[test]
        fn test_validate_key_accepted_keys_stay_in_charset(key in "[A-Za-z0-9][A-Za-z0-9._:-]{0,40}") {
            proptest::prop_assert!(validate_key(&key).is_ok());
        }

        #[test]
        fn test_validate_key_never_accepts_slash_or_space(prefix in "[a-z]{1,8}", bad in "[ /\\\\'\"]") {
            let key = format!("{prefix}{bad}");
            proptest::prop_assert!(validate_key(&key).is_err());
        }
    }
}
