use anyhow::{bail, Result};
use chrono::Utc;
use std::path::Path;

use super::types::*;

const STEWARDSHIP_DIR: &str = "stewardship";
const CROSS_PROJECT_FILE: &str = "cross-project.yaml";
const PROJECTS_DIR: &str = "projects";

/// Validate that a string is safe for use as a filesystem path component.
/// Rejects path traversal sequences and characters that could escape the directory.
fn sanitize_path_component(id: &str, label: &str) -> Result<()> {
    if id.is_empty() {
        bail!("{} must not be empty", label);
    }
    if id.contains("..") || id.contains('/') || id.contains('\\') || id.contains('\0') {
        bail!(
            "Invalid {}: '{}' contains path traversal or separator characters",
            label,
            id
        );
    }
    Ok(())
}

/// Ensure the stewardship directory structure exists
pub fn ensure_dirs(base_path: &Path) -> Result<()> {
    let steward_dir = base_path.join(STEWARDSHIP_DIR);
    std::fs::create_dir_all(steward_dir.join(PROJECTS_DIR))?;
    std::fs::create_dir_all(steward_dir.join("proposals").join("pending"))?;
    std::fs::create_dir_all(steward_dir.join("proposals").join("applied"))?;
    std::fs::create_dir_all(steward_dir.join("logs"))?;
    Ok(())
}

