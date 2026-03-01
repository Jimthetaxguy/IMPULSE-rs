# Guardrail Module Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a built-in guardrail engine to Impulse that blocks dangerous agent operations before they execute and warns on risky ones after observation.

**Architecture:** New `src/guardrail/` module with types, regex engine, compiled-in defaults, and config.json integration. Pre-execution gating via new `guard` CLI command (exit code 1 = blocked). Post-observation warnings via enhanced `track-tool` handler. Daemon IPC via new `GuardEvaluate` request variant with direct-mode fallback.

**Tech Stack:** Rust, regex crate (already a dependency), serde/serde_json, clap (existing CLI framework)

---

### Task 1: Define guardrail types

**Files:**
- Create: `src/guardrail/types.rs`
- Create: `src/guardrail/mod.rs`
- Modify: `src/main.rs:6-37` (add `pub mod guardrail;`)

**Step 1: Write the failing test**

In `src/guardrail/types.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard_action_serde_roundtrip() {
        let actions = vec![GuardAction::Block, GuardAction::Warn, GuardAction::Log];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let parsed: GuardAction = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, action);
        }
    }

    #[test]
    fn test_guard_target_serde_roundtrip() {
        let targets = vec![
            GuardTarget::Bash,
            GuardTarget::ToolCall,
            GuardTarget::FileWrite,
            GuardTarget::Any,
        ];
        for target in targets {
            let json = serde_json::to_string(&target).unwrap();
            let parsed: GuardTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, target);
        }
    }

    #[test]
    fn test_guard_rule_serde() {
        let rule = GuardRule {
            id: "test-rule".to_string(),
            pattern: r"rm\s+-rf".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Dangerous".to_string(),
            suggestion: Some("Use trash instead".to_string()),
            enabled: true,
            builtin: false,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: GuardRule = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-rule");
        assert_eq!(parsed.action, GuardAction::Block);
        assert_eq!(parsed.target, GuardTarget::Bash);
    }

    #[test]
    fn test_guard_action_display() {
        assert_eq!(GuardAction::Block.as_str(), "block");
        assert_eq!(GuardAction::Warn.as_str(), "warn");
        assert_eq!(GuardAction::Log.as_str(), "log");
    }

    #[test]
    fn test_guard_target_display() {
        assert_eq!(GuardTarget::Bash.as_str(), "bash");
        assert_eq!(GuardTarget::Any.as_str(), "any");
    }

    #[test]
    fn test_guard_config_default() {
        let config = GuardConfig::default();
        assert!(config.enabled);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_guard_result_blocked() {
        let result = GuardResult {
            rule_id: "test".to_string(),
            action: GuardAction::Block,
            matched_input: "git push --force main".to_string(),
            reason: "no".to_string(),
            suggestion: None,
        };
        assert!(result.is_blocked());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd impulse-rs && cargo test guardrail -v 2>&1 | head -20`
Expected: Compilation errors — types don't exist yet.

**Step 3: Write minimal implementation**

Create `src/guardrail/types.rs`:

```rust
use serde::{Deserialize, Serialize};

// ============================================================================
// Guard Action
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardAction {
    /// Prevent execution, return non-zero exit
    Block,
    /// Allow but log warning and notify
    Warn,
    /// Silent log for audit trail
    Log,
}

impl GuardAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Warn => "warn",
            Self::Log => "log",
        }
    }
}

impl std::fmt::Display for GuardAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ============================================================================
// Guard Target
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardTarget {
    /// Shell commands (git, rm, etc.)
    Bash,
    /// Agent tool invocations (Write, Edit, etc.)
    ToolCall,
    /// File system writes
    FileWrite,
    /// Match against all targets
    Any,
}

impl GuardTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::ToolCall => "tool-call",
            Self::FileWrite => "file-write",
            Self::Any => "any",
        }
    }

    /// Check if this target matches a given target kind
    pub fn matches(&self, other: &GuardTarget) -> bool {
        *self == GuardTarget::Any || *other == GuardTarget::Any || *self == *other
    }
}

impl std::fmt::Display for GuardTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ============================================================================
// Guard Rule
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardRule {
    /// Unique rule identifier (e.g., "block-force-push-main")
    pub id: String,
    /// Regex pattern to match against the action input
    pub pattern: String,
    /// What to do when matched: Block, Warn, or Log
    pub action: GuardAction,
    /// What kind of action this rule applies to
    pub target: GuardTarget,
    /// Human-readable explanation of why this rule exists
    pub reason: String,
    /// What the agent should do instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Whether this rule is active
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether this is a built-in rule (vs user-defined)
    #[serde(default)]
    pub builtin: bool,
}

fn default_true() -> bool {
    true
}

// ============================================================================
// Guard Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardConfig {
    /// Master switch for guardrails
    pub enabled: bool,
    /// User-defined rules (merged with built-in defaults)
    pub rules: Vec<GuardRule>,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: Vec::new(),
        }
    }
}

// ============================================================================
// Guard Result
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardResult {
    /// Which rule matched
    pub rule_id: String,
    /// What the rule says to do
    pub action: GuardAction,
    /// The input that matched the pattern
    pub matched_input: String,
    /// Why this was flagged
    pub reason: String,
    /// What to do instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl GuardResult {
    pub fn is_blocked(&self) -> bool {
        self.action == GuardAction::Block
    }
}
```

