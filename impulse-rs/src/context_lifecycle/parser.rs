//! Structured output parser — classifies agent PTY output lines.
//!
//! Replaces brittle string-prefix matching with schema-driven line classification.
//! Inspired by OpenSquirrel's line classification approach, adapted for IMPULSE's
//! multi-agent sidecar context.

use super::types::AgentKind;
use impulse_ops::DiffSummary;

/// Classification of a single line of agent output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineClassification {
    /// Diff content: additions, deletions, hunk headers, or diff headers.
    Diff(DiffKind),
    /// Code fence (opening or closing).
    CodeFence { lang: Option<String>, opening: bool },
    /// Markdown heading.
    Heading { level: u8 },
    /// Bulleted list item.
    Bullet { indent: usize },
    /// Thinking/reasoning block marker.
    ThinkingBlock { opening: bool },
    /// System or framework message (not agent output).
    SystemMessage,
    /// Error line detected.
    ErrorLine,
    /// Tool invocation detected.
    ToolInvocation { kind: ToolKind, target: String },
    /// Delegation marker (```delegate block).
    DelegationMarker,
    /// Unclassified text.
    PlainText,
}

/// Kind of diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Addition,
    Deletion,
    Hunk,
    Header,
}

/// Kind of tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolKind {
    Edit,
    Read,
    Write,
    Bash,
    Other(String),
}

/// Parsed output with aggregated statistics.
#[derive(Debug, Clone, Default)]
pub struct ParsedOutput {
    pub lines: Vec<LineClassification>,
    pub tool_invocations: Vec<(ToolKind, String)>,
    pub diff_summary: DiffSummary,
    pub error_count: usize,
    pub delegation_detected: bool,
}

/// Known file extensions for path validation.
const KNOWN_EXTENSIONS: &[&str] = &[
    ".rs",
    ".py",
    ".js",
    ".ts",
    ".tsx",
    ".jsx",
    ".go",
    ".java",
    ".c",
    ".cpp",
    ".h",
    ".hpp",
    ".rb",
    ".php",
    ".swift",
    ".kt",
    ".scala",
    ".sh",
    ".bash",
    ".zsh",
    ".fish",
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".xml",
    ".html",
    ".css",
    ".scss",
    ".md",
    ".txt",
    ".cfg",
    ".ini",
    ".lock",
    ".sql",
    ".graphql",
    ".proto",
    ".dockerfile",
    ".makefile",
    ".cmake",
    ".zig",
    ".nim",
    ".lua",
    ".ex",
    ".exs",
    ".erl",
    ".hrl",
    ".clj",
    ".cljs",
    ".vue",
    ".svelte",
];

/// Classify a single line of agent output.
pub fn classify_line(line: &str, _agent_kind: AgentKind) -> LineClassification {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return LineClassification::PlainText;
    }

    // Delegation markers (```delegate)
    if trimmed.starts_with("```delegate") {
        return LineClassification::DelegationMarker;
    }

    // Code fences
    if trimmed.starts_with("```") {
        let rest = trimmed.trim_start_matches('`');
        let opening = !trimmed.eq("```") || !rest.is_empty();
        let lang = if opening && !rest.is_empty() {
            Some(rest.split_whitespace().next().unwrap_or("").to_string())
        } else {
            None
        };
        // "```" alone is a closing fence; "```rust" is opening
        return if trimmed == "```" {
            LineClassification::CodeFence {
                lang: None,
                opening: false,
            }
        } else {
            LineClassification::CodeFence {
                lang,
                opening: true,
            }
        };
    }

    // Thinking blocks
    if trimmed.starts_with("<thinking>") || trimmed.starts_with("<THINKING>") {
        return LineClassification::ThinkingBlock { opening: true };
    }
    if trimmed.starts_with("</thinking>") || trimmed.starts_with("</THINKING>") {
        return LineClassification::ThinkingBlock { opening: false };
    }

    // Tool invocations — agent-agnostic patterns (before diff, to catch Write/Edit)
    if let Some((kind, target)) = detect_tool_invocation(trimmed) {
        return LineClassification::ToolInvocation { kind, target };
    }

    // Headings
    if trimmed.starts_with('#') {
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if level <= 6 && trimmed.chars().nth(level) == Some(' ') {
            return LineClassification::Heading { level: level as u8 };
        }
    }

    // Bullets (before diff — "- item" is a bullet, not a diff deletion)
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("• ") {
        let indent = line.len() - line.trim_start().len();
        return LineClassification::Bullet { indent };
    }

    // Numbered list items
    if trimmed.len() > 2 {
        let first_char = trimmed.chars().next().unwrap_or(' ');
        if first_char.is_ascii_digit() && trimmed.contains(". ") {
            let indent = line.len() - line.trim_start().len();
            return LineClassification::Bullet { indent };
        }
    }

    // Diff patterns (after bullets to avoid "- item" being classified as deletion)
    if let Some(kind) = classify_diff_line(trimmed) {
        return LineClassification::Diff(kind);
    }

    // System messages
    if trimmed.starts_with("[system]")
        || trimmed.starts_with("<system")
        || trimmed.starts_with("╭─")
        || trimmed.starts_with("╰─")
    {
        return LineClassification::SystemMessage;
    }

    // Error detection (contextual — avoid false positives inside prose)
    if is_error_line(trimmed) {
        return LineClassification::ErrorLine;
    }

    LineClassification::PlainText
}

