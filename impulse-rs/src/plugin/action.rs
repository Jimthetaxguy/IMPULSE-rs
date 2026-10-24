// Action plugin types and helpers

use super::{PluginInput, PluginOutput};

/// Status of an action execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    RolledBack,
}

/// An action execution record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionExecution {
    pub id: String,
    pub handler_name: String,
    pub status: ActionStatus,
    pub input: PluginInput,
    pub output: Option<PluginOutput>,
    pub error: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl ActionExecution {
    pub fn new(id: String, handler_name: String, input: PluginInput) -> Self {
        Self {
            id,
            handler_name,
            status: ActionStatus::Pending,
            input,
            output: None,
            error: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    pub fn mark_running(&mut self) {
        self.status = ActionStatus::Running;
    }

    pub fn mark_completed(&mut self, output: PluginOutput) {
        self.status = ActionStatus::Completed;
        self.output = Some(output);
        self.completed_at = Some(chrono::Utc::now());
    }

    pub fn mark_failed(&mut self, error: String) {
        self.status = ActionStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(chrono::Utc::now());
    }

    pub fn mark_rolled_back(&mut self) {
        self.status = ActionStatus::RolledBack;
        self.completed_at = Some(chrono::Utc::now());
    }
}

/// Action validation result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn valid() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execution() -> ActionExecution {
        ActionExecution::new("exec-1".into(), "formatter".into(), PluginInput::new())
    }

    #[test]
    fn test_action_execution_lifecycle_sets_status_output_and_completion() {
        let mut run = execution();
        assert_eq!(run.status, ActionStatus::Pending);
        assert!(run.completed_at.is_none());
        run.mark_running();
        assert_eq!(run.status, ActionStatus::Running);
        assert!(run.completed_at.is_none(), "running is not complete");
        run.mark_completed(PluginOutput::new("done"));
        assert_eq!(run.status, ActionStatus::Completed);
        assert_eq!(
            run.output.as_ref().map(|o| o.content.as_str()),
            Some("done")
        );
        assert!(run.completed_at.unwrap() >= run.started_at);
    }

    #[test]
    fn test_action_execution_failure_and_rollback_record_completion() {
        let mut failed = execution();
        failed.mark_failed("disk full".into());
        assert_eq!(failed.status, ActionStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("disk full"));
        assert!(failed.completed_at.is_some());

        let mut rolled_back = execution();
        rolled_back.mark_rolled_back();
        assert_eq!(rolled_back.status, ActionStatus::RolledBack);
        assert!(rolled_back.completed_at.is_some());
        assert!(rolled_back.output.is_none());
    }

    #[test]
    fn round_trip_action_execution_and_status_names() {
        let mut original = execution();
        original.mark_completed(PluginOutput::new("ok"));
        let json = serde_json::to_string(&original).unwrap();
        let recovered: ActionExecution = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.id, original.id);
        assert_eq!(recovered.status, original.status);
        assert_eq!(recovered.completed_at, original.completed_at);
        assert_eq!(recovered.output.map(|o| o.content), Some("ok".to_string()));
        assert_eq!(
            serde_json::to_string(&ActionStatus::RolledBack).unwrap(),
            "\"rolled_back\""
        );
    }

    #[test]
    fn test_validation_result_valid_invalid_and_warnings() {
        let ok = ValidationResult::valid().with_warning("slow");
        assert!(ok.valid && ok.errors.is_empty());
        assert_eq!(ok.warnings, vec!["slow".to_string()]);
        let bad = ValidationResult::invalid(vec!["missing path".into()]);
        assert!(!bad.valid);
        assert_eq!(bad.errors.len(), 1);
        let recovered: ValidationResult =
            serde_json::from_str(&serde_json::to_string(&bad).unwrap()).unwrap();
        assert_eq!(recovered.errors, bad.errors);
        assert!(!recovered.valid);
    }
}
