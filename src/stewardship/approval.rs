use anyhow::{bail, Result};
use std::path::Path;

use super::types::*;

const STEWARDSHIP_DIR: &str = "stewardship";
const PENDING_DIR: &str = "proposals/pending";
const APPLIED_DIR: &str = "proposals/applied";

/// Validate that an ID is safe for use as a filename component.
/// Rejects path traversal sequences and non-alphanumeric characters
/// (except hyphens and underscores).
fn sanitize_id(id: &str) -> Result<&str> {
    if id.is_empty() {
        bail!("ID must not be empty");
    }
    if id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        Ok(id)
    } else {
        bail!(
            "Invalid ID '{}': must contain only alphanumeric, hyphen, or underscore characters",
            id
        );
    }
}

/// Queue a proposal for user approval
pub fn queue_proposal(base_path: &Path, proposal: &CleanupProposal) -> Result<()> {
    let dir = base_path.join(STEWARDSHIP_DIR).join(PENDING_DIR);
    std::fs::create_dir_all(&dir)?;

    let path = dir.join(format!("{}.yaml", proposal.id));
    let yaml = serde_yaml::to_string(proposal)?;
    super::atomic_write_file(&path, yaml.as_bytes())?;
    Ok(())
}

/// List all pending proposals
pub fn list_pending(base_path: &Path) -> Result<Vec<CleanupProposal>> {
    let dir = base_path.join(STEWARDSHIP_DIR).join(PENDING_DIR);

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut proposals = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "yaml").unwrap_or(false) {
            let content = std::fs::read_to_string(&path)?;
            if let Ok(proposal) = serde_yaml::from_str::<CleanupProposal>(&content) {
                proposals.push(proposal);
            }
        }
    }

    proposals.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(proposals)
}

/// Approve a proposal (move from pending to applied)
pub fn approve_proposal(base_path: &Path, proposal_id: &str) -> Result<bool> {
    let safe_id = sanitize_id(proposal_id)?;
    let pending_path = base_path
        .join(STEWARDSHIP_DIR)
        .join(PENDING_DIR)
        .join(format!("{}.yaml", safe_id));

    if !pending_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&pending_path)?;
    let mut proposal: CleanupProposal = serde_yaml::from_str(&content)?;
    proposal.status = ProposalStatus::Approved;

    let applied_dir = base_path.join(STEWARDSHIP_DIR).join(APPLIED_DIR);
    std::fs::create_dir_all(&applied_dir)?;

    let applied_path = applied_dir.join(format!("{}.yaml", safe_id));
    let yaml = serde_yaml::to_string(&proposal)?;
    super::atomic_write_file(&applied_path, yaml.as_bytes())?;
    std::fs::remove_file(&pending_path)?;

    Ok(true)
}

/// Reject a proposal (remove from pending)
pub fn reject_proposal(base_path: &Path, proposal_id: &str) -> Result<bool> {
    let safe_id = sanitize_id(proposal_id)?;
    let pending_path = base_path
        .join(STEWARDSHIP_DIR)
        .join(PENDING_DIR)
        .join(format!("{}.yaml", safe_id));

    if !pending_path.exists() {
        return Ok(false);
    }

    std::fs::remove_file(&pending_path)?;
    Ok(true)
}

/// Auto-apply all proposals (used in auto mode)
pub fn auto_apply(base_path: &Path, proposals: Vec<CleanupProposal>) -> Result<Vec<String>> {
    let mut applied_ids = Vec::new();
    for proposal in &proposals {
        queue_proposal(base_path, proposal)?;
        approve_proposal(base_path, &proposal.id)?;
        applied_ids.push(proposal.id.clone());
    }
    Ok(applied_ids)
}

