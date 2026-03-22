//! Typed interaction tools — ask_question and ask_confirmation.
//!
//! These tools allow the LLM to pause execution and request explicit user
//! input before proceeding. They emit [`AgentTraceEvent::InteractionRequested`]
//! through the [`ToolContext`] event sink and return immediately with a
//! `{ status: "interaction_pending", interaction_id }` result.
//!
//! The run driver detects the pending interaction and emits `run_input_required`
//! instead of `run_finished`, signalling the frontend to show the interaction UI.
//! On the next run, the user's response arrives as a `ContentPart::Data` with
//! key `"interaction_response"`, which the LLM reads from conversation history
//! to continue from where it paused.
//!
//! ## Policy (hardcoded, not exposed to the LLM)
//!
//! - `allow_free_text: true` — options are always suggestions; user can type freely
//! - `allow_cancel: true` — user always has an exit
//! - `timeout_ms: None` — no expiry; system-level concern

use std::sync::Arc;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::agent::tool_context::ToolContext;
use crate::agent::tools::{ToolExecutor, ToolSpec};
use crate::agent::trace::AgentTraceEvent;
use crate::interaction::{InteractionKind, InteractionOption, InteractionRequest};

// ---------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------

fn schema_ask_confirmation() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {
                "type": "string",
                "description": "The approval question shown to the user. State clearly what action requires their sign-off."
            },
            "target_tool": {
                "type": "string",
                "description": "Name of the mutating tool that will be called if the user approves. Informational — helps the runtime trace confirmation-to-action pairs."
            }
        },
        "required": ["question"],
        "additionalProperties": false
    })
}

fn schema_ask_question() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {
                "type": "string",
                "description": "The question to present to the user."
            },
            "options": {
                "type": "array",
                "description": "Optional structured choices shown as clickable buttons. User can still type freely regardless. Omit for a pure free-text prompt.",
                "items": {
                    "type": "object",
                    "properties": {
                        "id":          { "type": "string", "description": "Unique identifier returned in selected_option_id." },
                        "label":       { "type": "string", "description": "Display text shown on the button." },
                        "description": { "type": "string", "description": "Optional clarifying text shown below the button." }
                    },
                    "required": ["id", "label"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["question"],
        "additionalProperties": false
    })
}

// ---------------------------------------------------------------------------
// Tool builders
// ---------------------------------------------------------------------------

fn make_ask_confirmation() -> ToolSpec {
    ToolSpec {
        name: "ask_confirmation".to_string(),
        description: Some(
            "MANDATORY safety gate. You MUST call this tool BEFORE any operation that \
             creates, updates, deletes, or deploys a resource. \
             Present a clear summary of the proposed action and STOP your turn — \
             do not call any other tool in the same turn. \
             The user sees three fixed options: \
             \"yes\" (approved, proceed), \"no\" (rejected, stop), \
             \"no_with_feedback\" (rejected with revision notes in free_text). \
             Returns { \"status\": \"interaction_pending\", \"interaction_id\": \"<uuid>\" } \
             which signals the runtime to pause execution until the user responds."
                .to_string(),
        ),
        parameters: schema_ask_confirmation(),
        strict: true,
        parallel_ok: false,
        executor: ToolExecutor::WithContext(Arc::new(|args: Value, ctx: ToolContext| {
            Box::pin(async move {
                let question = args
                    .get("question")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required: question".to_string())?
                    .to_string();

                let target_tool = args
                    .get("target_tool")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let interaction_id = Uuid::new_v4().to_string();

                let request = InteractionRequest {
                    interaction_id: interaction_id.clone(),
                    kind: InteractionKind::Confirmation,
                    question,
                    options: vec![
                        InteractionOption {
                            id: "yes".to_string(),
                            label: "Yes".to_string(),
                            description: None,
                        },
                        InteractionOption {
                            id: "no".to_string(),
                            label: "No".to_string(),
                            description: None,
                        },
                        InteractionOption {
                            id: "no_with_feedback".to_string(),
                            label: "No \u{2014} tell me what to change".to_string(),
                            description: None,
                        },
                    ],
                    allow_free_text: true,
                    allow_cancel: true,
                    default_option_id: None,
                    timeout_ms: None,
                    continuation_id: None,
                    source_node: None,
                    metadata: target_tool.map(|t| json!({ "target_tool": t })),
                };

                ctx.emit(AgentTraceEvent::InteractionRequested { request });

                Ok(json!({
                    "status": "interaction_pending",
                    "interaction_id": interaction_id,
                }))
            })
        })),
    }
}

