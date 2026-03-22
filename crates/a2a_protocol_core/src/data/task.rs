//! A2A v1.0 Task Lifecycle Management

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

#[cfg(feature = "time-stamps")]
use chrono::Utc;

/// **Task**: Core A2A work unit (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub context_id: String,
    pub status: TaskStatus,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<crate::data::message::Message>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<crate::data::artifact::Artifact>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

/// **Task Status**: Current state with optional message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub state: TaskState,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<crate::data::message::Message>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// **Task State**: A2A v1.0 SCREAMING_SNAKE enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
}

impl TaskState {
    /// Returns `true` for terminal states: Completed, Failed, Canceled, Rejected.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
        )
    }

    /// Returns `true` for active states: Submitted, Working, InputRequired.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            TaskState::Submitted | TaskState::Working | TaskState::InputRequired
        )
    }

    /// Lowercase string for logging/display (not wire format).
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Unspecified => "unspecified",
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input_required",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Canceled => "canceled",
            TaskState::Rejected => "rejected",
            TaskState::AuthRequired => "auth_required",
        }
    }
}

impl Task {
    pub fn new(context_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            context_id,
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    pub fn with_id(id: String, context_id: String) -> Self {
        Self {
            id,
            context_id,
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    pub fn update_status(&mut self, state: TaskState) {
        self.status.state = state;
        self.status.update_timestamp();
    }

    pub fn update_status_with_message(
        &mut self,
        state: TaskState,
        message: crate::data::message::Message,
    ) {
        self.status.state = state;
        self.status.message = Some(message);
        self.status.update_timestamp();
    }

    pub fn add_artifact(&mut self, artifact: crate::data::artifact::Artifact) {
        self.artifacts.get_or_insert_with(Vec::new).push(artifact);
    }

    pub fn add_to_history(&mut self, message: crate::data::message::Message) {
        self.history.get_or_insert_with(Vec::new).push(message);
    }

    pub fn set_metadata(&mut self, key: String, value: Value) {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key, value);
    }

    pub fn is_terminal(&self) -> bool {
        self.status.state.is_terminal()
    }

    pub fn is_active(&self) -> bool {
        self.status.state.is_active()
    }
}

impl TaskStatus {
    pub fn new(state: TaskState) -> Self {
        Self {
            state,
            message: None,
            timestamp: Some(Self::current_timestamp()),
        }
    }

    pub fn update_timestamp(&mut self) {
        self.timestamp = Some(Self::current_timestamp());
    }

    fn current_timestamp() -> String {
        #[cfg(feature = "time-stamps")]
        {
            Utc::now().to_rfc3339()
        }
        #[cfg(not(feature = "time-stamps"))]
        {
            "timestamp-feature-disabled".to_string()
        }
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_task_creation() {
        let task = Task::new("context-123".to_string());
        assert!(!task.id.is_empty());
        assert_eq!(task.context_id, "context-123");
        assert_eq!(task.status.state, TaskState::Submitted);
        assert!(task.is_active());
        assert!(!task.is_terminal());
    }

    #[test]
    fn test_task_state_screaming_snake() {
        let working: TaskState = serde_json::from_str("\"TASK_STATE_WORKING\"").unwrap();
        assert_eq!(working, TaskState::Working);
        let completed: TaskState = serde_json::from_str("\"TASK_STATE_COMPLETED\"").unwrap();
        assert_eq!(completed, TaskState::Completed);
        assert!(completed.is_terminal());
    }

    #[test]
    fn test_task_state_is_terminal() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(TaskState::Rejected.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(!TaskState::Submitted.is_terminal());
        assert!(!TaskState::AuthRequired.is_terminal());
    }

    #[test]
    fn test_task_serialization_camel_case() {
        let task = Task::new("ctx".to_string());
        let json = serde_json::to_value(&task).unwrap();
        assert!(json.get("contextId").is_some());
        assert_eq!(json["status"]["state"], "TASK_STATE_SUBMITTED");
    }

    #[test]
    fn test_task_status_updates() {
        let mut task = Task::new("context-123".to_string());
        task.update_status(TaskState::Working);
        assert_eq!(task.status.state, TaskState::Working);
        task.update_status(TaskState::Completed);
        assert!(task.is_terminal());
    }

    #[test]
    fn test_task_metadata() {
        let mut task = Task::new("context-123".to_string());
        task.set_metadata("priority".to_string(), json!("high"));
        assert_eq!(task.metadata.as_ref().unwrap()["priority"], "high");
    }
}
