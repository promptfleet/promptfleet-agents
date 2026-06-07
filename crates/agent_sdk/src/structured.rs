//! Protocol-free structured input/output helpers for typed agent execution.

use std::any::type_name;
use std::collections::HashMap;

use agent_core::{AgentMessage, ContentPart, Role};
use jsonschema::JSONSchema;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::agent::{MessageContext, RuntimeResponse, SkillExecutor};
use crate::{SdkError, SdkResult};

pub use crate::events::{
    CloudEventEnvelope, DataschemaConvention, EventError as CloudEventError, envelope_schema,
    payload_schema, promptfleet_dataschema_uri,
};

/// Protocol-free typed input for structured agent execution.
#[derive(Debug, Clone)]
pub struct StructuredInput<T> {
    pub payload: T,
    pub cloud_event: Option<CloudEventEnvelope<T>>,
    pub metadata: Option<Value>,
}

impl<T> StructuredInput<T> {
    pub fn from_payload(payload: T) -> Self {
        Self {
            payload,
            cloud_event: None,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl<T: Clone> StructuredInput<T> {
    pub fn from_cloudevent(cloud_event: CloudEventEnvelope<T>) -> Result<Self, CloudEventError> {
        let payload = cloud_event
            .data
            .clone()
            .ok_or(CloudEventError::MissingData)?;
        Ok(Self {
            payload,
            cloud_event: Some(cloud_event),
            metadata: None,
        })
    }
}

impl<T> StructuredInput<T>
where
    T: Serialize,
{
    pub(crate) fn into_message_context(
        self,
        skill_executor: Option<SkillExecutor>,
        stateless_mode: bool,
    ) -> SdkResult<MessageContext> {
        let mut user_metadata = HashMap::new();
        if let Some(metadata) = self.metadata {
            user_metadata.insert("structured_input_metadata".to_string(), metadata);
        }
        if let Some(cloud_event) = self.cloud_event {
            user_metadata.insert(
                "cloud_event".to_string(),
                serde_json::to_value(cloud_event)?,
            );
        }

        let runtime_message = AgentMessage::new(
            Role::User,
            vec![ContentPart::Data(serde_json::to_value(self.payload)?)],
        );

        Ok(MessageContext::from_runtime_message(
            runtime_message,
            HashMap::new(),
            user_metadata,
            stateless_mode,
            skill_executor,
        ))
    }
}

/// Schema-constrained final output contract for typed agent execution.
#[derive(Debug, Clone)]
pub struct StructuredOutputContract {
    pub schema_name: String,
    pub artifact_name: String,
    pub schema: Value,
    pub dataschema: Option<String>,
    pub strict: bool,
    pub required: bool,
}

impl StructuredOutputContract {
    pub fn new(
        schema_name: impl Into<String>,
        artifact_name: impl Into<String>,
        schema: Value,
    ) -> Self {
        let schema_name = schema_name.into();
        Self {
            dataschema: Some(promptfleet_dataschema_uri(&schema_name)),
            schema_name,
            artifact_name: artifact_name.into(),
            schema,
            strict: true,
            required: true,
        }
    }

    pub fn from_type<T>(schema_name: impl Into<String>, artifact_name: impl Into<String>) -> Self
    where
        T: JsonSchema,
    {
        Self::new(schema_name, artifact_name, payload_schema::<T>())
    }

    pub fn for_type<T>() -> Self
    where
        T: JsonSchema,
    {
        Self::from_type::<T>(type_name::<T>(), "structured_output")
    }

    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    pub fn with_dataschema(mut self, dataschema: impl Into<String>) -> Self {
        self.dataschema = Some(dataschema.into());
        self
    }

    pub fn without_dataschema(mut self) -> Self {
        self.dataschema = None;
        self
    }

    pub fn with_dataschema_convention(mut self, convention: DataschemaConvention) -> Self {
        self.dataschema = Some(convention.uri_for(&self.schema_name));
        self
    }

    pub fn with_promptfleet_dataschema(self) -> Self {
        self.with_dataschema_convention(DataschemaConvention::PromptfleetUrn)
    }

    pub fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn compiled_validator(&self) -> SdkResult<JSONSchema> {
        JSONSchema::compile(&self.schema).map_err(|error| {
            SdkError::invalid_input(format!(
                "Invalid structured output schema '{}': {}",
                self.schema_name, error
            ))
        })
    }

    pub fn validate_payload(&self, payload: &Value) -> SdkResult<()> {
        if !self.strict {
            return Ok(());
        }

        let validator = self.compiled_validator()?;
        match validator.validate(payload) {
            Ok(()) => Ok(()),
            Err(errors) => Err(SdkError::invalid_input(format!(
                "Structured output validation failed for '{}': {}",
                self.schema_name,
                errors
                    .map(|err| err.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            ))),
        }
    }
}

/// Typed structured run result plus the underlying runtime response.
#[derive(Debug, Clone)]
pub struct StructuredRunResult<O> {
    pub output: O,
    pub artifact_name: String,
    pub dataschema: Option<String>,
    pub raw_response: RuntimeResponse,
    pub final_text: Option<String>,
}

impl<O> StructuredRunResult<O>
where
    O: Serialize,
{
    pub fn into_cloud_event(
        self,
        event_type: impl Into<String>,
        source: impl Into<String>,
    ) -> CloudEventEnvelope<O> {
        let event = CloudEventEnvelope::new_json(event_type, source, self.output);
        match self.dataschema {
            Some(dataschema) => event.with_dataschema(dataschema),
            None => event,
        }
    }
}

fn missing_artifact_error(artifact_name: &str) -> SdkError {
    SdkError::method_execution(
        "structured_output",
        format!(
            "structured output artifact '{}' was not produced",
            artifact_name
        ),
    )
}

fn missing_artifact_error_for_response(
    response: &RuntimeResponse,
    artifact_name: &str,
) -> SdkError {
    let RuntimeResponse::Task(task) = response else {
        return missing_artifact_error(artifact_name);
    };

    let mut message = format!(
        "structured output artifact '{}' was not produced; task phase: {:?}",
        artifact_name, task.phase
    );
    if let Some(status_text) = task
        .status_text
        .as_deref()
        .filter(|status_text| !status_text.trim().is_empty())
    {
        message.push_str("; task status: ");
        message.push_str(status_text);
    }

    SdkError::method_execution("structured_output", message)
}

pub(crate) fn decode_optional_artifact<O>(
    response: &RuntimeResponse,
    artifact_name: &str,
) -> SdkResult<Option<O>>
where
    O: DeserializeOwned,
{
    let RuntimeResponse::Task(task) = response else {
        return Err(SdkError::method_execution(
            "structured_output",
            "structured execution expected a task response carrying artifacts",
        ));
    };

    let artifact = task
        .artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.name == artifact_name);

    let Some(artifact) = artifact else {
        return Ok(None);
    };

    serde_json::from_value(artifact.data.clone())
        .map(Some)
        .map_err(|error| {
            SdkError::method_execution(
                "structured_output",
                format!(
                    "failed to decode structured output artifact '{}': {}",
                    artifact_name, error
                ),
            )
        })
}

pub(crate) fn decode_artifact<O>(response: &RuntimeResponse, artifact_name: &str) -> SdkResult<O>
where
    O: DeserializeOwned,
{
    decode_optional_artifact(response, artifact_name)?
        .ok_or_else(|| missing_artifact_error_for_response(response, artifact_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
    struct ExampleOutput {
        verdict: String,
    }

    #[test]
    fn contract_uses_type_schema() {
        let contract = StructuredOutputContract::for_type::<ExampleOutput>();
        assert_eq!(contract.artifact_name, "structured_output");
        assert_eq!(contract.schema["type"], "object");
        assert_eq!(
            contract.dataschema.as_deref(),
            Some(promptfleet_dataschema_uri(type_name::<ExampleOutput>()).as_str())
        );
    }

    #[test]
    fn cloud_event_input_preserves_payload() {
        let event = CloudEventEnvelope::new_json(
            "com.example.test",
            "urn:test",
            ExampleOutput {
                verdict: "ok".to_string(),
            },
        );
        let input = StructuredInput::from_cloudevent(event).expect("input");
        assert_eq!(
            input.payload,
            ExampleOutput {
                verdict: "ok".to_string(),
            }
        );
    }

    #[test]
    fn structured_result_can_be_wrapped_as_cloud_event() {
        let result = StructuredRunResult {
            output: ExampleOutput {
                verdict: "ok".to_string(),
            },
            artifact_name: "analysis_output".to_string(),
            dataschema: Some(promptfleet_dataschema_uri("analysis_output")),
            raw_response: RuntimeResponse::Task(crate::agent::RuntimeTask {
                task_id: "task-1".to_string(),
                context_id: "ctx-1".to_string(),
                history: Vec::new(),
                artifacts: Vec::new(),
                metadata: HashMap::new(),
                phase: agent_core::TaskPhase::Completed,
                status_text: None,
                continuation_update: None,
            }),
            final_text: Some("done".to_string()),
        };

        let event = result.into_cloud_event("com.example.analysis", "urn:test");
        assert_eq!(event.event_type, "com.example.analysis");
        assert_eq!(event.source, "urn:test");
        assert_eq!(event.datacontenttype.as_deref(), Some("application/json"));
        assert_eq!(
            event.dataschema.as_deref(),
            Some("urn:promptfleet:schema:analysis_output")
        );
    }

    #[test]
    fn decode_optional_artifact_prefers_last_matching_artifact() {
        let response = RuntimeResponse::Task(crate::agent::RuntimeTask {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            history: Vec::new(),
            artifacts: vec![
                crate::agent::RuntimeArtifact {
                    name: "analysis_output".to_string(),
                    description: None,
                    data: serde_json::json!({ "verdict": 42 }),
                },
                crate::agent::RuntimeArtifact {
                    name: "analysis_output".to_string(),
                    description: None,
                    data: serde_json::json!({ "verdict": "ok" }),
                },
            ],
            metadata: HashMap::new(),
            phase: agent_core::TaskPhase::Completed,
            status_text: None,
            continuation_update: None,
        });

        let decoded: ExampleOutput =
            decode_artifact(&response, "analysis_output").expect("decode last artifact");
        assert_eq!(
            decoded,
            ExampleOutput {
                verdict: "ok".to_string()
            }
        );
    }

    #[test]
    fn decode_artifact_reports_failed_task_status_when_required_artifact_missing() {
        let response = RuntimeResponse::Task(crate::agent::RuntimeTask {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            history: Vec::new(),
            artifacts: Vec::new(),
            metadata: HashMap::new(),
            phase: agent_core::TaskPhase::Failed,
            status_text: Some(
                "Network error: Reqwest error: error sending request for url".to_string(),
            ),
            continuation_update: None,
        });

        let error = decode_artifact::<ExampleOutput>(&response, "analysis_output")
            .expect_err("missing required artifact should fail");
        let message = error.to_string();
        assert!(message.contains("structured output artifact 'analysis_output' was not produced"));
        assert!(message.contains("task phase: Failed"));
        assert!(message.contains("Network error: Reqwest error"));
    }
}
