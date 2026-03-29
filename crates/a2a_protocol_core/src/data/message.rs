//! A2A v1.0 Message and Part System
//!
//! Implements the Message and flat Part types per A2A Protocol v1.0.0.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// **Message**: Core A2A communication object (v1.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<Part>,
    pub message_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_task_ids: Option<Vec<String>>,
}

/// **Message Role** (v1.0 — SCREAMING_SNAKE serialization)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    #[serde(rename = "ROLE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

/// **Part**: Flat multi-modal content container (v1.0)
///
/// A Part carries exactly one of: text, raw (base64), url, or structured data.
/// Additional fields (filename, media_type, metadata) annotate the content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

// ── Part constructors ───────────────────────────────────────────────

impl Part {
    /// Text-only part.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            raw: None,
            url: None,
            data: None,
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// Structured data part.
    pub fn data(value: Value) -> Self {
        Self {
            text: None,
            raw: None,
            url: None,
            data: Some(value),
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// URL reference part.
    pub fn url(uri: impl Into<String>) -> Self {
        Self {
            text: None,
            raw: None,
            url: Some(uri.into()),
            data: None,
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// Base64-encoded raw bytes part.
    pub fn raw(base64: impl Into<String>) -> Self {
        Self {
            text: None,
            raw: Some(base64.into()),
            url: None,
            data: None,
            metadata: None,
            filename: None,
            media_type: None,
        }
    }

    /// URL part with media type annotation.
    pub fn url_with_media(uri: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self {
            text: None,
            raw: None,
            url: Some(uri.into()),
            data: None,
            metadata: None,
            filename: None,
            media_type: Some(media_type.into()),
        }
    }

    /// Check which content variant is populated.
    pub fn is_text(&self) -> bool {
        self.text.is_some()
    }
    pub fn is_data(&self) -> bool {
        self.data.is_some()
    }
    pub fn is_url(&self) -> bool {
        self.url.is_some()
    }
    pub fn is_raw(&self) -> bool {
        self.raw.is_some()
    }

    /// Extract text content (if present).
    pub fn get_text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

// ── Message constructors ────────────────────────────────────────────

impl Message {
    pub fn new(role: MessageRole, parts: Vec<Part>, task_id: String) -> Self {
        Self {
            role,
            parts,
            message_id: Uuid::new_v4().to_string(),
            task_id: Some(task_id),
            context_id: None,
            metadata: None,
            extensions: None,
            reference_task_ids: None,
        }
    }

    pub fn with_id(message_id: String, role: MessageRole, parts: Vec<Part>) -> Self {
        Self {
            role,
            parts,
            message_id,
            task_id: None,
            context_id: None,
            metadata: None,
            extensions: None,
            reference_task_ids: None,
        }
    }

    pub fn text(role: MessageRole, text: impl Into<String>, task_id: String) -> Self {
        Self::new(role, vec![Part::text(text)], task_id)
    }

    pub fn status(text: impl Into<String>, task_id: String) -> Self {
        Self::text(MessageRole::Agent, text, task_id)
    }

    pub fn error(text: impl Into<String>, task_id: String) -> Self {
        Self::text(MessageRole::Agent, text, task_id)
    }

    pub fn add_part(&mut self, part: Part) {
        self.parts.push(part);
    }

    pub fn set_metadata(&mut self, key: String, value: Value) {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key, value);
    }

    pub fn with_context(mut self, context_id: String) -> Self {
        self.context_id = Some(context_id);
        self
    }

    pub fn add_extension(&mut self, extension: String) {
        self.extensions.get_or_insert_with(Vec::new).push(extension);
    }

    pub fn get_text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn is_text_only(&self) -> bool {
        self.parts.iter().all(|p| p.is_text())
    }

    pub fn get_data_parts(&self) -> Vec<&Part> {
        self.parts.iter().filter(|p| p.is_data()).collect()
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::Unspecified => write!(f, "unspecified"),
            MessageRole::User => write!(f, "user"),
            MessageRole::Agent => write!(f, "agent"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_text_message_creation() {
        let message = Message::text(MessageRole::User, "Hello, agent!", "task-123".to_string());
        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.parts.len(), 1);
        assert_eq!(message.get_text_content(), "Hello, agent!");
        assert!(message.is_text_only());
    }

    #[test]
    fn test_flat_part_constructors() {
        let tp = Part::text("hello");
        assert!(tp.is_text());
        assert_eq!(tp.get_text(), Some("hello"));

        let dp = Part::data(json!({"key": "value"}));
        assert!(dp.is_data());

        let up = Part::url("https://example.com/file.pdf");
        assert!(up.is_url());

        let rp = Part::raw("aGVsbG8=");
        assert!(rp.is_raw());
    }

    #[test]
    fn test_message_serialization_camel_case() {
        let msg = Message::text(MessageRole::User, "hi", "t-1".to_string());
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("messageId").is_some());
        assert!(json.get("taskId").is_some());
        assert_eq!(json["role"], "ROLE_USER");
    }

    #[test]
    fn test_role_screaming_snake() {
        let user: MessageRole = serde_json::from_str("\"ROLE_USER\"").unwrap();
        assert_eq!(user, MessageRole::User);
        let agent: MessageRole = serde_json::from_str("\"ROLE_AGENT\"").unwrap();
        assert_eq!(agent, MessageRole::Agent);
        let unspec: MessageRole = serde_json::from_str("\"ROLE_UNSPECIFIED\"").unwrap();
        assert_eq!(unspec, MessageRole::Unspecified);
    }

    #[test]
    fn test_part_roundtrip() {
        let part = Part::text("hello world");
        let json = serde_json::to_string(&part).unwrap();
        let deser: Part = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.text.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_message_metadata() {
        let mut message = Message::text(MessageRole::User, "Test", "task-123".to_string());
        message.set_metadata("priority".to_string(), json!("high"));
        message.add_extension("openai".to_string());
        assert_eq!(message.metadata.as_ref().unwrap()["priority"], "high");
        assert_eq!(message.extensions.as_ref().unwrap()[0], "openai");
    }

    #[test]
    fn test_with_id_constructor() {
        let msg = Message::with_id(
            "fixed-id".to_string(),
            MessageRole::Agent,
            vec![Part::text("hi")],
        );
        assert_eq!(msg.message_id, "fixed-id");
        assert_eq!(msg.role, MessageRole::Agent);
        assert!(msg.task_id.is_none());
    }

    #[test]
    fn test_with_context_chaining() {
        let msg = Message::with_id(
            "m-1".to_string(),
            MessageRole::User,
            vec![Part::text("hello")],
        )
        .with_context("ctx-99".to_string());
        assert_eq!(msg.context_id.as_deref(), Some("ctx-99"));
    }

    #[test]
    fn test_get_text_content_multipart() {
        let msg = Message::new(
            MessageRole::User,
            vec![Part::text("hello"), Part::text("world")],
            "t-1".to_string(),
        );
        assert_eq!(msg.get_text_content(), "hello world");
    }

    #[test]
    fn test_is_text_only_mixed_parts() {
        let text_only = Message::new(
            MessageRole::User,
            vec![Part::text("a"), Part::text("b")],
            "t".to_string(),
        );
        assert!(text_only.is_text_only());

        let mixed = Message::new(
            MessageRole::User,
            vec![Part::text("a"), Part::data(json!({"x": 1}))],
            "t".to_string(),
        );
        assert!(!mixed.is_text_only());
    }

    #[test]
    fn test_get_data_parts_filtering() {
        let msg = Message::new(
            MessageRole::User,
            vec![
                Part::text("label"),
                Part::data(json!({"v": 42})),
                Part::text("outro"),
            ],
            "t".to_string(),
        );
        let data_parts = msg.get_data_parts();
        assert_eq!(data_parts.len(), 1);
        assert_eq!(data_parts[0].data.as_ref().unwrap()["v"], 42);
    }

    #[test]
    fn test_url_with_media_type() {
        let part = Part::url_with_media("https://example.com/img.png", "image/png");
        assert!(part.is_url());
        assert_eq!(part.media_type.as_deref(), Some("image/png"));
        assert_eq!(part.url.as_deref(), Some("https://example.com/img.png"));
    }

    #[test]
    fn test_add_extension_and_read_back() {
        let mut msg = Message::text(MessageRole::User, "hi", "t".to_string());
        msg.add_extension("ext-a".to_string());
        msg.add_extension("ext-b".to_string());
        let exts = msg.extensions.as_ref().unwrap();
        assert_eq!(exts.len(), 2);
        assert_eq!(exts[0], "ext-a");
    }
}
