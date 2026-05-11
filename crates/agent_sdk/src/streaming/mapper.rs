use crate::agent::trace::AgentTraceEvent;
use a2a_protocol_core::data::{Message, MessageRole, Part, TaskState, TaskStatus};
use a2a_protocol_core::streaming::{StreamResponse, TaskStatusUpdateEvent};
use serde_json::json;

use super::events::{AgentIoEvent, IoEventContext};

#[derive(Debug, Clone)]
pub struct A2aSseContext {
    pub task_id: String,
    pub context_id: String,
    pub jsonrpc_id: serde_json::Value,
}

fn status_update(ctx: &A2aSseContext, state: TaskState, message: Option<String>) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        id: ctx.jsonrpc_id.clone(),
        task_id: ctx.task_id.clone(),
        context_id: ctx.context_id.clone(),
        status: TaskStatus {
            state,
            message: message.map(|text| {
                Message::new(
                    MessageRole::Agent,
                    vec![Part::text(text)],
                    ctx.task_id.clone(),
                )
            }),
            timestamp: None,
        },
    })
}

pub fn map_trace_to_agent_io(event: AgentTraceEvent, ctx: &IoEventContext) -> Vec<AgentIoEvent> {
    match event {
        AgentTraceEvent::TurnStarted { turn, .. } => vec![AgentIoEvent::StepStarted {
            step_name: format!("turn_{turn}"),
        }],
        AgentTraceEvent::ContentDelta { delta } => vec![AgentIoEvent::TextMessageContent {
            message_id: ctx.message_id.clone(),
            delta,
        }],
        AgentTraceEvent::ReasoningStarted { message_id } => vec![
            AgentIoEvent::ReasoningStart {
                message_id: message_id.clone(),
            },
            AgentIoEvent::ReasoningMessageStart {
                message_id,
                role: "assistant".to_string(),
            },
        ],
        AgentTraceEvent::ReasoningDelta { delta } => vec![AgentIoEvent::ReasoningMessageContent {
            message_id: ctx.message_id.clone(),
            delta,
        }],
        AgentTraceEvent::ReasoningCompleted { message_id } => vec![
            AgentIoEvent::ReasoningMessageEnd {
                message_id: message_id.clone(),
            },
            AgentIoEvent::ReasoningEnd { message_id },
        ],
        AgentTraceEvent::ToolCallStarted { id, name, .. } => vec![AgentIoEvent::ToolCallStart {
            tool_call_id: id,
            tool_call_name: name,
            parent_message_id: Some(ctx.message_id.clone()),
        }],
        AgentTraceEvent::ToolCallArgsDelta { id, delta, .. } => vec![AgentIoEvent::ToolCallArgs {
            tool_call_id: id,
            delta,
        }],
        AgentTraceEvent::ToolCallArgsCompleted { id, .. } => {
            vec![AgentIoEvent::ToolCallEnd { tool_call_id: id }]
        }
        AgentTraceEvent::ToolCallCompleted {
            id,
            result,
            success,
            ..
        } => {
            let mut events = delegation_events_from_tool_result(&id, &result, success);
            events.push(AgentIoEvent::ToolCallResult {
                tool_call_id: id,
                message_id: Some(ctx.message_id.clone()),
                content: result,
                role: Some("tool".to_string()),
            });
            events
        }
        AgentTraceEvent::GovernanceDecision {
            name,
            decision_id,
            mode,
            decision,
            would_have,
            reason,
        } => vec![AgentIoEvent::Custom {
            name: "governance_decision".to_string(),
            value: json!({
                "toolName": name,
                "decisionId": decision_id,
                "mode": mode,
                "decision": decision,
                "wouldHave": would_have,
                "reason": reason,
            }),
        }],
        AgentTraceEvent::TurnCompleted { turn, .. } => vec![AgentIoEvent::StepFinished {
            step_name: format!("turn_{turn}"),
        }],
        AgentTraceEvent::Completed { usage, .. } => vec![AgentIoEvent::RunFinished {
            thread_id: ctx.thread_id.clone(),
            run_id: ctx.run_id.clone(),
            result: Some(json!({ "usage": usage })),
        }],
        AgentTraceEvent::ProgressUpdate {
            message,
            progress_pct,
            metadata,
        } => {
            if let Some(event) =
                delegation_event_from_progress(&message, progress_pct, metadata.as_ref())
            {
                vec![event]
            } else {
                vec![AgentIoEvent::Custom {
                    name: "progress_update".to_string(),
                    value: json!({
                        "message": message,
                        "progressPct": progress_pct,
                        "metadata": metadata
                    }),
                }]
            }
        }
        AgentTraceEvent::AgentHandoff {
            from_agent,
            to_agent,
            metadata,
        } => {
            if let Some(event) =
                delegation_event_from_handoff(&from_agent, &to_agent, metadata.as_ref())
            {
                vec![event]
            } else if metadata
                .as_ref()
                .and_then(|value| value.get("source"))
                .and_then(|value| value.as_str())
                == Some("sub_agent")
            {
                Vec::new()
            } else {
                vec![AgentIoEvent::Custom {
                    name: "agent_handoff".to_string(),
                    value: json!({
                        "fromAgent": from_agent,
                        "toAgent": to_agent,
                        "metadata": metadata,
                    }),
                }]
            }
        }
        AgentTraceEvent::InteractionRequested { request } => vec![AgentIoEvent::Custom {
            name: "interaction_requested".to_string(),
            value: json!(request),
        }],
        AgentTraceEvent::AppActionRequested { request } => vec![AgentIoEvent::Custom {
            name: "app_action_requested".to_string(),
            value: request,
        }],
        AgentTraceEvent::InteractionResolved { response } => vec![AgentIoEvent::Custom {
            name: "interaction_resolved".to_string(),
            value: json!(response),
        }],
        AgentTraceEvent::InteractionCancelled {
            interaction_id,
            reason,
        } => vec![AgentIoEvent::Custom {
            name: "interaction_cancelled".to_string(),
            value: json!({
                "interactionId": interaction_id,
                "reason": reason,
            }),
        }],
        AgentTraceEvent::InteractionExpired { interaction_id } => vec![AgentIoEvent::Custom {
            name: "interaction_expired".to_string(),
            value: json!({
                "interactionId": interaction_id,
            }),
        }],
        AgentTraceEvent::ContextTrimmed {
            evicted_count,
            remaining_count,
        } => vec![AgentIoEvent::Custom {
            name: "context_summarized".to_string(),
            value: json!({
                "evictedCount": evicted_count,
                "remainingCount": remaining_count
            }),
        }],
        AgentTraceEvent::Failed { message } => vec![AgentIoEvent::RunError {
            message,
            code: Some("ENGINE_ERROR".to_string()),
        }],
    }
}

