//! Protocol-agnostic message types.

use serde::{Deserialize, Serialize};

/// Universal message role used across conversational and tool-calling protocols.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Agent,
    System,
    Tool,
}

/// Multi-modal content part — text, file, or structured data.
///
/// Intentionally a subset of what A2A/MCP support. Protocol-specific
/// fields (metadata, extensions) are added during conversion in `agent_sdk`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum ContentPart {
    /// Plain text content.
    Text(String),
    /// File reference or inline data.
    File {
        /// URI or path to the file.
        uri: String,
        /// MIME type (e.g. "application/pdf").
        #[serde(skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        /// Inline file bytes (base64 in JSON).
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<Vec<u8>>,
    },
    /// Structured JSON data (tool calls, parameters, etc.).
    Data(serde_json::Value),
    /// A tool call requested by an agent message.
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// A tool execution result correlated to a prior tool call.
    ToolResult {
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// Protocol-agnostic agent message.
///
/// The minimal unit of communication: who said it and what they said.
/// No message IDs, no protocol envelopes, no extensions — just content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Who sent the message.
    pub role: Role,
    /// Content parts (text, files, data — can be mixed).
    pub parts: Vec<ContentPart>,
}

impl AgentMessage {
    /// Create a message with explicit role and parts.
    pub fn new(role: Role, parts: Vec<ContentPart>) -> Self {
        Self { role, parts }
    }

    /// Convenience: user text message.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            parts: vec![ContentPart::Text(text.into())],
        }
    }

    /// Convenience: agent text response.
    pub fn agent_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Agent,
            parts: vec![ContentPart::Text(text.into())],
        }
    }

    /// Convenience: system text message.
    pub fn system_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            parts: vec![ContentPart::Text(text.into())],
        }
    }

    /// Convenience: tool result message.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: Option<String>,
        content: impl Into<String>,
        error: Option<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            parts: vec![ContentPart::ToolResult {
                tool_call_id: tool_call_id.into(),
                name,
                content: content.into(),
                error,
            }],
        }
    }

    /// Extract concatenated text content from all text parts.
    ///
    /// Returns `None` if no text parts are present.
    pub fn text_content(&self) -> Option<String> {
        let texts: Vec<&str> = self
            .parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();

        if texts.is_empty() {
            None
        } else {
            Some(texts.join(" "))
        }
    }

    /// Check if the message contains only text parts.
    pub fn is_text_only(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|p| matches!(p, ContentPart::Text(_)))
    }

    /// Check if the message contains any data parts.
    pub fn has_data(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, ContentPart::Data(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_creates_correct_message() {
        let msg = AgentMessage::user_text("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.parts.len(), 1);
        assert_eq!(msg.text_content(), Some("hello".to_string()));
        assert!(msg.is_text_only());
    }

    #[test]
    fn agent_text_creates_correct_message() {
        let msg = AgentMessage::agent_text("response");
        assert_eq!(msg.role, Role::Agent);
        assert_eq!(msg.text_content(), Some("response".to_string()));
    }

    #[test]
    fn mixed_parts_detected() {
        let msg = AgentMessage::new(
            Role::User,
            vec![
                ContentPart::Text("context".into()),
                ContentPart::Data(serde_json::json!({"key": "value"})),
            ],
        );
        assert!(!msg.is_text_only());
        assert!(msg.has_data());
        assert_eq!(msg.text_content(), Some("context".to_string()));
    }

    #[test]
    fn empty_text_content_returns_none() {
        let msg = AgentMessage::new(Role::User, vec![ContentPart::Data(serde_json::json!(42))]);
        assert_eq!(msg.text_content(), None);
    }

    #[test]
    fn serialization_roundtrip() {
        let msg = AgentMessage::user_text("hello world");
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn multiple_text_parts_concatenated() {
        let msg = AgentMessage::new(
            Role::User,
            vec![
                ContentPart::Text("first".into()),
                ContentPart::Text("second".into()),
            ],
        );
        assert_eq!(msg.text_content(), Some("first second".to_string()));
    }
}
