//! Verification pipeline — auto-detects and runs project health checks.
//!
//! Scans the working directory for `package.json` (JS/TS) and `Cargo.toml`
//! (Rust), then assembles a sequence of verification steps (install, typecheck,
//! test, lint, build). Executes steps in order, halting on first failure, and
//! returns a [`VerificationReport`] summarizing pass/fail per step.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct VerificationStep {
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub step: VerificationStep,
    pub success: bool,
    pub output: String,
}

#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub results: Vec<VerificationResult>,
}

impl VerificationReport {
    pub fn success(&self) -> bool {
        self.results.iter().all(|r| r.success)
    }
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    scripts: Option<std::collections::HashMap<String, String>>,
}

fn pick_package_manager(cwd: &Path) -> &'static str {
    if cwd.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if cwd.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

fn collect_js_steps(cwd: &Path) -> Vec<VerificationStep> {
    let mut steps = Vec::new();
    let package_json_path = cwd.join("package.json");
    if !package_json_path.exists() {
        return steps;
    }

    let pm = pick_package_manager(cwd);
    steps.push(VerificationStep {
        name: "dependency install".to_string(),
        command: vec![pm.to_string(), "install".to_string()],
    });

    let scripts = fs::read_to_string(package_json_path)
        .ok()
        .and_then(|content| serde_json::from_str::<PackageJson>(&content).ok())
        .and_then(|pkg| pkg.scripts)
        .unwrap_or_default();

    let mut push_script = |name: &str, label: &str| {
        if scripts.contains_key(name) {
            steps.push(VerificationStep {
                name: label.to_string(),
                command: vec![pm.to_string(), "run".to_string(), name.to_string()],
            });
            true
        } else {
            false
        }
    };

    let _ = push_script("typecheck", "typecheck") || push_script("tsc", "typescript");
    let _ = push_script("test", "tests");
    let _ = push_script("lint:all", "lint") || push_script("lint", "lint");
    let _ = push_script("build", "build");

    steps
}

fn collect_rust_steps(cwd: &Path) -> Vec<VerificationStep> {
    if !cwd.join("Cargo.toml").exists() {
        return Vec::new();
    }
    vec![
        VerificationStep {
            name: "cargo check".to_string(),
            command: vec!["cargo".to_string(), "check".to_string()],
        },
        VerificationStep {
            name: "cargo test".to_string(),
            command: vec!["cargo".to_string(), "test".to_string()],
        },
    ]
}

fn collect_retrieval_contract_steps(cwd: &Path) -> Vec<VerificationStep> {
    if cwd.join("impulse-rs").join("Cargo.toml").exists() {
        return vec![VerificationStep {
            name: "retrieval contract check".to_string(),
            command: vec![
                "cargo".to_string(),
                "run".to_string(),
                "--manifest-path".to_string(),
                "impulse-rs/Cargo.toml".to_string(),
                "--".to_string(),
                "retrieval-status".to_string(),
                "--check".to_string(),
                "--json".to_string(),
            ],
        }];
    }

    if cwd.join("Cargo.toml").exists()
        && cwd.file_name().and_then(|n| n.to_str()) == Some("impulse-rs")
    {
        return vec![VerificationStep {
            name: "retrieval contract check".to_string(),
            command: vec![
                "cargo".to_string(),
                "run".to_string(),
                "--".to_string(),
                "retrieval-status".to_string(),
                "--check".to_string(),
                "--json".to_string(),
            ],
        }];
    }

    Vec::new()
}

pub fn default_steps(cwd: &Path) -> Vec<VerificationStep> {
    let mut steps = Vec::new();
    steps.extend(collect_js_steps(cwd));
    steps.extend(collect_rust_steps(cwd));
    steps.extend(collect_retrieval_contract_steps(cwd));
    steps
}

pub fn run_verification(steps: Vec<VerificationStep>) -> Result<VerificationReport> {
    if steps.is_empty() {
        bail!("No verification commands were detected for this repository");
    }

    let mut results = Vec::new();
    for step in steps {
        let mut iter = step.command.iter();
        let program = iter
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty command"))?;
        let args: Vec<&str> = iter.map(|s| s.as_str()).collect();
        let output = Command::new(program).args(args).output()?;

        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));

        let success = output.status.success();
        results.push(VerificationResult {
            step: step.clone(),
            success,
            output: combined,
        });

        if !success {
            break;
        }
    }

    Ok(VerificationReport { results })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_no_steps_errors() {
        let err = run_verification(Vec::new()).unwrap_err();
        assert!(err.to_string().contains("No verification commands"));
    }

    #[test]
    fn test_collect_rust_steps() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname='x'\nversion='0.1.0'\n",
        )
        .unwrap();
        let steps = default_steps(tmp.path());
        assert!(steps.iter().any(|s| s.name == "cargo check"));
        assert!(steps.iter().any(|s| s.name == "cargo test"));
    }
}
