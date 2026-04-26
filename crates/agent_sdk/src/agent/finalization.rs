//! Runtime finalization helpers for sentinel-tool driven completion.
//!
//! These helpers interpret `checkpoint_task` stop-signal arguments into
//! runtime responses and can force a follow-up finalization turn when the
//! model needs to produce structured output.

use super::llm_invoker::{LlmInvoker, LlmPolicy};
use crate::agent::tools::ToolRegistry;
use crate::agent::{MessageContext, TaskContext};
use crate::agent::{Response, RuntimeArtifact, RuntimeResponse, TaskOpts};
use crate::error::{SdkError, SdkResult};
use crate::runtime_vars::CheckpointMode;
use crate::structured::StructuredOutputContract;
use agent_core::{ContentPart, TaskPhase};
use log::debug;
use std::sync::Arc;

/// Map a sentinel-task label to the runtime task phase.
pub(super) fn map_task_phase(state: &str) -> Option<TaskPhase> {
    match state {
        "working" | "input_required" => Some(TaskPhase::Working),
        "completed" => Some(TaskPhase::Completed),
        "failed" | "rejected" => Some(TaskPhase::Failed),
        "canceled" => Some(TaskPhase::Cancelled),
        _ => None,
    }
}

/// Tool schemas exposing only the sentinel finalization tool.
fn build_finalization_tool_schemas(tools: &ToolRegistry) -> Vec<llm_client::ToolSchema> {
    match tools.get("checkpoint_task") {
        Some(spec) => vec![llm_client::ToolSchema {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters: spec.parameters.clone(),
            strict: if spec.strict { Some(true) } else { None },
        }],
        None => Vec::new(),
    }
}

