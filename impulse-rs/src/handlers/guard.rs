use anyhow::Result;
use std::sync::Arc;

use crate::{guardrail, state};

pub fn handle_guard(
    state: &Arc<state::State>,
    action: Option<String>,
    target: String,
    list: bool,
    enable: Option<String>,
    disable: Option<String>,
    json: bool,
) -> Result<()> {
    let config = state.config_snapshot()?;

    if list {
        let rules = guardrail::list_active_rules(&config.guardrails);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "rules": rules }))
                    .unwrap_or_else(|_| "{}".to_string())
            );
        } else if rules.is_empty() {
            println!("No active guardrail rules.");
        } else {
            println!("Active guardrail rules ({}):\n", rules.len());
            for rule in &rules {
                println!("{}\n", rule.format_human());
            }
        }
    } else if let Some(ref rule_id) = enable {
        let all_rules = guardrail::defaults::builtin_rules();
        let mut config = state.config_snapshot()?;
        let known = all_rules.iter().any(|r| r.id == *rule_id)
            || config.guardrails.rules.iter().any(|r| r.id == *rule_id);
        if !known {
            eprintln!(
                "Error: rule '{}' not found. Use --list to see available rules.",
                rule_id
            );
            std::process::exit(1);
        }
        config
            .guardrails
            .rules
            .retain(|r| r.id != *rule_id || r.enabled);
        state.update_guardrail_rules(config.guardrails.rules.clone())?;
        println!("Enabled rule: {}", rule_id);
    } else if let Some(ref rule_id) = disable {
        let all_rules = guardrail::defaults::builtin_rules();
        let mut config = state.config_snapshot()?;
        let known = all_rules.iter().any(|r| r.id == *rule_id)
            || config.guardrails.rules.iter().any(|r| r.id == *rule_id);
        if !known {
            eprintln!(
                "Error: rule '{}' not found. Use --list to see available rules.",
                rule_id
            );
            std::process::exit(1);
        }
        config.guardrails.rules.retain(|r| r.id != *rule_id);
        config.guardrails.rules.push(guardrail::GuardRule {
            id: rule_id.clone(),
            pattern: String::new(),
            action: guardrail::GuardAction::Log,
            target: guardrail::GuardTarget::Any,
            reason: "Disabled by user".to_string(),
            suggestion: None,
            enabled: false,
            builtin: false,
        });
        state.update_guardrail_rules(config.guardrails.rules.clone())?;
        println!("Disabled rule: {}", rule_id);
    } else if let Some(ref action_str) = action {
        match guardrail::evaluate_action(action_str, &target, &config.guardrails) {
            Ok(results) => {
                if json {
                    let has_block = guardrail::GuardEngine::has_blocking(&results);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "blocked": has_block,
                            "results": results,
                        }))
                        .unwrap_or_else(|_| "{}".to_string())
                    );
                    if has_block {
                        std::process::exit(1);
                    }
                } else if results.is_empty() {
                    eprintln!("PASS: No guardrail rules matched.");
                } else {
                    let has_block = guardrail::GuardEngine::has_blocking(&results);
                    for result in &results {
                        eprintln!("{}", result.format_human());
                    }
                    if has_block {
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Guardrail evaluation error: {}", e);
                std::process::exit(2);
            }
        }
    } else {
        println!("Usage:");
        println!("  impulse-rs guard --list                         List all active rules");
        println!("  impulse-rs guard --action \"<cmd>\" --target bash  Evaluate a command");
        println!("  impulse-rs guard --enable <rule-id>              Enable a rule");
        println!("  impulse-rs guard --disable <rule-id>             Disable a rule");
        println!("  impulse-rs guard --list --json                   List rules as JSON");
        println!("  impulse-rs guard --action \"<cmd>\" --json         Evaluate as JSON");
    }
    Ok(())
}

