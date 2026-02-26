//! JSONL persistence for agent panel conversation history.
//!
//! Messages are stored one-per-line in `.impulse/agent_history.jsonl`.
//! Uses atomic writes (temp file + rename) to prevent corruption.
//! File is rotated when it exceeds MAX_FILE_SIZE — old messages are
//! trimmed to keep the last MAX_MESSAGES entries.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use super::chat::ChatMessage;

/// Maximum file size before rotation (1 MB).
const MAX_FILE_SIZE: u64 = 1_048_576;

/// Maximum number of messages to keep after rotation.
const MAX_MESSAGES: usize = 500;

/// Number of recent messages to load on startup.
const LOAD_LIMIT: usize = 100;

/// Discover the history file path.
///
/// Looks for `.impulse/` directory walking up from cwd, or falls back to
/// `$IMPULSE_HOME/agent_history.jsonl`.
pub fn history_path() -> PathBuf {
    if let Ok(home) = std::env::var("IMPULSE_HOME") {
        return PathBuf::from(home).join("agent_history.jsonl");
    }

    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            let candidate = dir.join(".impulse");
            if candidate.is_dir() {
                return candidate.join("agent_history.jsonl");
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    // Fallback to relative path.
    PathBuf::from(".impulse/agent_history.jsonl")
}

/// Load the last N messages from the JSONL history file.
///
/// Skips unparseable lines gracefully — JSONL is append-only, so one
/// bad line doesn't corrupt the rest.
pub fn load_history(path: &Path) -> Vec<ChatMessage> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let reader = BufReader::new(file);
    let mut messages: Vec<ChatMessage> = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<ChatMessage>(trimmed) {
            Ok(msg) => messages.push(msg),
            Err(e) => {
                log::debug!("Skipping unparseable history line: {}", e);
            }
        }
    }

    // Only keep the last LOAD_LIMIT messages.
    if messages.len() > LOAD_LIMIT {
        messages.drain(..messages.len() - LOAD_LIMIT);
    }

    messages
}

/// Append a single message to the JSONL history file.
///
/// Creates the parent directory if needed. Uses append mode for
/// single-message writes (atomic enough for append-only operations).
pub fn append_message(path: &Path, msg: &ChatMessage) {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                log::warn!("Failed to create history directory: {}", e);
                return;
            }
        }
    }

    let json = match serde_json::to_string(msg) {
        Ok(j) => j,
        Err(e) => {
            log::warn!("Failed to serialize message: {}", e);
            return;
        }
    };

    let mut file = match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Failed to open history file: {}", e);
            return;
        }
    };

    if let Err(e) = writeln!(file, "{}", json) {
        log::warn!("Failed to write history line: {}", e);
    }

    // Check if rotation is needed.
    if let Ok(meta) = path.metadata() {
        if meta.len() > MAX_FILE_SIZE {
            rotate_history(path);
        }
    }
}

/// Rotate the history file: keep only the last MAX_MESSAGES entries.
///
/// Uses atomic write (temp file + rename) for the rotated file.
fn rotate_history(path: &Path) {
    log::info!("Rotating agent history (exceeds {} bytes)", MAX_FILE_SIZE);

    let messages = load_all_messages(path);
    let to_keep = if messages.len() > MAX_MESSAGES {
        &messages[messages.len() - MAX_MESSAGES..]
    } else {
        &messages
    };

    // Write to temp file, then rename atomically.
    let tmp_path = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));

    let write_result = (|| -> Result<(), std::io::Error> {
        let mut file = fs::File::create(&tmp_path)?;
        for msg in to_keep {
            if let Ok(json) = serde_json::to_string(msg) {
                writeln!(file, "{}", json)?;
            }
        }
        file.flush()?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if let Err(e) = write_result {
        log::warn!("Failed to rotate history: {}", e);
        // Clean up temp file on failure.
        let _ = fs::remove_file(&tmp_path);
    }
}

/// Load ALL messages from the history file (used during rotation).
fn load_all_messages(path: &Path) -> Vec<ChatMessage> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_panel::chat::{ChatMessage, ChatRole};

    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_history_path() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "impulse-test-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            id,
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("agent_history.jsonl")
    }

    #[test]
    fn test_load_empty_history() {
        let path = temp_history_path();
        let messages = load_history(&path);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_append_and_load() {
        let path = temp_history_path();

        append_message(&path, &ChatMessage::user("hello"));
        append_message(&path, &ChatMessage::agent("world"));

        let messages = load_history(&path);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, ChatRole::User);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].role, ChatRole::Agent);
        assert_eq!(messages[1].content, "world");

        // Clean up.
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_skips_bad_lines() {
        let path = temp_history_path();

        // Write a valid line, a bad line, and another valid line.
        let mut file = fs::File::create(&path).unwrap();
        let msg1 = ChatMessage::user("first");
        writeln!(file, "{}", serde_json::to_string(&msg1).unwrap()).unwrap();
        writeln!(file, "THIS IS NOT JSON").unwrap();
        writeln!(file, "").unwrap(); // empty line
        let msg2 = ChatMessage::agent("second");
        writeln!(file, "{}", serde_json::to_string(&msg2).unwrap()).unwrap();
        drop(file);

        let messages = load_history(&path);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "second");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_limits_to_load_limit() {
        let path = temp_history_path();

        // Write more than LOAD_LIMIT messages.
        for i in 0..(LOAD_LIMIT + 50) {
            append_message(&path, &ChatMessage::user(&format!("msg-{}", i)));
        }

        let messages = load_history(&path);
        assert_eq!(messages.len(), LOAD_LIMIT);
        // Last message should be the most recent.
        assert_eq!(
            messages.last().unwrap().content,
            format!("msg-{}", LOAD_LIMIT + 49)
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_rotation_keeps_max_messages() {
        let path = temp_history_path();

        // Write enough messages to trigger rotation by simulating large content.
        // Instead of writing MAX_FILE_SIZE bytes, just call rotate directly.
        for i in 0..(MAX_MESSAGES + 100) {
            let msg = ChatMessage::user(&format!("msg-{}", i));
            let json = serde_json::to_string(&msg).unwrap();
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(file, "{}", json).unwrap();
        }

        rotate_history(&path);

        let messages = load_all_messages(&path);
        assert_eq!(messages.len(), MAX_MESSAGES);
        // Should keep the LAST MAX_MESSAGES.
        assert_eq!(messages[0].content, format!("msg-{}", 100));
        assert_eq!(
            messages.last().unwrap().content,
            format!("msg-{}", MAX_MESSAGES + 99)
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_append_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "impulse-test-nested-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        let path = dir.join("deep").join("nested").join("history.jsonl");

        append_message(&path, &ChatMessage::user("hello"));

        assert!(path.exists());
        let messages = load_history(&path);
        assert_eq!(messages.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_system_messages_are_not_persisted_filter() {
        // Verify that system messages CAN be serialized (they just
        // shouldn't be appended by the caller — this is a design choice
        // enforced in mod.rs, not in persistence.rs).
        let msg = ChatMessage::system("system info");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("system"));
    }
}
