use std::sync::Arc;

use a2a_http_client::Client;
use a2a_protocol_core::data::{Message, MessageRole, Part, TaskState};
use a2a_protocol_core::streaming::StreamResponse;
use futures_util::StreamExt;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::agent::tool_context::ToolContext;
use crate::agent::trace::AgentTraceEvent;
use crate::sub_agent::adapter::{SubAgentAdapter, SubAgentContext, SubAgentFuture};
use crate::sub_agent::tool::DelegationMode;

pub type A2aMessageBuilder = Arc<dyn Fn(Value) -> Message + Send + Sync>;

#[derive(Debug, Clone)]
pub struct A2aStreamEvent(pub StreamResponse);

pub trait A2aTraceMapper: Send + Sync {
    fn map(&self, event: &A2aStreamEvent, ctx: &SubAgentContext) -> Vec<AgentTraceEvent>;
    fn extract_result(&self, event: &A2aStreamEvent) -> Option<String>;
}

#[derive(Debug, Default, Clone)]
pub struct DefaultA2aTraceMapper;

impl A2aTraceMapper for DefaultA2aTraceMapper {
    fn map(&self, event: &A2aStreamEvent, ctx: &SubAgentContext) -> Vec<AgentTraceEvent> {
        match &event.0 {
            StreamResponse::StatusUpdate(status) => {
                if matches!(status.status.state, TaskState::Completed) {
                    return Vec::new();
                }

                let text = status
                    .status
                    .message
                    .as_ref()
                    .map(|m| m.get_text_content())
                    .filter(|s| !s.is_empty());

                let Some(text) = text else {
                    return Vec::new();
                };

                let state_text = status.status.state.as_str().to_string();
                vec![AgentTraceEvent::ProgressUpdate {
                    message: text,
                    progress_pct: None,
                    metadata: Some(json!({
                        "source": "sub_agent",
                        "delegation_id": ctx.tool_call_id,
                        "tool_call_id": ctx.tool_call_id,
                        "agent_name": ctx.agent_name,
                        "task_id": ctx.task_id,
                        "state": state_text,
                        "terminal": status.status.state.is_terminal(),
                    })),
                }]
            }
            StreamResponse::ArtifactUpdate(artifact) => vec![AgentTraceEvent::ProgressUpdate {
                message: format!("Sub-agent '{}' produced artifact", ctx.agent_name),
                progress_pct: None,
                metadata: Some(json!({
                    "source": "sub_agent",
                    "delegation_id": ctx.tool_call_id,
                    "tool_call_id": ctx.tool_call_id,
                    "agent_name": ctx.agent_name,
                    "task_id": ctx.task_id,
                    "artifact": artifact.artifact,
                    "last_chunk": artifact.last_chunk,
                })),
            }],
            _ => Vec::new(),
        }
    }

