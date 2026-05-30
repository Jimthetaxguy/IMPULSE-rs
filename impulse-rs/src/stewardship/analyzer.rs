use anyhow::Result;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use super::types::*;
use crate::state::Config;

/// Estimate token count from character count (~4 chars per token)
pub fn estimate_tokens(content: &str) -> usize {
    estimate_tokens_from_len(content.len())
}

/// Estimate token count directly from a character/byte count (~4 chars per token).
/// Avoids unnecessary heap allocation compared to `estimate_tokens(&"x".repeat(n))`.
pub fn estimate_tokens_from_len(char_count: usize) -> usize {
    char_count.div_ceil(4)
}

/// Estimate context percentage from tokens vs window size
pub fn estimate_context_pct(total_tokens: usize, window_tokens: usize) -> f32 {
    if window_tokens == 0 {
        return 0.0;
    }
    (total_tokens as f32) / (window_tokens as f32)
}

/// Quick estimate from file size (no JSONL parsing)
/// Returns (file_size_bytes, estimated_tokens, estimated_pct)
pub fn quick_estimate_from_file(
    transcript_path: &Path,
    context_window_tokens: usize,
) -> Result<(u64, usize, f32)> {
    let metadata = std::fs::metadata(transcript_path)?;
    let file_size = metadata.len();
    // JSONL has JSON overhead (~40% non-content), so effective content is ~60%
    let effective_chars = (file_size as f64 * 0.6) as usize;
    let estimated_tokens = estimate_tokens_from_len(effective_chars);
    let pct = estimate_context_pct(estimated_tokens, context_window_tokens);
    Ok((file_size, estimated_tokens, pct))
}

/// Parse a single JSONL line into a TranscriptMessage (if it's a user/assistant message)
fn parse_transcript_entry(line: &str) -> Option<TranscriptMessage> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let msg_type = value.get("type")?.as_str()?;

    match msg_type {
        "user" => {
            let content = value.get("content")?;
            let text = extract_text_content(content);
            let char_count = text.len();
            Some(TranscriptMessage {
                role: "user".to_string(),
                text_content: text,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
                char_count,
                estimated_tokens: estimate_tokens_from_len(char_count),
            })
        }
        "assistant" => {
            let content = value.get("content")?;
            let (text, tool_uses, tool_results) = extract_assistant_content(content);
            let char_count = text.len()
                + tool_uses.iter().map(|t| t.input_chars).sum::<usize>()
                + tool_results.iter().map(|t| t.content_chars).sum::<usize>();
            Some(TranscriptMessage {
                role: "assistant".to_string(),
                text_content: text,
                tool_uses,
                tool_results,
                char_count,
                estimated_tokens: estimate_tokens_from_len(char_count),
            })
        }
        _ => None, // Skip file-history-snapshot, summary, etc.
    }
}

/// Extract text from user content (can be string or array)
fn extract_text_content(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type")?.as_str()? == "text" {
                    item.get("text")?.as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Extract text, tool uses, and tool results from assistant content
fn extract_assistant_content(
    content: &serde_json::Value,
) -> (String, Vec<ToolUse>, Vec<ParsedToolResult>) {
    let mut text_parts = Vec::new();
    let mut tool_uses = Vec::new();
    let mut tool_results = Vec::new();

    if let serde_json::Value::Array(arr) = content {
        for item in arr {
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match item_type {
                "text" => {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(t.to_string());
                    }
                }
                "tool_use" => {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input_str = match item.get("input") {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => String::new(),
                    };
                    let input_chars = input_str.len();
                    let preview_len = input_str.len().min(200);
                    tool_uses.push(ToolUse {
                        id,
                        name,
                        input_preview: input_str[..preview_len].to_string(),
                        input_chars,
                    });
                }
                "tool_result" => {
                    let tool_use_id = item
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let content_str = item
                        .get("content")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    tool_results.push(ParsedToolResult {
                        tool_use_id,
                        content_chars: content_str.len(),
                    });
                }
                _ => {}
            }
        }
    } else if let serde_json::Value::String(s) = content {
        text_parts.push(s.clone());
    }

    (text_parts.join("\n"), tool_uses, tool_results)
}