fn make_ask_question() -> ToolSpec {
    ToolSpec {
        name: "ask_question".to_string(),
        description: Some(
            "Ask the user an open-ended question and wait for their response. \
             Optionally provide structured options the user can click; free-form text input is always available. \
             Returns immediately with { \"status\": \"interaction_pending\", \"interaction_id\": \"<uuid>\" }. \
             When the interaction resolves (next run), the message will contain an \
             \"interaction_response\" data part with: selected_option_id (if option clicked) and/or free_text."
                .to_string(),
        ),
        parameters: schema_ask_question(),
        strict: false,
        parallel_ok: false,
        executor: ToolExecutor::WithContext(Arc::new(|args: Value, ctx: ToolContext| {
            Box::pin(async move {
                let question = args
                    .get("question")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required: question".to_string())?
                    .to_string();

                let options: Vec<InteractionOption> = args
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                let id = item.get("id")?.as_str()?.to_string();
                                let label = item.get("label")?.as_str()?.to_string();
                                let description = item
                                    .get("description")
                                    .and_then(|d| d.as_str())
                                    .map(|s| s.to_string());
                                Some(InteractionOption { id, label, description })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let interaction_id = Uuid::new_v4().to_string();

                let request = InteractionRequest {
                    interaction_id: interaction_id.clone(),
                    kind: InteractionKind::Question,
                    question,
                    options,
                    allow_free_text: true,
                    allow_cancel: true,
                    default_option_id: None,
                    timeout_ms: None,
                    continuation_id: None,
                    source_node: None,
                    metadata: None,
                };

                ctx.emit(AgentTraceEvent::InteractionRequested { request });

                Ok(json!({
                    "status": "interaction_pending",
                    "interaction_id": interaction_id,
                }))
            })
        })),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns all available interaction tools.
///
/// Callers may filter the returned list by `spec.name` to register only a
/// subset (e.g. only `"ask_confirmation"`).
///
/// Known names: `"ask_confirmation"`, `"ask_question"`.
pub fn make_interaction_tools() -> Vec<ToolSpec> {
    vec![make_ask_confirmation(), make_ask_question()]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use crate::agent::tool_context::ToolContext;
    use crate::agent::tools::ToolExecutor;
    use crate::agent::trace::AgentTraceEvent;
    use crate::interaction::InteractionKind;

    use super::make_interaction_tools;

    fn make_capturing_ctx() -> (ToolContext, Arc<Mutex<Vec<AgentTraceEvent>>>) {
        let captured: Arc<Mutex<Vec<AgentTraceEvent>>> = Arc::new(Mutex::new(vec![]));
        let captured_clone = captured.clone();
        let ctx = ToolContext::new(
            Arc::new(move |e| captured_clone.lock().unwrap().push(e)),
            Arc::new(AtomicBool::new(false)),
        );
        (ctx, captured)
    }

    async fn call_with_context(
        name: &str,
        args: serde_json::Value,
    ) -> (serde_json::Value, Vec<AgentTraceEvent>) {
        let tools = make_interaction_tools();
        let spec = tools.into_iter().find(|t| t.name == name).unwrap();
        let (ctx, captured) = make_capturing_ctx();
        let result = match spec.executor {
            ToolExecutor::WithContext(f) => f(args, ctx).await.unwrap(),
            _ => panic!("expected WithContext executor"),
        };
        let events = captured.lock().unwrap().clone();
        (result, events)
    }

    #[tokio::test]
    async fn ask_confirmation_emits_interaction_requested_with_confirmation_kind() {
        let (result, events) = call_with_context(
            "ask_confirmation",
            json!({ "question": "Proceed with execution?" }),
        )
        .await;

        assert_eq!(result["status"], "interaction_pending");
        assert!(result["interaction_id"].as_str().is_some());

        let interaction = events.iter().find_map(|e| match e {
            AgentTraceEvent::InteractionRequested { request } => Some(request.clone()),
            _ => None,
        });
        let req = interaction.expect("InteractionRequested not emitted");
        assert_eq!(req.kind, InteractionKind::Confirmation);
        assert_eq!(req.question, "Proceed with execution?");
        assert_eq!(req.options.len(), 3);
        assert_eq!(req.options[0].id, "yes");
        assert_eq!(req.options[1].id, "no");
        assert_eq!(req.options[2].id, "no_with_feedback");
        assert!(req.allow_free_text);
        assert!(req.allow_cancel);
        assert!(req.timeout_ms.is_none());
    }

    #[tokio::test]
    async fn ask_confirmation_interaction_id_matches_result() {
        let (result, events) =
            call_with_context("ask_confirmation", json!({ "question": "Are you sure?" })).await;

        let result_id = result["interaction_id"].as_str().unwrap();
        let emitted_id = events.iter().find_map(|e| match e {
            AgentTraceEvent::InteractionRequested { request } => {
                Some(request.interaction_id.clone())
            }
            _ => None,
        });
        assert_eq!(Some(result_id.to_string()), emitted_id);
    }

    #[tokio::test]
    async fn ask_question_emits_interaction_requested_with_options() {
        let (result, events) = call_with_context(
            "ask_question",
            json!({
                "question": "Which data source should I use?",
                "options": [
                    { "id": "postgres", "label": "PostgreSQL" },
                    { "id": "bigquery", "label": "BigQuery", "description": "Cloud warehouse" }
                ]
            }),
        )
        .await;

        assert_eq!(result["status"], "interaction_pending");

        let req = events
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::InteractionRequested { request } => Some(request.clone()),
                _ => None,
            })
            .expect("InteractionRequested not emitted");

        assert_eq!(req.kind, InteractionKind::Question);
        assert_eq!(req.question, "Which data source should I use?");
        assert_eq!(req.options.len(), 2);
        assert_eq!(req.options[0].id, "postgres");
        assert_eq!(req.options[1].id, "bigquery");
        assert_eq!(
            req.options[1].description,
            Some("Cloud warehouse".to_string())
        );
        assert!(req.allow_free_text);
        assert!(req.allow_cancel);
    }

    #[tokio::test]
    async fn ask_question_without_options_emits_empty_options() {
        let (_result, events) =
            call_with_context("ask_question", json!({ "question": "What is your goal?" })).await;

        let req = events
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::InteractionRequested { request } => Some(request.clone()),
                _ => None,
            })
            .expect("InteractionRequested not emitted");

        assert!(req.options.is_empty());
        assert!(req.allow_free_text);
    }

    #[tokio::test]
    async fn ask_confirmation_with_target_tool_includes_metadata() {
        let (result, events) = call_with_context(
            "ask_confirmation",
            json!({ "question": "Deploy agent X?", "target_tool": "mcp_platform_create_agent" }),
        )
        .await;

        assert_eq!(result["status"], "interaction_pending");

        let req = events
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::InteractionRequested { request } => Some(request.clone()),
                _ => None,
            })
            .expect("InteractionRequested not emitted");

        assert_eq!(req.kind, InteractionKind::Confirmation);
        let meta = req
            .metadata
            .expect("metadata should be set when target_tool is provided");
        assert_eq!(meta["target_tool"], "mcp_platform_create_agent");
    }

    #[tokio::test]
    async fn ask_confirmation_without_target_tool_has_no_metadata() {
        let (_result, events) =
            call_with_context("ask_confirmation", json!({ "question": "Proceed?" })).await;

        let req = events
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::InteractionRequested { request } => Some(request.clone()),
                _ => None,
            })
            .expect("InteractionRequested not emitted");

        assert!(req.metadata.is_none());
    }

    #[test]
    fn make_interaction_tools_returns_both() {
        let tools = make_interaction_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"ask_confirmation"),
            "missing ask_confirmation"
        );
        assert!(names.contains(&"ask_question"), "missing ask_question");
    }
}
