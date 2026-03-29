//! A2A v1.0 Streaming Event Types

use crate::data::{Artifact, Message, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Top-level SSE stream envelope (v1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamResponse {
    Task(crate::data::task::Task),
    Message(Message),
    StatusUpdate(TaskStatusUpdateEvent),
    ArtifactUpdate(TaskArtifactUpdateEvent),
}

/// Streaming status update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusUpdateEvent {
    pub id: Value,
    pub task_id: String,
    pub context_id: String,
    pub status: TaskStatus,
}

/// Streaming artifact update event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactUpdateEvent {
    pub id: Value,
    pub task_id: String,
    pub context_id: String,
    pub artifact: Artifact,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chunk: Option<bool>,
}

impl StreamResponse {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Task(_) => "task",
            Self::Message(_) => "message",
            Self::StatusUpdate(_) => "statusUpdate",
            Self::ArtifactUpdate(_) => "artifactUpdate",
        }
    }

    pub fn to_jsonrpc_data(&self) -> Value {
        match self {
            Self::Task(task) => json!({
                "jsonrpc": "2.0",
                "result": {
                    "task": task,
                }
            }),
            Self::Message(msg) => json!({
                "jsonrpc": "2.0",
                "result": {
                    "message": msg,
                }
            }),
            Self::StatusUpdate(ev) => json!({
                "jsonrpc": "2.0",
                "id": ev.id,
                "result": {
                    "statusUpdate": {
                        "taskId": ev.task_id,
                        "contextId": ev.context_id,
                        "status": ev.status,
                    }
                }
            }),
            Self::ArtifactUpdate(ev) => json!({
                "jsonrpc": "2.0",
                "id": ev.id,
                "result": {
                    "artifactUpdate": {
                        "taskId": ev.task_id,
                        "contextId": ev.context_id,
                        "artifact": ev.artifact,
                        "append": ev.append,
                        "lastChunk": ev.last_chunk,
                    }
                }
            }),
        }
    }

    /// Check whether this event signals stream termination.
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::StatusUpdate(ev) => ev.status.state.is_terminal(),
            Self::ArtifactUpdate(ev) => ev.last_chunk == Some(true),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::message::{Message, MessageRole, Part};
    use crate::data::task::{Task, TaskState, TaskStatus};
    use serde_json::json;

    fn make_task() -> crate::data::task::Task {
        Task::new("ctx-1".to_string())
    }

    fn make_message() -> Message {
        Message::new(
            MessageRole::Agent,
            vec![Part::text("hello")],
            "t-1".to_string(),
        )
    }

    fn make_status_update(state: TaskState) -> TaskStatusUpdateEvent {
        let mut status = TaskStatus::new(state);
        TaskStatusUpdateEvent {
            id: json!("ev-1"),
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            status,
        }
    }

    fn make_artifact_update(
        last_chunk: Option<bool>,
        append: Option<bool>,
    ) -> TaskArtifactUpdateEvent {
        use crate::data::artifact::Artifact;
        TaskArtifactUpdateEvent {
            id: json!("ev-2"),
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            artifact: Artifact::text("result"),
            append,
            last_chunk,
        }
    }

    #[test]
    fn event_name_all_variants() {
        assert_eq!(StreamResponse::Task(make_task()).event_name(), "task");
        assert_eq!(
            StreamResponse::Message(make_message()).event_name(),
            "message"
        );
        assert_eq!(
            StreamResponse::StatusUpdate(make_status_update(TaskState::Working)).event_name(),
            "statusUpdate"
        );
        assert_eq!(
            StreamResponse::ArtifactUpdate(make_artifact_update(None, None)).event_name(),
            "artifactUpdate"
        );
    }

    #[test]
    fn to_jsonrpc_data_task_shape() {
        let data = StreamResponse::Task(make_task()).to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert!(data["result"]["task"].is_object());
    }

    #[test]
    fn to_jsonrpc_data_message_shape() {
        let data = StreamResponse::Message(make_message()).to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert!(data["result"]["message"].is_object());
    }

    #[test]
    fn to_jsonrpc_data_status_update_shape() {
        let ev = make_status_update(TaskState::Completed);
        let data = StreamResponse::StatusUpdate(ev).to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert_eq!(data["id"], "ev-1");
        assert!(data["result"]["statusUpdate"].is_object());
    }

    #[test]
    fn to_jsonrpc_data_artifact_update_shape() {
        let ev = make_artifact_update(Some(true), Some(false));
        let data = StreamResponse::ArtifactUpdate(ev).to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert_eq!(data["id"], "ev-2");
        assert!(data["result"]["artifactUpdate"]["artifact"].is_object());
    }

    #[test]
    fn is_terminal_task_and_message_are_false() {
        assert!(!StreamResponse::Task(make_task()).is_terminal());
        assert!(!StreamResponse::Message(make_message()).is_terminal());
    }

    #[test]
    fn is_terminal_status_update_terminal_states() {
        for state in [
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Canceled,
            TaskState::Rejected,
        ] {
            assert!(
                StreamResponse::StatusUpdate(make_status_update(state.clone())).is_terminal(),
                "{state:?} should be terminal"
            );
        }
        assert!(
            !StreamResponse::StatusUpdate(make_status_update(TaskState::Working)).is_terminal()
        );
    }

    #[test]
    fn is_terminal_artifact_update_last_chunk_flag() {
        assert!(
            StreamResponse::ArtifactUpdate(make_artifact_update(Some(true), None)).is_terminal()
        );
        assert!(
            !StreamResponse::ArtifactUpdate(make_artifact_update(Some(false), None)).is_terminal()
        );
        assert!(!StreamResponse::ArtifactUpdate(make_artifact_update(None, None)).is_terminal());
    }

    #[test]
    fn task_status_update_roundtrip() {
        let ev = make_status_update(TaskState::Working);
        let json = serde_json::to_string(&ev).unwrap();
        let deser: TaskStatusUpdateEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.task_id, "task-1");
        assert_eq!(deser.context_id, "ctx-1");
    }

    #[test]
    fn task_artifact_update_append_and_last_chunk_serialized() {
        let ev = make_artifact_update(Some(true), Some(true));
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["lastChunk"], true);
        assert_eq!(json["append"], true);
    }
}