/// Parse a session JSONL file and produce a SessionAnalysis
pub fn analyze_session(
    transcript_path: &Path,
    session_id: &str,
    project_hash: &str,
    config: &Config,
) -> Result<SessionAnalysis> {
    let file = std::fs::File::open(transcript_path)?;
    let reader = std::io::BufReader::new(file);

    let mut messages = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(msg) = parse_transcript_entry(&line) {
            messages.push(msg);
        }
    }

    let message_count = messages.len();
    let total_chars: usize = messages.iter().map(|m| m.char_count).sum();
    let estimated_tokens = estimate_tokens_from_len(total_chars);
    let context_window = config.stewardship_context_window_tokens;
    let estimated_pct = estimate_context_pct(estimated_tokens, context_window);

    let decisions = extract_decisions(&messages);
    let files_touched = extract_files_touched(&messages);
    let tool_patterns = find_tool_patterns(&messages);
    let duplicate_regions = find_duplicate_regions(&messages);
    let rot_candidates = find_rot_candidates(&messages);
    let key_insights = extract_key_insights(&messages);

    Ok(SessionAnalysis {
        session_id: session_id.to_string(),
        project_hash: project_hash.to_string(),
        transcript_path: transcript_path.to_path_buf(),
        analyzed_at: chrono::Utc::now(),
        message_count,
        estimated_tokens,
        estimated_context_pct: estimated_pct,
        decisions,
        files_touched,
        tool_patterns,
        duplicate_regions,
        rot_candidates,
        key_insights,
    })
}

/// Extract decisions from assistant messages (pattern-match for decision language)
fn extract_decisions(messages: &[TranscriptMessage]) -> Vec<ExtractedDecision> {
    let decision_patterns = [
        "decided to",
        "chose ",
        "will use ",
        "going with ",
        "approach:",
        "decision:",
        "selected ",
        "opted for ",
    ];

    let mut decisions = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        if msg.role != "assistant" {
            continue;
        }
        let text_lower = msg.text_content.to_lowercase();
        for pattern in &decision_patterns {
            if let Some(pos) = text_lower.find(pattern) {
                // Extract surrounding context (up to 200 chars around the match)
                let start = pos.saturating_sub(50);
                let end = (pos + 200).min(msg.text_content.len());
                let context = msg.text_content[start..end].to_string();
                // Extract the decision sentence
                let sentence_start = msg.text_content[..pos]
                    .rfind(['.', '!', '\n'])
                    .map(|i| i + 1)
                    .unwrap_or(start);
                let sentence_end = msg.text_content[pos..]
                    .find(['.', '!', '\n'])
                    .map(|i| pos + i + 1)
                    .unwrap_or(end);
                let description = msg.text_content
                    [sentence_start..sentence_end.min(msg.text_content.len())]
                    .trim()
                    .to_string();

                if description.len() > 10 {
                    decisions.push(ExtractedDecision {
                        description,
                        context,
                        message_index: idx,
                    });
                }
                break; // One decision per message per pattern
            }
        }
    }
    decisions
}

/// Extract file paths from tool use inputs (Write, Read, Edit tools)
fn extract_files_touched(messages: &[TranscriptMessage]) -> Vec<String> {
    let file_tools = ["Write", "Read", "Edit", "Glob"];
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for msg in messages {
        for tool in &msg.tool_uses {
            if file_tools.contains(&tool.name.as_str()) {
                // Try to extract file_path from input preview
                if let Some(input) = parse_tool_input(&tool.input_preview) {
                    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                        if seen.insert(path.to_string()) {
                            files.push(path.to_string());
                        }
                    }
                }
            }
        }
    }
    files
}

fn parse_tool_input(input_preview: &str) -> Option<serde_json::Value> {
    let value = serde_json::from_str::<serde_json::Value>(input_preview).ok()?;
    match value {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(&s).ok(),
        other => Some(other),
    }
}

/// Find repeated tool call patterns
fn find_tool_patterns(messages: &[TranscriptMessage]) -> Vec<ToolPattern> {
    let mut patterns: HashMap<String, ToolPattern> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        for tool in &msg.tool_uses {
            // Hash: tool name + first 200 chars of input
            let hash_input = format!("{}:{}", tool.name, &tool.input_preview);
            let hash = format!("{:x}", sha2_hash(hash_input.as_bytes()));

            let entry = patterns.entry(hash.clone()).or_insert_with(|| ToolPattern {
                tool_name: tool.name.clone(),
                count: 0,
                input_hash: hash,
                first_index: idx,
                last_index: idx,
            });
            entry.count += 1;
            entry.last_index = idx;
        }
    }

    patterns.into_values().filter(|p| p.count >= 2).collect()
}

