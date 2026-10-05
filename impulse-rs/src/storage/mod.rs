//! Atomic file I/O layer and `.impulse/` directory management.
//!
//! All writes use temp file + rename for crash safety. Temp file names
//! include PID + timestamp to avoid collisions. Provides JSON, JSONL,
//! and plain-text read/write helpers via the [`Storage`] struct.
//!
//! JSONL reads tolerate a malformed record (skipping it with a warning) so a
//! crash-torn trailing line in an append-only log can't make the whole log
//! unreadable.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub struct Storage {
    base_path: PathBuf,
}

impl Storage {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.base_path).context("Failed to create storage directory")?;
        Ok(())
    }

    pub fn path(&self, filename: &str) -> PathBuf {
        self.base_path.join(filename)
    }

    pub fn read_json<T: DeserializeOwned + Default>(&self, filename: &str) -> Result<T> {
        let path = self.path(filename);
        if !path.exists() {
            return Ok(T::default());
        }
        let content = fs::read_to_string(&path).context("Failed to read file")?;
        let result = serde_json::from_str(&content).context("Failed to parse JSON")?;
        Ok(result)
    }

    /// Atomic write - uses temp file + rename
    pub fn write_json<T: Serialize>(&self, filename: &str, data: &T) -> Result<()> {
        self.ensure_dir()?;
        let path = self.path(filename);
        let json = serde_json::to_string_pretty(data).context("Failed to serialize JSON")?;
        self.atomic_write(&path, json.as_bytes())
    }

    /// Atomically write security-sensitive JSON with owner-only permissions.
    ///
    /// On Unix the unique temp inode is created as `0600` before any content is
    /// written, so there is no world-readable window before the final rename.
    /// Other platforms retain the same atomic replacement behavior and rely on
    /// their native ACLs.
    pub fn write_private_json<T: Serialize>(&self, filename: &str, data: &T) -> Result<()> {
        self.ensure_dir()?;
        let path = self.path(filename);
        let json = serde_json::to_string_pretty(data).context("Failed to serialize JSON")?;
        Self::atomic_write_private_path(&path, json.as_bytes())
    }

    /// Appends one JSON record as one line.
    ///
    /// The record and its newline go out in a single `write_all` on an
    /// `O_APPEND` file, so concurrent appenders (the daemon and hook
    /// processes) cannot interleave inside a line; `writeln!` used to issue
    /// them as separate writes. If the file ends without a newline (a write
    /// torn by a crash or a full disk), the record starts on a new line
    /// instead of being glued to the fragment and lost with it, and a failed
    /// write is truncated back so it leaves no fragment of its own.
    pub fn append_jsonl(&self, filename: &str, record: &impl Serialize) -> Result<()> {
        self.ensure_dir()?;
        let path = self.path(filename);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .context("Failed to open file for append")?;
        let json = serde_json::to_string(record).context("Failed to serialize JSONL record")?;
        let previous_len = file
            .metadata()
            .context("Failed to read JSONL file metadata")?
            .len();
        let mut line = Vec::with_capacity(json.len() + 2);
        if previous_len > 0 && !ends_with_newline(&mut file, previous_len)? {
            line.push(b'\n');
        }
        line.extend_from_slice(json.as_bytes());
        line.push(b'\n');
        if let Err(error) = file.write_all(&line) {
            let _ = file.set_len(previous_len);
            return Err(error).context("Failed to write JSONL record");
        }
        file.sync_all().context("Failed to sync JSONL")?;
        Ok(())
    }

    pub fn read_jsonl<T: DeserializeOwned>(&self, filename: &str) -> Result<Vec<T>> {
        let path = self.path(filename);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).context("Failed to open JSONL file")?;
        let reader = BufReader::new(file);
        let mut results = Vec::new();
        // Lines are read as bytes: a torn line cut inside a multi-byte
        // character is invalid UTF-8, and `lines()` turned that into an error
        // that aborted the whole read instead of skipping the one record.
        for (idx, line) in reader.split(b'\n').enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim_ascii().is_empty() {
                continue;
            }
            // Skip (don't fail on) a malformed record. A crash can still leave
            // a torn trailing line; one bad line must not make the whole log
            // unreadable.
            match serde_json::from_slice::<T>(&line) {
                Ok(record) => results.push(record),
                Err(err) => tracing::warn!(
                    "skipping malformed JSONL record in {:?} (line {}): {}",
                    path,
                    idx + 1,
                    err
                ),
            }
        }
        Ok(results)
    }

    pub fn read_jsonl_stream<T, F>(&self, filename: &str, mut on_record: F) -> Result<usize>
    where
        T: DeserializeOwned,
        F: FnMut(T) -> Result<()>,
    {
        let path = self.path(filename);
        if !path.exists() {
            return Ok(0);
        }

        let file = File::open(&path).context("Failed to open JSONL file")?;
        let reader = BufReader::new(file);
        let mut count = 0usize;
        for (idx, line) in reader.split(b'\n').enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim_ascii().is_empty() {
                continue;
            }
            // Skip malformed records (e.g. a crash-torn trailing line) rather
            // than aborting the whole stream — see read_jsonl.
            let record: T = match serde_json::from_slice(&line) {
                Ok(record) => record,
                Err(err) => {
                    tracing::warn!(
                        "skipping malformed JSONL record in {:?} (line {}): {}",
                        path,
                        idx + 1,
                        err
                    );
                    continue;
                }
            };
            on_record(record)?;
            count += 1;
        }
        Ok(count)
    }

    /// Unified atomic write - shared by write_json and write
    /// Public for use by stewardship and other modules.
    /// Uses a unique temp file name to prevent collisions from concurrent writes.
    pub fn atomic_write(&self, path: &Path, content: &[u8]) -> Result<()> {
        Self::atomic_write_path(path, content)
    }

    /// Atomic write helper for arbitrary paths.
    ///
    /// Uses a PID+timestamp-unique temp file to avoid collisions when
    /// multiple processes write concurrently (e.g. parallel hook installs).
    pub fn atomic_write_path(path: &Path, content: &[u8]) -> Result<()> {
        Self::atomic_write_path_with_mode(path, content, None)
    }

    /// Atomic write helper for data that must never be exposed through the
    /// process umask on Unix.
    pub fn atomic_write_private_path(path: &Path, content: &[u8]) -> Result<()> {
        Self::atomic_write_path_with_mode(path, content, Some(0o600))
    }

    fn atomic_write_path_with_mode(
        path: &Path,
        content: &[u8],
        unix_mode: Option<u32>,
    ) -> Result<()> {
        let unique_suffix = format!(
            "tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let temp_path = path.with_extension(unique_suffix);
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = unix_mode;
        // `create_new` means a failed open created nothing, so there is no
        // temp file to clean up yet.
        let file = options
            .open(&temp_path)
            .with_context(|| format!("Failed to create temp file {:?}", temp_path))?;
        let written = Self::fill_and_rename(file, &temp_path, path, content, unix_mode);
        if written.is_err() {
            // A failed write, sync, or rename used to leave the temp file
            // behind next to the target.
            let _ = fs::remove_file(&temp_path);
        }
        written
    }

    /// Writes `content` to the open temp file, syncs it, and renames it over
    /// `path`. The directory is synced afterwards so the rename itself
    /// survives a crash; that step is best effort, since some filesystems
    /// refuse to sync a directory and the rename has already taken effect.
    fn fill_and_rename(
        mut file: fs::File,
        temp_path: &Path,
        path: &Path,
        content: &[u8],
        unix_mode: Option<u32>,
    ) -> Result<()> {
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            // `open(mode)` is filtered through the process umask. Tighten or
            // restore the exact owner-only mode before writing any content.
            file.set_permissions(fs::Permissions::from_mode(mode))
                .with_context(|| format!("Failed to restrict temp file {:?}", temp_path))?;
        }
        #[cfg(not(unix))]
        let _ = unix_mode;
        file.write_all(content)
            .with_context(|| format!("Failed to write temp file {:?}", temp_path))?;
        file.sync_all()
            .with_context(|| format!("Failed to sync temp file {:?}", temp_path))?;
        drop(file);
        fs::rename(temp_path, path)
            .with_context(|| format!("Failed to rename {:?} to {:?}", temp_path, path))?;
        #[cfg(unix)]
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            if let Ok(dir) = fs::File::open(dir) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn exists(&self, filename: &str) -> bool {
        self.path(filename).exists()
    }

    #[cfg(test)]
    pub fn delete(&self, filename: &str) -> Result<()> {
        let path = self.path(filename);
        if path.exists() {
            fs::remove_file(&path).context("Failed to delete file")?;
        }
        Ok(())
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn get_working_dir_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Whether the file's last byte (at `len - 1`) is a newline.
fn ends_with_newline(file: &mut File, len: u64) -> Result<bool> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    file.seek(SeekFrom::Start(len - 1))
        .context("Failed to seek to the end of the JSONL file")?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)
        .context("Failed to read the end of the JSONL file")?;
    Ok(last[0] == b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    /// Review P3-2: a failed rename left the temp file next to the target.
    #[test]
    fn test_atomic_write_path_removes_its_temp_file_when_the_write_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        // A non-empty directory where the file should go makes the rename fail.
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"x").unwrap();

        assert!(Storage::atomic_write_path(&target, b"content").is_err());
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "target")
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn test_atomic_write_path_replaces_the_file_and_leaves_no_temp() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("state.json");
        fs::write(&target, b"old").unwrap();

        Storage::atomic_write_path(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_storage_new() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());
        assert_eq!(storage.base_path(), temp_dir.path());
    }

    #[test]
    fn test_storage_path() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        let path = storage.path("test.json");
        assert!(path.to_string_lossy().ends_with("test.json"));
    }

    #[test]
    fn test_write_and_read_json() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        #[derive(Serialize, Deserialize, Debug, PartialEq, Default)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        storage.write_json("data.json", &data).unwrap();

        let read: TestData = storage.read_json("data.json").unwrap();
        assert_eq!(read.name, "test");
        assert_eq!(read.value, 42);
    }

    #[test]
    fn test_read_json_default_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        #[derive(Deserialize, Default)]
        struct TestData {
            name: String,
            value: i32,
        }

        let read: TestData = storage.read_json("missing.json").unwrap();
        assert_eq!(read.name, "");
        assert_eq!(read.value, 0);
    }

    #[test]
    fn test_append_jsonl() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        #[derive(Serialize, Deserialize)]
        struct Record {
            id: i32,
            name: String,
        }

        storage
            .append_jsonl(
                "log.jsonl",
                &Record {
                    id: 1,
                    name: "first".to_string(),
                },
            )
            .unwrap();
        storage
            .append_jsonl(
                "log.jsonl",
                &Record {
                    id: 2,
                    name: "second".to_string(),
                },
            )
            .unwrap();

        let records: Vec<Record> = storage.read_jsonl("log.jsonl").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "first");
        assert_eq!(records[1].name, "second");
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Numbered {
        id: i32,
    }

    /// Review P2: a torn tail with no newline swallowed the next record
    /// (both landed on one unparsable line). The old test wrote its torn line
    /// with `writeln!`, so it could not see this.
    #[test]
    fn test_append_after_a_torn_tail_keeps_the_new_record() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());
        storage
            .append_jsonl("log.jsonl", &Numbered { id: 1 })
            .unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(storage.path("log.jsonl"))
            .unwrap();
        f.write_all(b"{\"id\":").unwrap(); // no newline
        drop(f);
        storage
            .append_jsonl("log.jsonl", &Numbered { id: 2 })
            .unwrap();

        let records: Vec<Numbered> = storage.read_jsonl("log.jsonl").unwrap();
        assert_eq!(records, vec![Numbered { id: 1 }, Numbered { id: 2 }]);
    }

    /// Review P2: `lines()` failed the whole read on invalid UTF-8 (a line
    /// cut inside a multi-byte character).
    #[test]
    fn test_read_jsonl_skips_a_line_with_invalid_utf8() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());
        storage
            .append_jsonl("log.jsonl", &Numbered { id: 1 })
            .unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(storage.path("log.jsonl"))
            .unwrap();
        f.write_all(b"{\"id\": \xe2\x82\n").unwrap();
        drop(f);
        storage
            .append_jsonl("log.jsonl", &Numbered { id: 3 })
            .unwrap();

        let records: Vec<Numbered> = storage.read_jsonl("log.jsonl").unwrap();
        assert_eq!(records, vec![Numbered { id: 1 }, Numbered { id: 3 }]);
        let mut streamed = Vec::new();
        storage
            .read_jsonl_stream("log.jsonl", |record: Numbered| {
                streamed.push(record);
                Ok(())
            })
            .unwrap();
        assert_eq!(streamed.len(), 2);
    }

    #[test]
    fn test_concurrent_appends_never_interleave_within_a_line() {
        let temp_dir = TempDir::new().unwrap();
        let storage = std::sync::Arc::new(Storage::new(temp_dir.path().to_path_buf()));
        let handles: Vec<_> = (0..16)
            .map(|thread| {
                let storage = std::sync::Arc::clone(&storage);
                std::thread::spawn(move || {
                    for i in 0..25 {
                        storage
                            .append_jsonl(
                                "log.jsonl",
                                &Numbered {
                                    id: thread * 100 + i,
                                },
                            )
                            .unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let raw = std::fs::read_to_string(storage.path("log.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 400);
        assert!(raw
            .lines()
            .all(|line| serde_json::from_str::<Numbered>(line).is_ok()));
    }

    #[test]
    fn test_read_jsonl_skips_malformed_lines() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        #[derive(Serialize, Deserialize)]
        struct Record {
            id: i32,
        }

        storage
            .append_jsonl("log.jsonl", &Record { id: 1 })
            .unwrap();
        // Simulate a crash-torn trailing line: a partial/invalid JSON record.
        let mut f = OpenOptions::new()
            .append(true)
            .open(storage.path("log.jsonl"))
            .unwrap();
        writeln!(f, "{{\"id\": 2, \"na").unwrap();
        drop(f);
        storage
            .append_jsonl("log.jsonl", &Record { id: 3 })
            .unwrap();

        // The valid records survive; the torn line is skipped, not fatal.
        let records: Vec<Record> = storage.read_jsonl("log.jsonl").unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, 1);
        assert_eq!(records[1].id, 3);

        // The streaming reader is equally resilient.
        let mut seen = Vec::new();
        let count = storage
            .read_jsonl_stream::<Record, _>("log.jsonl", |r| {
                seen.push(r.id);
                Ok(())
            })
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(seen, vec![1, 3]);
    }

    #[test]
    fn test_read_jsonl_empty_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        let records: Vec<String> = storage.read_jsonl("missing.jsonl").unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_read_jsonl_stream() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        #[derive(Serialize, Deserialize)]
        struct Record {
            id: i32,
            name: String,
        }

        storage
            .append_jsonl(
                "stream.jsonl",
                &Record {
                    id: 1,
                    name: "first".to_string(),
                },
            )
            .unwrap();
        storage
            .append_jsonl(
                "stream.jsonl",
                &Record {
                    id: 2,
                    name: "second".to_string(),
                },
            )
            .unwrap();

        let mut ids = Vec::new();
        let count = storage
            .read_jsonl_stream::<Record, _>("stream.jsonl", |r| {
                ids.push(r.id);
                Ok(())
            })
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn test_exists() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        assert!(!storage.exists("test.json"));

        storage
            .write_json("test.json", &serde_json::json!({"key": "value"}))
            .unwrap();

        assert!(storage.exists("test.json"));
    }

    #[test]
    fn test_delete() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Storage::new(temp_dir.path().to_path_buf());

        storage
            .write_json("test.json", &serde_json::json!({"key": "value"}))
            .unwrap();
        assert!(storage.exists("test.json"));

        storage.delete("test.json").unwrap();
        assert!(!storage.exists("test.json"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test.txt"), "test.txt");
        assert_eq!(sanitize_filename("test/file.txt"), "test-file.txt");
        assert_eq!(sanitize_filename("test\\file.txt"), "test-file.txt");
        assert_eq!(sanitize_filename("test:file.txt"), "test-file.txt");
    }

    #[test]
    fn test_get_working_dir_name() {
        let name = get_working_dir_name();
        assert!(!name.is_empty());
        assert_ne!(name, "unknown");
    }
}
