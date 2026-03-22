//! Protocol-agnostic task/conversation context types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::AgentMessage;

/// Simplified task lifecycle phase.
///
/// Maps to A2A `TaskState` and can represent any protocol's task lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskPhase {
    Pending,
    Working,
    Completed,
    Failed,
    Cancelled,
}

impl Default for TaskPhase {
    fn default() -> Self {
        Self::Pending
    }
}

/// Protocol-agnostic conversation context.
///
/// Carries the conversation history and task state needed by the agent's
/// processing loop, without any wire-protocol specifics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    /// Previous messages in the conversation (chronological, oldest first).
    pub history: Vec<AgentMessage>,
    /// Current phase of the task/conversation.
    pub task_phase: TaskPhase,
    /// Arbitrary metadata (protocol-specific fields can be stashed here).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ConversationContext {
    /// Create a new context with history and a working phase.
    pub fn new(history: Vec<AgentMessage>) -> Self {
        Self {
            history,
            task_phase: TaskPhase::Working,
            metadata: HashMap::new(),
        }
    }

    /// Create from simple text turns: `[("user", "hello"), ("assistant", "hi")]`.
    pub fn from_text_turns(turns: &[(&str, &str)]) -> Self {
        let history = turns
            .iter()
            .map(|(role, text)| {
                let role = match *role {
                    "assistant" | "agent" => crate::Role::Agent,
                    "system" => crate::Role::System,
                    _ => crate::Role::User,
                };
                AgentMessage::new(role, vec![crate::ContentPart::Text((*text).to_string())])
            })
            .collect();
        Self::new(history)
    }

    /// Check if the conversation has any history.
    pub fn has_history(&self) -> bool {
        !self.history.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_phase_is_pending() {
        assert_eq!(TaskPhase::default(), TaskPhase::Pending);
    }

    #[test]
    fn new_context_has_working_phase() {
        let ctx = ConversationContext::new(vec![]);
        assert_eq!(ctx.task_phase, TaskPhase::Working);
        assert!(!ctx.has_history());
    }

    #[test]
    fn from_text_turns_builds_history() {
        let ctx = ConversationContext::from_text_turns(&[
            ("user", "hello"),
            ("assistant", "hi there"),
            ("user", "how are you?"),
        ]);
        assert_eq!(ctx.history.len(), 3);
        assert!(ctx.has_history());
        assert_eq!(ctx.history[0].role, crate::Role::User);
        assert_eq!(ctx.history[1].role, crate::Role::Agent);
        assert_eq!(
            ctx.history[1].text_content(),
            Some("hi there".to_string())
        );
    }

    #[test]
    fn serialization_roundtrip() {
        let ctx = ConversationContext::from_text_turns(&[("user", "test")]);
        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: ConversationContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.history.len(), 1);
        assert_eq!(deserialized.task_phase, TaskPhase::Working);
    }
}