/// Simple hash for dedup (using first 16 bytes of SHA256)
fn sha2_hash(data: &[u8]) -> u64 {
    use sha2::{Digest, Sha256};
    let result = Sha256::digest(data);
    let mut prefix = [0_u8; 8];
    prefix.copy_from_slice(&result[..8]);
    u64::from_le_bytes(prefix)
}

/// Find duplicate regions (consecutive repeated tool calls)
fn find_duplicate_regions(messages: &[TranscriptMessage]) -> Vec<DuplicateRegion> {
    let mut regions = Vec::new();
    let mut consecutive: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, msg) in messages.iter().enumerate() {
        for tool in &msg.tool_uses {
            let key = format!(
                "{}:{}",
                tool.name,
                &tool.input_preview[..tool.input_preview.len().min(100)]
            );
            consecutive.entry(key).or_default().push(idx);
        }
    }

    for (key, indices) in &consecutive {
        if indices.len() >= 3 {
            let tool_name = key.split(':').next().unwrap_or("unknown").to_string();
            let preview = key.split(':').skip(1).collect::<Vec<_>>().join(":");
            let estimated_tokens: usize = indices
                .iter()
                .filter_map(|&i| messages.get(i))
                .map(|m| m.estimated_tokens)
                .sum();
            regions.push(DuplicateRegion {
                tool_name,
                occurrences: indices.len(),
                indices: indices.clone(),
                estimated_tokens,
                input_preview: preview[..preview.len().min(100)].to_string(),
            });
        }
    }

    regions
}

/// Find rot candidates (early context no longer relevant)
fn find_rot_candidates(messages: &[TranscriptMessage]) -> Vec<RotCandidate> {
    if messages.len() < 10 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let early_cutoff = messages.len() / 5; // First 20%
    let late_start = messages.len() * 3 / 5; // Last 40%

    // Collect files referenced in early vs late messages
    let early_files: std::collections::HashSet<String> = messages[..early_cutoff]
        .iter()
        .flat_map(|m| m.tool_uses.iter().map(|t| t.name.clone()))
        .collect();

    let late_files: std::collections::HashSet<String> = messages[late_start..]
        .iter()
        .flat_map(|m| m.tool_uses.iter().map(|t| t.name.clone()))
        .collect();

    // If early work used tools not seen in late work, it may be rot
    let abandoned: Vec<_> = early_files.difference(&late_files).cloned().collect();
    if !abandoned.is_empty() {
        let early_tokens: usize = messages[..early_cutoff]
            .iter()
            .map(|m| m.estimated_tokens)
            .sum();
        candidates.push(RotCandidate {
            description: format!(
                "Early context ({} messages) used tools not seen later: {}",
                early_cutoff,
                abandoned.join(", ")
            ),
            reason: "Tools/patterns from early session not referenced in later work".to_string(),
            message_range: (0, early_cutoff),
            estimated_tokens: early_tokens,
        });
    }

    candidates
}