fn delegation_event_from_handoff(
    from_agent: &str,
    to_agent: &str,
    metadata: Option<&serde_json::Value>,
) -> Option<AgentIoEvent> {
    let meta = metadata?.as_object()?;
    if meta.get("source")?.as_str()? != "sub_agent" {
        return None;
    }
    if meta.get("phase").and_then(|v| v.as_str()) != Some("start") {
        return None;
    }

    let tool_call_id = meta
        .get("tool_call_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if tool_call_id.is_empty() {
        return None;
    }
    let delegation_id = meta
        .get("delegation_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&tool_call_id)
        .to_string();
    let subagent = meta
        .get("agent_name")
        .and_then(|v| v.as_str())
        .unwrap_or(to_agent)
        .to_string();
    let task_id = meta
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    Some(AgentIoEvent::DelegationStarted {
        delegation_id,
        tool_call_id,
        subagent,
        task_id,
        metadata: Some(json!({
            "fromAgent": from_agent,
            "toAgent": to_agent,
            "mode": meta.get("mode").cloned(),
        })),
    })
}

fn delegation_event_from_progress(
    message: &str,
    progress_pct: Option<u8>,
    metadata: Option<&serde_json::Value>,
) -> Option<AgentIoEvent> {
    let meta = metadata?.as_object()?;
    if meta.get("source")?.as_str()? != "sub_agent" {
        return None;
    }

    let tool_call_id = meta
        .get("tool_call_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if tool_call_id.is_empty() {
        return None;
    }
    let delegation_id = meta
        .get("delegation_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&tool_call_id)
        .to_string();
    let subagent = meta
        .get("agent_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let task_id = meta
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let state = meta
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("working")
        .to_string();

    let passthrough_meta = json!({
        "progressPct": progress_pct,
        "metadata": metadata,
    });

    match state.as_str() {
        "failed" => Some(AgentIoEvent::DelegationFailed {
            delegation_id,
            tool_call_id,
            subagent,
            task_id,
            error_kind: meta
                .get("error_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("subagent_failed")
                .to_string(),
            message: message.to_string(),
            metadata: Some(passthrough_meta),
        }),
        "inputrequired" => Some(AgentIoEvent::DelegationInputRequired {
            delegation_id,
            tool_call_id,
            subagent,
            task_id,
            message: message.to_string(),
            metadata: Some(passthrough_meta),
        }),
        _ => Some(AgentIoEvent::DelegationProgress {
            delegation_id,
            tool_call_id,
            subagent,
            task_id,
            status: state,
            message: message.to_string(),
            metadata: Some(passthrough_meta),
        }),
    }
}

fn delegation_events_from_tool_result(
    tool_call_id: &str,
    result: &serde_json::Value,
    success: bool,
) -> Vec<AgentIoEvent> {
    if success {
        let Some(subagent) = result.get("agent_name").and_then(|v| v.as_str()) else {
            return Vec::new();
        };
        let task_id = result
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        return vec![AgentIoEvent::DelegationFinished {
            delegation_id: result
                .get("delegation_id")
                .and_then(|v| v.as_str())
                .unwrap_or(tool_call_id)
                .to_string(),
            tool_call_id: tool_call_id.to_string(),
            subagent: subagent.to_string(),
            task_id,
            result: result.get("result").cloned(),
            metadata: Some(json!({ "result": result })),
        }];
    }

    let Some(error_obj) = result.get("error").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    if error_obj.get("source").and_then(|v| v.as_str()) != Some("sub_agent") {
        return Vec::new();
    }

    vec![AgentIoEvent::DelegationFailed {
        delegation_id: error_obj
            .get("delegation_id")
            .and_then(|v| v.as_str())
            .unwrap_or(tool_call_id)
            .to_string(),
        tool_call_id: error_obj
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or(tool_call_id)
            .to_string(),
        subagent: error_obj
            .get("agent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        task_id: error_obj
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        error_kind: error_obj
            .get("error_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("subagent_failed")
            .to_string(),
        message: error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("delegation failed")
            .to_string(),
        metadata: Some(serde_json::Value::Object(error_obj.clone())),
    }]
}

pub fn map_trace_to_stream_response(
    event: AgentTraceEvent,
    ctx: &A2aSseContext,
) -> Vec<StreamResponse> {
    match event {
        AgentTraceEvent::TurnStarted { .. } => {
            vec![status_update(ctx, TaskState::Working, None)]
        }
        AgentTraceEvent::ContentDelta { delta } => {
            vec![status_update(ctx, TaskState::Working, Some(delta))]
        }
        AgentTraceEvent::Completed { text, .. } => {
            vec![status_update(ctx, TaskState::Completed, text)]
        }
        AgentTraceEvent::Failed { message } => {
            vec![status_update(ctx, TaskState::Failed, Some(message))]
        }
        AgentTraceEvent::ProgressUpdate {
            message,
            progress_pct,
            metadata,
        } => vec![status_update(
            ctx,
            TaskState::Working,
            Some(
                json!({
                    "_type": "progress_update",
                    "message": message,
                    "progress_pct": progress_pct,
                    "metadata": metadata,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::AgentHandoff {
            from_agent,
            to_agent,
            metadata,
        } => vec![status_update(
            ctx,
            TaskState::Working,
            Some(
                json!({
                    "_type": "agent_handoff",
                    "from_agent": from_agent,
                    "to_agent": to_agent,
                    "metadata": metadata,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::InteractionRequested { request } => vec![status_update(
            ctx,
            TaskState::InputRequired,
            Some(
                json!({
                    "_type": "interaction_requested",
                    "request": request,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::InteractionResolved { response } => vec![status_update(
            ctx,
            TaskState::Working,
            Some(
                json!({
                    "_type": "interaction_resolved",
                    "response": response,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::InteractionCancelled {
            interaction_id,
            reason,
        } => vec![status_update(
            ctx,
            TaskState::Working,
            Some(
                json!({
                    "_type": "interaction_cancelled",
                    "interaction_id": interaction_id,
                    "reason": reason,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::InteractionExpired { interaction_id } => vec![status_update(
            ctx,
            TaskState::Working,
            Some(
                json!({
                    "_type": "interaction_expired",
                    "interaction_id": interaction_id,
                })
                .to_string(),
            ),
        )],
        AgentTraceEvent::ToolCallCompleted { name, result, .. } if name.starts_with("coord.") => {
            vec![status_update(
                ctx,
                TaskState::Working,
                Some(
                    json!({
                        "_type": "tool_call_completed",
                        "name": name,
                        "result": result,
                    })
                    .to_string(),
                ),
            )]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{InteractionKind, InteractionRequest, InteractionResponse};
    use serde_json::json;

    fn ctx() -> A2aSseContext {
        A2aSseContext {
            task_id: "task-1".to_string(),
            context_id: "ctx-1".to_string(),
            jsonrpc_id: json!("rpc-1"),
        }
    }

    #[test]
    fn handoff_maps_to_a2a_working_status() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::AgentHandoff {
                from_agent: "coordinator".to_string(),
                to_agent: "planner".to_string(),
                metadata: None,
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, TaskState::Working);
                let msg = ev.status.message.as_ref().expect("expected message");
                assert!(
                    msg.get_text_content()
                        .contains("\"_type\":\"agent_handoff\"")
                );
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn progress_maps_to_a2a_working_status() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::ProgressUpdate {
                message: "Planning...".to_string(),
                progress_pct: Some(40),
                metadata: Some(json!({"phase":"plan"})),
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, TaskState::Working);
                let msg = ev.status.message.as_ref().expect("expected message");
                assert!(
                    msg.get_text_content()
                        .contains("\"_type\":\"progress_update\"")
                );
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn non_coord_tool_completed_maps_to_empty() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "tc-1".to_string(),
                name: "web_search".to_string(),
                result: json!({"results": []}),
                duration_ms: 50,
                success: true,
            },
            &ctx(),
        );
        assert!(
            out.is_empty(),
            "non-coord ToolCallCompleted should be dropped in A2A mapping"
        );
    }

    #[test]
    fn coord_tool_completed_maps_to_a2a_working_status() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "tc-1".to_string(),
                name: "coord.graph_created".to_string(),
                result: json!({"graph":{"nodes":[]}}),
                duration_ms: 0,
                success: true,
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, TaskState::Working);
                let msg = ev.status.message.as_ref().expect("expected message");
                assert!(
                    msg.get_text_content()
                        .contains("\"name\":\"coord.graph_created\"")
                );
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn interaction_requested_maps_to_a2a_input_required() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::InteractionRequested {
                request: InteractionRequest {
                    interaction_id: "ix-1".to_string(),
                    kind: InteractionKind::Question,
                    question: "Pick one".to_string(),
                    options: vec![],
                    allow_free_text: true,
                    allow_cancel: true,
                    default_option_id: None,
                    timeout_ms: None,
                    continuation_id: None,
                    source_node: None,
                    metadata: None,
                },
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, TaskState::InputRequired);
                let msg = ev.status.message.as_ref().expect("expected message");
                assert!(
                    msg.get_text_content()
                        .contains("\"_type\":\"interaction_requested\"")
                );
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn interaction_resolved_maps_to_agent_io_custom() {
        let ctx = IoEventContext {
            thread_id: "thread-1".to_string(),
            run_id: "run-1".to_string(),
            message_id: "msg-1".to_string(),
        };
        let out = map_trace_to_agent_io(
            AgentTraceEvent::InteractionResolved {
                response: InteractionResponse {
                    interaction_id: "ix-1".to_string(),
                    selected_option_id: Some("approve".to_string()),
                    free_text: None,
                    confirmed: Some(true),
                    cancelled: false,
                    metadata: None,
                },
            },
            &ctx,
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            AgentIoEvent::Custom { name, value } => {
                assert_eq!(name, "interaction_resolved");
                assert_eq!(value["interactionId"], "ix-1");
            }
            other => panic!("expected custom event, got {:?}", other),
        }
    }

    #[test]
    fn app_action_requested_maps_to_agent_io_custom() {
        let ctx = IoEventContext {
            thread_id: "thread-1".to_string(),
            run_id: "run-1".to_string(),
            message_id: "msg-1".to_string(),
        };
        let out = map_trace_to_agent_io(
            AgentTraceEvent::AppActionRequested {
                request: serde_json::json!({
                    "actionId": "chatflow.navigate",
                    "args": { "route": "/admin/agents/builder" }
                }),
            },
            &ctx,
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            AgentIoEvent::Custom { name, value } => {
                assert_eq!(name, "app_action_requested");
                assert_eq!(value["actionId"], "chatflow.navigate");
                assert_eq!(value["args"]["route"], "/admin/agents/builder");
            }
            other => panic!("expected custom event, got {:?}", other),
        }
    }
}
