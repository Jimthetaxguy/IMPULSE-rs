//! Delegation pattern detection in agent output.
//!
//! Scans terminal output for delegation markers:
//! - JSON code fences (```delegate { ... } ```) — OpenSquirrel pattern
//! - Natural language delegation signals

use super::types::DelegationSpec;

/// Scan text for a ```delegate JSON code block and parse it.
pub fn detect_delegation(text: &str) -> Option<DelegationSpec> {
    let mut in_delegate_block = false;
    let mut block_content = String::new();

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```delegate") {
            in_delegate_block = true;
            block_content.clear();
            continue;
        }

        if in_delegate_block {
            if trimmed == "```" {
                // End of delegate block — try parsing the JSON
                return serde_json::from_str::<DelegationSpec>(&block_content).ok();
            }
            block_content.push_str(trimmed);
            block_content.push('\n');
        }
    }

    None
}

/// Detect natural language delegation signals in a single line.
/// Returns the task description if a delegation pattern is found.
pub fn detect_delegation_natural(line: &str) -> Option<String> {
    let lower = line.to_lowercase();

    let patterns = [
        "i'll delegate",
        "delegating to",
        "hand off to",
        "handing off to",
        "spawning worker",
        "spawn a worker",
        "assigning to worker",
    ];

    for pattern in &patterns {
        if lower.contains(pattern) {
            return Some(line.trim().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_delegation_json_block() {
        let text = r#"Working on the task...
```delegate
{
    "task": "refactor auth module",
    "target_files": ["src/auth.rs"],
    "constraints": "Use zero-trust principles"
}
```
Continuing..."#;

        let spec = detect_delegation(text).unwrap();
        assert_eq!(spec.task, "refactor auth module");
        assert_eq!(spec.target_files, vec!["src/auth.rs"]);
        assert_eq!(
            spec.constraints.as_deref(),
            Some("Use zero-trust principles")
        );
    }

    #[test]
    fn test_detect_delegation_minimal() {
        let text = "```delegate\n{\"task\": \"fix bugs\"}\n```";
        let spec = detect_delegation(text).unwrap();
        assert_eq!(spec.task, "fix bugs");
        assert!(spec.target_files.is_empty());
    }

    #[test]
    fn test_detect_delegation_none() {
        let text = "Just some normal output\nNo delegation here";
        assert!(detect_delegation(text).is_none());
    }

    #[test]
    fn test_detect_delegation_malformed() {
        let text = "```delegate\nnot valid json\n```";
        assert!(detect_delegation(text).is_none());
    }

    #[test]
    fn test_detect_delegation_unclosed() {
        let text = "```delegate\n{\"task\": \"never closed\"}";
        assert!(detect_delegation(text).is_none());
    }

    #[test]
    fn test_detect_delegation_natural_patterns() {
        assert!(detect_delegation_natural("I'll delegate this to a worker").is_some());
        assert!(detect_delegation_natural("Delegating to worker-2").is_some());
        assert!(detect_delegation_natural("Let me hand off to another agent").is_some());
        assert!(detect_delegation_natural("Spawning worker for auth refactor").is_some());
    }

    #[test]
    fn test_detect_delegation_natural_no_match() {
        assert!(detect_delegation_natural("Just working on the code").is_none());
        assert!(detect_delegation_natural("This is a regular line").is_none());
    }
}