/// Parse multi-line output with state tracking for code fences.
pub fn parse_output(text: &str, agent_kind: AgentKind) -> ParsedOutput {
    let mut output = ParsedOutput::default();
    let mut in_code_fence = false;
    let mut diff_files_seen = std::collections::HashSet::new();

    for line in text.lines() {
        let classification = if in_code_fence {
            let trimmed = line.trim();
            // Check for closing fence
            if trimmed == "```" {
                in_code_fence = false;
                LineClassification::CodeFence {
                    lang: None,
                    opening: false,
                }
            } else if let Some(kind) = classify_diff_line(trimmed) {
                // Track diffs inside code fences too
                match kind {
                    DiffKind::Addition => output.diff_summary.lines_added += 1,
                    DiffKind::Deletion => output.diff_summary.lines_removed += 1,
                    DiffKind::Header => {
                        // Extract file path from diff header
                        if let Some(path) = extract_diff_file_path(trimmed) {
                            diff_files_seen.insert(path);
                        }
                    }
                    DiffKind::Hunk => {}
                }
                LineClassification::Diff(kind)
            } else {
                LineClassification::PlainText
            }
        } else {
            let c = classify_line(line, agent_kind);
            match &c {
                LineClassification::CodeFence { opening: true, .. } => {
                    in_code_fence = true;
                }
                LineClassification::ToolInvocation { kind, target } => {
                    output.tool_invocations.push((kind.clone(), target.clone()));
                }
                LineClassification::ErrorLine => {
                    output.error_count += 1;
                }
                LineClassification::DelegationMarker => {
                    output.delegation_detected = true;
                }
                LineClassification::Diff(kind) => match kind {
                    DiffKind::Addition => output.diff_summary.lines_added += 1,
                    DiffKind::Deletion => output.diff_summary.lines_removed += 1,
                    DiffKind::Header => {
                        if let Some(path) = extract_diff_file_path(line.trim()) {
                            diff_files_seen.insert(path);
                        }
                    }
                    DiffKind::Hunk => {}
                },
                _ => {}
            }
            c
        };
        output.lines.push(classification);
    }

    output.diff_summary.files_changed = diff_files_seen.len();
    output
}

/// Summarize diffs in text.
pub fn summarize_diffs(text: &str, agent_kind: AgentKind) -> DiffSummary {
    parse_output(text, agent_kind).diff_summary
}

// --- Internal helpers ---

fn classify_diff_line(line: &str) -> Option<DiffKind> {
    if line.starts_with("diff --git") || line.starts_with("--- ") || line.starts_with("+++ ") {
        Some(DiffKind::Header)
    } else if line.starts_with("@@ ") && line.contains("@@") {
        Some(DiffKind::Hunk)
    } else if line.starts_with('+') && !line.starts_with("+++") {
        Some(DiffKind::Addition)
    } else if line.starts_with('-') && !line.starts_with("---") {
        Some(DiffKind::Deletion)
    } else {
        None
    }
}

fn extract_diff_file_path(line: &str) -> Option<String> {
    // "diff --git a/path b/path" or "+++ b/path" or "--- a/path"
    if let Some(rest) = line.strip_prefix("diff --git ") {
        return rest
            .split_whitespace()
            .nth(1)
            .map(|p| p.strip_prefix("b/").unwrap_or(p).to_string());
    }
    if let Some(rest) = line.strip_prefix("+++ b/") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("--- a/") {
        return Some(rest.trim().to_string());
    }
    None
}