    fn extract_result(&self, event: &A2aStreamEvent) -> Option<String> {
        match &event.0 {
            StreamResponse::StatusUpdate(status) => status
                .status
                .message
                .as_ref()
                .map(|m| m.get_text_content())
                .filter(|s| !s.is_empty()),
            StreamResponse::ArtifactUpdate(artifact) => {
                serde_json::to_string(&artifact.artifact).ok()
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct A2aSubAgentAdapter {
    message_builder: A2aMessageBuilder,
    trace_mapper: Arc<dyn A2aTraceMapper>,
}

impl Default for A2aSubAgentAdapter {
    fn default() -> Self {
        Self {
            message_builder: Arc::new(|args| {
                let text = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                Message::new(
                    MessageRole::User,
                    vec![Part::text(text)],
                    Uuid::new_v4().to_string(),
                )
            }),
            trace_mapper: Arc::new(DefaultA2aTraceMapper),
        }
    }
}

impl A2aSubAgentAdapter {
    pub fn message_builder(mut self, builder: A2aMessageBuilder) -> Self {
        self.message_builder = builder;
        self
    }

    pub fn trace_mapper(mut self, mapper: Arc<dyn A2aTraceMapper>) -> Self {
        self.trace_mapper = mapper;
        self
    }
}

impl SubAgentAdapter for A2aSubAgentAdapter {
    fn execute(
        &self,
        mode: DelegationMode,
        agent_name: String,
        agent_url: String,
        args: Value,
        headers: Vec<(String, String)>,
        emit_handoff: bool,
        ctx: ToolContext,
    ) -> SubAgentFuture {
        let message_builder = self.message_builder.clone();
        let mapper = self.trace_mapper.clone();
        Box::pin(async move {
            execute_subagent_tool(
                mode,
                &agent_name,
                &agent_url,
                args,
                message_builder,
                mapper,
                headers,
                emit_handoff,
                ctx,
            )
            .await
        })
    }
}

async fn execute_subagent_tool(
    mode: DelegationMode,
    agent_name: &str,
    agent_url: &str,
    args: Value,
    message_builder: A2aMessageBuilder,
    mapper: Arc<dyn A2aTraceMapper>,
    headers: Vec<(String, String)>,
    emit_handoff: bool,
    ctx: ToolContext,
) -> Result<Value, String> {
    let task_id = format!("subagent-{}", Uuid::new_v4().simple());
    let tool_call_id = format!("sub_tool_{}", Uuid::new_v4().simple());
    let sub_ctx = SubAgentContext {
        agent_name: agent_name.to_string(),
        task_id: task_id.clone(),
        tool_call_id: tool_call_id.clone(),
    };

    if emit_handoff {
        ctx.emit(AgentTraceEvent::AgentHandoff {
            from_agent: "self".to_string(),
            to_agent: agent_name.to_string(),
            metadata: Some(json!({
                "source": "sub_agent",
                "phase": "start",
                "delegation_id": tool_call_id.clone(),
                "tool_call_id": tool_call_id.clone(),
                "agent_name": agent_name,
                "mode": format!("{:?}", mode).to_lowercase(),
                "task_id": task_id.clone(),
            })),
        });
    }

    let mut client = {
        let client = Client::external(agent_url.to_string());
        #[cfg(feature = "agent-observability")]
        {
            if let Some(obs) = crate::shared_observability() {
                client.with_observability(obs)
            } else {
                client
            }
        }
        #[cfg(not(feature = "agent-observability"))]
        {
            client
        }
    };
    for (k, v) in headers {
        client = client.with_header(k, v);
    }
    let forwarded_headers =
        protocol_transport_core::sanitize_headers(ctx.request_headers()).into_map();
    for (k, v) in forwarded_headers {
        client = client.with_header(k.clone(), v.clone());
    }

    let message = (message_builder)(args);
    let result = match mode {
        DelegationMode::Streaming => {
            execute_streaming(&client, &task_id, message, mapper, sub_ctx.clone(), &ctx).await
        }
        DelegationMode::Synchronous => execute_synchronous(&client, message).await,
    };

    if emit_handoff {
        ctx.emit(AgentTraceEvent::AgentHandoff {
            from_agent: agent_name.to_string(),
            to_agent: "self".to_string(),
            metadata: Some(json!({
                "source": "sub_agent",
                "phase": "end",
                "delegation_id": sub_ctx.tool_call_id.clone(),
                "tool_call_id": sub_ctx.tool_call_id.clone(),
                "agent_name": agent_name,
                "task_id": task_id.clone(),
            })),
        });
    }

    result
}

async fn execute_streaming(
    client: &Client,
    task_id: &str,
    message: Message,
    mapper: Arc<dyn A2aTraceMapper>,
    sub_ctx: SubAgentContext,
    ctx: &ToolContext,
) -> Result<Value, String> {
    let mut stream = client.send_subscribe(task_id, message).await.map_err(|e| {
        structured_subagent_error(
            &sub_ctx,
            "transport",
            &format!("sub-agent send_subscribe failed: {}", e),
        )
    })?;

    let mut final_text: Option<String> = None;
    let mut terminal_state: Option<TaskState> = None;
    let mut remote_task_id: Option<String> = None;
    while let Some(next) = stream.next().await {
        let event = next.map_err(|e| {
            structured_subagent_error(
                &sub_ctx,
                classify_stream_error(&e.message),
                &format!("sub-agent SSE read failed: {}", e),
            )
        })?;
        let event_wrapper = A2aStreamEvent(event.clone());
        if remote_task_id.is_none() {
            remote_task_id = remote_task_id_from_event(&event);
        }
        if let StreamResponse::StatusUpdate(status) = &event {
            if status.status.state.is_terminal() {
                terminal_state = Some(status.status.state.clone());
            }
        }
        for trace in mapper.map(&event_wrapper, &sub_ctx) {
            ctx.emit(trace);
        }
        if let Some(text) = mapper.extract_result(&event_wrapper) {
            final_text = Some(text);
        }
        if ctx.is_cancelled() {
            break;
        }
    }

    if ctx.is_cancelled() {
        let cancel_result = maybe_cancel_remote_task(client, remote_task_id.as_deref()).await;
        return Ok(json!({
            "delegation_id": sub_ctx.tool_call_id,
            "tool_call_id": sub_ctx.tool_call_id,
            "result": final_text.unwrap_or_default(),
            "task_id": sub_ctx.task_id,
            "agent_name": sub_ctx.agent_name,
            "cancelled": true,
            "remote_task_id": remote_task_id,
            "remote_cancel": cancel_result,
        }));
    }

    if terminal_state.is_none() {
        return Err(structured_subagent_error(
            &sub_ctx,
            "broken_stream",
            "sub-agent stream ended without a terminal event",
        ));
    }

    if matches!(terminal_state, Some(TaskState::Failed)) {
        return Err(structured_subagent_error(
            &sub_ctx,
            "subagent_failed",
            final_text
                .as_deref()
                .unwrap_or("sub-agent reported failed terminal state"),
        ));
    }

    Ok(json!({
        "delegation_id": sub_ctx.tool_call_id,
        "tool_call_id": sub_ctx.tool_call_id,
        "result": final_text.unwrap_or_default(),
        "task_id": sub_ctx.task_id,
        "agent_name": sub_ctx.agent_name,
    }))
}

fn remote_task_id_from_event(event: &StreamResponse) -> Option<String> {
    match event {
        StreamResponse::StatusUpdate(status) => Some(status.task_id.clone()),
        StreamResponse::ArtifactUpdate(artifact) => Some(artifact.task_id.clone()),
        _ => None,
    }
}

async fn maybe_cancel_remote_task(client: &Client, remote_task_id: Option<&str>) -> Value {
    let Some(task_id) = remote_task_id.filter(|value| !value.is_empty()) else {
        return json!({
            "attempted": false,
            "cancelled": false,
            "reason": "remote_task_id_unavailable"
        });
    };

    match client.task_cancel(task_id.to_string()).await {
        Ok(task) => json!({
            "attempted": true,
            "cancelled": true,
            "task_id": task.id,
            "state": format!("{:?}", task.status.state).to_lowercase(),
        }),
        Err(err) => json!({
            "attempted": true,
            "cancelled": false,
            "task_id": task_id,
            "error": err.message,
        }),
    }
}

async fn execute_synchronous(client: &Client, message: Message) -> Result<Value, String> {
    let value = client
        .message_send(message, None)
        .await
        .map_err(|e| format!("sub-agent message_send failed: {}", e))?;
    Ok(json!({
        "result": extract_text_from_response(&value).unwrap_or_else(|| value.to_string()),
        "raw": value
    }))
}

fn structured_subagent_error(sub_ctx: &SubAgentContext, error_kind: &str, message: &str) -> String {
    json!({
        "source": "sub_agent",
        "error_kind": error_kind,
        "message": message,
        "agent_name": sub_ctx.agent_name,
        "task_id": sub_ctx.task_id,
        "tool_call_id": sub_ctx.tool_call_id,
        "delegation_id": sub_ctx.tool_call_id,
    })
    .to_string()
}

fn classify_stream_error(message: &str) -> &str {
    if message.contains("Invalid SSE JSON payload")
        || message.contains("Invalid SSE payload")
        || message.contains("Invalid task")
    {
        "malformed_payload"
    } else {
        "broken_stream"
    }
}

/// Extract agent text from a `SendMessageResponse` result value.
///
/// The value is the `result` field from the JSON-RPC response (not the full
/// envelope). The v1.0 wire format wraps the payload under `"task"` or
/// `"message"` keys (externally-tagged `oneof`).
fn extract_text_from_response(value: &Value) -> Option<String> {
    if let Some(text) = value
        .get("task")
        .and_then(|t| t.get("status"))
        .and_then(|s| s.get("message"))
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
    {
        return Some(text.to_string());
    }
    value
        .get("message")
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
        .map(ToString::to_string)
}
