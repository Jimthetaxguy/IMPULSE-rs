pub mod config;
pub mod defaults;
pub mod engine;
pub mod types;

pub use config::merge_rules;
pub use engine::GuardEngine;
pub use types::{GuardAction, GuardConfig, GuardResult, GuardRule, GuardTarget};

/// Parse a target string into a GuardTarget.
///
/// Accepts canonical forms, hyphenated, underscored, and short aliases
/// so that hooks, CLI flags, and config files all resolve correctly.
pub fn parse_target(target: &str) -> GuardTarget {
    match target {
        "bash" | "shell" => GuardTarget::Bash,
        "tool-call" | "toolcall" | "tool_call" | "tool" => GuardTarget::ToolCall,
        "file-write" | "filewrite" | "file_write" | "file" => GuardTarget::FileWrite,
        "any" => GuardTarget::Any,
        _ => GuardTarget::Any,
    }
}

/// Evaluate an action against all active guardrail rules.
pub fn evaluate_action(
    action: &str,
    target: &str,
    config: &GuardConfig,
) -> Result<Vec<GuardResult>, String> {
    let rules = merge_rules(config);
    let engine = GuardEngine::new(&rules)?;
    let target = parse_target(target);
    Ok(engine.evaluate(action, &target))
}

/// List all active guardrail rules (built-in + user, after merge)
pub fn list_active_rules(config: &GuardConfig) -> Vec<GuardRule> {
    merge_rules(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_action_blocks_dangerous() {
        let config = GuardConfig::default();
        let results = evaluate_action("git push --force origin main", "bash", &config);
        assert!(results.is_ok());
        assert!(GuardEngine::has_blocking(&results.unwrap()));
    }

    #[test]
    fn test_evaluate_action_allows_safe() {
        let config = GuardConfig::default();
        let results = evaluate_action("git push origin main", "bash", &config).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_evaluate_action_unknown_target_defaults_to_any() {
        let config = GuardConfig::default();
        let results = evaluate_action("deploy production", "unknown-target", &config).unwrap();
        // "deploy" matches log-deploy-commands with target=Any (since unknown maps to Any)
        assert!(!results.is_empty());
    }

    #[test]
    fn test_parse_target_bash_aliases() {
        assert_eq!(parse_target("bash"), GuardTarget::Bash);
        assert_eq!(parse_target("shell"), GuardTarget::Bash);
    }

    #[test]
    fn test_parse_target_tool_call_aliases() {
        assert_eq!(parse_target("tool-call"), GuardTarget::ToolCall);
        assert_eq!(parse_target("toolcall"), GuardTarget::ToolCall);
        assert_eq!(parse_target("tool_call"), GuardTarget::ToolCall);
        assert_eq!(parse_target("tool"), GuardTarget::ToolCall);
    }

    #[test]
    fn test_parse_target_file_write_aliases() {
        assert_eq!(parse_target("file-write"), GuardTarget::FileWrite);
        assert_eq!(parse_target("filewrite"), GuardTarget::FileWrite);
        assert_eq!(parse_target("file_write"), GuardTarget::FileWrite);
        assert_eq!(parse_target("file"), GuardTarget::FileWrite);
    }

    #[test]
    fn test_parse_target_any_and_fallback() {
        assert_eq!(parse_target("any"), GuardTarget::Any);
        assert_eq!(parse_target("unknown-target"), GuardTarget::Any);
        assert_eq!(parse_target(""), GuardTarget::Any);
    }

    #[test]
    fn test_parse_target_file_alias_matches_filewrite_rules() {
        // This is the critical bug fix: hooks pass --target file, which must
        // resolve to FileWrite so that FileWrite-targeted rules match.
        let config = GuardConfig::default();
        let target = parse_target("file");
        assert_eq!(target, GuardTarget::FileWrite);

        // Verify FileWrite target does NOT match Bash-only rules
        let rules = list_active_rules(&config);
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git push --force", &target);
        // Force-push rule targets Bash, so it should not match FileWrite
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_active_rules() {
        let config = GuardConfig::default();
        let rules = list_active_rules(&config);
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r.enabled));
    }
}