Create `src/guardrail/mod.rs`:

```rust
pub mod types;

pub use types::{GuardAction, GuardConfig, GuardResult, GuardRule, GuardTarget};
```

Add `pub mod guardrail;` to `src/main.rs` at line 37 (after `pub mod verify;`).

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test guardrail -- --nocapture`
Expected: All 7 tests PASS.

**Step 5: Commit**

```bash
git add src/guardrail/types.rs src/guardrail/mod.rs src/main.rs
git commit -m "feat(guardrail): add types module with GuardRule, GuardAction, GuardTarget, GuardResult"
```

---

### Task 2: Build the pattern matching engine

**Files:**
- Create: `src/guardrail/engine.rs`
- Modify: `src/guardrail/mod.rs` (add `pub mod engine;`)

**Step 1: Write the failing test**

In `src/guardrail/engine.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::types::*;

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
            "block-force-push",
            r"git\s+push\s+.*--force.*\s+(origin\s+)?main",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git push --force origin main", &GuardTarget::Bash);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_blocked());
        assert_eq!(results[0].rule_id, "block-force-push");
    }

    #[test]
    fn test_engine_allows_normal_push() {
        let rules = vec![test_rule(
            "block-force-push",
            r"git\s+push\s+.*--force.*\s+(origin\s+)?main",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git push origin feature-branch", &GuardTarget::Bash);
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_target_mismatch_skips_rule() {
        let rules = vec![test_rule(
            "block-rm",
            r"rm\s+-rf",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        // FileWrite target should not match Bash-targeted rule
        let results = engine.evaluate("rm -rf /", &GuardTarget::FileWrite);
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_any_target_matches_all() {
        let rules = vec![test_rule(
            "log-deploys",
            r"deploy",
            GuardAction::Log,
            GuardTarget::Any,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("npm run deploy", &GuardTarget::Bash);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_engine_disabled_rule_skipped() {
        let mut rule = test_rule("disabled", r".*", GuardAction::Block, GuardTarget::Any);
        rule.enabled = false;
        let engine = GuardEngine::new(&[rule]).unwrap();
        let results = engine.evaluate("anything", &GuardTarget::Bash);
        assert!(results.is_empty());
    }

    #[test]
    fn test_engine_block_before_warn() {
        let rules = vec![
            test_rule("warn-first", r"git\s+add", GuardAction::Warn, GuardTarget::Bash),
            test_rule("block-second", r"git\s+add\s+-A", GuardAction::Block, GuardTarget::Bash),
        ];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git add -A", &GuardTarget::Bash);
        // Block results should appear first
        assert!(results[0].is_blocked());
    }

    #[test]
    fn test_engine_invalid_regex_returns_error() {
        let rules = vec![test_rule(
            "bad-regex",
            r"[invalid",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let result = GuardEngine::new(&rules);
        assert!(result.is_err());
    }

    #[test]
    fn test_engine_multiple_matches() {
        let rules = vec![
            test_rule("warn-git-add", r"git\s+add", GuardAction::Warn, GuardTarget::Bash),
            test_rule("warn-dot", r"\.\s*$", GuardAction::Warn, GuardTarget::Bash),
        ];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("git add .", &GuardTarget::Bash);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_engine_case_insensitive_sql() {
        let rules = vec![test_rule(
            "block-drop",
            r"(?i)DROP\s+TABLE",
            GuardAction::Block,
            GuardTarget::Bash,
        )];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("drop table users;", &GuardTarget::Bash);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_has_blocking_result() {
        let rules = vec![
            test_rule("warn-only", r"test", GuardAction::Warn, GuardTarget::Bash),
        ];
        let engine = GuardEngine::new(&rules).unwrap();
        let results = engine.evaluate("test", &GuardTarget::Bash);
        assert!(!GuardEngine::has_blocking(&results));

        let rules2 = vec![
            test_rule("blocks", r"test", GuardAction::Block, GuardTarget::Bash),
        ];
        let engine2 = GuardEngine::new(&rules2).unwrap();
        let results2 = engine2.evaluate("test", &GuardTarget::Bash);
        assert!(GuardEngine::has_blocking(&results2));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd impulse-rs && cargo test guardrail::engine -v 2>&1 | head -10`
Expected: Compilation error — `GuardEngine` doesn't exist.

**Step 3: Write minimal implementation**

Create `src/guardrail/engine.rs`:

```rust
use regex::Regex;

use super::types::{GuardAction, GuardResult, GuardRule, GuardTarget};

/// Compiled guardrail rule with pre-compiled regex
struct CompiledRule {
    rule: GuardRule,
    regex: Regex,
}

/// Pattern-matching engine for evaluating guardrail rules
pub struct GuardEngine {
    rules: Vec<CompiledRule>,
}

impl GuardEngine {
    /// Create a new engine, compiling all regex patterns.
    /// Returns error if any pattern is invalid.
    pub fn new(rules: &[GuardRule]) -> Result<Self, String> {
        let mut compiled = Vec::with_capacity(rules.len());
        for rule in rules {
            if !rule.enabled {
                continue;
            }
            let regex = Regex::new(&rule.pattern)
                .map_err(|e| format!("Invalid regex in rule '{}': {}", rule.id, e))?;
            compiled.push(CompiledRule {
                rule: rule.clone(),
                regex,
            });
        }
        Ok(Self { rules: compiled })
    }

    /// Evaluate an action against all rules.
    /// Returns matching results sorted: Block first, then Warn, then Log.
    pub fn evaluate(&self, input: &str, target: &GuardTarget) -> Vec<GuardResult> {
        let mut results = Vec::new();

        for compiled in &self.rules {
            // Skip if target doesn't match
            if !compiled.rule.target.matches(target) {
                continue;
            }

            // Check pattern match
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

        // Sort: Block first, then Warn, then Log
        results.sort_by_key(|r| match r.action {
            GuardAction::Block => 0,
            GuardAction::Warn => 1,
            GuardAction::Log => 2,
        });

        results
    }

    /// Check if any result is a blocking action
    pub fn has_blocking(results: &[GuardResult]) -> bool {
        results.iter().any(|r| r.is_blocked())
    }
}
```

Add `pub mod engine;` to `src/guardrail/mod.rs` and add `pub use engine::GuardEngine;` to the re-exports.

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test guardrail::engine -- --nocapture`
Expected: All 10 tests PASS.

**Step 5: Commit**

```bash
git add src/guardrail/engine.rs src/guardrail/mod.rs
git commit -m "feat(guardrail): add pattern matching engine with regex compilation and target filtering"
```

---

### Task 3: Add built-in default rules

**Files:**
- Create: `src/guardrail/defaults.rs`
- Modify: `src/guardrail/mod.rs` (add `pub mod defaults;`)

**Step 1: Write the failing test**

In `src/guardrail/defaults.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::engine::GuardEngine;

    #[test]
    fn test_default_rules_all_valid_regex() {
        let rules = builtin_rules();
        // All patterns must compile
        let engine = GuardEngine::new(&rules);
        assert!(engine.is_ok(), "Built-in rules have invalid regex: {:?}", engine.err());
    }

    #[test]
    fn test_default_rules_have_unique_ids() {
        let rules = builtin_rules();
        let mut ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), rules.len(), "Duplicate rule IDs found");
    }

    #[test]
    fn test_default_rules_all_enabled() {
        let rules = builtin_rules();
        for rule in &rules {
            assert!(rule.enabled, "Built-in rule '{}' should be enabled", rule.id);
            assert!(rule.builtin, "Built-in rule '{}' should have builtin=true", rule.id);
        }
    }

    #[test]
    fn test_blocks_force_push_main() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        assert!(GuardEngine::has_blocking(&engine.evaluate("git push --force origin main", &GuardTarget::Bash)));
        assert!(GuardEngine::has_blocking(&engine.evaluate("git push -f origin main", &GuardTarget::Bash)));
        // Normal push should be fine
        assert!(!GuardEngine::has_blocking(&engine.evaluate("git push origin main", &GuardTarget::Bash)));
        // Force push to feature branch should be fine
        assert!(!GuardEngine::has_blocking(&engine.evaluate("git push --force origin feature", &GuardTarget::Bash)));
    }

    #[test]
    fn test_blocks_bulk_git_add() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        assert!(GuardEngine::has_blocking(&engine.evaluate("git add -A", &GuardTarget::Bash)));
        assert!(GuardEngine::has_blocking(&engine.evaluate("git add --all", &GuardTarget::Bash)));
        assert!(GuardEngine::has_blocking(&engine.evaluate("git add .", &GuardTarget::Bash)));
        // Specific files should be fine
        assert!(!GuardEngine::has_blocking(&engine.evaluate("git add src/main.rs", &GuardTarget::Bash)));
    }

    #[test]
    fn test_blocks_rm_rf_root() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        assert!(GuardEngine::has_blocking(&engine.evaluate("rm -rf /", &GuardTarget::Bash)));
        assert!(GuardEngine::has_blocking(&engine.evaluate("rm -rf ~/", &GuardTarget::Bash)));
        // Targeted rm should be fine
        assert!(!GuardEngine::has_blocking(&engine.evaluate("rm -rf target/", &GuardTarget::Bash)));
    }

    #[test]
    fn test_blocks_drop_table() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        assert!(GuardEngine::has_blocking(&engine.evaluate("DROP TABLE users;", &GuardTarget::Bash)));
        assert!(GuardEngine::has_blocking(&engine.evaluate("drop database production;", &GuardTarget::Bash)));
    }

    #[test]
    fn test_warns_binary_staging() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        let results = engine.evaluate("git add release.zip", &GuardTarget::Bash);
        assert!(!results.is_empty());
        assert!(!GuardEngine::has_blocking(&results)); // Warn, not block
    }

    #[test]
    fn test_warns_artifact_staging() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        let results = engine.evaluate("git add node_modules/", &GuardTarget::Bash);
        assert!(!results.is_empty());
        assert!(!GuardEngine::has_blocking(&results));
    }

    #[test]
    fn test_warns_env_file() {
        let engine = GuardEngine::new(&builtin_rules()).unwrap();
        let results = engine.evaluate("git add .env", &GuardTarget::Bash);
        assert!(!results.is_empty());
        assert!(!GuardEngine::has_blocking(&results));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd impulse-rs && cargo test guardrail::defaults -v 2>&1 | head -10`
Expected: Compilation error — `builtin_rules()` doesn't exist.

**Step 3: Write minimal implementation**

Create `src/guardrail/defaults.rs`:

```rust
use super::types::{GuardAction, GuardRule, GuardTarget};

/// Returns the compiled-in default guardrail rules.
/// These ship with Impulse and protect against common dangerous operations.
pub fn builtin_rules() -> Vec<GuardRule> {
    vec![
        // ── Block rules (prevent execution) ──────────────────────────
        GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: r"git\s+push\s+(.*\s+)?(-f|--force)\s+(.*\s+)?(origin\s+)?main\b".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Force pushing to main rewrites shared history and can cause data loss".to_string(),
            suggestion: Some("Create a branch and open a PR instead: git checkout -b fix/my-changes && git push origin fix/my-changes".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-bulk-git-add".to_string(),
            pattern: r"git\s+add\s+(-A|--all|\.\s*$)".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Bulk git add can accidentally stage files from sibling projects, build artifacts, or secrets".to_string(),
            suggestion: Some("Stage specific files instead: git add src/ tests/ or git add file1.rs file2.rs".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-rm-rf-root".to_string(),
            pattern: r"rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?-?[a-zA-Z]*r[a-zA-Z]*\s+[/~]".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "Recursive force-delete from root or home directory can cause catastrophic data loss".to_string(),
            suggestion: Some("Target a specific subdirectory instead, or use trash-cli for safer deletion".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "block-drop-table".to_string(),
            pattern: r"(?i)(DROP\s+TABLE|DROP\s+DATABASE)".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: "DROP TABLE/DATABASE is irreversible and can destroy production data".to_string(),
            suggestion: Some("Use a migration tool with rollback support, or verify you're targeting the correct database first".to_string()),
            enabled: true,
            builtin: true,
        },

        // ── Warn rules (allow but flag) ──────────────────────────────
        GuardRule {
            id: "warn-binary-staging".to_string(),
            pattern: r"git\s+add\s+.*\.(zip|pdf|exe|dll|dmg|iso|tar\.gz|tgz|jar|war|wasm)\b".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Binary files bloat git history permanently — even deleting them later doesn't reclaim space".to_string(),
            suggestion: Some("Use Git LFS for binary files, or store them in cloud storage".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-artifact-staging".to_string(),
            pattern: r"git\s+add\s+.*(node_modules|\.venv|__pycache__|\.next|dist/|\.pnpm-store|target/)".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Build artifacts and virtual environments should not be committed — they can be hundreds of MB".to_string(),
            suggestion: Some("Add these to .gitignore and stage only source files".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-env-file-staging".to_string(),
            pattern: r"git\s+add\s+.*\.env\b".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "Environment files often contain API keys and secrets that should not be committed".to_string(),
            suggestion: Some("Add .env to .gitignore and use .env.example for templates".to_string()),
            enabled: true,
            builtin: true,
        },
        GuardRule {
            id: "warn-chmod-777".to_string(),
            pattern: r"chmod\s+(-R\s+)?777".to_string(),
            action: GuardAction::Warn,
            target: GuardTarget::Bash,
            reason: "chmod 777 makes files world-readable/writable/executable — a security risk".to_string(),
            suggestion: Some("Use more restrictive permissions: chmod 755 for dirs, chmod 644 for files".to_string()),
            enabled: true,
            builtin: true,
        },

        // ── Log rules (audit trail) ──────────────────────────────────
        GuardRule {
            id: "log-deploy-commands".to_string(),
            pattern: r"\b(deploy|publish|release)\b".to_string(),
            action: GuardAction::Log,
            target: GuardTarget::Bash,
            reason: "Deploy/publish/release commands logged for audit trail".to_string(),
            suggestion: None,
            enabled: true,
            builtin: true,
        },
    ]
}
```

Add `pub mod defaults;` to `src/guardrail/mod.rs`.

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test guardrail::defaults -- --nocapture`
Expected: All 10 tests PASS.

**Step 5: Commit**

```bash
git add src/guardrail/defaults.rs src/guardrail/mod.rs
git commit -m "feat(guardrail): add built-in default rules for git, shell, and deploy safety"
```

---

### Task 4: Add config.json integration

**Files:**
- Create: `src/guardrail/config.rs`
- Modify: `src/guardrail/mod.rs` (add `pub mod config;`)
- Modify: `src/state/mod.rs:17` (add `guardrails: GuardConfig` field to `Config`)
- Modify: `src/state/mod.rs:128-188` (add default in `Default` impl)

**Step 1: Write the failing test**

In `src/guardrail/config.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_no_user_rules_returns_defaults() {
        let config = GuardConfig::default();
        let merged = merge_rules(&config);
        let defaults = builtin_rules();
        assert_eq!(merged.len(), defaults.len());
    }

    #[test]
    fn test_merge_user_override_replaces_builtin() {
        let mut config = GuardConfig::default();
        config.rules.push(GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: r"git\s+push\s+--force".to_string(),
            action: GuardAction::Warn, // Downgrade from Block to Warn
            target: GuardTarget::Bash,
            reason: "Custom reason".to_string(),
            suggestion: None,
            enabled: true,
            builtin: false,
        });
        let merged = merge_rules(&config);
        let force_push = merged.iter().find(|r| r.id == "block-force-push-main").unwrap();
        assert_eq!(force_push.action, GuardAction::Warn);
        assert_eq!(force_push.reason, "Custom reason");
        assert!(!force_push.builtin);
    }

    #[test]
    fn test_merge_user_adds_new_rule() {
        let defaults_count = builtin_rules().len();
        let mut config = GuardConfig::default();
        config.rules.push(GuardRule {
            id: "custom-rule".to_string(),
            pattern: r"my-dangerous-thing".to_string(),
            action: GuardAction::Block,
            target: GuardTarget::Any,
            reason: "Custom".to_string(),
            suggestion: None,
            enabled: true,
            builtin: false,
        });
        let merged = merge_rules(&config);
        assert_eq!(merged.len(), defaults_count + 1);
    }

    #[test]
    fn test_merge_disabled_config_returns_empty() {
        let mut config = GuardConfig::default();
        config.enabled = false;
        let merged = merge_rules(&config);
        assert!(merged.is_empty());
    }

    #[test]
    fn test_merge_user_disables_builtin() {
        let mut config = GuardConfig::default();
        config.rules.push(GuardRule {
            id: "block-force-push-main".to_string(),
            pattern: String::new(),
            action: GuardAction::Block,
            target: GuardTarget::Bash,
            reason: String::new(),
            suggestion: None,
            enabled: false, // Disable it
            builtin: false,
        });
        let merged = merge_rules(&config);
        let force_push = merged.iter().find(|r| r.id == "block-force-push-main");
        assert!(force_push.is_none(), "Disabled rule should be filtered out");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd impulse-rs && cargo test guardrail::config -v 2>&1 | head -10`
Expected: Compilation error — `merge_rules()` doesn't exist.

**Step 3: Write minimal implementation**

Create `src/guardrail/config.rs`:

```rust
use std::collections::HashMap;

use super::defaults::builtin_rules;
use super::types::{GuardConfig, GuardRule};

/// Merge built-in default rules with user-configured rules.
/// User rules with the same `id` as a built-in rule override it.
/// If guardrails are disabled globally, returns empty vec.
pub fn merge_rules(config: &GuardConfig) -> Vec<GuardRule> {
    if !config.enabled {
        return Vec::new();
    }

    // Index user rules by ID for O(1) lookup
    let user_rules: HashMap<&str, &GuardRule> = config
        .rules
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();

    let mut merged = Vec::new();

    // Start with built-in defaults, applying user overrides
    for default in builtin_rules() {
        if let Some(user_override) = user_rules.get(default.id.as_str()) {
            // User override: use their version if enabled
            if user_override.enabled {
                merged.push((*user_override).clone());
            }
            // If disabled, skip entirely (don't add the builtin)
        } else {
            merged.push(default);
        }
    }

    // Add any user rules that don't override built-ins
    let builtin_ids: Vec<String> = builtin_rules().iter().map(|r| r.id.clone()).collect();
    for user_rule in &config.rules {
        if !builtin_ids.contains(&user_rule.id) && user_rule.enabled {
            merged.push(user_rule.clone());
        }
    }

    merged
}
```

Modify `src/state/mod.rs` — add `guardrails` field to `Config` struct (after line 125, before `}`):

```rust
    /// Guardrail configuration
    #[serde(default)]
    pub guardrails: crate::guardrail::GuardConfig,
```

Add to the `Default` impl (after line 185, before the closing `}`):

```rust
            guardrails: crate::guardrail::GuardConfig::default(),
```

Add `pub mod config;` to `src/guardrail/mod.rs`.

Also add to `Config::get()` match (in `src/state/mod.rs`):

```rust
            "guardrails_enabled" => Some(self.guardrails.enabled.to_string()),
```

And to `Config::set()` (in `src/state/mod.rs`):

```rust
            "guardrails_enabled" => {
                self.guardrails.enabled = value.parse().unwrap_or(true);
                Ok(())
            }
```

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test guardrail::config -- --nocapture`
Expected: All 5 tests PASS.

**Step 5: Commit**

```bash
git add src/guardrail/config.rs src/guardrail/mod.rs src/state/mod.rs
git commit -m "feat(guardrail): add config integration with builtin/user rule merging"
```

---

### Task 5: Add `guard` CLI command

**Files:**
- Modify: `src/main.rs:61-449` (add `Guard` variant to `Commands` enum)
- Modify: `src/main.rs` run_direct_mode (add `Commands::Guard` handler)
- Modify: `src/guardrail/mod.rs` (add public `evaluate_action()` convenience function)

**Step 1: Write the failing test**

Add to `src/guardrail/mod.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_action_blocks_dangerous() {
        let config = GuardConfig::default();
        let results = evaluate_action("git push --force origin main", "bash", &config);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert!(GuardEngine::has_blocking(&results));
    }

    #[test]
    fn test_evaluate_action_allows_safe() {
        let config = GuardConfig::default();
        let results = evaluate_action("git push origin main", "bash", &config);
        assert!(results.is_ok());
        assert!(results.unwrap().is_empty());
    }

    #[test]
    fn test_evaluate_action_invalid_target_defaults_to_any() {
        let config = GuardConfig::default();
        let results = evaluate_action("deploy production", "unknown-target", &config);
        assert!(results.is_ok());
    }

    #[test]
    fn test_list_active_rules() {
        let config = GuardConfig::default();
        let rules = list_active_rules(&config);
        assert!(!rules.is_empty());
        assert!(rules.iter().all(|r| r.enabled));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cd impulse-rs && cargo test guardrail::tests -v 2>&1 | head -10`
Expected: Compilation error — `evaluate_action()` and `list_active_rules()` don't exist.

**Step 3: Write minimal implementation**

Add to `src/guardrail/mod.rs`:

```rust
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
        "tool-call" => GuardTarget::ToolCall,
        "file-write" => GuardTarget::FileWrite,
        _ => GuardTarget::Any,
    }
}

/// Evaluate an action against all active guardrail rules.
/// Returns Ok(results) or Err if regex compilation fails.
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
```

Add to `Commands` enum in `src/main.rs` (after `AgentQuery` variant, before the closing `}`):

```rust
    /// Evaluate an action against guardrail rules
    Guard {
        /// The action/command to evaluate
        #[arg(long)]
        action: Option<String>,
        /// Target type: bash, tool-call, file-write, any
        #[arg(long, default_value = "bash")]
        target: String,
        /// List all active rules
        #[arg(long)]
        list: bool,
        /// Enable a rule by ID
        #[arg(long)]
        enable: Option<String>,
        /// Disable a rule by ID
        #[arg(long)]
        disable: Option<String>,
    },
```

Add handler in `run_direct_mode()` (after the last `Commands::` match arm):

```rust
        Commands::Guard {
            action,
            target,
            list,
            enable,
            disable,
        } => {
            let config = state.config().await;

            if list {
                let rules = guardrail::list_active_rules(&config.guardrails);
                println!("{} active guardrail rules:\n", rules.len());
                for rule in &rules {
                    let icon = match rule.action {
                        guardrail::GuardAction::Block => "🛑",
                        guardrail::GuardAction::Warn => "⚠️",
                        guardrail::GuardAction::Log => "📝",
                    };
                    println!(
                        "  {} {} [{}] {}",
                        icon,
                        rule.id,
                        rule.target,
                        rule.reason
                    );
                    if let Some(ref suggestion) = rule.suggestion {
                        println!("    → {}", suggestion);
                    }
                }
                return Ok(());
            }

            if let Some(rule_id) = enable {
                let mut config = config.clone();
                // Remove any disable override for this rule
                config.guardrails.rules.retain(|r| r.id != rule_id || r.enabled);
                state.update_config(config).await;
                println!("Enabled guardrail rule: {}", rule_id);
                return Ok(());
            }

            if let Some(rule_id) = disable {
                let mut config = config.clone();
                // Add a disabled override
                config.guardrails.rules.push(guardrail::GuardRule {
                    id: rule_id.clone(),
                    pattern: String::new(),
                    action: guardrail::GuardAction::Block,
                    target: guardrail::GuardTarget::Any,
                    reason: String::new(),
                    suggestion: None,
                    enabled: false,
                    builtin: false,
                });
                state.update_config(config).await;
                println!("Disabled guardrail rule: {}", rule_id);
                return Ok(());
            }

            if let Some(action_str) = action {
                match guardrail::evaluate_action(&action_str, &target, &config.guardrails) {
                    Ok(results) => {
                        if results.is_empty() {
                            // No rules matched — proceed
                            return Ok(());
                        }

                        let has_block = guardrail::GuardEngine::has_blocking(&results);

                        for result in &results {
                            let prefix = match result.action {
                                guardrail::GuardAction::Block => "BLOCKED",
                                guardrail::GuardAction::Warn => "WARNING",
                                guardrail::GuardAction::Log => "LOGGED",
                            };
                            eprintln!(
                                "{} by Impulse guardrail '{}': {}",
                                prefix, result.rule_id, result.reason
                            );
                            if let Some(ref suggestion) = result.suggestion {
                                eprintln!("  Suggestion: {}", suggestion);
                            }
                        }

                        // Output structured JSON to stderr for programmatic consumption
                        let json = serde_json::to_string(&results).unwrap_or_default();
                        eprintln!("\n{}", json);

                        if has_block {
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Guardrail evaluation error: {}", e);
                        // Don't block on internal errors — fail open
                    }
                }
            } else {
                eprintln!("Usage: impulse-rs guard --action \"command\" [--target bash]");
                eprintln!("       impulse-rs guard --list");
            }
        }
```

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test guardrail -- --nocapture && cargo build`
Expected: All guardrail tests PASS, build succeeds.

**Step 5: Commit**

```bash
git add src/guardrail/mod.rs src/main.rs
git commit -m "feat(guardrail): add guard CLI command with evaluate, list, enable/disable"
```

---

### Task 6: Add daemon IPC integration

**Files:**
- Modify: `src/daemon/mod.rs:20-81` (add `GuardEvaluate` variant to `DaemonRequest`)
- Modify: `src/daemon/mod.rs` process_request (add handler)

**Step 1: Write the failing test**

Add to daemon tests (or `src/guardrail/mod.rs` tests):

```rust
#[test]
fn test_guard_evaluate_request_serde() {
    let json = r#"{"type":"GuardEvaluate","data":{"target":"bash","action":"git push --force main"}}"#;
    let request: DaemonRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(request, DaemonRequest::GuardEvaluate { .. }));
}
```

**Step 2: Run test to verify it fails**

Expected: Compilation error — `GuardEvaluate` variant doesn't exist.

**Step 3: Write minimal implementation**

Add variant to `DaemonRequest` enum in `src/daemon/mod.rs` (after `AgentAssist`):

```rust
    /// Evaluate an action against guardrail rules
    GuardEvaluate {
        target: String,
        action: String,
    },
    /// List active guardrail rules
    GuardList,
```

Add handler in `process_request()` (after the `AgentAssist` arm):

```rust
            DaemonRequest::GuardEvaluate { target, action } => {
                let config = state.config().await;
                match crate::guardrail::evaluate_action(&action, &target, &config.guardrails) {
                    Ok(results) => {
                        let has_block = crate::guardrail::GuardEngine::has_blocking(&results);
                        DaemonResponse::Ok {
                            result: serde_json::json!({
                                "blocked": has_block,
                                "results": results,
                            }),
                        }
                    }
                    Err(e) => DaemonResponse::Error {
                        message: format!("Guardrail evaluation failed: {}", e),
                    },
                }
            }
            DaemonRequest::GuardList => {
                let config = state.config().await;
                let rules = crate::guardrail::list_active_rules(&config.guardrails);
                DaemonResponse::Ok {
                    result: serde_json::json!({ "rules": rules }),
                }
            }
```

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo test daemon -- --nocapture && cargo build`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/daemon/mod.rs
git commit -m "feat(guardrail): add GuardEvaluate and GuardList daemon IPC messages"
```

---

### Task 7: Update hook setup to include guard evaluation

**Files:**
- Modify: `src/main.rs:1062-1121` (update hooks handler to include pre-execution guard hooks)

**Step 1: Write the failing test**

This is an integration-level change. Test by verifying the hook config output contains guard commands:

```rust
// In a test, verify the hook JSON includes guard matchers
#[test]
fn test_hook_config_includes_guard() {
    let hook_config = generate_claude_hooks_config();
    assert!(hook_config.contains("guard"));
    assert!(hook_config.contains("PreToolUse"));
}
```

**Step 2: Run test to verify it fails**

Expected: `generate_claude_hooks_config()` doesn't exist.

**Step 3: Write minimal implementation**

Extract the hook config string from the handler and update to include guard hooks. Update the `Commands::Hooks` handler (lines 1071-1085) so the JSON includes PreToolUse matchers for guard evaluation:

```rust
                    let hook_config = serde_json::json!({
                        "hooks": {
                            "PreToolUse": [
                                {
                                    "matcher": "Bash",
                                    "hooks": [{
                                        "type": "command",
                                        "command": format!("impulse-rs -c {} guard --action \"$INPUT\" --target bash", impulse_path)
                                    }]
                                }
                            ],
                            "PostToolUse": [
                                {
                                    "matcher": "Bash",
                                    "hooks": [{
                                        "type": "command",
                                        "command": format!("impulse-rs -c {} track-tool --tool Bash --session-id $IMPULSE_SESSION_ID", impulse_path)
                                    }]
                                },
                                {
                                    "matcher": "Write",
                                    "hooks": [{
                                        "type": "command",
                                        "command": format!("impulse-rs -c {} track-write --file \"$INPUT\" --session-id $IMPULSE_SESSION_ID", impulse_path)
                                    }]
                                }
                            ]
                        }
                    });
```

Note: The exact Claude Code hook format may need to reference their latest docs. The key point is that `PreToolUse` hooks with a non-zero exit code will block the tool execution.

**Step 4: Run test to verify it passes**

Run: `cd impulse-rs && cargo build && cargo test`
Expected: Build succeeds, all tests pass.

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(guardrail): update hook setup to include PreToolUse guard evaluation"
```

---

### Task 8: Final verification and clippy/fmt

**Files:** All modified files

**Step 1: Run full test suite**

Run: `cd impulse-rs && cargo test`
Expected: All tests pass (929 existing + ~45 new guardrail tests)

**Step 2: Run clippy**

Run: `cd impulse-rs && cargo clippy -- -D warnings`
Expected: No warnings.

**Step 3: Run fmt check**

Run: `cd impulse-rs && cargo fmt --check`
Expected: No formatting issues.

**Step 4: Manual smoke test**

```bash
cd impulse-rs && cargo run -- guard --list
cd impulse-rs && cargo run -- guard --action "git push --force origin main" --target bash
echo $?  # Should be 1
cd impulse-rs && cargo run -- guard --action "git push origin main" --target bash
echo $?  # Should be 0
```

**Step 5: Final commit**

```bash
git add -A  # Safe here since we're inside impulse-rs, not parent
git commit -m "feat(guardrail): complete guardrail module with engine, defaults, CLI, and daemon IPC"
```

---

## Summary

| Task | What | Tests Added | Files |
|------|------|-------------|-------|
| 1 | Types (GuardRule, GuardAction, GuardTarget, GuardResult) | 7 | 3 |
| 2 | Pattern matching engine | 10 | 2 |
| 3 | Built-in default rules | 10 | 2 |
| 4 | Config integration (merge + state) | 5 | 3 |
| 5 | Guard CLI command | 4 | 2 |
| 6 | Daemon IPC (GuardEvaluate, GuardList) | 1 | 1 |
| 7 | Hook setup update | 1 | 1 |
| 8 | Verification (clippy, fmt, smoke test) | 0 | 0 |
| **Total** | | **~38** | **5 new + 3 modified** |
