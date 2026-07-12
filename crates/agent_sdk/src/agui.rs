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
use std::collections::HashMap;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::atomic::AtomicBool;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::{Arc, Mutex};

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
use futures_util::StreamExt;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use serde::{Deserialize, Serialize};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use serde_json::Value;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use uuid::Uuid;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use crate::agent::trace::AgentTraceEvent;
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingInteraction {
    interaction_id: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Default)]
struct ThreadConversationState {
    history: Vec<AgentMessage>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
struct AgUiState {
    agent: Arc<Agent>,
    pending_interactions: Mutex<HashMap<String, PendingInteraction>>,
    thread_conversations: Mutex<HashMap<String, ThreadConversationState>>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl AgUiState {
    fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            pending_interactions: Mutex::new(HashMap::new()),
            thread_conversations: Mutex::new(HashMap::new()),
        }
    }

    fn record_pending_interaction(&self, thread_id: String, interaction_id: String) {
        self.pending_interactions
            .lock()
            .expect("pending interaction lock poisoned")
            .insert(thread_id, PendingInteraction { interaction_id });
    }

    fn validate_and_consume_interaction_response(
        &self,
        thread_id: &str,
        interaction_id: Option<&str>,
    ) -> Result<(), ResumeValidationError> {
        let interaction_id = interaction_id
            .filter(|value| !value.trim().is_empty())
            .ok_or(ResumeValidationError::MissingInteractionId)?;
        let mut pending = self
            .pending_interactions
            .lock()
            .expect("pending interaction lock poisoned");
        let Some(expected) = pending.get(thread_id) else {
            return Err(ResumeValidationError::NoPendingInteraction);
        };
        if expected.interaction_id != interaction_id {
            return Err(ResumeValidationError::InteractionMismatch);
        }
        pending.remove(thread_id);
        Ok(())
    }

    fn history_for_thread(
        &self,
        thread_id: &str,
        request_history: Vec<AgentMessage>,
    ) -> Option<ConversationContext> {
        let mut conversations = self
            .thread_conversations
            .lock()
            .expect("thread conversation lock poisoned");

        if !request_history.is_empty() {
            conversations.insert(
                thread_id.to_string(),
                ThreadConversationState {
                    history: request_history.clone(),
                },
            );
            return Some(ConversationContext::new(request_history));
        }

        conversations.get(thread_id).cloned().and_then(|state| {
            (!state.history.is_empty()).then(|| ConversationContext::new(state.history))
        })
    }

