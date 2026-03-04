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

    #[test]
    fn test_rule_add_creates_new_rule() {
        let new_rule = GuardRule {
            id: "test-add-rule".to_string(),
            pattern: r"dangerous-command".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Test rule for add".to_string(),
            suggestion: None,
            enabled: true,
            builtin: false,
        };

        let mut config = GuardConfig::default();
        config.rules.push(new_rule.clone());

        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].id, "test-add-rule");
    }

    #[test]
    fn test_rule_remove_filters_correctly() {
        let custom_rule = GuardRule {
            id: "test-remove-rule".to_string(),
            pattern: r"test-pattern".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Test rule for remove".to_string(),
            suggestion: None,
            enabled: true,
            builtin: false,
        };

        let mut config = GuardConfig::default();
        config.rules.push(custom_rule);

        config.rules.retain(|r| r.id != "test-remove-rule");

        assert!(config.rules.iter().all(|r| r.id != "test-remove-rule"));
    }

    #[test]
    fn test_rule_enable_removes_disabled_override() {
        let disabled_override = GuardRule {
            id: "block-force-push".to_string(),
            pattern: String::new(),
            action: GuardAction::Log,
            target: GuardTarget::Any,
            reason: "Disabled by user".to_string(),
            suggestion: None,
            enabled: false,
            builtin: false,
        };

        let mut config = GuardConfig::default();
        config.rules.push(disabled_override);

        config
            .rules
            .retain(|r| r.id != "block-force-push" || r.enabled);

        assert!(config
            .rules
            .iter()
            .all(|r| r.id != "block-force-push" || r.enabled));
    }

    #[test]
    fn test_rule_disable_creates_disabled_override() {
        let builtin_rule = GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: r"git\s+push\s+(.*\s+)?(-f|--force)\s+(.*\s+)?(origin\s+)?main\b".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Test".to_string(),
            suggestion: None,
            enabled: true,
            builtin: true,
        };

        let mut config = GuardConfig::default();
        config.rules.push(builtin_rule);

        let initial_len = config.rules.len();

        config.rules.retain(|r| r.id != "block-force-push-main");
        config.rules.push(GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: String::new(),
            action: GuardAction::Log,
            target: GuardTarget::Any,
            reason: "Disabled by user".to_string(),
            suggestion: None,
            enabled: false,
            builtin: false,
        });

        assert_eq!(config.rules.len(), initial_len);
        assert!(config
            .rules
            .iter()
            .any(|r| r.id == "block-force-push-main" && !r.enabled));
    }

    #[test]
    fn test_cannot_remove_builtin_rules() {
        let builtins = defaults::builtin_rules();
        assert!(!builtins.is_empty());

        let builtin_ids: Vec<&str> = builtins.iter().map(|r| r.id.as_str()).collect();

        assert!(builtin_ids.contains(&"block-force-push-main"));
        assert!(builtin_ids.contains(&"block-rm-rf-root"));
    }
}
