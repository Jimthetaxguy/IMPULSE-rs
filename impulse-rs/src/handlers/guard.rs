use anyhow::Result;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;

use crate::{guardrail, state, storage};

/// Largest PreToolUse payload the hook reads. Write payloads carry the whole
/// file, so this is generous; a larger payload blocks the call.
const MAX_HOOK_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;

/// How long the hook waits for Claude Code to finish writing stdin.
const HOOK_PAYLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Claude Code blocks a PreToolUse call only on exit code 2 (stderr goes to
/// the model); any other non-zero code is a non-blocking error.
const HOOK_BLOCK: i32 = 2;

/// The outcome of guarding one Claude Code tool call.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HookDecision {
    /// 0 lets the call run; [`HOOK_BLOCK`] stops it.
    pub exit_code: i32,
    /// Written to stderr: the reason when blocking, a note when warning.
    pub message: Option<String>,
}

impl HookDecision {
    fn allow(message: Option<String>) -> Self {
        Self {
            exit_code: 0,
            message,
        }
    }

    fn block(message: String) -> Self {
        Self {
            exit_code: HOOK_BLOCK,
            message: Some(message),
        }
    }
}

/// Decides one PreToolUse call from Claude Code's stdin payload
/// (`{"tool_name": ..., "tool_input": {...}}`). Bash commands are checked
/// against `bash` rules; Write, Edit, and MultiEdit content against
/// `file-write` rules. Other tools pass. The guard fails closed: a payload
/// it cannot read, or rules it cannot evaluate, block the call.
pub(crate) fn decide_pre_tool_use(payload: &str, guards: &guardrail::GuardConfig) -> HookDecision {
    let call: serde_json::Value = match serde_json::from_str(payload) {
        Ok(call) => call,
        Err(error) => {
            return HookDecision::block(format!(
                "Impulse guard could not read the PreToolUse payload ({error}); the call is blocked"
            ))
        }
    };
    let input = &call["tool_input"];
    let text = |field: &str| input[field].as_str().unwrap_or_default().to_string();
    let (target, action) = match call["tool_name"].as_str().unwrap_or_default() {
        "Bash" => ("bash", text("command")),
        "Write" => ("file", text("content")),
        "Edit" => ("file", text("new_string")),
        "MultiEdit" => (
            "file",
            input["edits"]
                .as_array()
                .map(|edits| {
                    edits
                        .iter()
                        .filter_map(|edit| edit["new_string"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
        ),
        _ => return HookDecision::allow(None),
    };
    match guardrail::evaluate_action(&action, target, guards) {
        Err(error) => HookDecision::block(format!(
            "Impulse guard could not evaluate its rules ({error}); the call is blocked"
        )),
        Ok(results) if results.is_empty() => HookDecision::allow(None),
        Ok(results) => {
            let report = results
                .iter()
                .map(|result| result.format_human())
                .collect::<Vec<_>>()
                .join("\n");
            if guardrail::GuardEngine::has_blocking(&results) {
                HookDecision::block(format!("Blocked by the Impulse guard:\n{report}"))
            } else {
                HookDecision::allow(Some(report))
            }
        }
    }
}

/// Runs `impulse-rs guard --hook` and exits. It reads only `config.json`'s
/// guardrail section (not the governed ledgers `State::new` loads), so an
/// unrelated state problem cannot turn into an exit code Claude Code treats
/// as "allow".
pub fn run_guard_hook(impulse_dir: &Path) -> ! {
    let decision = guard_hook_decision(impulse_dir);
    if let Some(message) = &decision.message {
        eprintln!("{message}");
    }
    std::process::exit(decision.exit_code)
}

fn guard_hook_decision(impulse_dir: &Path) -> HookDecision {
    let config = match storage::Storage::new(impulse_dir.to_path_buf())
        .read_json::<state::Config>("config.json")
    {
        Ok(config) => config,
        Err(error) => {
            return HookDecision::block(format!(
                "Impulse guard could not load {}/config.json ({error:#}); the call is blocked",
                impulse_dir.display()
            ))
        }
    };
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return HookDecision::block(
            "impulse-rs guard --hook reads a Claude Code PreToolUse payload from stdin".to_string(),
        );
    }
    match super::common::read_payload_with_deadline(
        stdin,
        HOOK_PAYLOAD_TIMEOUT,
        MAX_HOOK_PAYLOAD_BYTES,
    ) {
        Some(payload) => decide_pre_tool_use(&payload, &config.guardrails),
        None => HookDecision::block(
            "Impulse guard received no PreToolUse payload on stdin; the call is blocked"
                .to_string(),
        ),
    }
}

/// Load the current guardrail config and confirm `rule_id` is known (either
/// a builtin rule or an already-configured rule). Exits the process with
/// code 1 and prints the standard "not found" error if the rule is unknown.
fn resolve_known_rule_id(state: &Arc<state::State>, rule_id: &str) -> Result<state::Config> {
    let all_rules = guardrail::defaults::builtin_rules();
    let config = state.config_snapshot()?;
    let known = all_rules.iter().any(|r| r.id == *rule_id)
        || config.guardrails.rules.iter().any(|r| r.id == *rule_id);
    if !known {
        eprintln!(
            "Error: rule '{}' not found. Use --list to see available rules.",
            rule_id
        );
        std::process::exit(1);
    }
    Ok(config)
}

/// Enables or disables `rule_id` in the configured rules. A built-in rule is
/// disabled by a configured override entry with its id and removed again to
/// enable it. A user-defined rule is toggled in place: replacing it with an
/// override entry used to discard its pattern, so disabling and then
/// enabling it deleted the rule.
fn set_rule_enabled(rules: &mut Vec<guardrail::GuardRule>, rule_id: &str, enabled: bool) {
    let builtin = guardrail::defaults::builtin_rules()
        .iter()
        .any(|rule| rule.id == rule_id);
    if !builtin {
        for rule in rules.iter_mut().filter(|rule| rule.id == rule_id) {
            rule.enabled = enabled;
        }
        return;
    }
    rules.retain(|rule| rule.id != rule_id);
    if !enabled {
        rules.push(guardrail::GuardRule {
            id: rule_id.to_string(),
            pattern: String::new(),
            action: guardrail::GuardAction::Log,
            target: guardrail::GuardTarget::Any,
            reason: "Disabled by user".to_string(),
            suggestion: None,
            enabled: false,
            builtin: false,
        });
    }
}

/// Handle the `guard` command.
///
/// Evaluates an action against guardrail rules, lists active rules, or
/// enables/disables a specific rule by ID. Exits with code 1 when a
/// blocking rule matches, or code 2 on evaluation error.
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
        let mut config = resolve_known_rule_id(state, rule_id)?;
        set_rule_enabled(&mut config.guardrails.rules, rule_id, true);
        state.update_guardrail_rules(config.guardrails.rules.clone())?;
        println!("Enabled rule: {}", rule_id);
    } else if let Some(ref rule_id) = disable {
        let mut config = resolve_known_rule_id(state, rule_id)?;
        set_rule_enabled(&mut config.guardrails.rules, rule_id, false);
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

/// Handle the `analytics` command.
///
/// Currently supports the `conflicts` subcommand, which displays conflict
/// analytics (totals, resolution rates, common files) grouped by the
/// specified period (day, week, or month).
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
                "month" if !analytics.conflicts_by_month.is_empty() => {
                    println!("\n--- Conflicts by Month ---");
                    let mut months: Vec<_> = analytics.conflicts_by_month.iter().collect();
                    months.sort_by(|a, b| a.0.cmp(b.0));
                    for (month, count) in months.iter().rev().take(6) {
                        println!("  {}: {}", month, count);
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

    // ── enable / disable ──────────────────────────────────────────────────

    fn custom_rule() -> guardrail::GuardRule {
        guardrail::GuardRule {
            id: "no-sudo".to_string(),
            pattern: r"\bsudo\b".to_string(),
            action: guardrail::GuardAction::Block,
            target: guardrail::GuardTarget::Bash,
            reason: "no sudo here".to_string(),
            suggestion: None,
            enabled: true,
            builtin: false,
        }
    }

    fn blocks(rules: &[guardrail::GuardRule], action: &str) -> bool {
        let config = guardrail::GuardConfig {
            rules: rules.to_vec(),
            ..guardrail::GuardConfig::default()
        };
        guardrail::GuardEngine::has_blocking(
            &guardrail::evaluate_action(action, "bash", &config).unwrap(),
        )
    }

    /// Review P2: disabling and re-enabling a user rule deleted it.
    #[test]
    fn test_disable_then_enable_keeps_a_custom_rule() {
        let mut rules = vec![custom_rule()];
        set_rule_enabled(&mut rules, "no-sudo", false);
        assert!(!blocks(&rules, "sudo rm x"));
        set_rule_enabled(&mut rules, "no-sudo", true);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, custom_rule().pattern);
        assert!(blocks(&rules, "sudo rm x"));
    }

    #[test]
    fn test_disable_then_enable_a_builtin_rule() {
        let force_push = "git push --force origin main";
        let mut rules = Vec::new();
        assert!(blocks(&rules, force_push));
        set_rule_enabled(&mut rules, "block-force-push-main", false);
        assert!(!blocks(&rules, force_push));
        set_rule_enabled(&mut rules, "block-force-push-main", true);
        assert!(rules.is_empty());
        assert!(blocks(&rules, force_push));
    }

    // ── PreToolUse hook decisions ─────────────────────────────────────────

    fn hook(payload: serde_json::Value) -> HookDecision {
        decide_pre_tool_use(&payload.to_string(), &guardrail::GuardConfig::default())
    }

    /// Review P1: a Block used to exit 1, which Claude Code treats as a
    /// non-blocking error, so the guard never stopped a call.
    #[test]
    fn test_hook_blocks_a_force_push_with_exit_two() {
        let decision = hook(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git push --force origin main"}
        }));
        assert_eq!(decision.exit_code, 2);
        assert!(decision
            .message
            .unwrap()
            .contains("Blocked by the Impulse guard"));
    }

    #[test]
    fn test_hook_allows_a_benign_command_silently() {
        let decision = hook(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test --workspace"}
        }));
        assert_eq!(decision, HookDecision::allow(None));
    }

    #[test]
    fn test_hook_warns_without_blocking() {
        let decision = hook(serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git add .env"}
        }));
        assert_eq!(decision.exit_code, 0);
        assert!(decision.message.is_some());
    }

    #[test]
    fn test_hook_checks_written_and_edited_content() {
        let secret = "api_key = 'sk_live_0123456789abcdefghij'";
        for payload in [
            serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "a.rs", "content": secret}}),
            serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "a.rs", "old_string": "x", "new_string": secret}}),
            serde_json::json!({"tool_name": "MultiEdit", "tool_input": {"file_path": "a.rs", "edits": [
                {"old_string": "a", "new_string": "fine"},
                {"old_string": "b", "new_string": secret}
            ]}}),
        ] {
            assert_eq!(hook(payload.clone()).exit_code, 2, "{payload}");
        }
    }

    #[test]
    fn test_hook_passes_tools_it_does_not_guard() {
        let decision = hook(serde_json::json!({
            "tool_name": "Read",
            "tool_input": {"file_path": "README.md"}
        }));
        assert_eq!(decision, HookDecision::allow(None));
    }

    #[test]
    fn test_hook_fails_closed_on_an_unreadable_payload() {
        let decision = decide_pre_tool_use("not json", &guardrail::GuardConfig::default());
        assert_eq!(decision.exit_code, 2);
        assert!(decision.message.unwrap().contains("could not read"));
    }

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
