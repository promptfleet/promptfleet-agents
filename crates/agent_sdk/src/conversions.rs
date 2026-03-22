//! Bidirectional conversions between `agent_core` and `a2a_protocol_core` types.
//!
//! Rust's orphan rule prevents `From` impls between two external crates, so we
//! provide standalone conversion functions. The convenience methods on
//! `MessageContext` and `TaskContext` call these internally.

use a2a_protocol_core::data::artifact::Artifact;
use a2a_protocol_core::data::message::{Message, MessageRole, Part};
use a2a_protocol_core::data::task::{Task, TaskState};
use agent_core::{AgentMessage, ContentPart, ConversationContext, Role, TaskPhase};
use uuid::Uuid;

// ─── Role ↔ MessageRole ────────────────────────────────────────────────

/// Convert `agent_core::Role` → `a2a_protocol_core::MessageRole`.
pub fn role_to_a2a(role: &Role) -> MessageRole {
    match role {
        Role::User => MessageRole::User,
        Role::Agent => MessageRole::Agent,
        Role::System => MessageRole::User,
    }
}

/// Convert `a2a_protocol_core::MessageRole` → `agent_core::Role`.
pub fn role_from_a2a(role: &MessageRole) -> Role {
    match role {
        MessageRole::User => Role::User,
        MessageRole::Agent => Role::Agent,
        MessageRole::Unspecified => Role::User,
    }
}

// ─── ContentPart ↔ Part ────────────────────────────────────────────────

/// Convert `agent_core::ContentPart` → `a2a_protocol_core::Part` (v1.0 flat Part).
pub fn content_part_to_a2a(part: ContentPart) -> Part {
    match part {
        ContentPart::Text(text) => Part::text(text),
        ContentPart::Data(value) => Part::data(value),
        ContentPart::File { uri, mime, .. } => {
            if let Some(media_type) = mime {
                Part::url_with_media(uri, media_type)
            } else {
                Part::url(uri)
            }
        }
    }
}

/// Convert `a2a_protocol_core::Part` (v1.0 flat) → `agent_core::ContentPart`.
pub fn content_part_from_a2a(part: Part) -> ContentPart {
    if let Some(text) = part.text {
        ContentPart::Text(text)
    } else if let Some(data) = part.data {
        ContentPart::Data(data)
    } else if let Some(uri) = part.url {
        ContentPart::File {
            uri,
            mime: part.media_type,
            data: None,
        }
    } else if let Some(raw) = part.raw {
        ContentPart::Data(serde_json::Value::String(raw))
    } else {
        ContentPart::Text(String::new())
    }
}

// ─── AgentMessage ↔ Message ────────────────────────────────────────────

/// Convert `agent_core::AgentMessage` → `a2a_protocol_core::Message`.
pub fn agent_message_to_a2a(msg: AgentMessage) -> Message {
    let role = role_to_a2a(&msg.role);
    let parts: Vec<Part> = msg.parts.into_iter().map(content_part_to_a2a).collect();
    Message::with_id(Uuid::new_v4().to_string(), role, parts)
}

/// Convert `a2a_protocol_core::Message` → `agent_core::AgentMessage`.
pub fn agent_message_from_a2a(msg: Message) -> AgentMessage {
    let role = role_from_a2a(&msg.role);
    let parts: Vec<ContentPart> = msg.parts.into_iter().map(content_part_from_a2a).collect();
    AgentMessage::new(role, parts)
}

// ─── TaskPhase ↔ TaskState ─────────────────────────────────────────────

/// Convert `agent_core::TaskPhase` → `a2a_protocol_core::TaskState`.
pub fn task_phase_to_a2a(phase: &TaskPhase) -> TaskState {
    match phase {
        TaskPhase::Pending => TaskState::Submitted,
        TaskPhase::Working => TaskState::Working,
        TaskPhase::Completed => TaskState::Completed,
        TaskPhase::Failed => TaskState::Failed,
        TaskPhase::Cancelled => TaskState::Canceled,
    }
}

/// Convert `a2a_protocol_core::TaskState` → `agent_core::TaskPhase`.
pub fn task_phase_from_a2a(state: &TaskState) -> TaskPhase {
    match state {
        TaskState::Submitted => TaskPhase::Pending,
        TaskState::Working => TaskPhase::Working,
        TaskState::InputRequired => TaskPhase::Working,
        TaskState::Completed => TaskPhase::Completed,
        TaskState::Failed => TaskPhase::Failed,
        TaskState::Canceled => TaskPhase::Cancelled,
        TaskState::Rejected => TaskPhase::Failed,
        TaskState::Unspecified => TaskPhase::Pending,
        TaskState::AuthRequired => TaskPhase::Working,
    }
}

// ─── Convenience: MessageContext / TaskContext from agent_core types ────

use crate::agent::message::{MessageContext, TaskContext};
use crate::agent::response::RuntimeArtifact;

impl MessageContext {
    /// Build a `MessageContext` from a protocol-agnostic `AgentMessage`.
    ///
    /// This is the primary entry point for consumers that don't want to
    /// import `a2a_protocol_core` types.
    pub fn from_agent_message(msg: AgentMessage) -> Self {
        Self::from_runtime_message(msg, Default::default(), Default::default(), false, None)
    }

    /// Convenience: build from plain text (user role).
    ///
    /// ```rust,ignore
    /// let ctx = MessageContext::from_text("What's the weather?");
    /// ```
    pub fn from_text(text: &str) -> Self {
        Self::from_agent_message(AgentMessage::user_text(text))
    }
}

