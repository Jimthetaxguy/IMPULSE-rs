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