/// Extract key insights from the session
fn extract_key_insights(messages: &[TranscriptMessage]) -> Vec<String> {
    let mut insights = Vec::new();
    let total_tokens: usize = messages.iter().map(|m| m.estimated_tokens).sum();
    let tool_count: usize = messages.iter().map(|m| m.tool_uses.len()).sum();
    let user_msgs = messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = messages.iter().filter(|m| m.role == "assistant").count();

    insights.push(format!(
        "{} messages ({} user, {} assistant), ~{} tokens, {} tool calls",
        messages.len(),
        user_msgs,
        assistant_msgs,
        total_tokens,
        tool_count
    ));

    // Tool distribution
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    for msg in messages {
        for tool in &msg.tool_uses {
            *tool_counts.entry(tool.name.clone()).or_default() += 1;
        }
    }
    let mut sorted_tools: Vec<_> = tool_counts.into_iter().collect();
    sorted_tools.sort_by(|a, b| b.1.cmp(&a.1));
    if !sorted_tools.is_empty() {
        let top_tools: Vec<String> = sorted_tools
            .iter()
            .take(5)
            .map(|(name, count)| format!("{}({})", name, count))
            .collect();
        insights.push(format!("Top tools: {}", top_tools.join(", ")));
    }

    insights
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn test_estimate_context_pct() {
        assert_eq!(estimate_context_pct(100_000, 200_000), 0.5);
        assert_eq!(estimate_context_pct(0, 200_000), 0.0);
        assert_eq!(estimate_context_pct(200_000, 200_000), 1.0);
        assert_eq!(estimate_context_pct(100, 0), 0.0);
    }

    #[test]
    fn test_parse_user_message() {
        let line = r#"{"type":"user","content":"Hello, how are you?"}"#;
        let msg = parse_transcript_entry(line).unwrap();
        assert_eq!(msg.role, "user");
        assert_eq!(msg.text_content, "Hello, how are you?");
        assert!(msg.tool_uses.is_empty());
    }

    #[test]
    fn test_parse_assistant_message_with_tool() {
        let line = r#"{"type":"assistant","content":[{"type":"text","text":"Let me read that file."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/test.rs"}}]}"#;
        let msg = parse_transcript_entry(line).unwrap();
        assert_eq!(msg.role, "assistant");
        assert!(msg.text_content.contains("Let me read"));
        assert_eq!(msg.tool_uses.len(), 1);
        assert_eq!(msg.tool_uses[0].name, "Read");
    }

    #[test]
    fn test_skip_non_message_entries() {
        let line = r#"{"type":"file-history-snapshot","messageId":"abc","snapshot":{}}"#;
        assert!(parse_transcript_entry(line).is_none());
    }

    #[test]
    fn test_analyze_session_with_fixture() {
        let dir = tempfile::TempDir::new().unwrap();
        let jsonl_path = dir.path().join("test-session.jsonl");
        let mut file = std::fs::File::create(&jsonl_path).unwrap();
        writeln!(file, r#"{{"type":"user","content":"Fix the auth bug"}}"#).unwrap();
        writeln!(file, r#"{{"type":"assistant","content":[{{"type":"text","text":"I decided to use JWT tokens for authentication. Let me read the file."}}]}}"#).unwrap();
        writeln!(file, r#"{{"type":"user","content":"Good, now test it"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","content":[{{"type":"text","text":"Running tests now."}}]}}"#
        )
        .unwrap();

        let config = Config::default();
        let analysis = analyze_session(&jsonl_path, "test-1", "proj-hash", &config).unwrap();

        assert_eq!(analysis.message_count, 4);
        assert_eq!(analysis.session_id, "test-1");
        assert!(analysis.estimated_tokens > 0);
        assert!(analysis.estimated_context_pct > 0.0);
        // Should find "decided to" decision
        assert!(!analysis.decisions.is_empty());
    }

    #[test]
    fn test_extract_decisions() {
        let messages = vec![
            TranscriptMessage {
                role: "assistant".to_string(),
                text_content: "I decided to use Rust for this project because of memory safety."
                    .to_string(),
                tool_uses: vec![],
                tool_results: vec![],
                char_count: 65,
                estimated_tokens: 16,
            },
            TranscriptMessage {
                role: "user".to_string(),
                text_content: "Sounds good".to_string(),
                tool_uses: vec![],
                tool_results: vec![],
                char_count: 11,
                estimated_tokens: 3,
            },
        ];

        let decisions = extract_decisions(&messages);
        assert!(!decisions.is_empty());
        assert!(decisions[0].description.contains("decided to"));
    }

    #[test]
    fn test_parse_assistant_message_with_stringified_tool_input() {
        let line = r#"{"type":"assistant","content":[{"type":"tool_use","id":"t1","name":"Write","input":"{\"file_path\":\"src/main.rs\",\"content\":\"fn main() {}\"}"}]}"#;
        let msg = parse_transcript_entry(line).unwrap();
        assert_eq!(msg.tool_uses.len(), 1);
        assert!(msg.tool_uses[0].input_preview.contains("src/main.rs"));
    }

    #[test]
    fn test_extract_files_touched_from_stringified_tool_input() {
        let messages = vec![TranscriptMessage {
            role: "assistant".to_string(),
            text_content: String::new(),
            tool_uses: vec![ToolUse {
                id: "t1".to_string(),
                name: "Write".to_string(),
                input_preview: r#"{"file_path":"src/main.rs","content":"fn main() {}"}"#
                    .to_string(),
                input_chars: 56,
            }],
            tool_results: vec![],
            char_count: 56,
            estimated_tokens: 14,
        }];

        let files = extract_files_touched(&messages);
        assert_eq!(files, vec!["src/main.rs".to_string()]);
    }

    #[test]
    fn test_quick_estimate() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.jsonl");
        // Write ~1000 bytes
        std::fs::write(&path, "x".repeat(1000)).unwrap();

        let (size, tokens, pct) = quick_estimate_from_file(&path, 200_000).unwrap();
        assert_eq!(size, 1000);
        assert!(tokens > 0);
        assert!(pct > 0.0);
        assert!(pct < 0.01); // 1000 bytes is tiny relative to 200K tokens
    }
}
