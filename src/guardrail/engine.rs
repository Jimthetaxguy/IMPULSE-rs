use std::sync::RwLock;

use chrono::{DateTime, Utc};

use regex::Regex;

use super::{GuardAction, GuardResult, GuardRule, GuardTarget};

// ============================================================================
// Audit Trail
// ============================================================================

/// A single guardrail evaluation entry for audit/debugging purposes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub input: String,
    pub target: GuardTarget,
    pub verdict: Vec<GuardResult>,
    pub blocked: bool,
}

impl Default for AuditEntry {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            input: String::new(),
            target: GuardTarget::Any,
            verdict: Vec::new(),
            blocked: false,
        }
    }
}

// ============================================================================
// Compiled Rule (private)
// ============================================================================

/// A guardrail rule with its regex pre-compiled for efficient repeated matching.
#[derive(Debug)]
struct CompiledRule {
    rule: GuardRule,
    regex: Regex,
}

// ============================================================================
// Guard Engine
// ============================================================================

/// Regex-based pattern matching engine for guardrail evaluation.
///
/// Compiles rule patterns once at construction, then evaluates inputs
/// against all compiled rules. Results are sorted by severity:
/// Block first, then Warn, then Log.
///
/// The audit trail records every evaluation and can be retrieved for
/// debugging or security audit purposes.
#[derive(Debug)]
pub struct GuardEngine {
    rules: Vec<CompiledRule>,
    audit: RwLock<Vec<AuditEntry>>,
}

// Send+Sync are auto-derived: Vec<CompiledRule> (Regex + plain data) and
// RwLock<Vec<AuditEntry>> are both Send+Sync without asserting it.

impl GuardEngine {
    /// Create a new engine from a slice of rules.
    ///
    /// Only enabled rules are compiled. Returns an error if any enabled
    /// rule has an invalid regex pattern.
    pub fn new(rules: &[GuardRule]) -> Result<Self, String> {
        let mut compiled = Vec::new();

        for rule in rules {
            if !rule.enabled {
                continue;
            }

            let regex = Regex::new(&rule.pattern).map_err(|e| {
                format!(
                    "Invalid regex in rule '{}': pattern '{}' — {}",
                    rule.id, rule.pattern, e
                )
            })?;

            compiled.push(CompiledRule {
                rule: rule.clone(),
                regex,
            });
        }

        Ok(Self {
            rules: compiled,
            audit: RwLock::new(Vec::new()),
        })
    }

    /// Evaluate an input string against all compiled rules for the given target.
    ///
    /// Rules whose target does not match the provided target are skipped.
    /// Results are sorted by severity: Block, then Warn, then Log.
    pub fn evaluate(&self, input: &str, target: &GuardTarget) -> Vec<GuardResult> {
        let mut results = Vec::new();

        for compiled in &self.rules {
            // Skip rules that don't apply to this target
            if !compiled.rule.target.matches(target) {
                continue;
            }

            if compiled.regex.is_match(input) {
                results.push(GuardResult {
                    rule_id: compiled.rule.id.clone(),
                    action: compiled.rule.action,
                    matched_input: input.to_string(),
                    reason: compiled.rule.reason.clone(),
                    suggestion: compiled.rule.suggestion.clone(),
                });
            }
        }

        // Sort by severity: Block (0) < Warn (1) < Log (2)
        results.sort_by_key(|r| match r.action {
            GuardAction::Block => 0,
            GuardAction::Warn => 1,
            GuardAction::Log => 2,
        });

        // Record in audit trail
        let entry = AuditEntry {
            timestamp: Utc::now(),
            input: input.to_string(),
            target: *target,
            verdict: results.clone(),
            blocked: Self::has_blocking(&results),
        };
        if let Ok(mut audit) = self.audit.write() {
            audit.push(entry);
        }

        results
    }

    /// Returns true if any result in the slice is a Block action.
    pub fn has_blocking(results: &[GuardResult]) -> bool {
        results.iter().any(|r| r.action == GuardAction::Block)
    }

    /// Returns a snapshot of all audit entries recorded so far.
    pub fn audit_entries(&self) -> Vec<AuditEntry> {
        self.audit
            .read()
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }

