//! Message Processing Module
//!
//! This module provides message classification, runtime contexts, and routing
//! helpers for agent execution.

use agent_core::{AgentMessage, ContentPart, TaskPhase};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::response::RuntimeArtifact;

/// **Skill Execution Capability for Custom Handlers**
///
/// Provides lightweight skill execution without requiring full agent access.
/// This eliminates the architectural discrepancy between LLM and custom handlers.
#[derive(Debug, Clone)]
pub struct SkillExecutor {
    skill_registry: Arc<crate::agent::skill::SkillRegistry>,
}

impl SkillExecutor {
    /// Create new skill executor with access to agent's skill registry
    pub(crate) fn new(skill_registry: Arc<crate::agent::skill::SkillRegistry>) -> Self {
        Self { skill_registry }
    }

    /// Transport-agnostic execution producing string-first output for prompt/context injection
    pub async fn execute_text(
        &self,
        skill_id: &str,
        parameters: Value,
    ) -> Result<crate::agent::skill::SkillOutput, crate::agent::skill::SkillError> {
        self.skill_registry
            .execute_skill_text(skill_id, &parameters)
            .await
    }

    /// Transport-agnostic execution with optional message/task context for handlers that need it
    pub async fn execute_text_with_ctx(
        &self,
        skill_id: &str,
        parameters: Value,
        message_ctx: MessageContext,
        task_ctx: Option<TaskContext>,
    ) -> Result<crate::agent::skill::SkillOutput, crate::agent::skill::SkillError> {
        let exec_ctx = crate::agent::skill::SkillExecutionContext::new(message_ctx, task_ctx);
        self.skill_registry
            .execute_skill_text_with_ctx(skill_id, &parameters, &exec_ctx)
            .await
    }

    /// Check if a skill exists in the registry
    pub fn has_skill(&self, skill_id: &str) -> bool {
        self.skill_registry
            .get_skill_definitions()
            .contains_key(skill_id)
    }

    /// Get list of available skills
    pub fn list_skills(&self) -> Vec<String> {
        self.skill_registry.list_skills()
    }
}

/// **Message Type Classification**
///
/// Classifies incoming messages based on their Part composition to determine
/// the appropriate handling strategy (tool-like vs conversational vs hybrid).
#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    /// Pure DataPart - structured tool calls
    Data,
    /// Pure TextPart - conversational interactions
    Text,
    /// Pure FilePart - file processing
    #[cfg(feature = "file-handling")]
    File,
    /// Multiple part types - hybrid interactions
    Mixed,
}

/// **Skill Call Information**
///
/// Extracted from DataPart when a structured skill call is detected.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillCall {
    pub skill_id: String,
    pub parameters: serde_json::Value,
}

/// **Message Context for Immediate Processing**
///
/// Contains all the information needed to process an incoming message,
/// with pre-computed classifications and extractions.
#[derive(Debug, Clone)]
pub struct MessageContext {
    /// Protocol-neutral message used by runtime logic.
    pub runtime_message: AgentMessage,
    /// Classified message type (Data/Text/Mixed/File)
    pub message_type: MessageType,
    /// Extracted text content if present
    pub text_content: Option<String>,
    /// Extracted skill call if present
    pub skill_call: Option<SkillCall>,
    /// Skill hints from message part metadata
    pub skill_hints: HashMap<String, serde_json::Value>,
    /// Message-level metadata
    pub user_metadata: HashMap<String, serde_json::Value>,
    /// Stateless mode preference
    pub stateless_mode: bool,
    /// **NEW: Skill execution capability for custom handlers**
    /// Provides lightweight skill execution without requiring full agent access
    pub skill_executor: Option<SkillExecutor>,
}

/// Durable continuation snapshot metadata surfaced to runtime execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContinuationState {
    pub source_revision: u64,
    pub strategy_kind: String,
    pub strategy_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_composition: Option<Vec<String>>,
    #[serde(default)]
    pub payload: Value,
}

