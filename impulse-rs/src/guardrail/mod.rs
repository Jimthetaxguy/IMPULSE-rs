pub mod config;
pub mod defaults;
pub mod engine;
pub mod types;

pub use config::merge_rules;
pub use engine::GuardEngine;
pub use types::{GuardAction, GuardConfig, GuardResult, GuardRule, GuardTarget};

/// Parse a target string into a GuardTarget
fn parse_target(target: &str) -> GuardTarget {
    match target {
        "bash" => GuardTarget::Bash,
        "tool-call" | "toolcall" => GuardTarget::ToolCall,
        "file-write" | "filewrite" => GuardTarget::FileWrite,
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
    fn test_list_active_rules() {
        let config = GuardConfig::default();
        let rules = list_active_rules(&config);
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r.enabled));
    }
}
