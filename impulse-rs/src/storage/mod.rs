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
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

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

    pub fn append_jsonl(&self, filename: &str, record: &impl Serialize) -> Result<()> {
        self.ensure_dir()?;
        let path = self.path(filename);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context("Failed to open file for append")?;
        let json = serde_json::to_string(record).context("Failed to serialize JSONL record")?;
        writeln!(file, "{}", json).context("Failed to write JSONL record")?;
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
        for (idx, line) in reader.lines().enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }
            // Skip (don't fail on) a malformed record. These are append-only
            // logs written non-atomically, so a crash can leave a torn trailing
            // line — one bad line must not make the whole log unreadable.
            match serde_json::from_str::<T>(&line) {
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
        for (idx, line) in reader.lines().enumerate() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }
            // Skip malformed records (e.g. a crash-torn trailing line) rather
            // than aborting the whole stream — see read_jsonl.
            let record: T = match serde_json::from_str(&line) {
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
        let unique_suffix = format!(
            "tmp.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let temp_path = path.with_extension(unique_suffix);
        let mut file = File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file {:?}", temp_path))?;
        file.write_all(content)
            .with_context(|| format!("Failed to write temp file {:?}", temp_path))?;
        file.sync_all()
            .with_context(|| format!("Failed to sync temp file {:?}", temp_path))?;
        drop(file);
        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to rename {:?} to {:?}", temp_path, path))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