    /// Clears the audit trail.
    #[allow(dead_code)]
    pub fn clear_audit(&self) {
        if let Ok(mut entries) = self.audit.write() {
            entries.clear();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test rule with sensible defaults.
    fn test_rule(id: &str, pattern: &str, action: GuardAction, target: GuardTarget) -> GuardRule {
        GuardRule {
            id: id.to_string(),
            pattern: pattern.to_string(),
            action,
            target,
            reason: format!("Test rule: {}", id),
            suggestion: None,
            enabled: true,
            builtin: false,
        }
    }

    #[test]
    fn test_engine_blocks_force_push() {
        let rules = vec![test_rule(
            "no-force-push",
            r"git\s+push\s+--force",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git push --force origin main", &GuardTarget::Bash);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "no-force-push");
        assert_eq!(results[0].action, GuardAction::Block);
    }

    #[test]
    fn test_engine_allows_normal_push() {
        let rules = vec![test_rule(
            "no-force-push",
            r"git\s+push\s+--force",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git push origin main", &GuardTarget::Bash);

        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_target_mismatch_skips_rule() {
        let rules = vec![test_rule(
            "bash-only",
            r"rm\s+-rf",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        // Evaluate against FileWrite target — the Bash rule should be skipped
        let results = engine.evaluate("rm -rf /", &GuardTarget::FileWrite);

        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_any_target_matches_all() {
        let rules = vec![test_rule(
            "catch-all",
            r"dangerous",
            GuardAction::Warn,
            GuardTarget::Any,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("this is dangerous", &GuardTarget::Bash);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "catch-all");
    }

    #[test]
    fn test_engine_disabled_rule_skipped() {
        let mut rule = test_rule(
            "disabled-rule",
            r"anything",
            GuardAction::Block,
            GuardTarget::Any,
        );
        rule.enabled = false;

        let engine = GuardEngine::new(&[rule]).unwrap();
        let results = engine.evaluate("anything goes", &GuardTarget::Bash);

        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_block_before_warn() {
        let rules = vec![
            test_rule("warn-rule", r"deploy", GuardAction::Warn, GuardTarget::Bash),
            test_rule(
                "block-rule",
                r"deploy",
                GuardAction::Block,
                GuardTarget::Bash,
            ),
        ];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("deploy to production", &GuardTarget::Bash);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].action, GuardAction::Block);
        assert_eq!(results[1].action, GuardAction::Warn);
    }

    #[test]
    fn test_engine_invalid_regex_returns_error() {
        let rules = vec![test_rule(
            "bad-regex",
            r"[invalid(",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let result = GuardEngine::new(&rules);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("bad-regex"), "Error should mention rule ID");
        assert!(
            err.contains("[invalid("),
            "Error should mention the pattern"
        );
    }

    #[test]
    fn test_engine_multiple_matches() {
        let rules = vec![
            test_rule(
                "rm-block",
                r"rm\s+-rf",
                GuardAction::Block,
                GuardTarget::Bash,
            ),
            test_rule("sudo-warn", r"sudo", GuardAction::Warn, GuardTarget::Bash),
        ];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("sudo rm -rf /tmp/data", &GuardTarget::Bash);

        assert_eq!(results.len(), 2);
        // Both rules matched
        let rule_ids: Vec<&str> = results.iter().map(|r| r.rule_id.as_str()).collect();
        assert!(rule_ids.contains(&"rm-block"));
        assert!(rule_ids.contains(&"sudo-warn"));
    }

    #[test]
    fn test_engine_case_insensitive_sql() {
        let rules = vec![test_rule(
            "no-drop-table",
            r"(?i)DROP\s+TABLE",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("drop table users;", &GuardTarget::Bash);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rule_id, "no-drop-table");
    }

    #[test]
    fn test_has_blocking_result() {
        let warn_only = vec![GuardResult {
            rule_id: "warn-rule".to_string(),
            action: GuardAction::Warn,
            matched_input: "test".to_string(),
            reason: "Just a warning".to_string(),
            suggestion: None,
        }];
        assert!(!GuardEngine::has_blocking(&warn_only));

        let with_block = vec![
            GuardResult {
                rule_id: "warn-rule".to_string(),
                action: GuardAction::Warn,
                matched_input: "test".to_string(),
                reason: "Just a warning".to_string(),
                suggestion: None,
            },
            GuardResult {
                rule_id: "block-rule".to_string(),
                action: GuardAction::Block,
                matched_input: "test".to_string(),
                reason: "Blocked".to_string(),
                suggestion: None,
            },
        ];
        assert!(GuardEngine::has_blocking(&with_block));

        // Empty results should not be blocking
        assert!(!GuardEngine::has_blocking(&[]));
    }

    // ── Audit trail tests ──────────────────────────────────────────────────

    #[test]
    fn test_engine_audit_trail_records_evaluation() {
        let rules = vec![test_rule(
            "block-rm",
            r"rm\s+-rf",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();

        // Initially empty
        assert!(engine.audit_entries().is_empty());

        // Evaluate — should record
        let results = engine.evaluate("rm -rf /", &GuardTarget::Bash);
        assert!(!results.is_empty());

        let entries = engine.audit_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input, "rm -rf /");
        assert!(entries[0].blocked);
    }

    #[test]
    fn test_engine_audit_trail_multiple_evaluations() {
        let rules = vec![
            test_rule(
                "block-rm",
                r"rm\s+-rf",
                GuardAction::Block,
                GuardTarget::Bash,
            ),
            test_rule("warn-sudo", r"sudo", GuardAction::Warn, GuardTarget::Bash),
        ];
        let engine = GuardEngine::new(&rules).unwrap();

        engine.evaluate("rm -rf /tmp", &GuardTarget::Bash);
        engine.evaluate("sudo ls", &GuardTarget::Bash);
        engine.evaluate("ls /tmp", &GuardTarget::Bash); // no match

        let entries = engine.audit_entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].input, "rm -rf /tmp");
        assert!(entries[0].blocked);
        assert_eq!(entries[1].input, "sudo ls");
        assert!(!entries[1].blocked);
        assert_eq!(entries[2].input, "ls /tmp");
        assert!(!entries[2].blocked);
    }

    #[test]
    fn test_engine_audit_trail_clear() {
        let rules = vec![test_rule(
            "block-rm",
            r"rm\s+-rf",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();

        engine.evaluate("rm -rf /", &GuardTarget::Bash);
        assert_eq!(engine.audit_entries().len(), 1);

        engine.clear_audit();
        assert!(engine.audit_entries().is_empty());
    }

    // ── Send+Sync trait bounds tests ───────────────────────────────────────

    #[test]
    fn test_guard_engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<GuardEngine>();
    }

    // ── AuditEntry serde round-trip ────────────────────────────────────────

    #[test]
    fn test_audit_entry_roundtrip() {
        let original = AuditEntry {
            timestamp: Utc::now(),
            input: "rm -rf /".to_string(),
            target: GuardTarget::Bash,
            verdict: vec![GuardResult {
                rule_id: "block-rm".to_string(),
                action: GuardAction::Block,
                matched_input: "rm -rf /".to_string(),
                reason: "Dangerous".to_string(),
                suggestion: None,
            }],
            blocked: true,
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.input, original.input);
        assert_eq!(recovered.blocked, original.blocked);
    }
}