    fn record_completed_turn(
        &self,
        thread_id: &str,
        user_message: AgentMessage,
        assistant_text: &str,
    ) {
        let mut conversations = self
            .thread_conversations
            .lock()
            .expect("thread conversation lock poisoned");
        let state = conversations.entry(thread_id.to_string()).or_default();
        if let Some(user_message) = message_without_file_parts(user_message) {
            state.history.push(user_message);
        }
        if !assistant_text.trim().is_empty() {
            state.history.push(AgentMessage::new(
                Role::Agent,
                vec![ContentPart::Text(assistant_text.to_string())],
            ));
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeValidationError {
    MissingInteractionId,
    NoPendingInteraction,
    InteractionMismatch,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl ResumeValidationError {
    fn as_code(self) -> &'static str {
        match self {
            Self::MissingInteractionId => "MISSING_INTERACTION_ID",
            Self::NoPendingInteraction => "NO_PENDING_INTERACTION",
            Self::InteractionMismatch => "INTERACTION_MISMATCH",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::MissingInteractionId => "interactionResponse must include interactionId",
            Self::NoPendingInteraction => {
                "interactionResponse does not match any pending interaction for this thread"
            }
            Self::InteractionMismatch => {
                "interactionResponse interactionId does not match the pending interaction for this thread"
            }
        }
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
    pub tools: Vec<Value>,
    #[serde(default)]
    pub context: Vec<Value>,
    #[serde(default)]
    pub forwarded_props: Value,
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

    pub fn interaction_response(&self) -> Option<Value> {
        self.forwarded_prop("interactionResponse", "interaction_response")
    }

    pub fn app_context(&self) -> Option<Value> {
        self.forwarded_prop("appContext", "app_context")
    }

    pub fn split_prompt_and_history(&self) -> (AgentMessage, Vec<AgentMessage>) {
        let interaction_response = self.interaction_response();
        let prompt_index = self
            .messages
            .iter()
            .rposition(|message| {
                !matches!(
                    message.role.as_str(),
                    "assistant" | "system" | "developer"
                )
            })
            .or_else(|| (!self.messages.is_empty()).then_some(self.messages.len() - 1));
        let mut user_message = prompt_index
            .map(|index| run_agent_message_to_agent_message(&self.messages[index]))
            .unwrap_or_else(|| AgentMessage::user_text(""));
        if let Some(response) = interaction_response {
            user_message
                .parts
                .push(ContentPart::Data(serde_json::json!({
                    "interaction_response": response
                })));
        }
        if let Some(context) = self.app_context() {
            user_message
                .parts
                .push(ContentPart::Data(serde_json::json!({
                    "app_context": context
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

    let interaction_response = payload.interaction_response();
    if let Some(response) = interaction_response.as_ref() {
        if let Err(err) = state
            .validate_and_consume_interaction_response(&thread_id, extract_interaction_id(response))
        {
            return agui_validation_error_response(thread_id, run_id, err).into_response();
        }
    }

    let (user_message, request_history) = payload.split_prompt_and_history();
    let history = state.history_for_thread(&thread_id, request_history);
    let request_headers =
        Arc::new(protocol_transport_core::sanitize_header_map(&headers).into_map());
    let assistant_text = Arc::new(Mutex::new(String::new()));
    let assistant_text_clone = Arc::clone(&assistant_text);
    let state_for_trace = Arc::clone(&state);
    let thread_id_for_trace = thread_id.clone();
    let user_message_for_trace = user_message.clone();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let trace_stream = match state.agent.run_stream(
        user_message,
        history,
        Some(cancel_flag.clone()),
        Some(request_headers),
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

    let trace_stream = trace_stream.inspect(move |event| match event {
        AgentTraceEvent::ContentDelta { delta } => {
            assistant_text_clone
                .lock()
                .expect("assistant text lock poisoned")
                .push_str(delta);
        }
        AgentTraceEvent::InteractionRequested { request } => {
            state_for_trace.record_pending_interaction(
                thread_id_for_trace.clone(),
                request.interaction_id.clone(),
            );
            let assistant_text = assistant_text_clone
                .lock()
                .expect("assistant text lock poisoned")
                .clone();
            state_for_trace.record_completed_turn(
                &thread_id_for_trace,
                user_message_for_trace.clone(),
                &assistant_text,
            );
        }
        AgentTraceEvent::Completed { text, .. } => {
            let assistant_text = text.clone().unwrap_or_else(|| {
                assistant_text_clone
                    .lock()
                    .expect("assistant text lock poisoned")
                    .clone()
            });
            state_for_trace.record_completed_turn(
                &thread_id_for_trace,
                user_message_for_trace.clone(),
                &assistant_text,
            );
        }
        _ => {}
    });

    ag_ui_sse_response(
        AgUiDriverConfig {
            ctx: IoEventContext {
                thread_id,
                run_id,
                message_id,
            },
            cancel_flag: Some(cancel_flag),
            enrichers: vec![],
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
        _ => Role::User,
    };
    let mut parts = content_value_to_parts(message.content.as_ref(), include_files);
    if let Some(tool_calls) = &message.tool_calls {
        parts.push(ContentPart::Data(
            serde_json::json!({ "toolCalls": tool_calls }),
        ));
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
fn extract_interaction_id(response: &serde_json::Value) -> Option<&str> {
    response
        .get("interactionId")
        .or_else(|| response.get("interaction_id"))
        .and_then(serde_json::Value::as_str)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn agui_validation_error_response(
    thread_id: String,
    run_id: String,
    err: ResumeValidationError,
) -> Response {
    agent_io_sse_stream(futures::stream::iter(vec![
        AgentIoEvent::RunStarted { thread_id, run_id },
        AgentIoEvent::RunError {
            message: err.message().to_string(),
            code: Some(err.as_code().to_string()),
        },
    ]))
    .into_response()
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "event-stream"))]
mod file_input_tests {
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
        };

        let converted = run_agent_message_to_agent_message(&message);
        assert_eq!(
            converted.parts,
            vec![ContentPart::Text("hello".to_string())]
        );
    }
}
