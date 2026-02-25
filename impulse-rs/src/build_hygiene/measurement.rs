// Measurement — disk usage analysis for Rust build artifacts

use crate::build_hygiene::discovery::RustProject;
use crate::build_hygiene::format_bytes;
use crate::tools::health::{HealthCheck, HealthStatus};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Disk usage report for all discovered Rust projects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskUsageReport {
    /// Individual project measurements
    pub projects: Vec<ProjectMeasurement>,
    /// Total bytes across all projects
    pub total_bytes: u64,
    /// Human-readable total
    pub total_human: String,
    /// Number of projects found
    pub project_count: usize,
    /// Recommendations
    pub recommendations: Vec<String>,
}

/// Measurement for a single project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeasurement {
    pub path: String,
    pub target_size_bytes: u64,
    pub target_size_human: String,
    pub has_cargo_lock: bool,
    pub toolchain_count: usize,
}

/// Measure a single project's target directory
pub fn measure_project(project: &RustProject) -> ProjectMeasurement {
    ProjectMeasurement {
        path: project.path.to_string_lossy().to_string(),
        target_size_bytes: project.target_size_bytes,
        target_size_human: format_bytes(project.target_size_bytes),
        has_cargo_lock: project.has_cargo_lock,
        toolchain_count: project.toolchain_versions.len(),
    }
}

/// Measure total target/ size across given search paths
pub fn measure_total_target_size(search_paths: &[PathBuf]) -> u64 {
    let projects = crate::build_hygiene::discover_rust_projects(search_paths);
    projects.iter().map(|p| p.target_size_bytes).sum()
}

/// Generate a full disk usage report
pub fn generate_report(projects: &[RustProject], threshold_gb: f64) -> DiskUsageReport {
    let measurements: Vec<ProjectMeasurement> = projects.iter().map(measure_project).collect();
    let total_bytes: u64 = projects.iter().map(|p| p.target_size_bytes).sum();
    let total_gb = total_bytes as f64 / 1_073_741_824.0;

    let mut recommendations = Vec::new();

    if total_gb > threshold_gb {
        recommendations.push(format!(
            "Total build artifacts ({:.1} GB) exceed {:.0} GB threshold. Run `impulse-rs sweep`.",
            total_gb, threshold_gb
        ));
    }

    // Check for very large individual projects
    for p in projects {
        let size_gb = p.target_size_bytes as f64 / 1_073_741_824.0;
        if size_gb > 2.0 {
            recommendations.push(format!(
                "{}: {:.1} GB — consider `impulse-rs sweep --path {}`",
                p.path.file_name().unwrap_or_default().to_string_lossy(),
                size_gb,
                p.path.display()
            ));
        }
    }

    // Check for projects without Cargo.lock (might be unused)
    let no_lock = projects.iter().filter(|p| !p.has_cargo_lock).count();
    if no_lock > 0 {
        recommendations.push(format!(
            "{} projects have no Cargo.lock — may be abandoned or template projects",
            no_lock
        ));
    }

    if recommendations.is_empty() {
        recommendations.push(format!(
            "Build artifacts at {:.1} GB — within {:.0} GB threshold.",
            total_gb, threshold_gb
        ));
    }

    DiskUsageReport {
        projects: measurements,
        total_bytes,
        total_human: format_bytes(total_bytes),
        project_count: projects.len(),
        recommendations,
    }
}

/// Generate a health check for build artifacts
pub fn check_build_health(projects: &[RustProject], threshold_gb: f64) -> HealthCheck {
    let total_bytes: u64 = projects.iter().map(|p| p.target_size_bytes).sum();
    let total_gb = total_bytes as f64 / 1_073_741_824.0;

    if total_gb > threshold_gb {
        HealthCheck::warning(
            "Rust build artifacts",
            &format!(
                "{:.1} GB — exceeds {:.0} GB threshold. Run `impulse-rs sweep`",
                total_gb, threshold_gb
            ),
        )
    } else if projects.is_empty() {
        HealthCheck::new(
            "Rust build artifacts",
            HealthStatus::Healthy,
            Some("No Rust projects found in scan paths".to_string()),
        )
    } else {
        HealthCheck::new(
            "Rust build artifacts",
            HealthStatus::Healthy,
            Some(format!(
                "{:.1} GB across {} projects",
                total_gb,
                projects.len()
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_project(path: &str, size: u64, has_lock: bool) -> RustProject {
        RustProject {
            path: PathBuf::from(path),
            target_size_bytes: size,
            last_modified: None,
            has_cargo_lock: has_lock,
            toolchain_versions: vec![],
        }
    }

    #[test]
    fn test_generate_report_within_threshold() {
        let projects = vec![mock_project("/tmp/a", 500_000_000, true)];
        let report = generate_report(&projects, 10.0);
        assert_eq!(report.project_count, 1);
        assert!(report.recommendations[0].contains("within"));
    }

    #[test]
    fn test_generate_report_exceeds_threshold() {
        // 15 GB worth
        let projects = vec![mock_project("/tmp/a", 15_000_000_000, true)];
        let report = generate_report(&projects, 10.0);
        assert!(report.recommendations.iter().any(|r| r.contains("exceed")));
    }

    #[test]
    fn test_health_check_healthy() {
        let projects = vec![mock_project("/tmp/a", 1_000_000, true)];
        let check = check_build_health(&projects, 10.0);
        assert_eq!(check.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_check_warning() {
        let projects = vec![mock_project("/tmp/a", 15_000_000_000, true)];
        let check = check_build_health(&projects, 10.0);
        assert_eq!(check.status, HealthStatus::Warning);
    }

    #[test]
    fn test_health_check_no_projects() {
        let check = check_build_health(&[], 10.0);
        assert_eq!(check.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_measure_project() {
        let proj = mock_project("/tmp/test", 1_073_741_824, true);
        let m = measure_project(&proj);
        assert_eq!(m.target_size_human, "1.0 GB");
        assert!(m.has_cargo_lock);
    }
}
