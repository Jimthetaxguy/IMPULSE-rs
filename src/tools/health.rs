// Health check module - system health verification

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Warning,
    Error,
}

impl HealthCheck {
    pub fn new(name: &str, status: HealthStatus, message: Option<String>) -> Self {
        Self {
            name: name.to_string(),
            status,
            message,
        }
    }

    pub fn healthy(name: &str) -> Self {
        Self::new(name, HealthStatus::Healthy, None)
    }

    pub fn warning(name: &str, message: &str) -> Self {
        Self::new(name, HealthStatus::Warning, Some(message.to_string()))
    }

    pub fn error(name: &str, message: &str) -> Self {
        Self::new(name, HealthStatus::Error, Some(message.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall_status: HealthStatus,
    pub checks: Vec<HealthCheck>,
}

/// Run a health check on the impulse directory
pub fn check_impulse_health(impulse_dir: &Path) -> HealthReport {
    let mut checks = Vec::new();

    // Check if impulse directory exists
    if impulse_dir.exists() {
        checks.push(HealthCheck::healthy("Impulse directory exists"));
    } else {
        checks.push(HealthCheck::error(
            "Impulse directory",
            "Directory does not exist. Run `impulse-rs init` to initialize.",
        ));
        return HealthReport {
            overall_status: HealthStatus::Error,
            checks,
        };
    }

    // Check required files
    let required_files = vec!["config.json", "LIVE_STATE.json"];

    for file in required_files {
        let path = impulse_dir.join(file);
        if path.exists() {
            checks.push(HealthCheck::healthy(&format!("File: {}", file)));
        } else {
            checks.push(HealthCheck::warning(
                &format!("File: {}", file),
                "Not found (may be created on first use)",
            ));
        }
    }

    // Check history file
    let history_path = impulse_dir.join("HISTORY.jsonl");
    if history_path.exists() {
        if let Ok(metadata) = std::fs::metadata(&history_path) {
            let size_kb = metadata.len() / 1024;
            if size_kb > 10000 {
                checks.push(HealthCheck::warning(
                    "History file",
                    &format!("History file is {} KB - consider archiving", size_kb),
                ));
            } else {
                checks.push(HealthCheck::healthy("History file"));
            }
        }
    }

    // Check genome
    let genome_path = impulse_dir.join("GENOME.md");
    if genome_path.exists() {
        checks.push(HealthCheck::healthy("GENOME.md exists"));
    } else {
        checks.push(HealthCheck::warning(
            "GENOME.md",
            "No genome file - decisions won't be persisted",
        ));
    }

    // Determine overall status
    let overall_status = if checks.iter().any(|c| c.status == HealthStatus::Error) {
        HealthStatus::Error
    } else if checks.iter().any(|c| c.status == HealthStatus::Warning) {
        HealthStatus::Warning
    } else {
        HealthStatus::Healthy
    };

    HealthReport {
        overall_status,
        checks,
    }
}

/// Check if Python is available
pub fn check_python_health() -> HealthCheck {
    use crate::tools::python;

    if python::is_python_available() {
        if let Some(version) = python::get_python_version() {
            HealthCheck::healthy("Python").with_message(Some(version.trim().to_string()))
        } else {
            HealthCheck::healthy("Python")
        }
    } else {
        HealthCheck::error("Python", "Python not installed or not in PATH")
    }
}

impl HealthCheck {
    pub fn with_message(mut self, message: Option<String>) -> Self {
        self.message = message;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_creation() {
        let check = HealthCheck::healthy("test");
        assert_eq!(check.name, "test");
        assert_eq!(check.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_report() {
        let checks = vec![
            HealthCheck::healthy("test1"),
            HealthCheck::warning("test2", "warning message"),
            HealthCheck::error("test3", "error message"),
        ];

        let report = HealthReport {
            overall_status: HealthStatus::Error,
            checks,
        };

        assert_eq!(report.overall_status, HealthStatus::Error);
    }
}
