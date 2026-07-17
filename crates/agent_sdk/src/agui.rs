//! AG-UI adapter surface for composing user-facing streaming around [`crate::Agent`].
//!
//! ## Platform
//!
//! - **Native only** (`not(target_arch = "wasm32")`): HTTP routes and SSE streaming require Tokio
//!   and Axum. Enable the **`event-stream`** feature (and typically **`llm-engine`** on the agent).
//! - **WASM**: this module is not built for `wasm32`; use A2A hosting from [`crate::host`] or
//!   `a2a_serve!` instead.
//!
//! ## Usage
//!
//! - Configure [`crate::agui::AgUiConfig`] (route path) and build [`crate::agui::AgUiApp`] from an [`crate::Agent`] that has a
//!   **stream-capable** LLM runtime ([`crate::Agent::configure_llm_runtime`] on native).
//! - Combine with [`crate::AgentHostBuilder::with_agui`] to serve AG-UI alongside A2A on one router.

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::atomic::AtomicBool;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::Arc;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use agent_core::{AgentMessage, ContentPart, ConversationContext, Role};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use base64::Engine as _;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::extract::State;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::http::HeaderMap;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::response::{IntoResponse, Response};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::routing::post;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::{Json, Router};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use serde::{Deserialize, Serialize};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use serde_json::Value;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use uuid::Uuid;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use crate::{Agent, SdkError, SdkResult};

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub use crate::streaming::{
    AgUiDriverConfig, AgUiStream, AgUiStreamDriver, AgentIoEvent, IoEventContext, RunStatus,
    RunSummary, StreamEnricher, SummaryHandle, ag_ui_sse_response, ag_ui_sse_response_with_summary,
    agent_io_sse_stream, map_trace_to_agent_io,
};

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone)]
pub struct AgUiConfig {
    pub route_path: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl Default for AgUiConfig {
    fn default() -> Self {
        Self {
            route_path: "/agui/v1/runs".to_string(),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Clone)]
pub struct AgUiApp {
    config: AgUiConfig,
    state: Arc<AgUiState>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl AgUiApp {
    pub fn from_agent(agent: Agent, config: AgUiConfig) -> SdkResult<Self> {
        Self::from_shared_agent(Arc::new(agent), config)
    }

    pub fn from_shared_agent(agent: Arc<Agent>, config: AgUiConfig) -> SdkResult<Self> {
        #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
        if !agent.can_run_stream() {
            return Err(SdkError::configuration(
                "AG-UI app requires a stream-capable LLM runtime; configure the agent with configure_llm_runtime",
            ));
        }

        Ok(Self {
            config,
            state: Arc::new(AgUiState::new(agent)),
        })
    }

    pub fn router(self) -> Router {
        Router::new()
            .route(&self.config.route_path, post(agui_run))
            .with_state(self.state)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
struct AgUiState {
    agent: Arc<Agent>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl AgUiState {
    fn new(agent: Arc<Agent>) -> Self {
        Self { agent }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub messages: Vec<RunAgentMessage>,
    #[serde(default)]
    pub tools: Vec<RunAgentTool>,
    #[serde(default)]
    pub context: Vec<Value>,
    #[serde(default)]
    pub forwarded_props: Value,
    #[serde(default)]
    pub resume: Vec<ResumeEntry>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_tool_parameters")]
    pub parameters: Value,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn default_tool_parameters() -> Value {
    serde_json::json!({ "type": "object", "properties": {} })
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    pub interrupt_id: String,
    pub status: ResumeStatus,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResumeStatus {
    Resolved,
    Cancelled,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentMessage {
    #[serde(default)]
    pub id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl RunAgentInput {
    pub fn normalized_thread_id(&self) -> String {
        (!self.thread_id.trim().is_empty())
            .then(|| self.thread_id.clone())
            .unwrap_or_else(|| format!("thread-{}", Uuid::new_v4()))
    }

    pub fn normalized_run_id(&self) -> String {
        (!self.run_id.trim().is_empty())
            .then(|| self.run_id.clone())
            .unwrap_or_else(|| format!("run-{}", Uuid::new_v4()))
    }

    pub fn forwarded_prop(&self, camel_case_key: &str, snake_case_key: &str) -> Option<Value> {
        self.forwarded_props
            .get(camel_case_key)
            .or_else(|| self.forwarded_props.get(snake_case_key))
            .cloned()
    }

    pub fn split_prompt_and_history(&self) -> (AgentMessage, Vec<AgentMessage>) {
        let prompt_index = self
            .messages
            .iter()
            .rposition(|message| {
                !matches!(
                    message.role.as_str(),
                    "assistant" | "system" | "developer"
                )
            })
            .or_else(|| (!self.messages.is_empty()).then(|| self.messages.len() - 1));
        let mut user_message = prompt_index
            .map(|index| run_agent_message_to_agent_message(&self.messages[index]))
            .unwrap_or_else(|| AgentMessage::user_text(""));
        if !self.resume.is_empty() {
            user_message
                .parts
                .push(ContentPart::Data(serde_json::json!({
                    "ag_ui_resume": self.resume
                })));
        }
        if !self.context.is_empty() {
            user_message
                .parts
                .push(ContentPart::Data(serde_json::json!({
                    "ag_ui_context": self.context
                })));
        }
        if !self.state.is_null() {
            user_message
                .parts
                .push(ContentPart::Data(serde_json::json!({
                    "ag_ui_state": self.state
                })));
        }
        let history = self
            .messages
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != prompt_index)
            .map(|(_, message)| run_agent_message_to_agent_message_with_files(message, false))
            .filter_map(message_without_file_parts)
            .collect();
        (user_message, history)
    }

    pub fn frontend_tool_registry(&self) -> Result<crate::agent::tools::ToolRegistry, String> {
        const MAX_TOOLS: usize = 32;
        const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;
        const MAX_SCHEMA_BYTES: usize = 64 * 1024;
        if self.tools.len() > MAX_TOOLS {
            return Err(format!("at most {MAX_TOOLS} frontend tools are allowed"));
        }
        let mut registry = crate::agent::tools::ToolRegistry::new();
        for tool in &self.tools {
            let name = tool.name.trim();
            if name.is_empty()
                || name.len() > 128
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
            {
                return Err(format!("invalid frontend tool name: {}", tool.name));
            }
            if tool
                .description
                .as_ref()
                .is_some_and(|description| description.len() > MAX_DESCRIPTION_BYTES)
            {
                return Err(format!("frontend tool '{name}' description is too large"));
            }
            if serde_json::to_vec(&tool.parameters)
                .map_err(|error| error.to_string())?
                .len()
                > MAX_SCHEMA_BYTES
            {
                return Err(format!("frontend tool '{name}' schema is too large"));
            }
            if tool.parameters.get("type").and_then(Value::as_str) != Some("object") {
                return Err(format!(
                    "frontend tool '{name}' parameters must be an object JSON Schema"
                ));
            }
            registry.try_register(crate::agent::tools::ToolSpec {
                name: name.to_string(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
                kind: crate::agent::tools::ToolKind::Frontend,
                strict: true,
                parallel_ok: false,
                executor: crate::agent::tools::ToolExecutor::Frontend,
            })?;
        }
        Ok(registry)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn message_without_file_parts(mut message: AgentMessage) -> Option<AgentMessage> {
    message
        .parts
        .retain(|part| !matches!(part, ContentPart::File { .. }));
    (!message.parts.is_empty()).then_some(message)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
async fn agui_run(
    State(state): State<Arc<AgUiState>>,
    headers: HeaderMap,
    Json(payload): Json<RunAgentInput>,
) -> Response {
    let run_id = payload.normalized_run_id();
    let thread_id = payload.normalized_thread_id();
    let message_id = format!("msg-{}", Uuid::new_v4());

    let (user_message, request_history) = payload.split_prompt_and_history();
    let history = (!request_history.is_empty()).then(|| ConversationContext::new(request_history));
    let frontend_tools = match payload.frontend_tool_registry() {
        Ok(tools) => tools,
        Err(message) => {
            return agui_validation_error_response(
                thread_id,
                run_id,
                "INVALID_FRONTEND_TOOLS",
                &message,
            )
            .into_response();
        }
    };
    let request_headers =
        Arc::new(protocol_transport_core::sanitize_header_map(&headers).into_map());
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let trace_stream = match state.agent.run_stream_with_additional_tools(
        user_message,
        history,
        Some(cancel_flag.clone()),
        Some(request_headers),
        frontend_tools,
    ) {
        Ok(stream) => stream,
        Err(err) => {
            return agent_io_sse_stream(futures::stream::iter(vec![
                AgentIoEvent::RunStarted { thread_id, run_id },
                AgentIoEvent::RunError {
                    message: err.to_string(),
                    code: Some("AGUI_RUNTIME".to_string()),
                },
            ]))
            .into_response();
        }
    };

    ag_ui_sse_response(
        AgUiDriverConfig {
            ctx: IoEventContext {
                thread_id,
                run_id,
                message_id,
            },
            cancel_flag: Some(cancel_flag),
            enrichers: vec![],
            initial_state: payload.state.clone(),
            initial_messages: payload
                .messages
                .iter()
                .filter_map(|message| serde_json::to_value(message).ok())
                .collect(),
        },
        trace_stream,
    )
    .into_response()
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn run_agent_message_to_agent_message(message: &RunAgentMessage) -> AgentMessage {
    run_agent_message_to_agent_message_with_files(message, true)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn run_agent_message_to_agent_message_with_files(
    message: &RunAgentMessage,
    include_files: bool,
) -> AgentMessage {
    let role = match message.role.as_str() {
        "assistant" => Role::Agent,
        "system" | "developer" => Role::System,
        "tool" => Role::Tool,
        _ => Role::User,
    };
    if role == Role::Tool {
        return AgentMessage::tool_result(
            message.tool_call_id.clone().unwrap_or_default(),
            message.name.clone(),
            message
                .content
                .as_ref()
                .map(|content| match content {
                    Value::String(content) => content.clone(),
                    value => value.to_string(),
                })
                .unwrap_or_default(),
            message.error.clone(),
        );
    }
    let mut parts = content_value_to_parts(message.content.as_ref(), include_files);
    if let Some(tool_calls) = &message.tool_calls {
        if let Some(tool_calls) = tool_calls.as_array() {
            for tool_call in tool_calls {
                let Some(id) = tool_call.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let function = tool_call.get("function").unwrap_or(tool_call);
                let Some(name) = function.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let arguments = function
                    .get("arguments")
                    .cloned()
                    .map(|arguments| match arguments {
                        Value::String(raw) => serde_json::from_str(&raw)
                            .unwrap_or_else(|_| serde_json::json!({ "_raw": raw })),
                        value => value,
                    })
                    .unwrap_or_else(|| serde_json::json!({}));
                parts.push(ContentPart::ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments,
                });
            }
        }
    }
    if parts.is_empty() && include_files {
        parts.push(ContentPart::Text(String::new()));
    }
    AgentMessage::new(role, parts)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn content_value_to_parts(content: Option<&Value>, include_files: bool) -> Vec<ContentPart> {
    match content {
        Some(Value::String(text)) => vec![ContentPart::Text(text.clone())],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| content_item_to_part(item, include_files))
            .collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(value) => content_item_to_part(value, include_files).into_iter().collect(),
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
const MAX_AGUI_INLINE_FILE_BYTES: usize = 25 * 1024 * 1024;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn content_item_to_part(item: &Value, include_files: bool) -> Option<ContentPart> {
    if item.get("type").and_then(Value::as_str) == Some("file") {
        return if include_files {
            inline_file_content_part(item)
        } else {
            None
        };
    }
    item.get("text")
        .and_then(Value::as_str)
        .map(|text| ContentPart::Text(text.to_string()))
        .or_else(|| Some(ContentPart::Data(item.clone())))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn inline_file_content_part(item: &Value) -> Option<ContentPart> {
    let filename = item.get("filename").and_then(Value::as_str)?.trim();
    let mime_type = item.get("mimeType").and_then(Value::as_str)?.trim();
    let encoded = item.get("data").and_then(Value::as_str)?;
    let max_encoded_len = ((MAX_AGUI_INLINE_FILE_BYTES + 2) / 3) * 4;
    if filename.is_empty()
        || filename.len() > 255
        || filename.contains(['/', '\\', '\0'])
        || mime_type.is_empty()
        || mime_type.len() > 255
        || !mime_type.contains('/')
        || mime_type.chars().any(char::is_whitespace)
        || encoded.is_empty()
        || encoded.len() > max_encoded_len
    {
        return None;
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    if data.is_empty() || data.len() > MAX_AGUI_INLINE_FILE_BYTES {
        return None;
    }
    Some(ContentPart::File {
        uri: filename.to_string(),
        mime: Some(mime_type.to_string()),
        data: Some(data),
    })
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn agui_validation_error_response(
    thread_id: String,
    run_id: String,
    code: &str,
    message: &str,
) -> Response {
    agent_io_sse_stream(futures::stream::iter(vec![
        AgentIoEvent::RunStarted { thread_id, run_id },
        AgentIoEvent::RunError {
            message: message.to_string(),
            code: Some(code.to_string()),
        },
    ]))
    .into_response()
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "event-stream"))]
mod file_input_tests {
    use std::collections::HashMap;

    use super::*;
    use crate::agent::MessageContext;
    use llm_client::{ChatContentPart, LlmRequest};

    #[test]
    fn test_agui_current_user_files_reach_llm_request_while_history_files_do_not() {
        let input: RunAgentInput = serde_json::from_value(serde_json::json!({
            "threadId": "thread-1",
            "runId": "run-1",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "previous response" },
                        {
                            "type": "file",
                            "filename": "old.pdf",
                            "mimeType": "application/pdf",
                            "data": "b2xk"
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": "Review these attachments" },
                        {
                            "type": "file",
                            "filename": "incident.pdf",
                            "mimeType": "application/pdf",
                            "data": "cGRm"
                        },
                        {
                            "type": "file",
                            "filename": "screenshot.png",
                            "mimeType": "image/png",
                            "data": "aW1n"
                        }
                    ]
                }
            ]
        }))
        .unwrap();

        let (current_user, history) = input.split_prompt_and_history();
        assert_eq!(
            history[0].parts,
            vec![ContentPart::Text("previous response".to_string())]
        );
        assert!(matches!(
            &current_user.parts[1],
            ContentPart::File {
                uri,
                mime: Some(mime),
                data: Some(data),
            } if uri == "incident.pdf" && mime == "application/pdf" && data == b"pdf"
        ));

        let message_context = MessageContext::from_runtime_message(
            current_user,
            HashMap::new(),
            HashMap::new(),
            false,
            None,
        );
        let request = LlmRequest {
            model: "gpt-5.4-mini".to_string(),
            messages: crate::agent::llm_orchestrator::build_runtime_messages_with_history(
                &message_context,
                &history,
                None,
            ),
            ..Default::default()
        };

        assert_eq!(
            request.messages[0].content.as_deref(),
            Some("previous response")
        );
        assert!(request.messages[0].content_parts.is_none());
        let current_parts = request.messages[1]
            .content_parts
            .as_ref()
            .expect("current user multimodal parts");
        assert!(matches!(
            &current_parts[0],
            ChatContentPart::Text { text } if text == "Review these attachments"
        ));
        assert!(matches!(
            &current_parts[1],
            ChatContentPart::FileBase64 {
                filename,
                media_type,
                data,
                detail: None,
            } if filename == "incident.pdf" && media_type == "application/pdf" && data == "cGRm"
        ));
        assert!(matches!(
            &current_parts[2],
            ChatContentPart::ImageBase64 {
                media_type,
                data,
                detail: None,
            } if media_type == "image/png" && data == "aW1n"
        ));
    }

    #[test]
    fn test_agui_malformed_file_part_is_dropped_without_exposing_encoded_data() {
        let message = RunAgentMessage {
            id: None,
            role: "user".to_string(),
            content: Some(serde_json::json!([
                { "type": "text", "text": "hello" },
                {
                    "type": "file",
                    "filename": "bad.pdf",
                    "mimeType": "application/pdf",
                    "data": "not base64"
                }
            ])),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            error: None,
        };

        let converted = run_agent_message_to_agent_message(&message);
        assert_eq!(
            converted.parts,
            vec![ContentPart::Text("hello".to_string())]
        );
    }

    #[test]
    fn test_agui_tool_messages_preserve_provider_correlation() {
        let input: RunAgentInput = serde_json::from_value(serde_json::json!({
            "threadId": "thread-tools",
            "runId": "run-tools",
            "messages": [
                { "id": "u1", "role": "user", "content": "Load the incident" },
                {
                    "id": "a1",
                    "role": "assistant",
                    "toolCalls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "load_incident",
                            "arguments": "{\"incidentId\":\"INC-1042\"}"
                        }
                    }]
                },
                {
                    "id": "tool-1",
                    "role": "tool",
                    "toolCallId": "call-1",
                    "name": "load_incident",
                    "content": "{\"status\":\"degraded\"}"
                }
            ]
        }))
        .unwrap();

        let (current, history) = input.split_prompt_and_history();
        let context = MessageContext::from_runtime_message(
            current,
            HashMap::new(),
            HashMap::new(),
            false,
            None,
        );
        let messages = crate::agent::llm_orchestrator::build_runtime_messages_with_history(
            &context,
            &history,
            None,
        );

        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].tool_calls.as_ref().unwrap()[0].id, "call-1");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(messages[2].name.as_deref(), Some("load_incident"));
    }

    #[test]
    fn test_agui_standard_state_context_and_resume_reach_runtime_data() {
        let input: RunAgentInput = serde_json::from_value(serde_json::json!({
            "threadId": "thread-resume",
            "runId": "run-resume",
            "state": { "incidentId": "INC-1042" },
            "context": [{ "description": "tenant locale", "value": "en-FI" }],
            "messages": [],
            "resume": [{
                "interruptId": "int-1",
                "status": "resolved",
                "payload": { "approved": true }
            }]
        }))
        .unwrap();

        let (current, history) = input.split_prompt_and_history();
        assert!(history.is_empty());
        assert!(current.parts.iter().any(|part| matches!(
            part,
            ContentPart::Data(value) if value.get("ag_ui_resume").is_some()
        )));
        assert!(current.parts.iter().any(|part| matches!(
            part,
            ContentPart::Data(value) if value.get("ag_ui_context").is_some()
        )));
        assert!(current.parts.iter().any(|part| matches!(
            part,
            ContentPart::Data(value) if value.get("ag_ui_state").is_some()
        )));
    }

    #[test]
    fn test_agui_frontend_tools_reject_name_collision() {
        let input: RunAgentInput = serde_json::from_value(serde_json::json!({
            "tools": [
                { "name": "load_incident", "parameters": { "type": "object" } },
                { "name": "load_incident", "parameters": { "type": "object" } }
            ]
        }))
        .unwrap();

        assert!(input.frontend_tool_registry().is_err());
    }
}