/// **Task Context for Conversation/Session State**
///
/// Contains conversation history and task state information,
/// provided only when conversation continuity is needed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskContext {
    /// Unique task identifier
    pub task_id: String,
    /// Optional conversation context identifier
    pub context_id: Option<String>,
    /// Previous messages in conversation in protocol-neutral form.
    pub runtime_history: Vec<AgentMessage>,
    /// Current task phase in protocol-neutral form.
    pub task_phase: TaskPhase,
    /// Task artifacts in runtime form.
    pub artifacts: Vec<RuntimeArtifact>,
    /// Task-level metadata
    pub task_metadata: HashMap<String, serde_json::Value>,
    /// Task creation timestamp (from status.timestamp)
    pub created_at: Option<String>,
    /// Task last update timestamp (from status.timestamp)
    pub updated_at: Option<String>,
    /// Durable derived continuation metadata from the runtime snapshot store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ContinuationState>,
}

/// **Message Classification and Context Utilities**
impl MessageContext {
    /// Classify message type based on runtime content parts.
    pub fn classify_runtime_message(message: &AgentMessage) -> MessageType {
        let has_text = message
            .parts
            .iter()
            .any(|p| matches!(p, ContentPart::Text(_)));
        let has_data = message
            .parts
            .iter()
            .any(|p| matches!(p, ContentPart::Data(_)));

        #[cfg(feature = "file-handling")]
        let has_file = message
            .parts
            .iter()
            .any(|p| matches!(p, ContentPart::File { .. }));
        #[cfg(not(feature = "file-handling"))]
        let has_file = false;

        match (has_text, has_data, has_file) {
            (false, true, false) => MessageType::Data,
            (true, false, false) => MessageType::Text,
            #[cfg(feature = "file-handling")]
            (false, false, true) => MessageType::File,
            _ => MessageType::Mixed,
        }
    }

    /// Extract skill call information from runtime data parts.
    pub fn extract_runtime_skill_call(message: &AgentMessage) -> Option<SkillCall> {
        for part in &message.parts {
            if let ContentPart::Data(data) = part {
                if let Some(skill_id) = data.get("skill").and_then(|v| v.as_str()) {
                    let mut parameters = serde_json::Map::new();
                    if let Some(obj) = data.as_object() {
                        for (key, value) in obj {
                            if key != "skill" {
                                parameters.insert(key.clone(), value.clone());
                            }
                        }
                    }

                    return Some(SkillCall {
                        skill_id: skill_id.to_string(),
                        parameters: serde_json::Value::Object(parameters),
                    });
                }
            }
        }

        None
    }

    /// Extract text content from runtime message parts.
    pub fn extract_runtime_text_content(message: &AgentMessage) -> Option<String> {
        let text_parts: Vec<&str> = message
            .parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();

        if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(" "))
        }
    }

    /// Build a runtime-native `MessageContext`.
    pub fn from_runtime_message(
        runtime_message: AgentMessage,
        skill_hints: HashMap<String, serde_json::Value>,
        user_metadata: HashMap<String, serde_json::Value>,
        stateless_mode: bool,
        skill_executor: Option<SkillExecutor>,
    ) -> Self {
        let message_type = Self::classify_runtime_message(&runtime_message);
        let text_content = Self::extract_runtime_text_content(&runtime_message);
        let skill_call = Self::extract_runtime_skill_call(&runtime_message);

        Self {
            runtime_message,
            message_type,
            text_content,
            skill_call,
            skill_hints,
            user_metadata,
            stateless_mode,
            skill_executor,
        }
    }
}

impl TaskContext {
    /// Create new task context with basic ISO 8601 timestamp
    pub fn create_new(context_id: Option<String>) -> Self {
        let ctx_id = context_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Basic ISO 8601 timestamp
        let timestamp = format!(
            "{}Z",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| {
                    let secs = d.as_secs();
                    // Basic ISO 8601 format (simplified)
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                        1970,
                        1,
                        1,
                        0,
                        0,
                        secs % 86400
                    ) // Simplified for now
                })
                .unwrap_or_else(|_| "1970-01-01T00:00:00".to_string())
        );

        Self {
            task_id: uuid::Uuid::new_v4().to_string(),
            context_id: Some(ctx_id),
            runtime_history: Vec::new(),
            task_phase: TaskPhase::Pending,
            artifacts: Vec::new(),
            task_metadata: HashMap::new(),
            created_at: Some(timestamp.clone()),
            updated_at: Some(timestamp),
            continuation: None,
        }
    }
}