pub fn handle_analytics(
    state: &Arc<state::State>,
    subcommand: String,
    json: bool,
    period: String,
) -> Result<()> {
    if subcommand == "conflicts" {
        let history = state.get_conflict_analytics()?;
        let analytics = history.get_analytics();

        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&analytics).unwrap_or_else(|_| "{}".to_string())
            );
        } else {
            println!("\n=== Conflict Analytics ===\n");
            println!("Total Conflicts: {}", analytics.total_conflicts);
            println!(
                "Resolved: {} ({:.1}%)",
                analytics.resolved_count, analytics.resolution_rate
            );
            println!("Unresolved: {}", analytics.unresolved_count);
            println!(
                "Avg Time to Resolution: {}",
                analytics.format_time_to_resolution()
            );

            if !analytics.most_common_files.is_empty() {
                println!("\n--- Most Common Conflict Files ---");
                for (file, count) in analytics.most_common_files.iter().take(5) {
                    println!("  {} ({} times)", file, count);
                }
            }

            if !analytics.resolution_methods.is_empty() {
                println!("\n--- Resolution Methods ---");
                for (method, count) in &analytics.resolution_methods {
                    println!("  {}: {}", method, count);
                }
            }

            match period.as_str() {
                "day" => {
                    if !analytics.conflicts_by_day.is_empty() {
                        println!("\n--- Conflicts by Day ---");
                        let mut days: Vec<_> = analytics.conflicts_by_day.iter().collect();
                        days.sort_by(|a, b| a.0.cmp(b.0));
                        for (day, count) in days.iter().rev().take(7) {
                            println!("  {}: {}", day, count);
                        }
                    }
                }
                "week" => {
                    if !analytics.conflicts_by_week.is_empty() {
                        println!("\n--- Conflicts by Week ---");
                        let mut weeks: Vec<_> = analytics.conflicts_by_week.iter().collect();
                        weeks.sort_by(|a, b| a.0.cmp(b.0));
                        for (week, count) in weeks.iter().rev().take(8) {
                            println!("  {}: {}", week, count);
                        }
                    }
                }
                "month" => {
                    if !analytics.conflicts_by_month.is_empty() {
                        println!("\n--- Conflicts by Month ---");
                        let mut months: Vec<_> = analytics.conflicts_by_month.iter().collect();
                        months.sort_by(|a, b| a.0.cmp(b.0));
                        for (month, count) in months.iter().rev().take(6) {
                            println!("  {}: {}", month, count);
                        }
                    }
                }
                _ => {}
            }
        }
    } else {
        println!(
            "Unknown analytics type: {}. Available: conflicts",
            subcommand
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<state::State>) {
        let tmp = TempDir::new().unwrap();
        let st = state::State::new(tmp.path().to_path_buf()).unwrap();
        (tmp, Arc::new(st))
    }

    /// Evaluate a guard action and return results instead of printing/exiting.
    /// Testable extraction of the action evaluation branch of handle_guard.
    fn evaluate_guard_action(
        action_str: &str,
        target: &str,
        config: &crate::state::Config,
    ) -> Result<(Vec<guardrail::GuardResult>, bool)> {
        match guardrail::evaluate_action(action_str, target, &config.guardrails) {
            Ok(results) => {
                let has_block = guardrail::GuardEngine::has_blocking(&results);
                Ok((results, has_block))
            }
            Err(e) => Err(anyhow::anyhow!("Guardrail evaluation error: {}", e)),
        }
    }

    // ── handle_guard: list mode ────────────────────────────────────────

    #[test]
    fn test_handle_guard_list_returns_ok() {
        let (_tmp, st) = test_state();
        let result = handle_guard(&st, None, "any".to_string(), true, None, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_guard_list_json_returns_ok() {
        let (_tmp, st) = test_state();
        let result = handle_guard(&st, None, "any".to_string(), true, None, None, true);
        assert!(result.is_ok());
    }

    // ── handle_guard: usage (no flags) ─────────────────────────────────

    #[test]
    fn test_handle_guard_no_flags_shows_usage() {
        let (_tmp, st) = test_state();
        // No action, no list, no enable, no disable => usage branch
        let result = handle_guard(&st, None, "any".to_string(), false, None, None, false);
        assert!(result.is_ok());
    }

    // ── handle_guard: enable a known builtin rule ──────────────────────

    #[test]
    fn test_handle_guard_enable_known_rule_succeeds() {
        let (_tmp, st) = test_state();
        // "block-force-push-main" is a builtin rule
        let result = handle_guard(
            &st,
            None,
            "any".to_string(),
            false,
            Some("block-force-push-main".to_string()),
            None,
            false,
        );
        assert!(result.is_ok());
    }

    // ── handle_guard: disable a known builtin rule ─────────────────────

    #[test]
    fn test_handle_guard_disable_known_rule_succeeds() {
        let (_tmp, st) = test_state();
        let result = handle_guard(
            &st,
            None,
            "any".to_string(),
            false,
            None,
            Some("block-force-push-main".to_string()),
            false,
        );
        assert!(result.is_ok());

        // Verify the rule was persisted as disabled
        let config = st.config_snapshot().unwrap();
        let disabled = config
            .guardrails
            .rules
            .iter()
            .find(|r| r.id == "block-force-push-main");
        assert!(
            disabled.is_some(),
            "disabled override should exist in config"
        );
        assert!(!disabled.unwrap().enabled, "rule should be marked disabled");
    }

    // ── evaluate_guard_action (testable core) ──────────────────────────

    #[test]
    fn test_evaluate_guard_action_safe_command_passes() {
        let (_tmp, st) = test_state();
        let config = st.config_snapshot().unwrap();
        let (results, has_block) = evaluate_guard_action("git status", "bash", &config).unwrap();
        assert!(results.is_empty(), "safe command should produce no results");
        assert!(!has_block);
    }

    #[test]
    fn test_evaluate_guard_action_dangerous_command_blocks() {
        let (_tmp, st) = test_state();
        let config = st.config_snapshot().unwrap();
        let (results, has_block) =
            evaluate_guard_action("git push --force origin main", "bash", &config).unwrap();
        assert!(!results.is_empty(), "dangerous command should match rules");
        assert!(has_block, "force push should be blocked");
    }

    #[test]
    fn test_evaluate_guard_action_no_rules_match_different_target() {
        let (_tmp, st) = test_state();
        let config = st.config_snapshot().unwrap();
        // force push is a bash rule, evaluating against file-write target should not match
        let (results, has_block) =
            evaluate_guard_action("git push --force origin main", "file", &config).unwrap();
        assert!(
            results.is_empty(),
            "bash rule should not match file-write target"
        );
        assert!(!has_block);
    }

    // ── handle_analytics ───────────────────────────────────────────────

    #[test]
    fn test_handle_analytics_conflicts_returns_ok() {
        let (_tmp, st) = test_state();
        let result = handle_analytics(&st, "conflicts".to_string(), false, "day".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_analytics_conflicts_json_returns_ok() {
        let (_tmp, st) = test_state();
        let result = handle_analytics(&st, "conflicts".to_string(), true, "day".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_analytics_conflicts_week_period() {
        let (_tmp, st) = test_state();
        let result = handle_analytics(&st, "conflicts".to_string(), false, "week".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_analytics_conflicts_month_period() {
        let (_tmp, st) = test_state();
        let result = handle_analytics(&st, "conflicts".to_string(), false, "month".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_analytics_unknown_subcommand_returns_ok() {
        let (_tmp, st) = test_state();
        // Unknown subcommand prints a message but returns Ok
        let result = handle_analytics(&st, "unknown".to_string(), false, "day".to_string());
        assert!(result.is_ok());
    }

    // ── handle_analytics with recorded conflicts ───────────────────────

    #[test]
    fn test_handle_analytics_with_recorded_conflicts() {
        let (_tmp, st) = test_state();
        st.record_conflict(
            "src/main.rs",
            vec!["session-a".to_string(), "session-b".to_string()],
        )
        .unwrap();
        st.record_conflict_resolution("src/main.rs", "manual-merge")
            .unwrap();

        let result = handle_analytics(&st, "conflicts".to_string(), false, "day".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_analytics_with_conflicts_json_output() {
        let (_tmp, st) = test_state();
        st.record_conflict("src/lib.rs", vec!["s1".to_string()])
            .unwrap();

        let result = handle_analytics(&st, "conflicts".to_string(), true, "day".to_string());
        assert!(result.is_ok());
    }

    // ── handle_guard: list confirms rules are returned ─────────────────

    #[test]
    fn test_handle_guard_list_rules_nonempty() {
        let (_tmp, st) = test_state();
        let config = st.config_snapshot().unwrap();
        let rules = guardrail::list_active_rules(&config.guardrails);
        assert!(
            !rules.is_empty(),
            "default config should have builtin guardrail rules"
        );
        assert!(
            rules.iter().all(|r| r.enabled),
            "all listed rules should be enabled"
        );
    }

    // ── handle_guard: disable then re-enable round trip ────────────────

    #[test]
    fn test_handle_guard_disable_then_enable_round_trip() {
        let (_tmp, st) = test_state();
        let rule_id = "block-force-push-main";

        // Disable
        let result = handle_guard(
            &st,
            None,
            "any".to_string(),
            false,
            None,
            Some(rule_id.to_string()),
            false,
        );
        assert!(result.is_ok());

        // Verify disabled
        let config = st.config_snapshot().unwrap();
        let disabled = config.guardrails.rules.iter().find(|r| r.id == rule_id);
        assert!(disabled.is_some());
        assert!(!disabled.unwrap().enabled);

        // Re-enable
        let result = handle_guard(
            &st,
            None,
            "any".to_string(),
            false,
            Some(rule_id.to_string()),
            None,
            false,
        );
        assert!(result.is_ok());
    }
}
