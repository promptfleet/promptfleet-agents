//! A2A v1.0 Artifact System
//!
//! Artifacts now contain `parts: Vec<Part>` (flat Part), matching the spec.

use crate::data::message::Part;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// **Artifact**: Agent output (v1.0 shape)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub parts: Vec<Part>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}

impl Artifact {
    /// Create an artifact with a generated ID.
    pub fn new(parts: Vec<Part>) -> Self {
        Self {
            artifact_id: Uuid::new_v4().to_string(),
            name: None,
            description: None,
            parts,
            metadata: None,
            extensions: None,
        }
    }

    /// Create a simple text artifact.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![Part::text(text)])
    }

    /// Create a data artifact.
    pub fn data(value: Value) -> Self {
        Self::new(vec![Part::data(value)])
    }

    /// Builder: set name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Builder: set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Builder: set artifact_id explicitly.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.artifact_id = id.into();
        self
    }

    pub fn set_metadata(&mut self, key: String, value: Value) {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key, value);
    }

    /// Extract text from the first text part (convenience).
    pub fn get_text_content(&self) -> Option<&str> {
        self.parts.iter().find_map(|p| p.text.as_deref())
    }

    /// Extract data from the first data part (convenience).
    pub fn get_data_content(&self) -> Option<&Value> {
        self.parts.iter().find_map(|p| p.data.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_text_artifact() {
        let a = Artifact::text("summary").with_name("result");
        assert_eq!(a.get_text_content(), Some("summary"));
        assert_eq!(a.name.as_deref(), Some("result"));
    }

    #[test]
    fn test_data_artifact() {
        let data = json!({"key": "value"});
        let a = Artifact::data(data.clone());
        assert_eq!(a.get_data_content(), Some(&data));
    }

    #[test]
    fn test_artifact_serialization_camel_case() {
        let a = Artifact::text("hi").with_name("greeting");
        let json = serde_json::to_value(&a).unwrap();
        assert!(json.get("artifactId").is_some());
        assert!(json.get("parts").is_some());
    }

    #[test]
    fn test_artifact_metadata() {
        let mut a = Artifact::text("content");
        a.set_metadata("priority".to_string(), json!("high"));
        assert_eq!(a.metadata.as_ref().unwrap()["priority"], "high");
    }

    #[test]
    fn test_with_description_builder() {
        let a = Artifact::text("result").with_description("The final output");
        assert_eq!(a.description.as_deref(), Some("The final output"));
    }

    #[test]
    fn test_with_id_builder() {
        let a = Artifact::text("content").with_id("art-fixed-id");
        assert_eq!(a.artifact_id, "art-fixed-id");
    }

    #[test]
    fn test_get_text_content_absent_when_no_text_parts() {
        let a = Artifact::data(json!({"key": "val"}));
        assert!(a.get_text_content().is_none());
    }

    #[test]
    fn test_get_data_content_absent_when_no_data_parts() {
        let a = Artifact::text("hello");
        assert!(a.get_data_content().is_none());
    }

    #[test]
    fn test_multiple_parts_roundtrip() {
        let a = Artifact::new(vec![Part::text("first"), Part::data(json!({"n": 1}))]);
        let json = serde_json::to_string(&a).unwrap();
        let deser: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.parts.len(), 2);
        assert!(deser.parts[0].is_text());
        assert!(deser.parts[1].is_data());
    }
}
