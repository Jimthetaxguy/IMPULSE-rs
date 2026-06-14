use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::retrieval::types::SearchResult;

pub struct RetrievalStore {
    conn: Connection,
    db_path: PathBuf,
}

pub struct HistoryUpsert<'a> {
    pub session_id: &'a str,
    pub session_name: &'a str,
    pub platform: Option<&'a str>,
    pub started_at: &'a str,
    pub ended_at: &'a str,
    pub summary: &'a str,
    pub files_touched_json: &'a str,
    pub tools_used_json: &'a str,
    pub search_text: &'a str,
    pub content_hash: &'a str,
}

pub struct GenomeUpsert<'a> {
    pub decision_id: &'a str,
    pub date: &'a str,
    pub description: &'a str,
    pub rationale: Option<&'a str>,
    pub tags_json: &'a str,
    pub search_text: &'a str,
    pub content_hash: &'a str,
}

impl RetrievalStore {
    pub fn open(base_path: &Path) -> Result<Self> {
        std::fs::create_dir_all(base_path).context("Failed to create retrieval store directory")?;
        let db_path = base_path.join("retrieval.db");
        let conn = Connection::open(&db_path).context("failed to open retrieval.db")?;

        conn.pragma_update(None, "foreign_keys", "ON")
            .context("Failed to enable foreign_keys pragma")?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        conn.busy_timeout(Duration::from_secs(5))
            .context("Failed to set busy_timeout on retrieval.db")?;

        Ok(Self { conn, db_path })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Execute a closure within a transaction, automatically committing on success.
    /// Returns the result of the closure.
    ///
    /// This is useful for batch operations that need to be atomic but don't
    /// warrant a dedicated method on RetrievalStore.
    ///
    /// # Example
    /// ```ignore
    /// store.with_transaction(|tx| {
    ///     tx.execute("DELETE FROM history_vec WHERE session_id = ?1", params![id])?;
    ///     tx.execute("INSERT INTO history_vec0(...)", ...)?;
    ///     Ok(())
    /// })?;
    /// ```
    ///
    /// # Errors
    /// Returns an error if the transaction fails to begin, if the closure returns
    /// an error, or if the commit fails.
    pub fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<T>,
    {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("failed to begin transaction")?;
        let result = f(&tx).context("transaction closure failed")?;
        tx.commit().context("failed to commit transaction")?;
        Ok(result)
    }

    pub fn table_exists(&self, table_name: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1 LIMIT 1",
            )
            .context("Failed to prepare table_exists query on sqlite_master")?;
        Ok(stmt
            .query_row(params![table_name], |row| row.get::<_, i64>(0))
            .optional()
            .context("Failed to query sqlite_master for table existence")?
            .is_some())
    }

    fn ensure_column(&self, table: &str, column: &str, ddl: &str) -> Result<()> {
        // Allowlist check: only known tables may be passed to ensure_column.
        // This prevents SQL injection via interpolated table names in PRAGMA.
        const ALLOWED_TABLES: &[&str] = &["history_entries", "genome_decisions"];
        if !ALLOWED_TABLES.contains(&table) {
            bail!(
                "ensure_column called with unexpected table '{}'; allowed: {:?}",
                table,
                ALLOWED_TABLES
            );
        }

        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .with_context(|| format!("failed to inspect schema for {}", table))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .with_context(|| format!("Failed to query table_info for {}", table))?;
        let mut found = false;
        for row in rows {
            if row
                .with_context(|| format!("Failed to read column name from {}", table))?
                .as_str()
                == column
            {
                found = true;
                break;
            }
        }
        if !found {
            self.conn
                .execute(ddl, [])
                .with_context(|| format!("Failed to add column {} to {}", column, table))?;
        }
        Ok(())
    }

    pub fn init_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
CREATE TABLE IF NOT EXISTS history_entries (
  session_id TEXT PRIMARY KEY,
  session_name TEXT NOT NULL,
  platform TEXT,
  started_at TEXT NOT NULL,
  ended_at TEXT NOT NULL,
  summary TEXT NOT NULL,
  files_touched_json TEXT NOT NULL,
  tools_used_json TEXT NOT NULL,
  search_text TEXT NOT NULL,
  content_hash TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS genome_decisions (
  decision_id TEXT PRIMARY KEY,
  date TEXT NOT NULL,
  description TEXT NOT NULL,
  rationale TEXT,
  tags_json TEXT NOT NULL,
  search_text TEXT NOT NULL,
  content_hash TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS retrieval_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS history_fts USING fts5(
  session_id,
  search_text
);

CREATE VIRTUAL TABLE IF NOT EXISTS genome_fts USING fts5(
  decision_id,
  search_text
);

CREATE TABLE IF NOT EXISTS history_vec (
  session_id TEXT PRIMARY KEY,
  vector_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS genome_vec (
  decision_id TEXT PRIMARY KEY,
  vector_json TEXT NOT NULL
);
"#,
            )
            .context("Failed to initialize retrieval schema")?;

        self.ensure_column(
            "history_entries",
            "content_hash",
            "ALTER TABLE history_entries ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
        )
        .context("Failed to ensure content_hash column on history_entries")?;
        self.ensure_column(
            "genome_decisions",
            "content_hash",
            "ALTER TABLE genome_decisions ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
        )
        .context("Failed to ensure content_hash column on genome_decisions")?;

        self.set_meta("schema_version", "2")
            .context("Failed to set schema_version in retrieval_meta")?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                r#"INSERT INTO retrieval_meta(key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value=excluded.value"#,
                params![key, value],
            )
            .with_context(|| format!("Failed to set retrieval_meta key '{}'", key))?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM retrieval_meta WHERE key=?1")
            .with_context(|| format!("Failed to prepare retrieval_meta query for key '{}'", key))?;
        let value = stmt
            .query_row(params![key], |row| row.get(0))
            .optional()
            .with_context(|| format!("Failed to query retrieval_meta for key '{}'", key))?;
        Ok(value)
    }