/// Build a runtime response from parsed sentinel finalization arguments.
pub(super) fn build_response_from_finalization_args(
    args: serde_json::Value,
    msg_ctx: &MessageContext,
    task_ctx: Option<TaskContext>,
    policy: &LlmPolicy,
    structured_output_contract: Option<&StructuredOutputContract>,
) -> SdkResult<RuntimeResponse> {
    let respond_kind = args
        .get("respond")
        .and_then(|v| v.get("kind"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(kind) = respond_kind.as_deref() {
        if kind == "message" {
            if policy.checkpoint_mode != CheckpointMode::ResponseControl
                || !policy.checkpoint_allow_message_response
            {
                debug!(
                    "checkpoint_task: respond.kind=message not allowed by policy; coercing to Task"
                );
            } else {
                let text = args
                    .get("respond")
                    .and_then(|v| v.get("message_text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if text.is_empty() {
                    return Err(SdkError::invalid_input(
                        "respond.kind=message requires respond.message_text",
                    ));
                }
                return Response::message_text(
                    text,
                    None,
                    None,
                    task_ctx.and_then(|t| t.context_id),
                );
            }
        }
    }

    let task_patch = if policy.checkpoint_mode == CheckpointMode::StateOnly {
        serde_json::Value::Null
    } else {
        args.get("task_patch")
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    let task_phase = task_patch
        .get("state")
        .and_then(|v| v.as_str())
        .and_then(map_task_phase);

    let status_text = task_patch
        .get("status_text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let structured_output_value = args.get("structured_output");
    let structured_payload = structured_output_value
        .and_then(|value| value.get("payload"))
        .cloned();
    let structured_text = structured_output_value
        .and_then(|value| value.get("text"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());

    if structured_output_value.is_some() && structured_payload.is_none() {
        return Err(SdkError::invalid_input(
            "structured_output.payload is required when structured_output is provided",
        ));
    }

    let append_history_text = task_patch
        .get("append_history_text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or(structured_text)
        .or_else(|| {
            args.get("respond")
                .and_then(|v| v.get("final_text"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    let mut task_meta: Option<std::collections::HashMap<String, serde_json::Value>> = None;
    if let Some(obj) = task_patch.get("meta").and_then(|v| v.as_object()) {
        let mut m = std::collections::HashMap::new();
        for (k, v) in obj {
            m.insert(k.clone(), v.clone());
        }
        task_meta = Some(m);
    }

    if policy.checkpoint_mirror_internal_state_to_task_meta {
        if let Some(internal_state) = args.get("internal_state") {
            let mut base = task_meta.unwrap_or_default();
            if !base.contains_key("internal_state") {
                base.insert("internal_state".to_string(), internal_state.clone());
            }
            task_meta = Some(base);
        }
    }

    let mut artifacts: Vec<RuntimeArtifact> = Vec::new();
    if let Some(arr) = task_patch.get("artifacts").and_then(|v| v.as_array()) {
        for a in arr {
            let name = a
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            artifacts.push(RuntimeArtifact {
                name,
                description: a
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                data: a.get("json").cloned().unwrap_or(serde_json::Value::Null),
            });
        }
    }

    let state = if let Some(s) = task_phase {
        Some(s)
    } else if respond_kind.as_deref() == Some("task")
        || append_history_text
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    {
        Some(TaskPhase::Completed)
    } else {
        Some(TaskPhase::Working)
    };

    if let Some(contract) = structured_output_contract {
        match structured_payload {
            Some(payload) => {
                contract.validate_payload(&payload)?;
                artifacts.push(RuntimeArtifact {
                    name: contract.artifact_name.clone(),
                    description: Some(format!(
                        "Structured output payload for schema '{}'",
                        contract.schema_name
                    )),
                    data: payload,
                });
            }
            None if contract.required && state == Some(TaskPhase::Completed) => {
                return Err(SdkError::invalid_input(format!(
                    "structured_output.payload is required for completed responses using schema '{}'",
                    contract.schema_name
                )));
            }
            None => {}
        }
    }

    let history_parts = append_history_text.and_then(|t| {
        if t.is_empty() {
            None
        } else {
            Some(vec![ContentPart::Text(t)])
        }
    });

    Response::task(
        TaskOpts {
            artifacts,
            state,
            status_text,
            task_meta,
            history_parts,
        },
        msg_ctx,
        task_ctx,
    )
}

/// Run a single finalization-tool-only LLM turn and extract the arguments.
pub(super) async fn run_finalization_turn(
    llm: Arc<dyn LlmInvoker>,
    model: &str,
    tools: &ToolRegistry,
    mut messages: Vec<llm_client::ChatMessage>,
    post_mortem: &str,
    structured_output_contract: Option<&StructuredOutputContract>,
) -> Result<serde_json::Value, String> {
    let structured_instruction = structured_output_contract
        .map(|contract| {
            format!(
                "\n- If you complete successfully, include structured_output.payload matching schema '{}'. You may also set structured_output.text with a concise human-readable summary.",
                contract.schema_name
            )
        })
        .unwrap_or_default();
    let instruction = format!(
        "You must now call checkpoint_task.\n\
        - If you are done, set task_patch.state='completed' and include append_history_text (and artifacts if any), and set respond.kind='task'.\n\
        - If you need more input, set task_patch.state='input_required' with status_text asking for the missing info, and set respond.kind='task'.\n\
        - If you cannot proceed due to an error, set task_patch.state='failed' with status_text explaining the error and next steps, and set respond.kind='task'.{} \n\
        Context: {}",
        structured_instruction, post_mortem
    );
    messages.push(llm_client::ChatMessage {
        role: "user".into(),
        content: Some(instruction),
        ..Default::default()
    });
    let tool_schemas = build_finalization_tool_schemas(tools);
    let mut parallel_ext = serde_json::Map::new();
    parallel_ext.insert(
        "parallel_tool_calls".to_string(),
        serde_json::Value::Bool(false),
    );
    let request = llm_client::LlmRequest {
        model: model.to_string(),
        messages,
        tools: Some(tool_schemas),
        tool_choice: Some(llm_client::ToolChoice::Auto),
        extensions: Some(parallel_ext),
        ..Default::default()
    };
    let raw = llm
        .request(request)
        .await
        .map_err(|e| format!("LLM request failed in finalization turn: {}", e))?;
    let tool_calls = raw
        .choices
        .first()
        .and_then(|c| c.message.tool_calls.as_ref())
        .into_iter()
        .flat_map(|t| t.iter());
    for tc in tool_calls {
        if tc.name == "checkpoint_task" {
            return Ok(tc.arguments.clone());
        }
    }
    Err("checkpoint_task was not called".to_string())
}
