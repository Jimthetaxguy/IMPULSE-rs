//! Shared test-only synchronization helpers (impulse-rs lib crate).
//!
//! `cargo test` runs all unit tests for this crate in one process on
//! multiple threads. A test that mutates a process-global env var used by
//! more than one module must serialize on the SAME lock — a per-file
//! `static ENV_LOCK` only serializes tests within that one file, not across
//! files, which silently reintroduces the exact race the pattern exists to
//! prevent. `ION_GATE_LAUNCHER` (`impulse_ion::pi_adapter::ION_GATE_LAUNCHER_ENV`)
//! is mutated by tests in `handlers::ion`, `ion_repl` (mod.rs), and
//! `ion_repl::tool_verify` — all three share this lock (TUI_SPEC.md T7).
//! `IMPULSE_EMBED_SCRIPT` is likewise mutated by both
//! `retrieval::embedding` and `retrieval::indexer`, which share a separate
//! retrieval-specific lock below.

#![cfg(test)]

/// Acquire the process-wide lock guarding mutation of `ION_GATE_LAUNCHER`
/// (and any other env var shared across these test modules). Poison-safe:
/// a prior test panicking while holding the lock must not deadlock every
/// subsequent test.
pub(crate) fn ion_gate_launcher_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Creates a throwaway git repository with one empty commit, hermetic
/// against the invoking user's real git configuration (Stage 1 lane,
/// `docs/superpowers/specs/2026-09-02-ion-tool-sandbox-and-untrusted-output.md`).
///
/// Before this helper, five call sites (`ion_repl::{mod, chat, tool_verify}`,
/// `handlers::ion`, `tests/ion_verify_cli.rs`) each hand-rolled their own
/// `git init` fixture with no environment isolation. Under a broad
/// `cargo test --lib -- ion_repl` filter the fixtures occasionally failed
/// with `insufficient permission for adding an object to repository
/// database .git/objects` -- each fixture uses its own `tempfile::TempDir`,
/// so the cause was never a shared path; the leading suspect is a global
/// git config (`~/.gitconfig`, e.g. `core.sharedRepository`) or a
/// credential helper leaking into the child process across concurrent test
/// threads. `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1` stop
/// git from reading any global or system config at all, and pointing `HOME`
/// at the same tempdir removes the legacy `~/.gitconfig` fallback path
/// those two variables don't cover on older git. `user.name`/`user.email`
/// are set locally so the commit succeeds with no ambient identity.
///
/// Panics with the captured stderr on any git failure -- this is test
/// setup, not a `Result`-returning production path (CLAUDE.md Principle #1
/// scopes `unwrap`/`expect` to tests and `main()`).
///
/// `tests/ion_verify_cli.rs` is a separate integration-test crate compiled
/// against a non-`--cfg test` build of this library, so it cannot see this
/// `#[cfg(test)]`-gated module regardless of visibility; it keeps its own
/// copy of this exact fixture (see that file's `init_git_repo`).
pub(crate) fn init_git_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", dir.path())
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "--quiet"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["commit", "--allow-empty", "--quiet", "-m", "init"]);
    dir
}

/// Acquire the process-wide lock guarding test mutation of
/// `IMPULSE_EMBED_SCRIPT` and related retrieval-embedding env configuration.
///
/// This must be shared by both `retrieval::embedding` and
/// `retrieval::indexer`; separate module-local locks allow one test to replace
/// the other test's script path while both run in the same libtest process.
/// Poison recovery keeps later tests runnable if an earlier assertion panics.
pub(crate) fn retrieval_embedding_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Poll until no process command line matches `pattern`, bounded by `timeout`.
///
/// Process-group SIGKILL is synchronous, but reaping can lag under a heavily
/// parallel test run. Tests must wait for the observable condition instead of
/// assuming an arbitrary fixed delay is sufficient. The last matching PID set
/// is returned on timeout so failures retain actionable evidence.
#[cfg(unix)]
pub(crate) async fn wait_for_no_matching_process(
    pattern: &str,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        let check = tokio::process::Command::new("pgrep")
            .arg("-f")
            .arg(pattern)
            .output()
            .await
            .map_err(|error| {
                format!(
                    "pgrep unavailable -- cannot verify orphan-kill; install pgrep or adjust the check: {error}"
                )
            })?;
        let stray = String::from_utf8_lossy(&check.stdout).trim().to_string();
        if stray.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let pid_list = stray.lines().collect::<Vec<_>>().join(",");
            let details = tokio::process::Command::new("ps")
                .args(["-o", "pid=,ppid=,pgid=,stat=,etime=,command=", "-p"])
                .arg(&pid_list)
                .output()
                .await
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .unwrap_or_else(|error| format!("unable to inspect matching pids: {error}"));
            return Err(format!("pids: {stray}; process details: {details}"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// Poll until every exact PID has disappeared, bounded by `timeout`.
///
/// Exact PID checks avoid the cross-test ambiguity of regex process scans.
/// Failure output includes process-group and state data for any survivors.
#[cfg(unix)]
pub(crate) async fn wait_for_pids_to_exit(
    pids: &[u32],
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    let pid_list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");

    loop {
        let check = tokio::process::Command::new("ps")
            .args(["-o", "pid=,ppid=,pgid=,stat=,etime=,command=", "-p"])
            .arg(&pid_list)
            .output()
            .await
            .map_err(|error| format!("unable to inspect child pids {pid_list}: {error}"))?;
        let survivors = String::from_utf8_lossy(&check.stdout).trim().to_string();
        if survivors.is_empty() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(survivors);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod init_git_repo_tests {
    use super::init_git_repo;

    #[test]
    fn test_init_git_repo_creates_a_repo_with_one_commit_on_head() {
        let repo = init_git_repo();
        assert!(repo.path().join(".git").exists());
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["rev-parse", "HEAD"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", repo.path())
            .output()
            .expect("git rev-parse should run");
        assert!(output.status.success(), "HEAD should resolve to a commit");
        assert!(!String::from_utf8_lossy(&output.stdout).trim().is_empty());
    }

    #[test]
    fn test_init_git_repo_two_calls_yield_independent_directories() {
        // Regression guard for the flake this fixture replaces: two
        // fixtures created back to back must never share a path or state.
        let a = init_git_repo();
        let b = init_git_repo();
        assert_ne!(a.path(), b.path());
    }
}
