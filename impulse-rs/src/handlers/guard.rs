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
