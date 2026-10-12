//! Shared child-process environment scrubbing helper.
//!
//! **Background:** `tokio::process::Command` inherits the *full* parent
//! process environment by default, exactly like `std::process::Command`. For
//! `bash_exec` (T9 follow-up, same-day adversarial review, see
//! `builtin/bash_exec.rs`'s module doc), that meant a model-triggered `env`
//! command — approved through a human confirmation gate — could print the
//! `ion` process's own secrets (`ANTHROPIC_API_KEY` etc.) straight into
//! `tool_result` content, which flows back into the model's context. The
//! fix there was `.env_clear()` + re-add of a small functional allowlist.
//!
//! `src/tooling/external.rs`'s `ProcessTool` spawns a second, independent
//! kind of child process (commands defined by an on-disk manifest, e.g.
//! `.impulse/tools.d/*.json`) with the same default full-inheritance
//! behavior. As of this writing `ProcessTool` is reached only via
//! `ToolRegistry::with_runtime()` (the daemon's `InvokeTool` IPC endpoint
//! and the `tooling-run` CLI handler), both driven by human/GUI input, not
//! by an LLM tool-calling loop — but the manifest's own `env_allowlist`
//! field already signals developer intent to scrub, and a manifest-defined
//! command is exactly the kind of "runs a fixed command with attacker- or
//! model-adjacent args" surface `bash_exec` was fixed for. Rather than
//! reinvent the allowlist/heuristic logic a second time, both tools share
//! it from this module.
//!
//! This is allowlist, not denylist — matching CLAUDE.md Principle #5's
//! deny-by-default capability philosophy: everything not explicitly named
//! is dropped, rather than trying to enumerate every possible secret name.

use tokio::process::Command;

/// Environment variables re-added to a scrubbed child process after
/// `.env_clear()`, if present in the parent process's own environment.
/// Deliberately an allowlist (everything else is dropped) rather than a
/// denylist. Covers both macOS and Linux, the two platforms this workspace
/// targets: `PATH`/`HOME`/`TERM` so ordinary commands resolve and run at
/// all, locale (`LANG`, `LC_ALL`) so text encoding stays consistent, and
/// temp-dir vars (`TMPDIR` on macOS/BSD, `TMP`/`TEMP` conventionally on
/// other platforms) so tools that need scratch space still find one.
pub(crate) const ENV_ALLOWLIST: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP",
];

/// Case-insensitive substring heuristic for "this env var name looks like
/// it holds a credential." Used defensively against `ENV_ALLOWLIST` itself
/// (see module doc) rather than as the primary scrubbing mechanism — the
/// primary mechanism is the allowlist's `.env_clear()` + re-add, which
/// drops everything not explicitly named regardless of whether its name
/// happens to match this heuristic.
pub(crate) fn is_secret_like(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "_PAT", "CREDENTIAL"]
        .iter()
        .any(|pattern| upper.contains(pattern))
}

/// Clears `cmd`'s inherited environment and re-adds only:
/// 1. [`ENV_ALLOWLIST`] — the small, fixed set of functional variables every
///    child needs (subject to the `is_secret_like` debug-assert guard).
/// 2. `extra_allowlist` — caller-supplied additional names to forward if
///    present in the parent env. **Not** run through `is_secret_like`: this
///    is a deliberate per-caller opt-in (e.g. a `ProcessTool` manifest that
///    explicitly declares `env_allowlist: ["MINIMAX_API_KEY"]` because the
///    external tool it wraps genuinely needs that credential to
///    authenticate) rather than an accidental default, so a credential-
///    shaped name here is expected, not a bug.
pub(crate) fn scrub_and_allowlist_env(cmd: &mut Command, extra_allowlist: &[String]) {
    cmd.env_clear();
    for name in ENV_ALLOWLIST {
        debug_assert!(
            !is_secret_like(name),
            "ENV_ALLOWLIST entry {name:?} matches the secret-name heuristic"
        );
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
    for name in extra_allowlist {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
}

/// Synchronous-command counterpart used by bounded control-plane probes such
/// as Git subject observation. Keeping it here prevents `GIT_*`, credential,
/// and provider variables from silently redirecting those probes.
pub(crate) fn scrub_and_allowlist_std_env(
    cmd: &mut std::process::Command,
    extra_allowlist: &[String],
) {
    cmd.env_clear();
    for name in ENV_ALLOWLIST {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
    for name in extra_allowlist {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_secret_like_matches_known_credential_shapes() {
        for name in [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GITHUB_TOKEN",
            "DB_PASSWORD",
            "GH_PAT",
            "AWS_SECRET_ACCESS_KEY",
            "SOME_CREDENTIAL",
        ] {
            assert!(
                is_secret_like(name),
                "{name} should be flagged as secret-like"
            );
        }
    }

    #[test]
    fn test_is_secret_like_does_not_flag_allowlisted_names() {
        for name in ENV_ALLOWLIST {
            assert!(
                !is_secret_like(name),
                "{name} is on ENV_ALLOWLIST and must not match the secret heuristic"
            );
        }
    }

    /// Serializes tests in this module that mutate process-global env vars.
    /// `cargo test` runs this crate's unit tests in one process across
    /// multiple threads, so a test that sets/removes a var must not race a
    /// sibling test doing the same.
    fn secret_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn test_scrub_and_allowlist_env_drops_unlisted_vars() {
        let _lock = secret_env_lock();
        std::env::set_var("ENV_SCRUB_TEST_SECRET", "should-not-survive");
        let mut cmd = Command::new("true");
        scrub_and_allowlist_env(&mut cmd, &[]);
        // `Command` doesn't expose a getter for its env map directly, so we
        // assert behavior indirectly via the std::process::Command Debug
        // output, which lists the retained env vars.
        let debug = format!("{cmd:?}");
        std::env::remove_var("ENV_SCRUB_TEST_SECRET");
        assert!(
            !debug.contains("ENV_SCRUB_TEST_SECRET"),
            "scrub must drop vars not on the allowlist: {debug}"
        );
    }

    #[test]
    fn test_scrub_and_allowlist_env_forwards_extra_allowlist_entries() {
        let _lock = secret_env_lock();
        std::env::set_var("ENV_SCRUB_TEST_EXTRA", "forwarded-value");
        let mut cmd = Command::new("true");
        scrub_and_allowlist_env(&mut cmd, &["ENV_SCRUB_TEST_EXTRA".to_string()]);
        let debug = format!("{cmd:?}");
        std::env::remove_var("ENV_SCRUB_TEST_EXTRA");
        assert!(
            debug.contains("ENV_SCRUB_TEST_EXTRA"),
            "scrub must forward names in extra_allowlist when present in the parent env: {debug}"
        );
    }
}