fn detect_tool_invocation(line: &str) -> Option<(ToolKind, String)> {
    // Claude Code patterns: "Write(path)", "Edit(path)", "Read(path)", "Bash(cmd)"
    for (prefix, kind) in &[
        ("Write(", ToolKind::Write),
        ("Edit(", ToolKind::Edit),
        ("Read(", ToolKind::Read),
        ("Bash(", ToolKind::Bash),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            if let Some(target) = rest.strip_suffix(')') {
                return Some((kind.clone(), target.to_string()));
            }
        }
    }

    // "Created file: path" pattern
    if let Some(rest) = line.strip_prefix("Created file: ") {
        let path = rest.trim();
        if is_likely_file_path(path) {
            return Some((ToolKind::Write, path.to_string()));
        }
    }

    // Natural-language file-operation announcements emitted by Codex, Gemini,
    // Cursor and OpenCode (Claude uses the parenthesized form above). Matched
    // case-insensitively at line start; the path is sliced from the ORIGINAL
    // line so its case is preserved on case-sensitive filesystems. The
    // `is_likely_file_path` guard keeps prose like "reading the docs" from
    // being misread as a tool call.
    for (prefix, kind) in &[
        ("wrote ", ToolKind::Write),
        ("writing ", ToolKind::Write),
        ("created ", ToolKind::Write),
        ("creating ", ToolKind::Write),
        ("modified ", ToolKind::Edit),
        ("modifying ", ToolKind::Edit),
        ("editing ", ToolKind::Edit),
        ("edited ", ToolKind::Edit),
        ("updated ", ToolKind::Edit),
        ("updating ", ToolKind::Edit),
        // Codex/Gemini apply-patch flows announce edits as patches.
        ("patched ", ToolKind::Edit),
        ("patching ", ToolKind::Edit),
        ("applied patch to ", ToolKind::Edit),
        ("reading ", ToolKind::Read),
    ] {
        // `get(..)` returns None unless `prefix.len()` is a char boundary, so
        // the subsequent slice can never panic on multibyte input.
        if let Some(head) = line.get(..prefix.len()) {
            if head.eq_ignore_ascii_case(prefix) {
                let path = line[prefix.len()..].trim();
                if is_likely_file_path(path) {
                    return Some((kind.clone(), path.to_string()));
                }
            }
        }
    }

    None
}

/// Check if a string looks like a file path (has known extension or clear path structure).
fn is_likely_file_path(s: &str) -> bool {
    if s.is_empty() || s.contains(' ') {
        return false;
    }
    // Check for known file extensions
    for ext in KNOWN_EXTENSIONS {
        if s.ends_with(ext) {
            return true;
        }
    }
    // Check for path-like structure: must contain / with at least one segment
    // that has a file-like name (contains a dot or common dir prefix)
    if s.contains('/') && !s.starts_with("http") && !s.starts_with("//") {
        // Reject patterns like "RFC/2045" (short segments, no file-like parts)
        let segments: Vec<&str> = s.split('/').collect();
        return segments.iter().any(|seg| seg.contains('.'))
            || segments.len() > 2
            || segments.first().is_some_and(|f| {
                f.eq_ignore_ascii_case("src")
                    || f.eq_ignore_ascii_case("lib")
                    || f.eq_ignore_ascii_case("test")
                    || f.eq_ignore_ascii_case("tests")
                    || f.eq_ignore_ascii_case("bin")
                    || f.eq_ignore_ascii_case("pkg")
                    || f.starts_with('.')
            });
    }
    false
}

