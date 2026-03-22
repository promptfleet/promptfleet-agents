//! A2A v1.0 Streaming Event Types

use crate::data::{Artifact, Message, TaskState, TaskStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
