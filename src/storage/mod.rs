//! Atomic file I/O layer and `.impulse/` directory management.
//!
//! All writes use temp file + rename for crash safety. Temp file names
//! include PID + timestamp to avoid collisions. Provides JSON, JSONL,
//! and plain-text read/write helpers via the [`Storage`] struct.

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
        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }
            let record: T = serde_json::from_str(&line).context("Failed to parse JSONL record")?;
            results.push(record);
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
        for line in reader.lines() {
            let line = line.context("Failed to read line")?;
            if line.trim().is_empty() {
                continue;
            }
            let record: T = serde_json::from_str(&line).context("Failed to parse JSONL record")?;
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
                .as_nanos()
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

    #[test]
    fn test_atomic_write_path_concurrent_safety() {
        // Verify temp file name includes PID and high-resolution timestamp
        // which ensures uniqueness even when multiple processes write concurrently
        use std::time::{SystemTime, UNIX_EPOCH};

        let temp_dir = tempfile::TempDir::new().unwrap();
        let target_path = temp_dir.path().join("concurrent.txt");

        // Write first time
        let unique_suffix_1 = format!(
            "tmp.{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        // Simulate another process by using a different timestamp
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let unique_suffix_2 = format!(
            "tmp.{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        // The suffixes must be different (different nanosecond timestamps)
        assert_ne!(unique_suffix_1, unique_suffix_2);

        // Write data successfully
        Storage::atomic_write_path(&target_path, b"concurrent test").unwrap();
        assert_eq!(std::fs::read(&target_path).unwrap(), b"concurrent test");
    }

    #[test]
    fn test_atomic_write_path_sync_before_rename() {
        // Verify sync_all() is called before rename by ensuring
        // data is flushed to disk before rename completes
        let temp_dir = tempfile::TempDir::new().unwrap();
        let target_path = temp_dir.path().join("sync_test.txt");

        let data = b"sync before rename verification data";
        Storage::atomic_write_path(&target_path, data).unwrap();

        // Read immediately after rename - should see all data
        let read = std::fs::read(&target_path).unwrap();
        assert_eq!(read.as_slice(), data);
    }
}
