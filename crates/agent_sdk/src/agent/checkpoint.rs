use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::tools::{ToolExecutor, ToolKind, ToolSpec};
use crate::runtime_vars::{CheckpointEnv, CheckpointMode};
use crate::structured::StructuredOutputContract;

pub(crate) fn build_checkpoint_schema(
    checkpoint_env: &CheckpointEnv,
    structured_output: Option<&StructuredOutputContract>,
) -> Value {
    let mut properties = Map::new();
    properties.insert("emit".to_string(), json!({
        "type": "array",
        "description": "Internal-only events for logging/diagnostics (not a user-visible message).",
        "items": {
            "type": "object",
            "properties": {
                "level": { "type": "string", "enum": ["debug","info","warn","error"], "default": "info" },
                "code": { "type": "string", "minLength": 1, "description": "Short stable identifier, e.g. delegate.start" },
                "message": { "type": "string", "minLength": 1 },
                "data": { "type": "object", "description": "Optional structured payload.", "additionalProperties": true }
            },
            "required": ["code","message"],
            "additionalProperties": false
        }
    }));
    properties.insert("internal_state".to_string(), json!({
        "type": "object",
        "description": "Internal-only snapshot; may optionally be mirrored into task metadata for polling.",
        "properties": {
            "stage": { "type": "string", "minLength": 1, "description": "e.g. discovering|delegating|waiting_external|synthesizing|waiting_input|done|failed" },
            "progress": { "type": "integer", "minimum": 0, "maximum": 100 },
            "note": { "type": "string" }
        },
        "additionalProperties": false
    }));

    if checkpoint_env.mode != CheckpointMode::StateOnly {
        properties.insert("task_patch".to_string(), json!({
            "type": "object",
            "description": "Patch to persist into durable task state so pollers can observe progress.",
            "properties": {
                "state": { "type": "string", "enum": ["working","input_required","completed","failed","canceled","rejected"] },
                "status_text": { "type": "string", "description": "Human-readable status persisted with the task state." },
                "meta": { "type": "object", "description": "Task metadata for pollers (stage/progress/errors/etc).", "additionalProperties": true },
                "append_history_text": { "type": "string", "description": "Optional assistant message to append to task history." },
                "artifacts": {
                    "type": "array",
                    "description": "Optional durable outputs to attach as task artifacts.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "minLength": 1 },
                            "description": { "type": "string" },
                            "json": { "description": "JSON payload to store as a data artifact." }
                        },
                        "required": ["name","json"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["state"],
            "additionalProperties": false
        }));
    }

    if checkpoint_env.mode == CheckpointMode::ResponseControl {
        let mut respond_enum = vec!["task"];
        if checkpoint_env.allow_message_response {
            respond_enum.push("message");
        }
        properties.insert("respond".to_string(), json!({
            "type": "object",
            "description": "If provided, finalize immediately with a single synchronous response.",
            "properties": {
                "kind": { "type": "string", "enum": respond_enum },
                "message_text": { "type": "string", "description": "Required if kind=message." },
                "final_text": { "type": "string", "description": "Optional final assistant text (prefer task_patch.append_history_text)." }
            },
            "required": ["kind"],
            "additionalProperties": false
        }));
    }

    if let Some(contract) = structured_output {
        properties.insert("structured_output".to_string(), json!({
            "type": "object",
            "description": format!(
                "Optional structured final payload for schema '{}'. When present, payload must match the configured schema.",
                contract.schema_name
            ),
            "properties": {
                "payload": contract.schema.clone(),
                "text": {
                    "type": "string",
                    "description": "Optional human-readable summary for task history."
                }
            },
            "additionalProperties": false
        }));
    }

    Value::Object(Map::from_iter([
        ("type".to_string(), json!("object")),
        ("properties".to_string(), Value::Object(properties)),
        ("required".to_string(), json!([])),
        ("additionalProperties".to_string(), json!(false)),
    ]))
}

pub(crate) fn checkpoint_tool_spec(
    checkpoint_env: &CheckpointEnv,
    structured_output: Option<&StructuredOutputContract>,
) -> ToolSpec {
    ToolSpec {
        name: "checkpoint_task".to_string(),
        description: Some(match structured_output {
            Some(contract) => format!(
                "Checkpoint progress and/or finalize the current response. \
Use this to set explicit task state, attach artifacts, and provide a status message. \
If the run completes successfully, include structured_output.payload matching schema '{}'.",
                contract.schema_name
            ),
            None => "Checkpoint progress and/or finalize the current response. \
Use this to set explicit task state, attach artifacts, and provide a status message."
                .to_string(),
        }),
        parameters: build_checkpoint_schema(checkpoint_env, structured_output),
        kind: ToolKind::Function,
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(|args| {
            Box::pin(async move {
                Ok(json!({
                    "__engine_stop": true,
                    "__checkpoint_args": args,
                }))
            })
        })),
    }
}