impl TaskContext {
    /// Build a `TaskContext` from a protocol-agnostic `ConversationContext`.
    pub fn from_conversation(ctx: ConversationContext) -> Self {
        let runtime_history = ctx.history;

        Self {
            task_id: String::new(),
            context_id: None,
            runtime_history,
            task_phase: ctx.task_phase.clone(),
            artifacts: Vec::new(),
            task_metadata: ctx.metadata,
            created_at: None,
            updated_at: None,
            continuation: None,
        }
    }

    /// Convenience: build from simple text turns.
    ///
    /// ```rust,ignore
    /// let ctx = TaskContext::from_text_turns(&[
    ///     ("user", "hello"),
    ///     ("assistant", "hi there"),
    /// ]);
    /// ```
    pub fn from_text_turns(turns: &[(&str, &str)]) -> Self {
        Self::from_conversation(ConversationContext::from_text_turns(turns))
    }
}

pub fn runtime_artifact_from_a2a(artifact: Artifact) -> Option<RuntimeArtifact> {
    let name = artifact
        .name
        .clone()
        .unwrap_or_else(|| "artifact".to_string());
    let description = artifact.description.clone();
    if let Some(data) = artifact.get_data_content().cloned() {
        Some(RuntimeArtifact {
            name,
            description,
            data,
        })
    } else {
        artifact.get_text_content().map(|text| RuntimeArtifact {
            name,
            description,
            data: serde_json::Value::String(text.to_string()),
        })
    }
}

pub fn task_context_from_a2a_task(task: &Task, context_id: String) -> TaskContext {
    let runtime_history = task
        .history
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(agent_message_from_a2a)
        .collect();

    TaskContext {
        task_id: task.id.clone(),
        context_id: Some(context_id),
        runtime_history,
        task_phase: task_phase_from_a2a(&task.status.state),
        // Canonical artifacts stay in task storage; continuation hydration keeps
        // runtime state compact and avoids cloning artifact payloads on every turn.
        artifacts: Vec::new(),
        task_metadata: task.metadata.clone().unwrap_or_default(),
        created_at: task.status.timestamp.clone(),
        updated_at: task.status.timestamp.clone(),
        continuation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_roundtrip_user() {
        let role = Role::User;
        let a2a = role_to_a2a(&role);
        let back = role_from_a2a(&a2a);
        assert_eq!(back, role);
    }

    #[test]
    fn role_roundtrip_agent() {
        let role = Role::Agent;
        let a2a = role_to_a2a(&role);
        let back = role_from_a2a(&a2a);
        assert_eq!(back, role);
    }

    #[test]
    fn system_role_maps_to_user() {
        let a2a = role_to_a2a(&Role::System);
        assert_eq!(a2a, MessageRole::User);
    }

    #[test]
    fn text_part_roundtrip() {
        let part = ContentPart::Text("hello".into());
        let a2a = content_part_to_a2a(part.clone());
        let back = content_part_from_a2a(a2a);
        assert_eq!(back, part);
    }

    #[test]
    fn data_part_roundtrip() {
        let part = ContentPart::Data(serde_json::json!({"key": "value"}));
        let a2a = content_part_to_a2a(part.clone());
        let back = content_part_from_a2a(a2a);
        assert_eq!(back, part);
    }

    #[test]
    fn agent_message_to_a2a_converts_correctly() {
        let msg = AgentMessage::user_text("hello world");
        let a2a = agent_message_to_a2a(msg);
        assert_eq!(a2a.role, MessageRole::User);
        assert_eq!(a2a.get_text_content(), "hello world");
        assert!(!a2a.message_id.is_empty());
    }

    #[test]
    fn a2a_message_to_agent_message_converts_correctly() {
        let a2a = Message::text(MessageRole::Agent, "response", "task-1".to_string());
        let msg = agent_message_from_a2a(a2a);
        assert_eq!(msg.role, Role::Agent);
        assert_eq!(msg.text_content(), Some("response".to_string()));
    }

    #[test]
    fn task_phase_roundtrip() {
        for phase in [
            TaskPhase::Pending,
            TaskPhase::Working,
            TaskPhase::Completed,
            TaskPhase::Failed,
            TaskPhase::Cancelled,
        ] {
            let a2a = task_phase_to_a2a(&phase);
            let back = task_phase_from_a2a(&a2a);
            assert_eq!(back, phase);
        }
    }

    #[test]
    fn message_context_from_text() {
        use crate::agent::message::MessageType;
        let ctx = MessageContext::from_text("hello");
        assert_eq!(ctx.message_type, MessageType::Text);
        assert_eq!(ctx.text_content, Some("hello".to_string()));
    }

    #[test]
    fn task_context_from_text_turns() {
        let ctx = TaskContext::from_text_turns(&[("user", "hi"), ("assistant", "hello")]);
        assert_eq!(ctx.runtime_history.len(), 2);
        assert_eq!(ctx.task_phase, TaskPhase::Working);
    }

    #[test]
    fn task_context_from_conversation() {
        let conv = ConversationContext::new(vec![
            AgentMessage::user_text("question"),
            AgentMessage::agent_text("answer"),
        ]);
        let ctx = TaskContext::from_conversation(conv);
        assert_eq!(ctx.runtime_history.len(), 2);
    }
}