/// Process proposals based on stewardship mode
pub fn process_proposals(
    base_path: &Path,
    proposals: Vec<CleanupProposal>,
    mode: StewardshipMode,
) -> Result<ProcessResult> {
    match mode {
        StewardshipMode::Auto => {
            let applied = auto_apply(base_path, proposals)?;
            let count = applied.len();
            Ok(ProcessResult {
                mode,
                applied,
                queued: 0,
                logged: count,
            })
        }
        StewardshipMode::Review => {
            let count = proposals.len();
            for proposal in &proposals {
                queue_proposal(base_path, proposal)?;
            }
            Ok(ProcessResult {
                mode,
                applied: Vec::new(),
                queued: count,
                logged: count,
            })
        }
        StewardshipMode::Off => {
            // Just log the count, don't create proposals
            Ok(ProcessResult {
                mode,
                applied: Vec::new(),
                queued: 0,
                logged: proposals.len(),
            })
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::cross_project::ensure_dirs;
    use super::*;
    use chrono::Utc;

    fn make_proposal(id: &str) -> CleanupProposal {
        CleanupProposal {
            id: id.to_string(),
            created_at: Utc::now(),
            session_id: "test-session".to_string(),
            threshold: ThresholdLevel::Surgical,
            strategy: CleanupStrategy::Deduplicate,
            estimated_tokens_freed: 500,
            regions: vec![ProposalRegion {
                description: "Test region".to_string(),
                message_indices: vec![1, 2, 3],
                estimated_tokens: 500,
            }],
            preserves: vec!["First call".to_string()],
            status: ProposalStatus::Pending,
        }
    }

    #[test]
    fn test_queue_and_list_proposals() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let p1 = make_proposal("p1");
        let p2 = make_proposal("p2");

        queue_proposal(dir.path(), &p1).unwrap();
        queue_proposal(dir.path(), &p2).unwrap();

        let pending = list_pending(dir.path()).unwrap();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_approve_proposal() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let p = make_proposal("approve-test");
        queue_proposal(dir.path(), &p).unwrap();

        assert!(approve_proposal(dir.path(), "approve-test").unwrap());

        // Should be gone from pending
        let pending = list_pending(dir.path()).unwrap();
        assert!(pending.is_empty());

        // Should exist in applied
        let applied_path = dir
            .path()
            .join(STEWARDSHIP_DIR)
            .join(APPLIED_DIR)
            .join("approve-test.yaml");
        assert!(applied_path.exists());
    }

    #[test]
    fn test_reject_proposal() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let p = make_proposal("reject-test");
        queue_proposal(dir.path(), &p).unwrap();

        assert!(reject_proposal(dir.path(), "reject-test").unwrap());
        assert!(list_pending(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_approve_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        assert!(!approve_proposal(dir.path(), "nonexistent").unwrap());
    }

    #[test]
    fn test_path_traversal_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        // Path traversal attempts should be rejected
        assert!(approve_proposal(dir.path(), "../../etc/passwd").is_err());
        assert!(reject_proposal(dir.path(), "../malicious").is_err());
        assert!(approve_proposal(dir.path(), "id with spaces").is_err());
        assert!(approve_proposal(dir.path(), "").is_err());

        // Valid IDs should work (return false because proposal doesn't exist)
        assert!(!approve_proposal(dir.path(), "valid-id_123").unwrap());
    }

    #[test]
    fn test_process_auto_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let proposals = vec![make_proposal("auto-1"), make_proposal("auto-2")];
        let result = process_proposals(dir.path(), proposals, StewardshipMode::Auto).unwrap();

        assert_eq!(result.applied.len(), 2);
        assert_eq!(result.queued, 0);
        assert!(list_pending(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_process_review_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let proposals = vec![make_proposal("review-1")];
        let result = process_proposals(dir.path(), proposals, StewardshipMode::Review).unwrap();

        assert!(result.applied.is_empty());
        assert_eq!(result.queued, 1);
        assert_eq!(list_pending(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn test_process_off_mode() {
        let dir = tempfile::TempDir::new().unwrap();
        ensure_dirs(dir.path()).unwrap();

        let proposals = vec![make_proposal("off-1")];
        let result = process_proposals(dir.path(), proposals, StewardshipMode::Off).unwrap();

        assert!(result.applied.is_empty());
        assert_eq!(result.queued, 0);
        assert_eq!(result.logged, 1);
        assert!(list_pending(dir.path()).unwrap().is_empty());
    }
}
