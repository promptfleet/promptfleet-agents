//! A2A v1.0 Request/Response Parameter Types

use crate::data::notification::TaskPushNotificationConfig;
use crate::{A2AError, A2AResult, data::message::Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ── SendMessage ─────────────────────────────────────────────────────

/// Request params for `SendMessage` (was `message/send`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub message: Message,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SendMessageConfiguration>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

/// Optional configuration for `SendMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_output_modes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_push_notification_config: Option<TaskPushNotificationConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,

    #[serde(default)]
    pub return_immediately: bool,
}

/// Response for `SendMessage` — either a Task or a direct Message.
///
/// Serialized as externally-tagged `{"task": {...}}` or `{"message": {...}}`
/// matching the protobuf `oneof` JSON mapping required by A2A v1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SendMessageResponse {
    Task(crate::data::task::Task),
    Message(crate::data::message::Message),
}

// ── GetTask ─────────────────────────────────────────────────────────

/// Request params for `GetTask` (was `tasks/get`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskRequest {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
}

// ── CancelTask ──────────────────────────────────────────────────────

/// Request params for `CancelTask` (was `tasks/cancel`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelTaskRequest {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

// ── ListTasks ───────────────────────────────────────────────────────

/// Request params for `ListTasks` (was `tasks/list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_timestamp_after: Option<String>,

    #[serde(default)]
    pub include_artifacts: bool,
}

/// Response for `ListTasks`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksResponse {
    pub tasks: Vec<crate::data::task::Task>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u32>,
}

// ── SubscribeToTask ─────────────────────────────────────────────────

/// Request params for `SubscribeToTask` (reconnect to existing task SSE).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeToTaskRequest {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

// ── Push Notification CRUD ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskPushNotificationConfigRequest {
    pub config: TaskPushNotificationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTaskPushNotificationConfigRequest {
    pub id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTaskPushNotificationConfigsRequest {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTaskPushNotificationConfigRequest {
    pub id: String,
    pub task_id: String,
}

// ── Backward-compat type aliases ────────────────────────────────────

pub type MessageSendParams = SendMessageRequest;
pub type MessageSendResponse = SendMessageResponse;
pub type TaskGetParams = GetTaskRequest;
pub type TaskCancelParams = CancelTaskRequest;
pub type TaskListParams = ListTasksRequest;
pub type TaskListResult = ListTasksResponse;

// ── Validation / parsing helpers ────────────────────────────────────

impl SendMessageRequest {
    pub fn from_json(params: Value) -> A2AResult<Self> {
        serde_json::from_value(params).map_err(|e| {
            A2AError::invalid_params(
                "SendMessage",
                &format!("Invalid SendMessage parameters: {}", e),
            )
        })
    }

    pub fn validate(&self) -> A2AResult<()> {
        if self.message.parts.is_empty() {
            return Err(A2AError::invalid_params(
                "SendMessage",
                "Message must contain at least one part",
            ));
        }
        Ok(())
    }
}

impl GetTaskRequest {
    pub fn from_json(params: Value) -> A2AResult<Self> {
        serde_json::from_value(params).map_err(|e| {
            A2AError::invalid_params("GetTask", &format!("Invalid GetTask parameters: {}", e))
        })
    }

    pub fn validate(&self) -> A2AResult<()> {
        if self.id.is_empty() {
            return Err(A2AError::invalid_params(
                "GetTask",
                "Task ID cannot be empty",
            ));
        }
        Ok(())
    }
}

impl CancelTaskRequest {
    pub fn from_json(params: Value) -> A2AResult<Self> {
        serde_json::from_value(params).map_err(|e| {
            A2AError::invalid_params(
                "CancelTask",
                &format!("Invalid CancelTask parameters: {}", e),
            )
        })
    }

    pub fn validate(&self) -> A2AResult<()> {
        if self.id.is_empty() {
            return Err(A2AError::invalid_params(
                "CancelTask",
                "Task ID cannot be empty",
            ));
        }
        Ok(())
    }
}

impl ListTasksRequest {
    pub fn from_json(params: Value) -> A2AResult<Self> {
        if params.is_null() {
            return Ok(Self::default());
        }
        serde_json::from_value(params).map_err(|e| {
            A2AError::invalid_params("ListTasks", &format!("Invalid ListTasks parameters: {}", e))
        })
    }

    pub fn validate(&self) -> A2AResult<()> {
        if let Some(ps) = self.page_size {
            if ps == 0 || ps > 1000 {
                return Err(A2AError::invalid_params(
                    "ListTasks",
                    "page_size must be between 1 and 1000",
                ));
            }
        }
        Ok(())
    }
}

impl Default for ListTasksRequest {
    fn default() -> Self {
        Self {
            tenant: None,
            context_id: None,
            status: None,
            page_size: Some(50),
            page_token: None,
            history_length: None,
            status_timestamp_after: None,
            include_artifacts: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::message::{MessageRole, Part};
    use serde_json::json;

    #[test]
    fn test_send_message_request_parsing() {
        let params = json!({
            "message": {
                "role": "ROLE_USER",
                "parts": [{"text": "Hello, agent!"}],
                "messageId": "msg-123"
            }
        });
        let parsed = SendMessageRequest::from_json(params).unwrap();
        assert_eq!(parsed.message.role, MessageRole::User);
        assert_eq!(parsed.message.parts.len(), 1);
    }

    #[test]
    fn test_send_message_request_validation() {
        let msg = Message::new(
            MessageRole::User,
            vec![Part::text("Hello")],
            "task-123".to_string(),
        );
        let req = SendMessageRequest {
            message: msg,
            tenant: None,
            configuration: None,
            metadata: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_get_task_request() {
        let params = json!({"id": "task-123"});
        let parsed = GetTaskRequest::from_json(params).unwrap();
        assert_eq!(parsed.id, "task-123");
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn test_list_tasks_request_default() {
        let req = ListTasksRequest::default();
        assert_eq!(req.page_size, Some(50));
    }

    #[test]
    fn test_cancel_task_request() {
        let params = json!({"id": "task-456"});
        let parsed = CancelTaskRequest::from_json(params).unwrap();
        assert_eq!(parsed.id, "task-456");
        assert!(parsed.validate().is_ok());
    }
}
