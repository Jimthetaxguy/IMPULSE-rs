//! Shared git repository fixture for unit and integration tests.
//!
//! One implementation avoids drift across `handlers::ion`, `ion_repl`, and
//! `tests/ion_verify_cli.rs`. Identity is passed per-invocation via `-c`
//! flags so tests do not depend on the operator's global git config.

#![allow(dead_code)]

use std::process::Command;
use tempfile::TempDir;

/// Creates a throwaway git repository on branch `main` with one empty commit
/// so refs like `HEAD` resolve inside ion-verify and governed-task tests.
pub fn init_git_repo_with_empty_commit() -> TempDir {
    let dir = TempDir::new().expect("failed to create tempdir");
    let repo = dir.path();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .args(args)
            .status()
            .expect("failed to spawn git");
        assert!(
            status.success(),
            "git {args:?} failed in {}",
            repo.display()
        );
    };
    git(&["init", "--quiet", "--initial-branch=main"]);
    git(&[
        "-c",
        "user.email=test@example.com",
        "-c",
        "user.name=Test",
        "commit",
        "--allow-empty",
        "--quiet",
        "-m",
        "init",
    ]);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_init_git_repo_with_empty_commit_resolves_head() {
        let dir = init_git_repo_with_empty_commit();
        let output = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .expect("git rev-parse HEAD");
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
    }
}