/// Detect error lines with reduced false positives.
/// Only matches lines that start with error indicators or contain strong error signals.
fn is_error_line(line: &str) -> bool {
    let lower = line.to_lowercase();

    // Strong prefix indicators
    if lower.starts_with("error:")
        || lower.starts_with("error[")
        || lower.starts_with("fatal:")
        || lower.starts_with("panic:")
        || lower.starts_with("thread '") && lower.contains("panicked")
    {
        return true;
    }

    // Contextual: "failed" at word boundary, not inside identifiers
    if lower.contains(" failed") && !lower.contains("failed=") && !lower.contains("_failed") {
        // Additional check: line should be short-ish (not prose)
        if line.len() < 200 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_tool_invocation_claude() {
        assert_eq!(
            classify_line("Write(src/main.rs)", AgentKind::ClaudeCode),
            LineClassification::ToolInvocation {
                kind: ToolKind::Write,
                target: "src/main.rs".to_string(),
            }
        );
        assert_eq!(
            classify_line("Edit(src/lib.rs)", AgentKind::ClaudeCode),
            LineClassification::ToolInvocation {
                kind: ToolKind::Edit,
                target: "src/lib.rs".to_string(),
            }
        );
        assert_eq!(
            classify_line("Read(Cargo.toml)", AgentKind::ClaudeCode),
            LineClassification::ToolInvocation {
                kind: ToolKind::Read,
                target: "Cargo.toml".to_string(),
            }
        );
    }

    #[test]
    fn test_classify_tool_invocation_opencode() {
        assert_eq!(
            classify_line("wrote src/handler.rs", AgentKind::OpenCode),
            LineClassification::ToolInvocation {
                kind: ToolKind::Write,
                target: "src/handler.rs".to_string(),
            }
        );
    }

    #[test]
    fn test_classify_tool_invocation_natural_language_verbs() {
        // Present-tense and additional verbs used by Codex/Gemini/Cursor.
        let cases = [
            ("Editing src/main.rs", ToolKind::Edit, "src/main.rs"),
            ("Creating src/new.rs", ToolKind::Write, "src/new.rs"),
            ("Updating src/lib.rs", ToolKind::Edit, "src/lib.rs"),
            ("Reading Cargo.toml", ToolKind::Read, "Cargo.toml"),
            ("patched src/auth/mod.rs", ToolKind::Edit, "src/auth/mod.rs"),
            (
                "Applied patch to src/parser.rs",
                ToolKind::Edit,
                "src/parser.rs",
            ),
        ];
        for (line, kind, target) in cases {
            assert_eq!(
                classify_line(line, AgentKind::Codex),
                LineClassification::ToolInvocation {
                    kind: kind.clone(),
                    target: target.to_string(),
                },
                "line: {line}"
            );
        }
    }

    #[test]
    fn test_tool_invocation_preserves_path_case() {
        // The path must retain its original case (previously lowercased).
        assert_eq!(
            classify_line("Writing src/MyModule.rs", AgentKind::Gemini),
            LineClassification::ToolInvocation {
                kind: ToolKind::Write,
                target: "src/MyModule.rs".to_string(),
            }
        );
    }

    #[test]
    fn test_tool_invocation_rejects_prose() {
        // Verb prefixes followed by non-paths must not be read as tool calls.
        for line in [
            "reading the documentation now",
            "editing in progress",
            "created a new plan for the refactor",
        ] {
            assert!(
                !matches!(
                    classify_line(line, AgentKind::Codex),
                    LineClassification::ToolInvocation { .. }
                ),
                "prose misread as tool call: {line}"
            );
        }
    }

    #[test]
    fn test_classify_diff_lines() {
        // Additions: +code (no space after +, not a bullet)
        assert_eq!(
            classify_line("+let x = 42;", AgentKind::ClaudeCode),
            LineClassification::Diff(DiffKind::Addition)
        );
        // Deletions: -code (no space after -, not a bullet)
        assert_eq!(
            classify_line("-let x = 41;", AgentKind::ClaudeCode),
            LineClassification::Diff(DiffKind::Deletion)
        );
        assert_eq!(
            classify_line("@@ -1,3 +1,4 @@", AgentKind::ClaudeCode),
            LineClassification::Diff(DiffKind::Hunk)
        );
        assert_eq!(
            classify_line(
                "diff --git a/src/main.rs b/src/main.rs",
                AgentKind::ClaudeCode
            ),
            LineClassification::Diff(DiffKind::Header)
        );
        // "- item" is a bullet, not a diff deletion
        assert_eq!(
            classify_line("- item", AgentKind::ClaudeCode),
            LineClassification::Bullet { indent: 0 }
        );
    }

    #[test]
    fn test_classify_code_fence() {
        assert_eq!(
            classify_line("```rust", AgentKind::ClaudeCode),
            LineClassification::CodeFence {
                lang: Some("rust".to_string()),
                opening: true,
            }
        );
        assert_eq!(
            classify_line("```", AgentKind::ClaudeCode),
            LineClassification::CodeFence {
                lang: None,
                opening: false,
            }
        );
    }

    #[test]
    fn test_classify_heading() {
        assert_eq!(
            classify_line("## Design", AgentKind::ClaudeCode),
            LineClassification::Heading { level: 2 }
        );
    }

    #[test]
    fn test_classify_bullet() {
        assert_eq!(
            classify_line("- item", AgentKind::ClaudeCode),
            LineClassification::Bullet { indent: 0 }
        );
        assert_eq!(
            classify_line("  - nested", AgentKind::ClaudeCode),
            LineClassification::Bullet { indent: 2 }
        );
    }

    #[test]
    fn test_classify_error() {
        assert_eq!(
            classify_line("error: cannot find value `x`", AgentKind::ClaudeCode),
            LineClassification::ErrorLine
        );
        assert_eq!(
            classify_line("error[E0425]: cannot find value", AgentKind::ClaudeCode),
            LineClassification::ErrorLine
        );
    }

    #[test]
    fn test_classify_error_no_false_positive() {
        // "failed" inside identifiers shouldn't trigger
        assert_eq!(
            classify_line("assertion_failed=false", AgentKind::ClaudeCode),
            LineClassification::PlainText
        );
    }

    #[test]
    fn test_classify_delegation_marker() {
        assert_eq!(
            classify_line("```delegate", AgentKind::ClaudeCode),
            LineClassification::DelegationMarker
        );
    }

    #[test]
    fn test_classify_thinking_block() {
        assert_eq!(
            classify_line("<thinking>", AgentKind::ClaudeCode),
            LineClassification::ThinkingBlock { opening: true }
        );
        assert_eq!(
            classify_line("</thinking>", AgentKind::ClaudeCode),
            LineClassification::ThinkingBlock { opening: false }
        );
    }

    #[test]
    fn test_classify_system_message() {
        assert_eq!(
            classify_line("[system] context loaded", AgentKind::ClaudeCode),
            LineClassification::SystemMessage
        );
    }

    #[test]
    fn test_parse_output_diff_summary() {
        let text = "diff --git a/src/main.rs b/src/main.rs\n\
                     @@ -1,3 +1,4 @@\n\
                     +let x = 42;\n\
                     +let y = 43;\n\
                     -let z = 44;\n\
                     Some other text";
        let output = parse_output(text, AgentKind::ClaudeCode);
        assert_eq!(output.diff_summary.files_changed, 1);
        assert_eq!(output.diff_summary.lines_added, 2);
        assert_eq!(output.diff_summary.lines_removed, 1);
    }

    #[test]
    fn test_parse_output_tool_invocations() {
        let text = "Write(src/main.rs)\nRead(Cargo.toml)\nSome text";
        let output = parse_output(text, AgentKind::ClaudeCode);
        assert_eq!(output.tool_invocations.len(), 2);
        assert_eq!(output.tool_invocations[0].0, ToolKind::Write);
        assert_eq!(output.tool_invocations[0].1, "src/main.rs");
    }

    #[test]
    fn test_parse_output_delegation() {
        let text = "Working on the task...\n```delegate\n{\"task\": \"refactor\"}\n```";
        let output = parse_output(text, AgentKind::ClaudeCode);
        assert!(output.delegation_detected);
    }

    #[test]
    fn test_is_likely_file_path() {
        assert!(is_likely_file_path("src/main.rs"));
        assert!(is_likely_file_path("Cargo.toml"));
        assert!(is_likely_file_path("src/lib.rs"));
        assert!(!is_likely_file_path("hello world"));
        assert!(!is_likely_file_path("https://example.com"));
        assert!(!is_likely_file_path(""));
        assert!(!is_likely_file_path("RFC/2045"));
    }

    #[test]
    fn test_parse_output_code_fence_suppresses_classification() {
        // Inside a code fence, lines shouldn't be classified as errors
        let text = "```python\nerror: this is code not a real error\n```";
        let output = parse_output(text, AgentKind::ClaudeCode);
        // The error line inside the fence should be PlainText, not ErrorLine
        assert_eq!(output.error_count, 0);
    }

    #[test]
    fn test_summarize_diffs() {
        let text = "+line1\n+line2\n-old_line\ndiff --git a/foo.rs b/foo.rs";
        let summary = summarize_diffs(text, AgentKind::ClaudeCode);
        assert_eq!(summary.lines_added, 2);
        assert_eq!(summary.lines_removed, 1);
        assert_eq!(summary.files_changed, 1);
    }

    #[test]
    fn test_created_file_pattern() {
        assert_eq!(
            classify_line("Created file: src/new.rs", AgentKind::ClaudeCode),
            LineClassification::ToolInvocation {
                kind: ToolKind::Write,
                target: "src/new.rs".to_string(),
            }
        );
    }

    #[test]
    fn test_empty_and_whitespace() {
        assert_eq!(
            classify_line("", AgentKind::ClaudeCode),
            LineClassification::PlainText
        );
        assert_eq!(
            classify_line("   ", AgentKind::ClaudeCode),
            LineClassification::PlainText
        );
    }
}