/// Load cross-project memory (creates default if not found)
pub fn load_cross_project(base_path: &Path) -> Result<CrossProjectMemory> {
    let path = base_path.join(STEWARDSHIP_DIR).join(CROSS_PROJECT_FILE);

    if !path.exists() {
        return Ok(CrossProjectMemory::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let memory: CrossProjectMemory = serde_yaml::from_str(&content)?;
    Ok(memory)
}

/// Save cross-project memory atomically
pub fn save_cross_project(base_path: &Path, memory: &CrossProjectMemory) -> Result<()> {
    ensure_dirs(base_path)?;
    let path = base_path.join(STEWARDSHIP_DIR).join(CROSS_PROJECT_FILE);

    let yaml = serde_yaml::to_string(memory)?;
    super::atomic_write_file(&path, yaml.as_bytes())?;
    Ok(())
}

/// Save a session analysis to per-project storage
pub fn save_session_analysis(base_path: &Path, analysis: &SessionAnalysis) -> Result<()> {
    sanitize_path_component(&analysis.project_hash, "project_hash")?;
    sanitize_path_component(&analysis.session_id, "session_id")?;
    ensure_dirs(base_path)?;
    let project_dir = base_path
        .join(STEWARDSHIP_DIR)
        .join(PROJECTS_DIR)
        .join(&analysis.project_hash);
    let sessions_dir = project_dir.join("sessions");
    std::fs::create_dir_all(&sessions_dir)?;

    let path = sessions_dir.join(format!("{}.yaml", analysis.session_id));
    let yaml = serde_yaml::to_string(analysis)?;
    super::atomic_write_file(&path, yaml.as_bytes())?;
    Ok(())
}

/// Load per-project memory
pub fn load_project_memory(base_path: &Path, project_hash: &str) -> Result<ProjectMemory> {
    sanitize_path_component(project_hash, "project_hash")?;
    let path = base_path
        .join(STEWARDSHIP_DIR)
        .join(PROJECTS_DIR)
        .join(project_hash)
        .join("project-memory.yaml");

    if !path.exists() {
        return Ok(ProjectMemory::new(project_hash.to_string(), String::new()));
    }

    let content = std::fs::read_to_string(&path)?;
    let memory: ProjectMemory = serde_yaml::from_str(&content)?;
    Ok(memory)
}

/// Save per-project memory
pub fn save_project_memory(base_path: &Path, memory: &ProjectMemory) -> Result<()> {
    sanitize_path_component(&memory.project_hash, "project_hash")?;
    ensure_dirs(base_path)?;
    let project_dir = base_path
        .join(STEWARDSHIP_DIR)
        .join(PROJECTS_DIR)
        .join(&memory.project_hash);
    std::fs::create_dir_all(&project_dir)?;

    let path = project_dir.join("project-memory.yaml");
    let yaml = serde_yaml::to_string(memory)?;
    super::atomic_write_file(&path, yaml.as_bytes())?;
    Ok(())
}

/// Extract patterns from multiple session analyses
pub fn extract_patterns(sessions: &[SessionAnalysis]) -> Vec<CrossProjectPattern> {
    use std::collections::HashMap;

    let mut tool_failures: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut file_patterns: HashMap<&str, Vec<&str>> = HashMap::new();

    for session in sessions {
        // Track tool usage patterns
        for pattern in &session.tool_patterns {
            if pattern.count >= 5 {
                tool_failures
                    .entry(pattern.tool_name.as_str())
                    .or_default()
                    .push(session.project_hash.as_str());
            }
        }

        // Track frequently touched files
        for file in &session.files_touched {
            file_patterns
                .entry(file.as_str())
                .or_default()
                .push(session.project_hash.as_str());
        }
    }

    let mut patterns = Vec::new();
    let now = Utc::now().format("%Y-%m-%d").to_string();

    // Convert tool patterns
    for (tool, projects) in &tool_failures {
        if projects.len() >= 2 {
            let unique_projects: Vec<String> = {
                let mut p: Vec<&str> = projects.clone();
                p.sort();
                p.dedup();
                p.into_iter().map(|s| s.to_string()).collect()
            };
            patterns.push(CrossProjectPattern {
                id: format!("tool-{}-{}", tool.to_lowercase(), uuid_short()),
                pattern_type: "tool_usage".to_string(),
                description: format!("Heavy {} usage across projects", tool),
                occurrences: projects.len(),
                projects: unique_projects,
                insight: format!("{} is heavily used — consider optimization", tool),
                first_seen: now.clone(),
                last_seen: now.clone(),
            });
        }
    }

    patterns
}

/// Merge new patterns into existing cross-project memory
pub fn merge_patterns(memory: &mut CrossProjectMemory, new_patterns: Vec<CrossProjectPattern>) {
    for new in new_patterns {
        // Check for existing pattern with same type and description
        if let Some(existing) = memory
            .patterns
            .iter_mut()
            .find(|p| p.pattern_type == new.pattern_type && p.description == new.description)
        {
            existing.occurrences += new.occurrences;
            existing.last_seen = new.last_seen;
            // Merge project lists
            for proj in &new.projects {
                if !existing.projects.contains(proj) {
                    existing.projects.push(proj.clone());
                }
            }
        } else {
            memory.patterns.push(new);
        }
    }

    memory.stats.total_patterns = memory.patterns.len();
    memory.updated = Utc::now();
}

/// Generate project hash from working directory path
/// (mirrors Claude Code convention: replace / with -)
pub fn project_hash(working_dir: &str) -> String {
    working_dir
        .replace(['/', '\\'], "-")
        .trim_start_matches('-')
        .to_string()
}

fn uuid_short() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_dirs() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();
        assert!(dir.path().join(STEWARDSHIP_DIR).join(PROJECTS_DIR).exists());
        assert!(dir
            .path()
            .join(STEWARDSHIP_DIR)
            .join("proposals")
            .join("pending")
            .exists());
        assert!(dir
            .path()
            .join(STEWARDSHIP_DIR)
            .join("proposals")
            .join("applied")
            .exists());
        assert!(dir.path().join(STEWARDSHIP_DIR).join("logs").exists());
    }

    #[test]
    fn test_cross_project_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut memory = CrossProjectMemory::default();
        memory.learnings.push("Test learning".to_string());
        memory.patterns.push(CrossProjectPattern {
            id: "test-1".to_string(),
            pattern_type: "workflow".to_string(),
            description: "Test pattern".to_string(),
            occurrences: 3,
            projects: vec!["proj-1".to_string()],
            insight: "Test insight".to_string(),
            first_seen: "2026-02-23".to_string(),
            last_seen: "2026-02-23".to_string(),
        });

        save_cross_project(dir.path(), &memory).unwrap();
        let loaded = load_cross_project(dir.path()).unwrap();

        assert_eq!(loaded.learnings.len(), 1);
        assert_eq!(loaded.patterns.len(), 1);
        assert_eq!(loaded.patterns[0].insight, "Test insight");
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let memory = load_cross_project(dir.path()).unwrap();
        assert_eq!(memory.version, "1.0");
        assert!(memory.patterns.is_empty());
    }

    #[test]
    fn test_merge_patterns_dedup() {
        let mut memory = CrossProjectMemory::default();
        let p1 = CrossProjectPattern {
            id: "p1".to_string(),
            pattern_type: "workflow".to_string(),
            description: "Same pattern".to_string(),
            occurrences: 2,
            projects: vec!["proj-1".to_string()],
            insight: "insight".to_string(),
            first_seen: "2026-01-01".to_string(),
            last_seen: "2026-01-01".to_string(),
        };
        memory.patterns.push(p1);

        let new = vec![CrossProjectPattern {
            id: "p2".to_string(),
            pattern_type: "workflow".to_string(),
            description: "Same pattern".to_string(),
            occurrences: 3,
            projects: vec!["proj-2".to_string()],
            insight: "insight".to_string(),
            first_seen: "2026-02-01".to_string(),
            last_seen: "2026-02-01".to_string(),
        }];

        merge_patterns(&mut memory, new);
        assert_eq!(memory.patterns.len(), 1); // Merged, not added
        assert_eq!(memory.patterns[0].occurrences, 5); // 2 + 3
        assert_eq!(memory.patterns[0].projects.len(), 2); // Both projects
    }

    #[test]
    fn test_project_hash() {
        assert_eq!(
            project_hash("/Users/james/projects/my-app"),
            "Users-james-projects-my-app"
        );
    }

    #[test]
    fn test_path_traversal_rejected() {
        let dir = tempfile::TempDir::new().unwrap();

        // Path traversal in project_hash
        let mut analysis = SessionAnalysis {
            session_id: "s1".to_string(),
            project_hash: "../../etc".to_string(),
            transcript_path: std::path::PathBuf::from("/tmp/test.jsonl"),
            analyzed_at: Utc::now(),
            message_count: 1,
            estimated_tokens: 100,
            estimated_context_pct: 0.01,
            decisions: vec![],
            files_touched: vec![],
            tool_patterns: vec![],
            duplicate_regions: vec![],
            rot_candidates: vec![],
            key_insights: vec![],
        };
        assert!(save_session_analysis(dir.path(), &analysis).is_err());

        // Path traversal in session_id
        analysis.project_hash = "safe-hash".to_string();
        analysis.session_id = "../../../etc/passwd".to_string();
        assert!(save_session_analysis(dir.path(), &analysis).is_err());

        // Load with traversal should fail
        assert!(load_project_memory(dir.path(), "../../etc").is_err());
    }

    #[test]
    fn test_session_analysis_save_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let analysis = SessionAnalysis {
            session_id: "s1".to_string(),
            project_hash: "proj-hash".to_string(),
            transcript_path: std::path::PathBuf::from("/tmp/test.jsonl"),
            analyzed_at: Utc::now(),
            message_count: 10,
            estimated_tokens: 5000,
            estimated_context_pct: 0.025,
            decisions: vec![],
            files_touched: vec!["src/main.rs".to_string()],
            tool_patterns: vec![],
            duplicate_regions: vec![],
            rot_candidates: vec![],
            key_insights: vec!["test".to_string()],
        };

        save_session_analysis(dir.path(), &analysis).unwrap();
        let path = dir
            .path()
            .join(STEWARDSHIP_DIR)
            .join(PROJECTS_DIR)
            .join("proj-hash")
            .join("sessions")
            .join("s1.yaml");
        assert!(path.exists());
    }
}
