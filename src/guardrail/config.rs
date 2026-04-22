use super::defaults::builtin_rules;
use super::types::{GuardConfig, GuardRule};

// ============================================================================
// Config-Based Rule Merging
// ============================================================================

/// Merges built-in default rules with user-configured rules from `GuardConfig`.
///
/// Merge strategy:
/// 1. If the config is disabled (`enabled == false`), returns an empty `Vec`.
/// 2. Starts with built-in defaults from `defaults::builtin_rules()`.
/// 3. For each built-in rule:
///    - If a user rule shares the same `id` and is enabled, the user version replaces it.
///    - If a user rule shares the same `id` but is disabled, the built-in is removed entirely.
/// 4. Any user rules whose `id` does not match a built-in are appended (custom rules).
/// 5. All disabled rules are filtered out of the final result.
pub fn merge_rules(config: &GuardConfig) -> Vec<GuardRule> {
    if !config.enabled {
        return Vec::new();
    }

    let builtins = builtin_rules();
    let builtin_ids: Vec<String> = builtins.iter().map(|r| r.id.clone()).collect();

    // Phase 1: Process built-in rules with user overrides
    let mut merged: Vec<GuardRule> = builtins
        .into_iter()
        .filter_map(|builtin| {
            // Check if the user has an override for this built-in rule
            if let Some(user_rule) = config.rules.iter().find(|r| r.id == builtin.id) {
                if user_rule.enabled {
                    Some(user_rule.clone())
                } else {
                    // User explicitly disabled this built-in
                    None
                }
            } else {
                // No user override — keep the built-in as-is
                Some(builtin)
            }
        })
        .collect();

    // Phase 2: Append user-defined custom rules (ids not matching any built-in)
    for user_rule in &config.rules {
        if !builtin_ids.contains(&user_rule.id) && user_rule.enabled {
            merged.push(user_rule.clone());
        }
    }

    // Phase 3: Final filter — remove any disabled rules
    merged.retain(|r| r.enabled);

    merged
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::defaults::builtin_rules;
    use crate::guardrail::types::{GuardAction, GuardConfig, GuardRule, GuardTarget};

    /// Helper to create a test rule with sensible defaults.
    fn test_rule(id: &str, action: GuardAction, enabled: bool) -> GuardRule {
        GuardRule {
            id: id.to_string(),
            pattern: r"test-pattern".to_string(),
            action,
            target: GuardTarget::Bash,
            reason: format!("Test rule: {}", id),
            suggestion: None,
            enabled,
            builtin: false,
        }
    }

    #[test]
    fn test_merge_no_user_rules_returns_defaults() {
        let config = GuardConfig {
            enabled: true,
            rules: Vec::new(),
        };

        let merged = merge_rules(&config);
        let defaults = builtin_rules();

        assert_eq!(
            merged.len(),
            defaults.len(),
            "With no user rules, merged should equal built-in defaults"
        );

        for (merged_rule, default_rule) in merged.iter().zip(defaults.iter()) {
            assert_eq!(merged_rule.id, default_rule.id);
            assert_eq!(merged_rule.action, default_rule.action);
            assert!(merged_rule.enabled);
        }
    }

    #[test]
    fn test_merge_user_override_replaces_builtin() {
        // Override the block-force-push-main rule from Block to Warn
        let mut override_rule = test_rule("block-force-push-main", GuardAction::Warn, true);
        override_rule.reason = "Downgraded to warn by user".to_string();

        let config = GuardConfig {
            enabled: true,
            rules: vec![override_rule],
        };

        let merged = merge_rules(&config);

        let force_push_rule = merged
            .iter()
            .find(|r| r.id == "block-force-push-main")
            .expect("block-force-push-main should still be in merged rules");

        assert_eq!(
            force_push_rule.action,
            GuardAction::Warn,
            "User override should have changed action from Block to Warn"
        );
        assert_eq!(force_push_rule.reason, "Downgraded to warn by user");
    }

    #[test]
    fn test_merge_user_adds_new_rule() {
        let custom_rule = test_rule("custom-no-sudo", GuardAction::Block, true);

        let config = GuardConfig {
            enabled: true,
            rules: vec![custom_rule],
        };

        let merged = merge_rules(&config);
        let defaults = builtin_rules();

        assert_eq!(
            merged.len(),
            defaults.len() + 1,
            "Custom rule should be appended to built-in defaults"
        );

        let found = merged
            .iter()
            .find(|r| r.id == "custom-no-sudo")
            .expect("Custom rule should be in merged result");
        assert_eq!(found.action, GuardAction::Block);
    }

    #[test]
    fn test_merge_disabled_config_returns_empty() {
        let config = GuardConfig {
            enabled: false,
            rules: vec![test_rule("some-rule", GuardAction::Block, true)],
        };

        let merged = merge_rules(&config);
        assert!(
            merged.is_empty(),
            "Disabled config should produce empty rule set"
        );
    }

    #[test]
    fn test_merge_user_disables_builtin() {
        // Disable the block-rm-rf-root built-in rule
        let disabled_rule = test_rule("block-rm-rf-root", GuardAction::Block, false);

        let config = GuardConfig {
            enabled: true,
            rules: vec![disabled_rule],
        };

        let merged = merge_rules(&config);
        let defaults = builtin_rules();

        assert_eq!(
            merged.len(),
            defaults.len() - 1,
            "Disabling a built-in should remove it from the merged result"
        );

        assert!(
            merged.iter().all(|r| r.id != "block-rm-rf-root"),
            "Disabled built-in rule should not appear in merged result"
        );
    }
}