    pub fn quick_check(&self) -> Result<String> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA quick_check(1)")
            .context("Failed to prepare quick_check pragma")?;
        let out: String = stmt
            .query_row([], |row| row.get(0))
            .context("Failed to execute quick_check pragma")?;
        Ok(out)
    }

    pub fn try_load_vec_extension(&self, ext_path_override: Option<&str>) -> Result<bool> {
        let ext_path = match ext_path_override {
            Some(p) => p.to_string(),
            None => match std::env::var("IMPULSE_SQLITE_VEC_EXT") {
                Ok(v) if !v.trim().is_empty() => v,
                _ => return Ok(false),
            },
        };

        // Validate extension path before unsafe load
        let ext_path_buf = std::path::PathBuf::from(&ext_path);
        if !ext_path_buf.is_absolute() {
            bail!(
                "IMPULSE_SQLITE_VEC_EXT must be an absolute path, got: {}",
                ext_path
            );
        }
        for component in ext_path_buf.components() {
            if matches!(component, std::path::Component::ParentDir) {
                bail!("IMPULSE_SQLITE_VEC_EXT must not contain '..': {}", ext_path);
            }
        }
        let valid_extensions = ["so", "dylib", "dll"];
        match ext_path_buf.extension().and_then(|e| e.to_str()) {
            Some(ext) if valid_extensions.contains(&ext) => {}
            _ => {
                bail!(
                    "IMPULSE_SQLITE_VEC_EXT must have a .so, .dylib, or .dll extension: {}",
                    ext_path
                );
            }
        }
        if !ext_path_buf.exists() {
            return Ok(false);
        }

        // SAFETY: `load_extension` loads arbitrary native code. We validate:
        // 1. ext_path is absolute (no relative path resolution tricks)
        // 2. ext_path contains no ".." (prevents path traversal)
        // 3. ext_path ends with a platform-specific library extension (.so/.dylib/.dll)
        // 4. ext_path exists on disk
        // The extension is loaded then immediately disabled to minimize exposure.
        unsafe {
            self.conn
                .load_extension_enable()
                .context("Failed to enable SQLite extension loading")?;
            let result = self.conn.load_extension(&ext_path, None).is_ok();
            self.conn
                .load_extension_disable()
                .context("Failed to disable SQLite extension loading")?;
            Ok(result)
        }
    }

    fn create_history_vec0_table(&self, dim: usize) -> Result<()> {
        if dim == 0 {
            bail!("history vec0 dimension must be greater than zero");
        }
        let sql = format!(
            "CREATE VIRTUAL TABLE history_vec0 USING vec0(session_id TEXT, embedding float[{}])",
            dim
        );
        self.conn
            .execute("DROP TABLE IF EXISTS history_vec0", [])
            .context("Failed to drop existing history_vec0 table")?;
        self.conn
            .execute(&sql, [])
            .context("Failed to create history_vec0 table")?;
        self.set_meta("history_vec0_dim", &dim.to_string())
            .context("Failed to store history_vec0_dim in retrieval_meta")?;
        Ok(())
    }

    fn create_genome_vec0_table(&self, dim: usize) -> Result<()> {
        if dim == 0 {
            bail!("genome vec0 dimension must be greater than zero");
        }
        let sql = format!(
            "CREATE VIRTUAL TABLE genome_vec0 USING vec0(decision_id TEXT, embedding float[{}])",
            dim
        );
        self.conn
            .execute("DROP TABLE IF EXISTS genome_vec0", [])
            .context("Failed to drop existing genome_vec0 table")?;
        self.conn
            .execute(&sql, [])
            .context("Failed to create genome_vec0 table")?;
        self.set_meta("genome_vec0_dim", &dim.to_string())
            .context("Failed to store genome_vec0_dim in retrieval_meta")?;
        Ok(())
    }

    pub fn ensure_history_vec0_table(&self, dim: usize) -> Result<()> {
        let current_dim = self
            .get_meta("history_vec0_dim")
            .context("Failed to read history_vec0_dim from retrieval_meta")?
            .and_then(|v| v.parse::<usize>().ok());
        let exists = self
            .table_exists("history_vec0")
            .context("Failed to check if history_vec0 table exists")?;
        if current_dim != Some(dim) || !exists {
            self.create_history_vec0_table(dim)
                .context("Failed to create history_vec0 table during ensure")?;
        }
        Ok(())
    }

    pub fn ensure_genome_vec0_table(&self, dim: usize) -> Result<()> {
        let current_dim = self
            .get_meta("genome_vec0_dim")
            .context("Failed to read genome_vec0_dim from retrieval_meta")?
            .and_then(|v| v.parse::<usize>().ok());
        let exists = self
            .table_exists("genome_vec0")
            .context("Failed to check if genome_vec0 table exists")?;
        if current_dim != Some(dim) || !exists {
            self.create_genome_vec0_table(dim)
                .context("Failed to create genome_vec0 table during ensure")?;
        }
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin clear_all transaction")?;
        tx.execute("DELETE FROM history_entries", [])
            .context("Failed to delete from history_entries")?;
        tx.execute("DELETE FROM genome_decisions", [])
            .context("Failed to delete from genome_decisions")?;
        tx.execute("DELETE FROM history_fts", [])
            .context("Failed to delete from history_fts")?;
        tx.execute("DELETE FROM genome_fts", [])
            .context("Failed to delete from genome_fts")?;
        tx.execute("DELETE FROM history_vec", [])
            .context("Failed to delete from history_vec")?;
        tx.execute("DELETE FROM genome_vec", [])
            .context("Failed to delete from genome_vec")?;
        let _ = tx.execute("DELETE FROM history_vec0", []);
        let _ = tx.execute("DELETE FROM genome_vec0", []);
        tx.commit()
            .context("Failed to commit clear_all transaction")?;
        Ok(())
    }

    pub fn upsert_history(&self, row: HistoryUpsert<'_>) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO history_entries (
  session_id, session_name, platform, started_at, ended_at, summary,
  files_touched_json, tools_used_json, search_text, content_hash
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
ON CONFLICT(session_id) DO UPDATE SET
  session_name=excluded.session_name,
  platform=excluded.platform,
  started_at=excluded.started_at,
  ended_at=excluded.ended_at,
  summary=excluded.summary,
  files_touched_json=excluded.files_touched_json,
  tools_used_json=excluded.tools_used_json,
  search_text=excluded.search_text,
  content_hash=excluded.content_hash
"#,
                params![
                    row.session_id,
                    row.session_name,
                    row.platform,
                    row.started_at,
                    row.ended_at,
                    row.summary,
                    row.files_touched_json,
                    row.tools_used_json,
                    row.search_text,
                    row.content_hash
                ],
            )
            .context("Failed to upsert history_entries row")?;
        Ok(())
    }

    pub fn upsert_genome(&self, row: GenomeUpsert<'_>) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO genome_decisions (
  decision_id, date, description, rationale, tags_json, search_text, content_hash
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
ON CONFLICT(decision_id) DO UPDATE SET
  date=excluded.date,
  description=excluded.description,
  rationale=excluded.rationale,
  tags_json=excluded.tags_json,
  search_text=excluded.search_text,
  content_hash=excluded.content_hash
"#,
                params![
                    row.decision_id,
                    row.date,
                    row.description,
                    row.rationale,
                    row.tags_json,
                    row.search_text,
                    row.content_hash
                ],
            )
            .context("Failed to upsert genome_decisions row")?;
        Ok(())
    }

    pub fn get_history_hash(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM history_entries WHERE session_id=?1")
            .context("Failed to prepare get_history_hash query")?;
        stmt.query_row(params![session_id], |row| row.get(0))
            .optional()
            .context("Failed to query content_hash from history_entries")
    }

    pub fn get_genome_hash(&self, decision_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM genome_decisions WHERE decision_id=?1")
            .context("Failed to prepare get_genome_hash query")?;
        stmt.query_row(params![decision_id], |row| row.get(0))
            .optional()
            .context("Failed to query content_hash from genome_decisions")
    }

    pub fn has_history_vector(&self, session_id: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM history_vec WHERE session_id=?1 LIMIT 1")
            .context("Failed to prepare has_history_vector query")?;
        Ok(stmt
            .query_row(params![session_id], |_| Ok(1_i64))
            .optional()
            .context("Failed to query history_vec for session existence")?
            .is_some())
    }

    pub fn has_history_vec0(&self, session_id: &str) -> Result<bool> {
        let mut stmt = match self
            .conn
            .prepare("SELECT 1 FROM history_vec0 WHERE session_id=?1 LIMIT 1")
        {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        Ok(stmt
            .query_row(params![session_id], |_| Ok(1_i64))
            .optional()
            .context("Failed to query history_vec0 for session existence")?
            .is_some())
    }

    pub fn has_genome_vector(&self, decision_id: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM genome_vec WHERE decision_id=?1 LIMIT 1")
            .context("Failed to prepare has_genome_vector query")?;
        Ok(stmt
            .query_row(params![decision_id], |_| Ok(1_i64))
            .optional()
            .context("Failed to query genome_vec for decision existence")?
            .is_some())
    }

    pub fn has_genome_vec0(&self, decision_id: &str) -> Result<bool> {
        let mut stmt = match self
            .conn
            .prepare("SELECT 1 FROM genome_vec0 WHERE decision_id=?1 LIMIT 1")
        {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        Ok(stmt
            .query_row(params![decision_id], |_| Ok(1_i64))
            .optional()
            .context("Failed to query genome_vec0 for decision existence")?
            .is_some())
    }

    /// Delete all history entries except those with the given session IDs.
    /// WARNING: This does NOT update the FTS tables. Caller MUST call `refresh_fts()`
    /// after this method to keep keyword search consistent.
    pub fn delete_history_except(&self, keep_ids: &HashSet<String>) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin delete_history_except transaction")?;
        if keep_ids.is_empty() {
            tx.execute("DELETE FROM history_entries", [])
                .context("Failed to delete all from history_entries")?;
            tx.execute("DELETE FROM history_vec", [])
                .context("Failed to delete all from history_vec")?;
            let _ = tx.execute("DELETE FROM history_vec0", []);
            tx.commit()
                .context("Failed to commit delete_history_except transaction")?;
            return Ok(());
        }

        let placeholders = std::iter::repeat_n("?", keep_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql_entries = format!(
            "DELETE FROM history_entries WHERE session_id NOT IN ({})",
            placeholders
        );
        let sql_vec = format!(
            "DELETE FROM history_vec WHERE session_id NOT IN ({})",
            placeholders
        );
        let sql_vec0 = format!(
            "DELETE FROM history_vec0 WHERE session_id NOT IN ({})",
            placeholders
        );
        tx.execute(&sql_entries, rusqlite::params_from_iter(keep_ids.iter()))
            .context("Failed to delete filtered rows from history_entries")?;
        tx.execute(&sql_vec, rusqlite::params_from_iter(keep_ids.iter()))
            .context("Failed to delete filtered rows from history_vec")?;
        let _ = tx.execute(&sql_vec0, rusqlite::params_from_iter(keep_ids.iter()));
        tx.commit()
            .context("Failed to commit delete_history_except transaction")?;
        Ok(())
    }

    /// Delete all genome decisions except those with the given decision IDs.
    /// WARNING: This does NOT update the FTS tables. Caller MUST call `refresh_fts()`
    /// after this method to keep keyword search consistent.
    pub fn delete_genome_except(&self, keep_ids: &HashSet<String>) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin delete_genome_except transaction")?;
        if keep_ids.is_empty() {
            tx.execute("DELETE FROM genome_decisions", [])
                .context("Failed to delete all from genome_decisions")?;
            tx.execute("DELETE FROM genome_vec", [])
                .context("Failed to delete all from genome_vec")?;
            let _ = tx.execute("DELETE FROM genome_vec0", []);
            tx.commit()
                .context("Failed to commit delete_genome_except transaction")?;
            return Ok(());
        }

        let placeholders = std::iter::repeat_n("?", keep_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql_entries = format!(
            "DELETE FROM genome_decisions WHERE decision_id NOT IN ({})",
            placeholders
        );
        let sql_vec = format!(
            "DELETE FROM genome_vec WHERE decision_id NOT IN ({})",
            placeholders
        );
        let sql_vec0 = format!(
            "DELETE FROM genome_vec0 WHERE decision_id NOT IN ({})",
            placeholders
        );
        tx.execute(&sql_entries, rusqlite::params_from_iter(keep_ids.iter()))
            .context("Failed to delete filtered rows from genome_decisions")?;
        tx.execute(&sql_vec, rusqlite::params_from_iter(keep_ids.iter()))
            .context("Failed to delete filtered rows from genome_vec")?;
        let _ = tx.execute(&sql_vec0, rusqlite::params_from_iter(keep_ids.iter()));
        tx.commit()
            .context("Failed to commit delete_genome_except transaction")?;
        Ok(())
    }

    pub fn refresh_fts(&self) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("Failed to begin refresh_fts transaction")?;
        tx.execute("DELETE FROM history_fts", [])
            .context("Failed to delete from history_fts")?;
        tx.execute(
            "INSERT INTO history_fts(session_id, search_text) SELECT session_id, search_text FROM history_entries",
            [],
        ).context("Failed to repopulate history_fts from history_entries")?;

        tx.execute("DELETE FROM genome_fts", [])
            .context("Failed to delete from genome_fts")?;
        tx.execute(
            "INSERT INTO genome_fts(decision_id, search_text) SELECT decision_id, search_text FROM genome_decisions",
            [],
        ).context("Failed to repopulate genome_fts from genome_decisions")?;
        tx.commit()
            .context("Failed to commit refresh_fts transaction")?;
        Ok(())
    }

    pub fn upsert_history_vector(&self, session_id: &str, vector_json: &str) -> Result<()> {
        self.conn
            .execute(
                r#"INSERT INTO history_vec(session_id, vector_json) VALUES (?1, ?2)
               ON CONFLICT(session_id) DO UPDATE SET vector_json=excluded.vector_json"#,
                params![session_id, vector_json],
            )
            .context("Failed to upsert vector into history_vec")?;
        Ok(())
    }

    /// Upsert history vector into vec0 table using atomic single statement.
    /// Uses INSERT OR REPLACE to avoid separate DELETE+INSERT operations.
    pub fn upsert_history_vector_vec0(&self, session_id: &str, vector_json: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO history_vec0(session_id, embedding) VALUES (?1, ?2)",
                params![session_id, vector_json],
            )
            .context("Failed to upsert vector into history_vec0")?;
        Ok(())
    }

    pub fn upsert_genome_vector(&self, decision_id: &str, vector_json: &str) -> Result<()> {
        self.conn
            .execute(
                r#"INSERT INTO genome_vec(decision_id, vector_json) VALUES (?1, ?2)
               ON CONFLICT(decision_id) DO UPDATE SET vector_json=excluded.vector_json"#,
                params![decision_id, vector_json],
            )
            .context("Failed to upsert vector into genome_vec")?;
        Ok(())
    }

    /// Upsert genome vector into vec0 table using atomic single statement.
    /// Uses INSERT OR REPLACE to avoid separate DELETE+INSERT operations.
    pub fn upsert_genome_vector_vec0(&self, decision_id: &str, vector_json: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO genome_vec0(decision_id, embedding) VALUES (?1, ?2)",
                params![decision_id, vector_json],
            )
            .context("Failed to upsert vector into genome_vec0")?;
        Ok(())
    }

    /// Delete history vector entries from both history_vec and history_vec0 tables.
    /// Note: Caller should wrap in a transaction for best performance.
    /// This method is idempotent and safe to call within an existing transaction.
    pub fn delete_history_vector(&self, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM history_vec WHERE session_id=?1",
                params![session_id],
            )
            .context("Failed to delete from history_vec")?;
        let _ = self.conn.execute(
            "DELETE FROM history_vec0 WHERE session_id=?1",
            params![session_id],
        );
        Ok(())
    }

    /// Delete genome vector entries from both genome_vec and genome_vec0 tables.
    /// Note: Caller should wrap in a transaction for best performance.
    /// This method is idempotent and safe to call within an existing transaction.
    pub fn delete_genome_vector(&self, decision_id: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM genome_vec WHERE decision_id=?1",
                params![decision_id],
            )
            .context("Failed to delete from genome_vec")?;
        let _ = self.conn.execute(
            "DELETE FROM genome_vec0 WHERE decision_id=?1",
            params![decision_id],
        );
        Ok(())
    }

    fn sanitize_fts_query(query: &str) -> String {
        let q = query.trim();
        if q.is_empty() {
            return String::new();
        }
        format!("\"{}\"", q.replace('"', "\"\""))
    }

    pub fn search_history_keyword(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let fts_query = Self::sanitize_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = match self.conn.prepare_cached(
            r#"
SELECT h.session_id, h.session_name, h.summary, bm25(history_fts) AS rank
FROM history_fts
JOIN history_entries h ON h.session_id = history_fts.session_id
WHERE history_fts MATCH ?1
ORDER BY rank
LIMIT ?2
"#,
        ) {
            Ok(s) => s,
            Err(_) => return self.search_history_keyword_like(query, limit),
        };

        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok(SearchResult {
                source: "history".to_string(),
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: row.get(2)?,
                score: row.get::<_, f64>(3)?,
            })
        });

        match rows {
            Ok(rows) => {
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.context("Failed to read FTS result row from history_fts")?);
                }
                Ok(out)
            }
            Err(_) => self.search_history_keyword_like(query, limit),
        }
    }

    fn search_history_keyword_like(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let like = format!("%{}%", query.to_lowercase());
        let mut stmt = self
            .conn
            .prepare_cached(
                r#"
SELECT session_id, session_name, summary
FROM history_entries
WHERE lower(search_text) LIKE ?1
ORDER BY ended_at DESC
LIMIT ?2
"#,
            )
            .context("Failed to prepare LIKE search on history_entries")?;

        let rows = stmt
            .query_map(params![like, limit as i64], |row| {
                Ok(SearchResult {
                    source: "history".to_string(),
                    id: row.get(0)?,
                    title: row.get(1)?,
                    snippet: row.get(2)?,
                    score: 0.0,
                })
            })
            .context("Failed to execute LIKE search on history_entries")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("Failed to read history_entries LIKE search result row")?);
        }
        Ok(out)
    }

    pub fn search_genome_keyword(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let fts_query = Self::sanitize_fts_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = match self.conn.prepare_cached(
            r#"
SELECT g.decision_id, g.description, COALESCE(g.rationale, ''), bm25(genome_fts) AS rank
FROM genome_fts
JOIN genome_decisions g ON g.decision_id = genome_fts.decision_id
WHERE genome_fts MATCH ?1
ORDER BY rank
LIMIT ?2
"#,
        ) {
            Ok(s) => s,
            Err(_) => return self.search_genome_keyword_like(query, limit),
        };

        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok(SearchResult {
                source: "genome".to_string(),
                id: row.get(0)?,
                title: row.get(1)?,
                snippet: row.get(2)?,
                score: row.get::<_, f64>(3)?,
            })
        });

        match rows {
            Ok(rows) => {
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.context("Failed to read FTS result row from genome_fts")?);
                }
                Ok(out)
            }
            Err(_) => self.search_genome_keyword_like(query, limit),
        }
    }

    fn search_genome_keyword_like(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let like = format!("%{}%", query.to_lowercase());
        let mut stmt = self
            .conn
            .prepare_cached(
                r#"
SELECT decision_id, description, COALESCE(rationale, '')
FROM genome_decisions
WHERE lower(search_text) LIKE ?1
ORDER BY date DESC
LIMIT ?2
"#,
            )
            .context("Failed to prepare LIKE search on genome_decisions")?;

        let rows = stmt
            .query_map(params![like, limit as i64], |row| {
                Ok(SearchResult {
                    source: "genome".to_string(),
                    id: row.get(0)?,
                    title: row.get(1)?,
                    snippet: row.get(2)?,
                    score: 0.0,
                })
            })
            .context("Failed to execute LIKE search on genome_decisions")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("Failed to read genome_decisions LIKE search result row")?);
        }
        Ok(out)
    }

    pub fn read_history_vectors(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT session_id, vector_json FROM history_vec")
            .context("Failed to prepare read_history_vectors query")?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let raw: String = row.get(1)?;
                Ok((id, raw))
            })
            .context("Failed to query history_vec for vectors")?;

        let mut out = Vec::new();
        for row in rows {
            let (id, raw) = row.context("Failed to read row from history_vec")?;
            let vec: Vec<f32> = serde_json::from_str(&raw).unwrap_or_default();
            if !vec.is_empty() {
                out.push((id, vec));
            }
        }
        Ok(out)
    }

    pub fn search_history_vec_knn(
        &self,
        query_vector_json: &str,
        candidate_limit: usize,
    ) -> Result<Vec<(String, f64)>> {
        if candidate_limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT session_id, distance
FROM history_vec0
WHERE embedding MATCH ?1
ORDER BY distance
LIMIT ?2
"#,
            )
            .context("Failed to prepare KNN search on history_vec0")?;
        let rows = stmt
            .query_map(params![query_vector_json, candidate_limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .context("Failed to execute KNN search on history_vec0")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("Failed to read KNN result row from history_vec0")?);
        }
        Ok(out)
    }

    pub fn read_genome_vectors(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT decision_id, vector_json FROM genome_vec")
            .context("Failed to prepare read_genome_vectors query")?;
        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let raw: String = row.get(1)?;
                Ok((id, raw))
            })
            .context("Failed to query genome_vec for vectors")?;

        let mut out = Vec::new();
        for row in rows {
            let (id, raw) = row.context("Failed to read row from genome_vec")?;
            let vec: Vec<f32> = serde_json::from_str(&raw).unwrap_or_default();
            if !vec.is_empty() {
                out.push((id, vec));
            }
        }
        Ok(out)
    }

    pub fn search_genome_vec_knn(
        &self,
        query_vector_json: &str,
        candidate_limit: usize,
    ) -> Result<Vec<(String, f64)>> {
        if candidate_limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT decision_id, distance
FROM genome_vec0
WHERE embedding MATCH ?1
ORDER BY distance
LIMIT ?2
"#,
            )
            .context("Failed to prepare KNN search on genome_vec0")?;
        let rows = stmt
            .query_map(params![query_vector_json, candidate_limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .context("Failed to execute KNN search on genome_vec0")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("Failed to read KNN result row from genome_vec0")?);
        }
        Ok(out)
    }

    pub fn get_history_by_id(&self, session_id: &str) -> Result<Option<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session_name, summary FROM history_entries WHERE session_id=?1")
            .context("Failed to prepare get_history_by_id query")?;
        let mut rows = stmt
            .query(params![session_id])
            .context("Failed to query history_entries by session_id")?;
        if let Some(row) = rows
            .next()
            .context("Failed to advance cursor on history_entries query")?
        {
            Ok(Some((
                row.get(0)
                    .context("Failed to read session_name from history_entries")?,
                row.get(1)
                    .context("Failed to read summary from history_entries")?,
            )))
        } else {
            Ok(None)
        }
    }

    pub fn get_genome_by_id(&self, decision_id: &str) -> Result<Option<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT description, COALESCE(rationale, '') FROM genome_decisions WHERE decision_id=?1",
        ).context("Failed to prepare get_genome_by_id query")?;
        let mut rows = stmt
            .query(params![decision_id])
            .context("Failed to query genome_decisions by decision_id")?;
        if let Some(row) = rows
            .next()
            .context("Failed to advance cursor on genome_decisions query")?
        {
            Ok(Some((
                row.get(0)
                    .context("Failed to read description from genome_decisions")?,
                row.get(1)
                    .context("Failed to read rationale from genome_decisions")?,
            )))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_store() -> (tempfile::TempDir, RetrievalStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = RetrievalStore::open(tmp.path()).unwrap();
        store.init_schema().unwrap();
        (tmp, store)
    }

    fn test_history_row<'a>(session_id: &'a str, session_name: &'a str) -> HistoryUpsert<'a> {
        HistoryUpsert {
            session_id,
            session_name,
            platform: None,
            started_at: "2026-01-01",
            ended_at: "2026-01-01",
            summary: "summary",
            files_touched_json: "[]",
            tools_used_json: "[]",
            search_text: "text",
            content_hash: "",
        }
    }

    fn test_genome_row<'a>(decision_id: &'a str, description: &'a str) -> GenomeUpsert<'a> {
        GenomeUpsert {
            decision_id,
            date: "2026-01-01",
            description,
            rationale: None,
            tags_json: "[]",
            search_text: "text",
            content_hash: "",
        }
    }

    #[test]
    fn test_open_and_init_schema() {
        let (_tmp, store) = open_test_store();
        assert!(store.table_exists("history_entries").unwrap());
        assert!(store.table_exists("genome_decisions").unwrap());
        assert!(store.table_exists("retrieval_meta").unwrap());
        assert!(store.table_exists("history_fts").unwrap());
        assert!(store.table_exists("genome_fts").unwrap());
        assert!(store.table_exists("history_vec").unwrap());
        assert!(store.table_exists("genome_vec").unwrap());
        assert_eq!(
            store.get_meta("schema_version").unwrap(),
            Some("2".to_string())
        );
    }

    #[test]
    fn test_upsert_and_get_history() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                platform: Some("claude"),
                started_at: "2026-01-01T00:00:00Z",
                ended_at: "2026-01-01T01:00:00Z",
                summary: "built retrieval",
                search_text: "session one built retrieval",
                content_hash: "hash1",
                ..test_history_row("s1", "Session One")
            })
            .unwrap();

        let result = store.get_history_by_id("s1").unwrap();
        assert!(result.is_some());
        let (name, summary) = result.unwrap();
        assert_eq!(name, "Session One");
        assert_eq!(summary, "built retrieval");
    }

    #[test]
    fn test_upsert_and_get_genome() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_genome(GenomeUpsert {
                rationale: Some("performance and safety"),
                tags_json: "[\"arch\"]",
                search_text: "use rust cli performance",
                content_hash: "ghash1",
                ..test_genome_row("d1", "Use Rust for CLI")
            })
            .unwrap();

        let result = store.get_genome_by_id("d1").unwrap();
        assert!(result.is_some());
        let (desc, rationale) = result.unwrap();
        assert_eq!(desc, "Use Rust for CLI");
        assert_eq!(rationale, "performance and safety");
    }

    #[test]
    fn test_content_hash_tracking() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                content_hash: "abc123",
                ..test_history_row("s1", "Sess")
            })
            .unwrap();

        let hash = store.get_history_hash("s1").unwrap();
        assert_eq!(hash, Some("abc123".to_string()));

        let missing = store.get_history_hash("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_search_history_keyword() {
        let (_tmp, store) = open_test_store();
        for i in 0..3 {
            let session_id = format!("s{}", i);
            let session_name = format!("Session {}", i);
            let summary = format!("summary {}", i);
            let search_text = format!("retrieval testing session {}", i);
            store
                .upsert_history(HistoryUpsert {
                    summary: &summary,
                    search_text: &search_text,
                    ..test_history_row(&session_id, &session_name)
                })
                .unwrap();
        }
        store.refresh_fts().unwrap();

        let results = store.search_history_keyword("retrieval", 10).unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.source == "history"));
    }

    #[test]
    fn test_search_genome_keyword() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_genome(GenomeUpsert {
                search_text: "rust performance safety",
                ..test_genome_row("d1", "Use Rust")
            })
            .unwrap();
        store
            .upsert_genome(GenomeUpsert {
                date: "2026-01-02",
                search_text: "sqlite embedded database",
                ..test_genome_row("d2", "Use SQLite")
            })
            .unwrap();
        store.refresh_fts().unwrap();

        let results = store.search_genome_keyword("rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "d1");
    }

    #[test]
    fn test_search_empty_query() {
        let (_tmp, store) = open_test_store();
        let results = store.search_history_keyword("", 10).unwrap();
        assert!(results.is_empty());

        let results = store.search_history_keyword("   ", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_clear_all() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                summary: "sum",
                ..test_history_row("s1", "Sess")
            })
            .unwrap();
        store
            .upsert_genome(GenomeUpsert {
                date: "d",
                ..test_genome_row("d1", "desc")
            })
            .unwrap();

        store.clear_all().unwrap();

        assert!(store.get_history_by_id("s1").unwrap().is_none());
        assert!(store.get_genome_by_id("d1").unwrap().is_none());
    }

    #[test]
    fn test_delete_history_except() {
        let (_tmp, store) = open_test_store();
        for i in 0..3 {
            let session_id = format!("s{}", i);
            store
                .upsert_history(HistoryUpsert {
                    summary: "sum",
                    ..test_history_row(&session_id, "Sess")
                })
                .unwrap();
        }

        let keep: HashSet<String> = ["s1".to_string()].into_iter().collect();
        store.delete_history_except(&keep).unwrap();

        assert!(store.get_history_by_id("s0").unwrap().is_none());
        assert!(store.get_history_by_id("s1").unwrap().is_some());
        assert!(store.get_history_by_id("s2").unwrap().is_none());
    }

    #[test]
    fn test_refresh_fts() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                summary: "sum",
                search_text: "unique_keyword_xyz",
                ..test_history_row("s1", "Sess")
            })
            .unwrap();

        // Before refresh, FTS should be empty
        let before = store
            .search_history_keyword("unique_keyword_xyz", 10)
            .unwrap();
        assert!(before.is_empty());

        store.refresh_fts().unwrap();

        let after = store
            .search_history_keyword("unique_keyword_xyz", 10)
            .unwrap();
        assert_eq!(after.len(), 1);
    }

    #[test]
    fn test_delete_history_except_cleans_vec_tables() {
        let (_tmp, store) = open_test_store();
        for i in 0..3 {
            let sid = format!("s{}", i);
            store
                .upsert_history(HistoryUpsert {
                    summary: "sum",
                    ..test_history_row(&sid, "Sess")
                })
                .unwrap();
            store
                .upsert_history_vector(&sid, "[1.0, 2.0, 3.0]")
                .unwrap();
        }

        // Keep only s1 — s0 and s2 should be removed from both tables atomically
        let keep: HashSet<String> = ["s1".to_string()].into_iter().collect();
        store.delete_history_except(&keep).unwrap();

        assert!(store.get_history_by_id("s0").unwrap().is_none());
        assert!(store.get_history_by_id("s1").unwrap().is_some());
        assert!(store.get_history_by_id("s2").unwrap().is_none());
        // Vec entries must also be cleaned
        assert!(!store.has_history_vector("s0").unwrap());
        assert!(store.has_history_vector("s1").unwrap());
        assert!(!store.has_history_vector("s2").unwrap());
    }

    #[test]
    fn test_delete_genome_except_cleans_vec_tables() {
        let (_tmp, store) = open_test_store();
        for i in 0..3 {
            let did = format!("d{}", i);
            store.upsert_genome(test_genome_row(&did, "desc")).unwrap();
            store.upsert_genome_vector(&did, "[1.0, 2.0, 3.0]").unwrap();
        }

        let keep: HashSet<String> = ["d1".to_string()].into_iter().collect();
        store.delete_genome_except(&keep).unwrap();

        assert!(store.get_genome_by_id("d0").unwrap().is_none());
        assert!(store.get_genome_by_id("d1").unwrap().is_some());
        assert!(store.get_genome_by_id("d2").unwrap().is_none());
        assert!(!store.has_genome_vector("d0").unwrap());
        assert!(store.has_genome_vector("d1").unwrap());
        assert!(!store.has_genome_vector("d2").unwrap());
    }

    #[test]
    fn test_delete_history_except_empty_keeps_clears_all() {
        let (_tmp, store) = open_test_store();
        for i in 0..3 {
            let sid = format!("s{}", i);
            store
                .upsert_history(HistoryUpsert {
                    summary: "sum",
                    ..test_history_row(&sid, "Sess")
                })
                .unwrap();
            store.upsert_history_vector(&sid, "[1.0, 2.0]").unwrap();
        }

        let empty: HashSet<String> = HashSet::new();
        store.delete_history_except(&empty).unwrap();

        for i in 0..3 {
            let sid = format!("s{}", i);
            assert!(store.get_history_by_id(&sid).unwrap().is_none());
            assert!(!store.has_history_vector(&sid).unwrap());
        }
    }

    #[test]
    fn test_clear_all_multi_table_consistency() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                summary: "sum",
                ..test_history_row("s1", "Sess")
            })
            .unwrap();
        store.upsert_history_vector("s1", "[1.0]").unwrap();
        store
            .upsert_genome(GenomeUpsert {
                date: "d",
                ..test_genome_row("d1", "desc")
            })
            .unwrap();
        store.upsert_genome_vector("d1", "[1.0]").unwrap();
        store.refresh_fts().unwrap();

        store.clear_all().unwrap();

        assert!(store.get_history_by_id("s1").unwrap().is_none());
        assert!(store.get_genome_by_id("d1").unwrap().is_none());
        assert!(!store.has_history_vector("s1").unwrap());
        assert!(!store.has_genome_vector("d1").unwrap());
        // FTS should also be empty
        let h = store.search_history_keyword("text", 10).unwrap();
        let g = store.search_genome_keyword("text", 10).unwrap();
        assert!(h.is_empty());
        assert!(g.is_empty());
    }

    #[test]
    fn test_refresh_fts_atomic_both_tables() {
        let (_tmp, store) = open_test_store();
        store
            .upsert_history(HistoryUpsert {
                summary: "sum",
                search_text: "history_kw",
                ..test_history_row("s1", "Sess")
            })
            .unwrap();
        store
            .upsert_genome(GenomeUpsert {
                date: "d",
                search_text: "genome_kw",
                ..test_genome_row("d1", "desc")
            })
            .unwrap();

        store.refresh_fts().unwrap();

        // Both FTS tables should be populated in one atomic operation
        let h = store.search_history_keyword("history_kw", 10).unwrap();
        let g = store.search_genome_keyword("genome_kw", 10).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(g.len(), 1);
    }

    #[test]
    fn test_extension_path_rejects_relative() {
        let (_tmp, store) = open_test_store();
        let result = store.try_load_vec_extension(Some("relative/path/vec.so"));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("absolute path"),
            "should mention absolute path requirement"
        );
    }

    #[test]
    fn test_extension_path_rejects_traversal() {
        let (_tmp, store) = open_test_store();
        let result = store.try_load_vec_extension(Some("/usr/lib/../etc/vec.so"));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains(".."),
            "should mention path traversal"
        );
    }

    #[test]
    fn test_extension_path_missing_file_returns_false() {
        let (_tmp, store) = open_test_store();
        let result = store
            .try_load_vec_extension(Some("/nonexistent/path/vec.so"))
            .unwrap();
        assert!(!result, "missing extension file should return Ok(false)");
    }

    #[test]
    fn test_with_transaction_success() {
        let (_tmp, store) = open_test_store();

        let result = store
            .with_transaction(|tx| {
                tx.execute(
                    "INSERT INTO history_entries (session_id, session_name, platform, started_at, ended_at, summary, files_touched_json, tools_used_json, search_text, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params!["tx-test-1", "TX Test", "test", "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z", "test summary", "[]", "[]", "test search", "txhash1"],
                )?;
                tx.execute(
                    "INSERT INTO history_entries (session_id, session_name, platform, started_at, ended_at, summary, files_touched_json, tools_used_json, search_text, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params!["tx-test-2", "TX Test 2", "test", "2026-01-02T00:00:00Z", "2026-01-02T01:00:00Z", "test summary 2", "[]", "[]", "test search 2", "txhash2"],
                )?;
                Ok::<_, anyhow::Error>(2)
            })
            .unwrap();

        assert_eq!(result, 2);

        // Verify data was committed
        let history = store.get_history_by_id("tx-test-1").unwrap();
        assert!(history.is_some());
        let history2 = store.get_history_by_id("tx-test-2").unwrap();
        assert!(history2.is_some());
    }

    #[test]
    fn test_with_transaction_rollback_on_error() {
        let (_tmp, store) = open_test_store();

        // First insert some data outside the transaction
        store
            .upsert_history(HistoryUpsert {
                platform: Some("test"),
                started_at: "2026-01-01T00:00:00Z",
                ended_at: "2026-01-01T01:00:00Z",
                summary: "before",
                search_text: "before search",
                content_hash: "beforehash",
                ..test_history_row("before-tx", "Before TX")
            })
            .unwrap();

        let result: Result<(), anyhow::Error> = store.with_transaction(|tx| {
            // Insert in transaction
            tx.execute(
                "INSERT INTO history_entries (session_id, session_name, platform, started_at, ended_at, summary, files_touched_json, tools_used_json, search_text, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params!["in-tx", "In TX", "test", "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z", "in tx", "[]", "[]", "in tx", "inhash"],
            )?;
            // Deliberate error to trigger rollback
            Err(anyhow::anyhow!("simulated error"))
        });

        assert!(result.is_err());

        // Verify the pre-transaction data still exists
        let before = store.get_history_by_id("before-tx").unwrap();
        assert!(before.is_some());

        // Verify the in-transaction data was rolled back
        let in_tx = store.get_history_by_id("in-tx").unwrap();
        assert!(
            in_tx.is_none(),
            "transaction data should have been rolled back"
        );
    }

    #[test]
    fn test_with_transaction_error_context() {
        let (_tmp, store) = open_test_store();

        // Test that error context is helpful
        let result: Result<(), anyhow::Error> =
            store.with_transaction(|_tx| Err(anyhow::anyhow!("inner error")));

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("transaction closure failed"),
            "error should mention transaction closure"
        );
    }
}
