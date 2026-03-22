use std::collections::HashMap;

use agent_core::{AgentMessage, ContentPart, Role, TaskPhase};
use serde_json::Value;
use uuid::Uuid;

use super::message::{MessageContext, TaskContext};
use crate::error::{SdkError, SdkResult};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeArtifact {
    pub name: String,
    pub description: Option<String>,
    pub data: Value,
}

impl RuntimeArtifact {
    pub fn data(name: impl Into<String>, data: Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            data,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeMessage {
    pub message: AgentMessage,
    pub context_id: Option<String>,
    pub metadata: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone)]
pub struct RuntimeTask {
    pub task_id: String,
    pub context_id: String,
    pub history: Vec<AgentMessage>,
    pub artifacts: Vec<RuntimeArtifact>,
    pub metadata: HashMap<String, Value>,
    pub phase: TaskPhase,
    pub status_text: Option<String>,
    pub continuation_update: Option<RuntimeContinuationUpdate>,
}

#[derive(Debug, Clone)]
pub enum RuntimeResponse {
    Message(RuntimeMessage),
    Task(RuntimeTask),
}

#[derive(Debug, Clone)]
pub struct RuntimeContinuationUpdate {
    pub strategy_kind: String,
    pub strategy_version: u32,
    pub strategy_composition: Option<Vec<String>>,
    pub payload: Value,
}

impl RuntimeResponse {
    pub fn text_content(&self) -> Option<String> {
        match self {
            RuntimeResponse::Message(runtime) => runtime.message.text_content(),
            RuntimeResponse::Task(runtime) => runtime
                .history
                .last()
                .and_then(|message| message.text_content())
                .or_else(|| runtime.status_text.clone()),
        }
    }
}

/// Options for constructing a runtime task response.
#[derive(Debug, Clone, Default)]
pub struct TaskOpts {
    pub artifacts: Vec<RuntimeArtifact>,
    pub state: Option<TaskPhase>,
    pub status_text: Option<String>,
    pub task_meta: Option<HashMap<String, Value>>,
    pub history_parts: Option<Vec<ContentPart>>,
}

/// Explicit runtime response API.
pub struct Response;

impl Response {
    pub fn message_parts(
        parts: Vec<ContentPart>,
        msg_meta: Option<HashMap<String, Value>>,
        context_id: Option<String>,
    ) -> SdkResult<RuntimeResponse> {
        if parts.is_empty() {
            return Err(SdkError::invalid_input(
                "At least one content part must be provided for a runtime message",
            ));
        }

        Ok(RuntimeResponse::Message(RuntimeMessage {
            message: AgentMessage::new(Role::Agent, parts),
            context_id,
            metadata: msg_meta,
        }))
    }

    pub fn message_text(
        text: impl Into<String>,
        _part_meta: Option<HashMap<String, Value>>,
        msg_meta: Option<HashMap<String, Value>>,
        context_id: Option<String>,
    ) -> SdkResult<RuntimeResponse> {
        Self::message_parts(vec![ContentPart::Text(text.into())], msg_meta, context_id)
    }

    pub fn task(
        opts: TaskOpts,
        msg_ctx: &MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> SdkResult<RuntimeResponse> {
        let (task_id, context_id, mut history, mut metadata) = match task_ctx {
            Some(ctx) => (
                ctx.task_id,
                ctx.context_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                ctx.runtime_history,
                ctx.task_metadata,
            ),
            None => (
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                vec![msg_ctx.runtime_message.clone()],
                HashMap::new(),
            ),
        };

        if let Some(parts) = opts.history_parts {
            if !parts.is_empty() {
                history.push(AgentMessage::new(Role::Agent, parts));
            }
        }

        if let Some(task_meta) = opts.task_meta {
            for (key, value) in task_meta {
                metadata.insert(key, value);
            }
        }

        // Return only the artifacts produced by this turn. Canonical storage can
        // preserve prior artifacts separately without inflating every runtime task.
        let artifacts = opts.artifacts;

        let phase = opts.state.unwrap_or_else(|| {
            if artifacts.is_empty() {
                TaskPhase::Pending
            } else {
                TaskPhase::Completed
            }
        });

        Ok(RuntimeResponse::Task(RuntimeTask {
            task_id,
            context_id,
            history,
            artifacts,
            metadata,
            phase,
            status_text: opts.status_text,
            continuation_update: None,
        }))
    }
}
