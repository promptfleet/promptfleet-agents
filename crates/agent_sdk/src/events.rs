//! CloudEvents helpers exposed through `agent_sdk` to avoid a separate release
//! dependency for the structured I/O surface.

use std::collections::BTreeMap;

use chrono::Utc;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const PROMPTFLEET_DATASCHEMA_URN_PREFIX: &str = "urn:promptfleet:schema:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataschemaConvention {
    PromptfleetUrn,
}

impl DataschemaConvention {
    pub fn uri_for(self, schema_name: &str) -> String {
        match self {
            Self::PromptfleetUrn => promptfleet_dataschema_uri(schema_name),
        }
    }
}

pub fn promptfleet_dataschema_uri(schema_name: &str) -> String {
    format!(
        "{}{}",
        PROMPTFLEET_DATASCHEMA_URN_PREFIX,
        canonical_schema_segment(schema_name)
    )
}

fn canonical_schema_segment(schema_name: &str) -> String {
    let trimmed = schema_name.trim();
    if trimmed.is_empty() {
        return "structured_output".to_string();
    }

    trimmed
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ':' | '/' => ch,
            _ => '-',
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("CloudEvent payload is missing")]
    MissingData,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloudEventEnvelope<T> {
    pub specversion: String,
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datacontenttype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataschema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl<T> CloudEventEnvelope<T> {
    pub fn new_json(event_type: impl Into<String>, source: impl Into<String>, data: T) -> Self {
        Self {
            specversion: "1.0".to_string(),
            id: Uuid::new_v4().to_string(),
            source: source.into(),
            event_type: event_type.into(),
            time: Utc::now().to_rfc3339(),
            subject: None,
            datacontenttype: Some("application/json".to_string()),
            dataschema: None,
            data: Some(data),
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_dataschema(mut self, dataschema: impl Into<String>) -> Self {
        self.dataschema = Some(dataschema.into());
        self
    }

    pub fn with_dataschema_convention(
        mut self,
        convention: DataschemaConvention,
        schema_name: impl AsRef<str>,
    ) -> Self {
        self.dataschema = Some(convention.uri_for(schema_name.as_ref()));
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    pub fn map_data<U, F>(self, f: F) -> CloudEventEnvelope<U>
    where
        F: FnOnce(T) -> U,
    {
        CloudEventEnvelope {
            specversion: self.specversion,
            id: self.id,
            source: self.source,
            event_type: self.event_type,
            time: self.time,
            subject: self.subject,
            datacontenttype: self.datacontenttype,
            dataschema: self.dataschema,
            data: self.data.map(f),
            extensions: self.extensions,
        }
    }

    pub fn into_data(self) -> Option<T> {
        self.data
    }

    pub fn try_into_data(self) -> Result<T, EventError> {
        self.data.ok_or(EventError::MissingData)
    }
}

pub fn payload_schema<T>() -> Value
where
    T: JsonSchema,
{
    let schema = schema_for!(T);
    let mut value = serde_json::to_value(schema).unwrap_or_else(|_| serde_json::json!({"type":"object"}));
    make_self_contained_schema(&mut value);
    value
}

pub fn envelope_schema<T>() -> Value
where
    T: JsonSchema,
{
    let schema = schema_for!(CloudEventEnvelope<T>);
    let mut value = serde_json::to_value(schema).unwrap_or_else(|_| serde_json::json!({"type":"object"}));
    make_self_contained_schema(&mut value);
    value
}

fn make_self_contained_schema(schema: &mut Value) {
    let definitions = schema
        .get("definitions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    inline_local_refs(schema, &definitions, &defs, 0);
    if let Some(object) = schema.as_object_mut() {
        object.remove("definitions");
        object.remove("$defs");
        object.remove("$schema");
    }
}

fn inline_local_refs(
    value: &mut Value,
    definitions: &Map<String, Value>,
    defs: &Map<String, Value>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }

    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if let Some(mut replacement) = resolve_local_ref(reference, definitions, defs) {
                    inline_local_refs(&mut replacement, definitions, defs, depth + 1);
                    merge_ref_siblings(&mut replacement, object);
                    *value = replacement;
                    return;
                }
            }

            for item in object.values_mut() {
                inline_local_refs(item, definitions, defs, depth + 1);
            }
            object.remove("definitions");
            object.remove("$defs");
        }
        Value::Array(items) => {
            for item in items {
                inline_local_refs(item, definitions, defs, depth + 1);
            }
        }
        _ => {}
    }
}

fn resolve_local_ref(
    reference: &str,
    definitions: &Map<String, Value>,
    defs: &Map<String, Value>,
) -> Option<Value> {
    reference
        .strip_prefix("#/definitions/")
        .and_then(|key| definitions.get(key).cloned())
        .or_else(|| {
            reference
                .strip_prefix("#/$defs/")
                .and_then(|key| defs.get(key).cloned())
        })
}

fn merge_ref_siblings(replacement: &mut Value, original: &Map<String, Value>) {
    let Some(replacement) = replacement.as_object_mut() else {
        return;
    };
    for (key, value) in original {
        if key != "$ref" && !replacement.contains_key(key) {
            replacement.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct Payload {
        id: String,
        severity: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct NestedPayload {
        evidence: Vec<Evidence>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct Evidence {
        id: String,
    }

    #[test]
    fn json_round_trip_preserves_extensions() {
        let event = CloudEventEnvelope::new_json(
            "com.example.alert",
            "urn:promptfleet:test",
            Payload {
                id: "a1".to_string(),
                severity: "high".to_string(),
            },
        )
        .with_subject("alert-1")
        .with_dataschema("https://schemas.example.test/alert.json")
        .with_extension("tenant", serde_json::json!("acme"));

        let wire = serde_json::to_value(&event).expect("serialize");
        assert_eq!(wire["type"], "com.example.alert");
        assert_eq!(wire["tenant"], "acme");
        assert_eq!(wire["datacontenttype"], "application/json");

        let parsed: CloudEventEnvelope<Payload> = serde_json::from_value(wire).expect("parse");
        assert_eq!(parsed.subject.as_deref(), Some("alert-1"));
        assert_eq!(
            parsed.extensions.get("tenant"),
            Some(&serde_json::json!("acme"))
        );
        assert_eq!(
            parsed.data,
            Some(Payload {
                id: "a1".to_string(),
                severity: "high".to_string(),
            })
        );
    }

    #[test]
    fn schema_helpers_return_objects() {
        let payload = payload_schema::<Payload>();
        let envelope = envelope_schema::<Payload>();
        assert_eq!(payload["type"], "object");
        assert_eq!(envelope["type"], "object");
        assert!(envelope.get("properties").is_some());
    }

    #[test]
    fn schema_helpers_return_self_contained_schemas() {
        let payload = payload_schema::<NestedPayload>();
        let envelope = envelope_schema::<NestedPayload>();

        assert!(serde_json::to_string(&payload).unwrap().find("$ref").is_none());
        assert!(serde_json::to_string(&envelope).unwrap().find("$ref").is_none());
        assert!(payload.get("definitions").is_none());
        assert!(envelope.get("definitions").is_none());
        jsonschema::JSONSchema::compile(&payload).expect("payload schema compiles");
        jsonschema::JSONSchema::compile(&envelope).expect("envelope schema compiles");
    }

    #[test]
    fn promptfleet_dataschema_convention_is_stable() {
        let uri = promptfleet_dataschema_uri("analysis_output");
        assert_eq!(uri, "urn:promptfleet:schema:analysis_output");

        let event = CloudEventEnvelope::new_json(
            "com.example.alert",
            "urn:promptfleet:test",
            Payload {
                id: "a1".to_string(),
                severity: "high".to_string(),
            },
        )
        .with_dataschema_convention(DataschemaConvention::PromptfleetUrn, "analysis_output");

        assert_eq!(
            event.dataschema.as_deref(),
            Some("urn:promptfleet:schema:analysis_output")
        );
    }

    #[test]
    fn try_into_data_requires_payload() {
        let event: CloudEventEnvelope<Payload> = CloudEventEnvelope {
            specversion: "1.0".to_string(),
            id: "id-1".to_string(),
            source: "urn:test".to_string(),
            event_type: "com.example.empty".to_string(),
            time: "2026-01-01T00:00:00Z".to_string(),
            subject: None,
            datacontenttype: Some("application/json".to_string()),
            dataschema: None,
            data: None,
            extensions: BTreeMap::new(),
        };

        assert!(matches!(
            event.try_into_data(),
            Err(EventError::MissingData)
        ));
    }
}
