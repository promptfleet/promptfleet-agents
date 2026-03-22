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

/// Build a tools array exposing only the sentinel finalization tool.
fn build_finalization_tools_json(tools: &ToolRegistry) -> Vec<serde_json::Value> {
    match tools.get("checkpoint_task") {
        Some(spec) => vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": spec.name,
                "description": spec.description,
                "parameters": spec.parameters
            }
        })],
        None => Vec::new(),
    }
}

/// Build a runtime response from parsed sentinel finalization arguments.
pub(super) fn build_response_from_finalization_args(
    args: serde_json::Value,
    msg_ctx: &MessageContext,
    task_ctx: Option<TaskContext>,
    policy: &LlmPolicy,
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

    let append_history_text = task_patch
        .get("append_history_text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
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
    mut messages: Vec<serde_json::Value>,
    post_mortem: &str,
) -> Result<serde_json::Value, String> {
    let instruction = format!(
        "You must now call checkpoint_task.\n\
        - If you are done, set task_patch.state='completed' and include append_history_text (and artifacts if any), and set respond.kind='task'.\n\
        - If you need more input, set task_patch.state='input_required' with status_text asking for the missing info, and set respond.kind='task'.\n\
        - If you cannot proceed due to an error, set task_patch.state='failed' with status_text explaining the error and next steps, and set respond.kind='task'.\n\
        Context: {}",
        post_mortem
    );
    messages.push(serde_json::json!({"role":"user","content": instruction }));
    let tools_json = build_finalization_tools_json(tools);
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools_json,
        "tool_choice": "auto",
        "parallel_tool_calls": false
    });
    let raw = llm
        .request(payload)
        .await
        .map_err(|e| format!("LLM request failed in finalization turn: {}", e))?;
    let maybe_msg = raw
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|ch| ch.get("message"));
    let tool_calls = maybe_msg
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for tc in tool_calls {
        let name = tc
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name == "checkpoint_task" {
            let args_str = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(args_str) {
                return Ok(val);
            }
        }
    }
    Err("checkpoint_task was not called".to_string())
}
