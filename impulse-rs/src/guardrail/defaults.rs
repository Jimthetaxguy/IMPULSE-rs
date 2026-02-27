use super::types::{GuardAction, GuardRule, GuardTarget};

// ============================================================================
// Built-in Default Rules
// ============================================================================

/// Returns the set of built-in guardrail rules that ship with Impulse.
///
/// These rules provide safety defaults for common dangerous operations:
/// - 4 Block rules: force-push main, bulk git add, rm -rf root, SQL DROP
/// - 4 Warn rules: binary staging, artifact staging, .env staging, chmod 777
/// - 1 Log rule: deploy/publish/release commands
///
/// All rules target Bash commands, are enabled by default, and marked as builtin.
pub fn builtin_rules() -> Vec<GuardRule> {
    vec![
        // ==================================================================
        // Block rules
        // ==================================================================
        GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: r"git\s+push\s+(.*\s+)?(-f|--force)\s+(.*\s+)?(origin\s+)?main\b".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Force-pushing to main rewrites shared history and can cause data loss \
                     for all collaborators."
                .to_string(),
            suggestion: Some(
                "Push to a feature branch and open a pull request instead.".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-bulk-git-add".to_string(),
            pattern: r"git\s+add\s+(-A|--all|\.\s*$)".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Bulk git add stages everything including secrets, binaries, and \
                     build artifacts."
                .to_string(),
            suggestion: Some(
                "Stage specific files by name: git add src/main.rs src/lib.rs".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-rm-rf-root".to_string(),
            pattern: r"rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?-?[a-zA-Z]*r[a-zA-Z]*\s+[/~]".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Recursive forced deletion of root or home directory is catastrophic \
                     and irreversible."
                .to_string(),
            suggestion: Some(
                "Target a specific subdirectory: rm -rf ./build/ or rm -rf target/".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-drop-table".to_string(),
            pattern: r"(?i)(DROP\s+TABLE|DROP\s+DATABASE)".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "DROP TABLE/DATABASE permanently destroys data with no undo.".to_string(),
            suggestion: Some(
                "Use a migration tool with rollback support, or back up first.".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        // ==================================================================
        // Warn rules
        // ==================================================================
        GuardRule {
            id: "warn-binary-staging".to_string(),
            pattern: r"git\s+add\s+.*\.(zip|pdf|exe|dll|dmg|iso|tar\.gz|tgz|jar|war|wasm)\b"
                .to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Binary files inflate repository size and cannot be meaningfully diffed."
                .to_string(),
            suggestion: Some(
                "Use Git LFS for large binaries, or add them to .gitignore.".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-artifact-staging".to_string(),
            pattern:
                r"git\s+add\s+.*(node_modules|\.venv|__pycache__|\.next|dist/|\.pnpm-store|target/)"
                    .to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Build artifacts and dependency directories should not be committed."
                .to_string(),
            suggestion: Some("Add these paths to .gitignore instead of staging them.".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-env-file-staging".to_string(),
            pattern: r"git\s+add\s+.*\.env\b".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Environment files often contain secrets, API keys, and credentials."
                .to_string(),
            suggestion: Some(
                "Add .env to .gitignore and use .env.example for templates.".to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-chmod-777".to_string(),
            pattern: r"chmod\s+(-R\s+)?777".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "chmod 777 grants read/write/execute to all users, which is a \
                     security risk."
                .to_string(),
            suggestion: Some(
                "Use more restrictive permissions: chmod 755 for dirs, chmod 644 for files."
                    .to_string(),
            ),
            enabled: true,
            builtin: true,
        },
        // ==================================================================
        // Log rules
        // ==================================================================
        GuardRule {
            id: "log-deploy-commands".to_string(),
            pattern: r"\b(deploy|publish|release)\b".to_string(),
            action: GuardAction::Log,
            target: GuardTarget::Bash,
            reason: "Deploy, publish, and release commands are logged for audit trails."
                .to_string(),
            suggestion: None,
            enabled: true,
            builtin: true,
        },
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::guardrail::engine::GuardEngine;

    #[test]
    fn test_default_rules_all_valid_regex() {
        let rules = builtin_rules();
        let result = GuardEngine::new(&rules);
        assert!(
            result.is_ok(),
            "All built-in rule patterns must be valid regex: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_default_rules_have_unique_ids() {
        let rules = builtin_rules();
        let mut seen = HashSet::new();
        for rule in &rules {
            assert!(
                seen.insert(&rule.id),
                "Duplicate rule ID found: {}",
                rule.id
            );
        }
    }

    #[test]
    fn test_default_rules_all_enabled() {
        let rules = builtin_rules();
        assert_eq!(rules.len(), 9, "Expected exactly 9 built-in rules");
        for rule in &rules {
            assert!(rule.enabled, "Rule '{}' should be enabled", rule.id);
            assert!(rule.builtin, "Rule '{}' should be marked builtin", rule.id);
        }
    }

    #[test]
    fn test_blocks_force_push_main() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        // Should block
        let results = engine.evaluate("git push --force origin main", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: git push --force origin main"
        );

        let results = engine.evaluate("git push -f origin main", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: git push -f origin main"
        );

        // Should allow
        let results = engine.evaluate("git push origin main", &GuardTarget::Bash);
        assert!(
            !GuardEngine::has_blocking(&results),
            "Should allow normal push to main"
        );

        let results = engine.evaluate("git push --force origin feature-branch", &GuardTarget::Bash);
        assert!(
            !GuardEngine::has_blocking(&results),
            "Should allow force push to feature branch"
        );
    }

    #[test]
    fn test_blocks_bulk_git_add() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        // Should block
        let results = engine.evaluate("git add -A", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: git add -A"
        );

        let results = engine.evaluate("git add --all", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: git add --all"
        );

        let results = engine.evaluate("git add .", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: git add ."
        );

        // Should allow
        let results = engine.evaluate("git add src/main.rs", &GuardTarget::Bash);
        assert!(
            !GuardEngine::has_blocking(&results),
            "Should allow adding specific files"
        );
    }

    #[test]
    fn test_blocks_rm_rf_root() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        // Should block
        let results = engine.evaluate("rm -rf /", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: rm -rf /"
        );

        let results = engine.evaluate("rm -rf ~/", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: rm -rf ~/"
        );

        // Should allow
        let results = engine.evaluate("rm -rf target/", &GuardTarget::Bash);
        assert!(
            !GuardEngine::has_blocking(&results),
            "Should allow: rm -rf target/"
        );
    }

    #[test]
    fn test_blocks_drop_table() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        // Should block (case-insensitive)
        let results = engine.evaluate("DROP TABLE users;", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: DROP TABLE users;"
        );

        let results = engine.evaluate("drop database production;", &GuardTarget::Bash);
        assert!(
            GuardEngine::has_blocking(&results),
            "Should block: drop database production;"
        );
    }

    #[test]
    fn test_warns_binary_staging() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        let results = engine.evaluate("git add release.zip", &GuardTarget::Bash);
        assert!(!results.is_empty(), "Should match: git add release.zip");
        assert!(
            !GuardEngine::has_blocking(&results),
            "Binary staging should warn, not block"
        );
        assert_eq!(results[0].action, GuardAction::Warn);
        assert_eq!(results[0].rule_id, "warn-binary-staging");
    }

    #[test]
    fn test_warns_artifact_staging() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        let results = engine.evaluate("git add node_modules/", &GuardTarget::Bash);
        assert!(!results.is_empty(), "Should match: git add node_modules/");
        assert!(
            !GuardEngine::has_blocking(&results),
            "Artifact staging should warn, not block"
        );
        assert_eq!(results[0].action, GuardAction::Warn);
        assert_eq!(results[0].rule_id, "warn-artifact-staging");
    }

    #[test]
    fn test_warns_env_file() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();

        let results = engine.evaluate("git add .env", &GuardTarget::Bash);
        assert!(!results.is_empty(), "Should match: git add .env");
        assert!(
            !GuardEngine::has_blocking(&results),
            ".env staging should warn, not block"
        );
        assert_eq!(results[0].action, GuardAction::Warn);
        assert_eq!(results[0].rule_id, "warn-env-file-staging");
    }
}
